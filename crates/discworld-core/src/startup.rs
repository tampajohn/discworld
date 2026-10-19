// SPDX-License-Identifier: GPL-3.0-or-later
//! Persistent declaration state consumed by the future interactive runtime.
//! Unsupported operations remain suspended. This module does not run scene transitions.
use crate::{
    film::Film,
    graphics::{Handle, Resources},
    iso::Error,
    script::{LibraryCall, Machine},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub title: Handle,
    pub capacity: i32,
    pub min: [i32; 2],
    pub size: [i32; 2],
    pub max: [i32; 2],
    pub position: [i32; 2],
    /// Persistent V1 FirstDisp, a backing-array index (not necessarily row aligned).
    pub first_display: usize,
    pub contents: Vec<i32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lead {
    pub id: i32,
    pub text: Handle,
    pub tagged: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalingReels {
    pub actor: i32,
    pub scale: i32,
    pub direction: i32,
    pub reels: [Handle; 4],
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declarations {
    pub cursor: Option<(Handle, Film)>,
    pub windows: [Option<Window>; 3],
    pub window_parts: Option<Handle>,
    pub tag_font: Option<Handle>,
    pub talk_font: Option<Handle>,
    pub flags: Option<Handle>,
    pub config_strings: Option<[Handle; 20]>,
    pub lead: Option<Lead>,
    pub scaling: Vec<ScalingReels>,
    pub movers: BTreeMap<i32, [[Handle; 12]; 10]>,
    pub permanent_topics: Vec<i32>,
    pub speech_colors: BTreeMap<i32, [u8; 3]>,
}
impl Declarations {
    /// Apply supported declarations atomically with VM continuation. Returns false
    /// for operations needing other subsystems, without changing either state.
    pub fn apply(
        &mut self,
        vm: &mut Machine,
        call: LibraryCall,
        resources: &Resources<'_>,
    ) -> Result<bool, Error> {
        let count = match call.id {
            0 => 4,
            10 => 14,
            59 => 7,
            89 => 41,
            19 | 21 | 22 => 8,
            16 | 20 | 23 | 25 | 26 | 105 => 1,
            24 => 62,
            101 => 20,
            _ => return Ok(false),
        };
        let stack = vm.operands();
        let begin = stack
            .len()
            .checked_sub(count)
            .ok_or(Error("declaration arguments missing"))?;
        let args = &stack[begin..];
        let mut next = self.clone();
        match call.id {
            0 => {
                if args[0] != -1 && args[0] <= 0 {
                    return Err(Error("invalid speech color actor"));
                }
                next.speech_colors
                    .insert(args[0], std::array::from_fn(|i| args[i + 1].min(255) as u8));
            }
            16 => {
                if !next.permanent_topics.contains(&args[0]) {
                    if next.permanent_topics.len() >= 10 {
                        return Err(Error("permanent topic limit"));
                    }
                    next.permanent_topics.push(args[0]);
                }
            }
            89 => {
                if args[0] <= 0 {
                    return Err(Error("invalid mover ID"));
                }
                let reels = std::array::from_fn(|scale| {
                    std::array::from_fn(|i| {
                        if i >= 8 {
                            return Handle(0);
                        }
                        let source = if scale < 5 {
                            scale
                        } else if i >= 4 {
                            2
                        } else {
                            4
                        };
                        Handle(args[1 + source * 8 + i] as u32)
                    })
                });
                next.movers.insert(args[0], reels);
            }
            59 => {
                if !(1..=5).contains(&args[1])
                    || !(0..=1).contains(&args[2])
                    || (args[1] == 1 && args[2] == 1)
                    || (args[1] == 5 && args[2] == 0)
                {
                    return Err(Error("invalid scaling reel scale"));
                }
                if next.scaling.len() >= 54 {
                    return Err(Error("scaling reel limit"));
                }
                next.scaling.push(ScalingReels {
                    actor: args[0],
                    scale: args[1],
                    direction: args[2],
                    reels: std::array::from_fn(|i| Handle(args[3 + i] as u32)),
                });
            }
            10 => {
                let actor = if args[0] == -2 {
                    next.lead.as_ref().ok_or(Error("lead not registered"))?.id
                } else {
                    args[0]
                };
                if !(1..=10).contains(&args[1]) {
                    return Err(Error("invalid AUXSCALE target"));
                }
                let reels = next
                    .movers
                    .get_mut(&actor)
                    .ok_or(Error("AUXSCALE mover not registered"))?;
                reels[args[1] as usize - 1] = std::array::from_fn(|i| Handle(args[i + 2] as u32));
            }
            20 => {
                let handle = Handle(args[0] as u32);
                let film = Film::read(resources, handle)?;
                next.cursor = Some((handle, film));
            }
            19 | 21 | 22 => {
                let slot = match call.id {
                    19 => 0,
                    21 => 1,
                    _ => 2,
                };
                next.windows[slot] = Some(Window {
                    title: Handle(args[0] as u32),
                    capacity: args[1].min(150),
                    min: [args[2], args[3]],
                    size: [args[4].min(args[6].min(10)), args[5].min(args[7].min(6))],
                    max: [args[6].min(10), args[7].min(6)],
                    position: if slot == 0 { [20, 8] } else { [100, 100] },
                    first_display: 0,
                    contents: Vec::new(),
                });
            }
            23 => next.window_parts = Some(Handle(args[0] as u32)),
            25 => next.tag_font = Some(Handle(args[0] as u32)),
            26 => next.talk_font = Some(Handle(args[0] as u32)),
            105 => next.flags = Some(Handle(args[0] as u32)),
            101 => next.config_strings = Some(std::array::from_fn(|i| Handle(args[i] as u32))),
            24 => {
                if args[0] <= 0 {
                    return Err(Error("invalid lead actor ID"));
                }
                let reels = std::array::from_fn(|scale| {
                    std::array::from_fn(|i| {
                        let source = if scale < 5 {
                            scale
                        } else if (4..8).contains(&i) {
                            2
                        } else {
                            4
                        };
                        Handle(args[1 + source * 12 + i] as u32)
                    })
                });
                next.movers.insert(args[0], reels);
                next.lead = Some(Lead {
                    id: args[0],
                    text: Handle(args[61] as u32),
                    tagged: true,
                });
            }
            _ => unreachable!(),
        }
        vm.complete_library(call, count, None)?;
        *self = next;
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Step;
    fn apply_args(state: &mut Declarations, id: u8, args: &[i32]) {
        let mut code = Vec::new();
        for a in args {
            code.push(1);
            code.extend_from_slice(&a.to_le_bytes());
        }
        code.extend_from_slice(&[0x4e, id, 0]);
        let mut vm = Machine::default();
        loop {
            match vm.step(&code, &mut []).unwrap() {
                Step::Continue => {}
                Step::Library(call) => {
                    assert!(state.apply(&mut vm, call, &Resources(vec![])).unwrap());
                }
                Step::Halted => break,
            }
        }
    }
    #[test]
    fn inventory_dimensions_are_capped_and_reset() {
        let mut state = Declarations::default();
        apply_args(&mut state, 21, &[9, 999, 2, 1, 20, 20, 20, 20]);
        let window = state.windows[1].as_ref().unwrap();
        assert_eq!(window.size, [10, 6]);
        assert_eq!(window.capacity, 150);
        assert_eq!(window.position, [100, 100]);
    }
    #[test]
    fn lead_alias_and_auxiliary_scale_override() {
        let mut state = Declarations::default();
        let mut args = vec![7];
        args.extend(100..160);
        args.push(99);
        apply_args(&mut state, 24, &args);
        let reels = &state.movers[&7];
        assert_eq!(reels[9][0], Handle(148));
        assert_eq!(reels[9][4], Handle(128));
        let mut aux = vec![-2, 10];
        aux.extend(200..212);
        apply_args(&mut state, 10, &aux);
        assert_eq!(state.movers[&7][9][0], Handle(200));
        assert_eq!(state.movers[&7][4][0], Handle(148));
    }
    #[test]
    fn topics_deduplicate_without_reordering() {
        let mut state = Declarations::default();
        for id in [327, 328, 327] {
            apply_args(&mut state, 16, &[id]);
        }
        assert_eq!(state.permanent_topics, [327, 328]);
    }
    #[test]
    fn completes_declaration_and_preserves_unsupported_call() {
        let code = [0x41, 42, 0x4e, 25, 0x4e, 44];
        let mut vm = Machine::default();
        let mut globals = [];
        vm.step(&code, &mut globals).unwrap();
        let Step::Library(call) = vm.step(&code, &mut globals).unwrap() else {
            panic!()
        };
        let mut state = Declarations::default();
        assert!(state.apply(&mut vm, call, &Resources(vec![])).unwrap());
        assert_eq!(state.tag_font, Some(Handle(42)));
        let Step::Library(call) = vm.step(&code, &mut globals).unwrap() else {
            panic!()
        };
        let before = vm.clone();
        assert!(!state.apply(&mut vm, call, &Resources(vec![])).unwrap());
        assert_eq!(vm, before);
    }
    #[test]
    fn invalid_cursor_keeps_state_and_vm() {
        let code = [3, 0x4e, 20];
        let mut vm = Machine::default();
        vm.step(&code, &mut []).unwrap();
        let Step::Library(call) = vm.step(&code, &mut []).unwrap() else {
            panic!()
        };
        let before = vm.clone();
        let mut state = Declarations::default();
        assert!(state.apply(&mut vm, call, &Resources(vec![])).is_err());
        assert_eq!(vm, before);
        assert_eq!(state, Declarations::default());
    }
}
