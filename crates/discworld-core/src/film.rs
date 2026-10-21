// SPDX-License-Identifier: GPL-3.0-or-later
//! Tinsel V1 film, reel, and multipart object formats; see pinned film.h/multiobj.h.
use crate::{
    graphics::{Handle, Resources},
    iso::Error,
};
pub fn word(data: &[u8], offset: usize) -> Result<u32, Error> {
    let end = offset.checked_add(4).ok_or(Error("word offset overflow"))?;
    Ok(u32::from_le_bytes(
        data.get(offset..end)
            .ok_or(Error("truncated word"))?
            .try_into()
            .unwrap(),
    ))
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Reel {
    pub object: Handle,
    pub script: Handle,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Film {
    pub rate: i32,
    pub reels: Vec<Reel>,
}
impl Film {
    pub fn read(resources: &Resources<'_>, handle: Handle) -> Result<Self, Error> {
        let header = resources.resolve(handle, 8)?;
        let rate = word(header, 0)? as i32;
        let count = word(header, 4)? as usize;
        if rate <= 0 || count == 0 || count > 4096 {
            return Err(Error("invalid film header"));
        }
        let data = resources.resolve(handle, 8 + count * 8)?;
        let reels = data[8..]
            .chunks_exact(8)
            .map(|b| {
                Ok(Reel {
                    object: Handle(word(b, 0)?),
                    script: Handle(word(b, 4)?),
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(Self { rate, reels })
    }
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub frame: Handle,
    pub flags: u32,
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
impl Object {
    pub fn read(resources: &Resources<'_>, handle: Handle) -> Result<Self, Error> {
        let b = resources.resolve(handle, 24)?;
        Ok(Self {
            frame: Handle(word(b, 0)?),
            flags: word(b, 4)?,
            id: word(b, 8)? as i32,
            x: word(b, 12)? as i32,
            y: word(b, 16)? as i32,
            z: word(b, 20)? as i32,
        })
    }
}
pub fn frame(resources: &Resources<'_>, handle: Handle) -> Result<Vec<Handle>, Error> {
    let mut images = Vec::new();
    for i in 0..4096 {
        let data = resources.resolve(handle, (i + 1) * 4)?;
        let image = word(data, i * 4)?;
        if image == 0 {
            return Ok(images);
        }
        images.push(Handle(image));
    }
    Err(Error("multipart frame limit exceeded"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_film_and_frame_bounds() {
        let b = [12u32, 1, 24, 48]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let r = Resources(vec![&b]);
        assert_eq!(
            Film::read(&r, Handle(0)).unwrap().reels[0].script,
            Handle(48)
        );
        assert!(Film::read(&Resources(vec![&b[..15]]), Handle(0)).is_err());
        assert!(frame(&r, Handle(0)).is_err());
    }
}
