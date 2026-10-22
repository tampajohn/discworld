// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 AlterMover reel state, from pinned movers.cpp.
//!
//! Selection is independent of movement/route state. The returned transition
//! must be committed only after its animation operation succeeds. This module
//! does not yet replace the legacy mover bookkeeping in the engine.
use crate::graphics::Handle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    #[default]
    Normal,
    Special,
    CustomWalk,
}

/// The reference has one pushed film slot; POP does not clear it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct State {
    mode: Mode,
    pushed: Handle,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            pushed: Handle(0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Normal(Handle),
    Walk(Handle),
    Push(Handle),
    Pop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Animation {
    /// No script, phase, last-film, depth or route change.
    Keep,
    /// Force the current direction/scale's walking reel. Unlike installing a
    /// film, this preserves the walking phase (SetMoverWalkReel).
    RestoreWalking,
    /// Install reel zero at the film rate, reset V1 walking phase, update
    /// hLastFilm/depth, and optionally step immediately. Hidden actors still
    /// take this initial step, then retain hidden display depth.
    Install { film: Handle, first_step: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub state: State,
    pub animation: Animation,
}

impl State {
    pub fn mode(self) -> Mode {
        self.mode
    }

    pub fn pushed(self) -> Handle {
        self.pushed
    }

    /// Special reels suspend movement without ending or clearing the route.
    pub fn suspends_movement(self) -> bool {
        self.mode == Mode::Special
    }

    /// SetMoverWalkReel restores actor ownership/visibility before this test;
    /// only script/scale/direction selection is suppressed by custom walking.
    pub fn suppresses_walk_selection(self) -> bool {
        self.mode == Mode::CustomWalk
    }

    /// Plan without mutation so the caller can validate/install resources
    /// before changing the pushed slot or mode.
    pub fn transition(self, last_film: Handle, operation: Operation) -> Transition {
        let mut state = self;
        let film = match operation {
            Operation::Pop => self.pushed,
            Operation::Push(film) => {
                state.pushed = if self.mode == Mode::Special {
                    last_film
                } else {
                    Handle(0)
                };
                film
            }
            Operation::Normal(film) | Operation::Walk(film) => film,
        };
        let animation = if film.0 == 0 {
            if state.mode == Mode::Special {
                state.mode = Mode::Normal;
                Animation::RestoreWalking
            } else {
                Animation::Keep
            }
        } else {
            let walking = matches!(operation, Operation::Walk(_));
            state.mode = if walking {
                Mode::CustomWalk
            } else {
                Mode::Special
            };
            Animation::Install {
                film,
                first_step: !walking,
            }
        };
        Transition { state, animation }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walking_speech_restores_walk_selection_not_a_standing_film() {
        let original = State::default();
        let talk = original.transition(Handle(100), Operation::Push(Handle(200)));
        assert_eq!(
            original,
            State::default(),
            "planning must not mutate the caller"
        );
        assert_eq!(talk.state.pushed(), Handle(0));
        assert!(talk.state.suspends_movement());
        assert_eq!(
            talk.animation,
            Animation::Install {
                film: Handle(200),
                first_step: true
            }
        );
        // FinishTalkingReel selects standing before POP. The slot must survive
        // that selection, and POP must force walking rather than restore 100.
        let standing = talk
            .state
            .transition(Handle(200), Operation::Normal(Handle(300)));
        let resumed = standing.state.transition(Handle(300), Operation::Pop);
        assert_eq!(resumed.animation, Animation::RestoreWalking);
        assert_eq!(resumed.state.mode(), Mode::Normal);
        assert!(!resumed.state.suspends_movement());
    }

    #[test]
    fn stationary_speech_restores_special_film_and_pop_retains_slot() {
        let standing = State::default().transition(Handle(0), Operation::Normal(Handle(100)));
        let talk = standing
            .state
            .transition(Handle(100), Operation::Push(Handle(200)));
        assert_eq!(talk.state.pushed(), Handle(100));
        let pop = talk.state.transition(Handle(200), Operation::Pop);
        assert_eq!(
            pop.animation,
            Animation::Install {
                film: Handle(100),
                first_step: true
            }
        );
        assert!(pop.state.suspends_movement());
        assert_eq!(pop.state.pushed(), Handle(100));
        assert_eq!(pop.state.transition(Handle(100), Operation::Pop), pop);
    }

    #[test]
    fn overlapping_pushes_use_one_slot_not_a_stack() {
        let standing = State::default().transition(Handle(0), Operation::Normal(Handle(100)));
        let first = standing
            .state
            .transition(Handle(100), Operation::Push(Handle(200)));
        let second = first
            .state
            .transition(Handle(200), Operation::Push(Handle(300)));
        let popped = second.state.transition(Handle(300), Operation::Pop);
        assert_eq!(popped.state.pushed(), Handle(200));
        assert_eq!(
            popped.animation,
            Animation::Install {
                film: Handle(200),
                first_step: true
            }
        );
        assert_eq!(popped.state.transition(Handle(200), Operation::Pop), popped);
    }

    #[test]
    fn new_walk_during_speech_releases_animation_mode_without_consuming_pop() {
        let standing = State::default().transition(Handle(0), Operation::Normal(Handle(100)));
        let talk = standing
            .state
            .transition(Handle(100), Operation::Push(Handle(200)));
        let walk = talk
            .state
            .transition(Handle(200), Operation::Normal(Handle(0)));
        assert_eq!(walk.animation, Animation::RestoreWalking);
        assert!(!walk.state.suspends_movement());
        assert_eq!(walk.state.pushed(), Handle(100));
        let later_pop = walk.state.transition(Handle(400), Operation::Pop);
        assert_eq!(
            later_pop.animation,
            Animation::Install {
                film: Handle(100),
                first_step: true
            }
        );
    }

    #[test]
    fn custom_walk_has_no_initial_step_and_zero_does_not_clear_it() {
        let walk = State::default().transition(Handle(100), Operation::Walk(Handle(200)));
        assert_eq!(
            walk.animation,
            Animation::Install {
                film: Handle(200),
                first_step: false
            }
        );
        assert!(walk.state.suppresses_walk_selection());
        assert!(!walk.state.suspends_movement());
        for operation in [
            Operation::Normal(Handle(0)),
            Operation::Walk(Handle(0)),
            Operation::Pop,
        ] {
            let zero = walk.state.transition(Handle(200), operation);
            assert_eq!(zero.animation, Animation::Keep);
            assert_eq!(zero.state, walk.state);
        }
        // PUSH saves zero for custom walking, too. Talking then POP does not
        // reinstate the prior custom walk film.
        let talk = walk
            .state
            .transition(Handle(200), Operation::Push(Handle(300)));
        assert_eq!(talk.state.pushed(), Handle(0));
        assert!(!talk.state.suppresses_walk_selection());
        let popped = talk.state.transition(Handle(300), Operation::Pop);
        assert_eq!(popped.animation, Animation::RestoreWalking);
        assert_eq!(popped.state.mode(), Mode::Normal);
    }

    #[test]
    fn zero_push_saves_slot_before_restoring_walk_selection() {
        let standing = State::default().transition(Handle(0), Operation::Normal(Handle(100)));
        let zero = standing
            .state
            .transition(Handle(100), Operation::Push(Handle(0)));
        assert_eq!(zero.state.pushed(), Handle(100));
        assert_eq!(zero.animation, Animation::RestoreWalking);
        assert_eq!(zero.state.mode(), Mode::Normal);
        let restored: State =
            postcard::from_bytes(&postcard::to_allocvec(&zero.state).unwrap()).unwrap();
        assert_eq!(restored, zero.state);
        assert_eq!(
            restored.transition(Handle(200), Operation::Pop),
            zero.state.transition(Handle(200), Operation::Pop)
        );
    }
}
