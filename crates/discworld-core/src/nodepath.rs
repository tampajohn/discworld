// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 node-path geometry from the pinned polygons.cpp.
use crate::{
    iso::Error,
    polygon::{Kind, Polygon},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Projection {
    pub position: [i32; 2],
    /// Segment index for a perpendicular, node index for a selected node.
    /// In particular, the last node may return nodes.len() - 1.
    pub line: usize,
}
fn nodes(path: &Polygon) -> Result<&[[i32; 2]], Error> {
    if path.kind != Kind::NodePath || path.nodes.len() < 2 {
        return Err(Error("invalid node path"));
    }
    Ok(&path.nodes)
}
fn distance(a: [i32; 2], b: [i32; 2]) -> i64 {
    (i64::from(a[0]) - i64::from(b[0])).abs() + (i64::from(a[1]) - i64::from(b[1])).abs()
}
/// V1 NearestNodeWithin keeps node zero if no node is within 1000 pixels.
pub fn nearest_node(path: &Polygon, point: [i32; 2]) -> Result<usize, Error> {
    let mut nearest = 0;
    let mut best = 1000;
    for (i, node) in nodes(path)?.iter().enumerate() {
        let d = distance(point, *node);
        if d < best {
            nearest = i;
            best = d;
        }
    }
    Ok(nearest)
}
/// Ties choose the last endpoint, as in NearestEndNode.
pub fn nearest_end(path: &Polygon, point: [i32; 2]) -> Result<usize, Error> {
    let ns = nodes(path)?;
    let last = ns.len() - 1;
    Ok(if distance(point, ns[last]) > distance(point, ns[0]) {
        0
    } else {
        last
    })
}
/// NearEndNode compares endpoint pairs in source-first order. Ties retain the
/// first source endpoint, unlike nearest_end's tie rule.
pub fn near_end(source: &Polygon, destination: &Polygon) -> Result<usize, Error> {
    let s = nodes(source)?;
    let d = nodes(destination)?;
    let last = s.len() - 1;
    let mut best = distance(s[0], d[0]).min(distance(s[0], d[d.len() - 1]));
    let mut selected = 0;
    for end in [d[0], d[d.len() - 1]] {
        let dist = distance(s[last], end);
        if dist < best {
            best = dist;
            selected = last;
        }
    }
    Ok(selected)
}
/// FindBestPoint uses truncated integer perpendiculars, the truncated length
/// of the line normal, and Manhattan distance to nodes. This deliberately is
/// not a generic Euclidean closest-point algorithm.
pub fn project(path: &Polygon, point: [i32; 2]) -> Result<Projection, Error> {
    let ns = nodes(path)?;
    if path.lines.len() != ns.len() - 1 {
        return Err(Error("invalid node path line count"));
    }
    let [h, k] = point.map(i128::from);
    let mut shortest = 10000i128;
    let mut closest = None;
    for (i, line) in path.lines.iter().enumerate() {
        let [a, b, c] = [line[0], line[1], line[2]].map(i128::from);
        if a == 0 && b == 0 {
            continue;
        }
        let denominator = a * a + b * b;
        let x = (b * b * h - a * b * k - a * c) / denominator;
        let y = (a * a * k - a * b * h - b * c) / denominator;
        let within = |v: i128, axis: usize| {
            let ends = [i128::from(ns[i][axis]), i128::from(ns[i + 1][axis])];
            v >= ends[0].min(ends[1]) && v <= ends[0].max(ends[1])
        };
        if !within(x, 0) || !within(y, 1) {
            continue;
        }
        // Match the reference float expression before truncating sqrt. Products
        // and projection numerators use i128 to avoid signed overflow on input.
        let af = line[0] as f32;
        let bf = line[1] as f32;
        let length = (af * af + bf * bf).sqrt() as i128;
        if length == 0 {
            return Err(Error("invalid node path line normal"));
        }
        let distance = ((a * h + b * k + c) / length).abs();
        if distance < shortest {
            shortest = distance;
            closest = Some(Projection {
                position: [
                    i32::try_from(x).map_err(|_| Error("node projection x overflow"))?,
                    i32::try_from(y).map_err(|_| Error("node projection y overflow"))?,
                ],
                line: i,
            });
        }
    }
    let nearest = nearest_node(path, point)?;
    if i128::from(distance(point, ns[nearest])) < shortest {
        Ok(Projection {
            position: ns[nearest],
            line: nearest,
        })
    } else {
        closest.ok_or(Error(
            "node projection has no candidate within reference threshold",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Handle;
    fn path(ns: &[[i32; 2]]) -> Polygon {
        Polygon {
            kind: Kind::NodePath,
            corners: [[-100, -100], [100, -100], [100, 100], [-100, 100]],
            tag_position: [0; 2],
            tag_text: Handle(0),
            node_position: [0; 2],
            film: Handle(0),
            refer_type: 0,
            id: 0,
            scales: [1, 0],
            reel: 0,
            z_factor: 0,
            nodes: ns.to_vec(),
            lines: ns
                .windows(2)
                .map(|pair| {
                    let a = pair[0][1] - pair[1][1];
                    let b = pair[1][0] - pair[0][0];
                    let c = -(a * pair[0][0] + b * pair[0][1]);
                    [a, b, c, 0, 0, 0, 0, 0, 0, 0]
                })
                .collect(),
            script: Handle(0),
        }
    }
    #[test]
    fn perpendicular_projection_truncation_and_terminal_node_index() {
        let p = path(&[[0, 0], [10, 0], [10, 10]]);
        assert_eq!(
            project(&p, [5, 3]).unwrap(),
            Projection {
                position: [5, 0],
                line: 0
            }
        );
        assert_eq!(
            project(&p, [13, 7]).unwrap(),
            Projection {
                position: [10, 7],
                line: 1
            }
        );
        assert_eq!(
            project(&p, [12, 13]).unwrap(),
            Projection {
                position: [10, 10],
                line: 2
            }
        );
        let p = path(&[[-10, -10], [10, 10]]);
        assert_eq!(project(&p, [-4, -1]).unwrap().position, [-2, -2]);
    }
    #[test]
    fn projection_ties_keep_first_line_and_line_over_node() {
        let p = path(&[[0, 0], [10, 0], [10, 10]]);
        assert_eq!(
            project(&p, [5, 5]).unwrap(),
            Projection {
                position: [5, 0],
                line: 0
            }
        );
        assert_eq!(
            project(&p, [10, 0]).unwrap(),
            Projection {
                position: [10, 0],
                line: 0
            }
        );
        assert_eq!(
            project(&p, [10, 10]).unwrap(),
            Projection {
                position: [10, 10],
                line: 1
            }
        );
    }
    #[test]
    fn endpoint_ties_and_nearest_node_threshold_are_distinct() {
        let p = path(&[[0, 0], [10, 0], [20, 0]]);
        assert_eq!(nearest_node(&p, [5, 0]).unwrap(), 0);
        assert_eq!(nearest_end(&p, [10, 0]).unwrap(), 2);
        assert_eq!(near_end(&p, &path(&[[10, -10], [10, 10]])).unwrap(), 0);
        assert_eq!(near_end(&p, &path(&[[19, -10], [19, 10]])).unwrap(), 2);
        assert_eq!(nearest_node(&p, [2000, 0]).unwrap(), 0);
    }
    #[test]
    fn degenerate_lines_and_invalid_data_are_bounded() {
        let p = path(&[[3, 4], [3, 4]]);
        assert_eq!(
            project(&p, [5, 4]).unwrap(),
            Projection {
                position: [3, 4],
                line: 0
            }
        );
        assert!(project(&p, [i32::MAX, i32::MIN]).is_err());
        let mut p = path(&[[0, 0], [10, 0]]);
        p.lines.clear();
        assert!(project(&p, [0, 0]).is_err());
        p.kind = Kind::Path;
        assert!(nearest_node(&p, [0, 0]).is_err());
    }

    fn moving(position: [i32; 2], overshoot: bool) -> Steering {
        Steering::Move {
            position,
            overshoot,
        }
    }
    #[test]
    fn traversal_follows_bends_in_both_directions_and_finishes_on_target_line() {
        let p = path(&[[0, 0], [10, 0], [10, 10], [20, 10]]);
        let mut travel = Travel {
            line: 0,
            status: TravelStatus::Entering,
            overshoot: false,
        };
        let goal = Projection {
            position: [15, 10],
            line: 2,
        };
        assert_eq!(
            travel.start_within(&p, goal).unwrap(),
            moving([10, 0], false)
        );
        assert_eq!(
            travel.advance(&p, [10, 0], goal.position, Some(2)).unwrap(),
            moving([10, 10], true)
        );
        let mut snapshot = travel;
        assert_eq!(
            travel
                .advance(&p, [10, 10], goal.position, Some(2))
                .unwrap(),
            moving([15, 10], false)
        );
        snapshot
            .advance(&p, [10, 10], goal.position, Some(2))
            .unwrap();
        assert_eq!(travel, snapshot);
        assert_eq!(travel.line, 2);
        let goal = Projection {
            position: [5, 0],
            line: 0,
        };
        assert_eq!(
            travel.start_within(&p, goal).unwrap(),
            moving([10, 10], false)
        );
        assert_eq!(
            travel
                .advance(&p, [10, 10], goal.position, Some(0))
                .unwrap(),
            moving([10, 0], true)
        );
        assert_eq!(
            travel.advance(&p, [10, 0], goal.position, Some(0)).unwrap(),
            moving([5, 0], false)
        );
        assert_eq!(travel.line, 0);
    }
    #[test]
    fn leaving_after_arrival_at_last_node_reuses_endpoint() {
        let p = path(&[[0, 0], [10, 0]]);
        let mut travel = Travel {
            line: 1,
            status: TravelStatus::GoingUp,
            overshoot: false,
        };
        assert_eq!(
            travel.start_leaving(&p, [10, 0], 1).unwrap(),
            Some(moving([10, 0], false))
        );
        assert_eq!(
            travel.advance(&p, [10, 0], [30, 0], None).unwrap(),
            Steering::Route
        );
        assert_eq!(travel.status, TravelStatus::Leaving);
        assert!(Travel::node(&p, 3).is_err());
    }
    #[test]
    fn traversal_enters_at_endpoints_and_requests_routing_when_leaving() {
        let p = path(&[[0, 0], [10, 0], [20, 0]]);
        for (position, expected, line, status) in [
            ([0, 0], [10, 0], 0, TravelStatus::GoingUp),
            ([20, 0], [10, 0], 1, TravelStatus::GoingDown),
        ] {
            let mut travel = Travel {
                line: 0,
                status: TravelStatus::Entering,
                overshoot: false,
            };
            assert_eq!(
                travel.advance(&p, position, [100, 0], None).unwrap(),
                moving(expected, true)
            );
            assert_eq!((travel.line, travel.status), (line, status));
            let final_node = if status == TravelStatus::GoingUp {
                [20, 0]
            } else {
                [0, 0]
            };
            assert_eq!(
                travel.advance(&p, expected, [100, 0], None).unwrap(),
                moving(final_node, true)
            );
            assert_eq!(
                travel.advance(&p, final_node, [100, 0], None).unwrap(),
                Steering::Route
            );
            assert_eq!(travel.status, TravelStatus::Leaving);
            let end = if final_node[0] == 0 { 0 } else { 2 };
            assert_eq!(travel.start_leaving(&p, final_node, end).unwrap(), None);
        }
        let p = path(&[[7, 8], [7, 8]]);
        let mut travel = Travel {
            line: 0,
            status: TravelStatus::Entering,
            overshoot: false,
        };
        assert_eq!(
            travel.advance(&p, [7, 8], [100, 0], None).unwrap(),
            Steering::Route
        );
        assert_eq!(travel.status, TravelStatus::Leaving);
    }
    #[test]
    fn traversal_reversal_and_invalid_requests_are_atomic() {
        let p = path(&[[0, 0], [10, 0], [20, 0]]);
        let mut travel = Travel {
            line: 1,
            status: TravelStatus::GoingUp,
            overshoot: true,
        };
        assert_eq!(
            travel.start_leaving(&p, [12, 0], 0).unwrap(),
            Some(moving([10, 0], true))
        );
        assert_eq!(travel.status, TravelStatus::GoingDown);
        let before = travel;
        assert!(travel.start_leaving(&p, [12, 0], 1).is_err());
        assert_eq!(travel, before);
        assert!(
            travel
                .start_within(
                    &p,
                    Projection {
                        position: [0, 0],
                        line: 99
                    }
                )
                .is_err()
        );
        assert_eq!(travel, before);
        assert!(travel.advance(&p, [12, 0], [0, 0], Some(99)).is_err());
        assert_eq!(travel, before);
    }
}

/// Node traversal state corresponding to V1 npstatus and line. The caller owns
/// path identity, ultimate destination, animation, collision, and scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TravelStatus {
    Entering,
    GoingUp,
    GoingDown,
    Leaving,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Travel {
    pub line: usize,
    pub status: TravelStatus,
    pub overshoot: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steering {
    Move {
        position: [i32; 2],
        overshoot: bool,
    },
    /// Recompute the intermediate destination in the connected path system.
    Route,
}
fn close(a: [i32; 2], b: [i32; 2]) -> bool {
    (i64::from(a[0]) - i64::from(b[0])).abs() < 4 && (i64::from(a[1]) - i64::from(b[1])).abs() < 2
}
impl Travel {
    fn node(path: &Polygon, index: usize) -> Result<[i32; 2], Error> {
        let ns = nodes(path)?;
        // getNpathNode permits one past the last node after arrival from
        // above, returning the last endpoint in that specific case.
        let index = if index == ns.len() { index - 1 } else { index };
        ns.get(index)
            .copied()
            .ok_or(Error("node traversal index out of bounds"))
    }
    fn leg(&self, position: [i32; 2]) -> Steering {
        Steering::Move {
            position,
            overshoot: self.overshoot,
        }
    }
    /// SetOffWithinNodePath for a destination on this same path. Retains the
    /// previous overshoot setting for non-final segments, as V1 does.
    pub fn start_within(&mut self, path: &Polygon, target: Projection) -> Result<Steering, Error> {
        Self::node(path, self.line)?;
        Self::node(path, target.line)?;
        let mut next = *self;
        let point = if self.line == target.line {
            next.overshoot = false;
            target.position
        } else if self.line < target.line {
            next.status = TravelStatus::GoingUp;
            Self::node(path, self.line + 1)?
        } else {
            next.status = TravelStatus::GoingDown;
            Self::node(path, self.line)?
        };
        *self = next;
        Ok(self.leg(point))
    }
    /// SetOffWithinNodePath after the connected-path router selects an exit
    /// endpoint. None means V1 leaves the current destination unchanged.
    pub fn start_leaving(
        &mut self,
        path: &Polygon,
        position: [i32; 2],
        end: usize,
    ) -> Result<Option<Steering>, Error> {
        let ns = nodes(path)?;
        if end != 0 && end != ns.len() - 1 {
            return Err(Error("node exit is not an endpoint"));
        }
        Self::node(path, self.line)?;
        if self.status == TravelStatus::Leaving && end == nearest_end(path, position)? {
            return Ok(None);
        }
        let (status, point) = if end != 0 {
            (TravelStatus::GoingUp, Self::node(path, self.line + 1)?)
        } else {
            (TravelStatus::GoingDown, Self::node(path, self.line)?)
        };
        self.status = status;
        Ok(Some(self.leg(point)))
    }
    /// Node-specific SetNextDest after reaching the current leg. target_line
    /// is Some only when this is the ultimate destination path.
    pub fn advance(
        &mut self,
        path: &Polygon,
        position: [i32; 2],
        ultimate: [i32; 2],
        target_line: Option<usize>,
    ) -> Result<Steering, Error> {
        let ns = nodes(path)?;
        Self::node(path, self.line)?;
        if let Some(line) = target_line {
            Self::node(path, line)?;
        }
        let mut next = *self;
        let result = match self.status {
            TravelStatus::Leaving => Steering::Route,
            TravelStatus::Entering => {
                let end = nearest_end(path, position)?;
                let point = if end != 0 {
                    next.status = TravelStatus::GoingDown;
                    next.line = end - 1;
                    ns[end - 1]
                } else {
                    next.status = TravelStatus::GoingUp;
                    next.line = 0;
                    ns[1]
                };
                if ns.len() == 2 && close(position, point) {
                    next.status = TravelStatus::Leaving;
                    Steering::Route
                } else {
                    next.overshoot = true;
                    next.leg(point)
                }
            }
            TravelStatus::GoingUp => {
                if target_line == Some(self.line + 1) {
                    next.line += 1;
                    next.overshoot = false;
                    next.leg(ultimate)
                } else if let Some(point) = ns.get(self.line + 2) {
                    next.line += 1;
                    next.overshoot = !close(ultimate, *point);
                    next.leg(*point)
                } else {
                    next.status = TravelStatus::Leaving;
                    Steering::Route
                }
            }
            TravelStatus::GoingDown => {
                if self
                    .line
                    .checked_sub(1)
                    .is_some_and(|line| target_line == Some(line))
                {
                    next.line -= 1;
                    next.overshoot = false;
                    next.leg(ultimate)
                } else if let Some(line) = self.line.checked_sub(1) {
                    next.line = line;
                    next.overshoot = !close(ultimate, ns[line]);
                    next.leg(ns[line])
                } else {
                    next.status = TravelStatus::Leaving;
                    Steering::Route
                }
            }
        };
        *self = next;
        Ok(result)
    }
}
