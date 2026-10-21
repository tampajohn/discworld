// SPDX-License-Identifier: GPL-3.0-or-later
//! English DOS V1 strings and bitmap font layout from strres.cpp/text.cpp.
use crate::{
    compositor::Surface,
    film::word,
    graphics::{Handle, Image, Resources, decode_tiles},
    iso::Error,
    resource::chunks,
};

pub struct Strings<'a>(Vec<&'a [u8]>);
impl<'a> Strings<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        let headers = chunks(bytes)?;
        let mut strings = Vec::new();
        let count = headers.len();
        for (ordinal, chunk) in headers.into_iter().enumerate() {
            if chunk.kind != 0x33340001 {
                return Err(Error("unsupported string chunk"));
            }
            let data = &bytes[chunk.payload];
            let mut at = 0;
            for _ in 0..64 {
                if ordinal + 1 == count && at == data.len() {
                    break;
                }
                let length = *data.get(at).ok_or(Error("truncated string length"))? as usize;
                at += 1;
                strings.push(
                    data.get(at..at + length)
                        .ok_or(Error("truncated string text"))?,
                );
                at += length;
            }
            if data[at..].len() > 3 || data[at..].iter().any(|b| *b != 0) {
                return Err(Error("unexpected string chunk tail"));
            }
        }
        Ok(Self(strings))
    }
    pub fn get(&self, id: u32) -> Result<&'a [u8], Error> {
        self.0
            .get(id as usize)
            .copied()
            .ok_or(Error("string ID outside archive"))
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Font {
    pub x_spacing: i32,
    pub y_spacing: i32,
    pub space: i32,
    pub flags: u32,
    #[serde(with = "crate::save_array")]
    pub glyphs: [Handle; 256],
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub image: Handle,
    pub x: i32,
    pub y: i32,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub glyphs: Vec<Glyph>,
}

impl Font {
    pub fn read(resources: &Resources<'_>, handle: Handle) -> Result<Self, Error> {
        let data = resources.resolve(handle, 44 + 256 * 4)?;
        let mut glyphs = [Handle(0); 256];
        for (i, glyph) in glyphs.iter_mut().enumerate() {
            *glyph = Handle(word(data, 44 + 4 * i)?);
        }
        Ok(Self {
            x_spacing: word(data, 0)? as i32,
            y_spacing: word(data, 4)? as i32,
            space: word(data, 16)? as i32,
            flags: word(data, 24)?,
            glyphs,
        })
    }
    pub fn centered(
        &self,
        resources: &Resources<'_>,
        bytes: &[u8],
        position: [i32; 2],
    ) -> Result<Layout, Error> {
        self.justified(resources, bytes, position, false)
    }
    /// V1 TXT_RIGHT subtracts the complete line width, including spaces but
    /// excluding the final inter-character spacing, separately for each line.
    pub fn right_aligned(
        &self,
        resources: &Resources<'_>,
        bytes: &[u8],
        position: [i32; 2],
    ) -> Result<Layout, Error> {
        self.justified(resources, bytes, position, true)
    }
    fn justified(
        &self,
        resources: &Resources<'_>,
        bytes: &[u8],
        position: [i32; 2],
        right: bool,
    ) -> Result<Layout, Error> {
        if bytes.len() > 4096 {
            return Err(Error("text layout length limit"));
        }
        let height = i32::from(Image::read(resources, self.glyphs[b'W' as usize])?.height);
        let mut result = Vec::new();
        let mut y = position[1];
        let text = &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())];
        for line in text.split(|b| *b == b'\n') {
            let mut width = 0i64;
            for c in line {
                let handle = self.glyphs[*c as usize];
                width += if handle.0 == 0 {
                    i64::from(self.space)
                } else {
                    i64::from(Image::read(resources, handle)?.width)
                } + i64::from(self.x_spacing);
            }
            width = (width - i64::from(self.x_spacing)).max(0);
            let mut x = i64::from(position[0]) - if right { width } else { width / 2 };
            for c in line {
                let handle = self.glyphs[*c as usize];
                if handle.0 != 0 {
                    let image = Image::read(resources, handle)?;
                    result.push(Glyph {
                        image: handle,
                        x: i32::try_from(x).map_err(|_| Error("text X overflow"))?,
                        y: y.checked_sub(i32::from(image.animation_y))
                            .ok_or(Error("text Y overflow"))?,
                    });
                    x += i64::from(image.width);
                } else {
                    x += i64::from(self.space);
                }
                x += i64::from(self.x_spacing);
            }
            y = y
                .checked_add(height)
                .and_then(|n| n.checked_add(self.y_spacing))
                .ok_or(Error("text line overflow"))?;
        }
        Ok(Layout { glyphs: result })
    }
}
impl Layout {
    pub fn shift(&mut self, delta: [i64; 2]) -> Result<(), Error> {
        for glyph in &mut self.glyphs {
            glyph.x = i32::try_from(i64::from(glyph.x) + delta[0])
                .map_err(|_| Error("text shift overflow"))?;
            glyph.y = i32::try_from(i64::from(glyph.y) + delta[1])
                .map_err(|_| Error("text shift overflow"))?;
        }
        Ok(())
    }
    /// V1 TALK anchors the bottom two pixels above the actor, with a four-pixel top margin.
    pub fn above_speaker(&mut self, resources: &Resources<'_>, head_y: i32) -> Result<(), Error> {
        if self.glyphs.is_empty() {
            return Ok(());
        }
        let mut bottom = i64::MIN;
        let mut top = i64::MAX;
        for glyph in &self.glyphs {
            let image = Image::read(resources, glyph.image)?;
            bottom = bottom.max(i64::from(glyph.y) + i64::from(image.height) - 1);
            top = top.min(i64::from(glyph.y));
        }
        let dy = (i64::from(head_y) - bottom - 2).max(4 - top);
        self.shift([0, dy])?;
        self.keep_visible(resources, 320, i32::MAX)
    }
    /// V1 PRINTOBJ clamps left first, then the inclusive right edge, with no
    /// vertical correction or TALK's margins.
    pub fn keep_inventory_horizontal(
        &mut self,
        resources: &Resources<'_>,
        width: i32,
    ) -> Result<(), Error> {
        let Some(left) = self.glyphs.iter().map(|g| g.x).min() else {
            return Ok(());
        };
        if left < 0 {
            self.shift([-i64::from(left), 0])?;
        }
        let mut right = i64::MIN;
        for glyph in &self.glyphs {
            let image = Image::read(resources, glyph.image)?;
            right = right.max(i64::from(glyph.x) + i64::from(image.width) - 1);
        }
        if right > i64::from(width) {
            self.shift([i64::from(width) - right, 0])?;
        }
        Ok(())
    }

