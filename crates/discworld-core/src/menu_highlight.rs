// SPDX-License-Identifier: GPL-3.0-or-later
//! Ownership of dialogs.cpp's single HL1 object, separate from cd.pointBox.
//! Input hit testing supplies the original box identity and whether its type
//! creates a highlight. A non-highlighted FLIP/RGROUP still becomes pointBox.

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Hover(u16),
    Effect,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    pointed: Option<u16>,
    owner: Option<Owner>,
}

impl State {
    pub fn owner(&self) -> Option<Owner> {
        self.owner
    }
    pub fn exists(&self) -> bool {
        self.owner.is_some()
    }

    /// Return true when invBoxes deletes/replaces HL1. Staying on the same box
    /// leaves a coroutine's graphic untouched; being outside removes it on
    /// every call, even if the preceding call was also outside.
    pub fn point_at(&mut self, box_at: Option<(u16, bool)>) -> bool {
        let Some((id, highlighted)) = box_at else {
            self.pointed = None;
            self.owner = None;
            return true;
        };
        if self.pointed == Some(id) {
            return false;
        }
        self.pointed = Some(id);
        self.owner = highlighted.then_some(Owner::Hover(id));
        true
    }

    /// ButtonPress/Toggle may replace HL1 without changing the pointed box.
    pub fn install_effect(&mut self) {
        self.owner = Some(Owner::Effect);
    }
    pub fn remove(&mut self) {
        self.owner = None;
    }
    /// popUpInventory resets pointBox as well as constructing fresh graphics.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn validate_snapshot(&self) -> Result<(), crate::iso::Error> {
        if let Some(Owner::Hover(id)) = self.owner {
            if self.pointed != Some(id) {
                return Err(crate::iso::Error("menu highlight differs from pointed box"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_and_effect_share_one_graphic_without_restarting_hover() {
        let mut state = State::default();
        assert!(state.point_at(Some((2, true))));
        assert_eq!(state.owner(), Some(Owner::Hover(2)));
        state.install_effect();
        assert!(!state.point_at(Some((2, true))));
        assert_eq!(state.owner(), Some(Owner::Effect));
        assert!(state.point_at(Some((3, true))));
        assert_eq!(state.owner(), Some(Owner::Hover(3)));
        // Losing the effect's own graphic does not imply missing HL1: another
        // hover object satisfies ButtonPress/Toggle's existence check.
        assert!(state.exists());
        state.install_effect();
        assert!(state.point_at(Some((4, false)))); // FLIP or save-list RGROUP
        assert!(!state.exists());
        state.install_effect();
        assert!(!state.point_at(Some((4, false))));
        assert!(state.exists());
        assert!(state.point_at(None));
        state.install_effect();
        assert!(state.point_at(None)); // outside again must delete it again
        assert!(!state.exists());
    }
    #[test]
    fn removal_preserves_pointed_box_and_snapshot_preserves_ownership() {
        let mut state = State::default();
        state.point_at(Some((5, true)));
        state.remove();
        assert!(!state.point_at(Some((5, true))));
        assert!(!state.exists());
        for owner in [None, Some(Owner::Effect), Some(Owner::Hover(5))] {
            state.owner = owner;
            state.validate_snapshot().unwrap();
            let bytes = postcard::to_allocvec(&state).unwrap();
            let copy: State = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(copy, state);
            copy.validate_snapshot().unwrap();
        }
        state.owner = Some(Owner::Hover(6));
        assert!(state.validate_snapshot().is_err());
        state.reset();
        assert!(state.point_at(Some((5, true))));
        assert!(state.exists());
    }
}
