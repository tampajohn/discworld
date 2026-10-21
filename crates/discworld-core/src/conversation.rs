// SPDX-License-Identifier: GPL-3.0-or-later
//! DOS V1 conversation lifecycle and topic order from tinlib.cpp/dialogs.cpp.
//! Rendering and CONVERSE scheduling are engine responsibilities.
use crate::{iso::Error, startup::Window};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Conversation {
    pub polygon: Option<usize>,
    /// SCANICON: -2 for the preamble, -1 for the postamble, otherwise item ID.
    pub icon: i32,
    pub hidden: bool,
}
impl Conversation {
    /// Returns the polygon whose CONVERSE preamble must be scheduled. Opening
    /// an already open conversation leaves its owner, contents and layout alone.
    pub fn open(
        &mut self,
        mode: i32,
        polygon: usize,
        window: &mut Window,
        permanent: &[i32],
    ) -> Result<Option<usize>, Error> {
        if !matches!(mode, 0 | 1 | 3) {
            return Err(Error("invalid conversation open mode"));
        }
        if self.polygon.is_some() {
            return Ok(None);
        }
        if permanent.len() > 10 {
            return Err(Error("permanent topic limit"));
        }
        match mode {
            0 => window.position[1] = 8,
            1 => window.position[1] = 150,
            // V1 convPos has no assignment for CONV_TOP.
            3 => {}
            _ => unreachable!(),
        }
        window.contents = permanent.to_vec();
        self.polygon = Some(polygon);
        self.icon = -2;
        self.hidden = false;
        Ok(Some(polygon))
    }
    /// CONVERSATION(END) closes without dispatching a postamble. It does not
    /// reset SCANICON or the contents/position retained by the window.
    pub fn end(&mut self) {
        self.polygon = None;
    }
    /// A visible window's close input schedules its CONVERSE postamble and
    /// closes the inventory. Hidden conversation windows ignore close input.
    pub fn close_input(&mut self) -> Option<usize> {
        if self.hidden {
            return None;
        }
        let polygon = self.polygon?;
        self.icon = -1;
        self.end();
        Some(polygon)
    }
    /// Selection is indexed into current contents, not interpreted as an item
    /// ID. The caller must dispatch CONVERSE on the returned owning polygon.
    pub fn select(&mut self, window: &Window, index: usize) -> Result<Option<usize>, Error> {
        let Some(polygon) = self.polygon.filter(|_| !self.hidden) else {
            return Ok(None);
        };
        let icon = *window
            .contents
            .get(index)
            .ok_or(Error("conversation topic index out of bounds"))?;
        self.icon = icon;
        Ok(Some(polygon))
    }
}
/// ADDTOPIC inserts before the last (goodbye) icon, rather than appending.
/// Empty contents would underflow V1's contents[-1]; reject instead of making
/// up a goodbye icon. The capacity here is the backing inventory array (150),
/// not the declared visible window size.
pub fn add_topic(window: &mut Window, icon: i32) -> Result<(), Error> {
    if window.contents.contains(&icon) {
        return Ok(());
    }
    if window.contents.len() >= 150 {
        return Err(Error("conversation contents limit"));
    }
    let at = window
        .contents
        .len()
        .checked_sub(1)
        .ok_or(Error("conversation has no goodbye icon"))?;
    window.contents.insert(at, icon);
    Ok(())
}
pub fn delete_topic(window: &mut Window, icon: i32) {
    if let Some(index) = window.contents.iter().position(|id| *id == icon) {
        window.contents.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Handle;
    fn window() -> Window {
        Window {
            title: Handle(0),
            capacity: 10,
            min: [1, 1],
            size: [3, 1],
            max: [10, 1],
            position: [20, 40],
            first_display: 0,
            contents: vec![1, 2],
        }
    }
    #[test]
    fn open_replaces_contents_and_duplicate_open_preserves_owner_and_layout() {
        let mut state = Conversation::default();
        let mut window = window();
        assert_eq!(state.open(1, 7, &mut window, &[10, 99]).unwrap(), Some(7));
        assert_eq!(
            (state.icon, window.position, window.contents.clone()),
            (-2, [20, 150], vec![10, 99])
        );
        let before = (state.clone(), window.clone());
        assert_eq!(state.open(0, 8, &mut window, &[20]).unwrap(), None);
        assert_eq!((state.clone(), window.clone()), before);
        state.end();
        state.open(3, 8, &mut window, &[99]).unwrap();
        assert_eq!(window.position, [20, 150]);
        state.end();
        state.open(0, 9, &mut window, &[99]).unwrap();
        assert_eq!(window.position, [20, 8]);
    }
    #[test]
    fn topics_precede_goodbye_deduplicate_and_delete_without_reordering() {
        let mut window = window();
        window.contents = vec![10, 99];
        add_topic(&mut window, 20).unwrap();
        add_topic(&mut window, 30).unwrap();
        add_topic(&mut window, 20).unwrap();
        assert_eq!(window.contents, [10, 20, 30, 99]);
        delete_topic(&mut window, 20);
        delete_topic(&mut window, 123);
        assert_eq!(window.contents, [10, 30, 99]);
        window.contents.clear();
        let before = window.clone();
        assert!(add_topic(&mut window, 20).is_err());
        assert_eq!(window, before);
        window.contents = (0..150).collect();
        let before = window.clone();
        assert!(add_topic(&mut window, 200).is_err());
        assert_eq!(window, before);
    }
    #[test]
    fn selection_and_close_preserve_preamble_postamble_and_hidden_rules() {
        let mut state = Conversation::default();
        let mut window = window();
        state.open(0, 42, &mut window, &[10, 99]).unwrap();
        assert_eq!(state.select(&window, 0).unwrap(), Some(42));
        assert_eq!(state.icon, 10);
        let before = state.clone();
        assert!(state.select(&window, 2).is_err());
        assert_eq!(state, before);
        state.hidden = true;
        assert_eq!(state.select(&window, 1).unwrap(), None);
        assert_eq!(state.close_input(), None);
        state.hidden = false;
        let mut restored = state.clone();
        assert_eq!(state.close_input(), Some(42));
        restored.close_input();
        assert_eq!(state, restored);
        assert_eq!(state.icon, -1);
        assert_eq!(state.close_input(), None);
        state.open(0, 5, &mut window, &[99]).unwrap();
        state.end();
        assert_eq!(state.icon, -2);
    }
    #[test]
    fn invalid_open_is_atomic() {
        let mut state = Conversation::default();
        let mut window = window();
        let before = (state.clone(), window.clone());
        assert!(state.open(2, 1, &mut window, &[]).is_err());
        assert!(state.open(0, 1, &mut window, &[0; 11]).is_err());
        assert_eq!((state, window), before);
    }
}
