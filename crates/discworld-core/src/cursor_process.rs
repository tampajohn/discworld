// SPDX-License-Identifier: GPL-3.0-or-later
//! Persistent V1 CursorProcess state, distinct from backend mouse coordinates.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct State {
    pub position: [i32; 2],
    pub hidden_wait: bool,
}
impl State {
    /// Resuming the hidden inner loop consumes a cycle even when another
    /// script has just unhidden the cursor. The outer sleep comes next.
    pub fn begin_cycle(&mut self, hidden: bool) -> bool {
        if self.hidden_wait {
            if !hidden {
                self.hidden_wait = false;
            }
            false
        } else {
            true
        }
    }
}
