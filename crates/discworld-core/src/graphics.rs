// SPDX-License-Identifier: GPL-3.0-or-later
// DOS tile semantics informed by ScummVM Tinsel graphics.cpp, WrtNonZero.
// See NOTICE for attribution and the pinned reference.
use crate::iso::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle(pub u32);
impl Handle {
    pub fn resource(self) -> usize {
        (self.0 >> 23) as usize
    }
    pub fn offset(self) -> usize {
        (self.0 & 0x7f_ffff) as usize
    }
}

pub struct Resources<'a>(pub Vec<&'a [u8]>);
impl<'a> Resources<'a> {
    pub fn resolve(&self, h: Handle, len: usize) -> Result<&'a [u8], Error> {
        let data = self
            .0
            .get(h.resource())
            .ok_or(Error("invalid resource handle"))?;
        span(data, h.offset(), len)
    }
}
fn span(data: &[u8], offset: usize, len: usize) -> Result<&[u8], Error> {
    data.get(offset..offset.checked_add(len).ok_or(Error("offset overflow"))?)
        .ok_or(Error("resource range out of bounds"))
}
fn word(data: &[u8], offset: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(
        span(data, offset, 4)?.try_into().unwrap(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u16,
    pub height: u16,
    pub animation_x: i16,
    pub animation_y: i16,
    pub bits: Handle,
    pub palette: Handle,
}
impl Image {
    pub fn read(resources: &Resources<'_>, handle: Handle) -> Result<Self, Error> {
        let b = resources.resolve(handle, 16)?;
        let width = i16::from_le_bytes(b[..2].try_into().unwrap());
        let height = u16::from_le_bytes(b[2..4].try_into().unwrap());
        if width <= 0 || height == 0 {
            return Err(Error("empty or negative image dimensions"));
        }
        Ok(Self {
            width: width as u16,
            height,
            animation_x: i16::from_le_bytes(b[4..6].try_into().unwrap()),
            animation_y: i16::from_le_bytes(b[6..8].try_into().unwrap()),
            bits: Handle(word(b, 8)?),
            palette: Handle(word(b, 12)?),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct IndexedImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    /// Separate coverage preserves opaque color zero versus transparent zero.
    pub coverage: Vec<bool>,
}

pub fn decode_tiles(resources: &Resources<'_>, image: &Image) -> Result<IndexedImage, Error> {
    let width = usize::from(image.width);
    let height = usize::from(image.height);
    let count = width
        .checked_mul(height)
        .ok_or(Error("image size overflow"))?;
    if count == 0 || count > 16_777_216 {
        return Err(Error("image allocation limit"));
    }
    let columns = width.div_ceil(4);
    let rows = height.div_ceil(4);
    let codes = resources.resolve(image.bits, columns * rows * 2)?;
    let file = resources
        .0
        .get(image.bits.resource())
        .ok_or(Error("invalid bitmap resource"))?;
    let base = word(file, 16)? as usize;
    let transparent_base = word(file, 20)? as usize;
    let mut output = IndexedImage {
        width,
        height,
        pixels: vec![0; count],
        coverage: vec![false; count],
    };
    for (i, code) in codes.chunks_exact(2).enumerate() {
        let value = u16::from_le_bytes(code.try_into().unwrap());
        let transparent = value & 0x8000 != 0;
        let tile = usize::from(value & 0x7fff);
        if transparent && tile == 0 {
            continue;
        }
        let tile = if transparent {
            transparent_base
                .checked_add(tile)
                .ok_or(Error("tile overflow"))?
        } else {
            tile
        };
        let offset = tile
            .checked_mul(16)
            .and_then(|n| base.checked_add(n))
            .ok_or(Error("tile overflow"))?;
        let pixels = span(file, offset, 16)?;
        for y in 0..4 {
            for x in 0..4 {
                let dx = i % columns * 4 + x;
                let dy = i / columns * 4 + y;
                if dx >= width || dy >= height {
                    continue;
                }
                let color = pixels[y * 4 + x];
                let at = dy * width + dx;
                output.pixels[at] = color;
                output.coverage[at] = !transparent || color != 0;
            }
        }
    }
    Ok(output)
}

pub fn read_palette(resources: &Resources<'_>, handle: Handle) -> Result<Vec<[u8; 3]>, Error> {
    let count = word(resources.resolve(handle, 4)?, 0)? as usize;
    if count == 0 || count > 256 {
        return Err(Error("invalid palette size"));
    }
    let data = resources.resolve(handle, 4 + count * 4)?;
    Ok(data[4..]
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2]])
        .collect())
}

impl IndexedImage {
    /// Converts local indices: zero is black, colors start at index one.
    /// Does not perform runtime palette allocation or scene palette changes.
    pub fn rgba(&self, palette: &[[u8; 3]]) -> Result<Vec<u8>, Error> {
        if self.width.checked_mul(self.height) != Some(self.pixels.len())
            || self.coverage.len() != self.pixels.len()
        {
            return Err(Error("inconsistent image buffers"));
        }
        let mut result = Vec::with_capacity(self.pixels.len() * 4);
        for (&pixel, &covered) in self.pixels.iter().zip(&self.coverage) {
            if !covered {
                result.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let rgb = if pixel == 0 {
                &[0, 0, 0]
            } else {
                palette
                    .get(usize::from(pixel) - 1)
                    .ok_or(Error("pixel outside palette"))?
            };
            result.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_indices_start_at_one_and_coverage_controls_alpha() {
        let image = IndexedImage {
            width: 4,
            height: 1,
            pixels: vec![0, 1, 2, 255],
            coverage: vec![true, true, true, false],
        };
        assert_eq!(
            image.rgba(&[[10, 20, 30], [40, 50, 60]]).unwrap(),
            vec![0, 0, 0, 255, 10, 20, 30, 255, 40, 50, 60, 255, 0, 0, 0, 0]
        );
        assert!(image.rgba(&[[10, 20, 30]]).is_err());
    }
    #[test]
    fn rejects_palette_and_image_allocation_abuse() {
        let data = 257u32.to_le_bytes();
        assert!(read_palette(&Resources(vec![&data]), Handle(0)).is_err());
        let image = Image {
            width: u16::MAX,
            height: u16::MAX,
            animation_x: 0,
            animation_y: 0,
            bits: Handle(0),
            palette: Handle(0),
        };
        assert!(decode_tiles(&Resources(vec![&data]), &image).is_err());
    }
    #[test]
    fn tiles_preserve_opaque_zero_and_transparency() {
        let mut file = vec![0; 100];
        file[16..20].copy_from_slice(&32u32.to_le_bytes());
        file[20..24].copy_from_slice(&1u32.to_le_bytes());
        file[24..30].copy_from_slice(&[0, 0, 1, 128, 0, 128]);
        file[64..80].fill(7);
        file[64] = 0;
        let resources = Resources(vec![&file]);
        let image = Image {
            width: 9,
            height: 3,
            animation_x: 0,
            animation_y: 0,
            bits: Handle(24),
            palette: Handle(0),
        };
        let decoded = decode_tiles(&resources, &image).unwrap();
        assert_eq!(decoded.pixels.len(), 27);
        assert!(decoded.coverage[0]);
        assert!(!decoded.coverage[4]);
        assert!(decoded.coverage[5]);
        assert!(!decoded.coverage[8]);
        assert_eq!(decoded.pixels[5], 7);
    }
    #[test]
    fn rejects_invalid_handle_and_tiles() {
        let data = vec![0; 32];
        let resources = Resources(vec![&data]);
        assert!(resources.resolve(Handle(1 << 23), 1).is_err());
        assert!(resources.resolve(Handle(31), 2).is_err());
        let image = Image {
            width: 20,
            height: 20,
            animation_x: 0,
            animation_y: 0,
            bits: Handle(24),
            palette: Handle(0),
        };
        assert!(decode_tiles(&resources, &image).is_err());
    }
}
