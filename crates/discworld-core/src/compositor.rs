// SPDX-License-Identifier: GPL-3.0-or-later
//! CPU preview composition. Runtime palette allocation and special effects remain separate.
use crate::{
    animation::Animation,
    graphics::{Image, IndexedImage, Resources, decode_tiles, read_palette},
    indexed_surface::{IndexedSurface, Translucency},
    iso::Error,
};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    backdrop: Option<Backdrop>,
    scene_palette: Option<crate::palette::BackgroundPalette>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct Backdrop {
    indices: IndexedSurface,
    table: Translucency,
    #[serde(with = "crate::save_array")]
    palette: [[u8; 3]; 256],
}
impl Surface {
    pub fn new(width: usize, height: usize) -> Result<Self, Error> {
        let pixels = width
            .checked_mul(height)
            .filter(|n| *n <= 16_777_216)
            .ok_or(Error("surface size limit"))?;
        let mut rgba = vec![0; pixels * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Ok(Self {
            width,
            height,
            rgba,
            backdrop: None,
            scene_palette: None,
        })
    }
    /// DOS V1 WrtNonZero copies raw tile indices. Keep those indices alongside
    /// the preview colors so WrtTrans never has to infer an index from RGB.
    /// Call before drawing this frame. Palette lifecycle outside the current
    /// background (including retained entries of shorter palettes) is separate.
    pub fn enable_backdrop(&mut self, colors: &[[u8; 3]]) -> Result<(), Error> {
        let mut table = Translucency::default();
        table.update(colors)?;
        let mut palette = [[0; 3]; 256];
        palette[1..=colors.len()].copy_from_slice(colors);
        self.backdrop = Some(Backdrop {
            indices: IndexedSurface::new(self.width, self.height)?,
            table,
            palette,
        });
        Ok(())
    }
    pub fn set_scene_palette(
        &mut self,
        palette: &crate::palette::BackgroundPalette,
    ) -> Result<(), Error> {
        self.enable_backdrop(&palette.colors)?;
        self.scene_palette = Some(palette.clone());
        Ok(())
    }
    pub fn image_palette(
        &self,
        resources: &Resources<'_>,
        handle: crate::graphics::Handle,
    ) -> Result<Vec<[u8; 3]>, Error> {
        if let Some(palette) = &self.scene_palette {
            if palette.handles.contains(&handle) {
                return Ok(palette.colors.clone());
            }
        }
        read_palette(resources, handle)
    }
    /// V1 FettleFontPal points both game fonts at the background palette.
    pub fn font_palette(
        &self,
        resources: &Resources<'_>,
        handle: crate::graphics::Handle,
    ) -> Result<Vec<[u8; 3]>, Error> {
        if let Some(palette) = &self.scene_palette {
            return Ok(palette.colors.clone());
        }
        read_palette(resources, handle)
    }
    pub fn blit_indexed(
        &mut self,
        image: &IndexedImage,
        palette: &[[u8; 3]],
        position: [i64; 2],
        flip: [bool; 2],
    ) -> Result<(), Error> {
        let rgba = image.rgba(palette)?;
        self.blit_rgba(image.width, image.height, &rgba, position, flip)?;
        if let Some(backdrop) = &mut self.backdrop {
            backdrop
                .indices
                .blit(image, &std::array::from_fn(|i| i as u8), position, flip)?;
        }
        Ok(())
    }
    /// V1 RectangleObject using the current background palette. Preserve the
    /// destination indices for any translucent objects drawn above the box.
    pub fn draw_palette_rect(&mut self, bounds: [i32; 4], color: u8) -> Result<(), Error> {
        let backdrop = self
            .backdrop
            .as_mut()
            .ok_or(Error("rectangle requires background palette"))?;
        backdrop.indices.solid_rect(bounds, color)?;
        let rgb = backdrop.palette[usize::from(color)];
        let [x, y, width, height] = bounds.map(i64::from);
        for yy in y.max(0)..(y + height).min(self.height as i64) {
            for xx in x.max(0)..(x + width).min(self.width as i64) {
                let at = (yy as usize * self.width + xx as usize) * 4;
                self.rgba[at..at + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        Ok(())
    }
    pub fn draw_backdrop(&mut self, bounds: [i32; 4]) -> Result<(), Error> {
        let Some(backdrop) = &mut self.backdrop else {
            return Ok(());
        };
        backdrop.indices.translucent_rect(&backdrop.table, bounds)?;
        let mapped = backdrop.indices.present(&backdrop.palette)?;
        let [x, y, width, height] = bounds.map(i64::from);
        for yy in y.max(0)..(y + height).min(self.height as i64) {
            for xx in x.max(0)..(x + width).min(self.width as i64) {
                let offset = (yy as usize * self.width + xx as usize) * 4;
                self.rgba[offset..offset + 4].copy_from_slice(&mapped.rgba[offset..offset + 4]);
            }
        }
        Ok(())
    }
    pub fn blit(
        &mut self,
        width: usize,
        height: usize,
        rgba: &[u8],
        position: [i64; 2],
        flip: [bool; 2],
    ) -> Result<(), Error> {
        if self.backdrop.is_some() {
            return Err(Error("RGBA blit would discard backdrop indices"));
        }
        self.blit_rgba(width, height, rgba, position, flip)
    }
    fn blit_rgba(
        &mut self,
        width: usize,
        height: usize,
        rgba: &[u8],
        position: [i64; 2],
        flip: [bool; 2],
    ) -> Result<(), Error> {
        let [x, y] = position;
        let [flip_x, flip_y] = flip;
        if width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(rgba.len())
            || self
                .width
                .checked_mul(self.height)
                .and_then(|n| n.checked_mul(4))
                != Some(self.rgba.len())
        {
            return Err(Error("invalid pixel buffer"));
        }
        for dy in 0..height {
            let Some(ty) = y.checked_add(dy as i64) else {
                continue;
            };
            if ty < 0 || ty >= self.height as i64 {
                continue;
            }
            for dx in 0..width {
                let Some(tx) = x.checked_add(dx as i64) else {
                    continue;
                };
                if tx < 0 || tx >= self.width as i64 {
                    continue;
                }
                let sx = if flip_x { width - 1 - dx } else { dx };
                let sy = if flip_y { height - 1 - dy } else { dy };
                let src = (sy * width + sx) * 4;
                if rgba[src + 3] == 0 {
                    continue;
                }
                let dst = (ty as usize * self.width + tx as usize) * 4;
                self.rgba[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
            }
        }
        Ok(())
    }
    pub fn draw_animation(
        &mut self,
        resources: &Resources<'_>,
        animation: &Animation,
    ) -> Result<(), Error> {
        self.draw_animation_offset(resources, animation, [0, 0])
    }
    pub fn draw_animation_offset(
        &mut self,
        resources: &Resources<'_>,
        animation: &Animation,
        origin: [i32; 2],
    ) -> Result<(), Error> {
        self.draw_animations_ordered(resources, std::iter::once(animation), origin)
    }
    /// V1 sorts individual display objects by Z then rendered top Y. Multipart
    /// frames must be allowed to interleave with other actors/foreground pieces.
    /// Equal-key order retains the supplied order; persistent insertion history
    /// is not yet represented by the caller.
    pub fn draw_animations_ordered<'a>(
        &mut self,
        resources: &Resources<'_>,
        animations: impl IntoIterator<Item = &'a Animation>,
        origin: [i32; 2],
    ) -> Result<(), Error> {
        let mut parts = Vec::new();
        for animation in animations {
            let handles = animation.images(resources)?;
            let Some(first) = handles.first() else {
                continue;
            };
            let palette = Image::read(resources, *first)?.palette;
            for handle in handles {
                let image = Image::read(resources, handle)?;
                let ax = if animation.flip_x {
                    -i64::from(image.animation_x) + i64::from(image.width) - 1
                } else {
                    i64::from(image.animation_x)
                };
                let ay = if animation.flip_y {
                    -i64::from(image.animation_y) + i64::from(image.height) - 1
                } else {
                    i64::from(image.animation_y)
                };
                let position = [
                    i64::from(animation.object.x) - ax - i64::from(origin[0]),
                    i64::from(animation.object.y) - ay - i64::from(origin[1]),
                ];
                parts.push((
                    animation.object.z,
                    position,
                    image,
                    palette,
                    [animation.flip_x, animation.flip_y],
                ));
            }
        }
        parts.sort_by_key(|part| (part.0, part.1[1]));
        for (_, position, image, palette, flip) in parts {
            let decoded = decode_tiles(resources, &image)?;
            self.blit_indexed(
                &decoded,
                &self.image_palette(resources, palette)?,
                position,
                flip,
            )?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_menu_rectangles_preserve_indices_for_later_translucency() {
        let mut surface = Surface::new(3, 1).unwrap();
        let mut colors = vec![[0, 0, 0]; 255];
        colors[227] = [0, 0, 126]; // index228 maps to229 on the next overlay
        colors[228] = [0, 0, 189];
        surface.enable_backdrop(&colors).unwrap();
        surface.draw_palette_rect([-1, 0, 3, 1], 228).unwrap();
        assert_eq!(
            &surface.rgba[..12],
            &[0, 0, 126, 255, 0, 0, 126, 255, 0, 0, 0, 255]
        );
        surface.draw_backdrop([0, 0, 1, 1]).unwrap();
        assert_eq!(&surface.rgba[..8], &[0, 0, 189, 255, 0, 0, 126, 255]);
        let before = surface.clone();
        assert!(surface.draw_palette_rect([0, 0, -1, 1], 0).is_err());
        assert_eq!(surface, before);
        surface
            .draw_palette_rect([i32::MAX, i32::MAX, i32::MAX, i32::MAX], 0)
            .unwrap();
        assert_eq!(surface, before);
        surface.draw_palette_rect([1, 0, 1, 1], 0).unwrap();
        assert_eq!(&surface.rgba[4..8], &[0, 0, 0, 255]);
    }
    #[test]
    fn backdrop_uses_raw_destination_indices_and_preserves_uncovered_pixels() {
        let mut background = vec![[0; 3]; 255];
        background[0] = [63, 0, 0];
        background[1] = [255, 0, 0];
        background[227] = [10, 20, 30];
        background[230] = [40, 50, 60];
        let mut surface = Surface::new(4, 1).unwrap();
        surface.enable_backdrop(&background).unwrap();
        // Identical displayed RGB must still remap differently: indices, not
        // brightness of the preview's per-object palette, drive WrtTrans.
        let image = IndexedImage {
            width: 4,
            height: 1,
            pixels: vec![1, 2, 0, 2],
            coverage: vec![true, true, true, true],
        };
        surface
            .blit_indexed(&image, &[[100; 3], [100; 3]], [0, 0], [false; 2])
            .unwrap();
        surface.draw_backdrop([-1, 0, 4, 1]).unwrap();
        assert_eq!(
            surface.rgba,
            [
                10, 20, 30, 255, 40, 50, 60, 255, 0, 0, 0, 255, 100, 100, 100, 255
            ]
        );
        // A second overlay reads the first overlay's resulting index.
        surface.draw_backdrop([0, 0, 1, 1]).unwrap();
        assert_eq!(&surface.rgba[..4], &[0, 0, 0, 255]);
        assert!(
            surface
                .blit(1, 1, &[1, 2, 3, 255], [0, 0], [false; 2])
                .is_err()
        );
    }
    #[test]
    fn swapped_scene_palette_rebuilds_translucent_overlay_mapping() {
        use crate::{graphics::Handle, palette::BackgroundPalette};
        let mut data = vec![0; 2048];
        for offset in [0, 1024] {
            data[offset..offset + 4].copy_from_slice(&255u32.to_le_bytes());
        }
        data[4..7].copy_from_slice(&[63, 0, 0]);
        data[4 + 227 * 4..7 + 227 * 4].copy_from_slice(&[10, 20, 30]);
        data[1028..1031].copy_from_slice(&[255, 0, 0]);
        data[1028 + 230 * 4..1031 + 230 * 4].copy_from_slice(&[40, 50, 60]);
        let resources = Resources(vec![&data]);
        let mut palette = BackgroundPalette::new(&resources, Handle(0)).unwrap();
        let image = IndexedImage {
            width: 1,
            height: 1,
            pixels: vec![1],
            coverage: vec![true],
        };
        let mut before = Surface::new(1, 1).unwrap();
        before.set_scene_palette(&palette).unwrap();
        before
            .blit_indexed(&image, &palette.colors, [0, 0], [false; 2])
            .unwrap();
        before.draw_backdrop([0, 0, 1, 1]).unwrap();
        assert_eq!(before.rgba, [10, 20, 30, 255]);
        palette.replace(&resources, Handle(1024)).unwrap();
        let mut after = Surface::new(1, 1).unwrap();
        after.set_scene_palette(&palette).unwrap();
        after
            .blit_indexed(&image, &palette.colors, [0, 0], [false; 2])
            .unwrap();
        after.draw_backdrop([0, 0, 1, 1]).unwrap();
        assert_eq!(after.rgba, [40, 50, 60, 255]);
    }

    #[test]
    fn multipart_pieces_interleave_using_top_y_and_z_takes_priority() {
        use crate::{film::Reel, graphics::Handle};
        let mut data = vec![0; 416];
        for (at, value) in [
            (16, 400u32),
            (100, 1),
            (108, 1),
            (200, 128),
            (204, 144),
            (220, 160),
            (240, 200),
            (264, 220),
            (280, 2),
        ] {
            data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        data[104..107].copy_from_slice(&[255, 0, 0]);
        data[112..115].copy_from_slice(&[0, 0, 255]);
        data[400..].fill(1);
        for (at, offset_y, palette) in [(128, 0i16, 100u32), (144, -4, 100), (160, 0, 108)] {
            data[at..at + 4].copy_from_slice(&[4, 0, 4, 0]);
            data[at + 6..at + 8].copy_from_slice(&offset_y.to_le_bytes());
            data[at + 8..at + 12].copy_from_slice(&96u32.to_le_bytes());
            data[at + 12..at + 16].copy_from_slice(&palette.to_le_bytes());
        }
        let r = Resources(vec![&data]);
        let a = Animation::new(
            &r,
            &Reel {
                object: Handle(240),
                script: Handle(300),
            },
            1,
        )
        .unwrap();
        let mut b = Animation::new(
            &r,
            &Reel {
                object: Handle(264),
                script: Handle(300),
            },
            1,
        )
        .unwrap();
        let mut surface = Surface::new(4, 8).unwrap();
        surface
            .draw_animations_ordered(&r, [&a, &b], [0, 0])
            .unwrap();
        let pixel = |s: &Surface, y: usize| s.rgba[y * 16..y * 16 + 4].to_vec();
        assert_eq!(pixel(&surface, 2), [0, 0, 255, 255]);
        assert_eq!(pixel(&surface, 4), [255, 0, 0, 255]);
        b.object.z = 1;
        surface
            .draw_animations_ordered(&r, [&a, &b], [0, 0])
            .unwrap();
        assert_eq!(pixel(&surface, 4), [0, 0, 255, 255]);
    }
    #[test]
    fn world_origin_moves_pixels_without_mutating_animation() {
        use crate::{animation::Animation, film::Reel, graphics::Handle};
        let mut data = vec![0; 128];
        for (at, value) in [
            (16, 112u32),
            (32, 64),
            (44, 640),
            (64, 80),
            (80, 0x00040004),
            (88, 96),
            (92, 100),
            (100, 1),
        ] {
            data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        data[104..107].copy_from_slice(&[90, 80, 70]);
        data[112..128].fill(1);
        let resources = Resources(vec![&data]);
        let animation = Animation::new(
            &resources,
            &Reel {
                object: Handle(32),
                script: Handle(108),
            },
            1,
        )
        .unwrap();
        let before = animation.clone();
        assert_eq!(
            animation.bounds(&resources).unwrap(),
            Some([640, 0, 643, 3])
        );
        let mut flipped = animation.clone();
        flipped.flip_x = true;
        flipped.flip_y = true;
        assert_eq!(flipped.bounds(&resources).unwrap(), Some([637, -3, 640, 0]));
        flipped.frame = Handle(0);
        assert_eq!(flipped.bounds(&resources).unwrap(), None);
        let mut world = Surface::new(4, 4).unwrap();
        world
            .draw_animation_offset(&resources, &animation, [640, 0])
            .unwrap();
        assert!(world.rgba.chunks_exact(4).all(|p| p == [90, 80, 70, 255]));
        let mut screen = Surface::new(4, 4).unwrap();
        screen.draw_animation(&resources, &animation).unwrap();
        assert!(screen.rgba.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        let mut extreme = Surface::new(4, 4).unwrap();
        extreme
            .draw_animation_offset(&resources, &animation, [i32::MIN, i32::MAX])
            .unwrap();
        assert_eq!(extreme, screen);
        assert_eq!(animation, before);
    }
    #[test]
    fn clipping_and_transparency() {
        let mut s = Surface::new(2, 1).unwrap();
        s.blit(
            3,
            1,
            &[1, 2, 3, 255, 4, 5, 6, 0, 7, 8, 9, 255],
            [-1, 0],
            [false, false],
        )
        .unwrap();
        assert_eq!(s.rgba, [0, 0, 0, 255, 7, 8, 9, 255]);
    }
    #[test]
    fn horizontal_flip() {
        let mut s = Surface::new(2, 1).unwrap();
        s.blit(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255], [0, 0], [true, false])
            .unwrap();
        assert_eq!(s.rgba, [4, 5, 6, 255, 1, 2, 3, 255]);
    }
}
