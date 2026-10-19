// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 final-target step and facing rules from pinned move.cpp.
//! Routing, collision response, and scheduling are separate from these calculations.
use crate::{iso::Error, mover::Direction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restriction {
    LeavingPath,
    EnteringBlock(usize),
    EnteringMover(i32),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstrainedStep {
    pub position: [i32; 2],
    pub target: [i32; 2],
    pub reached: [bool; 2],
    pub restricted: [bool; 2],
    pub stuck: bool,
    pub reason: Option<Restriction>,
}

/// Final-destination V1 NewCoOrdinates, including its horizontal-first slide
/// attempts. Intermediate-target overshoot and routing are handled separately.
pub fn constrained_step(
    from: [i32; 2],
    target: [i32; 2],
    force_minor: bool,
    polygons: &crate::polygon::Polygons,
    mover: Option<&Footprint>,
    others: &[Footprint],
) -> ConstrainedStep {
    let mut result = ConstrainedStep {
        position: direct_step(from, target, force_minor),
        target,
        reached: [false; 2],
        restricted: [false; 2],
        stuck: false,
        reason: None,
    };
    for axis in 0..2 {
        result.reached[axis] = target[axis] == -1 || result.position[axis] == target[axis];
        if from[axis] == target[axis] {
            result.target[axis] = -1;
        }
    }
    constrain(from, result, polygons, mover, others)
}

/// V1 movement retries a stalled step with at least one pixel on the minor
/// axis. Restore the original destination before retrying: the first attempt
/// may have cleared target axes when it hit a path edge.
pub fn retry_step(
    from: [i32; 2],
    target: [i32; 2],
    polygons: &crate::polygon::Polygons,
    mover: Option<&Footprint>,
    others: &[Footprint],
) -> ConstrainedStep {
    let first = constrained_step(from, target, false, polygons, mover, others);
    if first.position == from && first.reached != [true; 2] {
        constrained_step(from, target, true, polygons, mover, others)
    } else {
        first
    }
}

/// Full intermediate-leg step followed by the same V1 collision handling used
/// for final destinations. A coordinate overflow rejects the attempted step.
pub fn constrained_leg(
    from: [i32; 2],
    leg: Leg,
    polygons: &crate::polygon::Polygons,
    mover: Option<&Footprint>,
    others: &[Footprint],
) -> Result<ConstrainedStep, Error> {
    Ok(constrain(from, leg.attempt(from)?, polygons, mover, others))
}
fn constrain(
    from: [i32; 2],
    mut result: ConstrainedStep,
    polygons: &crate::polygon::Polygons,
    mover: Option<&Footprint>,
    others: &[Footprint],
) -> ConstrainedStep {
    use crate::polygon::Kind;
    // The reference returns before collision checks when both axes arrive.
    if result.reached == [true; 2] {
        return result;
    }
    let path = |point| {
        polygons.active.iter().any(|p| {
            p.enabled
                && matches!(p.definition.kind, Kind::Path | Kind::NodePath)
                && p.contains(point)
        })
    };
    let block = |point| polygons.at(point, Kind::Block);
    let collision = |point| mover.and_then(|m| m.blocker(point, others, false));
    let legal = |point| path(point) && block(point).is_none();
    let slide = |result: &mut ConstrainedStep, avoid_movers: bool| {
        let horizontal = [result.position[0], from[1]];
        let vertical = [from[0], result.position[1]];
        let allowed = |point| legal(point) && (!avoid_movers || collision(point).is_none());
        if horizontal[0] != from[0] && allowed(horizontal) {
            result.position = horizontal;
            result.restricted[1] = true;
        } else if vertical[1] != from[1] && allowed(vertical) {
            result.position = vertical;
            result.restricted[0] = true;
        } else {
            result.position = from;
            result.target = [-1; 2];
            result.stuck = true;
        }
    };
    if !path(result.position) {
        result.reason = Some(Restriction::LeavingPath);
        slide(&mut result, false);
        if result.stuck {
            return result;
        }
    }
    if let Some(index) = block(result.position) {
        result.reason = Some(Restriction::EnteringBlock(index));
        slide(&mut result, false);
    }
    if let Some(other) = collision(result.position) {
        if collision(from).is_none() {
            result.reason = Some(Restriction::EnteringMover(other.actor));
            slide(&mut result, true);
        }
    }
    result
}

/// Geometry used by V1 InMoverBlock. Supply active movers in declaration order;
/// hidden movers still block. Sprite top/bottom do not participate in this test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footprint {
    pub actor: i32,
    pub position: [i32; 2],
    pub left: i64,
    pub right: i64,
    pub node_path: bool,
}

impl Footprint {
    /// First blocking actor in the supplied mover order, at a proposed position.
    pub fn blocker<'a>(
        &self,
        position: [i32; 2],
        others: &'a [Self],
        no_blocking: bool,
    ) -> Option<&'a Self> {
        if self.node_path || no_blocking {
            return None;
        }
        let dx = i128::from(position[0]) - i128::from(self.position[0]);
        let left = i128::from(self.left) + dx;
        let right = i128::from(self.right) + dx;
        others.iter().find(|other| {
            other.actor != self.actor
                && !other.node_path
                && (i64::from(position[1]) - i64::from(other.position[1])).abs() <= 2
                && right > i128::from(other.left)
                && left < i128::from(other.right)
        })
    }

    /// InitExtraBlock corners, including the original signed-16-bit wrapping.
    /// This obstacle expands the other mover by the caller's asymmetric width.
    pub fn obstacle(&self, other: &Self) -> [[i32; 2]; 4] {
        let left =
            i128::from(other.left) - (i128::from(self.right) - i128::from(self.position[0])) - 2;
        let right =
            i128::from(other.right) + (i128::from(self.position[0]) - i128::from(self.left)) + 2;
        let top = i64::from(other.position[1]) - 3;
        let bottom = i64::from(other.position[1]) + 3;
        [
            [left as i16 as i32, top as i16 as i32],
            [right as i16 as i32, top as i16 as i32],
            [right as i16 as i32, bottom as i16 as i32],
            [left as i16 as i32, bottom as i16 as i32],
        ]
    }
}

