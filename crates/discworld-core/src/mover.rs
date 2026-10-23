// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 initial mover placement and standing selection from movers.cpp/polygons.cpp.
use crate::{
    graphics::Handle,
    iso::Error,
    polygon::{Kind, Polygons},
    startup::Declarations,
};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Forward,
    Away,
}
impl Direction {
    pub fn from_talk_constant(value: u32, current: Self) -> Result<Self, Error> {
        match value {
            0 => Ok(current),
            1 => Ok(Self::Away),
            2 => Ok(Self::Forward),
            3 => Ok(Self::Left),
            4 => Ok(Self::Right),
            _ => Err(Error("not a direction constant")),
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Forward => 2,
            Self::Away => 3,
        }
    }
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub actor: i32,
    pub position: [i32; 2],
    pub direction: Direction,
    pub path: Option<usize>,
    pub node: Option<usize>,
    pub scale: usize,
    pub standing: Handle,
    pub base_walk: Handle,
    pub z_factor: i32,
}
impl Placement {
    pub fn initial(
        declarations: &Declarations,
        polygons: &Polygons,
        actor: i32,
        mut position: [i32; 2],
        film: Handle,
    ) -> Result<Self, Error> {
        let actor = if actor == -2 {
            declarations
                .lead
                .as_ref()
                .ok_or(Error("missing lead actor"))?
                .id
        } else {
            actor
        };
        let reels = declarations
            .movers
            .get(&actor)
            .ok_or(Error("actor is not a declared mover"))?;
        let path = polygons.active.iter().position(|p| {
            p.enabled
                && matches!(p.definition.kind, Kind::Path | Kind::NodePath)
                && p.contains(position)
        });
        let mut node = None;
        if let Some(index) = path {
            let p = &polygons.active[index].definition;
            if p.kind == Kind::NodePath {
                let nearest = crate::nodepath::nearest_node(p, position)?;
                position = *p
                    .nodes
                    .get(nearest)
                    .ok_or(Error("node path has no nodes"))?;
                node = Some(nearest);
            }
        }
        let scale_path = path.or_else(|| {
            polygons
                .active
                .iter()
                .position(|p| p.enabled && matches!(p.definition.kind, Kind::Path | Kind::NodePath))
        });
        let (scale, z_factor) = if let Some(index) = scale_path {
            (
                polygons.active[index].scale_at(position[1])?,
                polygons.active[index].definition.z_factor,
            )
        } else {
            (1, 0)
        };
        let direction = if film.0 <= 4 {
            Direction::from_talk_constant(film.0, Direction::Forward)?
        } else {
            Direction::Forward
        };
        let standing = if film.0 <= 4 {
            reels[scale - 1][4 + direction.index()]
        } else {
            film
        };
        let base_walk = reels[0][Direction::Forward.index()];
        if standing.0 == 0 || base_walk.0 == 0 {
            return Err(Error("missing mover reel"));
        }
        Ok(Self {
            actor,
            position,
            direction,
            path,
            node,
            scale,
            standing,
            base_walk,
            z_factor,
        })
    }
}
/// Persistent mover object; standing changes only its animation script.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Mover {
    /// V1 mover visibility, distinct from Actor.hidden. Stored by snapshot v18.
    #[serde(skip)]
    pub(crate) hidden: bool,
    /// Explicit V1 reel mode. None retains legacy bookkeeping until migrated.
    /// Stored separately by the versioned snapshot codec.
    #[serde(skip)]
    pub(crate) reel_state: Option<crate::mover_reel::State>,
    /// V1 hLastFilm: last selected reel film, including walking and talking.
    pub last_film: Handle,
    pub node_travel: Option<crate::nodepath::Travel>,
    pub node_walk: Option<NodeWalk>,
    pub direct_leg: Option<crate::walk::Leg>,
    pub target_node_line: usize,
    pub placement: Placement,
    pub animation: crate::animation::Animation,
    /// V1 walking phase, independent of the current reel's script counter.
    pub step_phase: u8,
    pub walk_serial: u64,
    pub stop_requested: bool,
    pub walk_target: Option<[i32; 2]>,
    pub no_path: bool,
    pub constrained_walk: bool,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct NodeWalk {
    difficulty: u8,
    pub intermediate: [i32; 2],
    pub destination_path: usize,
    pub start: [i32; 2],
    pub cursor: [i32; 2],
    pub destination: crate::nodepath::Projection,
    pub leg: crate::walk::Leg,
}
impl Mover {
    /// Apply a V1 reel operation transactionally. The caller supplies the
    /// initial state when migrating a legacy mover; later calls use its stored
    /// state. Route, walk number and standing-film bookkeeping are preserved.
    pub fn alter_reel(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        initial: crate::mover_reel::State,
        operation: crate::mover_reel::Operation,
    ) -> Result<(), Error> {
        let transition = self
            .reel_state
            .unwrap_or(initial)
            .transition(self.last_film, operation);
        let mut next = self.clone();
        match transition.animation {
            crate::mover_reel::Animation::Keep => {}
            crate::mover_reel::Animation::RestoreWalking => {
                next.select_walk_reel(
                    resources,
                    declarations,
                    next.placement.direction,
                    next.placement.scale,
                    true,
                )?;
            }
            crate::mover_reel::Animation::Install { film, first_step } => {
                next.install_reel(resources, film, first_step)?;
            }
        }
        next.reel_state = Some(transition.state);
        *self = next;
        Ok(())
    }

