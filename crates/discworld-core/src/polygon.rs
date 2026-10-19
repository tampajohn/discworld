// SPDX-License-Identifier: GPL-3.0-or-later
//! Checked DOS Tinsel V1 polygon records; based on pinned polygons.cpp Poly.
use crate::{
    film::word,
    graphics::{Handle, Resources},
    iso::Error,
    resource::chunks,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Path,
    NodePath,
    Block,
    Refer,
    Effect,
    Exit,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Polygon {
    pub kind: Kind,
    pub corners: [[i32; 2]; 4],
    pub tag_position: [i32; 2],
    pub tag_text: Handle,
    pub node_position: [i32; 2],
    pub film: Handle,
    pub refer_type: i32,
    pub id: i32,
    pub scales: [i32; 2],
    pub reel: i32,
    pub z_factor: i32,
    pub nodes: Vec<[i32; 2]>,
    /// Original ten signed coefficients per segment, retained without recomputation.
    pub lines: Vec<[i32; 10]>,
    pub script: Handle,
}

fn array<const N: usize>(data: &[u8], offset: usize) -> Result<[i32; N], Error> {
    let mut result = [0; N];
    for (i, value) in result.iter_mut().enumerate() {
        *value = word(data, offset + i * 4)? as i32;
    }
    Ok(result)
}

/// Auxiliary offsets are relative to the polygon table, bounded by its chunk.
pub fn read(resources: &Resources<'_>, handle: Handle, count: u32) -> Result<Vec<Polygon>, Error> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if count > 4096 {
        return Err(Error("polygon count limit"));
    }
    let file = resources
        .0
        .get(handle.resource())
        .ok_or(Error("invalid polygon resource"))?;
    let headers = chunks(file)?;
    let chunk = headers
        .iter()
        .find(|c| c.payload.contains(&handle.offset()))
        .ok_or(Error("polygon handle outside chunk"))?;
    let data = &file[handle.offset()..chunk.payload.end];
    parse(data, count as usize)
}

fn parse(data: &[u8], count: usize) -> Result<Vec<Polygon>, Error> {
    let bytes = count
        .checked_mul(104)
        .ok_or(Error("polygon table overflow"))?;
    let table = data.get(..bytes).ok_or(Error("truncated polygon table"))?;
    table
        .chunks_exact(104)
        .map(|record| {
            let kind = match word(record, 0)? {
                0 => Kind::Path,
                1 => Kind::NodePath,
                2 => Kind::Block,
                3 => Kind::Refer,
                4 => Kind::Effect,
                5 => Kind::Exit,
                6 => Kind::Tag,
                _ => return Err(Error("invalid V1 polygon kind")),
            };
            let x = array::<4>(record, 4)?;
            let y = array::<4>(record, 20)?;
            let mut corners = [[0; 2]; 4];
            for i in 0..4 {
                corners[i] = [x[i], y[i]];
            }
            let mut nodes = Vec::new();
            let mut lines = Vec::new();
            if kind == Kind::NodePath {
                let count = word(record, 84)? as usize;
                if !(2..=4096).contains(&count) {
                    return Err(Error("invalid polygon node count"));
                }
                let xs = word(record, 88)? as usize;
                let ys = word(record, 92)? as usize;
                let ls = word(record, 96)? as usize;
                for (offset, length) in [(xs, count * 4), (ys, count * 4), (ls, (count - 1) * 40)] {
                    let end = offset
                        .checked_add(length)
                        .ok_or(Error("polygon auxiliary overflow"))?;
                    if offset < bytes || end > data.len() {
                        return Err(Error("polygon auxiliary outside data"));
                    }
                }
                for i in 0..count {
                    nodes.push([
                        word(data, xs + i * 4)? as i32,
                        word(data, ys + i * 4)? as i32,
                    ]);
                }
                for i in 0..count - 1 {
                    lines.push(array::<10>(data, ls + i * 40)?);
                }
            }
            Ok(Polygon {
                kind,
                corners,
                tag_position: array(record, 36)?,
                tag_text: Handle(word(record, 44)?),
                node_position: array(record, 48)?,
                film: Handle(word(record, 56)?),
                refer_type: word(record, 60)? as i32,
                id: word(record, 64)? as i32,
                scales: array(record, 68)?,
                reel: word(record, 76)? as i32,
                z_factor: word(record, 80)? as i32,
                nodes,
                lines,
                script: Handle(word(record, 100)?),
            })
        })
        .collect()
}