/// Attempt a final-destination step without overshooting. -1 disables an axis.
/// `force_minor` is the reference's fallback when its first attempt did not move.
pub fn direct_step(from: [i32; 2], target: [i32; 2], force_minor: bool) -> [i32; 2] {
    let (distance, sizes) = step_sizes(from, target, force_minor);
    std::array::from_fn(|axis| {
        if target[axis] == -1 {
            return from[axis];
        }
        let delta = i64::from(target[axis]) - i64::from(from[axis]);
        let step = sizes[axis].min(distance[axis]);
        (i64::from(from[axis]) + delta.signum() * step) as i32
    })
}

fn step_sizes(from: [i32; 2], target: [i32; 2], force_minor: bool) -> ([i64; 2], [i64; 2]) {
    let distance = [0, 1].map(|axis| {
        if target[axis] == -1 {
            0
        } else {
            (i64::from(target[axis]) - i64::from(from[axis])).abs()
        }
    });
    let [dx, dy] = distance;
    let (sx, sy) = if dx != 0 && dy > dx {
        ((2 * dx / dy).max(1), 2)
    } else if dy != 0 && dx > dy {
        let sy = 4 * dy / dx;
        (4, if sy == 0 && force_minor { 1 } else { sy.min(2) })
    } else {
        (if dx != 0 { 4 } else { 0 }, if dy != 0 { 2 } else { 0 })
    };
    (distance, [sx, sy])
}

