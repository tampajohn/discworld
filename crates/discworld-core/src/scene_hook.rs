// SPDX-License-Identifier: GPL-3.0-or-later
//! Single-disc V1 scene interception from pinned tinsel.cpp SetNewScene.
use crate::{graphics::Handle, iso::Error};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub scene: Handle,
    pub entrance: i32,
    pub transition: i32,
}

impl Request {
    pub fn new(scene: Handle, entrance: i32, transition: i32) -> Result<Self, Error> {
        if !(0..=2).contains(&transition) {
            return Err(Error("invalid transition mode"));
        }
        Ok(Self {
            scene,
            entrance,
            transition,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone, PartialEq, Eq)]
pub struct Hooks {
    hook: Option<Request>,
    delayed: Option<Request>,
    suspended: bool,
}

impl Hooks {
    pub fn set(&mut self, request: Request) {
        self.hook = (request.scene.0 != 0).then_some(request);
    }

    pub fn clear(&mut self) {
        self.hook = None;
    }

    pub fn suspend(&mut self) {
        self.suspended = true;
    }

    pub fn resume(&mut self) {
        self.suspended = false;
    }

    pub fn intercept(&mut self, request: Request) -> Request {
        if !self.suspended {
            if let Some(hook) = self.hook.take() {
                self.delayed = Some(request);
                return hook;
            }
        }
        request
    }

    /// UNHOOKSCENE schedules the delayed destination directly, without applying
    /// another hook that may have been installed in the intervening scene.
    pub fn release(&mut self) -> Result<Request, Error> {
        self.delayed
            .take()
            .ok_or(Error("no delayed scene to unhook"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: u32, transition: i32) -> Request {
        Request::new(Handle(id << 23), id as i32, transition).unwrap()
    }

    #[test]
    fn hook_is_consumed_once_and_delayed_destination_survives_clear() {
        let mut hooks = Hooks::default();
        hooks.set(request(1, 2));
        assert_eq!(hooks.intercept(request(2, 1)), request(1, 2));
        assert_eq!(hooks.intercept(request(3, 0)), request(3, 0));
        hooks.set(request(4, 0));
        hooks.clear();
        assert_eq!(hooks.release().unwrap(), request(2, 1));
        let before = hooks.clone();
        assert!(hooks.release().is_err());
        assert_eq!(hooks, before);
    }

    #[test]
    fn interruption_preserves_hook_and_release_bypasses_new_hook() {
        let mut hooks = Hooks::default();
        hooks.set(request(1, 2));
        hooks.suspend();
        assert_eq!(hooks.intercept(request(2, 1)), request(2, 1));
        hooks.resume();
        assert_eq!(hooks.intercept(request(3, 0)), request(1, 2));
        hooks.set(request(4, 1));
        let mut restored = hooks.clone();
        for h in [&mut hooks, &mut restored] {
            assert_eq!(h.release().unwrap(), request(3, 0));
            assert_eq!(h.intercept(request(5, 2)), request(4, 1));
            assert_eq!(h.release().unwrap(), request(5, 2));
        }
        assert_eq!(hooks, restored);
        hooks.set(request(1, 0));
        hooks.set(request(0, 0));
        assert_eq!(hooks.intercept(request(2, 0)), request(2, 0));
    }
}
