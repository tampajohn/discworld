// SPDX-License-Identifier: GPL-3.0-or-later
//! Headless V1 runtime with queued scene, actor, and polygon scripts.
//! Unsupported library operations remain explicit compatibility stops.
use crate::{
    animation::{Animation, Tick},
    audio::{SampleBank, SamplePlayer},
    compositor::Surface,
    film::{Film, word},
    graphics::{Handle, Resources},
    iso::Error,
    resource::chunks,
    scene,
    scheduler::Scheduler,
    script::{LibraryCall, Machine, Step},
    startup::Declarations,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Running,
    Unsupported(LibraryCall),
    Halted,
}
/// Original V1 EVENT values for resolved player interactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum PolygonInteraction {
    WalkTo = 1,
    Action = 2,
    Look = 3,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyWait {
    call: LibraryCall,
    events: u64,
    cursor: [i32; 2],
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct PrintWait {
    inventory: bool,
    restore_control: bool,
    call: LibraryCall,
    consumed: usize,
    layout: Option<crate::text::Layout>,
    remaining: u32,
    sample_serial: Option<u64>,
    timeout: u32,
    left_events: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct TalkWait {
    mover_saved: Option<Handle>,
    call: LibraryCall,
    actor: i32,
    text: Vec<u8>,
    sample: Option<u64>,
    layout: Option<crate::text::Layout>,
    initialized: bool,
    remaining: u32,
    timeout: u32,
    left_events: u64,
    origin: [i32; 2],
    restore_control: bool,
    restore_tags: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScriptProcess {
    walked_wait: Option<WalkedWait>,
    restored_context: bool,
    effect_actor: Option<i32>,
    player_walk: Option<(i32, [i32; 2])>,
    inventory_object: Option<i32>,
    object_print_position: Option<[i32; 2]>,
    conversation_started: bool,
    conversation_took_control: bool,
    polygon: Option<usize>,
    check_control_on_start: bool,
    walk_wait: Option<(LibraryCall, i32, u64, bool)>,
    tag_walk_wait: Option<(LibraryCall, i32, u64, Handle)>,
    talk_wait: Option<TalkWait>,
    scroll_wait: Option<(LibraryCall, u64, [i32; 2])>,
    play_wait: Option<(LibraryCall, Animation)>,
    actor: Option<u32>,
    print_wait: Option<PrintWait>,
    key_wait: Option<KeyWait>,
    music_wait: Option<LibraryCall>,
    event: i32,
    vm: Machine,
    code: Handle,
    wait: Option<(LibraryCall, u32)>,
    sample_wait: Option<LibraryCall>,
}
impl ScriptProcess {
    fn new(code: Handle) -> Self {
        Self {
            walked_wait: None,
            restored_context: false,
            effect_actor: None,
            player_walk: None,
            inventory_object: None,
            object_print_position: None,
            conversation_started: false,
            conversation_took_control: false,
            polygon: None,
            check_control_on_start: false,
            walk_wait: None,
            tag_walk_wait: None,
            play_wait: None,
            talk_wait: None,
            scroll_wait: None,
            actor: None,
            print_wait: None,
            key_wait: None,
            music_wait: None,
            event: 6, // T1_STARTUP; engine events use the V1 script numbering.
            vm: Machine::default(),
            code,
            wait: None,
            sample_wait: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WalkedWait {
    call: LibraryCall,
    actor: i32,
    target: [i32; 2],
    delay: u32,
    serial: Option<u64>,
}

/// Interpreter-only scene state. Host waits, animations, and scheduler sleeps
/// are rebuilt rather than resumed from their old coroutine locals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneScripts {
    scene: Handle,
    contexts: Vec<SavedScript>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SavedScript {
    code: Handle,
    vm: Machine,
    actor: Option<u32>,
    polygon: Option<usize>,
    inventory_object: Option<i32>,
    effect_actor: Option<i32>,
    event: i32,
}

/// V1 SaveMovers records reel banks even for movers absent from this scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMovers {
    scene: Handle,
    banks: std::collections::BTreeMap<i32, [[Handle; 12]; 10]>,
    active: Vec<(i32, [i32; 2], Handle)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneSnapshot {
    scripts: SceneScripts,
    movers: SceneMovers,
    reels: Vec<(i32, bool, crate::animation::ReelSource)>,
    background: Option<Handle>,
    polygons: crate::polygon::EnabledMemory,
    origin: [i32; 2],
    boundaries: crate::scroll::Boundaries,
    control: bool,
    no_blocking: bool,
    music: crate::music_player::MidiFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SceneRestore {
    snapshot: SceneSnapshot,
    remaining: u8,
    fade: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneCall {
    pub tick: u64,
    pub script: Handle,
    pub call: LibraryCall,
    pub operands: Vec<i32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    saved_scenes: Vec<SceneSnapshot>,
    scene_restore: Option<SceneRestore>,
    master_stopped: bool,
    pub conversation: crate::conversation::Conversation,
    inventory_display: Option<(
        crate::conversation_window::Frame,
        crate::conversation_window::Content,
    )>,
    inventory_dirty: bool,
    pub top_window: Option<crate::conversation_window::Frame>,
    /// Optional diagnostic history of the last 256 scene-process library
    /// dispatches. Failed transactional ticks do not commit their trace.
    pub scene_calls: Option<std::collections::VecDeque<SceneCall>>,
    /// Scene-local V1 NOBLOCKING disables mover/mover collisions only.
    pub no_blocking: bool,
    pub inventory_objects: std::collections::BTreeMap<i32, crate::inventory::Object>,
    /// V1 INV_NOICON is -1. Held item identity survives scene changes.
    pub held_object: i32,
    held_cursor: Option<crate::inventory::HeldCursor>,
    last_interaction_tick: Option<u64>,
    pub movers: std::collections::BTreeMap<i32, crate::mover::Mover>,
    polygon_memory: crate::polygon::EnabledMemory,
    polygon_table: Handle,
    playback: crate::playback::Playback,
    pub scroll: crate::scroll::Scroll,
    scroll_maximum: [i32; 2],
    pub world_origin: [i32; 2],
    pending_scene: Option<(Handle, i32, i32, Option<u8>)>,
    scene_hooks: crate::scene_hook::Hooks,
    fade_multiplier: u32,
    fade_in_step: Option<usize>,
    fade_on_background: bool,
    reel_owners: std::collections::BTreeMap<i32, (Handle, u64)>,
    next_playback: u64,
    pub polygons: crate::polygon::Polygons,
    pub random: crate::random::Random,
    pub actors: crate::actor::Actors,
    actor_tags: std::collections::BTreeMap<u32, (Handle, i32)>,
    actor_effects: std::collections::BTreeMap<i32, usize>,
    pub music: crate::music_player::MusicPlayer,
    pub music_frame: Vec<crate::sequencer::Message>,
    pub declarations: Declarations,
    pub globals: Vec<i32>,
    pub scene: Handle,
    pub ticks: u64,
    timers: crate::timers::Timers,
    user_events: u64,
    left_events: u64,
    pub subtitles: bool,
    talk_color: Option<[u8; 3]>,
    pub background: Option<Animation>,
    background_film: Option<Handle>,
    cursor_animation: Option<Animation>,
    pub films: Vec<Animation>,
    master: Machine,
    master_code: Handle,
    master_wait: Option<(LibraryCall, u32)>,
    processes: Scheduler<ScriptProcess>,
    generation: u64,
    pub control: crate::control::Control,
    /// V1 startup permission to open the configuration menu while control is off.
    /// Actual menu activation/rendering is not yet implemented by a host.
    pub menu_enabled: bool,
    active_inventory: Option<i32>,
    inventory_event_index: usize,
    pub sample_player: SamplePlayer,
    pub actor_steps: std::collections::BTreeMap<i32, u32>,
    pub audio_frame: Vec<i16>,
}

fn args(vm: &Machine, n: usize) -> Result<&[i32], Error> {
    let a = vm.operands();
    a.get(
        a.len()
            .checked_sub(n)
            .ok_or(Error("engine argument underflow"))?..,
    )
    .ok_or(Error("engine argument underflow"))
}
impl Engine {
    fn save_scene_call(&mut self, current: Option<&ScriptProcess>) -> Result<bool, Error> {
        if self.saved_scenes.len() >= 4 {
            return Err(Error("saved scene nesting limit"));
        }
        if self
            .saved_scenes
            .last()
            .is_some_and(|s| s.scripts.scene == self.scene)
        {
            return Ok(false);
        }
        let active = if let Some(process) = current {
            let id = self
                .processes
                .current_id()
                .ok_or(Error("scene save outside active dispatch"))?;
            let slot = self
                .processes
                .state_mut(id)
                .ok_or(Error("missing scene-save caller"))?;
            Some((id, std::mem::replace(slot, process.clone())))
        } else {
            None
        };
        let snapshot = self.capture_scene();
        if let Some((id, old)) = active {
            *self.processes.state_mut(id).unwrap() = old;
        }
        self.saved_scenes.push(snapshot?);
        self.scene_hooks.suspend();
        Ok(true)
    }

    fn request_scene_restore(&mut self, fade: bool) -> Result<(), Error> {
        self.scene_hooks.resume();
        if self.scene_restore.is_none() {
            let snapshot = self
                .saved_scenes
                .pop()
                .ok_or(Error("no saved scene to restore"))?;
            self.scene_restore = Some(SceneRestore {
                snapshot,
                remaining: if fade { 11 } else { 5 },
                fade,
            });
        }
        Ok(())
    }

    fn advance_scene_restore(
        &mut self,
        resources: &Resources<'_>,
        tracks: Option<&[crate::music::Track<'_>]>,
    ) -> Result<(), Error> {
        let Some(mut restore) = self.scene_restore.take() else {
            return Ok(());
        };
        match restore.remaining {
            6..=11 => {
                self.fade_in_step = None;
                self.fade_multiplier =
                    [0xd000, 0xa000, 0x7000, 0x4000, 0x1000, 0][(11 - restore.remaining) as usize];
            }
            5 => {
                self.rebuild_scene_objects(resources, &restore.snapshot)?;
                if restore.fade {
                    self.fade_multiplier = 0;
                    self.fade_in_step = Some(1);
                }
            }
            1 => {
                self.restore_scene_scripts(resources, &restore.snapshot.scripts)?;
                self.music
                    .restore_facts(tracks.unwrap_or(&[]), restore.snapshot.music)?;
                if restore.snapshot.control {
                    self.control.apply_at(1, false, restore.snapshot.origin)?;
                }
            }
            _ => {}
        }
        restore.remaining -= 1;
        if restore.remaining != 0 {
            self.scene_restore = Some(restore);
        }
        Ok(())
    }
    /// Capture supported scene-local reconstruction state at a tick boundary.
    pub fn capture_scene(&self) -> Result<SceneSnapshot, Error> {
        let reels = self
            .actor_reel_sources()
            .into_iter()
            .map(|(id, mut source)| {
                // V1 SAVED_ACTOR stores depth and play coordinates as signed shorts.
                source.depth = source.depth as i16 as i32;
                source.position = source.position.map(|v| v as i16 as i32);
                Ok((id, self.actors.get(id as u32)?.alive, source))
            })
            .collect::<Result<_, Error>>()?;
        Ok(SceneSnapshot {
            scripts: self.capture_scene_scripts(),
            movers: self.capture_scene_movers(),
            reels,
            background: self.background_film,
            polygons: self.polygon_memory.clone(),
            origin: self.world_origin,
            boundaries: self.scroll.boundaries.clone(),
            control: self.control.enabled,
            no_blocking: self.no_blocking,
            music: self.music.facts(),
        })
    }

    /// Rebuild a scene in memory. The timed RESTORESCENE lifecycle will stage
    /// these operations; this atomic API does not yet emulate its countdown.
    pub fn rebuild_scene(
        &mut self,
        resources: &Resources<'_>,
        tracks: &[crate::music::Track<'_>],
        saved: &SceneSnapshot,
    ) -> Result<(), Error> {
        let mut next = self.clone();
        next.rebuild_scene_objects(resources, saved)?;
        next.restore_scene_scripts(resources, &saved.scripts)?;
        next.music.restore_facts(tracks, saved.music)?;
        if saved.control {
            next.control.apply_at(1, false, saved.origin)?;
        }
        *self = next;
        Ok(())
    }

    fn rebuild_scene_objects(
        &mut self,
        resources: &Resources<'_>,
        saved: &SceneSnapshot,
    ) -> Result<(), Error> {
        let next = self;
        next.polygon_memory = saved.polygons.clone();
        next.enter_scene(resources, saved.scripts.scene, None, 1)?;
        next.pending_scene = None;
        if let Some(film) = saved.background {
            next.start_background(resources, film)?;
        }
        next.world_origin = saved.origin;
        next.scroll.boundaries = saved.boundaries.clone();
        next.no_blocking = saved.no_blocking;
        for (id, alive, source) in &saved.reels {
            next.actors.get_mut(*id as u32)?.alive = *alive;
            if *alive {
                next.restore_actor_reel(resources, *id, source)?;
            }
        }
        next.restore_scene_movers(resources, &saved.movers)?;
        Ok(())
    }

    pub fn capture_scene_movers(&self) -> SceneMovers {
        SceneMovers {
            scene: self.scene,
            banks: self.declarations.movers.clone(),
            active: self
                .movers
                .values()
                .map(|m| (m.placement.actor, m.placement.position, m.last_film))
                .collect(),
        }
    }

    /// Recreate standing movers after polygons are restored. Pending walks and
    /// animation frames are not persisted by V1 SaveMovers.
    pub fn restore_scene_movers(
        &mut self,
        resources: &Resources<'_>,
        saved: &SceneMovers,
    ) -> Result<(), Error> {
        if saved.scene != self.scene {
            return Err(Error("mover snapshot belongs to another scene"));
        }
        let mut declarations = self.declarations.clone();
        declarations.movers = saved.banks.clone();
        let mut movers = std::collections::BTreeMap::new();
        let mut actors = self.actors.clone();
        for &(actor, position, film) in &saved.active {
            let placement = crate::mover::Placement::initial(
                &declarations,
                &self.polygons,
                actor,
                position,
                film,
            )?;
            let mover = crate::mover::Mover::new(resources, placement)?;
            actors
                .get_mut(u32::try_from(actor).map_err(|_| Error("invalid saved mover actor"))?)?
                .position = mover.placement.position;
            if movers.insert(actor, mover).is_some() {
                return Err(Error("duplicate saved mover"));
            }
        }
        self.declarations.movers = declarations.movers;
        self.movers = movers;
        self.actors = actors;
        Ok(())
    }

    pub fn capture_scene_scripts(&self) -> SceneScripts {
        let contexts = self
            .processes
            .states()
            .filter(|p| {
                p.player_walk.is_none()
                    && (p.vm.ip() != 0 || p.vm.pending_call().is_some())
                    && !(p.inventory_object.is_some() && p.event == 0)
            })
            .map(|p| SavedScript {
                code: p.code,
                vm: p.vm.clone(),
                actor: p.actor,
                polygon: p.polygon,
                inventory_object: p.inventory_object,
                effect_actor: p.effect_actor,
                event: p.event,
            })
            .collect();
        SceneScripts {
            scene: self.scene,
            contexts,
        }
    }

    /// Replace scene interpreter processes after scene objects have been rebuilt.
    /// Master state, global variables, inventory and elapsed time are untouched.
    pub fn restore_scene_scripts(
        &mut self,
        resources: &Resources<'_>,
        saved: &SceneScripts,
    ) -> Result<(), Error> {
        if saved.scene != self.scene {
            return Err(Error("script snapshot belongs to another scene"));
        }
        let mut processes = Scheduler::default();
        // spawn inserts at the front while idle; preserve captured queue order.
        for context in saved.contexts.iter().rev() {
            scene::script_bytes(resources, context.code)?;
            if let Some(actor) = context.actor {
                self.actors.get(actor)?;
            }
            if let Some(polygon) = context.polygon {
                self.polygons
                    .active
                    .get(polygon)
                    .ok_or(Error("saved script polygon outside scene"))?;
            }
            if let Some(item) = context.inventory_object {
                if !self.inventory_objects.contains_key(&item) {
                    return Err(Error("saved script inventory object missing"));
                }
            }
            let mut process = ScriptProcess::new(context.code);
            process.restored_context = true;
            process.vm = context.vm.restored_context();
            process.actor = context.actor;
            process.polygon = context.polygon;
            process.inventory_object = context.inventory_object;
            process.effect_actor = context.effect_actor;
            process.event = context.event;
            processes.spawn(Some(self.generation), process)?;
        }
        self.processes = processes;
        Ok(())
    }

    /// Currently owned film reels, including the primary reel of a blocking PLAY.
    /// These are reconstruction inputs, not a complete scene snapshot.
    pub fn actor_reel_sources(&self) -> Vec<(i32, crate::animation::ReelSource)> {
        let mut sources: Vec<_> = self
            .films
            .iter()
            .chain(
                self.processes
                    .states()
                    .filter_map(|p| p.play_wait.as_ref().map(|(_, reel)| reel)),
            )
            .filter(|reel| self.owns_reel(reel))
            .filter_map(|reel| reel.source.clone().map(|source| (reel.object.id, source)))
            .collect();
        sources.sort_by_key(|(actor, _)| *actor);
        sources
    }

    /// Rebuild one V1 saved actor column from its beginning. Scene restoration
    /// must establish the actor's life state before calling this operation.
    pub fn restore_actor_reel(
        &mut self,
        resources: &Resources<'_>,
        actor: i32,
        source: &crate::animation::ReelSource,
    ) -> Result<bool, Error> {
        let film = Film::read(resources, source.film)?;
        let definition = film
            .reels
            .get(source.column)
            .ok_or(Error("saved reel column outside film"))?;
        let mut reel = Animation::new(resources, definition, 24 / film.rate as u32)?;
        if reel.object.id != actor {
            return Err(Error("saved reel actor mismatch"));
        }
        reel.reel = Some(Handle(
            source
                .film
                .0
                .checked_add(
                    u32::try_from(8 + source.column * 8)
                        .map_err(|_| Error("saved reel offset overflow"))?,
                )
                .ok_or(Error("saved reel identity overflow"))?,
        ));
        self.place_actor_reel(&mut reel, source.column, source.position)?;
        // Zero is the reference sentinel for recomputing normal film depth.
        reel.object.z = if source.depth == 0 {
            self.prepare_actor_film(resources, source.film, source.position)?[source.column]
                .object
                .z
        } else {
            source.depth
        };
        reel.source = Some(crate::animation::ReelSource {
            depth: reel.object.z,
            ..source.clone()
        });
        let mut next = self.clone();
        next.schedule_actor_film(actor, source.film);
        let started = next.playback.start(actor, source.film) && next.claim_reel(&mut reel)?;
        if started {
            next.films.push(reel);
        }
        *self = next;
        Ok(started)
    }

    fn queue_scene_request(&mut self, request: crate::scene_hook::Request) {
        self.pending_scene = Some((
            request.scene,
            request.entrance,
            request.transition,
            self.pending_scene.and_then(|p| p.3),
        ));
    }

    fn hold_object(&mut self, resources: &Resources<'_>, item: i32) -> Result<(), Error> {
        if item == self.held_object {
            return Ok(());
        }
        let cursor = if item == -1 {
            None
        } else {
            let object = self
                .inventory_objects
                .get(&item)
                .ok_or(Error("undefined held inventory object"))?;
            Some(crate::inventory::HeldCursor::new(resources, object.film)?)
        };
        // Pinned V1 workaround: a replaced held-only item goes into luggage.
        let mut windows = self.declarations.windows.clone();
        if self.held_object != -1
            && !windows[1..]
                .iter()
                .flatten()
                .any(|w| w.contents.contains(&self.held_object))
        {
            let luggage = windows[1]
                .as_mut()
                .ok_or(Error("missing luggage inventory"))?;
            if luggage.contents.len() >= 150 {
                return Err(Error("inventory contents limit"));
            }
            luggage.contents.push(self.held_object);
        }
        self.declarations.windows = windows;
        self.held_cursor = cursor;
        self.inventory_dirty |= self.held_object != item && self.active_inventory.is_some();
        self.held_object = item;
        Ok(())
    }
    /// Suspended script calls and their operand stacks for compatibility diagnostics.
    pub fn pending_calls(&self) -> Vec<(Handle, LibraryCall, Vec<i32>)> {
        std::iter::once((self.master_code, &self.master))
            .chain(self.processes.states().map(|p| (p.code, &p.vm)))
            .filter_map(|(code, vm)| {
                vm.pending_call()
                    .map(|call| (code, call, vm.operands().to_vec()))
            })
            .collect()
    }
    /// Host key/button event. Events predating WAITKEY do not satisfy it.
    pub fn user_event(&mut self) -> Result<(), Error> {
        self.user_events = self
            .user_events
            .checked_add(1)
            .ok_or(Error("input event overflow"))?;
        self.control.user_event(self.ticks);
        Ok(())
    }
    pub fn left_event(&mut self) -> Result<(), Error> {
        let next = self
            .left_events
            .checked_add(1)
            .ok_or(Error("left event overflow"))?;
        self.user_event()?;
        self.left_events = next;
        Ok(())
    }
    /// Logical game coordinates. V1 WAITKEY also wakes on cursor movement.
    pub fn move_cursor(&mut self, position: [i32; 2]) {
        let old = self.control.cursor;
        self.control.move_cursor(position);
        if self.control.cursor != old {
            self.control.user_event(self.ticks);
        }
    }

    /// Queue an already resolved V1 polygon interaction. The host/input layer
    /// must resolve actor/tag priority and single/double-click timing first.
    /// Does not synthesize raw input counters or bypass disabled control.
    pub fn interact_polygon(
        &mut self,
        resources: &Resources<'_>,
        index: usize,
        interaction: PolygonInteraction,
    ) -> Result<bool, Error> {
        let polygon = self
            .polygons
            .active
            .get(index)
            .ok_or(Error("invalid interaction polygon"))?;
        if !matches!(
            polygon.definition.kind,
            crate::polygon::Kind::Tag | crate::polygon::Kind::Exit
        ) {
            return Err(Error("interaction target is not a tag or exit"));
        }
        if !self.control.enabled
            || self.active_inventory.is_some()
            || !polygon.enabled
            || self.last_interaction_tick == Some(self.ticks)
        {
            return Ok(false);
        }
        if polygon.definition.script.0 == 0 {
            return Ok(false);
        }
        scene::script_bytes(resources, polygon.definition.script)?;
        let mut process = ScriptProcess::new(polygon.definition.script);
        process.polygon = Some(index);
        process.check_control_on_start = true;
        process.event = interaction as i32;
        self.processes.spawn(Some(self.generation), process)?;
        self.last_interaction_tick = Some(self.ticks);
        Ok(true)
    }

    /// Resolve a screen-space world gesture with actor/tag/exit priority.
    /// Hosts handle raw button timing; the core handles game coordinates.
    pub fn interact_world(
        &mut self,
        resources: &Resources<'_>,
        point: [i32; 2],
        event: PolygonInteraction,
    ) -> Result<bool, Error> {
        if !self.control.enabled
            || self.active_inventory.is_some()
            || self.last_interaction_tick == Some(self.ticks)
        {
            return Ok(false);
        }
        if let Some(actor) = self.actor_at(resources, point)? {
            return self.interact_actor(resources, actor, event);
        }
        let world = [
            point[0]
                .checked_add(self.world_origin[0])
                .ok_or(Error("world cursor overflow"))?,
            point[1]
                .checked_add(self.world_origin[1])
                .ok_or(Error("world cursor overflow"))?,
        ];
        if let Some(poly) = self
            .polygons
            .at(world, crate::polygon::Kind::Tag)
            .or_else(|| self.polygons.at(world, crate::polygon::Kind::Exit))
        {
            return self.interact_polygon(resources, poly, event);
        }
        if event != PolygonInteraction::WalkTo {
            return Ok(false);
        }
        let Some(actor) = self.declarations.lead.as_ref().map(|lead| lead.id) else {
            return Ok(false);
        };
        if !self.movers.contains_key(&actor) {
            return Ok(false);
        }
        let mut process = ScriptProcess::new(Handle(0));
        process.player_walk = Some((actor, world));
        self.processes.spawn(Some(self.generation), process)?;
        self.last_interaction_tick = Some(self.ticks);
        Ok(true)
    }

    /// V1 actor tags are searched in ascending actor ID, before polygon tags.
    /// The point is in screen coordinates; animation bounds are world coordinates.
    pub fn actor_at(
        &self,
        resources: &Resources<'_>,
        point: [i32; 2],
    ) -> Result<Option<u32>, Error> {
        if !self.control.tags_enabled || self.active_inventory.is_some() {
            return Ok(None);
        }
        let world = std::array::from_fn(|i| i64::from(point[i]) + i64::from(self.world_origin[i]));
        for (&id, &(_, kind)) in &self.actor_tags {
            let actor = self.actors.get(id)?;
            if actor.hidden || !actor.alive {
                continue;
            }
            let animation = self
                .movers
                .get(&(id as i32))
                .map(|m| &m.animation)
                .or_else(|| {
                    self.films
                        .iter()
                        .rev()
                        .find(|a| a.object.id == id as i32 && self.owns_reel(a))
                });
            if let Some(bounds) = animation
                .map(|a| a.bounds(resources))
                .transpose()?
                .flatten()
            {
                let coordinates = if animation.is_some_and(|a| a.status_layer) {
                    point.map(i64::from)
                } else {
                    world
                };
                if crate::actor::hotspot(bounds, kind, coordinates)?.is_some() {
                    return Ok(Some(id));
                }
            }
        }
        Ok(None)
    }

    /// Dispatch a resolved tagged actor. V1 actor scripts do not perform the
    /// polygon process's second control-token check after their click delay.
    pub fn interact_actor(
        &mut self,
        resources: &Resources<'_>,
        id: u32,
        interaction: PolygonInteraction,
    ) -> Result<bool, Error> {
        let actor = self.actors.get(id)?;
        if !self.control.enabled
            || self.active_inventory.is_some()
            || !actor.alive
            || actor.hidden
            || actor.script.0 == 0
            || self.last_interaction_tick == Some(self.ticks)
        {
            return Ok(false);
        }
        scene::script_bytes(resources, actor.script)?;
        let mut process = ScriptProcess::new(actor.script);
        process.actor = Some(id);
        process.event = interaction as i32;
        self.processes.spawn(Some(self.generation), process)?;
        self.last_interaction_tick = Some(self.ticks);
        Ok(true)
    }
    fn queue_converse(&mut self, resources: &Resources<'_>, polygon: usize) -> Result<(), Error> {
        let code = self
            .polygons
            .active
            .get(polygon)
            .ok_or(Error("invalid conversation polygon"))?
            .definition
            .script;
        scene::script_bytes(resources, code)?;
        let mut process = ScriptProcess::new(code);
        process.polygon = Some(polygon);
        process.event = 7;
        self.processes.spawn(Some(self.generation), process)?;
        Ok(())
    }
    /// A resolved inventory cell gesture, after host click arbitration.
    pub fn interact_inventory(
        &mut self,
        resources: &Resources<'_>,
        index: usize,
        interaction: PolygonInteraction,
    ) -> Result<bool, Error> {
        let Some(slot @ (1 | 2)) = self.active_inventory else {
            return Ok(false);
        };
        if self.conversation.hidden || self.last_interaction_tick == Some(self.ticks) {
            return Ok(false);
        }
        let window = self.declarations.windows[slot as usize]
            .as_ref()
            .ok_or(Error("missing inventory declaration"))?;
        if index >= (window.size[0] * window.size[1]) as usize {
            return Ok(false);
        }
        let index = window.first_display + index;
        let target = window.contents.get(index).copied();
        let mut next = self.clone();
        let (item, event) = if interaction == PolygonInteraction::WalkTo && self.held_object != -1 {
            let object = self
                .inventory_objects
                .get(&self.held_object)
                .ok_or(Error("missing held inventory object"))?;
            if object.attributes & 4 != 0 && object.script.0 != 0 {
                (self.held_object, 9)
            } else {
                if (object.attributes & 1 != 0 && slot != 1)
                    || (object.attributes & 2 != 0 && slot != 2)
                {
                    return Ok(false);
                }
                if !crate::inventory::put_down(
                    &mut next.declarations.windows,
                    slot as usize,
                    self.held_object,
                    index,
                )? {
                    return Ok(false);
                }
                next.held_object = -1;
                next.held_cursor = None;
                next.inventory_dirty = true;
                next.last_interaction_tick = Some(self.ticks);
                *self = next;
                return Ok(true);
            }
        } else {
            let Some(item) = target.filter(|id| *id != 0 && *id != self.held_object) else {
                return Ok(false);
            };
            (item, interaction as i32)
        };
        let object = self
            .inventory_objects
            .get(&item)
            .ok_or(Error("missing inventory object"))?;
        if object.script.0 == 0 {
            return Ok(false);
        }
        scene::script_bytes(resources, object.script)?;
        let mut process = ScriptProcess::new(object.script);
        process.inventory_object = Some(item);
        process.event = event;
        next.processes.spawn(Some(self.generation), process)?;
        next.inventory_event_index = index;
        next.last_interaction_tick = Some(self.ticks);
        *self = next;
        Ok(true)
    }

    /// Scroll a visible ordinary inventory; backing order and held membership do not change.
    pub fn scroll_inventory(&mut self, action: crate::inventory::Scroll) -> bool {
        let Some(slot @ (1 | 2)) = self.active_inventory else {
            return false;
        };
        if self.conversation.hidden {
            return false;
        }
        let Some(window) = &mut self.declarations.windows[slot as usize] else {
            return false;
        };
        crate::inventory::scroll(window, action);
        self.inventory_dirty = true;
        true
    }
    pub fn inventory_contains_point(&self, point: [i32; 2]) -> bool {
        self.inventory_display.as_ref().is_some_and(|(frame, _)| {
            let [x, y, w, h] = frame.layout.background;
            point[0] >= x - 1
                && i64::from(point[0]) <= i64::from(x) + i64::from(w)
                && point[1] >= y - 1
                && i64::from(point[1]) <= i64::from(y) + i64::from(h)
        })
    }
    pub fn inventory_scroll_at(&self, point: [i32; 2]) -> Option<crate::inventory::Scroll> {
        if !matches!(self.active_inventory, Some(1 | 2)) || self.conversation.hidden {
            return None;
        }
        self.inventory_display
            .as_ref()?
            .0
            .layout
            .scrollbar
            .as_ref()?
            .action_at(point)
    }

    pub fn active_inventory(&self) -> Option<i32> {
        self.active_inventory
    }

    /// Close the active window without a conversation postamble (scripted CLOSEINVENTORY).
    pub fn close_inventory(&mut self) {
        self.top_window = None;
        if self.active_inventory.take().is_some() {
            self.control.tags_enabled = true;
        }
        self.conversation.end();
        self.inventory_display = None;
        self.inventory_dirty = false;
    }

    fn close_conversation(&mut self) {
        self.close_inventory();
    }
    fn rebuild_inventory(
        &mut self,
        resources: &Resources<'_>,
        strings: &crate::text::Strings<'_>,
    ) -> Result<(), Error> {
        let slot = self.active_inventory.unwrap_or(0) as usize;
        if slot != 0 {
            if let Some(window) = &mut self.declarations.windows[slot] {
                crate::inventory::adjust_top(window);
            }
        }
        let window = self.declarations.windows[slot]
            .as_ref()
            .ok_or(Error("missing conversation declaration"))?;
        let constructor = if slot == 0 {
            crate::conversation_window::Frame::new
        } else {
            crate::conversation_window::Frame::inventory
        };
        let frame = constructor(
            resources,
            self.declarations
                .window_parts
                .ok_or(Error("missing window parts"))?,
            window,
        )?;
        let font = crate::text::Font::read(
            resources,
            self.declarations
                .tag_font
                .ok_or(Error("missing tag font"))?,
        )?;
        let content = crate::conversation_window::Content::new(
            resources,
            window,
            &frame.layout,
            &font,
            strings,
            &self.inventory_objects,
            self.held_object,
        )?;
        self.inventory_display = Some((frame, content));
        self.inventory_dirty = false;
        Ok(())
    }
    /// Resolved topic input; None requests the window's close/postamble event.
    /// Host hit testing and original click timing remain separate.
    pub fn interact_conversation(
        &mut self,
        resources: &Resources<'_>,
        topic: Option<usize>,
    ) -> Result<bool, Error> {
        if !self.control.enabled
            || self.conversation.hidden
            || self.conversation.polygon.is_none()
            || self.last_interaction_tick == Some(self.ticks)
        {
            return Ok(false);
        }
        let mut next = self.clone();
        let polygon = if let Some(topic) = topic {
            let window = next.declarations.windows[0]
                .as_ref()
                .ok_or(Error("missing conversation declaration"))?;
            if topic >= (window.size[0] * window.size[1]) as usize {
                return Err(Error("conversation topic outside visible page"));
            }
            next.conversation.select(window, topic)?
        } else {
            let polygon = next.conversation.close_input();
            next.close_conversation();
            polygon
        };
        let Some(polygon) = polygon else {
            return Ok(false);
        };
        next.queue_converse(resources, polygon)?;
        next.last_interaction_tick = Some(next.ticks);
        *self = next;
        Ok(true)
    }
    pub fn inventory_cell_position(&self, index: usize) -> Option<[i32; 2]> {
        let (frame, _) = self.inventory_display.as_ref()?;
        frame.layout.icons.get(index).map(|[x, y]| [x + 12, y + 12])
    }

    /// Visible ordinary inventory cells, including empty drop destinations.
    pub fn inventory_slot_at(&self, point: [i32; 2]) -> Option<usize> {
        if !matches!(self.active_inventory, Some(1 | 2)) || self.conversation.hidden {
            return None;
        }
        let (frame, _) = self.inventory_display.as_ref()?;
        frame.layout.icons.iter().position(|[x, y]| {
            (0..25).contains(&(i64::from(point[0]) - i64::from(*x)))
                && (0..25).contains(&(i64::from(point[1]) - i64::from(*y)))
        })
    }

    pub fn conversation_topic_at(&self, point: [i32; 2]) -> Option<usize> {
        if self.conversation.polygon.is_none() || self.conversation.hidden {
            return None;
        }
        let (frame, content) = self.inventory_display.as_ref()?;
        content.icons.iter().find_map(|topic| {
            let [x, y] = frame.layout.icons[topic.slot];
            let dx = i64::from(point[0]) - i64::from(x);
            let dy = i64::from(point[1]) - i64::from(y);
            ((0..25).contains(&dx) && (0..25).contains(&dy)).then_some(topic.slot)
        })
    }

    pub fn bootstrap(resources: &Resources<'_>) -> Result<Self, Error> {
        let master = resources
            .0
            .first()
            .ok_or(Error("missing master resource"))?;
        let headers = chunks(master)?;
        let globals = headers
            .iter()
            .find(|c| c.kind == 0x33340010)
            .ok_or(Error("missing global count"))?;
        let count = word(&master[globals.payload.clone()], 0)? as usize;
        if count > 65536 {
            return Err(Error("global allocation limit"));
        }
        let code = headers
            .iter()
            .find(|c| c.kind == 0x3334000a)
            .ok_or(Error("missing master code"))?;
        let actor_count = headers
            .iter()
            .find(|c| c.kind == 0x3334000f)
            .map(|c| word(&master[c.payload.clone()], 0))
            .transpose()?
            .unwrap_or(0);
        let mut engine = Self {
            saved_scenes: Vec::new(),
            scene_restore: None,
            master_stopped: false,
            conversation: crate::conversation::Conversation::default(),
            scene_hooks: crate::scene_hook::Hooks::default(),
            inventory_display: None,
            inventory_dirty: false,
            top_window: None,
            scene_calls: None,
            no_blocking: false,
            inventory_objects: crate::inventory::read(resources)?,
            actors: crate::actor::Actors::new(actor_count as usize)?,
            actor_tags: std::collections::BTreeMap::new(),
            actor_effects: std::collections::BTreeMap::new(),
            music: crate::music_player::MusicPlayer::default(),
            music_frame: Vec::new(),
            declarations: Declarations::default(),
            globals: vec![0; count],
            scene: Handle(0),
            ticks: 0,
            timers: crate::timers::Timers::default(),
            held_object: -1,
            held_cursor: None,
            last_interaction_tick: None,
            user_events: 0,
            left_events: 0,
            subtitles: true,
            talk_color: None,
            movers: std::collections::BTreeMap::new(),
            polygon_memory: crate::polygon::EnabledMemory::default(),
            polygon_table: Handle(0),
            playback: crate::playback::Playback::default(),
            world_origin: [0, 0],
            scroll: crate::scroll::Scroll::default(),
            scroll_maximum: [0, 0],
            pending_scene: None,
            fade_multiplier: 0x10000,
            fade_in_step: None,
            fade_on_background: false,
            reel_owners: std::collections::BTreeMap::new(),
            next_playback: 0,
            polygons: crate::polygon::Polygons::default(),
            random: crate::random::Random::default(),
            background: None,
            background_film: None,
            cursor_animation: None,
            films: Vec::new(),
            master: Machine::default(),
            master_code: Handle(code.payload.start as u32),
            master_wait: None,
            processes: Scheduler::default(),
            generation: 0,
            control: crate::control::Control::default(),
            menu_enabled: false,
            active_inventory: None,
            inventory_event_index: 0,
            sample_player: SamplePlayer::default(),
            actor_steps: std::collections::BTreeMap::new(),
            audio_frame: Vec::new(),
        };
        let mut vm = Machine::default();
        let bytes = scene::script_bytes(resources, engine.master_code)?;
        for _ in 0..10000 {
            match vm.step(bytes, &mut engine.globals)? {
                Step::Continue => {}
                Step::Halted => return Err(Error("master did not request a scene")),
                Step::Library(call) => {
                    if engine.declarations.apply(&mut vm, call, resources)? {
                        if call.id == 24 {
                            let lead = engine.declarations.lead.as_ref().unwrap();
                            engine.actor_tags.insert(lead.id as u32, (lead.text, 0));
                        }
                        continue;
                    }
                    if call.id != 44 {
                        return Err(Error("unsupported master operation"));
                    }
                    let a = args(&vm, 3)?;
                    let (handle, entry, transition) = (Handle(a[0] as u32), a[1], a[2]);
                    vm.complete_library(call, 3, None)?;
                    engine.master = vm.clone();
                    engine.enter(resources, handle, entry, transition)?;
                    return Ok(engine);
                }
            }
        }
        Err(Error("master instruction budget"))
    }
    fn run_master(&mut self, resources: &Resources<'_>) -> Result<Option<LibraryCall>, Error> {
        if self.master_stopped {
            return Ok(None);
        }
        if let Some((call, left)) = self.master_wait {
            if left > 1 {
                self.master_wait = Some((call, left - 1));
                return Ok(None);
            }
            self.master.complete_library(call, 2, None)?;
            self.master_wait = None;
        }
        let bytes = scene::script_bytes(resources, self.master_code)?;
        for _ in 0..10000 {
            let call = match self.master.step(bytes, &mut self.globals)? {
                Step::Continue => continue,
                Step::Halted => return Ok(None),
                Step::Library(c) => c,
            };
            if self.master.complete_scene_resume(call)? {
                continue;
            }
            match call.id {
                68 => {
                    let a = args(&self.master, 4)?;
                    self.timers.start(a[0], a[1], a[2] != 0, a[3] != 0)?;
                    self.master.complete_library(call, 4, None)?;
                }
                79 => {
                    let value = self.timers.get(args(&self.master, 1)?[0]);
                    self.master.complete_library(call, 1, Some(value))?;
                }
                58 => {
                    if !self.save_scene_call(None)? {
                        self.master_stopped = true;
                        return Ok(None);
                    }
                    self.master.complete_library(call, 0, None)?;
                }
                57 | 121 => {
                    self.request_scene_restore(call.id == 57)?;
                    self.master.complete_library(call, 0, None)?;
                }
                44 => {
                    let a = args(&self.master, 3)?;
                    let request = crate::scene_hook::Request::new(Handle(a[0] as u32), a[1], a[2])?;
                    let request = self.scene_hooks.intercept(request);
                    self.queue_scene_request(request);
                    self.control.apply_at(4, false, self.world_origin)?;
                    self.master.complete_library(call, 3, None)?;
                }
                95 => {
                    let a = args(&self.master, 3)?;
                    self.scene_hooks.set(crate::scene_hook::Request::new(
                        Handle(a[0] as u32),
                        a[1],
                        a[2],
                    )?);
                    self.master.complete_library(call, 3, None)?;
                }
                99 => {
                    let request = self.scene_hooks.release()?;
                    self.queue_scene_request(request);
                    self.master.complete_library(call, 0, None)?;
                }
                107 => {
                    self.scene_hooks.clear();
                    self.master.complete_library(call, 0, None)?;
                }
                97 => {
                    self.control.user_event(self.ticks);
                    self.master.complete_library(call, 0, None)?;
                }
                96 => {
                    let seconds = self.control.idle_seconds(self.ticks)?;
                    self.master.complete_library(call, 1, Some(seconds))?;
                }
                86 => {
                    let a = args(&self.master, 2)?;
                    let n = u32::try_from(a[0]).map_err(|_| Error("negative master wait"))?;
                    let n = n
                        .checked_mul(if a[1] == 0 { 24 } else { 1 })
                        .and_then(|n| n.checked_add(1))
                        .ok_or(Error("master wait overflow"))?;
                    self.master_wait = Some((call, n));
                    return Ok(None);
                }
                _ => return Ok(Some(call)),
            }
        }
        Err(Error("master instruction budget"))
    }
    fn enter(
        &mut self,
        resources: &Resources<'_>,
        handle: Handle,
        entry: i32,
        transition: i32,
    ) -> Result<(), Error> {
        self.enter_scene(resources, handle, Some(entry), transition)
    }
    fn enter_scene(
        &mut self,
        resources: &Resources<'_>,
        handle: Handle,
        entry: Option<i32>,
        transition: i32,
    ) -> Result<(), Error> {
        if handle.offset() != 0 {
            return Err(Error("scene handle must address resource start"));
        }
        if !(0..=2).contains(&transition) {
            return Err(Error("invalid transition mode"));
        }
        let scene =
            scene::read(resources, handle.resource())?.ok_or(Error("missing scene metadata"))?;
        let mut polygons = crate::polygon::Polygons::new(
            crate::polygon::read(resources, scene.polygons, scene.polygon_count)?,
            scene.default_refer,
        );
        self.polygon_memory.restore(scene.polygons, &mut polygons);
        let entrance = if let Some(entry) = entry {
            scene
                .entrances
                .iter()
                .find(|(id, _)| *id as i32 == entry)
                .ok_or(Error("unknown scene entrance"))?
                .1
        } else {
            Handle(0)
        };
        let mut actor_scripts = self.actors.enter(&scene.actors)?;
        if entry.is_none() {
            actor_scripts.clear();
        }
        let scene_script = if entry.is_some() {
            scene.script
        } else {
            Handle(0)
        };
        for (_, code) in &actor_scripts {
            scene::script_bytes(resources, *code)?;
        }
        for code in [entrance, scene_script] {
            if code.0 != 0 {
                scene::script_bytes(resources, code)?;
            }
        }
        self.processes.cancel_scene(self.generation);
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(Error("scene generation overflow"))?;
        self.scene = handle;
        self.world_origin = [0, 0];
        self.scroll.reset_scene();
        self.scroll_maximum = [0, 0];
        self.fade_in_step = None;
        self.fade_multiplier = 0x10000;
        self.fade_on_background = transition != 1;
        self.polygons = polygons;
        self.polygon_table = scene.polygons;
        // V1 does not send STARTUP to tag scripts (that is V2-only).
        for (id, code) in actor_scripts {
            let mut process = ScriptProcess::new(code);
            process.actor = Some(id);
            self.processes.spawn(Some(self.generation), process)?;
        }
        for code in [entrance, scene_script] {
            if code.0 != 0 {
                self.processes
                    .spawn(Some(self.generation), ScriptProcess::new(code))?;
            }
        }
        self.background = None;
        self.background_film = None;
        self.cursor_animation = if let Some((handle, _)) = &self.declarations.cursor {
            Self::start_film(resources, *handle, true)?.pop()
        } else {
            None
        };
        self.films.clear();
        self.reel_owners.clear();
        self.playback.clear();
        self.movers.clear();
        self.actor_effects.clear();
        self.no_blocking = false;
        self.control.apply_at(4, false, self.world_origin)?;
        self.menu_enabled = false;
        self.active_inventory = None;
        self.top_window = None;
        self.conversation.end();
        self.inventory_display = None;
        self.inventory_dirty = false;
        self.sample_player.stop();
        self.actor_steps.clear();
        Ok(())
    }
    fn start_background(&mut self, resources: &Resources<'_>, film: Handle) -> Result<(), Error> {
        let mut animations = Self::start_film(resources, film, true)?;
        self.background = animations.pop();
        if self.scroll.focus == 0 {
            if let Some(lead) = &self.declarations.lead {
                self.scroll.set_focus(lead.id);
            }
        }
        if let Some(background) = &self.background {
            let extent = background
                .bounds(resources)?
                .map(|b| [b[2] + 1, b[3] + 1])
                .unwrap_or([0, 0]);
            self.scroll_maximum = [
                i32::try_from(extent[0] - 320).map_err(|_| Error("scroll width overflow"))?,
                i32::try_from(extent[1] - 200).map_err(|_| Error("scroll height overflow"))?,
            ];
        }
        if self.fade_on_background {
            self.fade_on_background = false;
            self.fade_multiplier = 0;
            self.fade_in_step = Some(1);
        }
        self.background_film = Some(film);
        Ok(())
    }
    fn start_film(
        resources: &Resources<'_>,
        handle: Handle,
        background: bool,
    ) -> Result<Vec<Animation>, Error> {
        let film = Film::read(resources, handle)?;
        let period = 24 / film.rate as u32;
        film.reels
            .iter()
            .take(if background { 1 } else { film.reels.len() })
            .enumerate()
            .map(|(column, r)| {
                let mut animation = Animation::new(resources, r, period)?;
                animation.reel = Some(Handle(
                    handle
                        .0
                        .checked_add(8 + column as u32 * 8)
                        .ok_or(Error("reel identity overflow"))?,
                ));
                Ok(animation)
            })
            .collect()
    }
    fn advance_transition(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        const OUT: [u32; 6] = [0xd000, 0xa000, 0x7000, 0x4000, 0x1000, 0];
        const IN: [u32; 7] = [0, 0x1000, 0x4000, 0x7000, 0xa000, 0xd000, 0x10000];
        if let Some(step) = self.fade_in_step {
            self.fade_multiplier = IN[step];
            self.fade_in_step = (step + 1 < IN.len()).then_some(step + 1);
        }
        if self.scene_restore.is_some() {
            return Ok(());
        }
        if let Some((scene, entry, mode, remaining)) = self.pending_scene {
            match remaining {
                None => {
                    self.fade_in_step = None;
                    self.pending_scene =
                        Some((scene, entry, mode, Some(if mode == 1 { 1 } else { 6 })));
                    if mode != 1 {
                        self.fade_multiplier = OUT[0];
                    }
                }
                Some(1) => {
                    self.enter(resources, scene, entry, mode)?;
                    self.pending_scene = None;
                }
                Some(left) => {
                    self.fade_multiplier = OUT[usize::from(7 - left)];
                    self.pending_scene = Some((scene, entry, mode, Some(left - 1)));
                }
            }
        }
        Ok(())
    }
    fn set_film_depths(
        &self,
        resources: &Resources<'_>,
        reels: &mut [Animation],
    ) -> Result<(), Error> {
        let mut base = 0i64;
        let mut factor = 0i64;
        for (column, reel) in reels.iter_mut().enumerate() {
            let actor = self.actors.get(reel.object.id as u32)?;
            let mover_factor = self
                .movers
                .get(&reel.object.id)
                .map(|m| i64::from(m.placement.z_factor));
            let mask = actor.masking;
            let current = mover_factor.unwrap_or(match mask {
                0 | -1 => 0,
                -2 => 10,
                _ => i64::from(mask),
            });
            if column == 0 {
                factor = current;
                let bottom = reel.bounds(resources)?.map(|b| b[3]).unwrap_or(0);
                base = if mover_factor.is_some() {
                    bottom + ((factor - i64::from(factor != 0)) << 10)
                } else {
                    match mask {
                        0 => 2,
                        -1 => bottom,
                        -2 => 10000,
                        _ => (bottom + ((factor - i64::from(factor != 0)) << 10)).max(2),
                    }
                };
            } else if current > factor {
                factor = current;
                base = factor << 10;
            }
            reel.object.z =
                i32::try_from(base + column as i64).map_err(|_| Error("film depth overflow"))?;
        }
        Ok(())
    }

    fn place_actor_reel(
        &self,
        reel: &mut Animation,
        column: usize,
        position: [i32; 2],
    ) -> Result<[i32; 2], Error> {
        let mut stored = position;
        let embedded = reel.object.x != 0 || reel.object.y != 0;
        if column == 0 || !embedded {
            if position != [-1, -1] {
                reel.object.x = position[0];
                reel.object.y = position[1];
            } else if !embedded {
                stored = self.actors.get(reel.object.id as u32)?.position;
                reel.object.x = stored[0];
                reel.object.y = stored[1];
            }
        }
        Ok(stored)
    }

    fn prepare_actor_film(
        &self,
        resources: &Resources<'_>,
        film: Handle,
        position: [i32; 2],
    ) -> Result<Vec<Animation>, Error> {
        let mut reels = Self::start_film(resources, film, false)?;
        for (column, reel) in reels.iter_mut().enumerate() {
            let stored = self.place_actor_reel(reel, column, position)?;
            reel.source = Some(crate::animation::ReelSource {
                film,
                column,
                position: stored,
                depth: 0,
            });
        }
        self.set_film_depths(resources, &mut reels)?;
        for reel in &mut reels {
            reel.source.as_mut().unwrap().depth = reel.object.z;
        }
        Ok(reels)
    }

    fn prepare_top_film(
        &self,
        resources: &Resources<'_>,
        film: Handle,
        position: [i32; 2],
    ) -> Result<Vec<Animation>, Error> {
        let mut reels = self.prepare_actor_film(resources, film, position)?;
        let base = if let Some(first) = reels.first() {
            100i32
                .checked_add(self.actors.get(first.object.id as u32)?.masking)
                .ok_or(Error("top film depth overflow"))?
        } else {
            100
        };
        for (column, reel) in reels.iter_mut().enumerate() {
            reel.status_layer = true;
            reel.object.z = base
                .checked_add(i32::try_from(column).map_err(|_| Error("top film column overflow"))?)
                .ok_or(Error("top film depth overflow"))?;
            reel.source.as_mut().unwrap().depth = reel.object.z;
        }
        Ok(reels)
    }

    fn schedule_actor_film(&mut self, actor: i32, film: Handle) {
        // V1 SetActorLatestFilm resets steps even for duplicate/deferred reels.
        self.playback.schedule(actor, film);
        self.actor_steps.insert(actor, 0);
    }

    fn launch_film(&mut self, resources: &Resources<'_>, film: Handle) -> Result<(), Error> {
        let reels = self.prepare_actor_film(resources, film, [-1, -1])?;
        for reel in &reels {
            self.schedule_actor_film(reel.object.id, film);
        }
        for mut reel in reels {
            if self.playback.start(reel.object.id, film) && self.claim_reel(&mut reel)? {
                self.films.push(reel);
            }
        }
        Ok(())
    }
    fn advance_talk(
        &mut self,
        resources: &Resources<'_>,
        wait: &mut TalkWait,
    ) -> Result<bool, Error> {
        if !wait.initialized {
            wait.initialized = true;
            if self.subtitles || wait.sample.is_none() {
                let mut position = self
                    .actors
                    .get_mut(wait.actor as u32)?
                    .position
                    .map(i64::from);
                if let Some(animation) = self
                    .films
                    .iter()
                    .find(|a| a.object.id == wait.actor && self.owns_reel(a))
                {
                    if let Some([left, top, right, _]) = animation.bounds(resources)? {
                        position = [(left + right) / 2, top];
                    }
                }
                if wait.mover_saved.is_some() {
                    if let Some(mover) = self.movers.get(&wait.actor) {
                        if let Some([left, top, right, _]) = mover.animation.bounds(resources)? {
                            position = [(left + right) / 2, top];
                        }
                    }
                }
                let x = i32::try_from(position[0] - i64::from(self.world_origin[0]))
                    .map_err(|_| Error("speech x overflow"))?;
                let y = i32::try_from(position[1] - i64::from(self.world_origin[1]))
                    .map_err(|_| Error("speech y overflow"))?;
                let font = crate::text::Font::read(
                    resources,
                    self.declarations
                        .talk_font
                        .ok_or(Error("missing talk font"))?,
                )?;
                if font.flags != 1 {
                    return Err(Error("unsupported font draw mode"));
                }
                let mut layout = font.centered(resources, &wait.text, [x, y])?;
                layout.above_speaker(resources, y)?;
                wait.layout = Some(layout);
                self.talk_color = Some(
                    self.declarations
                        .speech_colors
                        .get(&wait.actor)
                        .copied()
                        .filter(|c| *c != [0; 3])
                        .or_else(|| self.declarations.speech_colors.get(&-1).copied())
                        .unwrap_or([0; 3]),
                );
            }
            wait.origin = self.world_origin;
            return Ok(false);
        }
        if let Some(layout) = &mut wait.layout {
            layout.shift([
                i64::from(wait.origin[0]) - i64::from(self.world_origin[0]),
                i64::from(wait.origin[1]) - i64::from(self.world_origin[1]),
            ])?;
        }
        wait.origin = self.world_origin;
        wait.timeout = wait.timeout.saturating_sub(1);
        let complete = if let Some(serial) = wait.sample {
            self.sample_player.serial != serial || self.sample_player.id.is_none()
        } else {
            let done = wait.remaining == 0;
            wait.remaining = wait.remaining.saturating_sub(1);
            done
        };
        if !complete && wait.timeout != 0 && wait.left_events == self.left_events {
            return Ok(false);
        }
        if wait.sample == Some(self.sample_player.serial) {
            self.sample_player.stop();
        }
        if let Some(saved) = wait.mover_saved {
            self.movers
                .get_mut(&wait.actor)
                .ok_or(Error("talking mover disappeared"))?
                .pop_talk(resources, &self.declarations, saved)?;
        } else if let Some(film) = self.playback.finish_talk(wait.actor) {
            self.launch_film(resources, film)?;
        } else {
            return Err(Error("talk actor has no normal film to restore"));
        }
        if wait.restore_control {
            self.control
                .apply_at(1, self.active_inventory.is_some(), self.world_origin)?;
        }
        if wait.restore_tags {
            self.control.tags_enabled = true;
        }
        Ok(true)
    }
    fn owns_reel(&self, animation: &Animation) -> bool {
        animation.reel.is_some_and(|reel| {
            self.reel_owners.get(&animation.object.id) == Some(&(reel, animation.playback))
        })
    }
    fn release_reel(&mut self, animation: &Animation) {
        if self.owns_reel(animation) {
            self.reel_owners.remove(&animation.object.id);
        }
    }
    /// A duplicate reel does not restart. Replacement gets a distinct generation.
    fn claim_reel(&mut self, animation: &mut Animation) -> Result<bool, Error> {
        let reel = animation.reel.ok_or(Error("missing reel identity"))?;
        let actor = animation.object.id;
        if !self
            .actors
            .get(u32::try_from(actor).map_err(|_| Error("invalid reel actor"))?)?
            .alive
        {
            return Ok(false);
        }
        if self
            .reel_owners
            .get(&actor)
            .is_some_and(|(current, _)| *current == reel)
        {
            return Ok(false);
        }
        self.next_playback = self
            .next_playback
            .checked_add(1)
            .ok_or(Error("playback generation overflow"))?;
        animation.playback = self.next_playback;
        self.reel_owners.insert(actor, (reel, animation.playback));
        self.films.retain(|old| old.object.id != actor);
        Ok(true)
    }
    fn disable_actor(&mut self, id: u32) -> Result<(), Error> {
        self.actors.disable(id)?;
        self.movers.remove(&(id as i32));
        self.reel_owners.remove(&(id as i32));
        self.films.retain(|film| film.object.id != id as i32);
        Ok(())
    }
    /// Advance a fixed 1/24 second tick. The initial ordering is provisional
    /// pending comparison against the reference coroutine scheduler.
    pub fn tick(&mut self, resources: &Resources<'_>) -> Result<Status, Error> {
        self.tick_audio(resources, None)
    }
    pub fn tick_audio(
        &mut self,
        resources: &Resources<'_>,
        bank: Option<&SampleBank<'_>>,
    ) -> Result<Status, Error> {
        self.tick_media(resources, bank, None)
    }
    /// Supplying tracks enables MIDI commands. The host must consume music_frame.
    pub fn tick_media(
        &mut self,
        resources: &Resources<'_>,
        bank: Option<&SampleBank<'_>>,
        tracks: Option<&[crate::music::Track<'_>]>,
    ) -> Result<Status, Error> {
        self.tick_content(resources, bank, tracks, None)
    }
    pub fn tick_content(
        &mut self,
        resources: &Resources<'_>,
        bank: Option<&SampleBank<'_>>,
        tracks: Option<&[crate::music::Track<'_>]>,
        strings: Option<&crate::text::Strings<'_>>,
    ) -> Result<Status, Error> {
        let mut next = self.clone();
        let status = next.advance(resources, bank, tracks, strings)?;
        *self = next;
        Ok(status)
    }
    fn advance(
        &mut self,
        resources: &Resources<'_>,
        bank: Option<&SampleBank<'_>>,
        tracks: Option<&[crate::music::Track<'_>]>,
        strings: Option<&crate::text::Strings<'_>>,
    ) -> Result<Status, Error> {
        self.ticks = self.ticks.checked_add(1).ok_or(Error("tick overflow"))?;
        self.advance_scene_restore(resources, tracks)?;
        self.advance_transition(resources)?;
        self.music_frame = self.music.tick()?;
        self.audio_frame = if let Some(bank) = bank {
            self.sample_player.tick(bank)?
        } else {
            Vec::new()
        };
        if let Some(call) = self.run_master(resources)? {
            return Ok(Status::Unsupported(call));
        }
        if !self.control.hidden {
            if let Some(cursor) = &mut self.cursor_animation {
                if cursor.tick(resources)? == Tick::Finished {
                    return Err(Error("cursor animation ended"));
                }
            }
        }
        if let Some(cursor) = &mut self.held_cursor {
            cursor.animation.tick(resources)?;
        }
        if let Some(bg) = &mut self.background {
            if bg.tick(resources)? == Tick::Finished {
                return Err(Error("background animation ended"));
            }
        }
        let mut active = Vec::new();
        for mut a in std::mem::take(&mut self.films) {
            if !self.owns_reel(&a) {
                continue;
            }
            let state = a.tick(resources)?;
            self.actor_steps.insert(a.object.id, a.steps);
            if state != Tick::Finished {
                self.actors.get_mut(a.object.id as u32)?.position = [a.object.x, a.object.y];
                active.push(a);
            } else {
                self.release_reel(&a);
            }
        }
        self.films = active;
        let mover_ids: Vec<_> = self.movers.keys().copied().collect();
        for actor in mover_ids {
            if self.reel_owners.contains_key(&actor) {
                continue;
            }
            if self.movers[&actor].walk_target.is_some() {
                let footprints = if self.no_blocking {
                    Vec::new()
                } else {
                    self.movers
                        .values()
                        .map(|m| m.footprint(resources))
                        .collect::<Result<Vec<_>, _>>()?
                };
                let mover = self.movers.get_mut(&actor).ok_or(Error("missing mover"))?;
                mover.advance_direct_walk(
                    resources,
                    &self.declarations,
                    &self.polygons,
                    &footprints,
                )?;
                self.actors.get_mut(actor as u32)?.position = mover.placement.position;
            } else {
                self.movers
                    .get_mut(&actor)
                    .ok_or(Error("missing mover"))?
                    .animation
                    .tick(resources)?;
            }
        }
        let effect_positions = self
            .movers
            .iter()
            .map(|(&id, m)| (id, m.placement.position))
            .collect::<Vec<_>>();
        self.monitor_effects(resources, &effect_positions)?;
        if self.scroll_maximum != [0, 0] {
            if let Some(mover) = self.movers.get(&self.scroll.focus) {
                if !self.reel_owners.contains_key(&self.scroll.focus)
                    && !self.actors.get(self.scroll.focus as u32)?.hidden
                {
                    if let Some([_, top, _, bottom]) = mover.animation.bounds(resources)? {
                        self.scroll.monitor(
                            mover.placement.position,
                            bottom - top,
                            self.world_origin,
                            self.scroll_maximum,
                        );
                    }
                }
            }
        }
        let old_origin = self.world_origin;
        let was_scrolling = self.scroll.is_scrolling();
        if was_scrolling && self.scroll.follow_cursor {
            let world_cursor = [
                self.control.cursor[0].wrapping_add(old_origin[0]),
                self.control.cursor[1].wrapping_add(old_origin[1]),
            ];
            if self
                .polygons
                .at(world_cursor, crate::polygon::Kind::Tag)
                .is_none()
                && self
                    .polygons
                    .at(world_cursor, crate::polygon::Kind::Exit)
                    .is_none()
            {
                self.scroll.follow_cursor = false;
            }
        }
        self.scroll
            .tick(&mut self.world_origin, self.scroll_maximum);
        if was_scrolling && self.scroll.follow_cursor {
            for (axis, old) in old_origin.into_iter().enumerate() {
                self.control.cursor[axis] = (i64::from(self.control.cursor[axis]) + i64::from(old)
                    - i64::from(self.world_origin[axis]))
                    as i16 as i32;
            }
        }
        self.processes.begin()?;
        let mut blocked = None;
        while let Some(id) = self.processes.ready()? {
            let mut process = self
                .processes
                .state_mut(id)
                .ok_or(Error("missing process"))?
                .clone();
            let result = self.run_process(resources, bank, tracks, strings, &mut process)?;
            *self
                .processes
                .state_mut(id)
                .ok_or(Error("missing active process"))? = process;
            self.processes.finish(if result == Status::Halted {
                None
            } else {
                Some(1)
            })?;
            if let Status::Unsupported(call) = result {
                blocked = Some(call);
                break;
            }
        }
        if let Some(call) = blocked {
            self.processes.end_pass()?;
            return Ok(Status::Unsupported(call));
        }
        if self.active_inventory.is_some() {
            if self.inventory_dirty && !self.conversation.hidden {
                self.rebuild_inventory(
                    resources,
                    strings.ok_or(Error("missing conversation strings"))?,
                )?;
            }
            if let Some((_, content)) = &mut self.inventory_display {
                content.tick(resources)?;
            }
        }
        self.timers.tick()?;
        Ok(
            if self.processes.is_empty()
                && self.scene_restore.is_none()
                && self.pending_scene.is_none()
                && self.master_wait.is_none()
            {
                Status::Halted
            } else {
                Status::Running
            },
        )
    }
    fn monitor_effects(
        &mut self,
        resources: &Resources<'_>,
        positions: &[(i32, [i32; 2])],
    ) -> Result<(), Error> {
        for &(actor, point) in positions {
            let current = self.polygons.at(point, crate::polygon::Kind::Effect);
            let previous = self.actor_effects.get(&actor).copied();
            let transition = match (previous, current) {
                (Some(old), now) if Some(old) != now => {
                    self.actor_effects.remove(&actor);
                    Some((old, 5))
                }
                (None, Some(new)) => {
                    self.actor_effects.insert(actor, new);
                    Some((new, 4))
                }
                _ => None,
            };
            if let Some((polygon, event)) = transition {
                let code = self.polygons.active[polygon].definition.script;
                if code.0 == 0 {
                    continue;
                }
                scene::script_bytes(resources, code)?;
                let mut process = ScriptProcess::new(code);
                process.polygon = Some(polygon);
                process.effect_actor = Some(actor);
                process.event = event;
                self.processes.spawn(Some(self.generation), process)?;
            }
        }
        Ok(())
    }

    fn run_process(
        &mut self,
        resources: &Resources<'_>,
        bank: Option<&SampleBank<'_>>,
        tracks: Option<&[crate::music::Track<'_>]>,
        strings: Option<&crate::text::Strings<'_>>,
        process: &mut ScriptProcess,
    ) -> Result<Status, Error> {
        if let Some((actor, target)) = process.player_walk.take() {
            let size = [
                self.scroll_maximum[0]
                    .checked_add(320)
                    .ok_or(Error("scene width overflow"))?,
                self.scroll_maximum[1]
                    .checked_add(200)
                    .ok_or(Error("scene height overflow"))?,
            ];
            let destination =
                crate::destination::resolve(&self.polygons, target, self.world_origin, size)?;
            if let Some(mover) = self.movers.get_mut(&actor) {
                mover.begin_walk(
                    resources,
                    &self.declarations,
                    &self.polygons,
                    destination.unwrap_or(mover.placement.position),
                    target,
                )?;
                self.scroll.follow_cursor = false;
                self.reel_owners.remove(&actor);
                self.films.retain(|a| a.object.id != actor);
            }
            return Ok(Status::Halted);
        }
        // V1 PolyTinselProcess takes control once, then hides the window for
        // the entire CONVERSE coroutine, including all script sleeps.
        if !process.restored_context
            && process.event == 7
            && process.polygon.is_some()
            && !process.conversation_started
        {
            process.conversation_started = true;
            process.conversation_took_control = self.control.enabled;
            if process.conversation_took_control {
                self.control
                    .apply_at(0, self.active_inventory.is_some(), self.world_origin)?;
            }
            if self.conversation.polygon.is_some() {
                self.conversation.hidden = true;
            }
        }
        if process.check_control_on_start {
            process.check_control_on_start = false;
            if !self.control.enabled {
                return Ok(Status::Halted);
            }
        }
        if let Some(mut wait) = process.walked_wait.take() {
            if wait.delay > 1 {
                wait.delay -= 1;
                process.walked_wait = Some(wait);
                return Ok(Status::Running);
            }
            if wait.serial.is_none() {
                let cursor = [
                    self.control.cursor[0].wrapping_add(self.world_origin[0]),
                    self.control.cursor[1].wrapping_add(self.world_origin[1]),
                ];
                let destination = crate::destination::resolve(
                    &self.polygons,
                    wait.target,
                    self.world_origin,
                    [
                        self.scroll_maximum[0]
                            .checked_add(320)
                            .ok_or(Error("scene width overflow"))?,
                        self.scroll_maximum[1]
                            .checked_add(200)
                            .ok_or(Error("scene height overflow"))?,
                    ],
                )?;
                let mover = self
                    .movers
                    .get_mut(&wait.actor)
                    .ok_or(Error("WALKED actor has no mover"))?;
                if mover.placement.path.is_none() {
                    return Err(Error("WALKED mover is outside paths"));
                }
                wait.serial = Some(mover.begin_walk(
                    resources,
                    &self.declarations,
                    &self.polygons,
                    destination.unwrap_or(mover.placement.position),
                    cursor,
                )?);
                wait.delay = 0;
                self.scroll.follow_cursor = false;
                self.reel_owners.remove(&wait.actor);
                self.films.retain(|a| a.object.id != wait.actor);
            }
            let mover = self
                .movers
                .get(&wait.actor)
                .ok_or(Error("WALKED mover disappeared"))?;
            if wait.serial == Some(mover.walk_serial) && mover.walk_target.is_some() {
                process.walked_wait = Some(wait);
                return Ok(Status::Running);
            }
            let reached =
                wait.serial == Some(mover.walk_serial) && mover.placement.position == wait.target;
            process
                .vm
                .complete_library(wait.call, 4, Some(i32::from(reached)))?;
        }
        if let Some((call, actor, serial, restore_control)) = process.walk_wait {
            let Some(mover) = self.movers.get(&actor) else {
                return Ok(Status::Halted);
            };
            if mover.walk_serial != serial {
                return Ok(Status::Halted);
            }
            if mover.walk_target.is_some() {
                return Ok(Status::Running);
            }
            if restore_control {
                self.control.apply_at(1, false, self.world_origin)?;
            }
            process
                .vm
                .complete_library(call, if call.id == 87 { 5 } else { 6 }, None)?;
            process.walk_wait = None;
        }
        if let Some((call, actor, serial, film)) = process.tag_walk_wait {
            let Some(mover) = self.movers.get_mut(&actor) else {
                return Ok(Status::Halted);
            };
            if mover.walk_serial != serial {
                return Ok(Status::Halted);
            }
            if mover.walk_target.is_some() {
                return Ok(Status::Running);
            }
            mover.adopt_tag_pose(resources, &self.declarations, film)?;
            process.vm.complete_library(call, 3, None)?;
            process.tag_walk_wait = None;
        }
        if let Some(wait) = &mut process.talk_wait {
            if !self.advance_talk(resources, wait)? {
                return Ok(Status::Running);
            }
            process.vm.complete_library(wait.call, 2, None)?;
            process.talk_wait = None;
        }
        if let Some((call, serial, target)) = process.scroll_wait {
            if serial != self.scroll.serial {
                return Ok(Status::Halted);
            }
            if self.world_origin != target {
                return Ok(Status::Running);
            }
            process.vm.complete_library(call, 4, None)?;
            process.scroll_wait = None;
        }
        if let Some((call, animation)) = &mut process.play_wait {
            let state = if self.owns_reel(animation) {
                let state = animation.tick(resources)?;
                self.actor_steps
                    .insert(animation.object.id, animation.steps);
                state
            } else {
                Tick::Finished
            };
            if state != Tick::Finished {
                self.actors.get_mut(animation.object.id as u32)?.position =
                    [animation.object.x, animation.object.y];
                return Ok(Status::Running);
            }
            process.vm.complete_library(*call, 6, None)?;
            self.release_reel(animation);
            process.play_wait = None;
        }
        if let Some(wait) = &mut process.print_wait {
            let cancelled = ((wait.inventory || wait.left_events != 0)
                && wait.left_events != self.left_events)
                || (wait.inventory && self.conversation.hidden);
            let finished = if let Some(serial) = wait.sample_serial {
                wait.timeout = wait.timeout.saturating_sub(1);
                self.sample_player.serial != serial
                    || self.sample_player.id.is_none()
                    || wait.timeout == 0
            } else {
                let done = wait.remaining == 0;
                wait.remaining = wait.remaining.saturating_sub(1);
                done
            };
            if !cancelled && !finished {
                return Ok(Status::Running);
            }
            if wait.sample_serial == Some(self.sample_player.serial) {
                self.sample_player.stop();
            }
            if wait.restore_control {
                self.control
                    .apply_at(1, self.active_inventory.is_some(), self.world_origin)?;
            }
            process
                .vm
                .complete_library(wait.call, wait.consumed, None)?;
            process.print_wait = None;
        }
        if let Some(call) = process.music_wait {
            if self.music.playing() {
                return Ok(Status::Running);
            }
            process.vm.complete_library(call, 3, None)?;
            process.music_wait = None;
        }
        if let Some(wait) = &process.key_wait {
            if wait.events == self.user_events && wait.cursor == self.control.cursor {
                return Ok(Status::Running);
            }
            process.vm.complete_library(wait.call, 0, None)?;
            process.key_wait = None;
        }
        if let Some(call) = process.sample_wait {
            if self.sample_player.id.is_some() {
                return Ok(Status::Running);
            }
            process.vm.complete_library(call, 2, None)?;
            process.sample_wait = None;
        }
        if let Some((call, left)) = process.wait {
            if left > 1 {
                process.wait = Some((call, left - 1));
                return Ok(Status::Running);
            }
            process.vm.complete_library(call, 2, None)?;
            process.wait = None;
        }
        let bytes = scene::script_bytes(resources, process.code)?;
        for _ in 0..10000 {
            let call = match process.vm.step(bytes, &mut self.globals)? {
                Step::Continue => continue,
                Step::Halted => {
                    if process.conversation_started {
                        if process.conversation_took_control {
                            self.control.apply_at(
                                1,
                                self.active_inventory.is_some(),
                                self.world_origin,
                            )?;
                        }
                        if self.conversation.polygon.is_some() {
                            self.conversation.hidden = false;
                        }
                    }
                    if let Some(id) = process.actor {
                        self.actors.get_mut(id)?.completed = true;
                    }
                    return Ok(Status::Halted);
                }
                Step::Library(c) => c,
            };
            if let Some(trace) = &mut self.scene_calls {
                if trace.len() == 256 {
                    trace.pop_front();
                }
                trace.push_back(SceneCall {
                    tick: self.ticks,
                    script: process.code,
                    call,
                    operands: process.vm.operands().to_vec(),
                });
            }
            if process.vm.complete_scene_resume(call)? {
                continue;
            }
            match call.id {
                68 => {
                    let a = args(&process.vm, 4)?;
                    self.timers.start(a[0], a[1], a[2] != 0, a[3] != 0)?;
                    process.vm.complete_library(call, 4, None)?;
                }
                79 => {
                    let value = self.timers.get(args(&process.vm, 1)?[0]);
                    process.vm.complete_library(call, 1, Some(value))?;
                }
                13 => {
                    self.close_inventory();
                    process.vm.complete_library(call, 0, None)?;
                }
                58 => {
                    if !self.save_scene_call(Some(process))? {
                        return Ok(Status::Halted);
                    }
                    process.vm.complete_library(call, 0, None)?;
                }
                57 | 121 => {
                    self.request_scene_restore(call.id == 57)?;
                    process.vm.complete_library(call, 0, None)?;
                }
                37 => {
                    let slot = args(&process.vm, 1)?[0];
                    if !(1..=2).contains(&slot) {
                        return Err(Error("invalid inventory slot"));
                    }
                    if self.active_inventory.is_none() && self.top_window.is_none() {
                        self.active_inventory = Some(slot);
                        self.conversation.hidden = false;
                        self.control.tags_enabled = false;
                        self.rebuild_inventory(
                            resources,
                            strings.ok_or(Error("missing inventory strings"))?,
                        )?;
                    }
                    process.vm.complete_library(call, 1, None)?;
                }
                15 => {
                    let mode = args(&process.vm, 1)?[0];
                    let polygon = process
                        .polygon
                        .ok_or(Error("CONVERSATION requires polygon context"))?;
                    if mode == 2 {
                        self.close_conversation();
                    } else if self.active_inventory.is_none() {
                        let strings = strings.ok_or(Error("missing conversation strings"))?;
                        let window = self.declarations.windows[0]
                            .as_mut()
                            .ok_or(Error("missing conversation declaration"))?;
                        if let Some(owner) = self.conversation.open(
                            mode,
                            polygon,
                            window,
                            &self.declarations.permanent_topics,
                        )? {
                            self.active_inventory = Some(0);
                            self.control.tags_enabled = false;
                            self.rebuild_inventory(resources, strings)?;
                            self.queue_converse(resources, owner)?;
                        }
                    }
                    process.vm.complete_library(call, 1, None)?;
                }
                9 => {
                    let item = args(&process.vm, 1)?[0];
                    let slot = self
                        .active_inventory
                        .filter(|s| matches!(s, 1 | 2))
                        .ok_or(Error("ADDOPENINV requires open inventory"))?;
                    crate::inventory::add_open(
                        &mut self.declarations.windows,
                        slot as usize,
                        item,
                        self.inventory_event_index,
                    )?;
                    self.inventory_dirty = true;
                    process.vm.complete_library(call, 1, None)?;
                }
                7 | 8 => {
                    let item = args(&process.vm, 1)?[0];
                    crate::inventory::add(
                        &mut self.declarations.windows,
                        (call.id - 6) as usize,
                        item,
                    )?;
                    self.inventory_dirty |= self.active_inventory.is_some();
                    process.vm.complete_library(call, 1, None)?;
                }
                28 => {
                    let item = args(&process.vm, 1)?[0];
                    crate::inventory::delete(&mut self.declarations.windows, item);
                    self.inventory_dirty |= self.active_inventory.is_some();
                    if self.held_object == item {
                        self.held_object = -1;
                        self.held_cursor = None;
                        self.inventory_dirty |= self.conversation.polygon.is_some();
                    }
                    process.vm.complete_library(call, 1, None)?;
                }
                29 => {
                    let actor = process
                        .effect_actor
                        .ok_or(Error("EFFECTACTOR requires effect context"))?;
                    process.vm.complete_library(call, 1, Some(actor))?;
                }
                32 => {
                    let slot = args(&process.vm, 1)?[0];
                    if !(1..=2).contains(&slot) {
                        return Err(Error("invalid inventory slot"));
                    }
                    let capacity = self.declarations.windows[slot as usize]
                        .as_ref()
                        .ok_or(Error("missing inventory declaration"))?
                        .capacity;
                    process.vm.complete_library(call, 1, Some(capacity))?;
                }
                65 => {
                    let a = args(&process.vm, 2)?;
                    if !(1..=2).contains(&a[0]) {
                        return Err(Error("invalid inventory slot"));
                    }
                    let window = self.declarations.windows[a[0] as usize]
                        .as_mut()
                        .ok_or(Error("missing inventory declaration"))?;
                    crate::inventory::set_limit(window, a[1])?;
                    process.vm.complete_library(call, 2, None)?;
                }
                35 => {
                    let item = args(&process.vm, 1)?[0];
                    let present = crate::inventory::contains(
                        &self.declarations.windows,
                        self.held_object,
                        item,
                    );
                    process
                        .vm
                        .complete_library(call, 1, Some(i32::from(present)))?;
                }
                109 => {
                    let item = args(&process.vm, 1)?[0];
                    let inventory =
                        crate::inventory::which(&self.declarations.windows, self.held_object, item);
                    process.vm.complete_library(call, 1, Some(inventory))?;
                }
                6 | 27 => {
                    let icon = args(&process.vm, 1)?[0];
                    let window = self.declarations.windows[0]
                        .as_mut()
                        .ok_or(Error("missing conversation declaration"))?;
                    let before = window.contents.clone();
                    if call.id == 6 {
                        crate::conversation::add_topic(window, icon)?;
                    } else {
                        crate::conversation::delete_topic(window, icon);
                    }
                    self.inventory_dirty |= window.contents != before;
                    process.vm.complete_library(call, 1, None)?;
                }
                76 => {
                    let a = args(&process.vm, 3)?;
                    let id = u32::try_from(a[0]).map_err(|_| Error("invalid actor ID"))?;
                    self.actors.get(id)?;
                    if !(0..=2).contains(&a[2]) {
                        return Err(Error("invalid actor tag type"));
                    }
                    self.actor_tags.insert(id, (Handle(a[1] as u32), a[2]));
                    process.vm.complete_library(call, 3, None)?;
                }
                83 => {
                    let id = u32::try_from(args(&process.vm, 1)?[0])
                        .map_err(|_| Error("invalid actor ID"))?;
                    self.actors.get(id)?;
                    self.actor_tags.remove(&id);
                    process.vm.complete_library(call, 1, None)?;
                }
                60 => {
                    process
                        .vm
                        .complete_library(call, 1, Some(self.conversation.icon))?;
                }
                10 | 89 => {
                    if call.id == 89 {
                        let id = u32::try_from(args(&process.vm, 41)?[0])
                            .map_err(|_| Error("invalid mover ID"))?;
                        self.actors.get(id)?;
                    }
                    self.declarations.apply(&mut process.vm, call, resources)?;
                }
                108 => {
                    let a = args(&process.vm, 7)?;
                    if !matches!(a[0], 1 | 2) {
                        return Err(Error("invalid SETINVSIZE inventory"));
                    }
                    let window = self.declarations.windows[a[0] as usize]
                        .as_mut()
                        .ok_or(Error("missing SETINVSIZE inventory"))?;
                    crate::inventory::set_size(window, [a[1], a[2]], [a[3], a[4]], [a[5], a[6]]);
                    process.vm.complete_library(call, 7, None)?;
                }
                52 => {
                    // V1 PREPARESCENE is only a preload hint. Pinned tinlib.cpp
                    // PrepareScene has no effect, even for an unknown handle.
                    process.vm.complete_library(call, 1, None)?;
                }
                39 | 63 => {
                    let id = args(&process.vm, 1)?[0];
                    self.polygons.set_block(id, call.id == 63);
                    process.vm.complete_library(call, 1, None)?;
                }
                40 | 41 | 64 | 67 => {
                    let id = args(&process.vm, 1)?[0];
                    let kind = if matches!(call.id, 40 | 64) {
                        crate::polygon::Kind::Exit
                    } else {
                        crate::polygon::Kind::Tag
                    };
                    self.polygon_memory.set(
                        self.polygon_table,
                        &mut self.polygons,
                        kind,
                        id,
                        matches!(call.id, 64 | 67),
                    )?;
                    process.vm.complete_library(call, 1, None)?;
                }
                77 => {
                    let (Some(strings), Some(bank)) = (strings, bank) else {
                        return Ok(Status::Unsupported(call));
                    };
                    let a = args(&process.vm, 2)?;
                    let film = Handle(a[0] as u32);
                    let actor = if film.0 <= 4 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead speaker"))?
                            .id
                    } else {
                        let reels = Self::start_film(resources, film, false)?;
                        reels.first().ok_or(Error("empty talk film"))?.object.id
                    };
                    if self
                        .movers
                        .get(&actor)
                        .is_some_and(|m| m.walk_target.is_some())
                    {
                        return Ok(Status::Unsupported(call));
                    }
                    self.actors.get_mut(
                        u32::try_from(actor).map_err(|_| Error("invalid speech actor"))?,
                    )?;
                    if let Some(id) = process.actor {
                        if !self.actors.get_mut(id)?.alive {
                            return Ok(Status::Halted);
                        }
                    }
                    let text_id = a[1];
                    let text = strings.get(text_id as u32)?.to_vec();
                    let sample = if bank.sample(text_id)?.is_some() {
                        self.sample_player.play(bank, text_id)?;
                        Some(self.sample_player.serial)
                    } else {
                        None
                    };
                    let restore_control = self
                        .declarations
                        .lead
                        .as_ref()
                        .is_some_and(|lead| lead.id == actor)
                        && self.control.enabled;
                    if restore_control {
                        self.control.apply_at(
                            0,
                            self.active_inventory.is_some(),
                            self.world_origin,
                        )?;
                    }
                    let restore_tags = self.control.tags_enabled;
                    self.control.tags_enabled = false;
                    let mover_saved = if let Some(mover) = self.movers.get_mut(&actor) {
                        Some(mover.push_talk(resources, &self.declarations, film)?)
                    } else {
                        if film.0 <= 4 {
                            return Err(Error("lead speaker has no mover"));
                        }
                        self.playback.begin_talk(actor, film);
                        self.launch_film(resources, film)?;
                        None
                    };
                    process.talk_wait = Some(TalkWait {
                        mover_saved,
                        call,
                        actor,
                        remaining: text.len() as u32 + 24,
                        text,
                        sample,
                        layout: None,
                        initialized: false,
                        timeout: 480,
                        left_events: self.left_events,
                        origin: self.world_origin,
                        restore_control,
                        restore_tags,
                    });
                    return Ok(Status::Running);
                }
                61 => {
                    let a = args(&process.vm, 4)?;
                    let target = [a[0], a[1]];
                    let serial = self.scroll.start(self.world_origin, target, a[2])?;
                    if a[3] != 0 {
                        process.scroll_wait = Some((call, serial, target));
                        return Ok(Status::Running);
                    }
                    process.vm.complete_library(call, 4, None)?;
                }
                1 | 3 | 4 | 5 => {
                    let id = args(&process.vm, 1)?[0];
                    let actor = if id == -2 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead actor"))?
                            .id
                    } else {
                        id
                    };
                    let stored = self
                        .actors
                        .get(u32::try_from(actor).map_err(|_| Error("invalid actor ID"))?)?
                        .position;
                    if call.id == 3 {
                        let scale = self
                            .movers
                            .get(&actor)
                            .ok_or(Error("ACTORSCALE actor has no mover"))?
                            .placement
                            .scale as i32;
                        process.vm.complete_library(call, 1, Some(scale))?;
                        continue;
                    }
                    if call.id == 1 {
                        let direction = self
                            .movers
                            .get(&actor)
                            .ok_or(Error("ACTORDIRECTION actor has no mover"))?
                            .placement
                            .direction
                            .index() as i32;
                        process.vm.complete_library(call, 1, Some(direction))?;
                        continue;
                    }
                    let position = self
                        .movers
                        .get(&actor)
                        .map_or(stored, |m| m.placement.position);
                    process
                        .vm
                        .complete_library(call, 1, Some(position[(call.id - 4) as usize]))?;
                }
                42 | 80 => {
                    let axis = usize::from(call.id == 80);
                    process
                        .vm
                        .complete_library(call, 1, Some(self.world_origin[axis]))?;
                }
                43 => {
                    let a = args(&process.vm, 2)?;
                    self.control
                        .script_cursor([a[0], a[1]], self.world_origin, self.ticks);
                    process.vm.complete_library(call, 2, None)?;
                }
                47 => {
                    let a = args(&process.vm, 2)?;
                    self.scroll.stop();
                    self.world_origin = [a[0], a[1]];
                    process.vm.complete_library(call, 2, None)?;
                }
                12 => {
                    let actor = args(&process.vm, 1)?[0];
                    self.scroll.set_focus(actor);
                    process.vm.complete_library(call, 1, None)?;
                }
                45 => {
                    let a = args(&process.vm, 4)?;
                    self.scroll.boundaries.add([a[0], a[1]], [a[2], a[3]])?;
                    process.vm.complete_library(call, 4, None)?;
                }
                88 => {
                    let a = args(&process.vm, 4)?;
                    if a[3] != 0 {
                        return Ok(Status::Unsupported(call));
                    }
                    let actor = if a[0] == -2 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead actor"))?
                            .id
                    } else {
                        a[0]
                    };
                    if !self.movers.contains_key(&actor) {
                        return Err(Error("WALKED actor has no mover"));
                    }
                    process.walked_wait = Some(WalkedWait {
                        call,
                        actor,
                        target: [a[1], a[2]],
                        delay: 24,
                        serial: None,
                    });
                    return Ok(Status::Running);
                }
                87 => {
                    let a = args(&process.vm, 5)?;
                    if a[3] != 0 {
                        return Ok(Status::Unsupported(call));
                    }
                    let actor = if a[0] == -2 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead actor"))?
                            .id
                    } else {
                        a[0]
                    };
                    let cursor = [
                        self.control.cursor[0]
                            .checked_add(self.world_origin[0])
                            .ok_or(Error("world cursor overflow"))?,
                        self.control.cursor[1]
                            .checked_add(self.world_origin[1])
                            .ok_or(Error("world cursor overflow"))?,
                    ];
                    let destination = crate::destination::resolve(
                        &self.polygons,
                        [a[1], a[2]],
                        self.world_origin,
                        [
                            self.scroll_maximum[0]
                                .checked_add(320)
                                .ok_or(Error("scene width overflow"))?,
                            self.scroll_maximum[1]
                                .checked_add(200)
                                .ok_or(Error("scene height overflow"))?,
                        ],
                    )?;
                    let mover = self
                        .movers
                        .get_mut(&actor)
                        .ok_or(Error("WALK actor has no mover"))?;
                    let serial = mover.begin_walk(
                        resources,
                        &self.declarations,
                        &self.polygons,
                        destination.unwrap_or(mover.placement.position),
                        cursor,
                    )?;
                    let moving = mover.walk_target.is_some();
                    self.scroll.follow_cursor = false;
                    self.reel_owners.remove(&actor);
                    self.films.retain(|a| a.object.id != actor);
                    if moving && a[4] != 2 {
                        process.walk_wait = Some((call, actor, serial, false));
                        return Ok(Status::Running);
                    }
                    process.vm.complete_library(call, 5, None)?;
                }
                91 => {
                    let a = args(&process.vm, 3)?;
                    let Some(polygon) = process.polygon else {
                        return Err(Error("WALKTAG requires polygon context"));
                    };
                    if a[1] != 0 {
                        return Ok(Status::Unsupported(call));
                    }
                    let actor = if a[0] == -2 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead actor"))?
                            .id
                    } else {
                        a[0]
                    };
                    let definition = &self
                        .polygons
                        .active
                        .get(polygon)
                        .ok_or(Error("invalid WALKTAG polygon"))?
                        .definition;
                    let target = definition.node_position;
                    let film = definition.film;
                    let cursor = [
                        self.control.cursor[0]
                            .checked_add(self.world_origin[0])
                            .ok_or(Error("world cursor overflow"))?,
                        self.control.cursor[1]
                            .checked_add(self.world_origin[1])
                            .ok_or(Error("world cursor overflow"))?,
                    ];
                    let destination = crate::destination::resolve(
                        &self.polygons,
                        target,
                        self.world_origin,
                        [
                            self.scroll_maximum[0]
                                .checked_add(320)
                                .ok_or(Error("scene width overflow"))?,
                            self.scroll_maximum[1]
                                .checked_add(200)
                                .ok_or(Error("scene height overflow"))?,
                        ],
                    )?;
                    let mover = self
                        .movers
                        .get_mut(&actor)
                        .ok_or(Error("WALKTAG actor has no mover"))?;
                    let serial = mover.begin_walk(
                        resources,
                        &self.declarations,
                        &self.polygons,
                        destination.unwrap_or(mover.placement.position),
                        cursor,
                    )?;
                    let moving = mover.walk_target.is_some();
                    self.scroll.follow_cursor = true;
                    if !moving {
                        mover.adopt_tag_pose(resources, &self.declarations, film)?;
                    }
                    self.reel_owners.remove(&actor);
                    self.films.retain(|a| a.object.id != actor);
                    if moving {
                        process.tag_walk_wait = Some((call, actor, serial, film));
                        return Ok(Status::Running);
                    }
                    process.vm.complete_library(call, 3, None)?;
                }
                75 => {
                    let a = args(&process.vm, 6)?;
                    let actor = if a[0] == -2 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead actor"))?
                            .id
                    } else {
                        a[0]
                    };
                    if a[5] != 0 || !self.declarations.movers.contains_key(&actor) {
                        return Ok(Status::Unsupported(call));
                    }
                    let start = [a[1], a[2]];
                    let target = [a[3], a[4]];
                    let mut placement = crate::mover::Placement::initial(
                        &self.declarations,
                        &self.polygons,
                        actor,
                        start,
                        Handle(0),
                    )?;
                    let initial_position = if placement.path.is_none() {
                        placement = crate::mover::Placement::initial(
                            &self.declarations,
                            &self.polygons,
                            actor,
                            target,
                            Handle(0),
                        )?;
                        target
                    } else {
                        start
                    };
                    if placement.path.is_none() || target.contains(&-1) {
                        return Ok(Status::Unsupported(call));
                    }
                    self.actors.get(actor as u32)?;
                    let mut mover = match self.movers.get(&actor) {
                        Some(mover) => {
                            let mut mover = mover.clone();
                            mover.stand(
                                resources,
                                &self.declarations,
                                &self.polygons,
                                initial_position,
                                Handle(0),
                            )?;
                            mover
                        }
                        None => crate::mover::Mover::new(resources, placement)?,
                    };
                    if initial_position != start {
                        mover.stand(
                            resources,
                            &self.declarations,
                            &self.polygons,
                            start,
                            Handle(0),
                        )?;
                    }
                    let cursor = [
                        self.control.cursor[0]
                            .checked_add(self.world_origin[0])
                            .ok_or(Error("world cursor overflow"))?,
                        self.control.cursor[1]
                            .checked_add(self.world_origin[1])
                            .ok_or(Error("world cursor overflow"))?,
                    ];
                    let serial = mover.begin_direct_walk(
                        resources,
                        &self.declarations,
                        &self.polygons,
                        target,
                        cursor,
                    )?;
                    let moving = mover.walk_target.is_some();
                    let position = mover.placement.position;
                    self.movers.insert(actor, mover);
                    self.scroll.follow_cursor = false;
                    self.actors.get_mut(actor as u32)?.position = position;
                    self.reel_owners.remove(&actor);
                    self.films.retain(|film| film.object.id != actor);
                    let restore_control = self.control.enabled
                        && self
                            .declarations
                            .lead
                            .as_ref()
                            .is_some_and(|lead| lead.id == actor);
                    if restore_control {
                        self.control.apply_at(3, false, self.world_origin)?;
                    }
                    if moving {
                        process.walk_wait = Some((call, actor, serial, restore_control));
                        return Ok(Status::Running);
                    }
                    if restore_control {
                        self.control.apply_at(1, false, self.world_origin)?;
                    }
                    process.vm.complete_library(call, 6, None)?;
                }
                74 => {
                    let requested = args(&process.vm, 1)?[0];
                    let actor = if requested == -2 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead actor"))?
                            .id
                    } else {
                        requested
                    };
                    self.movers
                        .get_mut(&actor)
                        .ok_or(Error("STOPWALK actor has no mover"))?
                        .stop_requested = true;
                    // V1 token acquisition kills normal WALK/SWALK/WALKTAG
                    // owners immediately. WALKED has already freed its token
                    // and must observe the changed walk serial instead.
                    let current = self.processes.current_id();
                    let owners: Vec<_> = self
                        .processes
                        .entries()
                        .filter(|(id, p)| {
                            Some(*id) != current
                                && (p.walk_wait.is_some_and(|(_, a, _, _)| a == actor)
                                    || p.tag_walk_wait.is_some_and(|(_, a, _, _)| a == actor))
                        })
                        .map(|(id, _)| id)
                        .collect();
                    for id in owners {
                        self.processes.cancel(id)?;
                    }
                    process.vm.complete_library(call, 1, None)?;
                }
                72 | 73 => {
                    let consumed = if call.id == 73 { 1 } else { 4 };
                    let a = if call.id == 73 {
                        let actor = args(&process.vm, 1)?[0];
                        let polygon = process
                            .polygon
                            .ok_or(Error("STANDTAG requires polygon context"))?;
                        let tag = &self
                            .polygons
                            .active
                            .get(polygon)
                            .ok_or(Error("invalid STANDTAG polygon"))?
                            .definition;
                        let lead = actor == -2
                            || self
                                .declarations
                                .lead
                                .as_ref()
                                .is_some_and(|lead| lead.id == actor);
                        [
                            actor,
                            tag.node_position[0],
                            tag.node_position[1],
                            if lead { tag.film.0 as i32 } else { 0 },
                        ]
                    } else {
                        args(&process.vm, 4)?
                            .try_into()
                            .map_err(|_| Error("invalid STAND arguments"))?
                    };
                    let actor = if a[0] == -2 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead actor"))?
                            .id
                    } else {
                        a[0]
                    };
                    if !self.declarations.movers.contains_key(&actor) {
                        return Ok(Status::Unsupported(call));
                    }
                    let id = u32::try_from(actor).map_err(|_| Error("invalid mover actor"))?;
                    self.actors.get(id)?;
                    if let Some(mover) = self.movers.get_mut(&actor) {
                        mover.stand(
                            resources,
                            &self.declarations,
                            &self.polygons,
                            [a[1], a[2]],
                            Handle(a[3] as u32),
                        )?;
                    } else {
                        let placement = crate::mover::Placement::initial(
                            &self.declarations,
                            &self.polygons,
                            actor,
                            [a[1], a[2]],
                            Handle(a[3] as u32),
                        )?;
                        let mover = crate::mover::Mover::new(resources, placement)?;
                        self.movers.insert(actor, mover);
                    }
                    self.actors.get_mut(id)?.position = self.movers[&actor].placement.position;
                    process.vm.complete_library(call, consumed, None)?;
                }
                38 => {
                    let actor = args(&process.vm, 1)?[0];
                    self.disable_actor(
                        u32::try_from(actor).map_err(|_| Error("invalid actor ID"))?,
                    )?;
                    process.vm.complete_library(call, 1, None)?;
                }
                62 => {
                    let id = u32::try_from(args(&process.vm, 1)?[0])
                        .map_err(|_| Error("invalid actor ID"))?;
                    if let Some(code) = self.actors.enable(id)? {
                        scene::script_bytes(resources, code)?;
                        let mut child = ScriptProcess::new(code);
                        child.actor = Some(id);
                        self.processes.spawn(Some(self.generation), child)?;
                    }
                    process.vm.complete_library(call, 1, None)?;
                }
                56 => {
                    let a = args(&process.vm, 3)?;
                    let value = self.random.range(a[0], a[1], a[2]);
                    process.vm.complete_library(call, 3, Some(value))?;
                }
                54 => {
                    if process.inventory_object.is_none() {
                        return Err(Error("PRINTOBJ requires inventory context"));
                    }
                    let text = args(&process.vm, 1)?[0];
                    // POINTED arbitration and ON/OFF await the hover lifecycle.
                    if text < 0 || process.event == 0 {
                        return Ok(Status::Unsupported(call));
                    }
                    let (Some(strings), Some(bank)) = (strings, bank) else {
                        return Ok(Status::Unsupported(call));
                    };
                    let Some(position) = process.object_print_position.take() else {
                        let Some(index) = self.inventory_slot_at(self.control.cursor) else {
                            process.vm.complete_library(call, 1, None)?;
                            continue;
                        };
                        let [x, y] = self.inventory_display.as_ref().unwrap().0.layout.icons[index];
                        process.object_print_position = Some([x + 12, y]);
                        // V1 lets POINTED text die for one scheduler tick.
                        return Ok(Status::Running);
                    };
                    let bytes = strings.get(text as u32)?;
                    let speech = bank.sample(text)?.is_some();
                    let layout = if self.subtitles || !speech {
                        let font = crate::text::Font::read(
                            resources,
                            self.declarations
                                .tag_font
                                .ok_or(Error("missing tag font"))?,
                        )?;
                        let mut layout = font.centered(resources, bytes, position)?;
                        layout.keep_inventory_horizontal(resources, 320)?;
                        Some(layout)
                    } else {
                        None
                    };
                    let sample_serial = if speech {
                        self.sample_player.play(bank, text)?;
                        Some(self.sample_player.serial)
                    } else {
                        None
                    };
                    let restore_control = self.control.enabled;
                    if restore_control {
                        self.control.apply_at(0, true, self.world_origin)?;
                    }
                    process.print_wait = Some(PrintWait {
                        inventory: true,
                        restore_control,
                        call,
                        consumed: 1,
                        layout,
                        remaining: bytes.len() as u32 + 24,
                        sample_serial,
                        timeout: 480,
                        left_events: self.left_events,
                    });
                    return Ok(Status::Running);
                }
                53 | 98 | 104 => {
                    let (Some(strings), Some(bank)) = (strings, bank) else {
                        return Ok(Status::Unsupported(call));
                    };
                    let consumed = match call.id {
                        53 => 6,
                        98 => 4,
                        _ => 5,
                    };
                    let a = args(&process.vm, consumed)?;
                    let (x, y, text, time, sustain, actor) = if call.id == 53 {
                        (a[0], a[1], a[3] as u32, a[4], a[5] == 2, None)
                    } else {
                        if call.id == 104 && a[4] != 2 {
                            return Err(Error("invalid TALKATS sustain"));
                        }
                        (a[1], a[2], a[3] as u32, 0, call.id == 104, Some(a[0]))
                    };
                    if time < 0 {
                        return Err(Error("negative print time"));
                    }
                    let bytes = strings.get(text)?;
                    let speech = bank.sample(text as i32)?.is_some();
                    if let Some(actor) = actor {
                        if actor != -1 {
                            self.actors.get_mut(
                                u32::try_from(actor).map_err(|_| Error("invalid speech actor"))?,
                            )?;
                        }
                        if self.subtitles || !speech || call.id == 104 {
                            let specific = self
                                .declarations
                                .speech_colors
                                .get(&actor)
                                .copied()
                                .filter(|c| *c != [0; 3]);
                            self.talk_color = Some(
                                specific
                                    .or_else(|| self.declarations.speech_colors.get(&-1).copied())
                                    .unwrap_or([0; 3]),
                            );
                        }
                    }
                    let layout = if self.subtitles || !speech {
                        let handle = self
                            .declarations
                            .talk_font
                            .ok_or(Error("missing talk font"))?;
                        let font = crate::text::Font::read(resources, handle)?;
                        if font.flags != 1 {
                            return Err(Error("unsupported font draw mode"));
                        }
                        let mut layout = font.centered(resources, bytes, [x, y])?;
                        layout.keep_visible(resources, 320, 200)?;
                        Some(layout)
                    } else {
                        None
                    };
                    let remaining = if time == 0 {
                        bytes.len() as u32 + 24
                    } else {
                        (time as u32)
                            .checked_mul(24)
                            .ok_or(Error("print time overflow"))?
                    };
                    let sample_serial = if speech {
                        self.sample_player.play(bank, text as i32)?;
                        Some(self.sample_player.serial)
                    } else {
                        None
                    };
                    process.print_wait = Some(PrintWait {
                        inventory: false,
                        restore_control: false,
                        call,
                        consumed,
                        layout,
                        remaining,
                        sample_serial,
                        timeout: 480,
                        left_events: if time == 0 && !sustain {
                            self.left_events
                        } else {
                            0
                        },
                    });
                    return Ok(Status::Running);
                }
                14 => {
                    let mode = args(&process.vm, 1)?[0];
                    self.control.apply_at(
                        mode,
                        self.active_inventory.is_some(),
                        self.world_origin,
                    )?;
                    if mode == 1 {
                        if let Some((handle, _)) = &self.declarations.cursor {
                            self.cursor_animation =
                                Self::start_film(resources, *handle, true)?.pop();
                            if let Some(cursor) = &mut self.cursor_animation {
                                cursor.tick(resources)?;
                            }
                        }
                    }
                    self.menu_enabled = false;
                    process.vm.complete_library(call, 1, None)?;
                }
                96 => {
                    let seconds = self.control.idle_seconds(self.ticks)?;
                    process.vm.complete_library(call, 1, Some(seconds))?;
                }
                97 => {
                    self.control.user_event(self.ticks);
                    process.vm.complete_library(call, 0, None)?;
                }
                92 => {
                    process.vm.complete_library(
                        call,
                        1,
                        Some(self.active_inventory.unwrap_or(0)),
                    )?;
                }
                50 => {
                    let Some(tracks) = tracks else {
                        return Ok(Status::Unsupported(call));
                    };
                    let a = args(&process.vm, 3)?;
                    if !(0..=1).contains(&a[1]) {
                        return Err(Error("invalid MIDI loop mode"));
                    }
                    self.music.play(tracks, a[0] as u32, a[1] == 1)?;
                    if a[2] != 0 && self.music.playing() {
                        process.music_wait = Some(call);
                        return Ok(Status::Running);
                    }
                    process.vm.complete_library(call, 3, None)?;
                }
                102 => {
                    self.music.stop();
                    process.vm.complete_library(call, 0, None)?;
                }
                113 => {
                    process.vm.complete_library(call, 0, None)?;
                    self.menu_enabled = true;
                }
                85 => {
                    process.key_wait = Some(KeyWait {
                        call,
                        events: self.user_events,
                        cursor: self.control.cursor,
                    });
                    return Ok(Status::Running);
                }
                110 => {
                    self.no_blocking = true;
                    process.vm.complete_library(call, 0, None)?;
                }
                46 => {
                    let item = args(&process.vm, 1)?[0];
                    self.hold_object(resources, item)?;
                    process.vm.complete_library(call, 1, None)?;
                }
                33 => {
                    process
                        .vm
                        .complete_library(call, 1, Some(self.held_object))?;
                }
                31 => {
                    process.vm.complete_library(call, 1, Some(process.event))?;
                }
                82 => {
                    let mode = args(&process.vm, 1)?[0];
                    let frame = if mode == 1 {
                        Some(crate::conversation_window::Frame::top_window(
                            resources,
                            self.declarations
                                .window_parts
                                .ok_or(Error("missing window parts"))?,
                        )?)
                    } else {
                        None
                    };
                    self.close_inventory();
                    self.top_window = frame;
                    process.vm.complete_library(call, 1, None)?;
                }
                100 => {
                    let a = args(&process.vm, 2)?;
                    if a[0] <= 0 || a[1] < 0 {
                        return Err(Error("invalid WAITFRAME arguments"));
                    }
                    if self.actor_steps.get(&a[0]).copied().unwrap_or(0) < a[1] as u32 {
                        return Ok(Status::Running);
                    }
                    process.vm.complete_library(call, 2, None)?;
                }
                103 => {
                    self.sample_player.stop();
                    process.vm.complete_library(call, 0, None)?;
                }
                111 => {
                    // V1 takes no arguments but writes into the caller's
                    // allocated result slot, without yielding or consuming audio.
                    process.vm.complete_library(
                        call,
                        1,
                        Some(i32::from(self.sample_player.id.is_some())),
                    )?;
                }
                51 | 112 => {
                    if call.id == 112 && self.sample_player.id.is_some() {
                        args(&process.vm, 2)?;
                        // V1 yields once even when declining playback, so
                        // tight ambient-sample loops cannot starve scripts.
                        process.wait = Some((call, 1));
                        return Ok(Status::Running);
                    }
                    let Some(bank) = bank else {
                        return Ok(Status::Unsupported(call));
                    };
                    let a = args(&process.vm, 2)?;
                    let (id, blocking) = (a[0], a[1] != 0);
                    if !self.sample_player.play(bank, id)? {
                        process.wait = Some((call, 1));
                        return Ok(Status::Running);
                    }
                    if blocking && self.sample_player.id.is_some() {
                        process.sample_wait = Some(call);
                        return Ok(Status::Running);
                    }
                    process.vm.complete_library(call, 2, None)?;
                }
                11 => {
                    let a = args(&process.vm, 1)?;
                    self.start_background(resources, Handle(a[0] as u32))?;
                    process.vm.complete_library(call, 1, None)?;
                }
                49 | 81 => {
                    if let Some(id) = process.actor {
                        if !self.actors.get(id)?.alive {
                            return Ok(Status::Halted);
                        }
                    }
                    let a = args(&process.vm, 6)?;
                    if !matches!(a[5], 0 | 1) {
                        return Ok(Status::Unsupported(call));
                    }
                    let film = Handle(a[0] as u32);
                    let reels = if call.id == 81 {
                        self.prepare_top_film(resources, film, [a[1], a[2]])?
                    } else {
                        self.prepare_actor_film(resources, film, [a[1], a[2]])?
                    };
                    for reel in &reels {
                        self.schedule_actor_film(reel.object.id, film);
                    }
                    let blocking = a[5] == 1;
                    let mut primary = None;
                    for (column, mut reel) in reels.into_iter().enumerate() {
                        if self.playback.start(reel.object.id, film)
                            && self.claim_reel(&mut reel)?
                        {
                            if blocking && column == 0 {
                                primary = Some(reel);
                            } else {
                                self.films.push(reel);
                            }
                        }
                    }
                    if let Some(mut primary) = primary {
                        // V1 PlayFilmc invokes the primary reel in the calling coroutine.
                        // Secondary reels have independent lifetimes and do not delay return.
                        let state = primary.tick(resources)?;
                        self.actor_steps.insert(primary.object.id, primary.steps);
                        if state != Tick::Finished {
                            self.actors.get_mut(primary.object.id as u32)?.position =
                                [primary.object.x, primary.object.y];
                            process.play_wait = Some((call, primary));
                            return Ok(Status::Running);
                        }
                        self.release_reel(&primary);
                    }
                    process.vm.complete_library(call, 6, None)?;
                }
                86 => {
                    let a = args(&process.vm, 2)?;
                    let time = u32::try_from(a[0]).map_err(|_| Error("negative wait time"))?;
                    let duration = time
                        .checked_mul(if a[1] == 0 { 24 } else { 1 })
                        .and_then(|n| n.checked_add(1))
                        .ok_or(Error("wait overflow"))?;
                    process.wait = Some((call, duration));
                    return Ok(Status::Running);
                }
                44 => {
                    let a = args(&process.vm, 3)?;
                    let request = crate::scene_hook::Request::new(Handle(a[0] as u32), a[1], a[2])?;
                    let request = self.scene_hooks.intercept(request);
                    self.queue_scene_request(request);
                    self.control.apply_at(4, false, self.world_origin)?;
                    return Ok(Status::Halted);
                }
                95 => {
                    let a = args(&process.vm, 3)?;
                    self.scene_hooks.set(crate::scene_hook::Request::new(
                        Handle(a[0] as u32),
                        a[1],
                        a[2],
                    )?);
                    process.vm.complete_library(call, 3, None)?;
                }
                99 => {
                    let request = self.scene_hooks.release()?;
                    self.queue_scene_request(request);
                    process.vm.complete_library(call, 0, None)?;
                }
                107 => {
                    self.scene_hooks.clear();
                    process.vm.complete_library(call, 0, None)?;
                }
                _ => return Ok(Status::Unsupported(call)),
            }
        }
        Err(Error("scene script instruction budget"))
    }
    pub fn render(&self, resources: &Resources<'_>) -> Result<Surface, Error> {
        let mut result = Surface::new(320, 200)?;
        if let Some(background) = &self.background {
            if let Some(image) = background.images(resources)?.first() {
                let palette = crate::graphics::Image::read(resources, *image)?.palette;
                result.enable_backdrop(&crate::graphics::read_palette(resources, palette)?)?;
            }
            result.draw_animation_offset(resources, background, self.world_origin)?;
        }
        let mut films = self.films.iter().collect::<Vec<_>>();
        films.extend(
            self.processes
                .states()
                .filter_map(|p| p.play_wait.as_ref().map(|(_, a)| a)),
        );
        films.retain(|a| self.owns_reel(a));
        films.extend(
            self.movers
                .iter()
                .filter(|(id, _)| !self.reel_owners.contains_key(id))
                .map(|(_, m)| &m.animation),
        );
        let (status_films, world_films): (Vec<_>, Vec<_>) =
            films.into_iter().partition(|a| a.status_layer);
        result.draw_animations_ordered(resources, world_films, self.world_origin)?;
        if let Some(frame) = &self.top_window {
            frame.draw(resources, &mut result)?;
            result.draw_animations_ordered(resources, status_films.iter().copied(), [0, 0])?;
        }
        for process in self.processes.states() {
            if let Some(layout) = process
                .talk_wait
                .as_ref()
                .and_then(|wait| wait.layout.as_ref())
            {
                layout.draw_colored(resources, &mut result, self.talk_color)?;
            }
            if let Some(wait) = &process.print_wait {
                if !wait.inventory {
                    if let Some(layout) = &wait.layout {
                        layout.draw_colored(resources, &mut result, self.talk_color)?;
                    }
                }
            }
        }
        if self.active_inventory.is_some() && !self.conversation.hidden {
            if let Some((frame, content)) = &self.inventory_display {
                frame.draw(resources, &mut result)?;
                content.draw(resources, &mut result)?;
            }
        }
        if self.top_window.is_none() {
            result.draw_animations_ordered(resources, status_films, [0, 0])?;
        }
        for process in self.processes.states() {
            if let Some(wait) = &process.print_wait {
                if wait.inventory {
                    if let Some(layout) = &wait.layout {
                        layout.draw(resources, &mut result)?;
                    }
                }
            }
        }
        if !self.control.hidden {
            if let Some(cursor) = &self.held_cursor {
                cursor.draw(resources, &mut result, self.control.cursor)?;
            }
            if let Some(cursor) = &self.cursor_animation {
                let mut cursor = cursor.clone();
                cursor.object.x = self.control.cursor[0];
                cursor.object.y = self.control.cursor[1];
                result.draw_animation(resources, &cursor)?;
            }
        }
        for pixel in result.rgba.chunks_exact_mut(4) {
            for color in &mut pixel[..3] {
                *color = ((u32::from(*color) * self.fade_multiplier) >> 16) as u8;
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixtures() -> (Vec<u8>, Vec<u8>) {
        fn put(b: &mut [u8], at: usize, value: u32) {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut master = vec![0; 32];
        put(&mut master, 0, 0x33340010);
        put(&mut master, 4, 12);
        put(&mut master, 8, 1);
        put(&mut master, 12, 0x3334000a);
        master[20..30].copy_from_slice(&[1, 0, 0, 128, 0, 3, 2, 0x4e, 44, 0]);
        let mut scene = vec![0; 120];
        put(&mut scene, 0, 0x3334000a);
        put(&mut scene, 4, 64);
        scene[8..13].copy_from_slice(&[3, 3, 0x4e, 86, 0]);
        put(&mut scene, 64, 0x3334000e);
        put(&mut scene, 68, 104);
        put(&mut scene, 72, 1);
        put(&mut scene, 88, (1 << 23) | 8);
        put(&mut scene, 92, (1 << 23) | 112);
        put(&mut scene, 104, 0x3334000b);
        put(&mut scene, 112, 1);
        (master, scene)
    }
    #[test]
    fn scene_walking_actor_consumes_reels_and_preserves_lower_stack() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let mut code = vec![0x0a, 0x00, 0x34, 0x33, 0, 0, 0, 0];
        code.extend([0x41, 77, 3]);
        for reel in 1..=40 {
            code.extend([0x41, reel]);
        }
        code.extend([0x4e, 89, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene, &code]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(1).unwrap();
        let mut process = ScriptProcess::new(Handle((2 << 23) | 8));
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(engine.globals, [77]);
        let banks = &engine.declarations.movers[&1];
        assert_eq!(banks[0][0], Handle(1));
        assert_eq!(banks[4][7], Handle(40));
        assert_eq!(banks[9][0], Handle(33));
        assert_eq!(banks[9][4], Handle(21));
        assert!(engine.movers.is_empty()); // Declaration does not position or activate it.
        engine.declarations.movers.get_mut(&1).unwrap()[0][8] = Handle(123);
        let mut repeated = ScriptProcess::new(Handle((2 << 23) | 8));
        engine
            .run_process(&resources, None, None, None, &mut repeated)
            .unwrap();
        assert_eq!(engine.declarations.movers[&1][0][8], Handle(123));
    }
    #[test]
    fn script_timers_share_master_scene_state_and_tick_after_scripts() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [
            0x41, 77, 3, 0x41, 10, 3, 3, 0x4e, 68, 3, 0x4e, 79, 0x4c, 0, 0x4c, 1, 0,
        ];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        for master_script in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.globals.resize(2, 0);
            let script = Handle((1 << 23) | 32);
            if master_script {
                engine.master_code = script;
                engine.master = Machine::default();
                assert_eq!(engine.run_master(&resources).unwrap(), None);
            } else {
                let mut process = ScriptProcess::new(script);
                assert_eq!(
                    engine
                        .run_process(&resources, None, None, None, &mut process)
                        .unwrap(),
                    Status::Halted
                );
            }
            assert_eq!(engine.globals, [10, 77]);
            assert_eq!(engine.timers.get(1), 10);
            engine.enter(&resources, Handle(1 << 23), 1, 0).unwrap();
            assert_eq!(engine.timers.get(1), 10);
            engine.tick(&resources).unwrap();
            assert_eq!(engine.timers.get(1), 11);
        }
    }
    #[test]
    fn inventory_gestures_dispatch_scripted_drop_and_reject_invalid_input() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        scene[32..38].copy_from_slice(&[2, 0x4e, 31, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.active_inventory = Some(2);
        engine.declarations.windows[2] = Some(crate::startup::Window {
            title: Handle(0),
            capacity: 2,
            min: [1, 1],
            size: [2, 1],
            max: [10, 6],
            position: [0; 2],
            first_display: 0,
            contents: vec![],
        });
        engine.inventory_objects.insert(
            284,
            crate::inventory::Object {
                id: 284,
                film: Handle(0),
                script: Handle((1 << 23) | 32),
                attributes: 4,
            },
        );
        engine.held_object = 284;
        assert!(
            !engine
                .interact_inventory(&resources, 2, PolygonInteraction::WalkTo)
                .unwrap()
        );
        assert!(
            engine
                .interact_inventory(&resources, 1, PolygonInteraction::WalkTo)
                .unwrap()
        );
        assert_eq!(engine.inventory_event_index, 1);
        assert!(
            !engine
                .interact_inventory(&resources, 0, PolygonInteraction::WalkTo)
                .unwrap()
        );
        // No display was built in this fixture; avoid unrelated redraw work.
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [9]);
        assert_eq!(engine.held_object, 284);
        assert!(
            engine.declarations.windows[2]
                .as_ref()
                .unwrap()
                .contents
                .is_empty()
        );
        engine.inventory_objects.get_mut(&284).unwrap().attributes = 1;
        assert!(
            !engine
                .interact_inventory(&resources, 0, PolygonInteraction::WalkTo)
                .unwrap()
        );
        engine.inventory_objects.get_mut(&284).unwrap().attributes = 0;
        assert!(
            engine
                .interact_inventory(&resources, 0, PolygonInteraction::WalkTo)
                .unwrap()
        );
        assert_eq!(engine.held_object, -1);
        assert_eq!(
            engine.declarations.windows[2].as_ref().unwrap().contents,
            [284]
        );
        // A visible cell maps through FirstDisp for both item dispatch and drop insertion.
        let window = engine.declarations.windows[2].as_mut().unwrap();
        window.contents = vec![10, 11, 12, 284];
        window.capacity = 6;
        window.first_display = 2;
        engine.last_interaction_tick = None;
        assert!(
            engine
                .interact_inventory(&resources, 1, PolygonInteraction::Look)
                .unwrap()
        );
        assert_eq!(engine.inventory_event_index, 3);
        engine.last_interaction_tick = None;
        engine.held_object = 284;
        assert!(
            engine
                .interact_inventory(&resources, 0, PolygonInteraction::WalkTo)
                .unwrap()
        );
        assert_eq!(
            engine.declarations.windows[2].as_ref().unwrap().contents,
            [10, 11, 284, 12]
        );
    }

    #[test]
    fn inventory_open_while_active_is_ignored_and_scripted_close_restores_tags() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        // INVENTORY(2), CLOSEINVENTORY, return. No window assets are needed:
        // the existing inventory must suppress construction of the new one.
        scene[32..39].copy_from_slice(&[0x41, 2, 0x4e, 37, 0x4e, 13, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.active_inventory = Some(1);
        engine.control.tags_enabled = false;
        engine
            .processes
            .spawn(
                Some(engine.generation),
                ScriptProcess::new(Handle((1 << 23) | 32)),
            )
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.active_inventory(), None);
        assert!(engine.control.tags_enabled);
        assert!(engine.inventory_display.is_none());
        engine.close_inventory();
        assert!(engine.control.tags_enabled);
    }

    #[test]
    fn converse_lifecycle_retains_control_ownership_across_waits() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        // EVENT -> global 0, WAITTIME(2, ticks), return.
        scene[32..44].copy_from_slice(&[2, 0x4e, 31, 0x4c, 0, 0x41, 2, 3, 0x4e, 86, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        for initially_enabled in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.conversation.polygon = Some(40);
            engine.active_inventory = Some(0);
            if initially_enabled {
                engine.control.apply(1, true).unwrap();
            }
            let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
            process.event = 7;
            process.polygon = Some(40);
            engine
                .processes
                .spawn(Some(engine.generation), process)
                .unwrap();
            engine.tick(&resources).unwrap();
            assert_eq!(engine.globals, [7]);
            assert!(!engine.control.enabled);
            assert!(engine.conversation.hidden);
            let mut restored = engine.clone();
            engine.tick(&resources).unwrap();
            restored.tick(&resources).unwrap();
            assert_eq!(engine, restored);
            assert!(engine.conversation.hidden);
            engine.tick(&resources).unwrap();
            assert!(engine.conversation.hidden); // WAITTIME includes its V1 extra yield.
            engine.tick(&resources).unwrap();
            assert!(!engine.conversation.hidden);
            assert_eq!(engine.control.enabled, initially_enabled);
            assert!(!engine.control.tags_enabled);
        }
    }
    #[test]
    fn converse_completion_does_not_reopen_a_closed_window() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        scene[32..37].copy_from_slice(&[3, 3, 0x4e, 86, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.control.apply(1, true).unwrap();
        engine.conversation.polygon = Some(40);
        engine.active_inventory = Some(0);
        let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
        process.event = 7;
        process.polygon = Some(40);
        engine
            .processes
            .spawn(Some(engine.generation), process)
            .unwrap();
        engine.tick(&resources).unwrap();
        engine.conversation.end();
        engine.active_inventory = None;
        engine.tick(&resources).unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.conversation.polygon, None);
        assert!(engine.conversation.hidden); // hideConversation(false) is a no-op when closed.
        assert!(engine.control.enabled);
        assert!(engine.control.tags_enabled);
        engine.conversation.polygon = Some(40);
        engine.enter(&resources, Handle(1 << 23), 1, 1).unwrap();
        assert_eq!(engine.conversation.polygon, None);
    }
    #[test]
    fn actor_input_runs_with_context_without_polygon_control_recheck() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        scene[32..38].copy_from_slice(&[2, 0x4e, 31, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(1).unwrap();
        engine.actors.get_mut(1).unwrap().script = Handle((1 << 23) | 32);
        assert!(
            !engine
                .interact_actor(&resources, 1, PolygonInteraction::Action)
                .unwrap()
        );
        engine.control.apply(1, false).unwrap();
        let before = engine.clone();
        assert!(
            engine
                .interact_actor(&resources, 2, PolygonInteraction::Action)
                .is_err()
        );
        assert_eq!(engine, before);
        assert!(
            engine
                .interact_actor(&resources, 1, PolygonInteraction::Look)
                .unwrap()
        );
        assert!(
            !engine
                .interact_actor(&resources, 1, PolygonInteraction::Action)
                .unwrap()
        );
        engine.control.apply(0, false).unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [3]);
        assert!(engine.actors.get(1).unwrap().completed);
        engine.control.apply(1, false).unwrap();
        engine.actors.get_mut(1).unwrap().hidden = true;
        assert!(
            !engine
                .interact_actor(&resources, 1, PolygonInteraction::Look)
                .unwrap()
        );
        engine.actors.get_mut(1).unwrap().hidden = false;
        engine.actors.disable(1).unwrap();
        assert!(
            !engine
                .interact_actor(&resources, 1, PolygonInteraction::Look)
                .unwrap()
        );
    }
    #[test]
    fn inventory_library_calls_transfer_query_and_delete_held_item() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        scene[32..48].copy_from_slice(&[
            0x41, 10, 0x4e, 7, 0x41, 10, 0x4e, 8, 0x41, 10, 0x4e, 109, 0x4c, 0, 0, 0,
        ]);
        scene[56..61].copy_from_slice(&[0x41, 10, 0x4e, 28, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        let window = crate::startup::Window {
            title: Handle(0),
            capacity: 10,
            min: [1, 1],
            size: [1, 1],
            max: [10, 6],
            position: [0; 2],
            first_display: 0,
            contents: vec![],
        };
        engine.declarations.windows[1] = Some(window.clone());
        engine.declarations.windows[2] = Some(window);
        engine
            .processes
            .spawn(
                Some(engine.generation),
                ScriptProcess::new(Handle((1 << 23) | 32)),
            )
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [2]);
        assert!(
            engine.declarations.windows[1]
                .as_ref()
                .unwrap()
                .contents
                .is_empty()
        );
        assert_eq!(
            engine.declarations.windows[2].as_ref().unwrap().contents,
            [10]
        );
        engine.held_object = 10;
        engine
            .processes
            .spawn(
                Some(engine.generation),
                ScriptProcess::new(Handle((1 << 23) | 56)),
            )
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.held_object, -1);
        assert!(
            engine.declarations.windows[1..]
                .iter()
                .flatten()
                .all(|w| w.contents.is_empty())
        );
    }
    #[test]
    fn conversation_selection_and_close_dispatch_scanicon_atomically() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        scene[32..38].copy_from_slice(&[2, 0x4e, 60, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.polygons = crate::polygon::Polygons::new(
            vec![crate::polygon::Polygon {
                kind: crate::polygon::Kind::Tag,
                corners: [[0, 0], [20, 0], [20, 20], [0, 20]],
                tag_position: [0; 2],
                tag_text: Handle(0),
                node_position: [0; 2],
                film: Handle(0),
                refer_type: 0,
                id: 1,
                scales: [1, 0],
                reel: 0,
                z_factor: 0,
                nodes: vec![],
                lines: vec![],
                script: Handle((1 << 23) | 32),
            }],
            0,
        );
        engine.declarations.windows[0] = Some(crate::startup::Window {
            title: Handle(0),
            capacity: 10,
            min: [1, 1],
            size: [3, 1],
            max: [10, 1],
            position: [20, 8],
            first_display: 0,
            contents: vec![327, 328, 331],
        });
        engine.conversation.polygon = Some(0);
        engine.active_inventory = Some(0);
        engine.control.apply(1, true).unwrap();
        assert!(
            !engine
                .interact_polygon(&resources, 0, PolygonInteraction::Action)
                .unwrap()
        );
        let before = engine.clone();
        assert!(engine.interact_conversation(&resources, Some(3)).is_err());
        assert_eq!(engine, before);
        assert!(engine.interact_conversation(&resources, Some(1)).unwrap());
        assert!(!engine.interact_conversation(&resources, Some(0)).unwrap());
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [328]);
        assert!(engine.control.enabled);
        assert!(!engine.conversation.hidden);
        engine.conversation.hidden = true;
        assert!(!engine.interact_conversation(&resources, None).unwrap());
        engine.conversation.hidden = false;
        assert!(engine.interact_conversation(&resources, None).unwrap());
        assert_eq!(engine.conversation.polygon, None);
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [-1]);
        assert!(engine.control.enabled);
        assert!(engine.control.tags_enabled);
    }
    #[test]
    fn polygon_interactions_dispatch_event_and_recheck_control() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        scene[32..38].copy_from_slice(&[2, 0x4e, 31, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.polygons = crate::polygon::Polygons::new(
            vec![crate::polygon::Polygon {
                kind: crate::polygon::Kind::Tag,
                corners: [[0, 0], [20, 0], [20, 20], [0, 20]],
                tag_position: [0; 2],
                tag_text: Handle(0),
                node_position: [0; 2],
                film: Handle(0),
                refer_type: 0,
                id: 1,
                scales: [1, 0],
                reel: 0,
                z_factor: 0,
                nodes: vec![],
                lines: vec![],
                script: Handle((1 << 23) | 32),
            }],
            0,
        );
        assert!(
            !engine
                .interact_polygon(&resources, 0, PolygonInteraction::Look)
                .unwrap()
        );
        engine.control.apply(1, false).unwrap();
        engine.polygons.active[0].enabled = false;
        assert!(
            !engine
                .interact_polygon(&resources, 0, PolygonInteraction::Look)
                .unwrap()
        );
        engine.polygons.active[0].enabled = true;
        let before = engine.clone();
        assert!(
            engine
                .interact_polygon(&resources, 99, PolygonInteraction::Look)
                .is_err()
        );
        assert_eq!(engine, before);
        assert!(
            engine
                .interact_polygon(&resources, 0, PolygonInteraction::Look)
                .unwrap()
        );
        assert!(
            !engine
                .interact_polygon(&resources, 0, PolygonInteraction::Action)
                .unwrap()
        );
        let mut disabled = engine.clone();
        disabled.control.apply(0, false).unwrap();
        assert_eq!(disabled.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(disabled.globals, [0]);
        let mut restored = engine.clone();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.globals, [3]);
        restored.tick(&resources).unwrap();
        assert_eq!(engine, restored);
        assert!(
            engine
                .interact_polygon(&resources, 0, PolygonInteraction::Action)
                .unwrap()
        );
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [2]);
        assert!(
            engine
                .interact_polygon(&resources, 0, PolygonInteraction::WalkTo)
                .unwrap()
        );
        engine.enter(&resources, Handle(1 << 23), 1, 1).unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(
            engine.globals,
            [2],
            "scene entry cancels queued interactions"
        );
    }
    #[test]
    fn entrance_runs_while_scene_sleeps_and_shares_globals() {
        let (master, mut scene) = fixtures();
        scene[8..20].copy_from_slice(&[3, 0x4c, 0, 3, 3, 0x4e, 86, 0x41, 2, 0x4c, 0, 0]);
        scene[32..37].copy_from_slice(&[0x41, 9, 0x4c, 0, 0]);
        scene[116..120].copy_from_slice(&((1u32 << 23) | 32).to_le_bytes());
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        assert_eq!(engine.globals, [9]);
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        assert_eq!(engine.globals, [9]);
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.globals, [2]);
    }
    #[test]
    fn scene_change_cancels_old_entrance_and_dispatches_new_scene() {
        let (master, mut old) = fixtures();
        let (_, mut new) = fixtures();
        // NEWSCENE(resource 2, entrance 1, transition 0).
        old[8..18].copy_from_slice(&[1, 0, 0, 0, 1, 3, 2, 0x4e, 44, 0]);
        old[32..37].copy_from_slice(&[0x41, 77, 0x4c, 0, 0]);
        old[116..120].copy_from_slice(&((1u32 << 23) | 32).to_le_bytes());
        new[8..13].copy_from_slice(&[0x41, 9, 0x4c, 0, 0]);
        new[88..92].copy_from_slice(&((2u32 << 23) | 8).to_le_bytes());
        new[92..96].copy_from_slice(&((2u32 << 23) | 112).to_le_bytes());
        let resources = Resources(vec![&master, &old, &new]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        assert_eq!(engine.scene, Handle(1 << 23));
        assert_eq!(
            engine.globals,
            [77],
            "peers run until the scene is actually changed"
        );
        let mut cut = engine.clone();
        cut.pending_scene.as_mut().unwrap().2 = 1;
        assert_eq!(cut.tick(&resources).unwrap(), Status::Running);
        assert_eq!(cut.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(cut.scene, Handle(2 << 23));
        assert!(!cut.fade_on_background);
        let mut restored = engine.clone();
        for multiplier in [0xd000, 0xa000, 0x7000, 0x4000, 0x1000, 0] {
            assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
            assert_eq!(engine.fade_multiplier, multiplier);
            restored.tick(&resources).unwrap();
            assert_eq!(engine, restored);
        }
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.scene, Handle(2 << 23));
        assert_eq!(engine.globals, [9]);
        assert!(engine.processes.is_empty());
    }
    #[test]
    fn preparescene_consumes_hint_without_loading_or_yielding() {
        let (master, mut scene) = fixtures();
        // An absent resource handle must remain a harmless preload hint.
        // The sentinel below it is stored in the same scheduler pass.
        scene[8..20].copy_from_slice(&[0x41, 77, 0x01, 0, 0, 0, 0x7f, 0x4e, 52, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [77]);
        assert_eq!(engine.scene, Handle(1 << 23));
        assert!(engine.pending_scene.is_none());
        assert!(engine.processes.is_empty());
    }
    #[test]
    fn setinvsize_consumes_seven_operands_and_preserves_other_inventory_state() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let mut code = Vec::new();
        for value in [77, 2, 2, 2, 9, 1, 4, 3] {
            code.extend([0x41, value]);
        }
        code.extend([0x4e, 108, 0x4c, 0, 0]);
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(1, 0);
        let window = crate::startup::Window {
            title: Handle(0),
            capacity: 150,
            min: [1, 1],
            size: [3, 2],
            max: [10, 6],
            position: [20, 30],
            first_display: 0,
            contents: vec![198, 314],
        };
        engine.declarations.windows[1] = Some(window.clone());
        engine.declarations.windows[2] = Some(window.clone());
        engine.active_inventory = Some(2);
        engine.inventory_dirty = false;
        let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(engine.globals[0], 77);
        assert_eq!(engine.declarations.windows[1], Some(window));
        let changed = engine.declarations.windows[2].as_ref().unwrap();
        assert_eq!(
            (changed.min, changed.size, changed.max),
            ([2, 2], [4, 1], [4, 3])
        );
        assert_eq!(changed.contents, [198, 314]);
        assert!(!engine.inventory_dirty);
        assert_eq!(engine.active_inventory, Some(2));
    }

    #[test]
    fn block_library_calls_preserve_stack_and_change_live_collision_state() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [0x41, 77, 0x41, 3, 0x4e, 39, 0x4c, 0, 0];
        scene[32..32 + code.len()].copy_from_slice(&code);
        scene[56..61].copy_from_slice(&[0x41, 3, 0x4e, 63, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(1, 0);
        engine.polygons = crate::polygon::Polygons::new(
            vec![crate::polygon::Polygon {
                kind: crate::polygon::Kind::Block,
                corners: [[0, 0], [10, 0], [10, 10], [0, 10]],
                tag_position: [0, 0],
                tag_text: Handle(0),
                node_position: [0, 0],
                film: Handle(0),
                refer_type: 0,
                id: 3,
                scales: [1, 0],
                reel: 0,
                z_factor: 0,
                nodes: vec![],
                lines: vec![],
                script: Handle(0),
            }],
            0,
        );
        let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(engine.globals[0], 77);
        assert_eq!(
            engine.polygons.at([5, 5], crate::polygon::Kind::Block),
            None
        );
        let mut process = ScriptProcess::new(Handle((1 << 23) | 56));
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(
            engine.polygons.at([5, 5], crate::polygon::Kind::Block),
            Some(0)
        );
    }

    #[test]
    fn topwindow_close_clears_overlay_without_blocking_world_input_or_losing_stack() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [0x41, 77, 2, 0x4e, 82, 0x4c, 0, 0];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(1, 0);
        engine.top_window = Some(crate::conversation_window::Frame {
            layout: crate::conversation_window::Layout {
                scrollbar: None,
                parts: vec![],
                background: [73, 24, 147, 125],
                title: [0, 0],
                icons: vec![],
            },
            parts: vec![],
        });
        assert_eq!(engine.active_inventory(), None);
        let enabled = engine.control.enabled;
        let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Halted
        );
        assert!(engine.top_window.is_none());
        assert_eq!(engine.control.enabled, enabled);
        assert_eq!(engine.globals[0], 77);
    }

    #[test]
    fn standtag_uses_polygon_node_and_lead_only_pose_preserving_stack() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let base = 2u32 << 23;
        let mut film = vec![0; 64];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, base + 24),
            (12, base + 48),
            (32, 1),
            (48, 10),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for actor in [-2i8, 1, 2] {
            let code = [0x41, 77, 0x41, actor as u8, 0x4e, 73, 0x4c, 0, 0];
            scene[32..32 + code.len()].copy_from_slice(&code);
            let resources = Resources(vec![&master, &scene, &film]);
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.globals.resize(1, 0);
            engine.actors = crate::actor::Actors::new(2).unwrap();
            engine.declarations.lead = Some(crate::startup::Lead {
                id: 1,
                text: Handle(0),
                tagged: false,
            });
            for id in [1, 2] {
                engine
                    .declarations
                    .movers
                    .insert(id, [[Handle(base); 12]; 10]);
            }
            engine.polygons = crate::polygon::Polygons::new(
                vec![crate::polygon::Polygon {
                    kind: crate::polygon::Kind::Tag,
                    corners: [[0, 0], [100, 0], [100, 100], [0, 100]],
                    tag_position: [5, 6],
                    tag_text: Handle(0),
                    node_position: [50, 70],
                    film: Handle(3),
                    refer_type: 0,
                    id: 1,
                    scales: [1, 0],
                    reel: 0,
                    z_factor: 0,
                    nodes: vec![],
                    lines: vec![],
                    script: Handle(0),
                }],
                0,
            );
            for _ in 0..2 {
                let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
                process.polygon = Some(0);
                assert_eq!(
                    engine
                        .run_process(&resources, None, None, None, &mut process)
                        .unwrap(),
                    Status::Halted
                );
                let id = if actor == -2 { 1 } else { i32::from(actor) };
                let mover = &engine.movers[&id];
                assert_eq!(mover.placement.position, [50, 70]);
                assert_eq!(engine.actors.get(id as u32).unwrap().position, [50, 70]);
                assert_eq!(
                    mover.placement.direction,
                    if id == 1 {
                        crate::mover::Direction::Left
                    } else {
                        crate::mover::Direction::Forward
                    }
                );
                assert_eq!(engine.globals[0], 77);
            }
            let mut invalid = ScriptProcess::new(Handle((1 << 23) | 32));
            assert!(
                engine
                    .run_process(&resources, None, None, None, &mut invalid)
                    .is_err()
            );
        }
    }

    #[test]
    fn actor_position_queries_use_live_movers_and_preserve_lower_stack() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [
            0x41, 77, 0x41, 0xfe, 0x4e, 4, 0x4c, 0, 3, 0x4e, 5, 0x4c, 1, 0x4c, 2, 0,
        ];
        scene[32..32 + code.len()].copy_from_slice(&code);
        scene[56..62].copy_from_slice(&[3, 0x4e, 1, 0x4c, 0, 0]);
        scene[16..28].copy_from_slice(&[0x41, 77, 0x41, 0xfe, 0x4e, 3, 0x4c, 0, 0x4c, 1, 0, 0]);
        let base = 2u32 << 23;
        let mut film = vec![0; 64];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, base + 24),
            (12, base + 48),
            (32, 1),
            (48, 10),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        for moving in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.globals.resize(3, 0);
            engine.actors = crate::actor::Actors::new(1).unwrap();
            engine.actors.get_mut(1).unwrap().position = [-10, 90];
            engine.declarations.lead = Some(crate::startup::Lead {
                id: 1,
                text: Handle(0),
                tagged: false,
            });
            engine.world_origin = [300, 20];
            if moving {
                engine
                    .declarations
                    .movers
                    .insert(1, [[Handle(base); 12]; 10]);
                let placement = crate::mover::Placement::initial(
                    &engine.declarations,
                    &engine.polygons,
                    1,
                    [50, 70],
                    Handle(base),
                )
                .unwrap();
                engine
                    .movers
                    .insert(1, crate::mover::Mover::new(&resources, placement).unwrap());
            }
            let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
            assert_eq!(
                engine
                    .run_process(&resources, None, None, None, &mut process)
                    .unwrap(),
                Status::Halted
            );
            assert_eq!(
                engine.globals,
                if moving {
                    vec![50, 70, 77]
                } else {
                    vec![-10, 90, 77]
                }
            );
            let mut scale_query = ScriptProcess::new(Handle((1 << 23) | 16));
            if !moving {
                assert_eq!(
                    engine.run_process(&resources, None, None, None, &mut scale_query),
                    Err(Error("ACTORSCALE actor has no mover"))
                );
            }
            if moving {
                for scale in 1..=10 {
                    engine.movers.get_mut(&1).unwrap().placement.scale = scale;
                    let before = engine.movers.clone();
                    let mut query = ScriptProcess::new(Handle((1 << 23) | 16));
                    assert_eq!(
                        engine
                            .run_process(&resources, None, None, None, &mut query)
                            .unwrap(),
                        Status::Halted
                    );
                    assert_eq!(&engine.globals[..2], &[scale as i32, 77]);
                    assert_eq!(engine.movers, before);
                }
                for (direction, expected) in [
                    (crate::mover::Direction::Left, 0),
                    (crate::mover::Direction::Right, 1),
                    (crate::mover::Direction::Forward, 2),
                    (crate::mover::Direction::Away, 3),
                ] {
                    engine.movers.get_mut(&1).unwrap().placement.direction = direction;
                    let mut query = ScriptProcess::new(Handle((1 << 23) | 56));
                    engine
                        .run_process(&resources, None, None, None, &mut query)
                        .unwrap();
                    assert_eq!(engine.globals[0], expected);
                }
            }
        }
    }

    #[test]
    fn short_swalk_completes_after_node_snap_and_faces_world_cursor() {
        for node_path in [false, true] {
            let (master, mut scene) = fixtures();
            scene[8] = 0;
            let start = if node_path { [55, 74] } else { [50, 70] };
            let code = [
                0x41, 77, 3, 0x41, start[0], 0x41, start[1], 0x41, 50, 0x41, 70, 2, 0x4e, 75, 0x4c,
                0, 0,
            ];
            scene[32..32 + code.len()].copy_from_slice(&code);
            let base = 2u32 << 23;
            let mut film = vec![0; 64];
            for (at, value) in [
                (0, 24u32),
                (4, 1),
                (8, base + 24),
                (12, base + 48),
                (32, 1),
                (48, 10),
            ] {
                film[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            let resources = Resources(vec![&master, &scene, &film]);
            for enabled in [false, true] {
                let mut engine = Engine::bootstrap(&resources).unwrap();
                engine.actors = crate::actor::Actors::new(1).unwrap();
                engine.declarations.lead = Some(crate::startup::Lead {
                    id: 1,
                    text: Handle(0),
                    tagged: false,
                });
                engine
                    .declarations
                    .movers
                    .insert(1, [[Handle(base); 12]; 10]);
                engine.polygons = crate::polygon::Polygons::new(
                    vec![crate::polygon::Polygon {
                        kind: if node_path {
                            crate::polygon::Kind::NodePath
                        } else {
                            crate::polygon::Kind::Path
                        },
                        corners: [[0, 0], [100, 0], [100, 100], [0, 100]],
                        tag_position: [0; 2],
                        tag_text: Handle(0),
                        node_position: [0; 2],
                        film: Handle(0),
                        refer_type: 0,
                        id: 1,
                        scales: [1, 0],
                        reel: 0,
                        z_factor: 0,
                        nodes: if node_path {
                            vec![[50, 70], [80, 70]]
                        } else {
                            vec![]
                        },
                        lines: vec![],
                        script: Handle(0),
                    }],
                    0,
                );
                engine.world_origin = [100, 0];
                engine.control.cursor = [0, 70];
                engine.control.enabled = enabled;
                let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
                assert_eq!(
                    engine
                        .run_process(&resources, None, None, None, &mut process)
                        .unwrap(),
                    Status::Halted
                );
                assert!(process.walk_wait.is_none());
                assert_eq!(
                    engine.globals[0], 77,
                    "SWALK consumes only its six arguments"
                );
                assert_eq!(engine.control.enabled, enabled);
                let mover = &engine.movers[&1];
                assert_eq!(mover.placement.position, [50, 70]);
                assert_eq!(engine.actors.get(1).unwrap().position, [50, 70]);
                assert_eq!(mover.placement.direction, crate::mover::Direction::Right);
                assert_eq!(mover.walk_target, None);
                assert_eq!(mover.walk_serial, 1);
            }
        }
    }

    #[test]
    fn walked_delays_and_returns_exact_arrival_or_interruption_without_losing_stack() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [
            0x41, 77, 3, 0x41, 50, 0x41, 70, 2, 0x4e, 88, 0x4c, 0, 0x4c, 1, 0,
        ];
        scene[32..32 + code.len()].copy_from_slice(&code);
        scene[56..64].copy_from_slice(&[0x41, 88, 3, 0x4e, 74, 0x4c, 0, 0]);
        let base = 2u32 << 23;
        let mut film = vec![0; 64];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, base + 24),
            (12, base + 48),
            (32, 1),
            (48, 10),
            (52, 1),
            (56, (-2i32) as u32),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(2, 0);
        engine.actors = crate::actor::Actors::new(1).unwrap();
        engine
            .declarations
            .movers
            .insert(1, [[Handle(base); 12]; 10]);
        let placement = crate::mover::Placement::initial(
            &engine.declarations,
            &engine.polygons,
            1,
            [50, 70],
            Handle(base),
        )
        .unwrap();
        engine
            .movers
            .insert(1, crate::mover::Mover::new(&resources, placement).unwrap());
        let id = engine
            .processes
            .spawn(
                Some(engine.generation),
                ScriptProcess::new(Handle((1 << 23) | 32)),
            )
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(
            engine
                .processes
                .state_mut(id)
                .unwrap()
                .walked_wait
                .as_ref()
                .unwrap()
                .delay,
            24
        );
        for _ in 0..23 {
            engine.tick(&resources).unwrap();
        }
        assert_eq!(engine.globals, [0, 0]);
        assert_eq!(engine.movers[&1].walk_serial, 0);
        assert_eq!(
            engine
                .processes
                .state_mut(id)
                .unwrap()
                .walked_wait
                .as_ref()
                .unwrap()
                .delay,
            1
        );
        let mut stopped = engine.clone();
        let wait = stopped
            .processes
            .state_mut(id)
            .unwrap()
            .walked_wait
            .as_mut()
            .unwrap();
        wait.delay = 0;
        wait.serial = Some(5);
        let mover = stopped.movers.get_mut(&1).unwrap();
        mover.walk_serial = 5;
        mover.walk_target = Some([80, 70]);
        let mut owner = ScriptProcess::new(Handle((1 << 23) | 32));
        owner.walk_wait = Some((
            LibraryCall {
                id: 87,
                instruction: 0,
                next: 2,
            },
            1,
            5,
            false,
        ));
        let owner_id = stopped
            .processes
            .spawn(Some(stopped.generation), owner)
            .unwrap();
        let mut stop = ScriptProcess::new(Handle((1 << 23) | 56));
        assert_eq!(
            stopped
                .run_process(&resources, None, None, None, &mut stop)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(stopped.globals[0], 88);
        assert!(stopped.processes.state_mut(owner_id).is_none());
        assert!(stopped.processes.state_mut(id).is_some());
        assert_eq!(stopped.movers[&1].walk_target, Some([80, 70]));
        assert_eq!(stopped.movers[&1].walk_serial, 5);
        let mover = stopped.movers.get_mut(&1).unwrap();
        mover
            .advance_direct_walk(&resources, &stopped.declarations, &stopped.polygons, &[])
            .unwrap();
        assert_eq!(mover.placement.position, [50, 70]);
        assert_eq!(mover.walk_target, None);
        assert_eq!(mover.walk_serial, 6);
        assert!(!mover.stop_requested);
        stopped.tick(&resources).unwrap();
        assert_eq!(stopped.globals, [0, 77]);
        for (serial, position, expected) in [(5, [50, 70], 1), (6, [50, 70], 0), (5, [49, 70], 0)] {
            let mut completed = engine.clone();
            let wait = completed
                .processes
                .state_mut(id)
                .unwrap()
                .walked_wait
                .as_mut()
                .unwrap();
            wait.delay = 0;
            wait.serial = Some(5);
            let mover = completed.movers.get_mut(&1).unwrap();
            mover.walk_serial = serial;
            mover.placement.position = position;
            completed.tick(&resources).unwrap();
            assert_eq!(completed.globals, [expected, 77]);
        }
    }

    #[test]
    fn new_film_waitframe_does_not_use_previous_film_steps() {
        for opcode in [49, 81] {
            let (master, mut scene) = fixtures();
            scene[8] = 0;
            let code = [
                1, 0, 0, 0, 1, 4, 4, 4, 2, 2, 0x4e, opcode, 3, 0x41, 2, 0x4e, 100, 0x41, 77, 0x4c,
                0, 0,
            ];
            scene[32..32 + code.len()].copy_from_slice(&code);
            let base = 2u32 << 23;
            let mut film = vec![0; 64];
            for (at, value) in [
                (0, 24u32),
                (4, 1),
                (8, base + 24),
                (12, base + 48),
                (32, 1),
                (48, 10),
                (52, 10),
            ] {
                film[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            let resources = Resources(vec![&master, &scene, &film]);
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.actors = crate::actor::Actors::new(1).unwrap();
            engine.actor_steps.insert(1, 80);
            engine
                .processes
                .spawn(
                    Some(engine.generation),
                    ScriptProcess::new(Handle((1 << 23) | 32)),
                )
                .unwrap();
            engine.tick(&resources).unwrap();
            assert_eq!(engine.actor_steps[&1], 0);
            assert_eq!(engine.globals, [0]);
            engine.tick(&resources).unwrap();
            assert_eq!(engine.actor_steps[&1], 1);
            assert_eq!(engine.globals, [0]);
            engine.tick(&resources).unwrap();
            assert_eq!(engine.actor_steps[&1], 2);
            assert_eq!(engine.globals, [77]);
            assert!(
                engine
                    .films
                    .iter()
                    .all(|a| a.status_layer == (opcode == 81))
            );
        }
    }

    #[test]
    fn scripted_scene_save_cutaway_and_restore_resume_the_active_caller() {
        for restore_call in [57, 121] {
            let (master, mut scene) = fixtures();
            scene[8] = 0;
            let code = [
                0x41, 77, 0x4e, 58, 1, 0, 0, 0, 1, 3, 3, 0x4e, 44, 0x4c, 0, 0,
            ];
            scene[32..32 + code.len()].copy_from_slice(&code);
            let mut cutaway = scene.clone();
            cutaway[8..11].copy_from_slice(&[0x4e, restore_call, 0]);
            cutaway[88..92].copy_from_slice(&((2u32 << 23) | 8).to_le_bytes());
            let resources = Resources(vec![&master, &scene, &cutaway]);
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine
                .processes
                .spawn(
                    Some(engine.generation),
                    ScriptProcess::new(Handle((1 << 23) | 32)),
                )
                .unwrap();
            engine.tick(&resources).unwrap();
            assert_eq!(engine.saved_scenes.len(), 1);
            assert_eq!(
                engine.saved_scenes[0].scripts.contexts[0]
                    .vm
                    .pending_call()
                    .unwrap()
                    .id,
                58
            );
            let mut saw_cutaway = false;
            let mut restore_ticks = 0;
            for _ in 0..30 {
                if engine.scene_restore.is_some() {
                    restore_ticks += 1;
                }
                engine.tick(&resources).unwrap();
                saw_cutaway |= engine.scene == Handle(2 << 23);
                if engine.globals[0] == 77 {
                    break;
                }
            }
            assert!(saw_cutaway);
            assert_eq!(restore_ticks, if restore_call == 57 { 11 } else { 5 });
            assert_eq!(engine.scene, Handle(1 << 23));
            assert_eq!(engine.globals, [77]);
            assert!(engine.saved_scenes.is_empty());
            assert!(engine.scene_restore.is_none());
        }
    }

    #[test]
    fn scene_save_stack_duplicate_limit_and_missing_restore_are_explicit() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert!(engine.request_scene_restore(false).is_err());
        assert!(engine.save_scene_call(None).unwrap());
        assert!(!engine.save_scene_call(None).unwrap());
        for id in 2..=4 {
            engine.scene = Handle(id << 23);
            assert!(engine.save_scene_call(None).unwrap());
        }
        assert!(engine.save_scene_call(None).is_err());
        engine.request_scene_restore(false).unwrap();
        assert_eq!(
            engine
                .scene_restore
                .as_ref()
                .unwrap()
                .snapshot
                .scripts
                .scene,
            Handle(4 << 23)
        );
        engine.request_scene_restore(true).unwrap();
        assert_eq!(engine.saved_scenes.len(), 3);
        assert_eq!(engine.scene_restore.as_ref().unwrap().remaining, 5);
    }

    #[test]
    fn scene_snapshot_rebuilds_objects_without_rewinding_global_state() {
        let (master, scene) = fixtures();
        let base = 2u32 << 23;
        let mut film = vec![0; 80];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, base + 24),
            (12, base + 48),
            (32, 1),
            (48, 10),
            (52, 1),
            (56, (-2i32) as u32),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(1).unwrap();
        engine.actors.get_mut(1).unwrap().position = [20, 30];
        engine.start_background(&resources, Handle(base)).unwrap();
        engine.launch_film(&resources, Handle(base)).unwrap();
        engine.control.apply(1, false).unwrap();
        engine.world_origin = [40, 10];
        engine.no_blocking = true;
        engine.scroll.boundaries.add([20, 0], [20, 100]).unwrap();
        engine.tick(&resources).unwrap();
        let saved = engine.capture_scene().unwrap();
        assert_eq!(saved.scripts.contexts.len(), 1);
        engine.disable_actor(1).unwrap();
        engine.globals[0] = 99;
        engine.held_object = 42;
        engine.ticks = 100;
        engine.music.clock = 500;
        let before = engine.clone();
        let mut invalid = saved.clone();
        invalid.background = Some(Handle(base + 1000));
        assert!(engine.rebuild_scene(&resources, &[], &invalid).is_err());
        assert_eq!(engine, before);
        engine.rebuild_scene(&resources, &[], &saved).unwrap();
        assert_eq!(engine.master, before.master);
        assert_eq!(engine.globals, [99]);
        assert_eq!(engine.held_object, 42);
        assert_eq!(engine.ticks, 100);
        assert_eq!(engine.music.clock, 500);
        assert_eq!(engine.world_origin, saved.origin);
        assert_eq!(engine.scroll.boundaries, saved.boundaries);
        assert!(engine.no_blocking && engine.control.enabled);
        assert!(engine.actors.get(1).unwrap().alive);
        assert_eq!(engine.films.len(), 1);
        assert_eq!(engine.films[0].steps, 0);
        assert_eq!(
            [engine.films[0].object.x, engine.films[0].object.y],
            [20, 30]
        );
        assert_eq!(engine.background.as_ref().unwrap().steps, 0);
        assert_eq!(engine.processes.states().count(), 1);
        assert!(
            engine
                .processes
                .states()
                .all(|p| p.restored_context && p.wait.is_none())
        );
        engine.tick(&resources).unwrap();
        assert!(engine.processes.states().all(|p| p.wait.is_some()));
    }

    #[test]
    fn scene_movers_rebuild_standing_state_and_restore_inactive_banks_atomically() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let base = 2u32 << 23;
        let mut film = vec![0; 64];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, base + 24),
            (12, base + 48),
            (32, 1),
            (48, 10),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(2).unwrap();
        for actor in [1, 2] {
            engine
                .declarations
                .movers
                .insert(actor, [[Handle(base); 12]; 10]);
        }
        let placement = crate::mover::Placement::initial(
            &engine.declarations,
            &engine.polygons,
            1,
            [50, 70],
            Handle(base),
        )
        .unwrap();
        let mut mover = crate::mover::Mover::new(&resources, placement).unwrap();
        assert_eq!(mover.last_film, Handle(base));
        mover.walk_target = Some([100, 100]);
        mover.step_phase = 4;
        engine.movers.insert(1, mover);
        let saved = engine.capture_scene_movers();
        engine.declarations.movers.get_mut(&2).unwrap()[0][0] = Handle(123);
        engine.globals[0] = 99;
        let before = engine.clone();
        let mut invalid = saved.clone();
        invalid.active[0].2 = Handle(base + 1000);
        assert!(engine.restore_scene_movers(&resources, &invalid).is_err());
        assert_eq!(engine, before);
        engine.restore_scene_movers(&resources, &saved).unwrap();
        assert_eq!(engine.globals, [99]);
        assert_eq!(engine.declarations.movers, saved.banks);
        assert_eq!(engine.movers.len(), 1);
        let mover = &engine.movers[&1];
        assert_eq!(mover.placement.position, [50, 70]);
        assert_eq!(mover.last_film, Handle(base));
        assert!(mover.walk_target.is_none());
        assert_eq!(mover.step_phase, 0);
        assert_eq!(engine.actors.get(1).unwrap().position, [50, 70]);
        assert_eq!(engine.capture_scene_movers(), saved);
    }

    #[test]
    fn scene_script_rebuild_restarts_waits_preserves_order_and_global_progress() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [0x41, 2, 3, 0x4e, 86, 2, 0x4e, 31, 0x4c, 0, 0];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        for event in [2, 3] {
            let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
            process.event = event;
            engine
                .processes
                .spawn(Some(engine.generation), process)
                .unwrap();
        }
        engine.tick(&resources).unwrap();
        engine.tick(&resources).unwrap();
        // Queued input without an allocated/running interpreter is not saved.
        engine
            .processes
            .spawn(
                Some(engine.generation),
                ScriptProcess::new(Handle((1 << 23) | 32)),
            )
            .unwrap();
        let saved = engine.capture_scene_scripts();
        assert_eq!(saved.contexts.len(), 2);
        assert_eq!(
            saved.contexts.iter().map(|c| c.event).collect::<Vec<_>>(),
            [3, 2]
        );
        engine.globals[0] = 99;
        engine.ticks = 100;
        engine.held_object = 42;
        let master = engine.master.clone();
        let declarations = engine.declarations.clone();
        engine.restore_scene_scripts(&resources, &saved).unwrap();
        assert_eq!(engine.master, master);
        assert_eq!(engine.declarations, declarations);
        assert_eq!(engine.held_object, 42);
        assert_eq!(engine.ticks, 100);
        assert_eq!(engine.globals, [99]);
        assert!(engine.processes.states().all(|p| p.wait.is_none()));
        let before = engine.clone();
        let mut invalid = saved.clone();
        invalid.scene = Handle(2 << 23);
        assert!(engine.restore_scene_scripts(&resources, &invalid).is_err());
        assert_eq!(engine, before);
        invalid = saved.clone();
        invalid.contexts[0].inventory_object = Some(999);
        assert!(engine.restore_scene_scripts(&resources, &invalid).is_err());
        assert_eq!(engine, before);
        for _ in 0..3 {
            engine.tick(&resources).unwrap();
            assert_eq!(engine.globals, [99]);
        }
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [2]);
        assert!(engine.processes.is_empty());
    }

    #[test]
    fn restore_actor_column_restarts_only_selected_reel_and_validates_atomically() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let base = 2u32 << 23;
        let mut film = vec![0; 112];
        for (at, value) in [
            (0, 24u32),
            (4, 2),
            (8, base + 1000),
            (12, base + 80),
            (16, base + 56),
            (20, base + 80),
            (64, 2),
            (68, 30),
            (72, 40),
            (80, 5),
            (84, 9),
            (88, 10),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(2).unwrap();
        let source = crate::animation::ReelSource {
            film: Handle(base),
            column: 1,
            position: [100, 110],
            depth: 4321,
        };
        // Column zero is deliberately unreadable; restoring column one must not
        // initialize it or infer depth from its geometry.
        assert!(engine.restore_actor_reel(&resources, 2, &source).unwrap());
        assert_eq!(engine.films.len(), 1);
        assert_eq!(engine.films[0].steps, 0);
        assert_eq!(
            [engine.films[0].object.x, engine.films[0].object.y],
            [30, 40]
        );
        assert_eq!(engine.films[0].object.z, 4321);
        engine.tick(&resources).unwrap();
        assert_eq!(engine.films[0].object.x, 39);
        let mut before = engine.clone();
        assert!(!engine.restore_actor_reel(&resources, 2, &source).unwrap());
        // Scheduling resets the actor's reported steps even if its duplicate
        // reel keeps running from the current animation frame.
        before.actor_steps.insert(2, 0);
        assert_eq!(engine, before);
        for (actor, column) in [(1, 1), (2, 2), (2, 0)] {
            let invalid = crate::animation::ReelSource {
                column,
                ..source.clone()
            };
            assert!(
                engine
                    .restore_actor_reel(&resources, actor, &invalid)
                    .is_err()
            );
            assert_eq!(engine, before);
        }
        engine.disable_actor(2).unwrap();
        assert!(!engine.restore_actor_reel(&resources, 2, &source).unwrap());
        engine.actors.enable(2).unwrap();
        assert!(engine.restore_actor_reel(&resources, 2, &source).unwrap());
        assert_eq!(engine.films[0].steps, 0);
        assert_eq!(engine.films[0].object.x, 30);
        assert_eq!(engine.actor_reel_sources(), vec![(2, source)]);
    }

    #[test]
    fn actor_film_sources_preserve_play_arguments_and_ignore_stale_reels() {
        let (master, scene) = fixtures();
        let base = 2u32 << 23;
        let mut film = vec![0; 112];
        for (at, value) in [
            (0, 24u32),
            (4, 2),
            (8, base + 32),
            (12, base + 80),
            (16, base + 56),
            (20, base + 80),
            (40, 1),
            (64, 2),
            (68, 30),
            (72, 40),
            (80, 5),
            (84, 9),
            (88, 10),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(2).unwrap();
        engine.actors.get_mut(1).unwrap().position = [70, 80];
        engine.actors.get_mut(1).unwrap().masking = 2;
        engine.actors.get_mut(2).unwrap().masking = 99;
        engine.world_origin = [300, 100];
        let top = engine
            .prepare_top_film(&resources, Handle(base), [80, 50])
            .unwrap();
        assert!(top.iter().all(|a| a.status_layer));
        assert_eq!([top[0].object.z, top[1].object.z], [102, 103]);
        assert_eq!([top[0].object.x, top[0].object.y], [80, 50]);
        assert_eq!([top[1].object.x, top[1].object.y], [30, 40]);
        engine.actors.get_mut(1).unwrap().masking = 0;
        engine.actors.get_mut(2).unwrap().masking = 0;
        let defaults = engine
            .prepare_actor_film(&resources, Handle(base), [-1, -1])
            .unwrap();
        assert_eq!([defaults[0].object.x, defaults[0].object.y], [70, 80]);
        assert_eq!(defaults[0].source.as_ref().unwrap().position, [70, 80]);
        assert_eq!([defaults[1].object.x, defaults[1].object.y], [30, 40]);
        assert_eq!(defaults[1].source.as_ref().unwrap().position, [-1, -1]);
        let mut reels = engine
            .prepare_actor_film(&resources, Handle(base), [100, 110])
            .unwrap();
        assert_eq!([reels[0].object.x, reels[0].object.y], [100, 110]);
        assert_eq!([reels[1].object.x, reels[1].object.y], [30, 40]);
        for (column, reel) in reels.iter_mut().enumerate() {
            let source = reel.source.clone().unwrap();
            assert_eq!(source.film, Handle(base));
            assert_eq!(source.column, column);
            assert_eq!(source.position, [100, 110]);
            assert_eq!(source.depth, 2 + column as i32);
            assert!(engine.claim_reel(reel).unwrap());
            reel.tick(&resources).unwrap();
            assert_eq!(reel.source.as_ref(), Some(&source));
        }
        let mut primary = reels.remove(0);
        let mut process = ScriptProcess::new(Handle(0));
        process.play_wait = Some((
            LibraryCall {
                id: 49,
                instruction: 0,
                next: 2,
            },
            primary.clone(),
        ));
        engine
            .processes
            .spawn(Some(engine.generation), process)
            .unwrap();
        engine.films = reels;
        assert_eq!(
            engine
                .actor_reel_sources()
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        // A replacement invalidates the old primary still waiting in its caller.
        primary.reel = Some(Handle(base + 100));
        assert!(engine.claim_reel(&mut primary).unwrap());
        engine.films.push(primary);
        assert_eq!(engine.actor_reel_sources().len(), 2);
        engine.disable_actor(1).unwrap();
        assert_eq!(
            engine
                .actor_reel_sources()
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [2]
        );
    }

    #[test]
    fn film_masking_depth_and_secondary_columns() {
        let (master, scene) = fixtures();
        let base = 2u32 << 23;
        let mut sprite = vec![0; 80];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, base + 16),
            (12, base + 64),
            (16, base + 40),
            (24, 1),
            (40, base + 48),
            (48, 8 | (10 << 16)),
            (52, 2 | ((-3i16 as u16 as u32) << 16)),
            (64, base + 40),
            (68, 1),
            (72, (-2i32) as u32),
        ] {
            sprite[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &sprite]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(2).unwrap();
        let original = Engine::start_film(&resources, Handle(base), false).unwrap();
        for (mask, expected) in [(0, 2), (-1, 12), (-2, 10000), (3, 2060)] {
            engine.actors.get_mut(1).unwrap().masking = mask;
            let mut reels = original.clone();
            engine.set_film_depths(&resources, &mut reels).unwrap();
            assert_eq!(reels[0].object.z, expected);
        }
        let mut reels = vec![original[0].clone(), original[0].clone()];
        reels[1].object.id = 2;
        engine.actors.get_mut(2).unwrap().masking = 4;
        engine.set_film_depths(&resources, &mut reels).unwrap();
        assert_eq!([reels[0].object.z, reels[1].object.z], [2060, 4097]);
    }

    #[test]
    fn holding_item_builds_cursor_and_returns_replaced_item_to_luggage() {
        let (master, scene) = fixtures();
        let base = 2u32 << 23;
        let mut sprite = vec![0; 80];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, base + 16),
            (12, base + 64),
            (16, base + 40),
            (40, base + 48),
            (48, 8 | (10 << 16)),
            (52, 2 | ((-3i16 as u16 as u32) << 16)),
            (64, base + 40),
            (68, 1),
            (72, (-2i32) as u32),
        ] {
            sprite[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &sprite]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.declarations.windows[1] = Some(crate::startup::Window {
            title: Handle(0),
            capacity: 150,
            min: [1; 2],
            size: [1; 2],
            max: [10; 2],
            position: [0; 2],
            first_display: 0,
            contents: vec![],
        });
        for id in [284, 285] {
            engine.inventory_objects.insert(
                id,
                crate::inventory::Object {
                    id,
                    film: Handle(base),
                    script: Handle(0),
                    attributes: 0,
                },
            );
        }
        engine.hold_object(&resources, 284).unwrap();
        assert_eq!(engine.held_object, 284);
        assert_eq!(engine.held_cursor.as_ref().unwrap().offset, [2, 8]);
        assert_eq!(engine.held_cursor.as_ref().unwrap().animation.steps, 0);
        let before = engine.clone();
        engine.hold_object(&resources, 284).unwrap();
        assert_eq!(
            engine, before,
            "holding the same item does not restart its animation"
        );
        assert!(engine.hold_object(&resources, 999).is_err());
        assert_eq!(engine, before);
        engine.hold_object(&resources, 285).unwrap();
        assert_eq!(
            engine.declarations.windows[1].as_ref().unwrap().contents,
            [284]
        );
        engine.hold_object(&resources, -1).unwrap();
        assert!(engine.held_cursor.is_none());
        assert_eq!(
            engine.declarations.windows[1].as_ref().unwrap().contents,
            [284, 285]
        );
        engine.hold_object(&resources, 284).unwrap();
        engine.hold_object(&resources, -1).unwrap();
        assert_eq!(
            engine.declarations.windows[1].as_ref().unwrap().contents,
            [284, 285]
        );
    }
    #[test]
    fn master_wait_keeps_empty_scene_scheduler_running() {
        let (mut master, mut scene) = fixtures();
        master.resize(40, 0);
        master[29..34].copy_from_slice(&[3, 3, 0x4e, 86, 0]);
        scene[8] = 0;
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        assert!(engine.processes.is_empty());
        // WAITTIME sleeps first, then performs its post-decrement wait.
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
    }
    #[test]
    fn scene_call_trace_records_dispatch_and_rolls_back_failed_ticks() {
        let (master, mut scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.scene_calls = Some(std::collections::VecDeque::new());
        engine.tick(&resources).unwrap();
        let trace = engine.scene_calls.as_ref().unwrap();
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].tick, 1);
        assert_eq!(trace[0].script, Handle((1 << 23) | 8));
        assert_eq!(trace[0].call.id, 86);
        assert_eq!(trace[0].operands, [1, 1]);
        let saved = trace.clone();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.scene_calls.as_ref().unwrap(), &saved);
        scene[8] = 4; // Negative WAITTIME fails after dispatch.
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.scene_calls = Some(std::collections::VecDeque::new());
        let before = engine.clone();
        assert!(engine.tick(&resources).is_err());
        assert_eq!(engine, before);
    }
    #[test]
    fn scene_auxscale_updates_only_selected_reel_bank_and_preserves_stack() {
        let (master, mut scene) = fixtures();
        let mut code = vec![0x41, 9];
        for value in [-2i8, 6, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22] {
            code.extend_from_slice(&[0x41, value as u8]);
        }
        code.extend_from_slice(&[0x4e, 10, 0x4c, 0, 0]);
        scene[8..8 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.declarations.lead = Some(crate::startup::Lead {
            id: 7,
            text: Handle(0),
            tagged: false,
        });
        engine.declarations.movers.insert(7, [[Handle(99); 12]; 10]);
        let mut restored = engine.clone();
        engine.tick(&resources).unwrap();
        restored.tick(&resources).unwrap();
        assert_eq!(engine, restored);
        assert_eq!(engine.globals, [9]);
        let reels = &engine.declarations.movers[&7];
        assert_eq!(reels[5], std::array::from_fn(|i| Handle(11 + i as u32)));
        assert_eq!(reels[4], [Handle(99); 12]);
        assert_eq!(reels[6], [Handle(99); 12]);
        engine.enter(&resources, Handle(1 << 23), 1, 1).unwrap();
        assert_eq!(engine.declarations.movers[&7][5], reels_for_test());
        fn reels_for_test() -> [Handle; 12] {
            std::array::from_fn(|i| Handle(11 + i as u32))
        }
    }
    #[test]
    fn noblocking_preserves_stack_and_resets_at_scene_entry() {
        let (master, mut scene) = fixtures();
        scene[8..15].copy_from_slice(&[0x41, 7, 0x4e, 110, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert!(!engine.no_blocking);
        engine.tick(&resources).unwrap();
        assert!(engine.no_blocking);
        assert_eq!(engine.globals, [7]);
        let mut restored = engine.clone();
        engine.enter(&resources, Handle(1 << 23), 1, 1).unwrap();
        restored.enter(&resources, Handle(1 << 23), 1, 1).unwrap();
        assert!(!engine.no_blocking);
        assert_eq!(engine, restored);
    }
    #[test]
    fn heldobject_query_preserves_stack_and_scene_persistent_identity() {
        let (master, mut scene) = fixtures();
        // HELDOBJECT replaces its return-slot operand, leaving 9 below it.
        scene[8..17].copy_from_slice(&[0x41, 9, 2, 0x4e, 33, 26, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert_eq!(engine.held_object, -1);
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [8]);
        engine.held_object = 42;
        engine.enter(&resources, Handle(1 << 23), 1, 1).unwrap();
        let mut restored = engine.clone();
        assert_eq!(engine.held_object, 42);
        engine.tick(&resources).unwrap();
        restored.tick(&resources).unwrap();
        assert_eq!(engine.globals, [51]);
        assert_eq!(engine, restored);
    }
    #[test]
    fn startup_event_uses_v1_script_number() {
        let (master, mut scene) = fixtures();
        scene[8..14].copy_from_slice(&[2, 0x4e, 31, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.globals, [6]);
    }
    #[test]
    fn waitkey_yields_until_new_input_or_cursor_movement() {
        let (master, mut scene) = fixtures();
        scene[8..11].copy_from_slice(&[0x4e, 85, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.user_event().unwrap();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        let mut moved = engine.clone();
        moved.move_cursor([4, 5]);
        assert_eq!(moved.tick(&resources).unwrap(), Status::Halted);
        engine.user_event().unwrap();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
    }
    #[test]
    fn blocking_midi_resumes_after_sequence_end() {
        let (master, mut scene) = fixtures();
        scene[8..15].copy_from_slice(&[0x41, 4, 2, 3, 0x4e, 50, 0]);
        let resources = Resources(vec![&master, &scene]);
        let tracks = [crate::music::Track {
            offset: 4,
            events: &[0x90, 60, 100, 10, 12, 0xff, 0x2f, 0],
        }];
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert!(matches!(
            engine.tick(&resources).unwrap(),
            Status::Unsupported(_)
        ));
        assert_eq!(
            engine.tick_media(&resources, None, Some(&tracks)).unwrap(),
            Status::Running
        );
        let mut restored = engine.clone();
        for e in [&mut engine, &mut restored] {
            assert_eq!(
                e.tick_media(&resources, None, Some(&tracks)).unwrap(),
                Status::Running
            );
            assert_eq!(e.music_frame[0].status, 0x90);
            assert_eq!(
                e.tick_media(&resources, None, Some(&tracks)).unwrap(),
                Status::Running
            );
            assert_eq!(
                e.tick_media(&resources, None, Some(&tracks)).unwrap(),
                Status::Halted
            );
        }
        assert_eq!(engine, restored);
    }
    #[test]
    fn restored_save_call_skips_cutaway_after_wait_in_both_dispatchers() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [
            0x41, 77, 0x4e, 58, // saved with sentinel on the stack
            2, 3, 0x4e, 86, // wait zero ticks; resume state must survive
            1, 0, 0, 0, 1, 3, 3, 0x4e, 44, // unavailable cutaway scene 2
            0x4c, 0, 0,
        ];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut saved = Machine::default();
        saved.step(&code, &mut []).unwrap();
        let Step::Library(call) = saved.step(&code, &mut []).unwrap() else {
            panic!("expected SAVESCENE");
        };
        assert_eq!(call.id, 58);
        assert!(!saved.complete_scene_resume(call).unwrap());
        for master_script in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            let script = Handle((1 << 23) | 32);
            let hook = crate::scene_hook::Request::new(Handle(3 << 23), 2, 1).unwrap();
            engine.scene_hooks.set(hook);
            engine.control.apply(1, false).unwrap();
            if master_script {
                engine.master_code = script;
                engine.master = saved.restored_context();
            } else {
                let mut process = ScriptProcess::new(script);
                process.vm = saved.restored_context();
                engine
                    .processes
                    .spawn(Some(engine.generation), process)
                    .unwrap();
            }
            engine.tick(&resources).unwrap();
            assert_eq!(engine.globals[0], 0);
            let mut cloned = engine.clone();
            for e in [&mut engine, &mut cloned] {
                e.tick(&resources).unwrap();
                assert_eq!(e.globals[0], 77);
                assert_eq!(e.scene, Handle(1 << 23));
                assert!(e.pending_scene.is_none());
                assert!(e.control.enabled);
                let request = crate::scene_hook::Request::new(Handle(2 << 23), 1, 1).unwrap();
                assert_eq!(e.scene_hooks.intercept(request), hook);
            }
            assert_eq!(engine, cloned);
        }
    }

    #[test]
    fn hook_scene_vm_intercepts_transition_and_only_master_continues() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [
            1, 0, 0, 128, 0, 0x41, 7, 2, 0x4e, 95, // Hook scene 1, entrance 7, fade
            1, 0, 0, 0, 1, 3, 3, 0x4e, 44, // Request scene 2, entrance 1, cut
            0x41, 9, 0x4c, 0, 0,
        ];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        for master_script in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            let script = Handle((1 << 23) | 32);
            if master_script {
                engine.master_code = script;
                engine.master = Machine::default();
            } else {
                engine
                    .processes
                    .spawn(Some(engine.generation), ScriptProcess::new(script))
                    .unwrap();
            }
            engine.tick(&resources).unwrap();
            assert_eq!(engine.pending_scene, Some((Handle(1 << 23), 7, 0, None)));
            assert_eq!(engine.globals[0], if master_script { 9 } else { 0 });
            assert!(!engine.control.enabled);
            let delayed = engine.scene_hooks.release().unwrap();
            assert_eq!(
                delayed,
                crate::scene_hook::Request::new(Handle(2 << 23), 1, 1).unwrap()
            );
        }
    }

    #[test]
    fn clear_and_unhook_vm_preserve_delayed_request_and_countdown() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [0x41, 77, 0x4e, 107, 0x4e, 99, 0x4c, 0, 0];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        for master_script in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            let hook = crate::scene_hook::Request::new(Handle(1 << 23), 7, 0).unwrap();
            let target = crate::scene_hook::Request::new(Handle(2 << 23), 3, 2).unwrap();
            engine.scene_hooks.set(hook);
            assert_eq!(engine.scene_hooks.intercept(target), hook);
            engine.scene_hooks.set(hook);
            engine.pending_scene = Some((Handle(1 << 23), 1, 0, Some(5)));
            let script = Handle((1 << 23) | 32);
            if master_script {
                engine.master_code = script;
                engine.master = Machine::default();
            } else {
                engine
                    .processes
                    .spawn(Some(engine.generation), ScriptProcess::new(script))
                    .unwrap();
            }
            engine.tick(&resources).unwrap();
            assert_eq!(engine.globals[0], 77);
            assert_eq!(engine.pending_scene, Some((target.scene, 3, 2, Some(4))));
            assert!(engine.scene_hooks.release().is_err());
            assert_eq!(engine.scene_hooks.intercept(target), target);
        }
    }

    #[test]
    fn reset_idle_time_preserves_stack_and_does_not_generate_input() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        // Keep a sentinel below IDLETIME's return slot across the zero-arg reset.
        let code = [
            0x41, 77, 2, 0x4e, 96, 0x4c, 0, 0x4e, 97, 2, 0x4e, 96, 0x4c, 1, 0x4c, 2, 0,
        ];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        for master_script in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.globals.resize(3, 0);
            engine.control.apply(1, false).unwrap();
            engine.ticks = 240;
            let script = Handle((1 << 23) | 32);
            if master_script {
                engine.master_code = script;
                engine.master = Machine::default();
            } else {
                engine
                    .processes
                    .spawn(Some(engine.generation), ScriptProcess::new(script))
                    .unwrap();
            }
            let events = engine.user_events;
            engine.tick(&resources).unwrap();
            assert_eq!(engine.globals, [10, 0, 77]);
            assert_eq!(engine.user_events, events);
            assert_eq!(engine.control.idle_seconds(engine.ticks + 48).unwrap(), 2);
        }
    }

    #[test]
    fn screen_queries_return_camera_origin_without_consuming_lower_stack_values() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [
            0x41, 77, 2, 0x4e, 42, 0x4c, 0, 2, 0x4e, 80, 0x4c, 1, 0x4c, 2, 0,
        ];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(3, 0);
        engine.world_origin = [345, -12];
        engine
            .processes
            .spawn(
                Some(engine.generation),
                ScriptProcess::new(Handle((1 << 23) | 32)),
            )
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [345, -12, 77]);
    }

    #[test]
    fn effect_entry_stay_exit_and_reentry_preserve_actor_context() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        scene[32..43].copy_from_slice(&[2, 0x4e, 31, 0x4c, 0, 2, 0x4e, 29, 0x4c, 1, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(2, 0);
        engine.polygons = crate::polygon::Polygons::new(
            vec![crate::polygon::Polygon {
                kind: crate::polygon::Kind::Effect,
                corners: [[0, 0], [20, 0], [20, 20], [0, 20]],
                tag_position: [0; 2],
                tag_text: Handle(0),
                node_position: [0; 2],
                film: Handle(0),
                refer_type: 0,
                id: 0,
                scales: [1, 0],
                reel: 0,
                z_factor: 0,
                nodes: vec![],
                lines: vec![],
                script: Handle((1 << 23) | 32),
            }],
            0,
        );
        engine
            .monitor_effects(&resources, &[(7, [10, 10])])
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [4, 7]);
        engine.globals[0] = 99;
        engine
            .monitor_effects(&resources, &[(7, [11, 10])])
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [99, 7]);
        engine
            .monitor_effects(&resources, &[(7, [21, 10])])
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [5, 7]);
        assert!(engine.actor_effects.is_empty());
        engine
            .monitor_effects(&resources, &[(8, [10, 10])])
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [4, 8]);
        engine.enter(&resources, Handle(1 << 23), 1, 1).unwrap();
        assert!(engine.actor_effects.is_empty());
    }

    #[test]
    fn inventory_capacity_and_membership_vm_calls() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        // Set limit(1,200), get limit(1), store global; ININVENTORY(42), store.
        let code = [
            3, 1, 200, 0, 0, 0, 0x4e, 65, 3, 0x4e, 32, 0x4c, 0, 0x41, 42, 0x4e, 35, 0x4c, 1, 0,
        ];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(2, 0);
        engine.declarations.windows[1] = Some(crate::startup::Window {
            title: Handle(0),
            capacity: 1,
            min: [1; 2],
            size: [1; 2],
            max: [10, 6],
            position: [0; 2],
            first_display: 0,
            contents: vec![42],
        });
        engine
            .processes
            .spawn(
                Some(engine.generation),
                ScriptProcess::new(Handle((1 << 23) | 32)),
            )
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [150, 1]);
        let before = engine.declarations.windows.clone();
        assert!(
            crate::inventory::set_limit(engine.declarations.windows[1].as_mut().unwrap(), 0)
                .is_err()
        );
        assert_eq!(engine.declarations.windows, before);
        assert!(crate::inventory::contains(&[None, None, None], 42, 42));
        assert!(!crate::inventory::contains(&[None, None, None], -1, 42));
    }

    #[test]
    fn actor_tag_script_lifecycle() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        // TAGACTOR(1, 42, full body), WAITTIME(1,ticks), UNTAGACTOR(1).
        let code = [
            3, 0x41, 42, 0x41, 2, 0x4e, 76, 3, 3, 0x4e, 86, 3, 0x4e, 83, 0,
        ];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(1).unwrap();
        engine
            .processes
            .spawn(
                Some(engine.generation),
                ScriptProcess::new(Handle((1 << 23) | 32)),
            )
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.actor_tags.get(&1), Some(&(Handle(42), 2)));
        assert_eq!(engine.actor_at(&resources, [0, 0]).unwrap(), None);
        engine.tick(&resources).unwrap();
        assert!(engine.actor_tags.contains_key(&1));
        engine.tick(&resources).unwrap();
        assert!(engine.actor_tags.is_empty());
    }

    #[test]
    fn inventory_description_cancellation_restores_only_owned_control() {
        for initially_enabled in [false, true] {
            let (master, mut scene) = fixtures();
            scene[8] = 0;
            scene[32..40].copy_from_slice(&[0x41, 77, 3, 0x4e, 54, 0x4c, 0, 0]);
            let resources = Resources(vec![&master, &scene]);
            let text = [1, 0, 52, 51, 0, 0, 0, 0, 0, 1, b'W'];
            let strings = crate::text::Strings::new(&text).unwrap();
            let index = [0, 0, 0, 0, 4, 0, 0, 0];
            let mut samples = vec![0; 8];
            samples[4..8].copy_from_slice(&10000u32.to_le_bytes());
            samples.extend([128; 10000]);
            let bank = SampleBank::new(&index, &samples).unwrap();
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.subtitles = false;
            engine.active_inventory = Some(2);
            if initially_enabled {
                engine.control.apply(1, true).unwrap();
            }
            let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
            process.inventory_object = Some(284);
            process.event = 3;
            process.object_print_position = Some([112, 120]);
            engine
                .processes
                .spawn(Some(engine.generation), process)
                .unwrap();
            engine
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap();
            assert!(!engine.control.enabled);
            assert!(engine.sample_player.id.is_some());
            assert_eq!(engine.left_events, 0);
            engine.left_event().unwrap();
            engine
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap();
            assert_eq!(engine.globals, [77]);
            assert_eq!(engine.control.enabled, initially_enabled);
            assert!(!engine.control.tags_enabled);
            assert_eq!(engine.sample_player.id, None);
        }
    }

    #[test]
    fn print_with_speech_and_no_subtitles_waits_for_its_sample() {
        let (master, mut scene) = fixtures();
        scene[8..18].copy_from_slice(&[2, 2, 2, 3, 2, 0x41, 2, 0x4e, 53, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut text = vec![1, 0, 52, 51, 0, 0, 0, 0, 0, 1, b'W'];
        text.extend([0; 62]);
        let strings = crate::text::Strings::new(&text).unwrap();
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let mut samples = vec![0; 8];
        samples[4..8].copy_from_slice(&1000u32.to_le_bytes());
        samples.extend([128; 1000]);
        let bank = SampleBank::new(&index, &samples).unwrap();
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.subtitles = false;
        assert_eq!(
            engine
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap(),
            Status::Running
        );
        assert_eq!(
            engine
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap(),
            Status::Running
        );
        assert_eq!(
            engine
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap(),
            Status::Halted
        );
    }
    #[test]
    fn positioned_speech_consumes_arguments_and_sustain_ignores_clicks() {
        for (id, sustain) in [(98u8, false), (104, true)] {
            assert_eq!(
                crate::library::name(i32::from(id)),
                Some(if sustain { "TALKATS" } else { "TALKAT" })
            );
            let (master, mut scene) = fixtures();
            // Leave a sentinel below the call's arguments, then store it afterward.
            let mut code = vec![0x41, 77, 0x41, -1i8 as u8, 2, 2, 3];
            if sustain {
                code.extend([0x41, 2]);
            }
            code.extend([0x4e, id, 0x4c, 0, 0]);
            scene[8..8 + code.len()].copy_from_slice(&code);
            let resources = Resources(vec![&master, &scene]);
            let strings =
                crate::text::Strings::new(&[1, 0, 52, 51, 0, 0, 0, 0, 0, 1, b'W']).unwrap();
            let index = [0, 0, 0, 0, 4, 0, 0, 0];
            let mut samples = vec![0; 8];
            samples[4..8].copy_from_slice(&3000u32.to_le_bytes());
            samples.extend([128; 3000]);
            let bank = SampleBank::new(&index, &samples).unwrap();
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.subtitles = false;
            engine.declarations.speech_colors.insert(-1, [12, 34, 56]);
            engine.left_event().unwrap();
            assert_eq!(
                engine
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap(),
                Status::Running
            );
            assert_eq!(engine.talk_color, sustain.then_some([12, 34, 56]));
            let calls = engine.pending_calls();
            let (_, call, operands) = calls
                .iter()
                .find(|(_, c, _)| c.id == i32::from(id))
                .unwrap();
            assert_eq!(call.id, i32::from(id));
            assert_eq!(operands[0], 77);
            engine.left_event().unwrap();
            let mut restored = engine.clone();
            for instance in [&mut engine, &mut restored] {
                let status = instance
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                assert_eq!(
                    status,
                    if sustain {
                        Status::Running
                    } else {
                        Status::Halted
                    }
                );
                if sustain {
                    for _ in 0..2 {
                        assert_eq!(
                            instance
                                .tick_content(&resources, Some(&bank), None, Some(&strings))
                                .unwrap(),
                            Status::Running
                        );
                    }
                    assert_eq!(
                        instance
                            .tick_content(&resources, Some(&bank), None, Some(&strings))
                            .unwrap(),
                        Status::Halted
                    );
                }
                assert_eq!(instance.globals, [77]);
                assert!(instance.sample_player.id.is_none());
            }
            assert_eq!(engine, restored);
        }
    }
    #[test]
    fn blocking_play_waits_only_for_primary_and_allows_peer_scripts() {
        let (master, mut scene) = fixtures();
        // PLAY(resource 2, 12, 13, -1, 0, complete), then set global 0 to 7.
        scene[8..26].copy_from_slice(&[
            1, 0, 0, 0, 1, 0x41, 12, 0x41, 13, 0x41, 255, 2, 3, 0x4e, 49, 0x41, 7, 0x4c,
        ]);
        scene[26..28].copy_from_slice(&[0, 0]);
        // Entrance runs while the primary is waiting.
        scene[32..37].copy_from_slice(&[0x41, 9, 0x4c, 0, 0]);
        scene[116..120].copy_from_slice(&((1u32 << 23) | 32).to_le_bytes());
        let base = 2u32 << 23;
        let mut film = vec![0; 104];
        for (at, value) in [
            (0, 24),
            (4, 2),
            (8, base + 24),
            (12, base + 72),
            (16, base + 48),
            (20, base + 84),
            (32, 1),
            (56, 2),
            (60, 5),
            (72, 10),
            (76, 10),
            (80, 0),
            (84, 10),
            (88, 1),
            (92, (-2i32) as u32),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(2).unwrap();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        assert_eq!(engine.globals, [9]);
        assert_eq!(engine.actor_steps.get(&1), Some(&1));
        assert_eq!(engine.films.len(), 1);
        assert_eq!((engine.films[0].object.x, engine.films[0].object.y), (5, 0));
        let primary = engine
            .processes
            .states()
            .find_map(|p| p.play_wait.as_ref())
            .unwrap();
        assert_eq!((primary.1.object.x, primary.1.object.y), (12, 13));
        let original = primary.1.clone();
        let mut killed = engine.clone();
        killed.disable_actor(1).unwrap();
        let mut dead_reel = original.clone();
        assert!(!killed.claim_reel(&mut dead_reel).unwrap());
        assert_eq!(killed.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(
            killed.globals,
            [7],
            "death releases the primary reel's waiting caller"
        );
        killed.disable_actor(2).unwrap();
        assert!(killed.films.is_empty());
        assert!(killed.reel_owners.is_empty());
        killed.actors.enable(1).unwrap();
        assert!(killed.claim_reel(&mut dead_reel).unwrap());
        let mut replaced = engine.clone();
        let mut duplicate = original.clone();
        assert!(!replaced.claim_reel(&mut duplicate).unwrap());
        assert_eq!(replaced, engine, "duplicate request must preserve progress");
        let mut replacement = original.clone();
        replacement.reel = Some(Handle(base + 16));
        assert!(replaced.claim_reel(&mut replacement).unwrap());
        let mut replay = original.clone();
        assert!(replaced.claim_reel(&mut replay).unwrap());
        assert!(
            !replaced.owns_reel(&original),
            "A-B-A must not revive the old A"
        );
        replaced.films.push(replay.clone());
        assert_eq!(replaced.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(
            replaced.globals,
            [7],
            "replacement releases the blocking caller"
        );
        assert!(
            replaced.owns_reel(&replay),
            "old caller must not release the new owner"
        );
        let mut secondary = replaced
            .films
            .iter()
            .find(|a| a.object.id == 2)
            .unwrap()
            .clone();
        secondary.reel = Some(Handle(base + 8));
        assert!(replaced.claim_reel(&mut secondary).unwrap());
        replaced.films.push(secondary);
        assert_eq!(
            replaced.films.iter().filter(|a| a.object.id == 2).count(),
            1
        );
        let mut restored = engine.clone();
        for instance in [&mut engine, &mut restored] {
            assert_eq!(instance.tick(&resources).unwrap(), Status::Running);
            assert_eq!(instance.globals, [9]);
            assert_eq!(instance.tick(&resources).unwrap(), Status::Halted);
            assert_eq!(instance.globals, [7]);
            assert_eq!(
                instance.films.len(),
                1,
                "looping secondary must not hold caller"
            );
            assert_eq!(instance.actor_steps.get(&1), Some(&3));
        }
        assert_eq!(engine, restored);
    }
    #[test]
    fn random_call_replaces_three_arguments_with_result() {
        let (master, mut scene) = fixtures();
        // RANDOM(2, 5, allow repeats), store result, halt.
        scene[8..18].copy_from_slice(&[0x41, 2, 0x41, 5, 2, 0x4e, 56, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        let mut restored = engine.clone();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.globals, [5]);
        assert_eq!(restored.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine, restored);
        let mut expected = crate::random::Random::new(1);
        expected.range(2, 5, 0);
        assert_eq!(engine.random, expected);
    }
    #[test]
    fn v1_polygon_scene_loads_without_running_tag_startup_scripts() {
        let (master, mut scene) = fixtures();
        scene[76..80].copy_from_slice(&1u32.to_le_bytes());
        scene[96..100].copy_from_slice(&((2u32 << 23) | 8).to_le_bytes());
        // Tag script would overwrite the global if incorrectly dispatched at startup.
        scene[32..37].copy_from_slice(&[0x41, 99, 0x4c, 0, 0]);
        let mut polygons = vec![0; 112];
        polygons[0..4].copy_from_slice(&0x33340001u32.to_le_bytes());
        polygons[8..12].copy_from_slice(&6u32.to_le_bytes());
        polygons[108..112].copy_from_slice(&((1u32 << 23) | 32).to_le_bytes());
        let resources = Resources(vec![&master, &scene, &polygons]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert_eq!(engine.polygons.active.len(), 1);
        assert_eq!(
            engine.polygons.active[0].definition.kind,
            crate::polygon::Kind::Tag
        );
        for _ in 0..3 {
            engine.tick(&resources).unwrap();
        }
        assert_eq!(engine.globals, [0]);
        assert!(engine.processes.is_empty());
    }
    #[test]
    fn setactor_dispatches_startup_once_after_its_caller_yields() {
        let (master, mut scene) = fixtures();
        scene[8..20].copy_from_slice(&[3, 0x4e, 62, 3, 0x4e, 62, 0x41, 2, 0x4c, 0, 0, 0]);
        // Child increments the global once; duplicate startup would produce 4.
        scene[32..39].copy_from_slice(&[0x4a, 0, 3, 26, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(1).unwrap();
        engine
            .actors
            .enter(&[scene::Actor {
                id: 1,
                masking: 0,
                script: Handle((1 << 23) | 32),
            }])
            .unwrap();
        engine.actors.get_mut(1).unwrap().completed = true;
        let mut restored = engine.clone();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.globals, [3]);
        assert!(engine.actors.get_mut(1).unwrap().completed);
        restored.tick(&resources).unwrap();
        assert_eq!(engine, restored);
    }
    #[test]
    fn movecursor_uses_world_coordinates_and_control_on_keeps_scripted_position() {
        let (master, mut scene) = fixtures();
        scene[8..18].copy_from_slice(&[0x41, 40, 0x41, 30, 0x4e, 43, 3, 0x4e, 14, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.world_origin = [20, 10];
        engine
            .control
            .apply_at(2, false, engine.world_origin)
            .unwrap();
        let mut restored = engine.clone();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.control.cursor, [20, 20]);
        assert!(engine.control.enabled);
        restored.tick(&resources).unwrap();
        assert_eq!(engine, restored);
    }
    #[test]
    fn offset_preserves_cursor_and_resets_on_scene_entry() {
        let (master, mut scene) = fixtures();
        scene[8..16].copy_from_slice(&[0x81, 128, 2, 0x41, -7i8 as u8, 0x4e, 47, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.control.cursor = [15, 25];
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.world_origin, [640, -7]);
        assert_eq!(engine.control.cursor, [15, 25]);
        engine.enter(&resources, Handle(1 << 23), 1, 1).unwrap();
        assert_eq!(engine.world_origin, [0, 0]);
    }
    #[test]
    fn blocking_scroll_waits_for_target_and_superseding_request_ends_caller() {
        let (master, mut scene) = fixtures();
        scene[8..20].copy_from_slice(&[0x41, 20, 2, 2, 3, 0x4e, 61, 0x41, 9, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.scroll_maximum = [640, 0];
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        assert_eq!(engine.world_origin, [0, 0]);
        let mut replaced = engine.clone();
        replaced.scroll.start([0, 0], [40, 0], 0).unwrap();
        assert_eq!(replaced.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(replaced.globals, [0]);
        let mut stopped = engine.clone();
        stopped.scroll.stop();
        assert_eq!(stopped.tick(&resources).unwrap(), Status::Running);
        assert_eq!(
            stopped.world_origin,
            [0, 0],
            "stopping motion does not satisfy the target wait"
        );
        let mut restored = engine.clone();
        for instance in [&mut engine, &mut restored] {
            for x in [8, 16] {
                assert_eq!(instance.tick(&resources).unwrap(), Status::Running);
                assert_eq!(instance.world_origin, [x, 0]);
            }
            assert_eq!(instance.tick(&resources).unwrap(), Status::Halted);
            assert_eq!(instance.world_origin, [20, 0]);
            assert_eq!(instance.globals, [9]);
        }
        assert_eq!(engine, restored);
    }
    #[test]
    fn animated_talk_yields_then_click_restores_normal_film_and_tags() {
        let (master, mut scene) = fixtures();
        scene[8..20].copy_from_slice(&[1, 0, 0, 0, 1, 3, 0x4e, 77, 0x41, 9, 0x4c, 0]);
        scene[20] = 0;
        let base = 2u32 << 23;
        let mut film = vec![0; 88];
        for (at, value) in [
            (0, 24),
            (4, 1),
            (8, base + 32),
            (12, base + 56),
            (40, 1),
            (56, 10),
            (60, 0),
            (64, 24),
            (68, 1),
            (72, base + 32),
            (76, base + 56),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        let strings = crate::text::Strings::new(&[1, 0, 52, 51, 0, 0, 0, 0, 0, 1, b'W']).unwrap();
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let mut samples = vec![0; 8];
        samples[4..8].copy_from_slice(&10000u32.to_le_bytes());
        samples.extend([128; 10000]);
        let bank = SampleBank::new(&index, &samples).unwrap();
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(1).unwrap();
        engine.subtitles = false;
        engine.control.tags_enabled = true;
        engine.launch_film(&resources, Handle(base + 64)).unwrap();
        for _ in 0..2 {
            assert_eq!(
                engine
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap(),
                Status::Running
            );
        }
        assert!(!engine.control.tags_enabled);
        assert!(engine.sample_player.id.is_some());
        let mut natural = engine.clone();
        let mut status = Status::Running;
        for _ in 0..12 {
            status = natural
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap();
            if status == Status::Halted {
                break;
            }
        }
        assert_eq!(status, Status::Halted);
        assert_eq!(natural.globals, [9]);
        assert!(natural.control.tags_enabled);
        assert_eq!(natural.films[0].reel, Some(Handle(base + 72)));
        engine.left_event().unwrap(); // zero-to-one cancels TALK, unlike PRINT.
        let mut restored = engine.clone();
        for instance in [&mut engine, &mut restored] {
            assert_eq!(
                instance
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap(),
                Status::Halted
            );
            assert_eq!(instance.globals, [9]);
            assert!(instance.control.tags_enabled);
            assert!(instance.sample_player.id.is_none());
            assert_eq!(instance.films.len(), 1);
            assert_eq!(instance.films[0].reel, Some(Handle(base + 72)));
            assert_eq!(instance.films[0].steps, 0, "normal film restarts");
        }
        assert_eq!(engine, restored);
    }
    #[test]
    fn scene_wait_and_snapshot_resume() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert_eq!(engine.scene, Handle(1 << 23));
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        let mut restored = engine.clone();
        assert_eq!(restored.tick(&resources).unwrap(), Status::Running);
        assert_eq!(restored.tick(&resources).unwrap(), Status::Halted);
        engine.tick(&resources).unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(restored, engine);
    }
    #[test]
    fn failed_scene_execution_is_atomic() {
        let (master, mut scene) = fixtures();
        scene[8] = 26;
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        let before = engine.clone();
        assert!(engine.tick(&resources).is_err());
        assert_eq!(engine, before);
    }
    #[test]
    fn sampleplaying_returns_channel_state_without_consuming_arguments_or_audio() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [0x41, 77, 0x50, 1, 0x4e, 111, 0x4c, 0, 0x4c, 1, 0];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let mut data = vec![0; 8];
        data[4..8].copy_from_slice(&1000u32.to_le_bytes());
        data.extend(vec![129; 1000]);
        let bank = SampleBank::new(&index, &data).unwrap();
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(2, 0);
        for phase in 0..4 {
            match phase {
                1 => {
                    engine.sample_player.play(&bank, 1).unwrap();
                }
                2 => {
                    engine.sample_player.tick(&bank).unwrap();
                }
                3 => {
                    engine.sample_player.tick(&bank).unwrap();
                }
                _ => {}
            }
            let before = engine.sample_player.clone();
            let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
            assert_eq!(
                engine
                    .run_process(&resources, None, None, None, &mut process)
                    .unwrap(),
                Status::Halted
            );
            assert_eq!(engine.globals, [i32::from(matches!(phase, 1 | 2)), 77]);
            assert_eq!(engine.sample_player, before);
        }
    }
    #[test]
    fn tryplaysample_busy_channel_yields_once_without_replacing_or_retrying() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let code = [0x41, 77, 0x41, 2, 3, 0x4e, 112, 0x4c, 0, 0];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(1, 0);
        engine.sample_player.id = Some(1);
        engine.sample_player.serial = 9;
        engine.sample_player.position = 123;
        let before = engine.sample_player.clone();
        let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Running
        );
        assert_eq!(engine.sample_player, before);
        assert_eq!(engine.globals[0], 0);
        // The channel can become free before resumption; this call must still
        // skip its sample, rather than retrying playback after the yield.
        engine.sample_player.stop();
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(engine.sample_player.serial, 9);
        assert_eq!(engine.globals[0], 77);
    }

    #[test]
    fn blocking_sample_waits_until_pcm_is_consumed() {
        let (master, mut scene) = fixtures();
        for opcode in [51, 112] {
            scene[8..13].copy_from_slice(&[3, 3, 0x4e, opcode, 0]);
            let resources = Resources(vec![&master, &scene]);
            let index = [0, 0, 0, 0, 4, 0, 0, 0];
            let mut data = vec![0; 8];
            data[4..8].copy_from_slice(&1000u32.to_le_bytes());
            data.extend(vec![129; 1000]);
            let bank = SampleBank::new(&index, &data).unwrap();
            let mut engine = Engine::bootstrap(&resources).unwrap();
            assert_eq!(
                engine.tick_audio(&resources, Some(&bank)).unwrap(),
                Status::Running
            );
            assert!(engine.audio_frame.iter().all(|x| *x == 0));
            let mut restored = engine.clone();
            assert_eq!(
                engine.tick_audio(&resources, Some(&bank)).unwrap(),
                Status::Running
            );
            assert_eq!(engine.audio_frame[0], 256);
            assert_eq!(
                engine.tick_audio(&resources, Some(&bank)).unwrap(),
                Status::Halted
            );
            assert!(engine.sample_player.id.is_none());
            restored.tick_audio(&resources, Some(&bank)).unwrap();
            restored.tick_audio(&resources, Some(&bank)).unwrap();
            assert_eq!(engine, restored);
        }
    }
}