    pub(crate) fn validate_reel_resources(
        &self,
        resources: &crate::graphics::Resources<'_>,
    ) -> Result<(), Error> {
        if let Some(state) = self.reel_state {
            for handle in [self.last_film, state.pushed()] {
                if handle.0 != 0 {
                    let film = crate::film::Film::read(resources, handle)?;
                    if film.rate > 24 {
                        return Err(Error("invalid mover reel rate"));
                    }
                    resources.resolve(film.reels[0].script, 4)?;
                }
            }
            if state.mode() != crate::mover_reel::Mode::Normal && self.last_film.0 == 0 {
                return Err(Error("explicit mover mode requires a film"));
            }
        }
        Ok(())
    }
    fn accept_routed_steering(
        &mut self,
        walk: &mut NodeWalk,
        steering: crate::nodepath::Steering,
    ) -> crate::nodepath::Steering {
        if let crate::nodepath::Steering::Shorten { position } = steering {
            // V1 SetNextDest uses the probe's reachable position as both the
            // ultimate and immediate destination when another mover blocks it.
            walk.destination.position = position;
            walk.leg.ultimate = position;
            self.walk_target = Some(position);
            crate::nodepath::Steering::Move {
                position,
                overshoot: false,
            }
        } else {
            steering
        }
    }
    fn route_node_leg(
        polygons: &Polygons,
        current: usize,
        position: [i32; 2],
        walk: &mut NodeWalk,
        overshoot: bool,
        footprint: Option<&crate::walk::Footprint>,
        others: &[crate::walk::Footprint],
    ) -> Result<crate::nodepath::Steering, Error> {
        let intermediate = crate::routing::intermediate(
            polygons,
            crate::routing::Request {
                current,
                position,
                ultimate: walk.destination.position,
                overshoot,
            },
            footprint,
            others,
        )?;
        walk.intermediate = intermediate.position;
        Self::probe_route_leg(
            polygons,
            crate::routing::Request {
                current,
                position,
                ultimate: walk.destination.position,
                overshoot,
            },
            intermediate,
            Some(&mut walk.difficulty),
            footprint,
            others,
        )
    }
    /// SetNextDest probes ahead from the actual position before choosing a
    /// waypoint. This is shared by node routes and ordinary obstacle detours.
    fn probe_route_leg(
        polygons: &Polygons,
        request: crate::routing::Request,
        intermediate: crate::routing::Intermediate,
        mut difficulty: Option<&mut u8>,
        footprint: Option<&crate::walk::Footprint>,
        others: &[crate::walk::Footprint],
    ) -> Result<crate::nodepath::Steering, Error> {
        let mut leg = crate::walk::Leg {
            target: intermediate.position,
            ultimate: request.ultimate,
            overshoot: request.overshoot,
            force_minor: false,
        };
        let mut from = request.position;
        let mut previous_restricted = [false; 2];
        for _ in 0..65536 {
            let mut step = crate::walk::constrained_leg(from, leg, polygons, footprint, others)?;
            if step.position == from && step.reached != [true; 2] {
                let retry = crate::walk::constrained_leg(
                    from,
                    crate::walk::Leg {
                        force_minor: true,
                        ..leg
                    },
                    polygons,
                    footprint,
                    others,
                )?;
                if retry.position != from {
                    step = retry;
                }
            }
            if step.reached == [true; 2] && step.restricted == [false; 2] && !step.stuck {
                return Ok(crate::nodepath::Steering::Move {
                    position: step.position,
                    overshoot: false,
                });
            }
            if step.stuck
                || step.restricted == [true; 2]
                || (step.reached == [true, false] && step.restricted == [false, true])
                || (step.reached == [false, true] && step.restricted == [true, false])
            {
                if matches!(
                    step.reason,
                    Some(
                        crate::walk::Restriction::EnteringBlock(_)
                            | crate::walk::Restriction::EnteringMover(_)
                    )
                ) {
                    let corners = match step.reason {
                        Some(crate::walk::Restriction::EnteringBlock(index)) => {
                            polygons.active[index].definition.corners
                        }
                        Some(crate::walk::Restriction::EnteringMover(actor)) => {
                            let footprint = footprint.ok_or(Error("missing moving footprint"))?;
                            if footprint.blocker(request.ultimate, others, false).is_some() {
                                return Ok(crate::nodepath::Steering::Shorten { position: from });
                            }
                            let other = others
                                .iter()
                                .find(|m| m.actor == actor)
                                .ok_or(Error("missing blocking mover"))?;
                            footprint.obstacle(other)
                        }
                        _ => unreachable!(),
                    };
                    // SetNextDest chooses from the actual mover position, not
                    // the simulated probe position where collision was found.
                    return Ok(crate::nodepath::Steering::Move {
                        position: crate::routing::blocking_corner(
                            polygons,
                            corners,
                            request.position,
                            intermediate.position,
                        )?,
                        overshoot: false,
                    });
                }
                let difficulty = difficulty
                    .as_deref_mut()
                    .ok_or(Error("unsupported direct path-edge recovery"))?;
                let target = if *difficulty == 0 {
                    *difficulty = 1;
                    crate::routing::center(&polygons.active[intermediate.path])
                } else {
                    *difficulty = difficulty.saturating_add(1);
                    crate::routing::corner(
                        &polygons.active[request.current],
                        &polygons.active[intermediate.path],
                        from,
                    )?
                };
                return Ok(crate::nodepath::Steering::Move {
                    position: target,
                    overshoot: false,
                });
            }
            if (previous_restricted[0] && !step.restricted[0])
                || (previous_restricted[1] && !step.restricted[1])
            {
                return Ok(crate::nodepath::Steering::Move {
                    position: step.position,
                    overshoot: false,
                });
            }
            if from == step.position {
                return Err(Error("stationary node intermediate probe"));
            }
            previous_restricted = step.restricted;
            from = step.position;
            leg.target = step.target;
        }
        Err(Error("node intermediate probe budget exhausted"))
    }
    fn begin_node_walk(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        polygons: &Polygons,
        target: [i32; 2],
        cursor: [i32; 2],
    ) -> Result<u64, Error> {
        let path = self.placement.path.ok_or(Error("node mover has no path"))?;
        let p = &polygons.active[path];
        let destination_path = polygons.active.iter().position(|p| {
            p.enabled
                && matches!(p.definition.kind, Kind::Path | Kind::NodePath)
                && p.contains(target)
        });
        let destination_path =
            destination_path.ok_or(Error("node walk destination has no path"))?;
        let target_path = &polygons.active[destination_path];
        if self.no_path || polygons.at(target, Kind::Block).is_some() {
            return Err(Error("invalid resolved node destination"));
        }
        let mut next = self.clone();
        next.restore_normal_reel(resources, declarations)?;
        next.stop_requested = false;
        next.walk_serial = next
            .walk_serial
            .checked_add(1)
            .ok_or(Error("walk serial overflow"))?;
        let from = next.placement.position;
        if (i64::from(from[0]) - i64::from(target[0])).abs() < 4
            && (i64::from(from[1]) - i64::from(target[1])).abs() < 2
        {
            let direction =
                crate::walk::direction(from, cursor, next.placement.direction, p.definition.reel)?;
            next.select_walk_reel(
                resources,
                declarations,
                direction,
                next.placement.scale,
                true,
            )?;
            next.finish_walk(resources, declarations)?;
        } else {
            let destination = if target_path.definition.kind == Kind::NodePath {
                crate::nodepath::project(&target_path.definition, target)?
            } else {
                // Ordinary destinations have no target segment. The line is
                // consulted only while following the ultimate node path.
                crate::nodepath::Projection {
                    position: target,
                    line: 0,
                }
            };
            if target_path.definition.kind == Kind::NodePath {
                next.target_node_line = destination.line;
            }
            if !target_path.contains(destination.position) {
                return Err(Error("projected node destination outside path"));
            }
            let mut travel = if p.definition.kind == Kind::NodePath {
                // A node polygon can be crossed without following its nodes
                // (V1 hFnpath == NOPOLY). Retarget that state through the
                // ordinary router instead of requiring a traversal record.
                next.node_travel.map(|mut travel| {
                    travel.overshoot = false;
                    travel
                })
            } else {
                None
            };
            let mut walk = NodeWalk {
                difficulty: 0,
                intermediate: crate::routing::intermediate(
                    polygons,
                    crate::routing::Request {
                        current: path,
                        position: from,
                        ultimate: destination.position,
                        overshoot: false,
                    },
                    None,
                    &[],
                )?
                .position,
                destination_path,
                start: from,
                cursor,
                destination,
                leg: crate::walk::Leg {
                    target: destination.position,
                    ultimate: destination.position,
                    overshoot: false,
                    force_minor: false,
                },
            };
            let steering = if let Some(travel) = &mut travel {
                if destination_path == path {
                    travel.start_within(&p.definition, destination)?
                } else {
                    let end = crate::routing::node_exit(polygons, path, destination_path)?;
                    travel
                        .start_leaving(&p.definition, from, end)?
                        .unwrap_or_else(|| crate::nodepath::Steering::Move {
                            // V1 SetOffWithinNodePath's LEAVING branch leaves
                            // targetX/Y untouched while SetActorDest replaces
                            // the ultimate/intermediate targets and clears over.
                            // An exhausted leg routes again without moving.
                            position: next
                                .node_walk
                                .as_ref()
                                .map(|w| w.leg.target)
                                .or_else(|| next.direct_leg.map(|leg| leg.target))
                                .unwrap_or(from),
                            overshoot: false,
                        })
                }
            } else {
                let footprint = next.footprint(resources)?;
                Self::route_node_leg(
                    polygons,
                    path,
                    from,
                    &mut walk,
                    false,
                    Some(&footprint),
                    &[],
                )?
            };
            let steering = next.accept_routed_steering(&mut walk, steering);
            let crate::nodepath::Steering::Move {
                position,
                overshoot,
            } = steering
            else {
                return Err(Error("unexpected initial node routing request"));
            };
            let direction = crate::walk::direction(
                from,
                position,
                next.placement.direction,
                p.definition.reel,
            )?;
            next.select_walk_reel(
                resources,
                declarations,
                direction,
                p.scale_at(from[1])?,
                true,
            )?;
            next.node_travel = travel;
            next.walk_target = Some(walk.destination.position);
            walk.leg.target = position;
            walk.leg.overshoot = overshoot;
            next.node_walk = Some(walk);
            next.constrained_walk = true;
        }
        *self = next;
        Ok(self.walk_serial)
    }
    fn advance_node_walk(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        polygons: &Polygons,
        others: &[crate::walk::Footprint],
    ) -> Result<(), Error> {
        let mut next = self.clone();
        let mut walk = next.node_walk.clone().ok_or(Error("missing node walk"))?;
        let mut travel = next.node_travel;
        let footprint = next.footprint(resources)?;
        let path = next.placement.path.ok_or(Error("node mover has no path"))?;
        let p = &polygons.active[path];
        let from = next.placement.position;
        // V1 retries a stationary attempt with the minor-axis fallback using
        // the original target, before invoking NotMoving.
        let mut step =
            crate::walk::constrained_leg(from, walk.leg, polygons, Some(&footprint), others)?;
        if step.position == from {
            step = crate::walk::constrained_leg(
                from,
                crate::walk::Leg {
                    force_minor: true,
                    ..walk.leg
                },
                polygons,
                Some(&footprint),
                others,
            )?;
        }
        if step.position == from {
            if (i64::from(from[0]) - i64::from(walk.destination.position[0])).abs() < 4
                && (i64::from(from[1]) - i64::from(walk.destination.position[1])).abs() < 2
            {
                if from == walk.start {
                    let direction = crate::walk::direction(
                        from,
                        walk.cursor,
                        next.placement.direction,
                        p.definition.reel,
                    )?;
                    next.select_walk_reel(
                        resources,
                        declarations,
                        direction,
                        next.placement.scale,
                        false,
                    )?;
                }
                next.finish_walk(resources, declarations)?;
            } else {
                let steering = if let Some(travel) = &mut travel {
                    travel.advance(
                        &p.definition,
                        from,
                        walk.destination.position,
                        (path == walk.destination_path).then_some(walk.destination.line),
                    )?
                } else {
                    crate::nodepath::Steering::Route
                };
                let steering = if steering == crate::nodepath::Steering::Route {
                    let overshoot = travel.map_or(walk.leg.overshoot, |t| t.overshoot);
                    Self::route_node_leg(
                        polygons,
                        path,
                        from,
                        &mut walk,
                        overshoot,
                        Some(&footprint),
                        others,
                    )?
                } else {
                    steering
                };
                let steering = next.accept_routed_steering(&mut walk, steering);
                let crate::nodepath::Steering::Move {
                    position,
                    overshoot,
                } = steering
                else {
                    return Err(Error("unsupported node walk leaving current path"));
                };
                walk.leg.target = position;
                walk.leg.overshoot = overshoot;
                let direction = crate::walk::direction(
                    from,
                    position,
                    next.placement.direction,
                    p.definition.reel,
                )?;
                next.select_walk_reel(
                    resources,
                    declarations,
                    direction,
                    p.scale_at(from[1])?,
                    false,
                )?;
                next.node_travel = travel;
                next.node_walk = Some(walk);
            }
        } else {
            if step.stuck {
                return Err(Error("unsupported node walk collision response"));
            }
            let entered = if p.contains(step.position) {
                path
            } else {
                crate::routing::path_at(polygons, step.position).unwrap_or(path)
            };
            let p = &polygons.active[entered];
            next.select_walk_reel(
                resources,
                declarations,
                next.placement.direction,
                p.scale_at(step.position[1])?,
                false,
            )?;
            next.animate_move(
                resources,
                declarations,
                step.position,
                p.definition.z_factor,
            )?;
            walk.leg.target = step.target;
            if entered != path && p.definition.kind == Kind::Path {
                next.node_travel = None;
                next.placement.node = None;
                next.placement.path = Some(entered);
                if !p.definition.corners.contains(&walk.intermediate) {
                    let footprint = next.footprint(resources)?;
                    let overshoot = walk.leg.overshoot;
                    let steering = Self::route_node_leg(
                        polygons,
                        entered,
                        step.position,
                        &mut walk,
                        overshoot,
                        Some(&footprint),
                        others,
                    )?;
                    let steering = next.accept_routed_steering(&mut walk, steering);
                    let crate::nodepath::Steering::Move {
                        position,
                        overshoot,
                    } = steering
                    else {
                        return Err(Error("unexpected ordinary path routing result"));
                    };
                    walk.leg.target = position;
                    walk.leg.overshoot = overshoot;
                    let direction = crate::walk::direction(
                        step.position,
                        position,
                        next.placement.direction,
                        p.definition.reel,
                    )?;
                    next.select_walk_reel(
                        resources,
                        declarations,
                        direction,
                        p.scale_at(step.position[1])?,
                        false,
                    )?;
                }
                next.node_walk = Some(walk);
                *self = next;
                return Ok(());
            }
            if entered != path {
                let entry = crate::routing::enter_node(
                    polygons,
                    entered,
                    step.position,
                    walk.destination_path,
                    walk.destination,
                )?;
                let mut entered_travel = entry.travel;
                next.placement.path = Some(entered);
                next.placement.node = entered_travel.map(|t| t.line);
                let steering = match entry.action {
                    crate::routing::EntryAction::Bypass => {
                        let footprint = next.footprint(resources)?;
                        let overshoot = walk.leg.overshoot;
                        Self::route_node_leg(
                            polygons,
                            entered,
                            step.position,
                            &mut walk,
                            overshoot,
                            Some(&footprint),
                            others,
                        )?
                    }
                    crate::routing::EntryAction::Move(position) => {
                        crate::nodepath::Steering::Move {
                            position,
                            overshoot: true,
                        }
                    }
                    crate::routing::EntryAction::Next => entered_travel
                        .as_mut()
                        .ok_or(Error("missing entering node state"))?
                        .advance(
                            &p.definition,
                            step.position,
                            walk.destination.position,
                            (entered == walk.destination_path).then_some(walk.destination.line),
                        )?,
                };
                let steering = next.accept_routed_steering(&mut walk, steering);
                let crate::nodepath::Steering::Move {
                    position,
                    overshoot,
                } = steering
                else {
                    return Err(Error("unsupported immediate node exit"));
                };
                walk.leg.target = position;
                walk.leg.overshoot = overshoot;
                let direction = crate::walk::direction(
                    step.position,
                    position,
                    next.placement.direction,
                    p.definition.reel,
                )?;
                next.select_walk_reel(
                    resources,
                    declarations,
                    direction,
                    p.scale_at(step.position[1])?,
                    false,
                )?;
                next.node_travel = entered_travel;
                next.placement.node = entered_travel.map(|t| t.line);
                next.placement.path = Some(entered);
            }
            next.node_walk = Some(walk);
        }
        *self = next;
        Ok(())
    }
    fn restore_normal_reel(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
    ) -> Result<(), Error> {
        if let Some(state) = self.reel_state {
            self.alter_reel(
                resources,
                declarations,
                state,
                crate::mover_reel::Operation::Normal(Handle(0)),
            )?;
        }
        Ok(())
    }
    pub(crate) fn special_reel(&self) -> bool {
        self.reel_state
            .is_some_and(|state| state.suspends_movement())
    }
    /// V1 talk push: direction constants select the current scale's talk bank
    /// without changing standing direction. Moving actors retain their route.
    pub fn push_talk(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        film: Handle,
    ) -> Result<Handle, Error> {
        let selected = if film.0 <= 4 {
            let direction = Direction::from_talk_constant(film.0, self.placement.direction)?;
            declarations
                .movers
                .get(&self.placement.actor)
                .ok_or(Error("missing talk mover declaration"))?[self.placement.scale - 1]
                [8 + direction.index()]
        } else {
            film
        };
        if selected.0 == 0 {
            return Err(Error("missing mover talk reel"));
        }
        let initial = if self.walk_target.is_some() {
            crate::mover_reel::State::default()
        } else {
            crate::mover_reel::State::stationary(Handle(0))
        };
        self.alter_reel(
            resources,
            declarations,
            initial,
            crate::mover_reel::Operation::Push(selected),
        )?;
        Ok(self.reel_state.unwrap().pushed())
    }

