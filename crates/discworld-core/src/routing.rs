// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 intermediate destinations, separate from per-tick mover scheduling.
use crate::{
    iso::Error,
    nodepath,
    polygon::{ActivePolygon, Kind, Polygons},
    walk::{self, Footprint, Leg, Restriction},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reachability {
    Direct,
    Restricted,
    LeavesPath,
    Blocked,
    Stationary,
}
/// V1 CanGetThere deliberately does not retry with the minor-axis fallback.
pub fn can_get_there(
    polygons: &Polygons,
    from: [i32; 2],
    mut leg: Leg,
    mover: Option<&Footprint>,
    others: &[Footprint],
) -> Result<Reachability, Error> {
    let mut position = from;
    leg.force_minor = false;
    for _ in 0..65536 {
        if leg.target == [-1; 2] {
            return Ok(Reachability::Restricted);
        }
        let step = walk::constrained_leg(position, leg, polygons, mover, others)?;
        let clean_arrival =
            step.reached == [true; 2] && step.restricted == [false; 2] && !step.stuck;
        if clean_arrival {
            return Ok(Reachability::Direct);
        }
        if !step.stuck
            && ((step.reached == [true, false] && step.restricted == [false, true])
                || (step.reached == [false, true] && step.restricted == [true, false]))
        {
            return Ok(Reachability::Restricted);
        }
        if step.stuck {
            return Ok(if step.reason == Some(Restriction::LeavingPath) {
                Reachability::LeavesPath
            } else {
                Reachability::Blocked
            });
        }
        if step.position == position {
            return Ok(Reachability::Stationary);
        }
        position = step.position;
        leg.target = step.target;
    }
    Err(Error("walking reachability probe budget exhausted"))
}
fn path(polygons: &Polygons, index: usize) -> Result<&ActivePolygon, Error> {
    polygons
        .active
        .get(index)
        .filter(|p| p.enabled && matches!(p.definition.kind, Kind::Path | Kind::NodePath))
        .ok_or(Error("invalid intermediate path"))
}
pub fn path_at(polygons: &Polygons, point: [i32; 2]) -> Option<usize> {
    polygons.active.iter().position(|p| {
        p.enabled && matches!(p.definition.kind, Kind::Path | Kind::NodePath) && p.contains(point)
    })
}
/// Preserve PseudoCenter's unusual fallback: the vertical scan changes x,
/// not y. Coordinates are already signed-16-bit runtime polygon corners.
pub fn center(p: &ActivePolygon) -> [i32; 2] {
    let mut point = [0, 1].map(|axis| {
        (p.definition
            .corners
            .iter()
            .map(|c| i64::from(c[axis]))
            .sum::<i64>()
            / 4) as i32
    });
    if !p.contains(point) {
        let top = p.definition.corners.iter().map(|c| c[1]).min().unwrap();
        let bottom = p.definition.corners.iter().map(|c| c[1]).max().unwrap();
        // Runtime corners are short; avoid an unbounded search on manually
        // constructed geometry that has not passed Polygons::new.
        let top = top as i16 as i32;
        let bottom = bottom as i16 as i32;
        let first = (top..=bottom)
            .find(|y| p.contains([point[0], *y]))
            .unwrap_or(0);
        let last = (top..=bottom)
            .rev()
            .find(|y| p.contains([point[0], *y]))
            .unwrap_or(0);
        point[0] = (first + last) / 2;
    }
    point
}
/// V1 BlockingCorner: offset corners by four pixels, preserve source order
/// and Manhattan-distance ties, and choose an adjacent route when already near
/// a corner. The reference leaves the position unchanged if none is reachable.
pub fn blocking_corner(
    polygons: &Polygons,
    corners: [[i32; 2]; 4],
    from: [i32; 2],
    target: [i32; 2],
) -> Result<[i32; 2], Error> {
    let distance = |a: [i32; 2], b: [i32; 2]| {
        (i64::from(a[0]) - i64::from(b[0])).abs() + (i64::from(a[1]) - i64::from(b[1])).abs()
    };
    let mut outside = corners;
    for i in 0..4 {
        for axis in 0..2 {
            let value = corners[i][axis];
            let next = corners[(i + 1) % 4][axis];
            let prev = corners[(i + 3) % 4][axis];
            let delta = if value <= next && value <= prev {
                -4
            } else if value >= next && value >= prev {
                4
            } else {
                0
            };
            outside[i][axis] = value
                .checked_add(delta)
                .ok_or(Error("blocking corner overflow"))?;
        }
    }
    let mut smallest = 1000;
    let mut nearest = None;
    let mut at = None;
    for i in 0..4 {
        let d = distance(from, outside[i]);
        if d < smallest
            && path_at(polygons, corners[i]).is_some()
            && path_at(polygons, outside[i]).is_some()
        {
            if d > 4 {
                nearest = Some(i);
                smallest = d;
            } else {
                at = Some(i);
            }
        }
    }
    let Some(at) = at else {
        return Ok(nearest.map_or(from, |i| outside[i]));
    };
    // The C++ reference indexes its sentinel in this malformed case. Reject
    // instead of inventing a route or indexing outside the corner array.
    let nearest = nearest.ok_or(Error("blocking corner has no alternate corner"))?;
    let clockwise = (at + 1) % 4;
    let counter = (at + 3) % 4;
    let toward = if distance(target, corners[counter]) > distance(target, corners[clockwise]) {
        clockwise
    } else {
        counter
    };
    let selected = if toward == nearest
        || distance(corners[at], corners[toward]) + distance(corners[toward], target)
            > distance(corners[at], corners[nearest]) + distance(corners[nearest], target)
    {
        nearest
    } else {
        toward
    };
    Ok(outside[selected])
}

/// NearestCorner searches source corners before destination corners, retaining
/// the reference's 1000-pixel threshold and special treatment within four pixels.
pub fn corner(
    source: &ActivePolygon,
    destination: &ActivePolygon,
    from: [i32; 2],
) -> Result<[i32; 2], Error> {
    let mut selected = None;
    let mut best = 1000i64;
    for (a, b) in [(source, destination), (destination, source)] {
        for point in a.definition.corners {
            if b.contains(point) {
                let distance = (i64::from(from[0]) - i64::from(point[0])).abs()
                    + (i64::from(from[1]) - i64::from(point[1])).abs();
                if distance < best {
                    selected = Some(point);
                    if distance > 4 {
                        best = distance;
                    }
                }
            }
        }
        if best != 1000 {
            break;
        }
    }
    selected.ok_or(Error("no shared walking corner"))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub current: usize,
    pub position: [i32; 2],
    pub ultimate: [i32; 2],
    pub overshoot: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intermediate {
    pub path: usize,
    pub position: [i32; 2],
}

/// Endpoint used to leave a node path toward the next path selected by the
/// ordered router. Ordinary neighbors use their pseudo-center; node neighbors
/// use the closest pair of endpoints.
pub fn node_exit(polygons: &Polygons, current: usize, destination: usize) -> Result<usize, Error> {
    let source = path(polygons, current)?;
    let next = polygons.path_on_way(current, destination)?;
    let target = path(polygons, next)?;
    if target.definition.kind == Kind::NodePath {
        nodepath::near_end(&source.definition, &target.definition)
    } else {
        nodepath::nearest_end(&source.definition, center(target))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryAction {
    /// Recompute intermediate routing without following this node chain.
    Bypass,
    Move([i32; 2]),
    /// Run SetNextDest, including its near-ultimate completion check, before
    /// advancing the ENTERING state. Do not invent an extra movement tick.
    Next,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeEntry {
    pub travel: Option<nodepath::Travel>,
    pub action: EntryAction,
}
/// The node-specific portion of V1 EnteringNewPath. `projection` is the
/// ultimate destination's projection, only consulted in the destination path.
pub fn enter_node(
    polygons: &Polygons,
    current: usize,
    position: [i32; 2],
    destination: usize,
    projection: nodepath::Projection,
) -> Result<NodeEntry, Error> {
    let p = &path(polygons, current)?.definition;
    let last = if current == destination {
        None
    } else {
        Some(node_exit(polygons, current, destination)?)
    };
    node_entry(
        p,
        position,
        last,
        (current == destination).then_some(projection),
    )
}

/// Scripted ignore-path movement uses target proximity, not graph routing,
/// when selecting the end through which it will leave a node polygon.
pub fn enter_node_direct(
    polygons: &Polygons,
    current: usize,
    position: [i32; 2],
    destination: Option<usize>,
    projection: nodepath::Projection,
) -> Result<NodeEntry, Error> {
    let p = &path(polygons, current)?.definition;
    let last = if destination == Some(current) {
        None
    } else {
        Some(nodepath::nearest_end(p, projection.position)?)
    };
    node_entry(
        p,
        position,
        last,
        (destination == Some(current)).then_some(projection),
    )
}

fn node_entry(
    p: &crate::polygon::Polygon,
    position: [i32; 2],
    last: Option<usize>,
    final_projection: Option<nodepath::Projection>,
) -> Result<NodeEntry, Error> {
    let mut first = nodepath::nearest_end(p, position)?;
    if let Some(last) = last {
        if p.nodes.len() == 2 && p.nodes[first] == p.nodes[last] {
            first = last;
        }
        if last == first {
            return Ok(NodeEntry {
                travel: None,
                action: EntryAction::Bypass,
            });
        }
    }
    let line = first.saturating_sub(1);
    let travel = nodepath::Travel {
        line,
        status: nodepath::TravelStatus::Entering,
        overshoot: true,
    };
    let action = if let Some(projection) = final_projection.filter(|p| p.line == line) {
        EntryAction::Move(projection.position)
    } else if (i64::from(position[0]) - i64::from(p.nodes[first][0])).abs() < 4
        && (i64::from(position[1]) - i64::from(p.nodes[first][1])).abs() < 2
    {
        EntryAction::Next
    } else {
        EntryAction::Move(p.nodes[first])
    };
    Ok(NodeEntry {
        travel: Some(travel),
        action,
    })
}
/// V1 SetMoverIntDest for path-bound walking. Returned path identity can differ
/// from the polygon containing the selected point, intentionally matching V1.
pub fn intermediate(
    polygons: &Polygons,
    request: Request,
    mover: Option<&Footprint>,
    others: &[Footprint],
) -> Result<Intermediate, Error> {
    let current = path(polygons, request.current)?;
    let destination =
        path_at(polygons, request.ultimate).ok_or(Error("ultimate destination has no path"))?;
    let target = path(polygons, destination)?;
    if request.current == destination || target.contains(request.position) {
        return Ok(Intermediate {
            path: destination,
            position: request.ultimate,
        });
    }
    let adjacent = current.adjacent.contains(&destination);
    let next = if adjacent {
        destination
    } else {
        polygons.path_on_way(request.current, destination)?
    };
    let p = path(polygons, next)?;
    let position = if p.definition.kind == Kind::NodePath {
        p.definition.nodes[nodepath::nearest_end(&p.definition, request.position)?]
    } else {
        let reach = can_get_there(
            polygons,
            request.position,
            Leg {
                target: request.ultimate,
                ultimate: request.ultimate,
                overshoot: request.overshoot,
                force_minor: false,
            },
            mover,
            others,
        )?;
        if adjacent {
            if reach == Reachability::LeavesPath {
                corner(current, target, request.ultimate)?
            } else {
                request.ultimate
            }
        } else if reach == Reachability::Direct {
            request.ultimate
        } else {
            center(p)
        }
    };
    Ok(Intermediate {
        path: next,
        position,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn node_entry_selects_endpoint_bypasses_return_route_and_targets_final_segment() {
        let mut a = rect(Kind::NodePath, [0, 0], [10, 10]);
        a.nodes = vec![[0, 5], [10, 5]];
        let mut b = rect(Kind::NodePath, [8, 0], [20, 10]);
        b.nodes = vec![[10, 5], [20, 5]];
        let p = Polygons::new(vec![a, b], 0);
        let goal = nodepath::Projection {
            position: [18, 5],
            line: 0,
        };
        assert_eq!(node_exit(&p, 0, 1).unwrap(), 1);
        let near = enter_node(&p, 0, [0, 5], 1, goal).unwrap();
        assert_eq!(near.action, EntryAction::Next);
        assert_eq!(near.travel.unwrap().line, 0);
        let far = enter_node(&p, 0, [2, 10], 1, goal).unwrap();
        assert_eq!(far.action, EntryAction::Move([0, 5]));
        assert!(far.travel.unwrap().overshoot);
        let bypass = enter_node(&p, 0, [10, 5], 1, goal).unwrap();
        assert_eq!(
            bypass,
            NodeEntry {
                travel: None,
                action: EntryAction::Bypass
            }
        );
        let final_path = enter_node(&p, 1, [10, 5], 1, goal).unwrap();
        assert_eq!(final_path.action, EntryAction::Move([18, 5]));
        let mut repeated = p.clone();
        repeated.active[0].definition.nodes = vec![[5, 5], [5, 5]];
        assert_eq!(
            enter_node(&repeated, 0, [5, 5], 1, goal).unwrap().action,
            EntryAction::Bypass
        );
    }
    #[test]
    fn scripted_node_entry_uses_target_proximity_without_a_connected_route() {
        let mut node = rect(Kind::NodePath, [0, 0], [10, 10]);
        node.nodes = vec![[0, 5], [10, 5]];
        let distant = rect(Kind::Path, [30, 0], [40, 10]);
        let mut polygons = Polygons::new(vec![node, distant], 0);
        let goal = nodepath::Projection {
            position: [35, 5],
            line: 0,
        };
        assert!(enter_node(&polygons, 0, [0, 5], 1, goal).is_err());
        let entry = enter_node_direct(&polygons, 0, [0, 5], Some(1), goal).unwrap();
        assert_eq!(entry.action, EntryAction::Next);
        assert_eq!(
            entry.travel.unwrap().status,
            nodepath::TravelStatus::Entering
        );
        assert_eq!(
            enter_node_direct(&polygons, 0, [10, 5], Some(1), goal)
                .unwrap()
                .action,
            EntryAction::Bypass
        );
        // Ignore-path targets may be outside every polygon.
        assert_eq!(
            enter_node_direct(&polygons, 0, [10, 5], None, goal)
                .unwrap()
                .action,
            EntryAction::Bypass
        );
        let within = nodepath::Projection {
            position: [8, 5],
            line: 0,
        };
        assert_eq!(
            enter_node_direct(&polygons, 0, [0, 5], Some(0), within)
                .unwrap()
                .action,
            EntryAction::Move([8, 5])
        );
        // Two coincident endpoints are a pseudo-single-node bypass.
        polygons.active[0].definition.nodes = vec![[5, 5], [5, 5]];
        assert_eq!(
            enter_node_direct(&polygons, 0, [0, 5], None, goal)
                .unwrap()
                .action,
            EntryAction::Bypass
        );
    }
    #[test]
    fn blocking_corners_keep_clear_of_narrow_path_edge_and_advance_around_obstacle() {
        let p = Polygons::new(vec![rect(Kind::Path, [0, 180], [300, 194])], 0);
        let obstacle = [[130, 177], [205, 177], [205, 183], [130, 183]];
        assert_eq!(
            blocking_corner(&p, obstacle, [206, 180], [100, 180]).unwrap(),
            [209, 187]
        );
        assert_eq!(
            blocking_corner(&p, obstacle, [209, 187], [100, 180]).unwrap(),
            [126, 187]
        );
        assert_eq!(
            blocking_corner(&Polygons::default(), obstacle, [206, 180], [100, 180]).unwrap(),
            [206, 180]
        );
        let wide = Polygons::new(vec![rect(Kind::Path, [-100, -100], [100, 100])], 0);
        let square = [[0, 0], [10, 0], [10, 10], [0, 10]];
        assert_eq!(
            blocking_corner(&wide, square, [5, -20], [5, 30]).unwrap(),
            [-4, -4]
        );
    }
    use crate::{graphics::Handle, polygon::Polygon};
    fn rect(kind: Kind, a: [i32; 2], b: [i32; 2]) -> Polygon {
        Polygon {
            kind,
            corners: [a, [b[0], a[1]], b, [a[0], b[1]]],
            tag_position: [0; 2],
            tag_text: Handle(0),
            node_position: [0; 2],
            film: Handle(0),
            refer_type: 0,
            id: 0,
            scales: [1, 0],
            reel: 0,
            z_factor: 0,
            nodes: vec![],
            lines: vec![],
            script: Handle(0),
        }
    }
    #[test]
    fn probe_distinguishes_path_exit_block_and_restricted_arrival() {
        let p = Polygons::new(vec![rect(Kind::Path, [0, 0], [10, 10])], 0);
        let leg = Leg {
            target: [9, 5],
            ultimate: [9, 5],
            overshoot: false,
            force_minor: false,
        };
        assert_eq!(
            can_get_there(&p, [5, 5], leg, None, &[]).unwrap(),
            Reachability::Direct
        );
        assert_eq!(
            can_get_there(
                &p,
                [5, 5],
                Leg {
                    target: [40, 5],
                    ultimate: [40, 5],
                    ..leg
                },
                None,
                &[]
            )
            .unwrap(),
            Reachability::LeavesPath
        );
        assert_eq!(
            can_get_there(
                &p,
                [5, 5],
                Leg {
                    target: [9, 40],
                    ultimate: [9, 40],
                    ..leg
                },
                None,
                &[]
            )
            .unwrap(),
            Reachability::Restricted
        );
        let p = Polygons::new(
            vec![
                rect(Kind::Path, [0, 0], [40, 10]),
                rect(Kind::Block, [8, 0], [12, 10]),
            ],
            0,
        );
        assert_eq!(
            can_get_there(
                &p,
                [5, 5],
                Leg {
                    target: [30, 5],
                    ultimate: [30, 5],
                    ..leg
                },
                None,
                &[]
            )
            .unwrap(),
            Reachability::Blocked
        );
    }
    #[test]
    fn node_intermediate_uses_endpoint_but_overlap_heads_to_ultimate() {
        let mut node = rect(Kind::NodePath, [8, 0], [20, 10]);
        node.nodes = vec![[9, 5], [19, 5]];
        let p = Polygons::new(vec![rect(Kind::Path, [0, 0], [10, 10]), node], 0);
        let request = Request {
            current: 0,
            position: [2, 5],
            ultimate: [16, 5],
            overshoot: false,
        };
        assert_eq!(
            intermediate(&p, request, None, &[]).unwrap(),
            Intermediate {
                path: 1,
                position: [9, 5]
            }
        );
        assert_eq!(
            intermediate(
                &p,
                Request {
                    position: [9, 5],
                    ..request
                },
                None,
                &[]
            )
            .unwrap(),
            Intermediate {
                path: 1,
                position: [16, 5]
            }
        );
        assert_eq!(
            intermediate(
                &p,
                Request {
                    position: [14, 20],
                    ..request
                },
                None,
                &[]
            )
            .unwrap()
            .position,
            [19, 5],
            "endpoint ties choose last"
        );
    }
    #[test]
    fn nonadjacent_route_chooses_center_only_when_direct_probe_fails() {
        let p = Polygons::new(
            vec![
                rect(Kind::Path, [0, 0], [10, 10]),
                rect(Kind::Path, [8, 0], [20, 10]),
                rect(Kind::Path, [18, 0], [30, 10]),
            ],
            0,
        );
        let request = Request {
            current: 0,
            position: [2, 5],
            ultimate: [28, 5],
            overshoot: false,
        };
        assert_eq!(
            intermediate(&p, request, None, &[]).unwrap(),
            Intermediate {
                path: 1,
                position: [28, 5]
            }
        );
        let mut blocked = p.clone();
        blocked.active.push(crate::polygon::ActivePolygon {
            definition: rect(Kind::Block, [22, 0], [25, 10]),
            enabled: true,
            pointing: false,
            adjacent: vec![],
        });
        assert_eq!(
            intermediate(&blocked, request, None, &[]).unwrap(),
            Intermediate {
                path: 1,
                position: [14, 5]
            }
        );
        assert!(
            intermediate(
                &p,
                Request {
                    current: 99,
                    ..request
                },
                None,
                &[]
            )
            .is_err()
        );
    }
    #[test]
    fn corner_search_preserves_source_priority_and_near_corner_rule() {
        let p = Polygons::new(
            vec![
                rect(Kind::Path, [0, 0], [10, 10]),
                rect(Kind::Path, [8, 0], [20, 10]),
            ],
            0,
        );
        assert_eq!(
            corner(&p.active[0], &p.active[1], [19, 1]).unwrap(),
            [10, 0]
        );
        assert_eq!(
            corner(&p.active[0], &p.active[1], [10, 0]).unwrap(),
            [10, 10]
        );
        assert_eq!(center(&p.active[0]), [5, 5]);
    }
}
