// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 actor identity, lifetime, and scene startup state from actors.cpp.
use crate::{graphics::Handle, iso::Error, scene::Actor as Definition};

/// V1 actor tag hotspot: strict horizontal edges and inclusive vertical edges.
pub fn hotspot(bounds: [i64; 4], kind: i32, point: [i64; 2]) -> Result<Option<[i64; 2]>, Error> {
    let [left, mut top, right, mut bottom] = bounds;
    let quarter = match kind {
        0 => {
            let half = (bottom - top) >> 1;
            top += half;
            bottom -= half >> 1;
            0
        }
        1 => {
            let quarter = (bottom - top) >> 2;
            bottom -= quarter;
            quarter
        }
        2 => 0,
        _ => return Err(Error("invalid actor tag type")),
    };
    Ok(
        (point[0] > left && point[0] < right && point[1] >= top && point[1] <= bottom)
            .then_some([(left + right) / 2, top + quarter]),
    )
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub alive: bool,
    pub hidden: bool,
    pub completed: bool,
    pub position: [i32; 2],
    pub masking: i32,
    pub script: Handle,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Actors {
    entries: Vec<Actor>,
    revive_on_entry: bool,
}
impl Actors {
    pub fn new(count: usize) -> Result<Self, Error> {
        if count > 512 {
            return Err(Error("actor count limit"));
        }
        Ok(Self {
            entries: vec![
                Actor {
                    alive: true,
                    hidden: false,
                    completed: false,
                    position: [0; 2],
                    masking: 0,
                    script: Handle(0)
                };
                count
            ],
            revive_on_entry: false,
        })
    }
    pub fn get_mut(&mut self, id: u32) -> Result<&mut Actor, Error> {
        let index = id.checked_sub(1).ok_or(Error("invalid actor ID"))? as usize;
        self.entries
            .get_mut(index)
            .ok_or(Error("actor ID outside declared count"))
    }
    pub fn get(&self, id: u32) -> Result<&Actor, Error> {
        let index = id.checked_sub(1).ok_or(Error("invalid actor ID"))? as usize;
        self.entries
            .get(index)
            .ok_or(Error("actor ID outside declared count"))
    }
    pub fn disable(&mut self, id: u32) -> Result<(), Error> {
        let actor = self.get_mut(id)?;
        actor.alive = false;
        actor.position = [0, 0];
        Ok(())
    }
    /// V1 SETACTOR restarts only dead actors or actors whose script completed.
    pub fn enable(&mut self, id: u32) -> Result<Option<Handle>, Error> {
        let actor = self.get_mut(id)?;
        if actor.alive && !actor.completed {
            return Ok(None);
        }
        actor.alive = true;
        actor.hidden = false;
        actor.completed = false;
        Ok((actor.script.0 != 0).then_some(actor.script))
    }
    /// V1 ACTORSON persists and affects subsequent StartActor calls only.
    pub fn set_actors_on(&mut self) {
        self.revive_on_entry = true;
    }
    /// Returns live actor scripts in creation order. Changes are atomic.
    pub fn enter(&mut self, definitions: &[Definition]) -> Result<Vec<(u32, Handle)>, Error> {
        self.enter_with_startup(definitions, true)
    }
    /// Restored scenes install definitions without reviving or starting actors.
    pub fn enter_with_startup(
        &mut self,
        definitions: &[Definition],
        run_startup: bool,
    ) -> Result<Vec<(u32, Handle)>, Error> {
        let mut next = self.clone();
        for actor in &mut next.entries {
            actor.script = Handle(0);
            actor.position = [0; 2];
            actor.masking = 0;
        }
        let mut scripts = Vec::new();
        for definition in definitions {
            // V1 StartTaggedActors processes every record in order. Repeated
            // IDs are valid (ALSHOP actor 56 starts with a null-code record).
            let revive = next.revive_on_entry;
            let actor = next.get_mut(definition.id)?;
            actor.hidden = false;
            actor.completed = false;
            actor.masking = definition.masking;
            actor.script = definition.script;
            if run_startup && revive {
                actor.alive = true;
            }
            if run_startup && actor.alive && actor.script.0 != 0 {
                scripts.push((definition.id, actor.script));
            }
        }
        // ActorEvent queues an actor ID; ActorTinselProcess resolves its code
        // only when scheduled, after StartTaggedActors has read the full table.
        for (id, code) in &mut scripts {
            *code = next.get(*id)?.script;
            if code.0 == 0 {
                return Err(Error("queued actor startup has no final script"));
            }
        }
        *self = next;
        Ok(scripts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_actor_records_keep_startup_order_and_last_definition() {
        let mut actors = Actors::new(2).unwrap();
        let definitions = [
            Definition {
                id: 1,
                masking: 0,
                script: Handle(0),
            },
            Definition {
                id: 2,
                masking: 1,
                script: Handle(12),
            },
            Definition {
                id: 1,
                masking: 2,
                script: Handle(24),
            },
            Definition {
                id: 2,
                masking: 9,
                script: Handle(36),
            },
        ];
        assert_eq!(
            actors.enter(&definitions).unwrap(),
            [(2, Handle(36)), (1, Handle(24)), (2, Handle(36))]
        );
        assert_eq!(
            (
                actors.get(1).unwrap().masking,
                actors.get(1).unwrap().script
            ),
            (2, Handle(24))
        );
        assert_eq!(
            (
                actors.get(2).unwrap().masking,
                actors.get(2).unwrap().script
            ),
            (9, Handle(36))
        );
        actors.disable(2).unwrap();
        assert_eq!(actors.enter(&definitions).unwrap(), [(1, Handle(24))]);
        assert_eq!(actors.enable(2).unwrap(), Some(Handle(36)));
        let before = actors.clone();
        let mut invalid = definitions.to_vec();
        invalid.push(Definition {
            id: 3,
            masking: 0,
            script: Handle(48),
        });
        assert!(actors.enter(&invalid).is_err());
        assert_eq!(actors, before);
    }
    #[test]
    fn v1_hotspot_quarters_and_edge_rules() {
        let bounds = [10, 20, 30, 100];
        assert_eq!(hotspot(bounds, 0, [20, 60]).unwrap(), Some([20, 60]));
        assert!(hotspot(bounds, 0, [20, 59]).unwrap().is_none());
        assert!(hotspot(bounds, 0, [20, 81]).unwrap().is_none());
        assert_eq!(hotspot(bounds, 1, [20, 20]).unwrap(), Some([20, 40]));
        assert!(hotspot(bounds, 1, [20, 81]).unwrap().is_none());
        assert!(hotspot(bounds, 2, [20, 100]).unwrap().is_some());
        assert!(hotspot(bounds, 2, [10, 60]).unwrap().is_none());
        assert!(hotspot(bounds, 2, [30, 60]).unwrap().is_none());
        assert!(hotspot(bounds, 3, [20, 60]).is_err());
    }

    #[test]
    fn disabling_clears_position_and_preserves_restart_information() {
        let mut actors = Actors::new(1).unwrap();
        actors
            .enter(&[Definition {
                id: 1,
                masking: 2,
                script: Handle(24),
            }])
            .unwrap();
        actors.get_mut(1).unwrap().position = [20, 30];
        actors.disable(1).unwrap();
        assert!(!actors.get(1).unwrap().alive);
        assert_eq!(actors.get(1).unwrap().position, [0, 0]);
        assert_eq!(actors.get(1).unwrap().script, Handle(24));
        assert_eq!(actors.enable(1).unwrap(), Some(Handle(24)));
        assert!(actors.get(1).unwrap().alive);
        let before = actors.clone();
        assert!(actors.disable(0).is_err());
        assert_eq!(actors, before);
    }
    #[test]
    fn scene_start_preserves_life_and_resets_local_state() {
        let mut actors = Actors::new(2).unwrap();
        actors.get_mut(1).unwrap().alive = false;
        actors.get_mut(2).unwrap().position = [9, 8];
        let definitions = [
            Definition {
                id: 1,
                masking: 1,
                script: Handle(12),
            },
            Definition {
                id: 2,
                masking: 2,
                script: Handle(24),
            },
        ];
        assert_eq!(actors.enter(&definitions).unwrap(), [(2, Handle(24))]);
        assert_eq!(actors.get_mut(2).unwrap().position, [0, 0]);
        let before = actors.clone();
        assert!(
            actors
                .enter(&[Definition {
                    id: 3,
                    masking: 0,
                    script: Handle(0)
                }])
                .is_err()
        );
        assert_eq!(actors, before);
    }
    #[test]
    fn enabling_restarts_only_dead_or_completed_current_scene_scripts() {
        let mut actors = Actors::new(1).unwrap();
        actors
            .enter(&[Definition {
                id: 1,
                masking: 0,
                script: Handle(24),
            }])
            .unwrap();
        assert_eq!(actors.enable(1).unwrap(), None);
        actors.get_mut(1).unwrap().completed = true;
        actors.get_mut(1).unwrap().hidden = true;
        assert_eq!(actors.enable(1).unwrap(), Some(Handle(24)));
        assert!(!actors.get_mut(1).unwrap().hidden);
        assert_eq!(actors.enable(1).unwrap(), None);
        actors.get_mut(1).unwrap().alive = false;
        assert_eq!(actors.enable(1).unwrap(), Some(Handle(24)));
        actors.enter(&[]).unwrap();
        actors.get_mut(1).unwrap().alive = false;
        assert_eq!(
            actors.enable(1).unwrap(),
            None,
            "old scene scripts must not be restarted"
        );
        assert!(actors.get_mut(1).unwrap().alive);
        assert!(actors.enable(0).is_err());
        assert!(actors.enable(2).is_err());
    }
}
