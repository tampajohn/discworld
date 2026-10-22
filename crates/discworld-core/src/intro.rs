// SPDX-License-Identifier: GPL-3.0-or-later
//! DOS V1 intro policy from pinned handle.cpp, scene.cpp and tinsel.cpp.
//! Engine dispatch and snapshot integration are separate from this policy.
use crate::{graphics::Handle, iso::Error, resource::IndexEntry, scene_hook::Request};

/// First case-insensitive `title.` prefix, matching the DOS handle-table scan.
/// Handle zero disables detection in the reference; PSX's `turtle.` is excluded.
pub fn title_scene(index: &[IndexEntry]) -> Result<Option<Handle>, Error> {
    let Some(id) = index.iter().position(|entry| {
        entry
            .name
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("title."))
    }) else {
        return Ok(None);
    };
    if id > (u32::MAX >> 23) as usize {
        return Err(Error("intro title handle overflow"));
    }
    Ok((id != 0).then_some(Handle((id as u32) << 23)))
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Intro {
    title: Option<Handle>,
    active: bool,
    initial_escape: Option<u64>,
}
impl Intro {
    pub fn new(title: Option<Handle>) -> Result<Self, Error> {
        if title.is_some_and(|h| h.0 == 0 || h.offset() != 0) {
            return Err(Error("invalid intro title handle"));
        }
        Ok(Self {
            title,
            active: true,
            initial_escape: None,
        })
    }
    /// While true, the interpreter must ignore both ESCON and ESCOFF.
    pub fn active(&self) -> bool {
        self.active && self.title.is_some()
    }
    /// At scene-script creation, capture the initial event once and supply
    /// this scene interpreter's escape generation. Title scripts are unarmed.
    pub fn scene_script(&mut self, code: Handle, event: u64) -> Result<Option<u64>, Error> {
        if event == 0 {
            return Err(Error("zero intro escape generation"));
        }
        if !self.active() {
            return Ok(None);
        }
        self.initial_escape.get_or_insert(event);
        if self
            .title
            .is_some_and(|title| title.resource() == code.resource())
        {
            self.end();
            Ok(None)
        } else {
            Ok(Some(event))
        }
    }
    /// Apply after scene-hook interception, not immediately at the input event.
    pub fn redirect(&self, request: Request, event: u64) -> Request {
        if self.active() && self.initial_escape.is_some_and(|initial| initial != event) {
            Request {
                scene: self.title.unwrap(),
                entrance: 1,
                transition: 0,
            }
        } else {
            request
        }
    }
    /// Title entry, user game-load and debugger scene entry end intro mode.
    pub fn end(&mut self) {
        self.active = false;
    }
    /// Game restart clears the baseline but keeps resource-name detection.
    pub fn reset(&mut self) {
        self.active = true;
        self.initial_escape = None;
    }
    pub fn validate(&self, event: u64) -> Result<(), Error> {
        if event == 0
            || self.title.is_some_and(|h| h.0 == 0 || h.offset() != 0)
            || self
                .initial_escape
                .is_some_and(|initial| initial == 0 || initial > event)
            || (self.title.is_none() && self.initial_escape.is_some())
        {
            return Err(Error("invalid intro checkpoint"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entries(names: &[&str]) -> Vec<IndexEntry> {
        names
            .iter()
            .map(|name| IndexEntry {
                name: (*name).into(),
                size: 0,
                flags: 0,
            })
            .collect()
    }
    #[test]
    fn dos_title_detection_uses_first_prefix_and_zero_failsafe() {
        assert_eq!(
            title_scene(&entries(&["dw.scn", "TITLE.SCN", "title.dat"])).unwrap(),
            Some(Handle(1 << 23))
        );
        assert_eq!(
            title_scene(&entries(&["title.scn", "TITLE.DAT"])).unwrap(),
            None
        );
        assert_eq!(
            title_scene(&entries(&["dw.scn", "turtle.scn", "titles.scn"])).unwrap(),
            None
        );
        let mut large = entries(&vec!["other.scn"; 513]);
        large[512].name = "title.scn".into();
        assert!(title_scene(&large).is_err());
    }
    #[test]
    fn skip_survives_scene_boundaries_until_title_and_roundtrips() {
        let title = Handle(11 << 23);
        let mut intro = Intro::new(Some(title)).unwrap();
        let request = Request::new(Handle(3 << 23), 7, 2).unwrap();
        assert_eq!(intro.redirect(request, 2), request, "no scene baseline yet");
        assert_eq!(
            intro.scene_script(Handle((1 << 23) | 8), 2).unwrap(),
            Some(2)
        );
        assert_eq!(intro.redirect(request, 2), request);
        let expected = Request::new(title, 1, 0).unwrap();
        assert_eq!(intro.redirect(request, 3), expected);
        // A later scene captures the current event but must not erase the
        // original baseline, or the skip would disappear between scenes.
        assert_eq!(
            intro.scene_script(Handle((2 << 23) | 8), 3).unwrap(),
            Some(3)
        );
        assert_eq!(intro.redirect(request, 3), expected);
        let bytes = postcard::to_allocvec(&intro).unwrap();
        let restored: Intro = postcard::from_bytes(&bytes).unwrap();
        restored.validate(3).unwrap();
        assert_eq!(restored, intro);
        assert_eq!(restored.redirect(request, 3), expected);
        assert_eq!(intro.scene_script(Handle(title.0 | 24), 3).unwrap(), None);
        assert!(!intro.active());
        assert_eq!(intro.redirect(request, 4), request);
        intro.reset();
        assert!(intro.active());
        assert_eq!(intro.redirect(request, 4), request);
        intro.scene_script(Handle(1 << 23), 4).unwrap();
        intro.end();
        assert_eq!(intro.redirect(request, 5), request);
    }
    #[test]
    fn missing_title_disables_policy_and_bad_checkpoints_are_rejected() {
        let mut missing = Intro::new(None).unwrap();
        assert!(!missing.active());
        assert_eq!(missing.scene_script(Handle(1 << 23), 1).unwrap(), None);
        missing.reset();
        assert!(!missing.active());
        assert!(Intro::new(Some(Handle(0))).is_err());
        assert!(Intro::new(Some(Handle((1 << 23) | 8))).is_err());
        let mut intro = Intro::new(Some(Handle(1 << 23))).unwrap();
        let before = intro.clone();
        assert!(intro.scene_script(Handle(2 << 23), 0).is_err());
        assert_eq!(intro, before);
        intro.initial_escape = Some(2);
        assert!(intro.validate(1).is_err());
        intro.initial_escape = Some(0);
        assert!(intro.validate(1).is_err());
        missing.initial_escape = Some(1);
        assert!(missing.validate(1).is_err());
    }
}
