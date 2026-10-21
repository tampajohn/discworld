// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 inventory POINTED/UNPOINT and PRINTOBJ arbitration decisions.
//!
//! The engine's scheduler owns script execution and text objects. These values
//! model the coroutine suspension points in dialogs.cpp::ObjectProcess and
//! tinlib.cpp::PrintObjPointed without retaining pointers to either resource.
use crate::iso::Error;

/// invLabels remembers an object identity, not the slot it currently occupies.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Monitor {
    pointed: Option<i32>,
    completed: u64,
    non_pointed_running: bool,
}
impl Monitor {
    /// Call with None outside the body, over an empty cell, or over the held item.
    /// invCursor does not update labels at all while a window gesture is active.
    pub fn observe(&mut self, item: Option<i32>, dragging: bool) -> Option<i32> {
        if dragging || item == self.pointed {
            return None;
        }
        self.pointed = item;
        item
    }
    pub fn suppressed(&self) -> bool {
        self.non_pointed_running
    }
    /// PRINTOBJ(-1), and the start of a non-POINTED print after finding its cell.
    pub fn suppress(&mut self) {
        self.non_pointed_running = true;
    }
    /// PRINTOBJ(-2), or the completion of non-POINTED text.
    pub fn allow(&mut self) {
        self.non_pointed_running = false;
    }
    /// V1 clears a previous unmatched OFF when a POINTED print actually begins.
    pub fn begin_pointed_print(&mut self, item: i32, cell: usize) -> Label {
        self.allow();
        Label {
            item,
            cell,
            waiting_for_clear: false,
        }
    }
    /// ObjectProcess increments its shared counter AFTER POINTED interpretation
    /// completes, then sleeps one tick before inspecting the pointer again.
    pub fn completed_pointed(&mut self, item: i32) -> Result<LeaveWait, Error> {
        let serial = self
            .completed
            .checked_add(1)
            .ok_or(Error("inventory pointed counter overflow"))?;
        self.completed = serial;
        Ok(LeaveWait { item, serial })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LeaveWait {
    item: i32,
    serial: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveAction {
    Wait,
    Unpoint,
    Discard,
}
impl LeaveWait {
    /// ObjectProcess checks item departure before its repeated-press counter.
    /// An older waiter still runs UNPOINT if the pointer has already left it.
    pub fn wake(&self, monitor: &Monitor, item_at_pointer: Option<i32>) -> LeaveAction {
        if item_at_pointer != Some(self.item) {
            LeaveAction::Unpoint
        } else if self.serial != monitor.completed {
            LeaveAction::Discard
        } else {
            LeaveAction::Wait
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Label {
    item: i32,
    cell: usize,
    waiting_for_clear: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelAction {
    Keep,
    Hide,
    Hidden,
    Redisplay,
    Finish,
}
impl Label {
    /// Enter the do/while body (including the initial entry before its sleep).
    /// Item departure belongs to wake(), after that sleep, not to this method.
    pub fn enter(&mut self, monitor: &Monitor, held_item: Option<i32>) -> LabelAction {
        if held_item == Some(self.item) {
            LabelAction::Finish
        } else if monitor.suppressed() {
            self.waiting_for_clear = true;
            LabelAction::Hide
        } else {
            LabelAction::Keep
        }
    }
    /// Resume one suspended PrintObjPointed iteration. While suppressed, V1
    /// waits without checking held state or item identity. On clear it compares
    /// the original CELL, redisplays at the original position, and sleeps again.
    /// Ordinary iterations compare OBJECT identity, so scrolling differs from
    /// leaving a suppressed label. The caller retains the original text layout.
    pub fn wake(
        &mut self,
        monitor: &Monitor,
        item_at_pointer: Option<i32>,
        cell_at_pointer: Option<usize>,
        held_item: Option<i32>,
    ) -> LabelAction {
        if self.waiting_for_clear {
            if monitor.suppressed() {
                return LabelAction::Hidden;
            }
            if cell_at_pointer != Some(self.cell) {
                return LabelAction::Finish;
            }
            self.waiting_for_clear = false;
            return LabelAction::Redisplay;
        }
        if item_at_pointer != Some(self.item) {
            return LabelAction::Finish;
        }
        self.enter(monitor, held_item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suspended_label_and_leave_wait_resume_after_serialization() {
        let mut monitor = Monitor::default();
        monitor.observe(Some(190), false);
        let waiter = monitor.completed_pointed(200).unwrap();
        let mut label = monitor.begin_pointed_print(190, 3);
        monitor.suppress();
        assert_eq!(label.enter(&monitor, None), LabelAction::Hide);
        let encoded =
            postcard::to_allocvec(&(monitor.clone(), label.clone(), waiter.clone())).unwrap();
        let (mut restored, mut restored_label, restored_waiter): (Monitor, Label, LeaveWait) =
            postcard::from_bytes(&encoded).unwrap();
        assert_eq!(restored.observe(Some(190), false), None);
        assert_eq!(
            restored_label.wake(&restored, None, None, None),
            LabelAction::Hidden
        );
        assert_eq!(
            restored_waiter.wake(&restored, Some(200)),
            LeaveAction::Wait
        );
        restored.allow();
        assert_eq!(
            restored_label.wake(&restored, Some(190), Some(3), None),
            LabelAction::Redisplay
        );
        restored.completed_pointed(190).unwrap();
        assert_eq!(
            restored_waiter.wake(&restored, Some(200)),
            LeaveAction::Discard
        );
        assert_eq!(restored_waiter.wake(&restored, None), LeaveAction::Unpoint);
    }
    #[test]
    fn object_identity_and_drag_freeze_control_new_pointed_events() {
        let mut state = Monitor::default();
        assert_eq!(state.observe(Some(190), false), Some(190));
        assert_eq!(state.observe(Some(190), false), None);
        assert_eq!(state.observe(Some(200), true), None);
        assert_eq!(state.observe(None, true), None);
        assert_eq!(state.observe(Some(190), false), None);
        assert_eq!(state.observe(Some(200), false), Some(200));
        assert_eq!(state.observe(None, false), None);
        assert_eq!(state.observe(Some(200), false), Some(200));
    }
    #[test]
    fn repeated_pointed_completion_discards_only_waiters_still_over_same_item() {
        let mut state = Monitor::default();
        let old = state.completed_pointed(190).unwrap();
        assert_eq!(old.wake(&state, Some(190)), LeaveAction::Wait);
        let new = state.completed_pointed(190).unwrap();
        assert_eq!(old.wake(&state, Some(190)), LeaveAction::Discard);
        assert_eq!(old.wake(&state, Some(200)), LeaveAction::Unpoint);
        assert_eq!(new.wake(&state, Some(190)), LeaveAction::Wait);
        assert_eq!(new.wake(&state, None), LeaveAction::Unpoint);
        state.completed = u64::MAX;
        let before = state.clone();
        assert!(state.completed_pointed(190).is_err());
        assert_eq!(state, before);
    }
    #[test]
    fn pointed_print_clears_off_and_defers_departure_until_after_sleep() {
        let mut state = Monitor::default();
        state.suppress();
        let mut label = state.begin_pointed_print(190, 3);
        assert!(!state.suppressed());
        assert_eq!(label.enter(&state, None), LabelAction::Keep);
        assert_eq!(
            label.wake(&state, Some(200), Some(3), None),
            LabelAction::Finish
        );
        assert_eq!(label.enter(&state, Some(190)), LabelAction::Finish);
    }
    #[test]
    fn hidden_label_waits_for_clear_then_compares_cell_before_item_or_held_state() {
        let mut state = Monitor::default();
        let mut label = state.begin_pointed_print(190, 3);
        state.suppress();
        assert_eq!(label.enter(&state, None), LabelAction::Hide);
        assert_eq!(
            label.wake(&state, None, None, Some(190)),
            LabelAction::Hidden
        );
        state.allow();
        let mut moved = label.clone();
        assert_eq!(
            moved.wake(&state, Some(190), Some(4), None),
            LabelAction::Finish
        );
        // A different object in the same cell is displayed for one sleep, then
        // the ordinary object-identity test ends the original label.
        assert_eq!(
            label.wake(&state, Some(200), Some(3), None),
            LabelAction::Redisplay
        );
        assert_eq!(
            label.wake(&state, Some(200), Some(3), None),
            LabelAction::Finish
        );
        let mut held = state.begin_pointed_print(190, 3);
        state.suppress();
        assert_eq!(held.enter(&state, None), LabelAction::Hide);
        state.allow();
        assert_eq!(
            held.wake(&state, Some(190), Some(3), Some(190)),
            LabelAction::Redisplay
        );
        assert_eq!(
            held.wake(&state, Some(190), Some(3), Some(190)),
            LabelAction::Finish
        );
    }
}