    pub fn pop_talk(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        saved: Handle,
    ) -> Result<(), Error> {
        let mut next = self.clone();
        if let Some(state) = next.reel_state {
            // FinishTalkingReel selects standing before POP, but does not
            // finish or clear the interrupted walk.
            next.placement.standing = declarations
                .movers
                .get(&next.placement.actor)
                .ok_or(Error("missing talk mover declaration"))?[next.placement.scale - 1]
                [4 + next.placement.direction.index()];
            next.apply_standing(resources)?;
            next.alter_reel(
                resources,
                declarations,
                state,
                crate::mover_reel::Operation::Pop,
            )?;
            *self = next;
            return Ok(());
        }
        next.finish_walk(resources, declarations)?;
        next.placement.standing = saved;
        next.apply_standing(resources)?;
        *self = next;
        Ok(())
    }
    pub fn new(
        resources: &crate::graphics::Resources<'_>,
        placement: Placement,
    ) -> Result<Self, Error> {
        let base = crate::film::Film::read(resources, placement.base_walk)?;
        let reel = base.reels.first().ok_or(Error("empty mover base film"))?;
        let animation = crate::animation::Animation::new(resources, reel, 24 / base.rate as u32)?;
        let no_path = placement.path.is_none();
        let mut mover = Self {
            hidden: false,
            reel_state: None,
            last_film: Handle(0),
            node_travel: placement.node.map(|line| crate::nodepath::Travel {
                line,
                status: crate::nodepath::TravelStatus::GoingUp,
                overshoot: false,
            }),
            node_walk: None,
            direct_leg: None,
            target_node_line: 0,
            placement,
            animation,
            step_phase: 0,
            walk_serial: 0,
            stop_requested: false,
            walk_target: None,
            no_path,
            constrained_walk: false,
        };
        mover.apply_standing(resources)?;
        Ok(mover)
    }
    pub fn footprint(
        &self,
        resources: &crate::graphics::Resources<'_>,
    ) -> Result<crate::walk::Footprint, Error> {
        let [left, _, right, _] = self
            .animation
            .bounds(resources)?
            .ok_or(Error("mover collision shape is empty"))?;
        Ok(crate::walk::Footprint {
            actor: self.placement.actor,
            position: self.placement.position,
            left,
            right,
            node_path: self.placement.node.is_some(),
        })
    }
    fn finish_walk(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
    ) -> Result<(), Error> {
        self.placement.standing = declarations
            .movers
            .get(&self.placement.actor)
            .ok_or(Error("actor is not a declared mover"))?[self.placement.scale - 1]
            [4 + self.placement.direction.index()];
        self.apply_standing(resources)?;
        self.direct_leg = None;
        self.walk_target = None;
        self.node_walk = None;
        Ok(())
    }
    /// Begin the normal-reel, ignore-static-path walking mode used by SWALK.
    pub fn begin_direct_walk(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        polygons: &Polygons,
        target: [i32; 2],
        cursor: [i32; 2],
    ) -> Result<u64, Error> {
        let mut next = self.clone();
        next.restore_normal_reel(resources, declarations)?;
        let path = next
            .placement
            .path
            .ok_or(Error("walking mover has no current path"))?;
        let p = &polygons.active[path];
        next.stop_requested = false;
        next.walk_serial = next
            .walk_serial
            .checked_add(1)
            .ok_or(Error("walk serial overflow"))?;
        next.walk_target = Some(target);
        next.constrained_walk = false;
        next.direct_leg = None;
        next.node_walk = None;
        let from = next.placement.position;
        if (i64::from(from[0]) - i64::from(target[0])).abs() < 4
            && (i64::from(from[1]) - i64::from(target[1])).abs() < 2
        {
            // V1 SetActorDest calls GotThere before setting off, even when
            // ignoring paths. GotThereWithoutMoving faces the world cursor.
            let facing =
                crate::walk::direction(from, cursor, next.placement.direction, p.definition.reel)?;
            next.select_walk_reel(resources, declarations, facing, next.placement.scale, true)?;
            next.finish_walk(resources, declarations)?;
        } else {
            let facing =
                crate::walk::direction(from, target, next.placement.direction, p.definition.reel)?;
            next.select_walk_reel(resources, declarations, facing, p.scale_at(from[1])?, true)?;
        }
        *self = next;
        Ok(self.walk_serial)
    }
    /// Use a direct leg for clear same/adjacent ordinary paths, otherwise
    /// use the shared waypoint router and its SetNextDest recovery.
    pub fn begin_path_walk(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        polygons: &Polygons,
        target: [i32; 2],
        cursor: [i32; 2],
    ) -> Result<u64, Error> {
        let current = self
            .placement
            .path
            .ok_or(Error("walking mover has no path"))?;
        if polygons.at(target, Kind::Path).is_none()
            && polygons.at(target, Kind::NodePath).is_some()
        {
            return Err(Error("unsupported walking node path destination"));
        }
        let destination = polygons.at(target, Kind::Path).ok_or(Error(
            "unsupported walking destination outside ordinary path",
        ))?;
        if self.no_path || polygons.at(target, Kind::Block).is_some() {
            return Err(Error("unsupported walking destination adjustment"));
        }
        if current != destination && !polygons.active[current].adjacent.contains(&destination) {
            return Err(Error("unsupported walking route waypoints"));
        }
        let mut position = self.placement.position;
        let mut arrived = false;
        for _ in 0..65536 {
            let step = crate::walk::retry_step(position, target, polygons, None, &[]);
            if step.stuck || (step.position == position && position != target) {
                // Even adjacent ordinary paths can need center/corner recovery,
                // including when the direct probe crosses a nearby node path.
                return self.begin_node_walk(resources, declarations, polygons, target, cursor);
            }
            if step.reached == [true; 2] {
                arrived = true;
                break;
            }
            position = step.position;
        }
        if !arrived {
            return Err(Error("walking route probe budget exhausted"));
        }
        let serial = self.begin_direct_walk(resources, declarations, polygons, target, cursor)?;
        self.constrained_walk = true;
        Ok(serial)
    }
    /// SetActorDest's optional explicit walking film is installed before route
    /// selection. Zero retains the normal walk path, including special-reel
    /// restoration. Failed resource or route validation leaves the mover intact.
    pub fn begin_walk_film(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        polygons: &Polygons,
        target: [i32; 2],
        cursor: [i32; 2],
        film: Handle,
    ) -> Result<u64, Error> {
        if film.0 == 0 {
            return self.begin_walk(resources, declarations, polygons, target, cursor);
        }
        let mut next = self.clone();
        next.alter_reel(
            resources,
            declarations,
            crate::mover_reel::State::default(),
            crate::mover_reel::Operation::Walk(film),
        )?;
        let serial = next.begin_walk(resources, declarations, polygons, target, cursor)?;
        *self = next;
        Ok(serial)
    }

    /// Normal WALK's short-distance completion keeps position and faces the
    /// cursor. Callers supply world coordinates, not screen coordinates.
    pub fn begin_walk(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        polygons: &Polygons,
        target: [i32; 2],
        cursor: [i32; 2],
    ) -> Result<u64, Error> {
        // SetNextDest treats bNoPath like an ignore-path leg until MoveActor
        // finds a path again. Keep this a normal walk, so constraints resume
        // immediately after re-entry instead of remaining SWALK-like.
        if self.no_path
            && ((i64::from(self.placement.position[0]) - i64::from(target[0])).abs() >= 4
                || (i64::from(self.placement.position[1]) - i64::from(target[1])).abs() >= 2)
        {
            let serial =
                self.begin_direct_walk(resources, declarations, polygons, target, cursor)?;
            self.constrained_walk = true;
            return Ok(serial);
        }
        if self
            .placement
            .path
            .is_some_and(|i| polygons.active[i].definition.kind == Kind::NodePath)
        {
            return self.begin_node_walk(resources, declarations, polygons, target, cursor);
        }
        if crate::routing::path_at(polygons, target).is_some_and(|destination| {
            polygons.active[destination].definition.kind == Kind::NodePath
                || self.placement.path.is_some_and(|current| {
                    current != destination
                        && !polygons.active[current].adjacent.contains(&destination)
                })
        }) {
            return self.begin_node_walk(resources, declarations, polygons, target, cursor);
        }
        let from = self.placement.position;
        if (i64::from(from[0]) - i64::from(target[0])).abs() >= 4
            || (i64::from(from[1]) - i64::from(target[1])).abs() >= 2
        {
            return self.begin_path_walk(resources, declarations, polygons, target, cursor);
        }
        let path = self
            .placement
            .path
            .ok_or(Error("walking mover has no path"))?;
        // PositionMover can retain bNoPath after a scripted STAND returns
        // to a path. SetActorDest's short-distance GotThere does not reject
        // that flag when the resolved destination itself is valid.
        if polygons.at(target, Kind::Path).is_none() || polygons.at(target, Kind::Block).is_some() {
            return Err(Error("unsupported walking destination adjustment"));
        }
        let p = polygons
            .active
            .get(path)
            .ok_or(Error("invalid mover path"))?;
        if p.definition.kind == Kind::NodePath {
            return Err(Error("unsupported walking node path"));
        }
        let mut next = self.clone();
        next.restore_normal_reel(resources, declarations)?;
        let direction =
            crate::walk::direction(from, cursor, next.placement.direction, p.definition.reel)?;
        next.stop_requested = false;
        next.walk_serial = next
            .walk_serial
            .checked_add(1)
            .ok_or(Error("walk serial overflow"))?;
        next.select_walk_reel(
            resources,
            declarations,
            direction,
            next.placement.scale,
            true,
        )?;
        next.finish_walk(resources, declarations)?;
        next.constrained_walk = true;
        *self = next;
        Ok(self.walk_serial)
    }

