// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit cooperative queue, following the basic ScummVM coroutine traversal.
//! Hosts dispatch one process at a time; VM state remains owned by this queue.
//! Explicit reschedule/giveWay and event objects are not implemented yet.
use crate::iso::Error;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessId(u64);

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct Process<T> {
    id: ProcessId,
    scene: Option<u64>,
    sleep: u32,
    state: T,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Scheduler<T> {
    processes: Vec<Process<T>>,
    next_id: u64,
    cursor: usize,
    current: Option<ProcessId>,
    running: bool,
    dispatches: usize,
}

impl<T> Default for Scheduler<T> {
    fn default() -> Self {
        Self {
            processes: Vec::new(),
            next_id: 1,
            cursor: 0,
            current: None,
            running: false,
            dispatches: 0,
        }
    }
}

impl<T> Scheduler<T> {
    /// Runtime snapshots are only valid between complete scheduler passes.
    pub(crate) fn validate_snapshot_structure(&self) -> Result<(), Error> {
        if self.running || self.current.is_some() || self.cursor > self.processes.len() {
            return Err(Error("snapshot scheduler is not idle"));
        }
        if self.processes.len() > 1024 || self.dispatches > 10000 || self.next_id == 0 {
            return Err(Error("invalid snapshot scheduler limits"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for process in &self.processes {
            if process.id.0 == 0
                || process.id.0 >= self.next_id
                || !ids.insert(process.id.0)
                || process.sleep == 0
            {
                return Err(Error("invalid snapshot process identity or sleep"));
            }
        }
        Ok(())
    }

    pub fn current_id(&self) -> Option<ProcessId> {
        self.current
    }
    pub fn entries(&self) -> impl Iterator<Item = (ProcessId, &T)> {
        self.processes.iter().map(|p| (p.id, &p.state))
    }
    pub(crate) fn states_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.processes.iter_mut().map(|p| &mut p.state)
    }
    pub fn states(&self) -> impl Iterator<Item = &T> {
        self.processes.iter().map(|p| &p.state)
    }
    pub fn is_empty(&self) -> bool {
        self.processes.is_empty()
    }

    /// Stop a diagnostic pass after a dispatch yielded. Remaining peers are
    /// retained without decrementing their sleeps. Normal passes drain ready().
    pub fn end_pass(&mut self) -> Result<(), Error> {
        if self.current.is_some() {
            return Err(Error("dispatch must finish before ending pass"));
        }
        self.running = false;
        self.cursor = 0;
        Ok(())
    }
    pub fn spawn(&mut self, scene: Option<u64>, state: T) -> Result<ProcessId, Error> {
        if self.running && self.current.is_none() {
            return Err(Error("spawn requires active dispatch or idle scheduler"));
        }
        let index = if self.current.is_some() {
            self.cursor + 1
        } else {
            0
        };
        self.insert(index, scene, state)
    }

    /// Insert a migrated native process without moving existing peers or
    /// changing their sleep counts. None appends at the tail. Unlike spawn,
    /// this operation is only valid between complete scheduler passes.
    pub(crate) fn insert_idle_before(
        &mut self,
        before: Option<ProcessId>,
        scene: Option<u64>,
        state: T,
    ) -> Result<ProcessId, Error> {
        if self.running || self.current.is_some() {
            return Err(Error("migration requires idle scheduler"));
        }
        let index = match before {
            Some(id) => self
                .processes
                .iter()
                .position(|p| p.id == id)
                .ok_or(Error("missing migration insertion point"))?,
            None => self.processes.len(),
        };
        self.insert(index, scene, state)
    }

    fn insert(&mut self, index: usize, scene: Option<u64>, state: T) -> Result<ProcessId, Error> {
        if self.processes.len() >= 1024 {
            return Err(Error("process capacity exceeded"));
        }
        let next = self
            .next_id
            .checked_add(1)
            .ok_or(Error("process identity overflow"))?;
        let id = ProcessId(self.next_id);
        self.processes.insert(
            index,
            Process {
                id,
                scene,
                sleep: 1,
                state,
            },
        );
        self.next_id = next;
        Ok(id)
    }

    pub fn begin(&mut self) -> Result<(), Error> {
        if self.running {
            return Err(Error("scheduler pass already active"));
        }
        self.running = true;
        self.cursor = 0;
        self.dispatches = 0;
        Ok(())
    }

    /// Obtain the next ready process, decrementing sleep before dispatch.
    /// Finish the current dispatch before asking for another process.
    pub fn ready(&mut self) -> Result<Option<ProcessId>, Error> {
        if !self.running || self.current.is_some() {
            return Err(Error("invalid scheduler dispatch state"));
        }
        if self.dispatches >= 10000 {
            return Err(Error("scheduler dispatch budget"));
        }
        while let Some(process) = self.processes.get_mut(self.cursor) {
            process.sleep = process.sleep.saturating_sub(1);
            if process.sleep == 0 {
                self.current = Some(process.id);
                self.dispatches += 1;
                return Ok(self.current);
            }
            self.cursor += 1;
        }
        self.running = false;
        Ok(None)
    }

    pub(crate) fn sleep_for(&self, id: ProcessId) -> Option<u32> {
        self.processes.iter().find(|p| p.id == id).map(|p| p.sleep)
    }

    pub fn state_mut(&mut self, id: ProcessId) -> Option<&mut T> {
        self.processes
            .iter_mut()
            .find(|p| p.id == id)
            .map(|p| &mut p.state)
    }

    /// None completes the process; a positive sleep yields it to a later pass.
    pub fn finish(&mut self, sleep: Option<u32>) -> Result<(), Error> {
        if self.current.is_none() || sleep == Some(0) {
            return Err(Error("invalid scheduler completion"));
        }
        if let Some(sleep) = sleep {
            self.processes[self.cursor].sleep = sleep;
            self.cursor += 1;
        } else {
            self.processes.remove(self.cursor);
        }
        self.current = None;
        Ok(())
    }

    /// Cancel another process. The active process must use finish(None).
    pub fn cancel(&mut self, id: ProcessId) -> Result<bool, Error> {
        if self.current == Some(id) {
            return Err(Error("cannot cancel active dispatch"));
        }
        let Some(index) = self.processes.iter().position(|p| p.id == id) else {
            return Ok(false);
        };
        self.processes.remove(index);
        if index < self.cursor {
            self.cursor -= 1;
        }
        Ok(true)
    }

    /// Remove scene-local peers, retaining global processes and current dispatch.
    /// The caller completes its current process explicitly after a scene change.
    pub fn cancel_scene(&mut self, scene: u64) {
        let ids: Vec<_> = self
            .processes
            .iter()
            .filter(|p| p.scene == Some(scene) && Some(p.id) != self.current)
            .map(|p| p.id)
            .collect();
        for id in ids {
            self.cancel(id).expect("filtered active process");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_requires_idle_queue_and_unique_bounded_identities() {
        let mut valid = Scheduler::default();
        valid.spawn(None, 1).unwrap();
        valid.spawn(None, 2).unwrap();
        assert!(valid.validate_snapshot_structure().is_ok());
        let mut invalid = valid.clone();
        invalid.processes[1].id = invalid.processes[0].id;
        assert!(invalid.validate_snapshot_structure().is_err());
        for id in [0, valid.next_id, u64::MAX] {
            let mut invalid = valid.clone();
            invalid.processes[0].id = ProcessId(id);
            assert!(invalid.validate_snapshot_structure().is_err());
        }
        let mut invalid = valid.clone();
        invalid.processes[0].sleep = 0;
        assert!(invalid.validate_snapshot_structure().is_err());
        let mut invalid = valid.clone();
        invalid.cursor = usize::MAX;
        assert!(invalid.validate_snapshot_structure().is_err());
        valid.begin().unwrap();
        assert!(valid.validate_snapshot_structure().is_err());
        valid.ready().unwrap();
        assert!(valid.validate_snapshot_structure().is_err());
        valid.finish(Some(2)).unwrap();
        assert!(valid.validate_snapshot_structure().is_err());
        valid.end_pass().unwrap();
        assert!(valid.validate_snapshot_structure().is_ok());
    }

    #[test]
    fn idle_migration_preserves_peers_sleeps_and_rejects_live_insertion() {
        let mut q = Scheduler::default();
        let tail = q.spawn(None, 1).unwrap();
        let head = q.spawn(None, 2).unwrap();
        q.begin().unwrap();
        assert_eq!(q.ready().unwrap(), Some(head));
        let before = q.clone();
        assert!(q.insert_idle_before(Some(tail), None, 3).is_err());
        assert_eq!(q, before);
        q.finish(Some(4)).unwrap();
        assert_eq!(q.ready().unwrap(), Some(tail));
        q.finish(Some(2)).unwrap();
        assert_eq!(q.ready().unwrap(), None);
        let point = q.insert_idle_before(Some(tail), None, 3).unwrap();
        let tag = q.insert_idle_before(Some(tail), None, 4).unwrap();
        assert_eq!(
            q.entries().map(|(id, _)| id).collect::<Vec<_>>(),
            [head, point, tag, tail]
        );
        assert_eq!(q.sleep_for(head), Some(4));
        assert_eq!(q.sleep_for(tail), Some(2));
        q.validate_snapshot_structure().unwrap();
        let appended = q.insert_idle_before(None, None, 5).unwrap();
        assert_eq!(q.entries().last().unwrap().0, appended);
        let before = q.clone();
        assert!(q.insert_idle_before(Some(ProcessId(999)), None, 6).is_err());
        assert_eq!(q, before);
    }

    #[test]
    fn spawned_children_run_same_pass_in_reverse_creation_order() {
        let mut q = Scheduler::default();
        let older = q.spawn(None, 1).unwrap();
        let parent = q.spawn(None, 2).unwrap();
        q.begin().unwrap();
        assert_eq!(q.ready().unwrap(), Some(parent));
        let first = q.spawn(Some(1), 3).unwrap();
        let second = q.spawn(Some(1), 4).unwrap();
        q.finish(None).unwrap();
        for id in [second, first, older] {
            assert_eq!(q.ready().unwrap(), Some(id));
            q.finish(None).unwrap();
        }
        assert_eq!(q.ready().unwrap(), None);
    }
    #[test]
    fn sleep_and_snapshot_preserve_wakeup_order() {
        let mut q = Scheduler::default();
        let id = q.spawn(None, 7).unwrap();
        q.begin().unwrap();
        assert_eq!(q.ready().unwrap(), Some(id));
        q.finish(Some(2)).unwrap();
        assert_eq!(q.ready().unwrap(), None);
        let mut restored = q.clone();
        for scheduler in [&mut q, &mut restored] {
            scheduler.begin().unwrap();
            assert_eq!(scheduler.ready().unwrap(), None);
            scheduler.begin().unwrap();
            assert_eq!(scheduler.ready().unwrap(), Some(id));
            *scheduler.state_mut(id).unwrap() += 1;
            scheduler.finish(None).unwrap();
            assert_eq!(scheduler.ready().unwrap(), None);
        }
        assert_eq!(q, restored);
    }
    #[test]
    fn scene_change_removes_peers_without_skipping_new_scene() {
        let mut q = Scheduler::default();
        let global = q.spawn(None, 0).unwrap();
        q.spawn(Some(1), 1).unwrap();
        let current = q.spawn(Some(1), 2).unwrap();
        q.begin().unwrap();
        assert_eq!(q.ready().unwrap(), Some(current));
        q.cancel_scene(1);
        let next = q.spawn(Some(2), 3).unwrap();
        q.finish(None).unwrap();
        assert_eq!(q.ready().unwrap(), Some(next));
        q.finish(None).unwrap();
        assert_eq!(q.ready().unwrap(), Some(global));
    }
    #[test]
    fn cancelling_earlier_process_preserves_current_position() {
        let mut q = Scheduler::default();
        let later = q.spawn(None, 0).unwrap();
        let earlier = q.spawn(None, 1).unwrap();
        q.begin().unwrap();
        assert_eq!(q.ready().unwrap(), Some(earlier));
        q.finish(Some(1)).unwrap();
        assert_eq!(q.ready().unwrap(), Some(later));
        q.cancel(earlier).unwrap();
        let child = q.spawn(None, 2).unwrap();
        q.finish(None).unwrap();
        assert_eq!(q.ready().unwrap(), Some(child));
    }
}
