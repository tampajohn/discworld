// SPDX-License-Identifier: GPL-3.0-or-later
//! DOS V1 scene metadata, based on ScummVM scene.cpp parseSceneV1.
use crate::{
    graphics::{Handle, Resources},
    iso::Error,
    resource::chunks,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    pub script: Handle,
    pub default_refer: i32,
    pub polygon_count: u32,
    pub polygons: Handle,
    pub actors: Vec<Actor>,
    pub entrances: Vec<(u32, Handle)>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub masking: i32,
    pub id: u32,
    pub script: Handle,
}
fn actors(resources: &Resources<'_>, handle: Handle, count: u32) -> Result<Vec<Actor>, Error> {
    if count > 65536 {
        return Err(Error("scene actor limit exceeded"));
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let data = resources.resolve(handle, count as usize * 12)?;
    data.chunks_exact(12)
        .map(|r| {
            Ok(Actor {
                masking: word(r, 0)? as i32,
                id: word(r, 4)?,
                script: Handle(word(r, 8)?),
            })
        })
        .collect()
}
fn word(data: &[u8], offset: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(
        data.get(offset..offset + 4)
            .ok_or(Error("truncated scene metadata"))?
            .try_into()
            .unwrap(),
    ))
}
fn table(
    resources: &Resources<'_>,
    handle: Handle,
    count: u32,
) -> Result<Vec<(u32, Handle)>, Error> {
    if count > 65536 {
        return Err(Error("scene table limit exceeded"));
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let data = resources.resolve(handle, count as usize * 8)?;
    data.chunks_exact(8)
        .map(|r| Ok((word(r, 0)?, Handle(word(r, 4)?))))
        .collect()
}
pub fn read(resources: &Resources<'_>, resource: usize) -> Result<Option<Scene>, Error> {
    let file = resources
        .0
        .get(resource)
        .ok_or(Error("invalid scene resource"))?;
    let headers = chunks(file)?;
    let Some(chunk) = headers.iter().find(|c| c.kind == 0x3334000e) else {
        return Ok(None);
    };
    let data = &file[chunk.payload.clone()];
    Ok(Some(Scene {
        script: Handle(word(data, 16)?),
        default_refer: word(data, 12)? as i32,
        polygon_count: word(data, 4)?,
        polygons: Handle(word(data, 24)?),
        entrances: table(resources, Handle(word(data, 20)?), word(data, 0)?)?,
        actors: actors(resources, Handle(word(data, 28)?), word(data, 8)?)?,
    }))
}

/// Bound the script to the remaining bytecode chunk, not the rest of the resource.
/// Multiple procedures share a chunk; their precise reachable extent needs CFG analysis.
pub fn script_bytes<'a>(resources: &Resources<'a>, handle: Handle) -> Result<&'a [u8], Error> {
    let file = resources
        .0
        .get(handle.resource())
        .ok_or(Error("invalid script resource"))?;
    let headers = chunks(file)?;
    let chunk = headers
        .iter()
        .find(|c| c.kind == 0x3334000a && c.payload.contains(&handle.offset()))
        .ok_or(Error("script handle outside bytecode chunk"))?;
    Ok(&file[handle.offset()..chunk.payload.end])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actor_records_include_masking_before_id_and_script() {
        let values = [2u32, 7, 0x1234, 1, 9, 0x5678];
        let data = values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let parsed = actors(&Resources(vec![&data]), Handle(0), 2).unwrap();
        assert_eq!(
            parsed[0],
            Actor {
                masking: 2,
                id: 7,
                script: Handle(0x1234)
            }
        );
        assert_eq!(
            parsed[1],
            Actor {
                masking: 1,
                id: 9,
                script: Handle(0x5678)
            }
        );
        assert!(actors(&Resources(vec![&data[..23]]), Handle(0), 2).is_err());
    }
    #[test]
    fn rejects_truncated_scene() {
        let bytes = [14, 0, 52, 51, 0, 0, 0, 0];
        assert!(read(&Resources(vec![&bytes]), 0).is_err());
    }
    #[test]
    fn scripts_must_point_into_pcode() {
        let bytes = [10, 0, 52, 51, 0, 0, 0, 0, 0];
        let resources = Resources(vec![&bytes]);
        assert_eq!(script_bytes(&resources, Handle(8)).unwrap(), [0]);
        assert!(script_bytes(&resources, Handle(0)).is_err());
        assert!(script_bytes(&resources, Handle(9)).is_err());
    }
}