    pub fn keep_visible(
        &mut self,
        resources: &Resources<'_>,
        width: i32,
        height: i32,
    ) -> Result<(), Error> {
        if self.glyphs.is_empty() {
            return Ok(());
        }
        let mut left = i64::MAX;
        let mut right = i64::MIN;
        let mut bottom = i64::MIN;
        for g in &self.glyphs {
            let image = Image::read(resources, g.image)?;
            left = left.min(i64::from(g.x));
            right = right.max(i64::from(g.x) + i64::from(image.width) - 1);
            bottom = bottom.max(i64::from(g.y) + i64::from(image.height) - 1);
        }
        let mut dx = if right + 2 >= i64::from(width) {
            i64::from(width) - right - 2
        } else {
            0
        };
        if left + dx - 1 <= 0 {
            dx += 1 - left - dx;
        }
        let dy = (i64::from(height) - bottom).min(0);
        for g in &mut self.glyphs {
            g.x = i32::try_from(i64::from(g.x) + dx).map_err(|_| Error("text clamp overflow"))?;
            g.y = i32::try_from(i64::from(g.y) + dy).map_err(|_| Error("text clamp overflow"))?;
        }
        Ok(())
    }
    pub fn draw(&self, resources: &Resources<'_>, surface: &mut Surface) -> Result<(), Error> {
        self.draw_colored(resources, surface, None)
    }
    pub fn draw_colored(
        &self,
        resources: &Resources<'_>,
        surface: &mut Surface,
        talk_color: Option<[u8; 3]>,
    ) -> Result<(), Error> {
        for glyph in &self.glyphs {
            let image = Image::read(resources, glyph.image)?;
            let mut palette = surface.font_palette(resources, image.palette)?;
            if let Some(color) = talk_color {
                *palette
                    .get_mut(232)
                    .ok_or(Error("missing V1 talk palette entry"))? = color;
            }
            let decoded = decode_tiles(resources, &image)?;
            surface.blit_indexed(
                &decoded,
                &palette,
                [i64::from(glyph.x), i64::from(glyph.y)],
                [false, false],
            )?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn centered_lines_use_space_width_and_vertical_animation_offset() {
        let mut data = vec![0; 1120];
        data[16..20].copy_from_slice(&2u32.to_le_bytes());
        let glyph = 44 + usize::from(b'W') * 4;
        data[glyph..glyph + 4].copy_from_slice(&1100u32.to_le_bytes());
        data[1100..1108].copy_from_slice(&[4, 0, 6, 0, 3, 0, 2, 0]);
        let resources = Resources(vec![&data]);
        let font = Font::read(&resources, Handle(0)).unwrap();
        let layout = font.centered(&resources, b"W W\nW", [160, 10]).unwrap();
        assert_eq!(
            layout.glyphs.iter().map(|g| (g.x, g.y)).collect::<Vec<_>>(),
            [(155, 8), (161, 8), (158, 14)]
        );
        let right = font
            .right_aligned(&resources, b"W W\nW", [160, 10])
            .unwrap();
        assert_eq!(
            right.glyphs.iter().map(|g| (g.x, g.y)).collect::<Vec<_>>(),
            [(150, 8), (156, 8), (156, 14)]
        );
        let mut speech = layout.clone();
        speech.above_speaker(&resources, 40).unwrap();
        assert_eq!(speech.glyphs.last().unwrap().y + 5, 38);
        speech.above_speaker(&resources, 0).unwrap();
        assert_eq!(speech.glyphs.iter().map(|g| g.y).min(), Some(4));
        let before = speech.glyphs[0].clone();
        speech.shift([-8, 3]).unwrap();
        assert_eq!(
            (speech.glyphs[0].x, speech.glyphs[0].y),
            (before.x - 8, before.y + 3)
        );
    }
    #[test]
    fn string_ids_include_empty_entries_and_high_length_bytes() {
        let mut data = vec![1, 0, 52, 51, 0, 0, 0, 0];
        data.push(0);
        data.push(128);
        data.extend([b'x'; 128]);
        data.extend([0; 62]);
        let strings = Strings::new(&data).unwrap();
        assert_eq!(strings.len(), 64);
        assert!(strings.get(0).unwrap().is_empty());
        assert_eq!(strings.get(1).unwrap().len(), 128);
        assert!(strings.get(64).is_err());
        assert!(Strings::new(&data[..100]).is_err());
    }
}
