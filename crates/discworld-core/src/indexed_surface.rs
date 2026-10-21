// SPDX-License-Identifier: GPL-3.0-or-later
//! Global palette-index composition for DOS V1 effects. Unlike RGBA previews,
//! destination indices survive drawing and can be remapped by WrtTrans.
use crate::{compositor::Surface, graphics::IndexedImage, iso::Error};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Translucency {
    #[serde(with = "crate::save_array")]
    pub lookup: [u8; 256],
}
impl Default for Translucency {
    fn default() -> Self {
        Self { lookup: [0; 256] }
    }
}
impl Translucency {
    /// CreateTranslucentPalette: source colors are the raw background palette,
    /// before screen dimming. Entry zero stays black. Unwritten entries retain
    /// their previous values until the palette subsystem itself is reset.
    pub fn update(&mut self, background: &[[u8; 3]]) -> Result<(), Error> {
        if background.is_empty() || background.len() > 255 {
            return Err(Error("invalid translucent background palette size"));
        }
        self.lookup[0] = 0;
        for (index, color) in background.iter().enumerate() {
            let value = *color.iter().max().unwrap() / 63;
            self.lookup[index + 1] = if value == 0 { 0 } else { 227 + value };
        }
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct IndexedSurface {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}
impl IndexedSurface {
    pub fn new(width: usize, height: usize) -> Result<Self, Error> {
        let len = width
            .checked_mul(height)
            .filter(|n| *n <= 16_777_216)
            .ok_or(Error("indexed surface size limit"))?;
        Ok(Self {
            width,
            height,
            pixels: vec![0; len],
        })
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The caller supplies the active local-to-global palette mapping. Coverage
    /// is independent of index zero: opaque black must overwrite its destination.
    pub fn blit(
        &mut self,
        image: &IndexedImage,
        mapping: &[u8; 256],
        position: [i64; 2],
        flip: [bool; 2],
    ) -> Result<(), Error> {
        if image.width.checked_mul(image.height) != Some(image.pixels.len())
            || image.coverage.len() != image.pixels.len()
        {
            return Err(Error("inconsistent indexed image buffers"));
        }
        // Clip in destination space before traversing source pixels. i128 keeps
        // extreme host coordinates from overflowing during bounds arithmetic.
        let bounds = self.clip(position, [image.width, image.height]);
        for y in bounds[1]..bounds[3] {
            for x in bounds[0]..bounds[2] {
                let mut sx = (x as i128 - i128::from(position[0])) as usize;
                let mut sy = (y as i128 - i128::from(position[1])) as usize;
                if flip[0] {
                    sx = image.width - 1 - sx;
                }
                if flip[1] {
                    sy = image.height - 1 - sy;
                }
                let source = sy * image.width + sx;
                if image.coverage[source] {
                    self.pixels[y * self.width + x] = mapping[image.pixels[source] as usize];
                }
            }
        }
        Ok(())
    }
    fn clip(&self, position: [i64; 2], size: [usize; 2]) -> [usize; 4] {
        let limits = [self.width, self.height];
        let mut bounds = [0; 4];
        for axis in 0..2 {
            let start = i128::from(position[axis]);
            let end = start + size[axis] as i128;
            bounds[axis] = start.clamp(0, limits[axis] as i128) as usize;
            bounds[axis + 2] = end.clamp(0, limits[axis] as i128) as usize;
        }
        bounds
    }
    /// WrtTrans maps the current destination once per covered pixel. Repeated
    /// overlays therefore remap the result of the previous overlay as well.
    pub fn translucent_rect(
        &mut self,
        table: &Translucency,
        bounds: [i32; 4],
    ) -> Result<(), Error> {
        let [x, y, width, height] = bounds;
        if width < 0 || height < 0 {
            return Err(Error("negative translucent rectangle size"));
        }
        let b = self.clip(
            [i64::from(x), i64::from(y)],
            [width as usize, height as usize],
        );
        for y in b[1]..b[3] {
            for x in b[0]..b[2] {
                let pixel = &mut self.pixels[y * self.width + x];
                *pixel = table.lookup[*pixel as usize];
            }
        }
        Ok(())
    }
    /// Resolve the current global palette at presentation time, allowing palette
    /// changes to recolor previously drawn indices without decoding assets again.
    pub fn present(&self, palette: &[[u8; 3]; 256]) -> Result<Surface, Error> {
        let mut surface = Surface::new(self.width, self.height)?;
        for (dst, &index) in surface.rgba.chunks_exact_mut(4).zip(&self.pixels) {
            dst[..3].copy_from_slice(&palette[index as usize]);
        }
        Ok(surface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translucent_thresholds_and_shorter_palette_preserve_reference_state() {
        let mut table = Translucency::default();
        let colors = [0, 62, 63, 125, 126, 188, 189, 251, 252, 255].map(|v| [0, v, 0]);
        table.update(&colors).unwrap();
        assert_eq!(
            &table.lookup[..11],
            &[0, 0, 0, 228, 228, 229, 229, 230, 230, 231, 231]
        );
        table.update(&[[255, 0, 0]]).unwrap();
        assert_eq!(table.lookup[1], 231);
        assert_eq!(table.lookup[10], 231);
        let before = table.clone();
        assert!(table.update(&[[0; 3]; 256]).is_err());
        assert_eq!(table, before);
    }
    #[test]
    fn coverage_mapping_clipping_flips_and_destination_remapping() {
        let image = IndexedImage {
            width: 3,
            height: 2,
            pixels: vec![1, 2, 0, 3, 4, 5],
            coverage: vec![true, false, true, true, true, true],
        };
        let mut mapping = std::array::from_fn(|i| i as u8);
        mapping[1] = 8;
        let mut surface = IndexedSurface::new(3, 2).unwrap();
        surface
            .blit(&image, &mapping, [0, 0], [false, false])
            .unwrap();
        assert_eq!(surface.pixels(), [8, 0, 0, 3, 4, 5]);
        surface
            .blit(&image, &mapping, [-1, 0], [true, true])
            .unwrap();
        assert_eq!(surface.pixels(), [4, 3, 0, 3, 8, 5]);
        let mut covered = surface.clone();
        let black = IndexedImage {
            width: 2,
            height: 1,
            pixels: vec![0, 0],
            coverage: vec![true, false],
        };
        covered
            .blit(&black, &mapping, [0, 0], [false, false])
            .unwrap();
        assert_eq!(&covered.pixels()[..2], &[0, 3]);
        let mut table = Translucency::default();
        table.update(&[[255, 0, 0]; 8]).unwrap();
        surface.translucent_rect(&table, [-1, 0, 3, 1]).unwrap();
        assert_eq!(surface.pixels(), [231, 231, 0, 3, 8, 5]);
        surface.translucent_rect(&table, [0, 0, 1, 1]).unwrap();
        assert_eq!(surface.pixels()[0], 0); // Unwritten reserved index maps to black.
        let before = surface.clone();
        surface
            .blit(&image, &mapping, [i64::MAX, i64::MIN], [true, false])
            .unwrap();
        assert_eq!(surface, before);
        assert!(surface.translucent_rect(&table, [0, 0, -1, 1]).is_err());
        assert_eq!(surface, before);
        let mut palette = [[0; 3]; 256];
        palette[231] = [12, 34, 56];
        assert_eq!(
            &surface.present(&palette).unwrap().rgba[4..8],
            &[12, 34, 56, 255]
        );
        palette[231] = [65, 43, 21];
        assert_eq!(
            &surface.present(&palette).unwrap().rgba[4..8],
            &[65, 43, 21, 255]
        );
    }
}
