//! Scheduler-owned V1 AllowDclick token. Hosts must not approximate this with
//! a wall-clock timer: acquiring the token kills the previous script process.
//! Integration with engine input dispatch and snapshot migration is pending.
use crate::{
    iso::Error,
    scheduler::{ProcessId, Scheduler},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClickToken {
    owner: Option<ProcessId>,
}
impl ClickToken {
    fn acquire<T>(&mut self, queue: &mut Scheduler<T>) -> Result<(), Error> {
        let current = queue
            .current_id()
            .ok_or(Error("click token requires active dispatch"))?;
        if self.owner == Some(current) {
            return Err(Error("click token already held by active process"));
        }
        if let Some(previous) = self.owner {
            if !queue.cancel(previous)? {
                return Err(Error("click token owner is missing"));
            }
        }
        self.owner = Some(current);
        Ok(())
    }

    /// Called on a single-click script's first dispatch. The returned sleep is
    /// passed to Scheduler::finish; on waking, release before running bytecode.
    pub fn single<T>(&mut self, queue: &mut Scheduler<T>, threshold: u32) -> Result<u32, Error> {
        let sleep = threshold
            .checked_add(1)
            .ok_or(Error("click delay overflow"))?;
        self.acquire(queue)?;
        Ok(sleep)
    }

    /// A double-click script cancels the pending single and immediately frees
    /// the token, then continues in the current dispatch without sleeping.
    pub fn double<T>(&mut self, queue: &mut Scheduler<T>) -> Result<(), Error> {
        self.acquire(queue)?;
        self.release(queue)
    }

    pub fn release<T>(&mut self, queue: &Scheduler<T>) -> Result<(), Error> {
        let current = queue
            .current_id()
            .ok_or(Error("click release requires active dispatch"))?;
        if self.owner != Some(current) {
            return Err(Error("click token released by non-owner"));
        }
        self.owner = None;
        Ok(())
    }

    /// Call when cancellation/completion removes a process, including scene
    /// teardown. Never transfer ownership to a recycled process identifier.
    pub fn terminated(&mut self, process: ProcessId) {
        if self.owner == Some(process) {
            self.owner = None;
        }
    }

    pub fn validate<T>(&self, queue: &Scheduler<T>) -> Result<(), Error> {
        if let Some(owner) = self.owner {
            if !queue.entries().any(|(id, _)| id == owner) {
                return Err(Error("click token owner is missing"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Phase {
    Single,
    Double,
    Waiting,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct State {
    token: ClickToken,
    pending: Vec<(ProcessId, Phase)>,
}
pub(crate) enum Dispatch {
    Ready,
    Sleep(u32),
    Resumed,
}
impl State {
    pub(crate) fn entries(&self) -> impl Iterator<Item = (ProcessId, Phase)> + '_ {
        self.pending.iter().copied()
    }
    pub(crate) fn schedule(&mut self, id: ProcessId, phase: Phase) {
        self.pending.push((id, phase));
    }
    pub(crate) fn prepare<T>(
        &mut self,
        queue: &mut Scheduler<T>,
        id: ProcessId,
    ) -> Result<Dispatch, Error> {
        let Some(index) = self.pending.iter().position(|(p, _)| *p == id) else {
            return Ok(Dispatch::Ready);
        };
        match self.pending[index].1 {
            Phase::Single => {
                let sleep = self
                    .token
                    .single(queue, crate::mouse::DEFAULT_DOUBLE_CLICK_TICKS)?;
                self.pending[index].1 = Phase::Waiting;
                self.cleanup(queue);
                Ok(Dispatch::Sleep(sleep))
            }
            Phase::Double => {
                self.token.double(queue)?;
                self.pending.remove(index);
                self.cleanup(queue);
                Ok(Dispatch::Ready)
            }
            Phase::Waiting => {
                self.token.release(queue)?;
                self.pending.remove(index);
                Ok(Dispatch::Resumed)
            }
        }
    }
    pub(crate) fn cleanup<T>(&mut self, queue: &Scheduler<T>) {
        self.pending
            .retain(|(p, _)| queue.entries().any(|(id, _)| id == *p));
        if let Some(owner) = self.token.owner {
            if !queue.entries().any(|(id, _)| id == owner) {
                self.token.terminated(owner);
            }
        }
    }
    pub(crate) fn validate<T>(&self, queue: &Scheduler<T>) -> Result<(), Error> {
        self.token.validate(queue)?;
        if self.pending.len() > 1024 {
            return Err(Error("too many pending clicks"));
        }
        for (index, (id, phase)) in self.pending.iter().enumerate() {
            let sleep_valid = match phase {
                Phase::Single | Phase::Double => queue.sleep_for(*id) == Some(1),
                Phase::Waiting => queue.sleep_for(*id).is_some_and(|sleep| {
                    (1..=crate::mouse::DEFAULT_DOUBLE_CLICK_TICKS + 1).contains(&sleep)
                }),
            };
            if !sleep_valid
                || !queue.entries().any(|(p, _)| p == *id)
                || self.pending[..index].iter().any(|(p, _)| p == id)
                || (*phase == Phase::Waiting) != (self.token.owner == Some(*id))
            {
                return Err(Error("invalid pending click process"));
            }
        }
        if self.token.owner.is_some_and(|owner| {
            !self
                .pending
                .iter()
                .any(|(id, phase)| *id == owner && *phase == Phase::Waiting)
        }) {
            return Err(Error("missing pending click wait"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_waits_exact_scheduler_passes_and_survives_serialization() {
        let mut queue = Scheduler::default();
        let script = queue.spawn(Some(1), 42u32).unwrap();
        let mut token = ClickToken::default();
        queue.begin().unwrap();
        assert_eq!(queue.ready().unwrap(), Some(script));
        let sleep = token.single(&mut queue, 10).unwrap();
        assert_eq!(sleep, 11);
        queue.finish(Some(sleep)).unwrap();
        assert_eq!(queue.ready().unwrap(), None);
        let bytes = postcard::to_allocvec(&(queue, token)).unwrap();
        let (mut queue, mut token): (Scheduler<u32>, ClickToken) =
            postcard::from_bytes(&bytes).unwrap();
        token.validate(&queue).unwrap();
        for _ in 0..10 {
            queue.begin().unwrap();
            assert_eq!(queue.ready().unwrap(), None);
        }
        queue.begin().unwrap();
        assert_eq!(queue.ready().unwrap(), Some(script));
        token.release(&queue).unwrap();
        queue.finish(None).unwrap();
        assert_eq!(queue.ready().unwrap(), None);
        assert!(queue.is_empty());
        token.validate(&queue).unwrap();
    }

    #[test]
    fn double_cancels_sleeping_single_without_losing_neighbor_dispatch() {
        let mut queue = Scheduler::default();
        let neighbor = queue.spawn(None, 1).unwrap();
        let single = queue.spawn(Some(1), 2).unwrap();
        let mut token = ClickToken::default();
        queue.begin().unwrap();
        assert_eq!(queue.ready().unwrap(), Some(single));
        let sleep = token.single(&mut queue, 10).unwrap();
        queue.finish(Some(sleep)).unwrap();
        assert_eq!(queue.ready().unwrap(), Some(neighbor));
        let double = queue.spawn(Some(1), 3).unwrap();
        queue.finish(Some(1)).unwrap();
        assert_eq!(queue.ready().unwrap(), Some(double));
        token.double(&mut queue).unwrap(); // Cancels an earlier queue entry.
        assert!(queue.state_mut(single).is_none());
        queue.finish(None).unwrap();
        assert_eq!(queue.ready().unwrap(), None);
        queue.begin().unwrap();
        assert_eq!(queue.ready().unwrap(), Some(neighbor));
        queue.finish(None).unwrap();
        assert_eq!(queue.ready().unwrap(), None);
        token.validate(&queue).unwrap();
    }

    #[test]
    fn invalid_acquisition_is_atomic_and_teardown_releases_owner() {
        let mut queue = Scheduler::default();
        let id = queue.spawn(Some(1), 1).unwrap();
        let mut token = ClickToken::default();
        assert!(token.single(&mut queue, 10).is_err());
        queue.begin().unwrap();
        queue.ready().unwrap();
        assert!(token.single(&mut queue, u32::MAX).is_err());
        let sleep = token.single(&mut queue, 10).unwrap();
        let before = (queue.clone(), token.clone());
        assert!(token.single(&mut queue, 10).is_err());
        assert_eq!((queue.clone(), token.clone()), before);
        queue.finish(Some(sleep)).unwrap();
        queue.ready().unwrap();
        queue.cancel_scene(1);
        assert!(token.validate(&queue).is_err());
        token.terminated(id);
        token.validate(&queue).unwrap();
    }
}