    pub fn adopt_tag_pose(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        film: Handle,
    ) -> Result<(), Error> {
        if film.0 == 0 {
            return Ok(());
        }
        let mut next = self.clone();
        if film.0 <= 4 {
            next.placement.direction =
                Direction::from_talk_constant(film.0, next.placement.direction)?;
            next.finish_walk(resources, declarations)?;
        } else if declarations
            .lead
            .as_ref()
            .is_some_and(|lead| lead.id == next.placement.actor)
        {
            next.placement.standing = film;
            next.apply_standing(resources)?;
        } else {
            next.finish_walk(resources, declarations)?;
        }
        *self = next;
        Ok(())
    }
    pub fn advance_direct_walk(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        polygons: &Polygons,
        others: &[crate::walk::Footprint],
    ) -> Result<(), Error> {
        // V1 MoveActor returns before processing bStop when both active axes
        // are disabled, even though bMoving and the script wait remain set.
        if !self.constrained_walk
            && self.node_walk.is_none()
            && self
                .walk_target
                .is_some_and(|target| self.direct_leg.map_or(target, |leg| leg.target) == [-1; 2])
        {
            return Ok(());
        }
        if self.walk_target.is_some() && self.stop_requested {
            let mut next = self.clone();
            next.finish_walk(resources, declarations)?;
            next.stop_requested = false;
            next.walk_serial = next
                .walk_serial
                .checked_add(1)
                .ok_or(Error("walk serial overflow"))?;
            *self = next;
            return Ok(());
        }
        if self.node_walk.is_some() {
            return self.advance_node_walk(resources, declarations, polygons, others);
        }
        let Some(target) = self.walk_target else {
            return Ok(());
        };
        let mut next = self.clone();
        let from = next.placement.position;
        if from == target {
            next.finish_walk(resources, declarations)?;
        } else {
            let position = if next.constrained_walk && !next.no_path {
                let footprint = next.footprint(resources)?;
                let leg_target = next.direct_leg.map_or(target, |leg| leg.target);
                if from == leg_target && leg_target != target {
                    let path = next
                        .placement
                        .path
                        .ok_or(Error("walking mover has no path"))?;
                    let request = crate::routing::Request {
                        current: path,
                        position: from,
                        ultimate: target,
                        overshoot: false,
                    };
                    let intermediate =
                        crate::routing::intermediate(polygons, request, Some(&footprint), others)?;
                    let steering = Self::probe_route_leg(
                        polygons,
                        request,
                        intermediate,
                        None,
                        Some(&footprint),
                        others,
                    )?;
                    let (destination, overshoot, ultimate) = match steering {
                        crate::nodepath::Steering::Move {
                            position,
                            overshoot,
                        } => (position, overshoot, target),
                        crate::nodepath::Steering::Shorten { position } => {
                            next.walk_target = Some(position);
                            (position, false, position)
                        }
                        _ => return Err(Error("unexpected ordinary detour steering")),
                    };
                    next.direct_leg = Some(crate::walk::Leg {
                        target: destination,
                        ultimate,
                        overshoot,
                        force_minor: false,
                    });
                    let direction = crate::walk::direction(
                        from,
                        destination,
                        next.placement.direction,
                        polygons.active[path].definition.reel,
                    )?;
                    next.select_walk_reel(
                        resources,
                        declarations,
                        direction,
                        next.placement.scale,
                        false,
                    )?;
                    *self = next;
                    return Ok(());
                }
                let step =
                    crate::walk::retry_step(from, leg_target, polygons, Some(&footprint), others);
                if step.stuck {
                    let corners = match step.reason {
                        Some(crate::walk::Restriction::EnteringMover(actor)) => {
                            // V1 SetNextDest accepts the closest reachable
                            // position when the ultimate target is occupied.
                            // Do this before choosing an obstacle corner: a
                            // detour cannot make that destination reachable.
                            if footprint.blocker(target, others, false).is_some() {
                                next.finish_walk(resources, declarations)?;
                                *self = next;
                                return Ok(());
                            }
                            let other = others
                                .iter()
                                .find(|m| m.actor == actor)
                                .ok_or(Error("missing blocking mover"))?;
                            footprint.obstacle(other)
                        }
                        Some(crate::walk::Restriction::EnteringBlock(index)) => {
                            polygons.active[index].definition.corners
                        }
                        _ => return Err(Error("unsupported path-walk collision response")),
                    };
                    let corner = crate::routing::blocking_corner(polygons, corners, from, target)?;
                    if corner == from {
                        return Err(Error("no reachable obstacle corner"));
                    }
                    next.direct_leg = Some(crate::walk::Leg {
                        target: corner,
                        ultimate: target,
                        overshoot: false,
                        force_minor: false,
                    });
                    let path = next
                        .placement
                        .path
                        .ok_or(Error("walking mover has no path"))?;
                    let direction = crate::walk::direction(
                        from,
                        corner,
                        next.placement.direction,
                        polygons.active[path].definition.reel,
                    )?;
                    next.select_walk_reel(
                        resources,
                        declarations,
                        direction,
                        next.placement.scale,
                        false,
                    )?;
                    *self = next;
                    return Ok(());
                }
                step.position
            } else {
                let leg = next.direct_leg.unwrap_or(crate::walk::Leg {
                    target,
                    ultimate: target,
                    overshoot: false,
                    force_minor: false,
                });
                let mut step = leg.attempt(from)?;
                if step.position == from {
                    step = crate::walk::Leg {
                        force_minor: true,
                        ..leg
                    }
                    .attempt(from)?;
                }
                if step.position == from {
                    // NotMoving -> SetNextDest: ignore-path movement resumes
                    // the ultimate target before consulting node traversal.
                    next.direct_leg = None;
                    let path = next
                        .placement
                        .path
                        .ok_or(Error("direct mover has no path"))?;
                    if (i64::from(from[0]) - i64::from(target[0])).abs() < 4
                        && (i64::from(from[1]) - i64::from(target[1])).abs() < 2
                    {
                        next.finish_walk(resources, declarations)?;
                    } else {
                        let direction = crate::walk::direction(
                            from,
                            target,
                            next.placement.direction,
                            polygons.active[path].definition.reel,
                        )?;
                        next.select_walk_reel(
                            resources,
                            declarations,
                            direction,
                            next.placement.scale,
                            false,
                        )?;
                    }
                    *self = next;
                    return Ok(());
                }
                next.direct_leg = Some(crate::walk::Leg {
                    target: step.target,
                    ..leg
                });
                step.position
            };
            let current = next
                .placement
                .path
                .ok_or(Error("walking mover has no current path"))?;
            let path = if polygons.active[current].contains(position) {
                current
            } else {
                polygons
                    .active
                    .iter()
                    .position(|p| {
                        p.enabled
                            && matches!(p.definition.kind, Kind::Path | Kind::NodePath)
                            && p.contains(position)
                    })
                    .unwrap_or(current)
            };
            let p = &polygons.active[path];
            if next.no_path && !p.contains(position) {
                next.animate_move(resources, declarations, position, p.definition.z_factor)?;
                *self = next;
                return Ok(());
            }
            let entered_path = next.no_path || path != current;
            next.no_path = false;
            if path != current && p.definition.kind == Kind::Path {
                next.node_travel = None;
                next.placement.node = None;
            }
            next.select_walk_reel(
                resources,
                declarations,
                next.placement.direction,
                p.scale_at(position[1])?,
                false,
            )?;
            let footprint = next.footprint(resources)?;
            if let Some(other) = footprint.blocker(position, others, false) {
                if footprint.blocker(target, others, false).is_some() {
                    next.finish_walk(resources, declarations)?;
                    *self = next;
                    return Ok(());
                }
                if footprint.blocker(from, others, false).is_none() {
                    let corner = crate::routing::blocking_corner(
                        polygons,
                        footprint.obstacle(other),
                        from,
                        target,
                    )?;
                    next.direct_leg = Some(crate::walk::Leg {
                        target: corner,
                        ultimate: target,
                        overshoot: next.direct_leg.is_some_and(|leg| leg.overshoot),
                        force_minor: false,
                    });
                    let current_path = &polygons.active[current];
                    let direction = crate::walk::direction(
                        from,
                        corner,
                        next.placement.direction,
                        current_path.definition.reel,
                    )?;
                    next.select_walk_reel(
                        resources,
                        declarations,
                        direction,
                        current_path.scale_at(from[1])?,
                        false,
                    )?;
                    *self = next;
                    return Ok(());
                }
            }
            next.animate_move(resources, declarations, position, p.definition.z_factor)?;
            next.placement.path = Some(path);
            if next.constrained_walk
                && (p.definition.kind == Kind::NodePath
                    || (entered_path && !p.definition.corners.contains(&target)))
            {
                // MoveActor treats bNoPath re-entry as EnteringNewPath even
                // when hCpath still names this polygon. Resume graph routing
                // for ordinary paths too, rather than walking straight into
                // a dead end. Keep the walk serial and animation phase.
                let destination_path = crate::routing::path_at(polygons, target)
                    .ok_or(Error("node re-entry destination has no path"))?;
                let destination_definition = &polygons.active[destination_path].definition;
                let destination = if destination_definition.kind == Kind::NodePath {
                    crate::nodepath::project(destination_definition, target)?
                } else {
                    crate::nodepath::Projection {
                        position: target,
                        line: 0,
                    }
                };
                let mut walk = NodeWalk {
                    difficulty: 0,
                    intermediate: destination.position,
                    destination_path,
                    start: from,
                    cursor: target,
                    destination,
                    leg: crate::walk::Leg {
                        target: destination.position,
                        ultimate: destination.position,
                        overshoot: false,
                        force_minor: false,
                    },
                };
                let entry = if p.definition.kind == Kind::NodePath {
                    crate::routing::enter_node(
                        polygons,
                        path,
                        position,
                        destination_path,
                        destination,
                    )?
                } else {
                    crate::routing::NodeEntry {
                        travel: None,
                        action: crate::routing::EntryAction::Bypass,
                    }
                };
                let mut travel = entry.travel;
                next.node_travel = travel;
                next.placement.node = travel.map(|t| t.line);
                let steering = match entry.action {
                    crate::routing::EntryAction::Bypass => {
                        let footprint = next.footprint(resources)?;
                        Self::route_node_leg(
                            polygons,
                            path,
                            position,
                            &mut walk,
                            false,
                            Some(&footprint),
                            others,
                        )?
                    }
                    crate::routing::EntryAction::Move(position) => {
                        crate::nodepath::Steering::Move {
                            position,
                            overshoot: true,
                        }
                    }
                    crate::routing::EntryAction::Next => travel
                        .as_mut()
                        .ok_or(Error("missing re-entering node state"))?
                        .advance(
                            &p.definition,
                            position,
                            destination.position,
                            (path == destination_path).then_some(destination.line),
                        )?,
                };
                let steering = next.accept_routed_steering(&mut walk, steering);
                let crate::nodepath::Steering::Move {
                    position: leg_target,
                    overshoot,
                } = steering
                else {
                    return Err(Error("unsupported immediate node re-entry exit"));
                };
                walk.leg.target = leg_target;
                walk.leg.overshoot = overshoot;
                let direction = crate::walk::direction(
                    position,
                    leg_target,
                    next.placement.direction,
                    p.definition.reel,
                )?;
                next.select_walk_reel(
                    resources,
                    declarations,
                    direction,
                    p.scale_at(position[1])?,
                    false,
                )?;
                next.node_travel = travel;
                next.placement.node = travel.map(|t| t.line);
                next.target_node_line = destination.line;
                next.walk_target = Some(walk.destination.position);
                next.node_walk = Some(walk);
                next.direct_leg = None;
                *self = next;
                return Ok(());
            }
            if path != current && p.definition.kind == Kind::NodePath {
                let entry = crate::routing::enter_node_direct(
                    polygons,
                    path,
                    position,
                    crate::routing::path_at(polygons, target),
                    crate::nodepath::Projection {
                        position: target,
                        line: next.target_node_line,
                    },
                )?;
                next.node_travel = entry.travel;
                next.placement.node = entry.travel.map(|travel| travel.line);
                let (leg_target, overshoot) = match entry.action {
                    crate::routing::EntryAction::Move(point) => (point, true),
                    crate::routing::EntryAction::Bypass | crate::routing::EntryAction::Next => {
                        (target, false)
                    }
                };
                next.direct_leg = Some(crate::walk::Leg {
                    target: leg_target,
                    ultimate: target,
                    overshoot,
                    force_minor: false,
                });
                let direction = crate::walk::direction(
                    position,
                    leg_target,
                    next.placement.direction,
                    p.definition.reel,
                )?;
                next.select_walk_reel(
                    resources,
                    declarations,
                    direction,
                    next.placement.scale,
                    false,
                )?;
            }
        }
        *self = next;
        Ok(())
    }
    /// Select a normal walking reel (including an available scale transition).
    /// Actor PLAY ownership and explicit special walking reels are handled by
    /// the caller. This changes the script, not the persistent mover object.
    pub fn select_walk_reel(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        direction: Direction,
        scale: usize,
        force: bool,
    ) -> Result<(), Error> {
        if self
            .reel_state
            .is_some_and(|state| state.suppresses_walk_selection())
        {
            if force || self.placement.scale != scale || self.placement.direction != direction {
                self.hidden = false;
            }
            return Ok(());
        }
        if !(1..=10).contains(&scale) || self.step_phase >= 6 {
            return Err(Error("invalid mover walking scale or phase"));
        }
        if !force && self.placement.scale == scale && self.placement.direction == direction {
            return Ok(());
        }
        let old_scale = self.placement.scale;
        let transition = if old_scale != scale && old_scale <= 5 && scale <= 5 {
            declarations
                .scaling
                .iter()
                .find(|entry| {
                    entry.actor == self.placement.actor
                        && entry.scale == old_scale as i32
                        && entry.direction == i32::from(old_scale > scale)
                })
                .map(|entry| entry.reels[direction.index()])
                .filter(|h| h.0 != 0)
        } else {
            None
        };
        let handle = match transition {
            Some(handle) => handle,
            None => declarations
                .movers
                .get(&self.placement.actor)
                .ok_or(Error("actor is not a declared mover"))?[scale - 1][direction.index()],
        };
        if handle.0 == 0 {
            return Err(Error("missing walking reel"));
        }
        let film = crate::film::Film::read(resources, handle)?;
        let reel = film.reels.first().ok_or(Error("empty walking film"))?;
        let mut next = self.clone();
        next.animation.set_script(reel.script, 1)?;
        next.animation
            .skip_frames(resources, i32::from(next.step_phase))?;
        next.placement.scale = scale;
        next.placement.direction = direction;
        next.last_film = handle;
        next.hidden = false;
        *self = next;
        Ok(())
    }