/// V1 NewCoOrdinates inputs for an intermediate or final leg. An axis set to
/// -1 is disabled, and arrival can change either axis to -1 or to an overshoot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leg {
    pub target: [i32; 2],
    pub ultimate: [i32; 2],
    pub overshoot: bool,
    pub force_minor: bool,
}
impl Leg {
    pub fn attempt(self, from: [i32; 2]) -> Result<ConstrainedStep, Error> {
        let final_target = (self.target[0] == self.ultimate[0]
            && (self.target[1] == -1 || self.target[1] == self.ultimate[1]))
            || (self.target[1] == self.ultimate[1]
                && (self.target[0] == -1 || self.target[0] == self.ultimate[0]));
        let over = self.overshoot && !final_target;
        let (distance, sizes) = step_sizes(from, self.target, self.force_minor);
        let mut result = ConstrainedStep {
            position: from,
            target: self.target,
            reached: [false; 2],
            restricted: [false; 2],
            stuck: false,
            reason: None,
        };
        for axis in 0..2 {
            if self.target[axis] == -1 {
                result.reached[axis] = true;
                continue;
            }
            if from[axis] == self.target[axis] {
                result.target[axis] = -1;
                result.reached[axis] = true;
                continue;
            }
            let delta = i64::from(self.target[axis]) - i64::from(from[axis]);
            let size = if over {
                sizes[axis]
            } else {
                sizes[axis].min(distance[axis])
            };
            result.position[axis] = i32::try_from(i64::from(from[axis]) + delta.signum() * size)
                .map_err(|_| Error("intermediate walking coordinate overflow"))?;
            if sizes[axis] >= distance[axis] {
                result.reached[axis] = true;
                if over && sizes[axis] > distance[axis] {
                    result.target[axis] = result.position[axis];
                }
            }
        }
        Ok(result)
    }
}