/// Runtime geometry uses signed 16-bit coordinates, as CommonInits does in V1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePolygon {
    pub definition: Polygon,
    pub enabled: bool,
    pub pointing: bool,
    pub adjacent: Vec<usize>,
}
impl ActivePolygon {
    pub fn scale_at(&self, y: i32) -> Result<usize, Error> {
        let [first, last] = self.definition.scales;
        if !(1..=10).contains(&first) || !(0..=first).contains(&last) {
            return Err(Error("invalid path scale range"));
        }
        if last == 0 {
            return Ok(first as usize);
        }
        let top = self.definition.corners.iter().map(|p| p[1]).min().unwrap();
        let bottom = self.definition.corners.iter().map(|p| p[1]).max().unwrap();
        let depth = (i64::from(bottom) - i64::from(top)) / i64::from(first - last + 1);
        let mut boundary = i64::from(top);
        // Deliberately preserve V1's decrement to zero, rather than clamp to last.
        for scale in (1..=first).rev() {
            boundary += depth;
            if i64::from(y) < boundary {
                return Ok(scale as usize);
            }
        }
        Ok(last as usize)
    }
    /// Reference side tests, including the exclusion of blocking corners.
    pub fn contains(&self, point: [i32; 2]) -> bool {
        let [x, y] = point.map(|v| i64::from(v as i16));
        let corners = self.definition.corners.map(|p| p.map(i64::from));
        if x < corners.iter().map(|p| p[0]).min().unwrap()
            || x > corners.iter().map(|p| p[0]).max().unwrap()
            || y < corners.iter().map(|p| p[1]).min().unwrap()
            || y > corners.iter().map(|p| p[1]).max().unwrap()
        {
            return false;
        }
        let mut tested = false;
        for i in 0..4 {
            let [ax, ay] = corners[i];
            let [bx, by] = corners[(i + 1) % 4];
            if ((ax.min(bx)..=ax.max(bx)).contains(&x) && ((y > ay) == (by > ay)))
                || ((ay.min(by)..=ay.max(by)).contains(&y) && ((x > ax) == (bx > ax)))
            {
                if x * (ay - by) + y * (bx - ax) < ay * bx - ax * by {
                    return false;
                }
                tested = true;
            }
        }
        if tested {
            !(self.definition.kind == Kind::Block && corners.contains(&[x, y]))
        } else {
            corners.iter().filter(|p| p[0] < x).count() == 2
                && corners.iter().filter(|p| p[1] < y).count() == 2
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnabledMemory(std::collections::BTreeMap<(u32, bool, i32), bool>);
impl EnabledMemory {
    /// V1 remembers tags/exits by polygon table handle and ID across scene visits.
    pub fn restore(&self, table: Handle, polygons: &mut Polygons) {
        for p in &mut polygons.active {
            if matches!(p.definition.kind, Kind::Tag | Kind::Exit) {
                p.enabled = self
                    .0
                    .get(&(table.0, p.definition.kind == Kind::Exit, p.definition.id))
                    .copied()
                    .unwrap_or(true);
            }
        }
    }
    pub fn set(
        &mut self,
        table: Handle,
        polygons: &mut Polygons,
        kind: Kind,
        id: i32,
        enabled: bool,
    ) -> Result<(), Error> {
        if !matches!(kind, Kind::Tag | Kind::Exit) {
            return Err(Error("persistent enable requires tag or exit"));
        }
        let mut found = false;
        // V1 tags affect one matching polygon, while exits affect all matches.
        for p in &mut polygons.active {
            if p.definition.kind == kind && p.definition.id == id {
                found = true;
                if p.enabled != enabled {
                    p.enabled = enabled;
                    if !enabled {
                        p.pointing = false;
                    }
                    if kind == Kind::Tag {
                        break;
                    }
                }
            }
        }
        if found {
            self.0.insert((table.0, kind == Kind::Exit, id), enabled);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Polygons {
    pub active: Vec<ActivePolygon>,
    pub default_refer: i32,
}
impl Polygons {
    /// V1 FindPolygon(BLOCK/EX_BLOCK, id) changes only the first matching
    /// polygon in the requested source state. Missing IDs are a no-op.
    pub fn set_block(&mut self, id: i32, enabled: bool) {
        if let Some(polygon) = self.active.iter_mut().find(|p| {
            p.definition.kind == Kind::Block && p.definition.id == id && p.enabled != enabled
        }) {
            polygon.enabled = enabled;
        }
    }
    pub fn new(mut definitions: Vec<Polygon>, default_refer: i32) -> Self {
        for definition in &mut definitions {
            definition.corners = definition.corners.map(|p| p.map(|v| i32::from(v as i16)));
        }
        let mut active: Vec<_> = definitions
            .into_iter()
            .map(|definition| ActivePolygon {
                definition,
                enabled: true,
                pointing: false,
                adjacent: Vec::new(),
            })
            .collect();
        for i in 0..active.len() {
            if !matches!(active[i].definition.kind, Kind::Path | Kind::NodePath) {
                continue;
            }
            for j in i + 1..active.len() {
                if !matches!(active[j].definition.kind, Kind::Path | Kind::NodePath) {
                    continue;
                }
                let a = &active[i];
                let b = &active[j];
                let mut distinct = 0i32;
                for corner in a.definition.corners {
                    distinct += i32::from(b.contains(corner));
                }
                for corner in b.definition.corners {
                    distinct += i32::from(a.contains(corner));
                }
                for ca in a.definition.corners {
                    for cb in b.definition.corners {
                        if ca == cb {
                            distinct -= 1;
                        }
                    }
                }
                if distinct >= 2 {
                    active[i].adjacent.push(j);
                    active[j].adjacent.push(i);
                }
            }
        }
        Self {
            active,
            default_refer,
        }
    }
    pub fn at(&self, point: [i32; 2], kind: Kind) -> Option<usize> {
        self.active
            .iter()
            .position(|p| p.enabled && p.definition.kind == kind && p.contains(point))
    }
    /// Uncached V1 PathOnTheWay/TryPath. Adjacency order is significant; this
    /// deliberately preserves depth-first traversal instead of shortest paths.
    /// A bounded explicit stack replaces reference recursion.
    pub fn path_on_way(&self, from: usize, to: usize) -> Result<usize, Error> {
        let valid = |index: usize| {
            self.active.get(index).is_some_and(|p| {
                p.enabled && matches!(p.definition.kind, Kind::Path | Kind::NodePath)
            })
        };
        if !valid(from) || !valid(to) {
            return Err(Error("invalid route endpoint"));
        }
        let adjacent = |a: usize, b: usize| a == b || self.active[a].adjacent.contains(&b);
        if adjacent(from, to) {
            return Ok(to);
        }
        let mut tried = vec![false; self.active.len()];
        tried[from] = true;
        let mut stack = vec![(from, 0usize)];
        for _ in 0..65536 {
            let Some((current, next)) = stack.last_mut() else {
                return Err(Error("unconnected walking paths"));
            };
            let Some(&candidate) = self.active[*current].adjacent.get(*next) else {
                let (finished, _) = stack.pop().ok_or(Error("empty route stack"))?;
                tried[finished] = false;
                continue;
            };
            *next += 1;
            if !valid(candidate) {
                return Err(Error("invalid path adjacency"));
            }
            if candidate == to {
                // TryPath appends the route on unwind, destination first.
                // PathOnTheWay then chooses its first neighbor of the origin.
                return stack
                    .iter()
                    .skip(1)
                    .rev()
                    .map(|(path, _)| *path)
                    .find(|&path| adjacent(from, path))
                    .ok_or(Error("route has no first leg"));
            }
            if tried[candidate] {
                continue;
            }
            tried[candidate] = true;
            stack.push((candidate, 0));
        }
        Err(Error("path search instruction budget exhausted"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set(data: &mut [u8], at: usize, value: u32) {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn rectangle(kind: Kind, left: i32, top: i32, right: i32, bottom: i32) -> Polygon {
        let mut p = parse(&[0; 104], 1).unwrap().remove(0);
        p.kind = kind;
        p.corners = [[left, top], [right, top], [right, bottom], [left, bottom]];
        p
    }
    #[test]
    fn block_toggles_first_matching_source_state_and_changes_collision_queries() {
        let mut block = rectangle(Kind::Block, 0, 0, 10, 10);
        block.id = 3;
        let mut tag = block.clone();
        tag.kind = Kind::Tag;
        let mut polygons = Polygons::new(vec![tag, block.clone(), block], 0);
        assert_eq!(polygons.at([5, 5], Kind::Block), Some(1));
        polygons.set_block(3, false);
        assert!(polygons.active[0].enabled);
        assert!(!polygons.active[1].enabled);
        assert_eq!(polygons.at([5, 5], Kind::Block), Some(2));
        polygons.set_block(3, false);
        assert_eq!(polygons.at([5, 5], Kind::Block), None);
        polygons.set_block(3, true);
        assert_eq!(polygons.at([5, 5], Kind::Block), Some(1));
        assert!(!polygons.active[2].enabled);
        let before = polygons.clone();
        polygons.set_block(99, false);
        assert_eq!(polygons, before);
    }

    #[test]
    fn route_preserves_depth_first_order_and_unwind_shortcut() {
        let mut polygons = Polygons::new(
            (0..7)
                .map(|i| rectangle(Kind::Path, i * 30, 0, i * 30 + 10, 10))
                .collect(),
            0,
        );
        // First branch reaches the goal through 1,2,3; 4 offers a shorter path.
        for (p, adjacency) in polygons.active.iter_mut().zip([
            vec![1, 4],
            vec![0, 2],
            vec![1, 3],
            vec![2, 5],
            vec![0, 5],
            vec![3, 4],
            vec![],
        ]) {
            p.adjacent = adjacency;
        }
        assert_eq!(polygons.path_on_way(0, 5).unwrap(), 1);
        assert_eq!(polygons.path_on_way(0, 4).unwrap(), 4);
        assert_eq!(polygons.path_on_way(0, 0).unwrap(), 0);
        assert!(polygons.path_on_way(0, 6).is_err());
        // A later origin neighbor on the discovered route skips an earlier leg.
        polygons.active[0].adjacent.push(3);
        assert_eq!(polygons.path_on_way(0, 5).unwrap(), 3);
        assert!(polygons.path_on_way(99, 5).is_err());
        polygons.active[5].enabled = false;
        assert!(polygons.path_on_way(0, 5).is_err());
    }
    #[test]
    fn mover_path_scale_zones_and_node_placement_follow_v1() {
        let mut path = rectangle(Kind::Path, 0, 0, 100, 30);
        path.scales = [5, 3];
        let polygons = Polygons::new(vec![path.clone()], 0);
        for (y, expected) in [(0, 5), (9, 5), (10, 4), (20, 3), (30, 2), (100, 3)] {
            assert_eq!(polygons.active[0].scale_at(y).unwrap(), expected);
        }
        path.kind = Kind::NodePath;
        path.scales = [2, 0];
        path.nodes = vec![[10, 10], [90, 20]];
        path.z_factor = 3;
        let polygons = Polygons::new(vec![path], 0);
        let mut declarations = crate::startup::Declarations::default();
        declarations.movers.insert(
            1,
            std::array::from_fn(|s| {
                std::array::from_fn(|r| Handle(100 + s as u32 * 12 + r as u32))
            }),
        );
        let p = crate::mover::Placement::initial(&declarations, &polygons, 1, [85, 25], Handle(3))
            .unwrap();
        assert_eq!(
            (p.position, p.path, p.node, p.scale, p.standing, p.z_factor),
            ([90, 20], Some(0), Some(1), 2, Handle(116), 3)
        );
        let outside =
            crate::mover::Placement::initial(&declarations, &polygons, 1, [200, 100], Handle(0))
                .unwrap();
        assert_eq!(
            (outside.position, outside.path, outside.node, outside.scale),
            ([200, 100], None, None, 2)
        );
    }
    #[test]
    fn tag_and_exit_enable_state_survives_scene_visits_without_leaking() {
        let mut tag = rectangle(Kind::Tag, 0, 0, 10, 10);
        tag.id = 551;
        let mut exit = tag.clone();
        exit.kind = Kind::Exit;
        let definitions = vec![tag, exit];
        let table = Handle(100);
        let mut polygons = Polygons::new(definitions.clone(), 0);
        let mut memory = EnabledMemory::default();
        polygons.active[0].pointing = true;
        memory
            .set(table, &mut polygons, Kind::Tag, 551, false)
            .unwrap();
        assert_eq!(polygons.at([5, 5], Kind::Tag), None);
        assert_eq!(polygons.at([5, 5], Kind::Exit), Some(1));
        assert!(!polygons.active[0].pointing);
        let mut revisit = Polygons::new(definitions.clone(), 0);
        memory.restore(table, &mut revisit);
        assert!(!revisit.active[0].enabled);
        assert!(revisit.active[1].enabled);
        let mut other_scene = Polygons::new(definitions.clone(), 0);
        memory.restore(Handle(200), &mut other_scene);
        assert!(other_scene.active.iter().all(|p| p.enabled));
        let before = memory.clone();
        memory
            .set(table, &mut revisit, Kind::Tag, 999, false)
            .unwrap();
        assert_eq!(memory, before, "missing IDs are no-ops");
        memory
            .set(table, &mut revisit, Kind::Tag, 551, true)
            .unwrap();
        memory
            .set(table, &mut revisit, Kind::Exit, 551, false)
            .unwrap();
        let snapshot = memory.clone();
        let mut reload = Polygons::new(definitions, 0);
        snapshot.restore(table, &mut reload);
        assert_eq!(reload, revisit);
    }
    #[test]
    fn edges_blocking_corners_and_signed_coordinates() {
        let polygons = Polygons::new(
            vec![
                rectangle(Kind::Path, -10, -10, 10, 10),
                rectangle(Kind::Block, -10, -10, 10, 10),
            ],
            7,
        );
        for point in [[0, 0], [0, -10], [10, 0]] {
            assert!(polygons.active[0].contains(point));
            assert!(polygons.active[1].contains(point));
        }
        assert!(polygons.active[0].contains([-10, -10]));
        assert!(!polygons.active[1].contains([-10, -10]));
        assert!(!polygons.active[0].contains([11, 0]));
        assert_eq!(polygons.at([0, 0], Kind::Path), Some(0));
        assert_eq!(polygons.default_refer, 7);
        let wrapped = Polygons::new(vec![rectangle(Kind::Path, 65526, 65526, 65546, 65546)], 0);
        assert_eq!(
            wrapped.active[0].definition.corners,
            polygons.active[0].definition.corners
        );
    }
    #[test]
    fn adjacency_requires_two_distinct_shared_points() {
        let polygons = Polygons::new(
            vec![
                rectangle(Kind::Path, 0, 0, 10, 10),
                rectangle(Kind::NodePath, 10, 0, 20, 10),
                rectangle(Kind::Path, -10, -10, 0, 0),
                rectangle(Kind::Tag, 0, 0, 10, 10),
            ],
            0,
        );
        assert_eq!(polygons.active[0].adjacent, [1]);
        assert_eq!(polygons.active[1].adjacent, [0]);
        assert!(polygons.active[2].adjacent.is_empty());
        assert!(polygons.active[3].adjacent.is_empty());
        assert!(polygons.active.iter().all(|p| p.enabled && !p.pointing));
    }
    #[test]
    fn v1_fields_and_node_offsets() {
        let mut data = vec![0; 160];
        set(&mut data, 0, 1);
        set(&mut data, 4, (-5i32) as u32);
        set(&mut data, 20, 8);
        set(&mut data, 84, 2);
        set(&mut data, 88, 104);
        set(&mut data, 92, 112);
        set(&mut data, 96, 120);
        set(&mut data, 100, 0x800010);
        set(&mut data, 104, 10);
        set(&mut data, 108, 20);
        set(&mut data, 112, 30);
        set(&mut data, 116, 40);
        set(&mut data, 120, 7);
        let p = parse(&data, 1).unwrap().remove(0);
        assert_eq!(p.corners[0], [-5, 8]);
        assert_eq!(p.nodes, [[10, 30], [20, 40]]);
        assert_eq!(p.lines[0][0], 7);
        assert_eq!(p.script, Handle(0x800010));
        assert!(parse(&data[..159], 1).is_err());
        set(&mut data, 88, 0);
        assert!(parse(&data, 1).is_err());
    }
    #[test]
    fn rejects_bad_records_without_panics() {
        let data = vec![0; 104];
        for length in 0..104 {
            assert!(parse(&data[..length], 1).is_err());
        }
        let mut data = data;
        set(&mut data, 0, 7);
        assert!(parse(&data, 1).is_err());
    }
}