    /// Animate one accepted displacement. Collision/path decisions precede this
    /// operation; an ended scale-transition reel reverts to normal walking.
    pub fn animate_move(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        position: [i32; 2],
        z_factor: i32,
    ) -> Result<(), Error> {
        let mut next = self.clone();
        next.placement.position = position;
        next.placement.z_factor = z_factor;
        next.animation.object.x = position[0];
        next.animation.object.y = position[1];
        let factor = i64::from(z_factor) - i64::from(z_factor != 0);
        next.animation.object.z = i32::try_from(i64::from(position[1]) + (factor << 10))
            .map_err(|_| Error("mover depth overflow"))?;
        if next.animation.tick(resources)? == crate::animation::Tick::Finished {
            next.reel_state = next.reel_state.map(|state| state.walking_film_finished());
            next.step_phase = 0;
            next.select_walk_reel(
                resources,
                declarations,
                next.placement.direction,
                next.placement.scale,
                true,
            )?;
        }
        next.step_phase = (next.step_phase + 1) % 6;
        *self = next;
        Ok(())
    }
    fn apply_standing(&mut self, resources: &crate::graphics::Resources<'_>) -> Result<(), Error> {
        self.install_reel(resources, self.placement.standing, true)?;
        if let Some(state) = self.reel_state {
            self.reel_state = Some(
                state
                    .transition(
                        self.last_film,
                        crate::mover_reel::Operation::Normal(self.placement.standing),
                    )
                    .state,
            );
        }
        Ok(())
    }
    fn install_reel(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        handle: Handle,
        first_step: bool,
    ) -> Result<(), Error> {
        let film = crate::film::Film::read(resources, handle)?;
        let reel = film.reels.first().ok_or(Error("empty standing film"))?;
        resources.resolve(reel.script, 4)?;
        self.animation.object.x = self.placement.position[0];
        self.animation.object.y = self.placement.position[1];
        let factor = i64::from(self.placement.z_factor) - i64::from(self.placement.z_factor != 0);
        self.animation.object.z =
            i32::try_from(i64::from(self.placement.position[1]) + (factor << 10))
                .map_err(|_| Error("mover depth overflow"))?;
        self.animation
            .set_script(reel.script, 24 / film.rate as u32)?;
        self.step_phase = 0;
        if first_step {
            self.animation.tick(resources)?;
        }
        self.last_film = handle;
        Ok(())
    }
    /// V1 restoreMovement/SSetActorDest after SPLAY. The caller supplies the
    /// actor's stored film coordinates (which may belong to a replacement).
    /// This does not acquire a script walk token. A restarted route advances
    /// the mover serial through SetActorDest's normal path.
    pub fn restore_after_splay(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        polygons: &Polygons,
        position: [i32; 2],
        cursor: [i32; 2],
    ) -> Result<(), Error> {
        if self.placement.position == position {
            return Ok(());
        }
        let mut next = self.clone();
        let target = next.walk_target;
        let constrained = next.constrained_walk;
        // restoreMovement writes objX/Y before calling STAND. Its animation
        // position is updated only if STAND accepts both coordinates.
        next.placement.position = position;
        if position[0] != -1 && position[1] != -1 {
            // Preserve the standing special-reel state even for a legacy mover:
            // a disabled-axis target remains suspended rather than restarting.
            next.reel_state.get_or_insert_default();
        }
        next.stand(resources, declarations, polygons, position, Handle(0))?;
        if let Some(target) = target.filter(|target| !target.contains(&-1)) {
            if constrained {
                next.begin_walk(resources, declarations, polygons, target, cursor)?;
            } else {
                next.begin_direct_walk(resources, declarations, polygons, target, cursor)?;
            }
        } else {
            next.walk_target = target;
        }
        *self = next;
        Ok(())
    }