/// V1 GetDirection uses a doubled vertical distance and avoids small gratuitous turns.
pub fn direction(
    from: [i32; 2],
    target: [i32; 2],
    previous: Direction,
    reel_type: i32,
) -> Result<Direction, Error> {
    let dx = if target[0] == -1 {
        0
    } else {
        i64::from(target[0]) - i64::from(from[0])
    };
    let dy = if target[1] == -1 {
        0
    } else {
        i64::from(target[1]) - i64::from(from[1])
    };
    let horizontal = if dx < 0 {
        Direction::Left
    } else {
        Direction::Right
    };
    let vertical = if dy < 0 {
        Direction::Away
    } else {
        Direction::Forward
    };
    match reel_type {
        3 => {
            return Ok(if dy != 0 {
                vertical
            } else if previous == Direction::Away {
                previous
            } else {
                Direction::Forward
            });
        }
        2 => {
            return Ok(if dx != 0 {
                horizontal
            } else if previous == Direction::Left {
                previous
            } else {
                Direction::Right
            });
        }
        0 | 1 => {}
        _ => return Err(Error("invalid path reel type")),
    }
    if dx == 0 && dy == 0 {
        return Ok(previous);
    }
    if dx == 0 {
        return Ok(vertical);
    }
    if dy == 0 {
        return Ok(horizontal);
    }
    if dx.abs() <= 4 && dy.abs() * 2 <= 4 && (previous == horizontal || previous == vertical) {
        return Ok(previous);
    }
    Ok(if dx.abs() > dy.abs() * 2 {
        horizontal
    } else {
        vertical
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intermediate_overshoot_updates_targets_in_both_directions() {
        for (from, target, expected) in [([0, 0], [1, 1], [4, 2]), ([10, 10], [9, 9], [6, 8])] {
            let leg = Leg {
                target,
                ultimate: [50, 50],
                overshoot: true,
                force_minor: false,
            };
            let step = leg.attempt(from).unwrap();
            assert_eq!(step.position, expected);
            assert_eq!(step.target, expected);
            assert_eq!(step.reached, [true; 2]);
            let next = Leg {
                target: step.target,
                ..leg
            }
            .attempt(step.position)
            .unwrap();
            assert_eq!(next.position, expected);
            assert_eq!(next.target, [-1; 2]);
            assert_eq!(next.reached, [true; 2]);
        }
    }
    #[test]
    fn ultimate_destination_suppresses_overshoot_with_disabled_axes() {
        for (target, ultimate, expected) in [
            ([1, 1], [1, 1], [1, 1]),
            ([1, -1], [1, 40], [1, 0]),
            ([-1, 1], [40, 1], [0, 1]),
            ([1, 1], [1, 40], [4, 2]),
        ] {
            let step = Leg {
                target,
                ultimate,
                overshoot: true,
                force_minor: false,
            }
            .attempt([0, 0])
            .unwrap();
            assert_eq!(step.position, expected);
        }
        let leg = Leg {
            target: [100, 1],
            ultimate: [200, 50],
            overshoot: true,
            force_minor: false,
        };
        assert_eq!(leg.attempt([0, 0]).unwrap().position, [4, 0]);
        assert_eq!(
            Leg {
                force_minor: true,
                ..leg
            }
            .attempt([0, 0])
            .unwrap()
            .position,
            [4, 1]
        );
        assert!(
            Leg {
                target: [i32::MAX, -1],
                ..leg
            }
            .attempt([i32::MAX - 1, 0])
            .is_err()
        );
    }
    #[test]
    fn intermediate_constraints_preserve_arrival_short_circuit_and_final_steps() {
        let empty = crate::polygon::Polygons::new(vec![], 0);
        let leg = Leg {
            target: [1, 1],
            ultimate: [100, 100],
            overshoot: true,
            force_minor: false,
        };
        let step = constrained_leg([0, 0], leg, &empty, None, &[]).unwrap();
        assert_eq!(step.position, [4, 2]);
        assert_eq!(step.reason, None, "arrival precedes path checks in V1");
        for x in -8..=8 {
            for y in -8..=8 {
                let target = [x, y];
                let leg = Leg {
                    target,
                    ultimate: target,
                    ..leg
                };
                assert_eq!(
                    constrained_leg([0, 0], leg, &empty, None, &[]).unwrap(),
                    constrained_step([0, 0], target, false, &empty, None, &[])
                );
            }
        }
    }
    fn rectangle(kind: crate::polygon::Kind, corners: [[i32; 2]; 4]) -> crate::polygon::Polygon {
        crate::polygon::Polygon {
            kind,
            corners,
            tag_position: [0; 2],
            tag_text: crate::graphics::Handle(0),
            node_position: [0; 2],
            film: crate::graphics::Handle(0),
            refer_type: 0,
            id: 0,
            scales: [1, 0],
            reel: 0,
            z_factor: 0,
            nodes: vec![],
            lines: vec![],
            script: crate::graphics::Handle(0),
        }
    }
    #[test]
    fn stalled_shallow_walk_retries_minor_axis_without_leaving_path() {
        use crate::polygon::{Kind, Polygons};
        let polygons = Polygons::new(
            vec![rectangle(
                Kind::Path,
                [[60, 194], [60, 177], [196, 177], [300, 194]],
            )],
            0,
        );
        let mut position = [200, 178];
        let target = [260, 188];
        let mut retries = 0;
        for _ in 0..100 {
            if position == target {
                break;
            }
            let first = constrained_step(position, target, false, &polygons, None, &[]);
            let step = retry_step(position, target, &polygons, None, &[]);
            if first.position == position {
                retries += 1;
            }
            assert!(!step.stuck);
            assert!(polygons.active[0].contains(step.position));
            assert_ne!(step.position, position);
            position = step.position;
        }
        assert_eq!(position, target);
        assert!(retries > 0);
        let empty = Polygons::default();
        assert!(retry_step([0, 0], [20, 5], &empty, None, &[]).stuck);
    }

    #[test]
    fn path_boundary_slides_horizontally_then_vertically_or_stops() {
        use crate::polygon::{Kind, Polygons};
        let horizontal = Polygons::new(
            vec![rectangle(Kind::Path, [[0, 0], [20, 0], [20, 1], [0, 1]])],
            0,
        );
        let step = constrained_step([0, 0], [20, 20], false, &horizontal, None, &[]);
        assert_eq!(step.position, [4, 0]);
        assert_eq!(step.restricted, [false, true]);
        assert_eq!(step.reason, Some(Restriction::LeavingPath));
        let vertical = Polygons::new(
            vec![rectangle(
                Kind::NodePath,
                [[0, 0], [1, 0], [1, 20], [0, 20]],
            )],
            0,
        );
        let step = constrained_step([0, 0], [20, 20], false, &vertical, None, &[]);
        assert_eq!(step.position, [0, 2]);
        assert_eq!(step.restricted, [true, false]);
        let step = constrained_step([0, 0], [20, 20], false, &Polygons::default(), None, &[]);
        assert_eq!(step.position, [0, 0]);
        assert_eq!(step.target, [-1, -1]);
        assert!(step.stuck);
        // Final arrival bypasses path checking, as NewCoOrdinates does.
        let step = constrained_step([0, 0], [4, 2], false, &Polygons::default(), None, &[]);
        assert_eq!(step.position, [4, 2]);
        assert_eq!(step.reached, [true, true]);
        assert!(!step.stuck);
    }
    #[test]
    fn block_and_mover_sliding_preserve_reference_priority() {
        use crate::polygon::{Kind, Polygons};
        let path = rectangle(Kind::Path, [[-20, -20], [40, -20], [40, 40], [-20, 40]]);
        let block = rectangle(Kind::Block, [[2, 1], [10, 1], [10, 10], [2, 10]]);
        let mut polygons = Polygons::new(vec![path, block], 0);
        let step = constrained_step([0, 0], [20, 20], false, &polygons, None, &[]);
        assert_eq!(step.position, [4, 0]);
        assert_eq!(step.reason, Some(Restriction::EnteringBlock(1)));
        polygons.active[1].enabled = false;
        assert_eq!(
            constrained_step([0, 0], [20, 20], false, &polygons, None, &[]).position,
            [4, 2]
        );
        let caller = Footprint {
            actor: 1,
            position: [0, 0],
            left: -1,
            right: 1,
            node_path: false,
        };
        let other = Footprint {
            actor: 2,
            position: [4, 3],
            left: 2,
            right: 6,
            node_path: false,
        };
        let step = constrained_step([0, 0], [20, 20], false, &polygons, Some(&caller), &[other]);
        assert_eq!(step.position, [4, 0]);
        assert_eq!(step.reason, Some(Restriction::EnteringMover(2)));
        let overlap = Footprint {
            position: [0, 0],
            left: -10,
            right: 10,
            ..other
        };
        let step = constrained_step(
            [0, 0],
            [20, 20],
            false,
            &polygons,
            Some(&caller),
            &[overlap],
        );
        assert_eq!(
            step.position,
            [4, 2],
            "an existing mover overlap may be escaped"
        );
        assert_eq!(step.reason, None);
        let step = constrained_step([0, 0], [-1, 20], false, &polygons, None, &[]);
        assert_eq!(step.position, [0, 2]);
        assert_eq!(step.reached, [true, false]);
    }
    #[test]
    fn mover_collision_projects_width_and_uses_foot_height() {
        let caller = Footprint {
            actor: 7,
            position: [10, 20],
            left: 6,
            right: 16,
            node_path: false,
        };
        let other = Footprint {
            actor: 8,
            position: [30, 20],
            left: 25,
            right: 33,
            node_path: false,
        };
        let others = [caller, other];
        assert!(caller.blocker([19, 20], &others, false).is_none());
        assert_eq!(caller.blocker([20, 22], &others, false), Some(&other));
        assert!(caller.blocker([20, 23], &others, false).is_none());
        assert!(caller.blocker([37, 20], &others, false).is_none());
        assert!(caller.blocker([20, 20], &others, true).is_none());
        let on_node = Footprint {
            node_path: true,
            ..caller
        };
        assert!(on_node.blocker([20, 20], &others, false).is_none());
        assert!(
            caller
                .blocker(
                    [20, 20],
                    &[Footprint {
                        node_path: true,
                        ..other
                    }],
                    false
                )
                .is_none()
        );
        let reordered = [Footprint { actor: 9, ..other }, other];
        assert_eq!(
            caller.blocker([20, 20], &reordered, false).unwrap().actor,
            9
        );
        assert_eq!(
            caller.obstacle(&other),
            [[17, 17], [39, 17], [39, 23], [17, 23]]
        );
    }
    #[test]
    fn mover_geometry_widens_arithmetic_and_wraps_polygon_coordinates() {
        let caller = Footprint {
            actor: 1,
            position: [i32::MIN, i32::MIN],
            left: i64::MIN,
            right: i64::MAX,
            node_path: false,
        };
        let other = Footprint {
            actor: 2,
            position: [i32::MAX, i32::MAX],
            left: 32766,
            right: 32767,
            node_path: false,
        };
        assert!(caller.blocker([0, i32::MIN], &[other], false).is_none());
        assert!(
            caller
                .blocker([i32::MAX, i32::MAX], &[other], false)
                .is_some()
        );
        let narrow = Footprint {
            position: [0, 0],
            left: 0,
            right: 0,
            ..caller
        };
        assert_eq!(
            narrow.obstacle(&other),
            [[32764, -4], [-32767, -4], [-32767, 2], [32764, 2]]
        );
    }
    #[test]
    fn room_walk_uses_twelve_steps_and_never_overshoots() {
        let mut position = [248, 178];
        for _ in 0..12 {
            position = direct_step(position, [200, 178], false);
        }
        assert_eq!(position, [200, 178]);
        assert_eq!(direct_step([202, 179], [200, 178], false), [200, 178]);
        assert_eq!(direct_step([10, 10], [-1, 14], false), [10, 12]);
    }
    #[test]
    fn diagonal_rounding_and_full_signed_range_are_bounded() {
        assert_eq!(direct_step([0, 0], [100, 1], false), [4, 0]);
        assert_eq!(direct_step([0, 0], [100, 1], true), [4, 1]);
        assert_eq!(direct_step([0, 0], [1, 100], false), [1, 2]);
        assert_eq!(direct_step([0, 0], [10, 10], false), [4, 2]);
        assert_eq!(
            direct_step([i32::MIN, 0], [i32::MAX, 0], false),
            [i32::MIN + 4, 0]
        );
        for x in -12..=12 {
            for y in -12..=12 {
                let next = direct_step([0, 0], [x, y], false);
                assert!(next[0].abs() <= 4 && next[1].abs() <= 2);
                if x != -1 {
                    assert!(next[0].abs() <= x.abs());
                }
                if y != -1 {
                    assert!(next[1].abs() <= y.abs());
                }
            }
        }
    }
    #[test]
    fn facing_bias_path_constraints_and_small_turn_suppression() {
        assert_eq!(
            direction([0, 0], [10, 6], Direction::Left, 0).unwrap(),
            Direction::Forward
        );
        assert_eq!(
            direction([0, 0], [4, 2], Direction::Right, 0).unwrap(),
            Direction::Right
        );
        assert_eq!(
            direction([0, 0], [100, 2], Direction::Left, 3).unwrap(),
            Direction::Forward
        );
        assert_eq!(
            direction([0, 0], [0, 100], Direction::Left, 2).unwrap(),
            Direction::Left
        );
        assert_eq!(
            direction([0, 0], [0, 0], Direction::Away, 0).unwrap(),
            Direction::Away
        );
        assert!(direction([0, 0], [1, 1], Direction::Left, 9).is_err());
    }
}
