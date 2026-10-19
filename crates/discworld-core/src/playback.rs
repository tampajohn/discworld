// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 normal/talk film arbitration from actors.cpp and t1PlayReel in play.cpp.
use crate::graphics::Handle;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ActorFilms {
    latest: Option<Handle>,
    normal: Option<Handle>,
    talk: Option<Handle>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Playback(BTreeMap<i32, ActorFilms>);
impl Playback {
    /// NewestFilm runs when a film is scheduled, before its reel processes start.
    pub fn schedule(&mut self, actor: i32, film: Handle) {
        self.0.entry(actor).or_default().latest = Some(film);
    }
    /// Returns whether this scheduled reel should start displaying now.
    pub fn start(&mut self, actor: i32, film: Handle) -> bool {
        let state = self.0.entry(actor).or_default();
        if state.latest != Some(film) {
            if state.talk.is_some() {
                state.normal = Some(film);
            }
            return false;
        }
        if let Some(talk) = state.talk {
            if film != talk {
                state.normal = Some(film);
                return false;
            }
        } else {
            state.normal = Some(film);
        }
        true
    }
    pub fn begin_talk(&mut self, actor: i32, film: Handle) {
        self.0.entry(actor).or_default().talk = Some(film);
    }
    /// FinishTalkingReel starts the saved film from the beginning, not a saved frame.
    pub fn finish_talk(&mut self, actor: i32) -> Option<Handle> {
        let state = self.0.entry(actor).or_default();
        state.talk = None;
        state.normal
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normal_requests_during_talk_are_deferred_and_latest_is_restored() {
        let mut p = Playback::default();
        p.schedule(7, Handle(100));
        assert!(p.start(7, Handle(100)));
        p.begin_talk(7, Handle(200));
        p.schedule(7, Handle(200));
        assert!(p.start(7, Handle(200)));
        p.schedule(7, Handle(300));
        assert!(!p.start(7, Handle(300)));
        let mut snapshot = p.clone();
        assert_eq!(p.finish_talk(7), Some(Handle(300)));
        assert_eq!(snapshot.finish_talk(7), Some(Handle(300)));
        assert_eq!(p, snapshot);
        p.schedule(7, Handle(300));
        assert!(p.start(7, Handle(300)));
    }
    #[test]
    fn stale_scheduled_reels_follow_v1_talking_rules_and_scene_reset() {
        let mut p = Playback::default();
        p.schedule(1, Handle(10));
        p.schedule(1, Handle(20));
        assert!(!p.start(1, Handle(10)));
        assert!(p.start(1, Handle(20)));
        p.begin_talk(1, Handle(30));
        p.schedule(1, Handle(30));
        assert!(!p.start(1, Handle(10)));
        assert!(p.start(1, Handle(30)));
        assert_eq!(p.finish_talk(1), Some(Handle(10)));
        p.clear();
        assert_eq!(p.finish_talk(1), None);
    }
}