    pub fn stand(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        declarations: &Declarations,
        polygons: &Polygons,
        position: [i32; 2],
        film: Handle,
    ) -> Result<(), Error> {
        if film.0 == 0 && (position[0] == -1 || position[1] == -1) {
            return Ok(());
        }
        let mut next = self.clone();
        let position = if position[0] == -1 || position[1] == -1 {
            self.placement.position
        } else {
            position
        };
        let direction_constant = match self.placement.direction {
            Direction::Left => 3,
            Direction::Right => 4,
            Direction::Forward => 2,
            Direction::Away => 1,
        };
        let effective_film = if film.0 == 0 {
            Handle(direction_constant)
        } else {
            film
        };
        next.placement = Placement::initial(
            declarations,
            polygons,
            self.placement.actor,
            position,
            effective_film,
        )?;
        if film.0 > 4 {
            next.placement.direction = self.placement.direction;
        }
        if next.placement.path.is_none() {
            next.no_path = true;
            next.placement.path = self.placement.path;
            next.placement.z_factor = self.placement.z_factor;
            next.placement.scale = self.placement.scale;
            if film.0 <= 4 {
                next.placement.standing = declarations.movers[&self.placement.actor]
                    [next.placement.scale - 1][4 + next.placement.direction.index()];
            }
        }
        next.node_travel = next.placement.node.map(|line| crate::nodepath::Travel {
            line,
            status: crate::nodepath::TravelStatus::GoingUp,
            overshoot: false,
        });
        next.node_walk = None;
        next.walk_target = None;
        next.apply_standing(resources)?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::Lead;
    #[test]
    fn routed_leg_handles_minor_axis_retry_and_restriction_release() {
        let polygons = Polygons::new(
            vec![crate::polygon::Polygon {
                kind: Kind::Path,
                corners: [[60, 194], [60, 177], [196, 177], [300, 194]],
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
            }],
            0,
        );
        let target = [260, 188];
        let mut walk = NodeWalk {
            difficulty: 0,
            intermediate: target,
            destination_path: 0,
            start: [200, 178],
            cursor: target,
            destination: crate::nodepath::Projection {
                position: target,
                line: 0,
            },
            leg: crate::walk::Leg {
                target,
                ultimate: target,
                overshoot: false,
                force_minor: false,
            },
        };
        let mut position = walk.start;
        for _ in 0..100 {
            if position == target {
                break;
            }
            let steering =
                Mover::route_node_leg(&polygons, 0, position, &mut walk, false, None, &[]).unwrap();
            let crate::nodepath::Steering::Move {
                position: next,
                overshoot,
            } = steering
            else {
                panic!("unexpected routing request")
            };
            assert!(!overshoot);
            assert_ne!(next, position);
            assert!(polygons.active[0].contains(next));
            position = next;
        }
        assert_eq!(position, target);

        // Probe ahead into an obstacle. Corner selection must use the actual
        // starting point; the corner must not be selected from the probe endpoint.
        let mut floor = polygons.active[0].definition.clone();
        floor.corners = [[0, 0], [300, 0], [300, 200], [0, 200]];
        let mut block = floor.clone();
        block.kind = Kind::Block;
        block.corners = [[128, 97], [172, 97], [172, 103], [128, 103]];
        let blocked = Polygons::new(vec![floor.clone(), block], 0);
        walk.destination.position = [250, 100];
        walk.start = [50, 100];
        assert_eq!(
            Mover::route_node_leg(&blocked, 0, walk.start, &mut walk, false, None, &[]).unwrap(),
            crate::nodepath::Steering::Move {
                position: [124, 93],
                overshoot: false
            }
        );
        let footprint = crate::walk::Footprint {
            actor: 8,
            position: walk.start,
            left: 40,
            right: 60,
            node_path: false,
        };
        let other = crate::walk::Footprint {
            actor: 7,
            position: [150, 100],
            left: 140,
            right: 160,
            node_path: false,
        };
        let open = Polygons::new(vec![floor], 0);
        assert_eq!(
            Mover::route_node_leg(
                &open,
                0,
                walk.start,
                &mut walk,
                false,
                Some(&footprint),
                &[other]
            )
            .unwrap(),
            crate::nodepath::Steering::Move {
                position: [124, 93],
                overshoot: false
            }
        );
        walk.destination.position = other.position;
        assert_eq!(
            Mover::route_node_leg(
                &open,
                0,
                walk.start,
                &mut walk,
                false,
                Some(&footprint),
                &[other]
            )
            .unwrap(),
            crate::nodepath::Steering::Shorten {
                position: [130, 100]
            }
        );
    }

    #[test]
    fn walk_reels_keep_object_phase_and_finish_scale_transitions() {
        let mut data = vec![0; 400];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, 32),
            (12, 256),
            (32, 200),
            (40, 7),
            (64, 12),
            (68, 1),
            (72, 32),
            (76, 256),
            (128, 12),
            (132, 1),
            (136, 32),
            (140, 320),
            (200, 220),
            (220, 8 | (16 << 16)),
            (256, 200),
            (260, 1),
            (264, (-2i32) as u32),
            (320, 200),
        ] {
            data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = crate::graphics::Resources(vec![&data]);
        let mut declarations = Declarations::default();
        declarations.movers.insert(7, [[Handle(64); 12]; 10]);
        declarations.scaling.push(crate::startup::ScalingReels {
            actor: 7,
            scale: 1,
            direction: 0,
            reels: [Handle(128); 4],
        });
        let mut mover = Mover::new(
            &resources,
            Placement {
                actor: 7,
                position: [248, 178],
                direction: Direction::Left,
                path: None,
                node: None,
                scale: 1,
                standing: Handle(64),
                base_walk: Handle(0),
                z_factor: 2,
            },
        )
        .unwrap();
        // Applying explicit reel state must not discard an interrupted route.
        let mut talking = mover.clone();
        talking.walk_target = Some([200, 178]);
        talking.walk_serial = 17;
        talking.step_phase = 4;
        let initial = crate::mover_reel::State::default();
        talking
            .alter_reel(
                &resources,
                &declarations,
                initial,
                crate::mover_reel::Operation::Push(Handle(128)),
            )
            .unwrap();
        assert_eq!(talking.walk_target, Some([200, 178]));
        assert_eq!(talking.walk_serial, 17);
        assert_eq!(talking.placement, mover.placement);
        assert_eq!(talking.animation.object.id, mover.animation.object.id);
        assert_eq!(talking.animation.capacity, mover.animation.capacity);
        assert_eq!(talking.animation.steps, 1);
        assert_eq!(talking.step_phase, 0);
        assert_eq!(talking.reel_state.unwrap().pushed(), Handle(0));
        let before_invalid = talking.clone();
        assert!(
            talking
                .alter_reel(
                    &resources,
                    &declarations,
                    initial,
                    crate::mover_reel::Operation::Push(Handle(399))
                )
                .is_err()
        );
        assert_eq!(
            talking, before_invalid,
            "bad film must not change pushed slot or mover"
        );
        talking
            .alter_reel(
                &resources,
                &declarations,
                initial,
                crate::mover_reel::Operation::Normal(Handle(64)),
            )
            .unwrap();
        talking
            .alter_reel(
                &resources,
                &declarations,
                initial,
                crate::mover_reel::Operation::Pop,
            )
            .unwrap();
        assert_eq!(
            talking.reel_state.unwrap().mode(),
            crate::mover_reel::Mode::Normal
        );
        assert_eq!(talking.walk_target, Some([200, 178]));
        assert_eq!(talking.walk_serial, 17);
        assert_eq!(talking.animation.steps, 0);
        talking
            .alter_reel(
                &resources,
                &declarations,
                initial,
                crate::mover_reel::Operation::Walk(Handle(128)),
            )
            .unwrap();
        assert_eq!(
            talking.animation.steps, 0,
            "custom walking must not step at installation"
        );
        assert_eq!(talking.last_film, Handle(128));
        assert_eq!(talking.step_phase, 0);
        assert_eq!(talking.walk_target, Some([200, 178]));
        // SetMoverWalkReel must not override an explicit walking film, even
        // for a forced direction/scale selection. Its eventual end, however,
        // clears custom mode and restores the ordinary reel at phase zero.
        let custom = talking.clone();
        talking
            .select_walk_reel(&resources, &declarations, Direction::Right, 2, true)
            .unwrap();
        assert_eq!(talking, custom);
        for (position, phase) in [([244, 178], 1), ([240, 178], 2), ([236, 178], 1)] {
            talking
                .animate_move(&resources, &declarations, position, 2)
                .unwrap();
            assert_eq!(talking.step_phase, phase);
            assert_eq!(talking.walk_target, Some([200, 178]));
            assert_eq!(talking.walk_serial, 17);
        }
        assert_eq!(
            talking.reel_state.unwrap().mode(),
            crate::mover_reel::Mode::Normal
        );
        assert_eq!(talking.last_film, Handle(64));
        assert_eq!(talking.placement.direction, Direction::Left);
        assert_eq!(talking.placement.scale, 1);
        assert_eq!(talking.animation.steps, 0);
        mover
            .select_walk_reel(&resources, &declarations, Direction::Left, 1, true)
            .unwrap();
        for step in 1..=12 {
            mover
                .animate_move(&resources, &declarations, [248 - step * 4, 178], 2)
                .unwrap();
            assert_eq!(mover.step_phase, (step % 6) as u8);
            assert_eq!(mover.animation.steps, step as u32);
        }
        assert_eq!(mover.placement.position, [200, 178]);
        assert_eq!(mover.animation.object.id, 7);
        assert_eq!(mover.animation.object.z, 1202);
        mover
            .select_walk_reel(&resources, &declarations, Direction::Right, 2, false)
            .unwrap();
        assert_eq!(mover.last_film, Handle(128));
        mover
            .animate_move(&resources, &declarations, [204, 178], 2)
            .unwrap();
        mover
            .animate_move(&resources, &declarations, [208, 178], 2)
            .unwrap();
        assert_eq!(
            mover.step_phase, 1,
            "ended transition resets phase before increment"
        );
        assert_eq!(
            mover.animation.steps, 0,
            "normal reel is initialized but not stepped"
        );
        mover
            .animate_move(&resources, &declarations, [212, 178], 2)
            .unwrap();
        assert_eq!(mover.step_phase, 2);
        assert_eq!(mover.animation.steps, 1);
        let before = mover.clone();
        mover
            .select_walk_reel(&resources, &declarations, Direction::Right, 2, false)
            .unwrap();
        assert_eq!(
            mover, before,
            "unchanged direction/scale leaves animation alone"
        );
        assert!(
            mover
                .select_walk_reel(&resources, &declarations, Direction::Left, 11, true)
                .is_err()
        );
        assert_eq!(mover, before);
        assert!(
            mover
                .animate_move(&resources, &declarations, [0, i32::MAX], i32::MAX)
                .is_err()
        );
        assert_eq!(mover, before);
        let polygons = Polygons::new(
            vec![crate::polygon::Polygon {
                kind: Kind::Path,
                corners: [[0, 0], [220, 0], [220, 200], [0, 200]],
                tag_position: [0; 2],
                tag_text: Handle(0),
                node_position: [0; 2],
                film: Handle(0),
                refer_type: 0,
                id: 1,
                scales: [2, 0],
                reel: 0,
                z_factor: 2,
                nodes: vec![],
                lines: vec![],
                script: Handle(0),
            }],
            0,
        );
        // Raw -1 destinations are retained: arrival on the enabled axis
        // need not satisfy V1's raw-coordinate NotMoving completion check.
        for (start, target, positions, completes) in [
            ([100, 50], [-1, 54], [[100, 52], [100, 54]], false),
            ([0, 50], [-1, 54], [[0, 52], [0, 54]], true),
            ([100, 50], [108, -1], [[104, 50], [108, 50]], false),
            ([100, 0], [108, -1], [[104, 0], [108, 0]], true),
            ([100, 50], [-1, -1], [[100, 50], [100, 50]], false),
            ([0, 0], [-1, -1], [[0, 0], [0, 0]], true),
        ] {
            let mut axis = mover.clone();
            axis.stand(&resources, &declarations, &polygons, start, Handle(0))
                .unwrap();
            let serial = axis
                .begin_direct_walk(&resources, &declarations, &polygons, target, start)
                .unwrap();
            for tick in 0..8 {
                let bytes = postcard::to_allocvec(&axis).unwrap();
                let mut restored: Mover = postcard::from_bytes(&bytes).unwrap();
                for m in [&mut axis, &mut restored] {
                    m.advance_direct_walk(&resources, &declarations, &polygons, &[])
                        .unwrap();
                    assert_eq!(m.placement.position, positions[tick.min(1)]);
                    if tick >= 2 {
                        assert_eq!(m.walk_target.is_none(), completes);
                    }
                    assert_eq!(m.walk_serial, serial);
                }
                assert_eq!(axis, restored);
            }
            if !completes {
                axis.stop_requested = true;
                axis.advance_direct_walk(&resources, &declarations, &polygons, &[])
                    .unwrap();
                if target == [-1; 2] {
                    assert!(axis.stop_requested);
                    assert_eq!(axis.walk_target, Some(target));
                    assert_eq!(axis.walk_serial, serial);
                } else {
                    assert!(!axis.stop_requested);
                    assert_eq!(axis.walk_target, None);
                    assert_eq!(axis.walk_serial, serial + 1);
                }
            }
        }
        mover
            .stand(&resources, &declarations, &polygons, [200, 178], Handle(0))
            .unwrap();
        mover
            .stand(&resources, &declarations, &polygons, [248, 178], Handle(0))
            .unwrap();
        assert_eq!(mover.placement.path, Some(0));
        assert!(mover.no_path);
        // Holretro places the actor just outside a path, then stands him
        // inside it and issues WALK to that same position. PositionMover
        // retains bNoPath; SetActorDest must still take its GotThere branch.
        let psychiatrist = Polygons::new(
            [
                [[0, 163], [0, 150], [95, 150], [95, 163]],
                [[95, 199], [95, 150], [319, 150], [319, 199]],
            ]
            .into_iter()
            .map(|corners| crate::polygon::Polygon {
                corners,
                ..polygons.active[0].definition.clone()
            })
            .collect(),
            0,
        );
        let mut seated = mover.clone();
        seated
            .stand(
                &resources,
                &declarations,
                &psychiatrist,
                [90, 164],
                Handle(4),
            )
            .unwrap();
        seated
            .stand(
                &resources,
                &declarations,
                &psychiatrist,
                [109, 164],
                Handle(4),
            )
            .unwrap();
        assert!(seated.no_path);
        let mut resumed = seated.clone();
        for m in [&mut seated, &mut resumed] {
            m.begin_walk(
                &resources,
                &declarations,
                &psychiatrist,
                [109, 164],
                [120, 164],
            )
            .unwrap();
            assert_eq!(m.placement.position, [109, 164]);
            assert_eq!(m.walk_target, None);
        }
        assert_eq!(seated, resumed);
        let mut direct = mover.clone();
        direct
            .begin_direct_walk(&resources, &declarations, &polygons, [200, 178], [200, 178])
            .unwrap();
        let serial = mover
            .begin_walk(&resources, &declarations, &polygons, [200, 178], [200, 178])
            .unwrap();
        assert!(mover.constrained_walk);
        let mut restored = mover.clone();
        for step in 1..=12 {
            direct
                .advance_direct_walk(&resources, &declarations, &polygons, &[])
                .unwrap();
            for m in [&mut mover, &mut restored] {
                m.advance_direct_walk(&resources, &declarations, &polygons, &[])
                    .unwrap();
                assert_eq!(m.placement.position, [248 - 4 * step, 178]);
                assert_eq!(m.placement.position, direct.placement.position);
                assert_eq!(m.no_path, step < 7);
                assert!(m.constrained_walk);
                assert_eq!(m.walk_target, Some([200, 178]));
            }
        }
        assert!(!mover.no_path);
        mover
            .advance_direct_walk(&resources, &declarations, &polygons, &[])
            .unwrap();
        restored
            .advance_direct_walk(&resources, &declarations, &polygons, &[])
            .unwrap();
        assert_eq!(mover, restored);
        assert_eq!(mover.walk_target, None);
        // Off-path entry must resume graph routing, not continue directly
        // toward a destination around the bend in another ordinary polygon.
        let bent = Polygons::new(
            [
                [[0, 0], [40, 0], [40, 40], [0, 40]],
                [[0, 38], [40, 38], [40, 80], [0, 80]],
                [[38, 60], [120, 60], [120, 80], [38, 80]],
                [[38, 0], [80, 0], [80, 40], [38, 40]],
            ]
            .into_iter()
            .map(|corners| crate::polygon::Polygon {
                corners,
                ..polygons.active[0].definition.clone()
            })
            .collect(),
            0,
        );
        let mut reentry = mover.clone();
        reentry
            .stand(&resources, &declarations, &bent, [10, 20], Handle(0))
            .unwrap();
        reentry
            .stand(&resources, &declarations, &bent, [-20, 0], Handle(0))
            .unwrap();
        let reentry_serial = reentry
            .begin_walk(&resources, &declarations, &bent, [100, 70], [100, 70])
            .unwrap();
        let mut restored = reentry.clone();
        let mut crossed_middle = false;
        for _ in 0..200 {
            if reentry.walk_target.is_none() {
                break;
            }
            for m in [&mut reentry, &mut restored] {
                m.advance_direct_walk(&resources, &declarations, &bent, &[])
                    .unwrap();
                assert_eq!(m.walk_serial, reentry_serial);
                if !m.no_path {
                    assert!(crate::routing::path_at(&bent, m.placement.position).is_some());
                }
            }
            assert_eq!(reentry, restored);
            crossed_middle |= reentry.placement.path == Some(1);
        }
        assert!(crossed_middle);
        assert_eq!(reentry.walk_target, None);
        assert_eq!(reentry.placement.position, [100, 70]);
        // Adjacent ordinary paths can require a center waypoint before the
        // final destination: the direct diagonal exits the narrow upper path.
        // Geometry and endpoints reproduce the toy-shop display approach.
        let display_paths = Polygons::new(
            [
                [[96, 162], [96, 160], [98, 160], [98, 162]],
                [[98, 162], [98, 160], [122, 163], [122, 167]],
                [[122, 168], [122, 163], [202, 163], [210, 168]],
                [[91, 192], [122, 168], [210, 168], [248, 192]],
            ]
            .into_iter()
            .enumerate()
            .map(|(index, corners)| {
                let mut p = crate::polygon::Polygon {
                    corners,
                    ..polygons.active[0].definition.clone()
                };
                if index == 1 {
                    p.kind = Kind::NodePath;
                    p.nodes = vec![[99, 161], [121, 165]];
                    p.lines = vec![[-4, 22, -3146, 16, 484, 500, 22, -88, 12584, -69212]];
                    p.reel = 2;
                }
                p
            })
            .collect(),
            0,
        );
        let mut display = mover.clone();
        display
            .stand(
                &resources,
                &declarations,
                &display_paths,
                [124, 165],
                Handle(0),
            )
            .unwrap();
        display
            .begin_walk(
                &resources,
                &declarations,
                &display_paths,
                [95, 191],
                [95, 191],
            )
            .unwrap();
        let mut display_copy = display.clone();
        for _ in 0..200 {
            if display.walk_target.is_none() {
                break;
            }
            for m in [&mut display, &mut display_copy] {
                m.advance_direct_walk(&resources, &declarations, &display_paths, &[])
                    .unwrap();
                assert!(crate::routing::path_at(&display_paths, m.placement.position).is_some());
            }
            assert_eq!(display, display_copy);
        }
        assert_eq!(display.walk_target, None);
        assert_eq!(display.placement.position, [95, 191]);
        let mut detour = mover.clone();
        detour
            .stand(&resources, &declarations, &polygons, [50, 100], Handle(0))
            .unwrap();
        detour
            .begin_direct_walk(&resources, &declarations, &polygons, [180, 100], [180, 100])
            .unwrap();
        let other = crate::walk::Footprint {
            actor: 8,
            position: [110, 100],
            left: 100,
            right: 120,
            node_path: false,
        };
        let mut detour_replay = detour.clone();
        let mut routed = false;
        for _ in 0..200 {
            if detour.walk_target.is_none() {
                break;
            }
            let before_position = detour.placement.position;
            detour
                .advance_direct_walk(&resources, &declarations, &polygons, &[other])
                .unwrap();
            detour_replay
                .advance_direct_walk(&resources, &declarations, &polygons, &[other])
                .unwrap();
            assert_eq!(detour, detour_replay);
            if detour
                .direct_leg
                .is_some_and(|leg| leg.target[1] != 100 && leg.target[1] != -1)
            {
                if !routed {
                    assert_eq!(detour.placement.position, before_position);
                }
                routed = true;
            }
        }
        assert!(routed);
        assert_eq!(detour.placement.position, [180, 100]);
        assert_eq!(detour.walk_target, None);
        let mut occupied = detour.clone();
        occupied
            .stand(&resources, &declarations, &polygons, [50, 100], Handle(0))
            .unwrap();
        let occupied_serial = occupied
            .begin_path_walk(&resources, &declarations, &polygons, [110, 100], [110, 100])
            .unwrap();
        let mut occupied_replay = occupied.clone();
        for _ in 0..30 {
            if occupied.walk_target.is_none() {
                break;
            }
            occupied
                .advance_direct_walk(&resources, &declarations, &polygons, &[other])
                .unwrap();
            occupied_replay
                .advance_direct_walk(&resources, &declarations, &polygons, &[other])
                .unwrap();
            assert_eq!(occupied, occupied_replay);
            assert!(
                occupied.direct_leg.is_none(),
                "occupied target must not detour"
            );
        }
        assert_eq!(occupied.walk_target, None);
        assert_eq!(occupied.walk_serial, occupied_serial);
        assert_eq!(occupied.placement.position[1], 100);
        assert!(occupied.placement.position[0] > 50);
        assert!(occupied.placement.position[0] < 100);
        assert_eq!(occupied.placement.standing, Handle(64));
        // An obstacle reaches the right path edge. The only usable detour
        // goes around its left side, then above it. Resuming the ultimate
        // destination at the lower-left corner used to loop back indefinitely.
        let mut narrow_path = polygons.active[0].definition.clone();
        narrow_path.corners = [[140, 140], [303, 140], [303, 178], [140, 178]];
        let narrow = Polygons::new(vec![narrow_path], 0);
        let blocker = crate::walk::Footprint {
            actor: 8,
            position: [279, 170],
            left: 264,
            right: 310,
            node_path: false,
        };
        let mut around = occupied.clone();
        around
            .stand(&resources, &declarations, &narrow, [240, 177], Handle(0))
            .unwrap();
        let around_serial = around
            .begin_path_walk(&resources, &declarations, &narrow, [302, 167], [302, 167])
            .unwrap();
        let mut resumed = around.clone();
        let mut passed_above = false;
        for _ in 0..200 {
            if around.walk_target.is_none() {
                break;
            }
            for actor in [&mut around, &mut resumed] {
                actor
                    .advance_direct_walk(&resources, &declarations, &narrow, &[blocker])
                    .unwrap();
                assert!(narrow.active[0].contains(actor.placement.position));
            }
            assert_eq!(around, resumed);
            passed_above |= around.placement.position[1] < 167;
        }
        assert!(
            passed_above,
            "must take the second corner, not repeat the first approach"
        );
        assert_eq!(around.walk_target, None);
        assert_eq!(around.placement.position, [302, 167]);
        assert_eq!(around.walk_serial, around_serial);
        assert_eq!(around.placement.standing, Handle(64));

        assert_eq!(mover.walk_serial, serial);
        assert_eq!(mover.placement.standing, Handle(64));
        assert_eq!(
            mover
                .begin_direct_walk(&resources, &declarations, &polygons, [208, 178], [208, 178])
                .unwrap(),
            serial + 1
        );
        let position = mover.placement.position;
        let short_serial = mover
            .begin_direct_walk(&resources, &declarations, &polygons, [201, 178], [0, 178])
            .unwrap();
        assert_eq!(short_serial, serial + 2);
        assert_eq!(mover.placement.direction, Direction::Left);
        assert_eq!(mover.placement.position, position);
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.direct_leg, None);
        assert_eq!(mover.node_walk, None);
        let serial = mover
            .begin_path_walk(&resources, &declarations, &polygons, [148, 178], [148, 178])
            .unwrap();
        assert!(mover.constrained_walk);
        let mut restored = mover.clone();
        for _ in 0..14 {
            mover
                .advance_direct_walk(&resources, &declarations, &polygons, &[])
                .unwrap();
            restored
                .advance_direct_walk(&resources, &declarations, &polygons, &[])
                .unwrap();
        }
        assert_eq!(mover, restored);
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.walk_serial, serial);
        assert_eq!(mover.placement.position, [148, 178]);
        mover
            .adopt_tag_pose(&resources, &declarations, Handle(1))
            .unwrap();
        assert_eq!(mover.placement.direction, Direction::Away);
        assert_eq!(mover.animation.object.id, 7);
        let before = mover.clone();
        assert!(
            mover
                .begin_path_walk(&resources, &declarations, &polygons, [300, 178], [300, 178])
                .is_err()
        );
        assert_eq!(mover, before);
        mover
            .adopt_tag_pose(&resources, &declarations, Handle(0))
            .unwrap();
        assert_eq!(mover, before);
        let serial = mover.walk_serial;
        mover
            .begin_walk(&resources, &declarations, &polygons, [151, 179], [200, 178])
            .unwrap();
        assert_eq!(
            mover.placement.position,
            [148, 178],
            "near destinations do not snap position"
        );
        assert_eq!(mover.placement.direction, Direction::Right);
        assert_eq!(mover.walk_serial, serial + 1);
        assert_eq!(mover.walk_target, None);
        mover
            .begin_walk(&resources, &declarations, &polygons, [148, 178], [148, 100])
            .unwrap();
        assert_eq!(mover.placement.direction, Direction::Away);
        let mut boundary = mover.clone();
        boundary
            .begin_walk(&resources, &declarations, &polygons, [152, 178], [148, 100])
            .unwrap();
        assert_eq!(
            boundary.walk_target,
            Some([152, 178]),
            "four pixels starts movement"
        );
        declarations.movers.get_mut(&7).unwrap()[1][8 + Direction::Away.index()] = Handle(128);
        let saved = mover
            .push_talk(&resources, &declarations, Handle(0))
            .unwrap();
        assert_eq!(saved, Handle(64));
        assert_eq!(mover.step_phase, 0);
        assert_eq!(mover.animation.object.id, 7);
        assert_eq!(mover.animation.capacity, before.animation.capacity);
        assert_eq!(mover.placement.direction, Direction::Away);
        assert_eq!(
            mover.animation.tick(&resources).unwrap(),
            crate::animation::Tick::Waiting
        );
        assert_eq!(
            mover.animation.tick(&resources).unwrap(),
            crate::animation::Tick::Finished
        );
        mover.pop_talk(&resources, &declarations, saved).unwrap();
        assert_eq!(mover.placement.standing, Handle(64));
        assert_eq!(
            mover.animation.tick(&resources).unwrap(),
            crate::animation::Tick::Waiting
        );
        assert_eq!(
            mover.animation.tick(&resources).unwrap(),
            crate::animation::Tick::Frame
        );
        declarations.movers.get_mut(&7).unwrap()[1][8 + Direction::Right.index()] = Handle(0);
        let before = mover.clone();
        assert!(
            mover
                .push_talk(&resources, &declarations, Handle(4))
                .is_err()
        );
        assert_eq!(mover, before);
        let stationary = mover.clone();
        mover
            .begin_walk(&resources, &declarations, &polygons, [160, 178], [160, 178])
            .unwrap();
        let before = mover.clone();
        let saved = mover
            .push_talk(&resources, &declarations, Handle(1))
            .unwrap();
        assert_eq!(saved, Handle(0));
        assert!(mover.special_reel());
        assert_eq!(mover.walk_target, before.walk_target);
        mover.pop_talk(&resources, &declarations, saved).unwrap();
        assert!(!mover.special_reel());
        assert_eq!(mover.walk_target, before.walk_target);
        assert_eq!(mover.walk_serial, before.walk_serial);
        // The remaining fixture cases start from the stationary state.
        mover = stationary;
        let mut nodes = polygons.clone();
        let p = &mut nodes.active[0].definition;
        p.kind = Kind::NodePath;
        p.nodes = vec![[20, 20], [60, 20], [60, 60], [100, 60]];
        p.lines = vec![
            [0, 40, -800, 0, 0, 0, 0, 0, 0, 0],
            [-40, 0, 2400, 0, 0, 0, 0, 0, 0, 0],
            [0, 40, -2400, 0, 0, 0, 0, 0, 0, 0],
        ];
        // SPLAY restarts the saved ultimate destination from its film position,
        // including PositionMover's node snap and off-path reentry.
        for position in [[61, 58], [248, 178]] {
            let mut resumed = mover.clone();
            resumed
                .stand(&resources, &declarations, &nodes, [20, 20], Handle(0))
                .unwrap();
            resumed.no_path = false;
            resumed
                .begin_walk(&resources, &declarations, &nodes, [90, 62], [90, 62])
                .unwrap();
            let serial = resumed.walk_serial;
            resumed.hidden = true;
            resumed
                .restore_after_splay(&resources, &declarations, &nodes, position, [90, 62])
                .unwrap();
            assert_eq!(resumed.walk_serial, serial + 1);
            assert!(!resumed.hidden);
            assert_eq!(resumed.walk_target, Some([90, 60]));
            if position == [61, 58] {
                assert_eq!(resumed.placement.position, [60, 60]);
            } else {
                assert!(resumed.no_path);
                assert_eq!(resumed.placement.position, position);
            }
            let mut continuation = resumed.clone();
            for _ in 0..200 {
                if resumed.walk_target.is_none() {
                    break;
                }
                resumed
                    .advance_direct_walk(&resources, &declarations, &nodes, &[])
                    .unwrap();
                continuation
                    .advance_direct_walk(&resources, &declarations, &nodes, &[])
                    .unwrap();
                assert_eq!(resumed, continuation);
            }
            assert_eq!(resumed.walk_target, None);
            assert_eq!(resumed.placement.position, [90, 60]);
        }
        // PositionMover preserves bNoPath after a scripted off-path pose,
        // even when a later STAND places the actor on a node polygon.
        let mut reentry = mover.clone();
        reentry
            .stand(&resources, &declarations, &nodes, [248, 178], Handle(0))
            .unwrap();
        reentry
            .stand(&resources, &declarations, &nodes, [100, 60], Handle(0))
            .unwrap();
        assert!(reentry.no_path);
        let serial = reentry
            .begin_walk(&resources, &declarations, &nodes, [30, 20], [30, 20])
            .unwrap();
        let mut restored = reentry.clone();
        let mut visited_reentry = Vec::new();
        for _ in 0..100 {
            if reentry.walk_target.is_none() {
                break;
            }
            for m in [&mut reentry, &mut restored] {
                m.advance_direct_walk(&resources, &declarations, &nodes, &[])
                    .unwrap();
                assert_eq!(m.walk_serial, serial);
            }
            assert_eq!(reentry, restored);
            visited_reentry.push(reentry.placement.position);
        }
        assert!(!reentry.no_path);
        assert_eq!(reentry.walk_target, None);
        assert_eq!(reentry.placement.position, [30, 20]);
        assert!(visited_reentry.contains(&[60, 60]));
        assert!(visited_reentry.contains(&[60, 20]));
        mover
            .stand(&resources, &declarations, &nodes, [20, 20], Handle(0))
            .unwrap();
        assert_eq!(mover.walk_target, None);
        mover.no_path = false;
        mover
            .begin_walk(&resources, &declarations, &nodes, [90, 62], [90, 62])
            .unwrap();
        assert_eq!(mover.walk_target, Some([90, 60]));
        let mut snapshot = mover.clone();
        let mut visited = Vec::new();
        for _ in 0..100 {
            if mover.walk_target.is_none() {
                break;
            }
            mover
                .advance_direct_walk(&resources, &declarations, &nodes, &[])
                .unwrap();
            snapshot
                .advance_direct_walk(&resources, &declarations, &nodes, &[])
                .unwrap();
            assert_eq!(mover, snapshot);
            visited.push(mover.placement.position);
        }
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.placement.position, [90, 60]);
        assert!(visited.contains(&[60, 20]));
        assert!(visited.contains(&[60, 60]));
        assert_eq!(mover.node_travel.unwrap().line, 2);
        mover
            .begin_walk(&resources, &declarations, &nodes, [30, 20], [30, 20])
            .unwrap();
        for _ in 0..100 {
            if mover.walk_target.is_none() {
                break;
            }
            mover
                .advance_direct_walk(&resources, &declarations, &nodes, &[])
                .unwrap();
        }
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.placement.position, [30, 20]);
        assert_eq!(mover.node_travel.unwrap().line, 0);
        mover
            .begin_walk(&resources, &declarations, &nodes, [30, 30], [30, 0])
            .unwrap();
        mover
            .advance_direct_walk(&resources, &declarations, &nodes, &[])
            .unwrap();
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.placement.position, [30, 20]);
        assert_eq!(mover.placement.direction, Direction::Away);
        let before = mover.clone();
        assert!(
            mover
                .begin_walk(&resources, &declarations, &nodes, [300, 300], [300, 300])
                .is_err()
        );
        assert_eq!(mover, before);
        let mut source = nodes.active[0].definition.clone();
        source.corners = [[0, 0], [60, 0], [60, 40], [0, 40]];
        source.nodes = vec![[20, 20], [50, 20]];
        source.lines = vec![[0, 30, -600, 0, 0, 0, 0, 0, 0, 0]];
        let mut destination = source.clone();
        destination.corners = [[58, 0], [120, 0], [120, 80], [58, 80]];
        destination.nodes = vec![[70, 20], [100, 20], [100, 60]];
        destination.lines = vec![
            [0, 30, -600, 0, 0, 0, 0, 0, 0, 0],
            [-40, 0, 4000, 0, 0, 0, 0, 0, 0, 0],
        ];
        let connected = Polygons::new(vec![source, destination], 0);
        // SWALK retains Tline but ignores path constraints. A fresh node entry
        // may steer to its endpoint; SetNextDest then resumes the ultimate
        // target without following the remaining node bends.
        let mut direct = mover.clone();
        direct
            .stand(&resources, &declarations, &connected, [20, 20], Handle(0))
            .unwrap();
        direct.target_node_line = 1;
        direct
            .begin_direct_walk(&resources, &declarations, &connected, [110, 60], [110, 60])
            .unwrap();
        let serial = direct.walk_serial;
        let mut replay = direct.clone();
        let mut entry_endpoint = false;
        let mut resumed = false;
        for _ in 0..150 {
            if direct.walk_target.is_none() {
                break;
            }
            direct
                .advance_direct_walk(&resources, &declarations, &connected, &[])
                .unwrap();
            replay
                .advance_direct_walk(&resources, &declarations, &connected, &[])
                .unwrap();
            assert_eq!(direct, replay);
            assert_eq!(direct.walk_serial, serial);
            entry_endpoint |= direct
                .direct_leg
                .is_some_and(|leg| leg.target == [70, 20] && leg.overshoot);
            resumed |= entry_endpoint
                && direct
                    .direct_leg
                    .is_some_and(|leg| leg.target == [110, 60] && !leg.overshoot);
        }
        assert!(entry_endpoint);
        assert!(resumed);
        assert_eq!(direct.walk_target, None);
        assert_eq!(direct.direct_leg, None);
        assert_eq!(direct.placement.position, [110, 60]);
        direct
            .begin_direct_walk(&resources, &declarations, &connected, [20, 20], [20, 20])
            .unwrap();
        assert_eq!(direct.direct_leg, None);

        mover
            .stand(&resources, &declarations, &connected, [20, 20], Handle(0))
            .unwrap();
        mover
            .begin_walk(&resources, &declarations, &connected, [100, 50], [100, 50])
            .unwrap();
        let mut restored = mover.clone();
        let mut crossed = false;
        let mut retargeted_exit = false;
        for _ in 0..200 {
            if mover.walk_target.is_none() {
                break;
            }
            if !retargeted_exit
                && mover.placement.path == Some(0)
                && mover
                    .node_travel
                    .is_some_and(|t| t.status == crate::nodepath::TravelStatus::Leaving)
            {
                let old_target = mover.node_walk.as_ref().unwrap().leg.target;
                let old_serial = mover.walk_serial;
                mover
                    .begin_walk(&resources, &declarations, &connected, [100, 50], [100, 50])
                    .unwrap();
                assert_eq!(mover.node_walk.as_ref().unwrap().leg.target, old_target);
                assert!(!mover.node_walk.as_ref().unwrap().leg.overshoot);
                assert_eq!(mover.walk_serial, old_serial + 1);
                // The legacy mover payload deliberately excludes reel state;
                // DWRSAVE v13 carries that state in its companion extension.
                let bytes = postcard::to_allocvec(&(&mover, mover.reel_state)).unwrap();
                let (decoded, reel_state): (Mover, Option<crate::mover_reel::State>) =
                    postcard::from_bytes(&bytes).unwrap();
                restored = decoded;
                restored.reel_state = reel_state;
                retargeted_exit = true;
            }
            mover
                .advance_direct_walk(&resources, &declarations, &connected, &[])
                .unwrap();
            restored
                .advance_direct_walk(&resources, &declarations, &connected, &[])
                .unwrap();
            assert_eq!(mover, restored);
            crossed |= mover.placement.path == Some(1);
        }
        assert!(crossed);
        assert!(retargeted_exit);
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.placement.position, [100, 50]);
        let source = connected.active[0].definition.clone();
        let mut middle = source.clone();
        middle.corners = [[58, 0], [82, 0], [82, 40], [58, 40]];
        middle.nodes = vec![[70, 20], [70, 20]];
        middle.lines = vec![[0; 10]];
        let mut destination = connected.active[1].definition.clone();
        destination.corners = [[80, 0], [120, 0], [120, 80], [80, 80]];
        destination.nodes = vec![[90, 20], [110, 20], [110, 60]];
        destination.lines = vec![
            [0, 20, -400, 0, 0, 0, 0, 0, 0, 0],
            [-40, 0, 4400, 0, 0, 0, 0, 0, 0, 0],
        ];
        let bypass = Polygons::new(vec![source, middle, destination], 0);
        mover
            .stand(&resources, &declarations, &bypass, [20, 20], Handle(0))
            .unwrap();
        mover
            .begin_walk(&resources, &declarations, &bypass, [110, 50], [110, 50])
            .unwrap();
        let mut restored = mover.clone();
        let mut bypassed = false;
        for _ in 0..200 {
            if mover.walk_target.is_none() {
                break;
            }
            mover
                .advance_direct_walk(&resources, &declarations, &bypass, &[])
                .unwrap();
            restored
                .advance_direct_walk(&resources, &declarations, &bypass, &[])
                .unwrap();
            assert_eq!(mover, restored);
            if mover.placement.path == Some(1) {
                assert_eq!(mover.node_travel, None);
                assert!(!mover.footprint(&resources).unwrap().node_path);
                if !bypassed {
                    mover
                        .begin_walk(&resources, &declarations, &bypass, [110, 50], [110, 50])
                        .unwrap();
                    restored = mover.clone();
                }
                bypassed = true;
            }
        }
        assert!(bypassed);
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.placement.position, [110, 50]);
        mover
            .stand(&resources, &declarations, &bypass, [20, 20], Handle(0))
            .unwrap();
        let travel = mover.node_travel;
        mover
            .begin_direct_walk(&resources, &declarations, &bypass, [-20, 20], [-20, 20])
            .unwrap();
        let mut restored = mover.clone();
        for tick in 1..=10 {
            mover
                .advance_direct_walk(&resources, &declarations, &bypass, &[])
                .unwrap();
            restored
                .advance_direct_walk(&resources, &declarations, &bypass, &[])
                .unwrap();
            assert_eq!(mover, restored);
            assert_eq!(mover.placement.position, [20 - 4 * tick, 20]);
            assert_eq!(mover.placement.path, Some(0));
            assert_eq!(mover.node_travel, travel);
        }
        mover
            .advance_direct_walk(&resources, &declarations, &bypass, &[])
            .unwrap();
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.placement.position, [-20, 20]);
        let mut ordinary = bypass.clone();
        ordinary.active[1].definition.kind = Kind::Path;
        ordinary.active[1].definition.nodes.clear();
        ordinary.active[1].definition.lines.clear();
        mover
            .stand(&resources, &declarations, &ordinary, [20, 20], Handle(0))
            .unwrap();
        mover
            .begin_walk(&resources, &declarations, &ordinary, [110, 50], [110, 50])
            .unwrap();
        let mut restored = mover.clone();
        let mut crossed_ordinary = false;
        for _ in 0..200 {
            if mover.walk_target.is_none() {
                break;
            }
            mover
                .advance_direct_walk(&resources, &declarations, &ordinary, &[])
                .unwrap();
            restored
                .advance_direct_walk(&resources, &declarations, &ordinary, &[])
                .unwrap();
            assert_eq!(mover, restored);
            if mover.placement.path == Some(1) {
                crossed_ordinary = true;
                assert!(mover.node_travel.is_none());
                assert!(!mover.footprint(&resources).unwrap().node_path);
            }
        }
        assert!(crossed_ordinary);
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.placement.position, [110, 50]);
        mover
            .stand(&resources, &declarations, &ordinary, [20, 20], Handle(0))
            .unwrap();
        mover
            .begin_walk(&resources, &declarations, &ordinary, [72, 30], [72, 30])
            .unwrap();
        let mut restored = mover.clone();
        for _ in 0..100 {
            if mover.walk_target.is_none() {
                break;
            }
            mover
                .advance_direct_walk(&resources, &declarations, &ordinary, &[])
                .unwrap();
            restored
                .advance_direct_walk(&resources, &declarations, &ordinary, &[])
                .unwrap();
            assert_eq!(mover, restored);
        }
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.placement.position, [72, 30]);
        assert_eq!(mover.placement.path, Some(1));
        assert!(mover.node_travel.is_none());
        assert_eq!(mover.node_walk, None);
        mover
            .begin_walk(&resources, &declarations, &ordinary, [110, 50], [110, 50])
            .unwrap();
        assert!(mover.node_travel.is_none());
        assert!(mover.node_walk.is_some());
        let mut restored = mover.clone();
        let mut entered_node = false;
        for _ in 0..100 {
            if mover.walk_target.is_none() {
                break;
            }
            mover
                .advance_direct_walk(&resources, &declarations, &ordinary, &[])
                .unwrap();
            restored
                .advance_direct_walk(&resources, &declarations, &ordinary, &[])
                .unwrap();
            assert_eq!(mover, restored);
            entered_node |= mover.placement.path == Some(2) && mover.node_travel.is_some();
        }
        assert!(entered_node);
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.placement.position, [110, 50]);
    }
    #[test]
    fn standing_changes_script_but_preserves_base_object_and_capacity() {
        let mut data = vec![0; 180];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, 32),
            (12, 56),
            (32, 96),
            (40, 7),
            (64, 12),
            (68, 1),
            (72, 128),
            (76, 160),
            (96, 112),
            (136, 99),
            (160, 168),
            (168, 112),
            (172, 112),
        ] {
            data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = crate::graphics::Resources(vec![&data]);
        let placement = Placement {
            actor: 7,
            position: [200, 178],
            direction: Direction::Left,
            path: None,
            node: None,
            scale: 1,
            standing: Handle(64),
            base_walk: Handle(0),
            z_factor: 2,
        };
        let mover = Mover::new(&resources, placement).unwrap();
        assert_eq!(
            mover.animation.object.id, 7,
            "standing film's object must not replace base object"
        );
        assert_eq!(mover.animation.capacity, 1);
        assert_eq!(mover.animation.images(&resources).unwrap(), [Handle(112)]);
        assert_eq!(mover.animation.object.z, 1202);
        assert_eq!(
            (mover.animation.object.x, mover.animation.object.y),
            (200, 178)
        );
        assert_eq!(
            mover.animation.steps, 1,
            "standing immediately steps its script"
        );
        let mut resumed = mover.clone();
        assert_eq!(
            resumed.animation.tick(&resources).unwrap(),
            crate::animation::Tick::Waiting
        );
        let before = resumed.clone();
        resumed
            .stand(
                &resources,
                &Declarations::default(),
                &Polygons::default(),
                [-1, -1],
                Handle(0),
            )
            .unwrap();
        assert_eq!(
            resumed, before,
            "STAND with no position/direction leaves the current animation alone"
        );
    }
    #[test]
    fn lead_alias_direction_and_standing_bank_selection() {
        let mut declarations = Declarations {
            lead: Some(Lead {
                id: 7,
                text: Handle(0),
                tagged: false,
            }),
            ..Declarations::default()
        };
        declarations.movers.insert(
            7,
            std::array::from_fn(|scale| {
                std::array::from_fn(|i| Handle(100 + scale as u32 * 12 + i as u32))
            }),
        );
        let p = Placement::initial(
            &declarations,
            &Polygons::default(),
            -2,
            [200, 178],
            Handle(3),
        )
        .unwrap();
        assert_eq!(
            (p.actor, p.direction, p.standing, p.base_walk),
            (7, Direction::Left, Handle(104), Handle(102))
        );
        assert_eq!(p.position, [200, 178]);
        assert_eq!(p.path, None);
        assert_eq!(
            Placement::initial(&declarations, &Polygons::default(), 7, [0, 0], Handle(900))
                .unwrap()
                .standing,
            Handle(900)
        );
        assert!(
            Placement::initial(&declarations, &Polygons::default(), 8, [0, 0], Handle(3)).is_err()
        );
        assert_eq!(
            Direction::from_talk_constant(0, Direction::Right).unwrap(),
            Direction::Right
        );
        assert_eq!(
            Direction::from_talk_constant(1, Direction::Left).unwrap(),
            Direction::Away
        );
    }
}
