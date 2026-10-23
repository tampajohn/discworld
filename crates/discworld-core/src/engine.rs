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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Running,
    Unsupported(LibraryCall),
    Halted,
}
/// Original V1 EVENT values for resolved player interactions.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum PolygonInteraction {
    WalkTo = 1,
    Action = 2,
    Look = 3,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct KeyWait {
    call: LibraryCall,
    events: u64,
    cursor: [i32; 2],
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
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
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct TalkWait {
    #[serde(skip)]
    reel_finished: bool,
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
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
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
    #[serde(skip)]
    sample_serial: Option<u64>,
}
impl ScriptProcess {
    fn wait_calls(&self) -> [Option<LibraryCall>; 11] {
        [
            self.walked_wait.as_ref().map(|w| w.call),
            self.walk_wait.map(|w| w.0),
            self.tag_walk_wait.map(|w| w.0),
            self.talk_wait.as_ref().map(|w| w.call),
            self.scroll_wait.map(|w| w.0),
            self.play_wait.as_ref().map(|w| w.0),
            self.print_wait.as_ref().map(|w| w.call),
            self.key_wait.as_ref().map(|w| w.call),
            self.music_wait,
            self.wait.map(|w| w.0),
            self.sample_wait,
        ]
    }
    fn validate_snapshot_waits(&self) -> Result<(), Error> {
        let pending = self.vm.pending_call();
        let calls = self.wait_calls();
        if calls.iter().flatten().any(|call| Some(*call) != pending)
            || calls.iter().flatten().count() > 1
        {
            return Err(Error("snapshot host wait does not match pending call"));
        }
        Ok(())
    }

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
            sample_serial: None,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct WalkedWait {
    call: LibraryCall,
    actor: i32,
    target: [i32; 2],
    delay: u32,
    serial: Option<u64>,
}

/// Interpreter-only scene state. Host waits, animations, and scheduler sleeps
/// are rebuilt rather than resumed from their old coroutine locals.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SceneScripts {
    scene: Handle,
    contexts: Vec<SavedScript>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
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
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SceneMovers {
    scene: Handle,
    banks: std::collections::BTreeMap<i32, [[Handle; 12]; 10]>,
    active: Vec<(i32, [i32; 2], Handle)>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct SceneRestore {
    snapshot: SceneSnapshot,
    remaining: u8,
    fade: bool,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SceneCall {
    pub tick: u64,
    pub script: Handle,
    pub call: LibraryCall,
    pub operands: Vec<i32>,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    saved_scenes: Vec<SceneSnapshot>,
    scene_restore: Option<SceneRestore>,
    master_stopped: bool,
    restart_pending: bool,
    has_restarted: bool,
    pub conversation: crate::conversation::Conversation,
    inventory_display: Option<(
        crate::conversation_window::Frame,
        crate::conversation_window::Content,
    )>,
    inventory_dirty: bool,
    pub top_window: Option<crate::conversation_window::Frame>,
    /// Shared menu state; serialized by the versioned snapshot extension.
    /// Input/lifecycle integration is separate from checkpoint ownership.
    #[serde(skip)]
    pub configuration: Option<crate::menu_controller::Controller>,
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
    #[serde(skip)]
    pub(crate) clicks: crate::click::State,
    #[serde(skip)]
    pub(crate) mouse: Option<crate::mouse::MouseInput>,
    #[serde(skip)]
    pub(crate) inventory_drag: Option<i32>,
    #[serde(skip)]
    pub(crate) inventory_slider: Option<crate::inventory_slider::Drag>,
    #[serde(skip)]
    pub(crate) inventory_resize: Option<crate::inventory_resize::Drag>,
    #[serde(skip)]
    pub(crate) inventory_geometry: crate::inventory_geometry::State,
    #[serde(skip)]
    pub(crate) inventory_hover: crate::inventory_hover::State,
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
    #[serde(skip)]
    pub(crate) reel_escape: std::collections::BTreeMap<i32, (u64, u64)>,
    /// Live SPLAY reels whose exit must restore mover coordinates, including
    /// superseded reels waiting for their next coroutine turn.
    #[serde(skip)]
    pub(crate) splay_reels: std::collections::BTreeSet<u64>,
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
    #[serde(skip)]
    escape_events: Option<u64>,
    #[serde(skip)]
    pub(crate) intro: Option<crate::intro::Intro>,
    user_events: u64,
    left_events: u64,
    pub subtitles: bool,
    talk_color: Option<[u8; 3]>,
    pub background: Option<Animation>,
    background_film: Option<Handle>,
    background_palette: Option<crate::palette::BackgroundPalette>,
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
    fn capture_escape(counter: &mut Option<u64>, vm: &mut Machine, enabled: bool) {
        vm.escape_generation = if enabled {
            Some(*counter.get_or_insert(1))
        } else {
            None
        };
    }
    fn migrate_escape(counter: &mut Option<u64>, vm: &mut Machine) {
        if vm.escape_enabled && vm.escape_generation.is_none() {
            Self::capture_escape(counter, vm, true);
        }
    }
    fn escaped(&self, vm: &Machine) -> bool {
        vm.escape_enabled
            && vm
                .escape_generation
                .is_some_and(|captured| Some(captured) != self.escape_events)
    }
    /// Shared script event; frontends must still supply menu/intro precedence.
    /// Counter overflow rejects the event before mutating any input state.
    pub fn script_escape_event(&mut self) -> Result<(), Error> {
        let previous = self.escape_events.unwrap_or(1);
        let next = previous
            .checked_add(1)
            .ok_or(Error("escape event overflow"))?;
        let users = self
            .user_events
            .checked_add(1)
            .ok_or(Error("input event overflow"))?;
        let left = self
            .left_events
            .checked_add(1)
            .ok_or(Error("left event overflow"))?;
        // Legacy snapshots had a Boolean but no event stamp. Arm them against
        // the pre-event generation so this first new event is not lost.
        for vm in self.escape_contexts_mut() {
            if vm.escape_enabled && vm.escape_generation.is_none() {
                vm.escape_generation = Some(previous);
            }
        }
        self.escape_events = Some(next);
        self.user_events = users;
        self.left_events = left;
        self.control.user_event(self.ticks);
        Ok(())
    }

    fn escape_contexts(&self) -> impl Iterator<Item = &Machine> {
        std::iter::once(&self.master)
            .chain(self.processes.states().map(|p| &p.vm))
            .chain(
                self.saved_scenes
                    .iter()
                    .flat_map(|s| s.scripts.contexts.iter().map(|c| &c.vm)),
            )
            .chain(
                self.scene_restore
                    .iter()
                    .flat_map(|r| r.snapshot.scripts.contexts.iter().map(|c| &c.vm)),
            )
    }
    fn escape_contexts_mut(&mut self) -> impl Iterator<Item = &mut Machine> {
        std::iter::once(&mut self.master)
            .chain(self.processes.states_mut().map(|p| &mut p.vm))
            .chain(
                self.saved_scenes
                    .iter_mut()
                    .flat_map(|s| s.scripts.contexts.iter_mut().map(|c| &mut c.vm)),
            )
            .chain(
                self.scene_restore
                    .iter_mut()
                    .flat_map(|r| r.snapshot.scripts.contexts.iter_mut().map(|c| &mut c.vm)),
            )
    }
    pub(crate) fn escape_checkpoint(&self) -> Option<(u64, Vec<Option<u64>>)> {
        self.escape_events.map(|event| {
            (
                event,
                self.escape_contexts()
                    .map(|vm| vm.escape_generation)
                    .collect(),
            )
        })
    }
    pub(crate) fn clear_escape_checkpoint(&mut self) {
        self.escape_events = None;
        for vm in self.escape_contexts_mut() {
            vm.escape_generation = None;
        }
    }
    pub(crate) fn restore_escape_checkpoint(
        &mut self,
        event: u64,
        captures: &[Option<u64>],
    ) -> Result<(), Error> {
        if event == 0 || captures.len() != self.escape_contexts().count() {
            return Err(Error("invalid escape checkpoint shape"));
        }
        for (vm, captured) in self.escape_contexts().zip(captures) {
            if captured.is_some_and(|value| value == 0 || value > event || !vm.escape_enabled) {
                return Err(Error("invalid captured escape generation"));
            }
        }
        self.escape_events = Some(event);
        for (vm, captured) in self.escape_contexts_mut().zip(captures) {
            vm.escape_generation = *captured;
        }
        Ok(())
    }

    pub(crate) fn sample_wait_checkpoint(&self) -> Vec<Option<u64>> {
        self.processes.states().map(|p| p.sample_serial).collect()
    }
    pub(crate) fn clear_sample_wait_checkpoint(&mut self) {
        for process in self.processes.states_mut() {
            process.sample_serial = None;
        }
    }
    pub(crate) fn restore_sample_wait_checkpoint(
        &mut self,
        serials: &[Option<u64>],
    ) -> Result<(), Error> {
        if serials.len() != self.processes.states().count()
            || self.processes.states().zip(serials).any(|(p, serial)| {
                serial.is_some_and(|s| {
                    s == 0 || s > self.sample_player.serial || p.sample_wait.is_none()
                })
            })
        {
            return Err(Error("invalid sample wait checkpoint"));
        }
        for (process, serial) in self.processes.states_mut().zip(serials) {
            process.sample_serial = *serial;
        }
        Ok(())
    }

    fn snapshot_animations(&self) -> impl Iterator<Item = &Animation> {
        self.background
            .iter()
            .chain(self.cursor_animation.iter())
            .chain(self.films.iter())
            .chain(self.movers.values().map(|m| &m.animation))
            .chain(
                self.processes
                    .states()
                    .filter_map(|p| p.play_wait.as_ref().map(|w| &w.1)),
            )
            .chain(self.inventory_resize.iter().flat_map(|r| r.animations()))
            .chain(self.held_cursor.iter().map(|c| &c.animation))
            .chain(self.top_window.iter().flat_map(|f| &f.parts))
            .chain(self.inventory_display.iter().flat_map(|(f, _)| &f.parts))
            .chain(
                self.inventory_display
                    .iter()
                    .flat_map(|(_, c)| c.icons.iter().map(|i| &i.animation)),
            )
    }

    pub(crate) fn validate_snapshot_structure(&self) -> Result<(), Error> {
        for &playback in &self.splay_reels {
            let matches = self
                .films
                .iter()
                .chain(
                    self.processes
                        .states()
                        .filter_map(|p| p.play_wait.as_ref().map(|w| &w.1)),
                )
                .filter(|a| a.playback == playback && a.reel.is_some())
                .count();
            if playback == 0 || playback > self.next_playback || matches != 1 {
                return Err(Error("invalid snapshot SPLAY reel identity"));
            }
        }
        if let Some(intro) = &self.intro {
            intro.validate(
                self.escape_events
                    .ok_or(Error("intro missing escape counter"))?,
            )?;
        }
        if self.processes.states().any(|p| {
            p.sample_serial.is_some_and(|serial| {
                serial == 0 || serial > self.sample_player.serial || p.sample_wait.is_none()
            })
        }) {
            return Err(Error("invalid snapshot sample wait ownership"));
        }
        for (&actor, &(playback, captured)) in &self.reel_escape {
            if captured == 0
                || self.escape_events.is_none_or(|event| captured > event)
                || self
                    .reel_owners
                    .get(&actor)
                    .is_none_or(|(_, owner)| *owner != playback)
            {
                return Err(Error("invalid snapshot reel escape owner"));
            }
        }

        if self.escape_events == Some(0)
            || self.escape_contexts().any(|vm| {
                vm.escape_generation.is_some_and(|value| {
                    value == 0
                        || !vm.escape_enabled
                        || self.escape_events.is_none_or(|event| value > event)
                })
            })
        {
            return Err(Error("invalid snapshot escape generation"));
        }

        for wait in self
            .processes
            .states()
            .filter_map(|process| process.talk_wait.as_ref())
        {
            if wait.reel_finished
                && (!wait.initialized
                    || wait.sample.is_some()
                    || wait.layout.is_none()
                    || wait.mover_saved.is_some())
            {
                return Err(Error("invalid finished talking reel state"));
            }
        }
        if self
            .configuration
            .as_ref()
            .is_some_and(|menu| menu.page().is_some())
            && (self.active_inventory.is_some() || self.top_window.is_some())
        {
            return Err(Error("configuration overlaps another inventory window"));
        }
        if let Some(slot) = self.inventory_drag {
            if !(0..=2).contains(&slot)
                || self.active_inventory != Some(slot)
                || self.declarations.windows[slot as usize].is_none()
                || self.inventory_display.is_none()
                || self.conversation.hidden
                || self.mouse.is_none()
            {
                return Err(Error("invalid inventory drag owner"));
            }
        }
        if let Some(slider) = &self.inventory_slider {
            if !matches!(self.inventory_drag, Some(1 | 2)) {
                return Err(Error("invalid slider owner"));
            }
            let bar = self
                .inventory_display
                .as_ref()
                .and_then(|(frame, _)| frame.layout.scrollbar.as_ref())
                .ok_or(Error("missing saved slider"))?;
            slider.validate(bar)?;
        }
        if let Some(resize) = &self.inventory_resize {
            let Some(slot @ (1 | 2)) = self.inventory_drag else {
                return Err(Error("invalid resize owner"));
            };
            if self.inventory_slider.is_some() {
                return Err(Error("conflicting inventory gestures"));
            }
            resize.validate(
                self.declarations.windows[slot as usize]
                    .as_ref()
                    .ok_or(Error("missing resize window"))?,
            )?;
        }
        self.inventory_geometry
            .validate(&self.declarations.windows, self.active_inventory)?;
        self.clicks.validate(&self.processes)?;
        for (index, (id, wait)) in self.inventory_hover.waits.iter().enumerate() {
            if self.inventory_hover.waits[..index]
                .iter()
                .any(|(other, _)| other == id)
            {
                return Err(Error("duplicate inventory hover process"));
            }
            let process = self
                .processes
                .entries()
                .find(|(p, _)| p == id)
                .map(|(_, p)| p)
                .ok_or(Error("missing inventory hover process"))?;
            if process.event != 0 || process.inventory_object != Some(wait.item()) {
                return Err(Error("invalid inventory hover process"));
            }
            wait.validate(&self.inventory_hover.monitor)?;
            if let crate::inventory_hover::Wait::Leave(_) = wait {
                if !process.vm.is_halted() {
                    return Err(Error("inventory unpoint waiter has unfinished script"));
                }
            }
            if let crate::inventory_hover::Wait::Label { call, .. } = wait {
                if process.vm.pending_call() != Some(*call)
                    || process.wait_calls().iter().any(Option::is_some)
                    || process.object_print_position.is_some()
                {
                    return Err(Error("inventory hover call mismatch"));
                }
            }
        }
        for (id, phase) in self.clicks.entries() {
            let process = self
                .processes
                .entries()
                .find(|(p, _)| *p == id)
                .map(|(_, p)| p)
                .ok_or(Error("missing click process"))?;
            if let crate::click::Phase::Drop(index) | crate::click::Phase::DropWaiting(index) =
                phase
            {
                if index > 4096 || process != &ScriptProcess::new(Handle(0)) {
                    return Err(Error("invalid pending inventory drop"));
                }
            } else {
                let expected = if phase == crate::click::Phase::Double {
                    2
                } else {
                    1
                };
                let scripted_drop =
                    expected == 1 && process.inventory_object.is_some() && process.event == 9;
                if process.player_walk.is_some()
                    || (process.polygon.is_none()
                        && process.actor.is_none()
                        && process.inventory_object.is_none())
                    || (process.event != expected && !scripted_drop)
                {
                    return Err(Error("invalid click script target"));
                }
            }
        }
        if self
            .active_inventory
            .is_some_and(|slot| !(0..=2).contains(&slot))
            || self.fade_in_step.is_some_and(|step| step >= 7)
            || self
                .pending_scene
                .is_some_and(|(_, _, _, left)| left.is_some_and(|n| !(1..=6).contains(&n)))
            || self
                .scene_restore
                .as_ref()
                .is_some_and(|r| r.remaining > 11)
        {
            return Err(Error("invalid snapshot inventory or transition index"));
        }
        for window in self.declarations.windows.iter().flatten() {
            if !(1..=10).contains(&window.size[0])
                || !(1..=6).contains(&window.size[1])
                || window.contents.len() > 150
                || window.first_display > 150
            {
                return Err(Error("invalid snapshot inventory dimensions"));
            }
        }
        for (&id, mover) in &self.movers {
            if mover.hidden
                && (id != mover.placement.actor
                    || u32::try_from(id)
                        .ok()
                        .is_none_or(|id| self.actors.get(id).is_err()))
            {
                return Err(Error("invalid hidden mover identity"));
            }
            if !(1..=10).contains(&mover.placement.scale)
                || mover
                    .placement
                    .path
                    .is_some_and(|p| p >= self.polygons.active.len())
            {
                return Err(Error("invalid snapshot mover scale or path"));
            }
        }
        for animation in self.snapshot_animations() {
            animation.validate_snapshot_structure()?;
        }
        if self.fade_multiplier > 0x10000 {
            return Err(Error("snapshot fade multiplier out of range"));
        }
        self.sample_player.validate_snapshot_structure()?;
        self.music.validate_snapshot_structure()?;
        self.master.validate_snapshot_structure()?;
        self.processes.validate_snapshot_structure()?;
        for process in self.processes.states() {
            if let Some((actor, target)) = process.player_walk {
                let mut expected = ScriptProcess::new(Handle(0));
                expected.player_walk = Some((actor, target));
                if process != &expected
                    || self.declarations.lead.as_ref().map(|lead| lead.id) != Some(actor)
                    || !self.movers.contains_key(&actor)
                {
                    return Err(Error("invalid queued player walk"));
                }
            }
            process.vm.validate_snapshot_structure()?;
            process.validate_snapshot_waits()?;
            if let Some((call, actor, _, _)) = process.tag_walk_wait {
                if call.id == 90
                    && (!self.movers.contains_key(&actor)
                        || process
                            .polygon
                            .is_none_or(|index| index >= self.polygons.active.len()))
                {
                    return Err(Error("invalid WALKPOLY snapshot context"));
                }
            }
        }
        if self
            .master_wait
            .is_some_and(|(call, _)| self.master.pending_call() != Some(call))
        {
            return Err(Error("snapshot master wait does not match pending call"));
        }
        if self.saved_scenes.len() > 4 {
            return Err(Error("snapshot saved scene nesting limit"));
        }
        for saved in self
            .saved_scenes
            .iter()
            .chain(self.scene_restore.iter().map(|restore| &restore.snapshot))
        {
            for context in &saved.scripts.contexts {
                context.vm.validate_snapshot_structure()?;
            }
        }
        Ok(())
    }

    pub(crate) fn validate_snapshot_scripts(&self, resources: &Resources<'_>) -> Result<(), Error> {
        self.validate_snapshot_structure()?;
        Self::validate_intro_title(
            resources,
            self.intro.as_ref().and_then(crate::intro::Intro::title),
        )?;
        self.master
            .validate_snapshot_code(scene::script_bytes(resources, self.master_code)?)?;
        for (id, process) in self.processes.entries() {
            // Walk and ordinary putdown tasks are native coroutines, not
            // bytecode. Structural validation requires pristine task state.
            if process.player_walk.is_some()
                || self.clicks.entries().any(|(pending, phase)| {
                    pending == id
                        && matches!(
                            phase,
                            crate::click::Phase::Drop(_) | crate::click::Phase::DropWaiting(_)
                        )
                })
            {
                continue;
            }
            process
                .vm
                .validate_snapshot_code(scene::script_bytes(resources, process.code)?)?;
        }
        for saved in self
            .saved_scenes
            .iter()
            .chain(self.scene_restore.iter().map(|r| &r.snapshot))
        {
            for context in &saved.scripts.contexts {
                context
                    .vm
                    .validate_snapshot_code(scene::script_bytes(resources, context.code)?)?;
            }
        }
        Ok(())
    }

    pub(crate) fn snapshot_preview(&self, resources: &Resources<'_>) -> Result<Surface, Error> {
        self.validate_snapshot_structure()?;
        for mover in self.movers.values() {
            mover.validate_reel_resources(resources)?;
        }
        for animation in self.snapshot_animations() {
            animation.validate_snapshot_resources(resources)?;
        }
        // Hidden inventory and deferred text must also have drawable resources.
        let mut scratch = Surface::new(320, 200)?;
        for (_, wait) in &self.inventory_hover.waits {
            if let crate::inventory_hover::Wait::Label { layout, .. } = wait {
                layout.draw(resources, &mut scratch)?;
            }
        }
        if let Some((frame, content)) = &self.inventory_display {
            frame.draw(resources, &mut scratch)?;
            content.draw(resources, &mut scratch)?;
        }
        for process in self.processes.states() {
            for layout in process
                .talk_wait
                .iter()
                .filter_map(|w| w.layout.as_ref())
                .chain(process.print_wait.iter().filter_map(|w| w.layout.as_ref()))
            {
                layout.draw(resources, &mut scratch)?;
            }
        }
        self.render(resources)
    }

    pub(crate) fn validate_snapshot_polygons(
        &self,
        resources: &Resources<'_>,
    ) -> Result<(), Error> {
        let scene = scene::read(resources, self.scene.resource())?;
        let expected = if let Some(scene) = scene {
            crate::polygon::Polygons::new(
                crate::polygon::read(resources, scene.polygons, scene.polygon_count)?,
                self.polygons.default_refer,
            )
        } else {
            crate::polygon::Polygons::default()
        };
        if self.polygons.active.len() != expected.active.len()
            || self
                .polygons
                .active
                .iter()
                .zip(&expected.active)
                .any(|(a, b)| a.definition != b.definition || a.adjacent != b.adjacent)
        {
            return Err(Error("snapshot polygons do not match original scene"));
        }
        Ok(())
    }

    /// Original HASRESTARTED state, set only after a committed game reset.
    pub fn has_restarted(&self) -> bool {
        self.has_restarted
    }

    /// Whether a scene change, restoration, or its background fade is unfinished.
    /// Script control can return before the new scene has faded into view.
    pub fn scene_transition_active(&self) -> bool {
        self.pending_scene.is_some()
            || self.scene_restore.is_some()
            || self.fade_on_background
            || self.fade_in_step.is_some()
    }

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
        // V1 saved interpreter contexts exclude POINTED. New scheduler IDs
        // may reuse old numbers, so retaining process-keyed hover waits here
        // could attach a departed label to an unrelated restored script.
        self.inventory_hover.waits.clear();
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
    /// V1 inventoryActive includes configuration pages, but excludes the
    /// tutorial's BOGUS_INV top window. Used by CONTROL_ON's tag restoration.
    fn control_inventory_active(&self) -> bool {
        self.active_inventory.is_some()
            || self
                .configuration
                .as_ref()
                .is_some_and(|menu| menu.page().is_some())
    }
    /// ProcessKeyEvent/openMenu's gate only. A true result lets the shared menu
    /// controller open key.page(); activation/rendering are a separate step.
    /// Ordinary rejected keys still count as user events, while ENABLEMENU's
    /// startup F1 path applies CONTROL_ON without waking WAITKEY via the counter.
    pub fn configuration_key(
        &mut self,
        resources: &Resources<'_>,
        key: crate::menu_navigation::Key,
        configuration_active: bool,
    ) -> Result<bool, Error> {
        let mut next = self.clone();
        if next.menu_enabled && key == crate::menu_navigation::Key::Main {
            next.control.apply_at(
                1,
                next.control_inventory_active() || configuration_active,
                next.world_origin,
            )?;
            if let Some((handle, _)) = &next.declarations.cursor {
                next.cursor_animation = Self::start_film(resources, *handle, true)?.pop();
                if let Some(cursor) = &mut next.cursor_animation {
                    cursor.tick(resources)?;
                }
            }
            next.menu_enabled = false;
        } else {
            next.user_event()?;
        }
        let permitted = next.control.enabled
            && !next.control_inventory_active()
            && next.top_window.is_none()
            && !configuration_active;
        *self = next;
        Ok(permitted)
    }
    /// Apply a menu-opening key to an initialized configuration controller.
    /// Resource failures leave the engine (including input counters) unchanged.
    pub fn open_configuration(
        &mut self,
        resources: &Resources<'_>,
        strings: &crate::text::Strings<'_>,
        saves: &[crate::save_menu::Entry],
        key: crate::menu_navigation::Key,
    ) -> Result<bool, Error> {
        let mut next = self.clone();
        let active = next
            .configuration
            .as_ref()
            .is_some_and(|menu| menu.page().is_some());
        if !next.configuration_key(resources, key, active)? {
            *self = next;
            return Ok(false);
        }
        let menu = next
            .configuration
            .as_mut()
            .ok_or(Error("configuration settings not initialized"))?;
        let assets = crate::menu_controller::Assets {
            resources,
            strings,
            declarations: &next.declarations,
            saves,
        };
        let opened = menu.open(key.page(), &assets)?;
        if opened {
            // V1 popUpInventory disables tags but leaves pointing enabled.
            next.control.tags_enabled = false;
            if let Some(position) = menu.take_cursor_warp() {
                next.control.move_cursor(position);
            }
            menu.point_at(Some(next.control.cursor));
        }
        *self = next;
        Ok(opened)
    }

    /// Diverted save-name characters do not reach PlayerEvent and therefore
    /// do not increment its user-event counter or wake WAITKEY.
    pub fn edit_configuration(
        &mut self,
        resources: &Resources<'_>,
        byte: u8,
        backspace: bool,
    ) -> Result<bool, Error> {
        match &mut self.configuration {
            Some(menu) => menu.edit(resources, byte, backspace),
            None => Ok(false),
        }
    }

    /// Page/Home/End and wheel events still pass through PlayerEvent's gate.
    pub fn scroll_configuration(
        &mut self,
        action: crate::save_menu::Scroll,
    ) -> Result<bool, Error> {
        self.user_event()?;
        Ok(self.control.enabled
            && self
                .configuration
                .as_mut()
                .is_some_and(|menu| menu.scroll(action)))
    }

    /// Deliver a resolved menu gesture and return any immediate save/load
    /// request. Hosts must consume the dispatch before advancing the game.
    pub fn configuration_click(
        &mut self,
        point: [i32; 2],
        interaction: PolygonInteraction,
    ) -> Result<crate::menu_controller::Dispatch, Error> {
        let mut next = self.clone();
        next.left_event()?;
        let mut dispatch = crate::menu_controller::Dispatch {
            outcome: crate::menu_navigation::Outcome::default(),
            selection: None,
        };
        if next.control.enabled {
            if let Some(menu) = &mut next.configuration {
                let before = menu.page();
                dispatch = menu.click(point, interaction == PolygonInteraction::Action)?;
                let changed_page = before != menu.page();
                next.finish_configuration_transition(before.is_some(), changed_page);
            }
        }
        *self = next;
        Ok(dispatch)
    }

    /// Escape closes Main, or returns a submenu to Main and requests that the
    /// host persist settings. Input event accounting belongs to the dispatcher.
    pub fn escape_configuration(
        &mut self,
        resources: &Resources<'_>,
        strings: &crate::text::Strings<'_>,
        saves: &[crate::save_menu::Entry],
    ) -> Result<crate::menu_navigation::Outcome, Error> {
        let mut next = self.clone();
        let Some(menu) = &mut next.configuration else {
            return Ok(crate::menu_navigation::Outcome::default());
        };
        let was_active = menu.page().is_some();
        let outcome = menu.escape(&crate::menu_controller::Assets {
            resources,
            strings,
            declarations: &next.declarations,
            saves,
        })?;
        next.finish_configuration_transition(was_active, outcome.changed);
        *self = next;
        Ok(outcome)
    }

    /// V1 player Escape: account for the event before the control gate, then
    /// route active menus/inventories. Commit only a valid complete transition.
    pub fn escape_input(
        &mut self,
        resources: &Resources<'_>,
        strings: &crate::text::Strings<'_>,
        saves: &[crate::save_menu::Entry],
    ) -> Result<crate::menu_navigation::Outcome, Error> {
        let mut next = self.clone();
        next.script_escape_event()?;
        let mut outcome = crate::menu_navigation::Outcome::default();
        if next.control.enabled {
            if next
                .configuration
                .as_ref()
                .is_some_and(|menu| menu.page().is_some())
            {
                outcome = next.escape_configuration(resources, strings, saves)?;
            } else if !next.conversation.hidden {
                if next.active_inventory == Some(0) {
                    if let Some(polygon) = next.conversation.close_input() {
                        next.close_conversation();
                        next.queue_converse(resources, polygon)?;
                    }
                } else if next.active_inventory.is_some() || next.top_window.is_some() {
                    next.close_inventory();
                }
            }
        }
        *self = next;
        Ok(outcome)
    }

    /// One InventoryProcess update, including effects retained after closure.
    /// The caller must consume Save/Load/Restart/Quit dispatches at this boundary.
    pub fn advance_configuration(
        &mut self,
        resources: &Resources<'_>,
        strings: &crate::text::Strings<'_>,
        saves: &[crate::save_menu::Entry],
    ) -> Result<crate::menu_controller::Dispatch, Error> {
        let mut next = self.clone();
        let Some(menu) = &mut next.configuration else {
            return Ok(crate::menu_controller::Dispatch {
                outcome: crate::menu_navigation::Outcome::default(),
                selection: None,
            });
        };
        let was_active = menu.page().is_some();
        let dispatch = menu.advance(&crate::menu_controller::Assets {
            resources,
            strings,
            declarations: &next.declarations,
            saves,
        })?;
        next.subtitles = menu.settings().subtitles;
        next.finish_configuration_transition(was_active, dispatch.outcome.changed);
        *self = next;
        Ok(dispatch)
    }

    fn finish_configuration_transition(&mut self, was_active: bool, refresh_pointer: bool) {
        if let Some(menu) = &mut self.configuration {
            let active = menu.page().is_some();
            if was_active || active {
                self.control.tags_enabled = !active;
            }
            if let Some(position) = menu.take_cursor_warp() {
                self.control.move_cursor(position);
            }
            if active && refresh_pointer {
                menu.point_at(Some(self.control.cursor));
            }
        }
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
    /// Deliver a physical edge at the deterministic engine tick.
    /// Menu-aware counterpart returning synchronous Save/Load dispatches.
    /// Hosts must consume the result; the legacy entry point handles world input.
    pub fn mouse_button_with_configuration(
        &mut self,
        resources: &Resources<'_>,
        button: crate::mouse::Button,
        down: bool,
    ) -> Result<crate::menu_controller::Dispatch, Error> {
        use crate::mouse::{Button, MouseInput, PlayerEvent};
        let mut dispatch = crate::menu_controller::Dispatch {
            outcome: crate::menu_navigation::Outcome::default(),
            selection: None,
        };
        let Some(settings) = self
            .configuration
            .as_ref()
            .filter(|menu| menu.page().is_some())
            .map(|menu| menu.settings())
        else {
            self.mouse_button(resources, button, down)?;
            return Ok(dispatch);
        };
        let mut next = self.clone();
        let mouse = next
            .mouse
            .get_or_insert_with(|| MouseInput::new(0, u32::from(settings.double_click)));
        mouse.set_double_click_ticks(u32::from(settings.double_click));
        let events = if down {
            mouse
                .button_down(button, next.ticks as u32, settings.swap_buttons)
                .to_vec()
        } else {
            vec![mouse.button_up(button, next.ticks as u32, settings.swap_buttons)]
        };
        for event in events {
            match event {
                PlayerEvent::WalkTo | PlayerEvent::Look | PlayerEvent::Action => {
                    let interaction = match event {
                        PlayerEvent::Action => PolygonInteraction::Action,
                        PlayerEvent::Look => PolygonInteraction::Look,
                        _ => PolygonInteraction::WalkTo,
                    };
                    dispatch = next.configuration_click(next.control.cursor, interaction)?;
                }
                _ => {
                    next.user_event()?;
                    if let Some(menu) = &mut next.configuration {
                        match event {
                            PlayerEvent::DragStart(Button::Left) if next.control.enabled => {
                                menu.begin_drag(resources, next.control.cursor)?;
                            }
                            PlayerEvent::DragEnd(Button::Left) => {
                                // PlayerEvent admits drag-end even with control off.
                                menu.end_drag()?;
                                menu.point_at(Some(next.control.cursor));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        *self = next;
        Ok(dispatch)
    }

    /// Deliver a physical edge at the deterministic engine tick.
    pub fn mouse_button(
        &mut self,
        resources: &Resources<'_>,
        button: crate::mouse::Button,
        down: bool,
    ) -> Result<(), Error> {
        use crate::mouse::{DEFAULT_DOUBLE_CLICK_TICKS, MouseInput, PlayerEvent};
        let mut next = self.clone();
        next.update_inventory_hover(resources)?;
        let settings = next.configuration.as_ref().map(|menu| menu.settings());
        let speed = settings
            .as_ref()
            .map_or(DEFAULT_DOUBLE_CLICK_TICKS, |settings| {
                u32::from(settings.double_click)
            });
        let swap = settings
            .as_ref()
            .is_some_and(|settings| settings.swap_buttons);
        let mouse = next.mouse.get_or_insert_with(|| MouseInput::new(0, speed));
        mouse.set_double_click_ticks(speed);
        let events = if down {
            mouse.button_down(button, next.ticks as u32, swap).to_vec()
        } else {
            vec![mouse.button_up(button, next.ticks as u32, swap)]
        };
        for event in events {
            use crate::mouse::Button;
            if event == PlayerEvent::DragEnd(Button::Left) || event == PlayerEvent::Action {
                next.finish_inventory_drag(resources)?;
            }
            if event == PlayerEvent::DragStart(Button::Left)
                && next.control.enabled
                && !next.conversation.hidden
                && matches!(next.active_inventory, Some(0..=2))
            {
                if let Some((frame, _)) = &next.inventory_display {
                    use crate::conversation_window::Area;
                    match frame.layout.inventory_area_at(next.control.cursor) {
                        Area::Header => {
                            next.inventory_drag = next.active_inventory;
                            next.inventory_slider = None;
                            next.inventory_resize = None;
                        }
                        Area::Thumb if matches!(next.active_inventory, Some(1 | 2)) => {
                            let window = next.declarations.windows
                                [next.active_inventory.unwrap() as usize]
                                .as_ref()
                                .ok_or(Error("missing slider window"))?;
                            let bar = frame
                                .layout
                                .scrollbar
                                .as_ref()
                                .ok_or(Error("missing slider"))?;
                            next.inventory_resize = None;
                            next.inventory_slider =
                                Some(crate::inventory_slider::Drag::new(window, bar)?);
                            next.inventory_drag = next.active_inventory;
                        }
                        area if matches!(next.active_inventory, Some(1 | 2)) => {
                            if let Some(direction) = crate::inventory_resize::Drag::direction(area)
                            {
                                let window = next.declarations.windows
                                    [next.active_inventory.unwrap() as usize]
                                    .as_ref()
                                    .ok_or(Error("missing resize window"))?;
                                if window.min[0] != window.max[0] && window.min[1] != window.max[1]
                                {
                                    next.inventory_resize =
                                        Some(crate::inventory_resize::Drag::new(
                                            resources,
                                            next.declarations
                                                .window_parts
                                                .ok_or(Error("missing window parts"))?,
                                            window,
                                            direction,
                                        )?);
                                    next.inventory_drag = next.active_inventory;
                                    next.inventory_slider = None;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            let action = match event {
                PlayerEvent::WalkTo => PolygonInteraction::WalkTo,
                PlayerEvent::Action => PolygonInteraction::Action,
                PlayerEvent::Look => PolygonInteraction::Look,
                _ => {
                    next.user_event()?;
                    continue;
                }
            };
            let point = next.control.cursor;
            if !next.control.enabled {
                next.left_event()?;
            } else if next.conversation.polygon.is_some() && !next.conversation.hidden {
                next.left_event()?;
                if let Some(topic) = next.conversation_topic_at(point) {
                    if action == PolygonInteraction::Look {
                        next.look_conversation_topic(resources, topic)?;
                    } else {
                        next.interact_conversation(resources, Some(topic))?;
                    }
                } else if action != PolygonInteraction::Action
                    && !next.inventory_contains_point(point)
                {
                    next.interact_conversation(resources, None)?;
                }
            } else if matches!(next.active_inventory, Some(1 | 2)) {
                if action == PolygonInteraction::Action
                    && next.inventory_display.as_ref().is_some_and(|(frame, _)| {
                        frame.layout.inventory_area_at(point)
                            == crate::conversation_window::Area::Header
                    })
                {
                    next.left_event()?;
                    next.toggle_inventory_maximized(resources)?;
                } else if let Some(slot) = next.inventory_click_slot_at(point, action) {
                    next.inventory_click(resources, slot, action)?;
                } else {
                    next.left_event()?;
                    if !next.inventory_contains_point(point) && action != PolygonInteraction::Action
                    {
                        next.close_inventory();
                    } else if let Some(scroll) = next.inventory_scroll_at(point) {
                        if action == PolygonInteraction::WalkTo
                            || (action == PolygonInteraction::Action
                                && matches!(
                                    scroll,
                                    crate::inventory::Scroll::Up | crate::inventory::Scroll::Down
                                ))
                        {
                            next.scroll_inventory(scroll);
                        }
                    }
                }
            } else if next.active_inventory.is_some() {
                next.left_event()?;
            } else {
                next.world_click(resources, point, action)?;
            }
        }
        *self = next;
        Ok(())
    }

    fn look_conversation_topic(
        &mut self,
        resources: &Resources<'_>,
        topic: usize,
    ) -> Result<(), Error> {
        if self.last_interaction_tick == Some(self.ticks) {
            return Ok(());
        }
        let window = self.declarations.windows[0]
            .as_ref()
            .ok_or(Error("missing conversation window"))?;
        let index = window
            .first_display
            .checked_add(topic)
            .ok_or(Error("topic index overflow"))?;
        let Some(item) = window
            .contents
            .get(index)
            .copied()
            .filter(|item| *item != 0 && *item != self.held_object)
        else {
            return Ok(());
        };
        let object = self
            .inventory_objects
            .get(&item)
            .ok_or(Error("missing topic object"))?;
        if object.script.0 == 0 {
            return Ok(());
        }
        scene::script_bytes(resources, object.script)?;
        let mut process = ScriptProcess::new(object.script);
        process.inventory_object = Some(item);
        process.event = PolygonInteraction::Look as i32;
        self.processes.spawn(Some(self.generation), process)?;
        self.inventory_event_index = index;
        self.last_interaction_tick = Some(self.ticks);
        Ok(())
    }

    /// Route the hosts' immediate primary click. This development control does
    /// not implement the original single/double-click delay and cancellation.
    pub fn primary_click(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        let point = self.control.cursor;
        if self
            .configuration
            .as_ref()
            .is_some_and(|menu| menu.page().is_some())
        {
            self.left_event()?;
        } else if self.conversation.polygon.is_some() && !self.conversation.hidden {
            if let Some(topic) = self.conversation_topic_at(point) {
                self.interact_conversation(resources, Some(topic))?;
            }
        } else if matches!(self.active_inventory, Some(1 | 2)) && self.control.enabled {
            if let Some(action) = self.inventory_scroll_at(point) {
                self.scroll_inventory(action);
            } else if let Some(slot) = self.inventory_slot_at(point) {
                self.interact_inventory(resources, slot, PolygonInteraction::WalkTo)?;
            } else if !self.inventory_contains_point(point) {
                self.close_inventory();
            }
        } else if self.control.enabled && self.active_inventory.is_none() {
            self.interact_world(resources, point, PolygonInteraction::WalkTo)?;
        } else {
            // Hidden conversation inventory still owns the click. Wake speech
            // waits instead of attempting a world action which would be rejected.
            self.left_event()?;
        }
        Ok(())
    }
    /// Logical game coordinates. V1 WAITKEY also wakes on cursor movement.
    /// Transactional movement that also advances an active configuration drag.
    pub fn move_cursor_with_configuration(&mut self, position: [i32; 2]) -> Result<(), Error> {
        let mut next = self.clone();
        let before = next.control.cursor;
        next.move_cursor(position);
        if let Some(menu) = &mut next.configuration {
            if menu.dragging() {
                let dx = next.control.cursor[0]
                    .checked_sub(before[0])
                    .ok_or(Error("menu cursor delta overflow"))?;
                let dy = next.control.cursor[1]
                    .checked_sub(before[1])
                    .ok_or(Error("menu cursor delta overflow"))?;
                menu.drag_by([dx, dy])?;
            }
        }
        *self = next;
        Ok(())
    }

    /// Logical game coordinates. V1 WAITKEY also wakes on cursor movement.
    pub fn move_cursor(&mut self, position: [i32; 2]) {
        let old = self.control.cursor;
        self.control.move_cursor(position);
        if self.control.cursor != old {
            if let Some(slot) = self.inventory_drag {
                let delta = self.control.cursor[0]
                    .checked_sub(old[0])
                    .zip(self.control.cursor[1].checked_sub(old[1]));
                if delta.is_none_or(|(x, y)| self.move_inventory_drag(slot, [x, y]).is_err()) {
                    if let Some(resize) = self.inventory_resize.take() {
                        if let Some(window) = &mut self.declarations.windows[slot as usize] {
                            resize.cancel(window);
                        }
                    }
                    // Malformed/extreme coordinates must never partly move a window.
                    self.inventory_drag = None;
                    self.inventory_slider = None;
                    self.inventory_resize = None;
                }
            }
            self.control.user_event(self.ticks);
        }
        self.update_configuration_hover();
    }

    fn update_configuration_hover(&mut self) {
        if let Some(menu) = &mut self.configuration {
            if menu.page().is_some() {
                menu.point_at(Some(self.control.cursor));
            }
        }
    }

    pub fn inventory_dragging(&self) -> bool {
        self.inventory_drag.is_some()
    }

    fn inventory_item_at(&self, point: [i32; 2]) -> Option<i32> {
        if self.conversation.hidden {
            return None;
        }
        let slot = usize::try_from(self.active_inventory?).ok()?;
        let (index, _) = self.inventory_text_cell_at(point)?;
        self.declarations
            .windows
            .get(slot)?
            .as_ref()?
            .contents
            .get(index)
            .copied()
            .filter(|id| *id != 0 && *id != -1)
    }

    fn update_inventory_hover(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        if self.inventory_dragging() {
            return Ok(());
        }
        let item = self
            .inventory_display
            .as_ref()
            .filter(|(frame, _)| {
                frame.layout.inventory_area_at(self.control.cursor)
                    == crate::conversation_window::Area::Body
            })
            .and_then(|_| self.inventory_item_at(self.control.cursor))
            .filter(|id| *id != self.held_object);
        if let Some(item) = self.inventory_hover.monitor.observe(item, false) {
            let object = self
                .inventory_objects
                .get(&item)
                .ok_or(Error("missing pointed inventory object"))?;
            if object.script.0 != 0 {
                scene::script_bytes(resources, object.script)?;
                let mut process = ScriptProcess::new(object.script);
                process.inventory_object = Some(item);
                process.event = 0;
                self.processes.spawn(Some(self.generation), process)?;
                self.inventory_event_index =
                    usize::try_from(item).map_err(|_| Error("invalid pointed inventory object"))?;
            }
        }
        Ok(())
    }

    fn update_inventory_cursor(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        if self.inventory_drag.is_some() || self.control.hidden {
            return Ok(());
        }
        let Some(cursor) = &self.cursor_animation else {
            return Ok(());
        };
        let Some(parts) = self.declarations.window_parts else {
            return Ok(());
        };
        let film = Film::read(resources, parts)?;
        // V1 window-parts reels: left/right, up/down, rising/falling diagonal.
        let scripts = [24, 25, 26, 27].map(|index| film.reels.get(index).map(|r| r.script));
        let custom = scripts
            .iter()
            .flatten()
            .any(|script| cursor.uses_script(*script));
        let target = self.active_inventory.and_then(|slot| {
            if self.conversation.hidden {
                return None;
            }
            let window = self.declarations.windows.get(slot as usize)?.as_ref()?;
            if window.min[0] == window.max[0] || window.min[1] == window.max[1] {
                return None;
            }
            use crate::conversation_window::Area;
            let area = self
                .inventory_display
                .as_ref()?
                .0
                .layout
                .inventory_area_at(self.control.cursor);
            let index = match area {
                Area::Left | Area::Right => 0,
                Area::Top | Area::Bottom => 1,
                Area::TopRight | Area::BottomLeft => 2,
                Area::TopLeft | Area::BottomRight => 3,
                _ => return None,
            };
            scripts[index]
        });
        if let Some(script) = target {
            if !cursor.uses_script(script) {
                let cursor = self.cursor_animation.as_mut().unwrap();
                cursor.set_script(script, 2)?;
                cursor.flip_x = false;
                cursor.flip_y = false;
            }
        } else if custom {
            let (_, normal) = self
                .declarations
                .cursor
                .as_ref()
                .ok_or(Error("missing normal cursor film"))?;
            let script = normal
                .reels
                .first()
                .ok_or(Error("empty normal cursor film"))?
                .script;
            let cursor = self.cursor_animation.as_mut().unwrap();
            cursor.set_script(script, 24 / normal.rate as u32)?;
            cursor.flip_x = false;
            cursor.flip_y = false;
            // RestoreMainCursor steps immediately; SetTempCursor waits until
            // the next AnimateProcess. Preserve that asymmetry.
            cursor.tick(resources)?;
        }
        Ok(())
    }

    fn toggle_inventory_maximized(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        let Some(slot @ (1 | 2)) = self.active_inventory else {
            return Ok(());
        };
        let window = self.declarations.windows[slot as usize]
            .as_mut()
            .ok_or(Error("missing inventory declaration"))?;
        if !self.inventory_geometry.toggle(slot as usize, window)? {
            return Ok(());
        }
        self.reconstruct_inventory(resources)
    }

    fn reconstruct_inventory(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        let slot = self
            .active_inventory
            .ok_or(Error("missing active inventory"))?;
        let window = self.declarations.windows[slot as usize]
            .as_mut()
            .ok_or(Error("missing inventory declaration"))?;
        crate::inventory::adjust_top(window);
        let handle = self
            .declarations
            .window_parts
            .ok_or(Error("missing window parts"))?;
        let mut frame = crate::conversation_window::Frame::inventory(resources, handle, window)?;
        let delta = frame.layout.reposition_delta()?;
        if delta != [0, 0] {
            window.position = [
                window.position[0]
                    .checked_add(delta[0])
                    .ok_or(Error("inventory reposition overflow"))?,
                window.position[1]
                    .checked_add(delta[1])
                    .ok_or(Error("inventory reposition overflow"))?,
            ];
            frame = crate::conversation_window::Frame::inventory(resources, handle, window)?;
        }
        let (old_frame, old_content) = self
            .inventory_display
            .as_ref()
            .ok_or(Error("missing inventory display"))?;
        let mut title = old_content.title.clone();
        let dx = i64::from(frame.layout.title[0]) - i64::from(old_frame.layout.title[0]);
        let dy = i64::from(frame.layout.title[1]) - i64::from(old_frame.layout.title[1]);
        for glyph in &mut title.glyphs {
            glyph.x = i32::try_from(i64::from(glyph.x) + dx)
                .map_err(|_| Error("inventory title overflow"))?;
            glyph.y = i32::try_from(i64::from(glyph.y) + dy)
                .map_err(|_| Error("inventory title overflow"))?;
        }
        let content = crate::conversation_window::Content::with_title(
            resources,
            window,
            &frame.layout,
            title,
            &self.inventory_objects,
            self.held_object,
        )?;
        self.inventory_display = Some((frame, content));
        self.inventory_dirty = false;
        Ok(())
    }

    fn finish_inventory_drag(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        if let Some(resize) = self.inventory_resize.take() {
            let slot = self.inventory_drag.ok_or(Error("missing resize owner"))?;
            let window = self.declarations.windows[slot as usize]
                .as_mut()
                .ok_or(Error("missing resize window"))?;
            resize.finish(window);
            self.reconstruct_inventory(resources)?;
            let position = self.declarations.windows[slot as usize]
                .as_ref()
                .unwrap()
                .position;
            self.inventory_geometry.resized(slot as usize, position);
        }
        if let Some(slider) = self.inventory_slider.take() {
            if let Some(slot @ (1 | 2)) = self.inventory_drag {
                if let Some(window) = &mut self.declarations.windows[slot as usize] {
                    window.first_display = slider.first_display();
                    self.inventory_dirty = true;
                }
            }
        }
        self.inventory_drag = None;
        Ok(())
    }

    fn move_inventory_drag(&mut self, slot: i32, delta: [i32; 2]) -> Result<(), Error> {
        if let Some(mut resize) = self.inventory_resize.clone() {
            let mut window = self.declarations.windows[slot as usize]
                .clone()
                .ok_or(Error("missing resize window"))?;
            resize.move_by(&mut window, delta)?;
            self.declarations.windows[slot as usize] = Some(window);
            self.inventory_resize = Some(resize);
            return Ok(());
        }
        let Some(mut slider) = self.inventory_slider.clone() else {
            return self.translate_inventory(slot, delta);
        };
        if delta[1] == 0 {
            return Ok(());
        }
        let first = slider.move_by(delta[1])?;
        let (frame, _) = self
            .inventory_display
            .as_mut()
            .ok_or(Error("missing slider display"))?;
        frame.move_slider(slider.y())?;
        if let Some(first) = first {
            self.declarations.windows[slot as usize]
                .as_mut()
                .ok_or(Error("missing slider window"))?
                .first_display = first;
            self.inventory_dirty = true;
        }
        self.inventory_slider = Some(slider);
        Ok(())
    }

    fn translate_inventory(&mut self, slot: i32, delta: [i32; 2]) -> Result<(), Error> {
        if self.active_inventory != Some(slot) || self.conversation.hidden {
            return Err(Error("inactive inventory drag"));
        }
        let window = self
            .declarations
            .windows
            .get(slot as usize)
            .and_then(Option::as_ref)
            .ok_or(Error("missing drag window"))?;
        let position = [
            window.position[0].checked_add(delta[0]),
            window.position[1].checked_add(delta[1]),
        ];
        let position = [
            position[0].ok_or(Error("inventory movement overflow"))?,
            position[1].ok_or(Error("inventory movement overflow"))?,
        ];
        let (frame, content) = self
            .inventory_display
            .as_ref()
            .ok_or(Error("missing drag display"))?;
        let display = crate::conversation_window::translated(frame, content, delta)?;
        self.declarations.windows[slot as usize]
            .as_mut()
            .unwrap()
            .position = position;
        self.inventory_display = Some(display);
        Ok(())
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
            || self.control_inventory_active()
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
            || self.control_inventory_active()
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

    /// Timed world click: actor/polygon scripts participate in V1 double-click
    /// cancellation, while bare-ground movement remains immediate. Inventory
    /// routing and raw physical button delivery are separate integration work.
    pub fn world_click(
        &mut self,
        resources: &Resources<'_>,
        point: [i32; 2],
        event: PolygonInteraction,
    ) -> Result<bool, Error> {
        let mut next = self.clone();
        next.left_event()?;
        let before: Vec<_> = next.processes.entries().map(|(id, _)| id).collect();
        let accepted = next.interact_world(resources, point, event)?;
        if accepted && event != PolygonInteraction::Look {
            let scripted = next
                .processes
                .entries()
                .find(|(id, p)| !before.contains(id) && p.player_walk.is_none())
                .map(|(id, _)| id);
            if let Some(id) = scripted {
                next.clicks.schedule(
                    id,
                    if event == PolygonInteraction::WalkTo {
                        crate::click::Phase::Single
                    } else {
                        crate::click::Phase::Double
                    },
                );
            }
        }
        *self = next;
        Ok(accepted)
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
            if self
                .movers
                .get(&(id as i32))
                .map_or(actor.hidden, |m| m.hidden)
                || !actor.alive
            {
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
            || self.control_inventory_active()
            || !actor.alive
            || self
                .movers
                .get(&(id as i32))
                .map_or(actor.hidden, |m| m.hidden)
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
        // Pointer movement precedes the click's InvTinselEvent. In particular,
        // POINTED's object-valued glitter index must not overwrite the click's
        // cell-valued index on the next tick before ADDOPENINV executes.
        next.update_inventory_hover(resources)?;
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

    /// Timed inventory gesture. Scripted pickup/drop shares the world-click
    /// token; an ordinary putdown is also delayed, as V1 InvPdProcess requires.
    pub fn inventory_click(
        &mut self,
        resources: &Resources<'_>,
        index: usize,
        event: PolygonInteraction,
    ) -> Result<bool, Error> {
        let mut next = self.clone();
        next.left_event()?;
        if !next.control.enabled {
            *self = next;
            return Ok(false);
        }
        if event == PolygonInteraction::WalkTo
            && next.held_object != -1
            && !next.conversation.hidden
            && next.last_interaction_tick != Some(next.ticks)
        {
            if let Some(slot @ (1 | 2)) = next.active_inventory {
                let object = next
                    .inventory_objects
                    .get(&next.held_object)
                    .ok_or(Error("missing held inventory object"))?;
                let window = next.declarations.windows[slot as usize]
                    .as_ref()
                    .ok_or(Error("missing drop inventory"))?;
                let scripted = object.attributes & 4 != 0 && object.script.0 != 0;
                let wrong_inventory = (object.attributes & 1 != 0 && slot != 1)
                    || (object.attributes & 2 != 0 && slot != 2);
                if !scripted
                    && !wrong_inventory
                    && index < (window.size[0] * window.size[1]) as usize
                {
                    let position = window
                        .first_display
                        .checked_add(index)
                        .ok_or(Error("drop index overflow"))?;
                    let id = next
                        .processes
                        .spawn(Some(next.generation), ScriptProcess::new(Handle(0)))?;
                    next.clicks
                        .schedule(id, crate::click::Phase::Drop(position));
                    next.last_interaction_tick = Some(next.ticks);
                    *self = next;
                    return Ok(true);
                }
            }
        }
        let before: Vec<_> = next.processes.entries().map(|(id, _)| id).collect();
        let accepted = next.interact_inventory(resources, index, event)?;
        if accepted && event != PolygonInteraction::Look {
            if let Some(id) = next
                .processes
                .entries()
                .find(|(id, _)| !before.contains(id))
                .map(|(id, _)| id)
            {
                next.clicks.schedule(
                    id,
                    if event == PolygonInteraction::WalkTo {
                        crate::click::Phase::Single
                    } else {
                        crate::click::Phase::Double
                    },
                );
            }
        }
        *self = next;
        Ok(accepted)
    }

    fn finish_inventory_drop(&mut self, index: usize) -> Result<(), Error> {
        let Some(slot @ (1 | 2)) = self.active_inventory else {
            return Ok(());
        };
        if self.held_object == -1 {
            return Ok(());
        }
        if crate::inventory::put_down(
            &mut self.declarations.windows,
            slot as usize,
            self.held_object,
            index,
        )? {
            self.held_object = -1;
            self.held_cursor = None;
            self.inventory_dirty = true;
        }
        Ok(())
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
            frame.layout.inventory_area_at(point) != crate::conversation_window::Area::Outside
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
            .inventory_area_at(point)
            .scroll()
    }

    pub fn active_inventory(&self) -> Option<i32> {
        self.active_inventory
    }

    /// Close the active window without a conversation postamble (scripted CLOSEINVENTORY).
    pub fn close_inventory(&mut self) {
        if let Some(menu) = &mut self.configuration {
            if menu.page().is_some() {
                menu.close();
                self.control.tags_enabled = true;
            }
        }
        self.top_window = None;
        if let Some(slot) = self.active_inventory.take() {
            self.inventory_geometry.close(slot as usize);
            self.control.tags_enabled = true;
        }
        self.conversation.end();
        self.inventory_display = None;
        self.inventory_drag = None;
        self.inventory_slider = None;
        self.inventory_resize = None;
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
        if self.inventory_resize.is_some() {
            // V1 redraw -> fillInInventory preserves the full frame/title while
            // using the in-progress declaration for icon origins and dimensions.
            let (mut frame, old_content) = self
                .inventory_display
                .as_ref()
                .ok_or(Error("missing resize display"))?
                .clone();
            let window = self.declarations.windows[slot]
                .as_mut()
                .ok_or(Error("missing resize window"))?;
            if let Some(bar) = &frame.layout.scrollbar {
                crate::inventory::adjust_top(window);
                let cols = window.size[0] as usize;
                let top = window.first_display.div_ceil(cols);
                let bottom = window
                    .contents
                    .len()
                    .saturating_sub(window.first_display)
                    .div_ceil(cols)
                    .saturating_sub(window.size[1] as usize);
                let [min, max] = bar.range.map(i64::from);
                let y = if top == 0 {
                    min
                } else if bottom == 0 {
                    max
                } else {
                    min + top as i64 * (max - min) / (top + bottom) as i64
                };
                frame.move_slider(
                    i32::try_from(y).map_err(|_| Error("inventory thumb overflow"))?,
                )?;
            }
            let mut icon_layout = frame.layout.clone();
            icon_layout.icons = crate::conversation_window::Layout::inventory_icons(window)?;
            let content = crate::conversation_window::Content::with_title(
                resources,
                window,
                &icon_layout,
                old_content.title,
                &self.inventory_objects,
                self.held_object,
            )?;
            self.inventory_display = Some((frame, content));
            self.inventory_dirty = false;
            return Ok(());
        }
        if slot != 0 && self.inventory_slider.is_none() {
            if let Some(window) = &mut self.declarations.windows[slot] {
                crate::inventory::adjust_top(window);
            }
        }
        let window = self.declarations.windows[slot]
            .as_mut()
            .ok_or(Error("missing conversation declaration"))?;
        let constructor = if slot == 0 {
            crate::conversation_window::Frame::new
        } else {
            crate::conversation_window::Frame::inventory
        };
        let mut frame = if self.inventory_slider.is_some() {
            self.inventory_display
                .as_ref()
                .ok_or(Error("missing slider display"))?
                .0
                .clone()
        } else {
            constructor(
                resources,
                self.declarations
                    .window_parts
                    .ok_or(Error("missing window parts"))?,
                window,
            )?
        };
        if self.inventory_display.is_none() {
            let delta = frame.layout.reposition_delta()?;
            if delta != [0, 0] {
                window.position = [
                    window.position[0]
                        .checked_add(delta[0])
                        .ok_or(Error("inventory reposition overflow"))?,
                    window.position[1]
                        .checked_add(delta[1])
                        .ok_or(Error("inventory reposition overflow"))?,
                ];
                frame = constructor(
                    resources,
                    self.declarations
                        .window_parts
                        .ok_or(Error("missing window parts"))?,
                    window,
                )?;
            }
        }
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
        self.inventory_display.as_ref()?;
        let position = if let Some(slot @ (1 | 2)) = self.active_inventory {
            let window = self.declarations.windows[slot as usize].as_ref()?;
            *crate::conversation_window::Layout::inventory_icons(window)
                .ok()?
                .get(index)?
        } else {
            *self.inventory_display.as_ref()?.0.layout.icons.get(index)?
        };
        Some([position[0].checked_add(12)?, position[1].checked_add(12)?])
    }

    /// V1 invItem uses declared geometry, even while the old frame remains
    /// during resize. It returns a backing-array cell and text anchor, including
    /// empty cells; unlike invItemId it does not test inventory visibility.
    fn inventory_text_cell_at(&self, point: [i32; 2]) -> Option<(usize, [i32; 2])> {
        let slot = usize::try_from(self.active_inventory?).ok()?;
        let window = self.declarations.windows.get(slot)?.as_ref()?;
        let icons = crate::conversation_window::Layout::inventory_icons(window).ok()?;
        let (index, [x, y]) = icons.iter().enumerate().find(|(_, [x, y])| {
            (0..25).contains(&(i64::from(point[0]) - i64::from(*x)))
                && (0..25).contains(&(i64::from(point[1]) - i64::from(*y)))
        })?;
        Some((
            window.first_display.checked_add(index)?,
            [x.checked_add(12)?, *y],
        ))
    }

    /// Visible ordinary inventory cells, including empty drop destinations.
    pub fn inventory_slot_at(&self, point: [i32; 2]) -> Option<usize> {
        if !matches!(self.active_inventory, Some(1 | 2)) || self.conversation.hidden {
            return None;
        }
        self.inventory_display.as_ref()?;
        let slot = self.active_inventory? as usize;
        let window = self.declarations.windows[slot].as_ref()?;
        crate::conversation_window::Layout::inventory_icons(window)
            .ok()?
            .iter()
            .position(|[x, y]| {
                (0..25).contains(&(i64::from(point[0]) - i64::from(*x)))
                    && (0..25).contains(&(i64::from(point[1]) - i64::from(*y)))
            })
    }

    /// invWalkTo checks the window region before icons. Only a held-item
    /// single click searches the one-pixel gaps: right, down, then down-right.
    fn inventory_click_slot_at(
        &self,
        point: [i32; 2],
        action: PolygonInteraction,
    ) -> Option<usize> {
        let (frame, _) = self.inventory_display.as_ref()?;
        if frame.layout.inventory_area_at(point) != crate::conversation_window::Area::Body {
            return None;
        }
        if let Some(slot) = self.inventory_slot_at(point) {
            return Some(slot);
        }
        if self.held_object == -1 || action != PolygonInteraction::WalkTo {
            return None;
        }
        [[1, 0], [0, 1], [1, 1]].into_iter().find_map(|[dx, dy]| {
            self.inventory_slot_at([point[0].checked_add(dx)?, point[1].checked_add(dy)?])
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
        Self::bootstrap_after_restart(resources, false, None)
    }

    fn validate_intro_title(resources: &Resources<'_>, title: Option<Handle>) -> Result<(), Error> {
        if let Some(title) = title {
            let scene = scene::read(resources, title.resource())?
                .ok_or(Error("intro title scene missing"))?;
            if !scene.entrances.iter().any(|(id, _)| *id == 1) {
                return Err(Error("intro title entrance missing"));
            }
            if scene.script.0 == 0 {
                return Err(Error("intro title script missing"));
            }
            scene::script_bytes(resources, scene.script)?;
        }
        Ok(())
    }

    pub fn bootstrap_with_title(
        resources: &Resources<'_>,
        title: Option<Handle>,
    ) -> Result<Self, Error> {
        Self::validate_intro_title(resources, title)?;
        let intro = title
            .map(|title| crate::intro::Intro::new(Some(title)))
            .transpose()?;
        Self::bootstrap_after_restart(resources, false, intro)
    }

    /// User game loads end intro mode; snapshot audit restoration must not.
    pub fn end_intro(&mut self) {
        if let Some(intro) = &mut self.intro {
            intro.end();
        }
    }

    pub fn intro_active(&self) -> bool {
        self.intro.as_ref().is_some_and(crate::intro::Intro::active)
    }

    fn bootstrap_after_restart(
        resources: &Resources<'_>,
        has_restarted: bool,
        intro: Option<crate::intro::Intro>,
    ) -> Result<Self, Error> {
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
            restart_pending: false,
            has_restarted,
            conversation: crate::conversation::Conversation::default(),
            scene_hooks: crate::scene_hook::Hooks::default(),
            inventory_display: None,
            inventory_dirty: false,
            top_window: None,
            configuration: None,
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
            clicks: crate::click::State::default(),
            mouse: None,
            inventory_drag: None,
            inventory_slider: None,
            inventory_resize: None,
            inventory_hover: Default::default(),
            inventory_geometry: crate::inventory_geometry::State::default(),
            escape_events: intro.as_ref().map(|_| 1),
            intro,
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
            reel_escape: std::collections::BTreeMap::new(),
            splay_reels: std::collections::BTreeSet::new(),
            next_playback: 0,
            polygons: crate::polygon::Polygons::default(),
            random: crate::random::Random::default(),
            background: None,
            background_film: None,
            background_palette: None,
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
            let previous_escape = (vm.escape_enabled, vm.escape_generation);
            match vm.step(bytes, &mut engine.globals)? {
                Step::Continue => {}
                Step::Escape(enabled) => {
                    if engine.intro_active() {
                        (vm.escape_enabled, vm.escape_generation) = previous_escape;
                    } else {
                        Self::capture_escape(&mut engine.escape_events, &mut vm, enabled);
                    }
                }
                Step::Halted => return Err(Error("master did not request a scene")),
                Step::Library(call) => {
                    if call.id == 120 {
                        vm.complete_library(call, 0, Some(i32::from(has_restarted)))?;
                        continue;
                    }
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
        Self::migrate_escape(&mut self.escape_events, &mut self.master);
        if let Some((call, left)) = self.master_wait {
            if left > 1 && !self.escaped(&self.master) {
                self.master_wait = Some((call, left - 1));
                return Ok(None);
            }
            self.master.complete_library(
                call,
                match call.id {
                    48 => 0,
                    106 => 1,
                    _ => 2,
                },
                None,
            )?;
            self.master_wait = None;
        }
        let bytes = scene::script_bytes(resources, self.master_code)?;
        for _ in 0..10000 {
            let previous_escape = (self.master.escape_enabled, self.master.escape_generation);
            let call = match self.master.step(bytes, &mut self.globals)? {
                Step::Continue => continue,
                Step::Escape(enabled) => {
                    if self.intro_active() {
                        (self.master.escape_enabled, self.master.escape_generation) =
                            previous_escape;
                    } else {
                        Self::capture_escape(&mut self.escape_events, &mut self.master, enabled);
                    }
                    continue;
                }
                Step::Halted => return Ok(None),
                Step::Library(c) => c,
            };
            if self.master.complete_scene_resume(call)? {
                continue;
            }
            match call.id {
                114 => {
                    self.request_restart();
                    self.master.complete_library(call, 0, None)?;
                }
                120 => {
                    self.master
                        .complete_library(call, 0, Some(i32::from(self.has_restarted)))?;
                }
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
                    let request = self.intro.as_ref().map_or(request, |intro| {
                        intro.redirect(request, self.escape_events.unwrap_or(1))
                    });
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
                69 | 70 | 84 => {
                    // Release V1 SHOWPOS/SHOWSTRING and VIBRATE have no effect.
                    self.master.complete_library(call, 0, None)?;
                }
                106 => {
                    if !matches!(args(&self.master, 1)?[0], 0 | 1) {
                        return Err(Error("invalid MIDI fade direction"));
                    }
                    // Pinned V1 FadeMidi only sleeps once; it does not fade audio.
                    self.master_wait = Some((call, 1));
                    return Ok(None);
                }
                48 => {
                    if self.escaped(&self.master) {
                        self.master.complete_library(call, 0, None)?;
                        continue;
                    }
                    self.master_wait = Some((call, 2));
                    return Ok(None);
                }
                86 => {
                    let a = args(&self.master, 2)?;
                    if self.escaped(&self.master) {
                        self.master.complete_library(call, 2, None)?;
                        continue;
                    }
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
        let actor_scripts = self
            .actors
            .enter_with_startup(&scene.actors, entry.is_some())?;
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
        self.clicks.cleanup(&self.processes);
        self.inventory_hover.cleanup(&self.processes);
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
                let mut process = ScriptProcess::new(code);
                if let Some(intro) = &mut self.intro {
                    if let Some(event) =
                        intro.scene_script(code, self.escape_events.unwrap_or(1))?
                    {
                        process.vm.escape_enabled = true;
                        process.vm.escape_generation = Some(event);
                    }
                }
                self.processes.spawn(Some(self.generation), process)?;
            }
        }
        self.background = None;
        self.background_film = None;
        self.background_palette = None;
        self.cursor_animation = if let Some((handle, _)) = &self.declarations.cursor {
            Self::start_film(resources, *handle, true)?.pop()
        } else {
            None
        };
        self.films.clear();
        self.splay_reels.clear();
        self.reel_owners.clear();
        self.reel_escape.clear();
        self.playback.clear();
        self.movers.clear();
        self.actor_effects.clear();
        self.no_blocking = false;
        // EndScene calls killInventory before CONTROL_STARTOFF. The constant
        // InventoryProcess survives scene changes, including pending effects.
        if let Some(menu) = &mut self.configuration {
            menu.close();
        }
        self.control.apply_at(4, false, self.world_origin)?;
        self.menu_enabled = false;
        if let Some(slot) = self.active_inventory {
            self.inventory_geometry.close(slot as usize);
        }
        self.active_inventory = None;
        self.top_window = None;
        self.conversation.end();
        self.inventory_display = None;
        self.inventory_drag = None;
        self.inventory_slider = None;
        self.inventory_resize = None;
        self.inventory_dirty = false;
        self.sample_player.stop();
        self.actor_steps.clear();
        Ok(())
    }
    fn start_background(&mut self, resources: &Resources<'_>, film: Handle) -> Result<(), Error> {
        let mut animations = Self::start_film(resources, film, true)?;
        self.background = animations.pop();
        self.background_palette = None;
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
    fn change_palette(&mut self, resources: &Resources<'_>, handle: Handle) -> Result<(), Error> {
        let mut palette = if let Some(palette) = &self.background_palette {
            palette.clone()
        } else {
            let image = self
                .background
                .as_ref()
                .ok_or(Error("SETPALETTE without background"))?
                .images(resources)?
                .first()
                .copied()
                .ok_or(Error("SETPALETTE without background image"))?;
            crate::palette::BackgroundPalette::new(
                resources,
                crate::graphics::Image::read(resources, image)?.palette,
            )?
        };
        palette.replace(resources, handle)?;
        self.background_palette = Some(palette);
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
                stored = if let Some(mover) = self.movers.get(&reel.object.id) {
                    [mover.animation.object.x, mover.animation.object.y]
                } else if self.declarations.movers.contains_key(&reel.object.id) {
                    // V1 GetActorPos delegates to GetMoverPosition even before
                    // actorObj exists; that case reports zero coordinates.
                    [0, 0]
                } else {
                    self.actors.get(reel.object.id as u32)?.position
                };
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
    fn migrate_legacy_talk_reels(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        let mut pending = std::collections::BTreeMap::new();
        for wait in self.processes.states().filter_map(|p| p.talk_wait.as_ref()) {
            if let Some(saved) = wait.mover_saved {
                let mover = self
                    .movers
                    .get(&wait.actor)
                    .ok_or(Error("talking mover disappeared"))?;
                if mover.reel_state.is_none()
                    && pending
                        .insert(wait.actor, saved)
                        .is_some_and(|previous| previous != saved)
                {
                    return Err(Error("ambiguous legacy talk reel slot"));
                }
            }
        }
        // Validate all recovered resources before committing any migration.
        for (&actor, &saved) in &pending {
            let mut mover = self.movers[&actor].clone();
            mover.reel_state = Some(crate::mover_reel::State::legacy_talk(
                saved,
                mover.walk_target.is_some(),
            ));
            mover.validate_reel_resources(resources)?;
        }
        for (actor, saved) in pending {
            let mover = self.movers.get_mut(&actor).unwrap();
            mover.reel_state = Some(crate::mover_reel::State::legacy_talk(
                saved,
                mover.walk_target.is_some(),
            ));
        }
        Ok(())
    }

    pub(crate) fn finished_talks(&self) -> Vec<crate::scheduler::ProcessId> {
        self.processes
            .entries()
            .filter_map(|(id, process)| {
                process
                    .talk_wait
                    .as_ref()
                    .is_some_and(|wait| wait.reel_finished)
                    .then_some(id)
            })
            .collect()
    }
    pub(crate) fn restore_finished_talks(
        &mut self,
        ids: &[crate::scheduler::ProcessId],
    ) -> Result<(), Error> {
        for id in ids {
            let wait = self
                .processes
                .state_mut(*id)
                .and_then(|process| process.talk_wait.as_mut())
                .ok_or(Error("finished talk process missing"))?;
            if wait.reel_finished
                || !wait.initialized
                || wait.sample.is_some()
                || wait.layout.is_none()
                || wait.mover_saved.is_some()
            {
                return Err(Error("invalid finished talk process"));
            }
            wait.reel_finished = true;
        }
        Ok(())
    }
    fn voice_enabled(&self) -> bool {
        self.configuration
            .as_ref()
            .is_none_or(|menu| menu.settings().volumes[2] != 0)
    }
    fn text_speed(&self) -> u16 {
        self.configuration
            .as_ref()
            .map_or(0, |menu| menu.settings().text_speed)
    }
    fn text_time(&self, length: usize) -> Result<u32, Error> {
        u32::try_from(length)
            .ok()
            .and_then(|length| length.checked_add(24 + u32::from(self.text_speed()) * 120 / 100))
            .ok_or(Error("text time overflow"))
    }
    fn finish_talk_reel(
        &mut self,
        resources: &Resources<'_>,
        wait: &mut TalkWait,
    ) -> Result<(), Error> {
        if wait.reel_finished {
            return Ok(());
        }
        if let Some(saved) = wait.mover_saved.take() {
            self.movers
                .get_mut(&wait.actor)
                .ok_or(Error("talking mover disappeared"))?
                .pop_talk(resources, &self.declarations, saved)?;
        } else if let Some(film) = self.playback.finish_talk(wait.actor) {
            self.launch_film(resources, film)?;
        } else {
            return Err(Error("talk actor has no normal film to restore"));
        }
        wait.reel_finished = true;
        Ok(())
    }
    fn advance_talk(
        &mut self,
        resources: &Resources<'_>,
        wait: &mut TalkWait,
        escaped: bool,
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
        let cancelled = escaped || wait.timeout == 0 || wait.left_events != self.left_events;
        let complete = if let Some(serial) = wait.sample {
            let ended = self.sample_player.serial != serial || self.sample_player.id.is_none();
            if ended && !cancelled && wait.layout.is_some() && self.text_speed() != 0 {
                self.finish_talk_reel(resources, wait)?;
                wait.sample = None;
                wait.remaining = wait.remaining.saturating_sub(1);
                false
            } else {
                wait.remaining = wait.remaining.saturating_sub(1);
                ended
            }
        } else {
            let done = wait.remaining == 0;
            wait.remaining = wait.remaining.saturating_sub(1);
            done
        };
        if !complete && !cancelled {
            return Ok(false);
        }
        if wait.sample == Some(self.sample_player.serial) {
            self.sample_player.stop();
        }
        self.finish_talk_reel(resources, wait)?;
        if wait.restore_control {
            self.control
                .apply_at(1, self.control_inventory_active(), self.world_origin)?;
        }
        if wait.restore_tags {
            self.control.tags_enabled = true;
        }
        Ok(true)
    }
    fn capture_reel_escape(&mut self, actor: i32, captured: Option<u64>) {
        self.reel_escape.remove(&actor);
        if let (Some((_, playback)), Some(captured)) = (self.reel_owners.get(&actor), captured) {
            self.reel_escape.insert(actor, (*playback, captured));
        }
    }

    fn reel_escaped(&self, animation: &Animation) -> bool {
        self.reel_escape
            .get(&animation.object.id)
            .is_some_and(|&(owner, captured)| {
                owner == animation.playback && Some(captured) != self.escape_events
            })
    }

    fn owns_reel(&self, animation: &Animation) -> bool {
        animation.reel.is_some_and(|reel| {
            self.reel_owners.get(&animation.object.id) == Some(&(reel, animation.playback))
        })
    }
    fn release_reel(&mut self, animation: &Animation) {
        if self.owns_reel(animation) {
            self.reel_owners.remove(&animation.object.id);
            self.reel_escape.remove(&animation.object.id);
            if let Some(mover) = self.movers.get_mut(&animation.object.id) {
                mover.hidden = false;
            }
        }
    }
    fn finish_reel(
        &mut self,
        resources: &Resources<'_>,
        animation: &Animation,
    ) -> Result<(), Error> {
        self.release_reel(animation);
        if self.splay_reels.remove(&animation.playback) {
            let actor = animation.object.id;
            let position = self.actors.get(actor as u32)?.position;
            let cursor =
                std::array::from_fn(|i| self.control.cursor[i].wrapping_add(self.world_origin[i]));
            if let Some(mover) = self.movers.get_mut(&actor) {
                mover.restore_after_splay(
                    resources,
                    &self.declarations,
                    &self.polygons,
                    position,
                    cursor,
                )?;
                if !mover.hidden {
                    self.reel_owners.remove(&actor);
                    self.reel_escape.remove(&actor);
                }
            }
        }
        Ok(())
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
        self.reel_escape.remove(&actor);
        self.films
            .retain(|old| old.object.id != actor || self.splay_reels.contains(&old.playback));
        if let Some(mover) = self.movers.get_mut(&actor) {
            mover.hidden = true;
        }
        Ok(true)
    }
    fn disable_actor(&mut self, id: u32) -> Result<(), Error> {
        self.actors.disable(id)?;
        self.movers.remove(&(id as i32));
        self.reel_owners.remove(&(id as i32));
        self.reel_escape.remove(&(id as i32));
        for animation in self.films.iter().chain(
            self.processes
                .states()
                .filter_map(|p| p.play_wait.as_ref().map(|w| &w.1)),
        ) {
            if animation.object.id == id as i32 {
                self.splay_reels.remove(&animation.playback);
            }
        }
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
        self.tick_content_impl(resources, bank, tracks, strings, None)
    }

    /// Complete a game cycle including InventoryProcess. The returned request
    /// belongs to this cycle and must be consumed by the host before another.
    /// Restart is applied at cycle end; its returned request is a notification,
    /// not an instruction for the host to restart the engine a second time.
    pub fn tick_with_configuration(
        &mut self,
        resources: &Resources<'_>,
        bank: Option<&SampleBank<'_>>,
        tracks: Option<&[crate::music::Track<'_>]>,
        strings: &crate::text::Strings<'_>,
        saves: &[crate::save_menu::Entry],
    ) -> Result<(Status, crate::menu_controller::Dispatch), Error> {
        let mut dispatch = crate::menu_controller::Dispatch {
            outcome: crate::menu_navigation::Outcome::default(),
            selection: None,
        };
        let status = self.tick_content_impl(
            resources,
            bank,
            tracks,
            Some(strings),
            Some((saves, &mut dispatch)),
        )?;
        Ok((status, dispatch))
    }

    fn tick_content_impl(
        &mut self,
        resources: &Resources<'_>,
        bank: Option<&SampleBank<'_>>,
        tracks: Option<&[crate::music::Track<'_>]>,
        strings: Option<&crate::text::Strings<'_>>,
        menu: Option<(
            &[crate::save_menu::Entry],
            &mut crate::menu_controller::Dispatch,
        )>,
    ) -> Result<Status, Error> {
        let mut next = self.clone();
        let mut status = next.advance(resources, bank, tracks, strings, menu)?;
        if next.restart_pending && !matches!(status, Status::Unsupported(_)) {
            // Pinned V1 resets after the game cycle, outside script dispatch.
            // Keep host-facing time, settings and the RNG stream continuous.
            let mut intro = next.intro.clone();
            if let Some(policy) = &mut intro {
                policy.reset();
            }
            let mut fresh = Self::bootstrap_after_restart(resources, true, intro)?;
            fresh.ticks = next.ticks;
            fresh.configuration = next
                .configuration
                .as_ref()
                .map(|menu| crate::menu_controller::Controller::new(menu.settings()))
                .transpose()?;
            if next.mouse.is_some() {
                fresh.mouse = Some(crate::mouse::MouseInput::new(
                    next.ticks as u32,
                    fresh
                        .configuration
                        .as_ref()
                        .map_or(crate::mouse::DEFAULT_DOUBLE_CLICK_TICKS, |menu| {
                            u32::from(menu.settings().double_click)
                        }),
                ));
            }
            fresh.subtitles = next.subtitles;
            fresh.random = next.random;
            next.music.stop();
            fresh.music = next.music;
            fresh.music_frame = next.music_frame;
            // PCM and MIDI share the host clock. Keep this cycle's PCM and the
            // fractional sample accumulator while stopping the old voice.
            next.sample_player.stop();
            fresh.sample_player = next.sample_player;
            fresh.audio_frame = next.audio_frame;
            fresh.scene_calls = next.scene_calls;
            next = fresh;
            status = Status::Running;
        }
        *self = next;
        Ok(status)
    }
    fn request_restart(&mut self) {
        self.music.stop();
        self.sample_player.stop();
        self.restart_pending = true;
    }
    fn advance(
        &mut self,
        resources: &Resources<'_>,
        bank: Option<&SampleBank<'_>>,
        tracks: Option<&[crate::music::Track<'_>]>,
        strings: Option<&crate::text::Strings<'_>>,
        menu: Option<(
            &[crate::save_menu::Entry],
            &mut crate::menu_controller::Dispatch,
        )>,
    ) -> Result<Status, Error> {
        self.ticks = self.ticks.checked_add(1).ok_or(Error("tick overflow"))?;
        self.advance_scene_restore(resources, tracks)?;
        self.advance_transition(resources)?;
        self.music_frame = self.music.tick()?;
        // Older snapshots have no channel marker. Their live speech waits
        // identify the owner without changing the stored legacy representation.
        if self.sample_player.id.is_some() && !self.sample_player.is_speech() {
            let serial = self.sample_player.serial;
            if self.processes.states().any(|process| {
                process
                    .talk_wait
                    .as_ref()
                    .is_some_and(|wait| wait.sample == Some(serial))
                    || process
                        .print_wait
                        .as_ref()
                        .is_some_and(|wait| wait.sample_serial == Some(serial))
            }) {
                self.sample_player.restore_speech()?;
            }
        }
        let sample_volume = self.configuration.as_ref().map_or(255, |menu| {
            menu.settings().volumes[if self.sample_player.is_speech() { 2 } else { 1 }]
        });
        self.audio_frame = if let Some(bank) = bank {
            self.sample_player.tick_with_volume(bank, sample_volume)?
        } else {
            Vec::new()
        };
        // CreateConstProcesses creates master, cursor, then inventory. Idle
        // insertion is at the head, so inventory precedes both other processes.
        if let Some((saves, dispatch)) = menu {
            *dispatch = self.advance_configuration(
                resources,
                strings.ok_or(Error("menu update requires strings"))?,
                saves,
            )?;
            if dispatch.outcome.request == Some(crate::menu_navigation::Request::Restart) {
                self.request_restart();
            }
        }
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
        self.update_inventory_cursor(resources)?;
        self.update_configuration_hover();
        self.update_inventory_hover(resources)?;
        if let Some(bg) = &mut self.background {
            if bg.tick(resources)? == Tick::Finished {
                return Err(Error("background animation ended"));
            }
        }
        let mut active = Vec::new();
        for mut a in std::mem::take(&mut self.films) {
            if !self.owns_reel(&a) {
                self.finish_reel(resources, &a)?;
                continue;
            }
            if self.reel_escaped(&a) {
                self.finish_reel(resources, &a)?;
                continue;
            }
            let state = a.tick(resources)?;
            self.actor_steps.insert(a.object.id, a.steps);
            if state != Tick::Finished {
                self.actors.get_mut(a.object.id as u32)?.position = [a.object.x, a.object.y];
                active.push(a);
            } else {
                self.finish_reel(resources, &a)?;
            }
        }
        self.films = active;
        // Recover old stationary TALK slots at the first tick, not while
        // loading: importing and re-exporting legacy snapshots stays exact.
        self.migrate_legacy_talk_reels(resources)?;
        let mover_ids: Vec<_> = self.movers.keys().copied().collect();
        for actor in mover_ids {
            if self.reel_owners.contains_key(&actor) {
                // Older snapshots predate independent mover visibility. PLAY
                // hides its mover but does not suspend ordinary walking.
                self.movers.get_mut(&actor).unwrap().hidden = true;
            }
            let special = self.movers[&actor].special_reel();
            if special && self.movers[&actor].hidden {
                continue;
            }
            if self.movers[&actor].walk_target.is_some() && !special {
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
                // SetMoverWalkReel clears PLAY ownership when a new walking
                // reel reveals the mover. Otherwise keep the film's stored
                // actor position distinct from the underlying moving object.
                if !mover.hidden {
                    self.reel_owners.remove(&actor);
                    self.reel_escape.remove(&actor);
                }
                let pending_restore = self
                    .films
                    .iter()
                    .chain(
                        self.processes
                            .states()
                            .filter_map(|p| p.play_wait.as_ref().map(|w| &w.1)),
                    )
                    .any(|a| a.object.id == actor && self.splay_reels.contains(&a.playback));
                if !self.reel_owners.contains_key(&actor) && !pending_restore {
                    self.actors.get_mut(actor as u32)?.position = mover.placement.position;
                }
            } else if special || self.movers[&actor].reel_state.is_none() {
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
                if !self.reel_owners.contains_key(&self.scroll.focus) && !mover.hidden {
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
            let double_click_ticks = self
                .configuration
                .as_ref()
                .map_or(crate::mouse::DEFAULT_DOUBLE_CLICK_TICKS, |menu| {
                    u32::from(menu.settings().double_click)
                });
            match self
                .clicks
                .prepare(&mut self.processes, id, double_click_ticks)?
            {
                crate::click::Dispatch::Sleep(delay) => {
                    self.processes.finish(Some(delay))?;
                    continue;
                }
                crate::click::Dispatch::Resumed => {
                    if self.last_interaction_tick == Some(self.ticks) {
                        self.processes.finish(None)?;
                        continue;
                    }
                    self.last_interaction_tick = Some(self.ticks);
                }
                crate::click::Dispatch::Drop(index) => {
                    self.finish_inventory_drop(index)?;
                    self.processes.finish(None)?;
                    continue;
                }
                crate::click::Dispatch::Ready => {}
            }
            let mut process = self
                .processes
                .state_mut(id)
                .ok_or(Error("missing process"))?
                .clone();
            let mut result = self.run_process(resources, bank, tracks, strings, &mut process)?;
            if result == Status::Halted && process.event == 0 {
                if let Some(item) = process.inventory_object {
                    let wait = self.inventory_hover.monitor.completed_pointed(item)?;
                    self.inventory_hover
                        .waits
                        .push((id, crate::inventory_hover::Wait::Leave(wait)));
                    result = Status::Running;
                }
            }
            *self
                .processes
                .state_mut(id)
                .ok_or(Error("missing active process"))? = process;
            self.processes.finish(if result == Status::Halted {
                None
            } else {
                Some(1)
            })?;
            self.clicks.cleanup(&self.processes);
            self.inventory_hover.cleanup(&self.processes);
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

    /// V1 acquiring an actor token terminates its previous blocking walk.
    /// WALKED releases the token before waiting and is not an owner here.
    fn acquire_walk_token(&mut self, actor: i32) -> Result<(), Error> {
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
        self.clicks.cleanup(&self.processes);
        self.inventory_hover.cleanup(&self.processes);
        Ok(())
    }

    fn stand_mover(
        &mut self,
        resources: &Resources<'_>,
        actor: i32,
        position: [i32; 2],
        film: Handle,
    ) -> Result<(), Error> {
        let id = u32::try_from(actor).map_err(|_| Error("invalid mover actor"))?;
        self.actors.get(id)?;
        if let Some(mover) = self.movers.get_mut(&actor) {
            mover.stand(
                resources,
                &self.declarations,
                &self.polygons,
                position,
                film,
            )?;
        } else {
            let placement = crate::mover::Placement::initial(
                &self.declarations,
                &self.polygons,
                actor,
                position,
                film,
            )?;
            self.movers
                .insert(actor, crate::mover::Mover::new(resources, placement)?);
        }
        self.actors.get_mut(id)?.position = self.movers[&actor].placement.position;
        Ok(())
    }
    fn escape_stand_tag(
        &mut self,
        resources: &Resources<'_>,
        actor: i32,
        polygon: Option<usize>,
    ) -> Result<(), Error> {
        let tag = &self
            .polygons
            .active
            .get(polygon.ok_or(Error("WALKTAG requires polygon context"))?)
            .ok_or(Error("invalid WALKTAG polygon"))?
            .definition;
        let film = if self
            .declarations
            .lead
            .as_ref()
            .is_some_and(|lead| lead.id == actor)
        {
            tag.film
        } else {
            Handle(0)
        };
        self.stand_mover(resources, actor, tag.node_position, film)
    }

    fn run_process(
        &mut self,
        resources: &Resources<'_>,
        bank: Option<&SampleBank<'_>>,
        tracks: Option<&[crate::music::Track<'_>]>,
        strings: Option<&crate::text::Strings<'_>>,
        process: &mut ScriptProcess,
    ) -> Result<Status, Error> {
        Self::migrate_escape(&mut self.escape_events, &mut process.vm);
        if let Some(index) = self
            .inventory_hover
            .waits
            .iter()
            .position(|(id, _)| Some(*id) == self.processes.current_id())
        {
            use crate::inventory_hover::{LabelAction, LeaveAction, Wait};
            let item = self.inventory_item_at(self.control.cursor);
            let cell = self
                .inventory_text_cell_at(self.control.cursor)
                .map(|(cell, _)| cell);
            let held = (self.held_object != -1).then_some(self.held_object);
            let (_, wait) = &mut self.inventory_hover.waits[index];
            match wait {
                Wait::Leave(wait) => match wait.wake(&self.inventory_hover.monitor, item) {
                    LeaveAction::Wait => return Ok(Status::Running),
                    LeaveAction::Discard => {
                        self.inventory_hover.waits.remove(index);
                        process.event = 8;
                        return Ok(Status::Halted);
                    }
                    LeaveAction::Unpoint => {
                        let mut unpoint = ScriptProcess::new(process.code);
                        unpoint.inventory_object = process.inventory_object;
                        unpoint.event = 8;
                        *process = unpoint;
                        self.inventory_hover.waits.remove(index);
                    }
                },
                Wait::Label {
                    label,
                    call,
                    visible,
                    ..
                } => match label.wake(&self.inventory_hover.monitor, item, cell, held) {
                    LabelAction::Finish => {
                        process.vm.complete_library(*call, 1, None)?;
                        self.inventory_hover.waits.remove(index);
                    }
                    action => {
                        *visible = matches!(action, LabelAction::Keep | LabelAction::Redisplay);
                        return Ok(Status::Running);
                    }
                },
            }
        }
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
                self.reel_escape.remove(&actor);
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
                self.inventory_drag = None;
                self.inventory_slider = None;
                self.inventory_resize = None;
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
                // The delayed call retains its original film argument on the
                // VM stack, including across legacy snapshot restoration.
                let film = Handle(args(&process.vm, 4)?[3] as u32);
                let target = destination.unwrap_or(mover.placement.position);
                // V1 SetActorDest increments walkNumber but returns zero for
                // GotThere's early completion, before node projection. WALKED
                // compares that returned number and must report false here.
                let early = (i64::from(target[0]) - i64::from(mover.placement.position[0])).abs()
                    < 4
                    && (i64::from(target[1]) - i64::from(mover.placement.position[1])).abs() < 2;
                let serial = mover.begin_walk_film(
                    resources,
                    &self.declarations,
                    &self.polygons,
                    target,
                    cursor,
                    film,
                )?;
                self.acquire_walk_token(wait.actor)?;
                wait.serial = Some(if early { 0 } else { serial });
                wait.delay = 0;
                self.scroll.follow_cursor = false;
                self.reel_owners.remove(&wait.actor);
                self.reel_escape.remove(&wait.actor);
                self.films.retain(|a| a.object.id != wait.actor);
            }
            let mover = self
                .movers
                .get(&wait.actor)
                .ok_or(Error("WALKED mover disappeared"))?;
            if wait.serial == Some(mover.walk_serial) && mover.walk_target.is_some() {
                if self.escaped(&process.vm) {
                    self.stand_mover(resources, wait.actor, wait.target, Handle(0))?;
                    process.vm.complete_library(wait.call, 4, Some(1))?;
                } else {
                    process.walked_wait = Some(wait);
                    return Ok(Status::Running);
                }
            } else {
                let reached = wait.serial == Some(mover.walk_serial)
                    && mover.placement.position == wait.target;
                process
                    .vm
                    .complete_library(wait.call, 4, Some(i32::from(reached)))?;
            }
        }
        if let Some((call, actor, serial, restore_control)) = process.walk_wait {
            let Some(mover) = self.movers.get(&actor) else {
                return Ok(Status::Halted);
            };
            if mover.walk_serial != serial {
                return Ok(Status::Halted);
            }
            if self.escaped(&process.vm) {
                let a = args(&process.vm, if call.id == 87 { 5 } else { 6 })?;
                let target = if call.id == 87 {
                    [a[1], a[2]]
                } else {
                    [a[3], a[4]]
                };
                self.stand_mover(resources, actor, target, Handle(0))?;
            } else if self.movers[&actor].walk_target.is_some() {
                return Ok(Status::Running);
            }
            if restore_control {
                self.control
                    .apply_at(1, self.control_inventory_active(), self.world_origin)?;
            }
            process
                .vm
                .complete_library(call, if call.id == 87 { 5 } else { 6 }, None)?;
            process.walk_wait = None;
        }
        if let Some((call, actor, serial, film)) = process.tag_walk_wait {
            let escaped = self.escaped(&process.vm);
            let Some(mover) = self.movers.get_mut(&actor) else {
                return Ok(Status::Halted);
            };
            if call.id == 90 {
                let polygon = process
                    .polygon
                    .and_then(|index| self.polygons.active.get(index))
                    .ok_or(Error("WALKPOLY requires valid polygon context"))?;
                // V1 checks Escape after the yielded tick, before rechecking
                // polygon entry. It never adopts the tag pose on normal return.
                if escaped {
                    self.escape_stand_tag(resources, actor, process.polygon)?;
                } else if mover.walk_target.is_some()
                    && !polygon.contains([mover.animation.object.x, mover.animation.object.y])
                {
                    return Ok(Status::Running);
                }
            } else if mover.walk_serial != serial {
                return Ok(Status::Halted);
            } else if mover.walk_target.is_some() {
                if escaped {
                    self.escape_stand_tag(resources, actor, process.polygon)?;
                } else {
                    return Ok(Status::Running);
                }
            } else {
                mover.adopt_tag_pose(resources, &self.declarations, film)?;
            }
            process.vm.complete_library(call, 3, None)?;
            process.tag_walk_wait = None;
        }
        let talk_escaped = self.escaped(&process.vm);
        if let Some(wait) = &mut process.talk_wait {
            if !self.advance_talk(resources, wait, talk_escaped)? {
                return Ok(Status::Running);
            }
            process.vm.complete_library(wait.call, 2, None)?;
            process.talk_wait = None;
        }
        if let Some((call, serial, target)) = process.scroll_wait {
            // V1 applies escaped Offset before checking supersession.
            if self.escaped(&process.vm) {
                self.scroll.stop();
                self.world_origin = target;
            } else if serial != self.scroll.serial {
                return Ok(Status::Halted);
            }
            if self.world_origin != target {
                return Ok(Status::Running);
            }
            process.vm.complete_library(call, 4, None)?;
            process.scroll_wait = None;
        }
        if let Some((call, animation)) = &mut process.play_wait {
            let state = if self.owns_reel(animation) && !self.reel_escaped(animation) {
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
            process
                .vm
                .complete_library(*call, if call.id == 71 { 7 } else { 6 }, None)?;
            self.finish_reel(resources, animation)?;
            process.play_wait = None;
        }
        let print_escaped = self.escaped(&process.vm);
        if let Some(wait) = &mut process.print_wait {
            let cancelled = (matches!(wait.call.id, 53 | 98 | 104) && print_escaped)
                || ((wait.inventory || wait.left_events != 0)
                    && wait.left_events != self.left_events)
                || (wait.inventory && self.conversation.hidden);
            let finished = if let Some(serial) = wait.sample_serial {
                wait.timeout = wait.timeout.saturating_sub(1);
                let ended = self.sample_player.serial != serial || self.sample_player.id.is_none();
                // PRINT's text duration starts counting only after speech. TALKAT
                // and inventory text count down concurrently with the sample.
                if wait.call.id != 53 {
                    wait.remaining = wait.remaining.saturating_sub(1);
                }
                if ended && wait.timeout != 0 && wait.layout.is_some() && self.text_speed() != 0 {
                    wait.sample_serial = None;
                    false
                } else {
                    ended || wait.timeout == 0
                }
            } else {
                if wait.call.id != 53 {
                    wait.timeout = wait.timeout.saturating_sub(1);
                }
                let done = wait.remaining == 0;
                wait.remaining = wait.remaining.saturating_sub(1);
                done || (wait.call.id != 53 && wait.timeout == 0)
            };
            if !cancelled && !finished {
                return Ok(Status::Running);
            }
            if wait.sample_serial == Some(self.sample_player.serial) {
                self.sample_player.stop();
            }
            if wait.restore_control {
                self.control
                    .apply_at(1, self.control_inventory_active(), self.world_origin)?;
            }
            process
                .vm
                .complete_library(wait.call, wait.consumed, None)?;
            if wait.inventory {
                self.inventory_hover.monitor.allow();
            }
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
            let owns = process.sample_serial == Some(self.sample_player.serial);
            let active =
                self.sample_player.id.is_some() && (owns || process.sample_serial.is_none());
            if active && !self.escaped(&process.vm) {
                return Ok(Status::Running);
            }
            if active && owns {
                self.sample_player.stop();
            }
            // Legacy saves lack the original handle. Preserve their ordinary
            // channel wait, but never stop an unproven owner on escape.
            process.vm.complete_library(call, 2, None)?;
            process.sample_wait = None;
            process.sample_serial = None;
        }
        if let Some((call, left)) = process.wait {
            if left > 1 && !(matches!(call.id, 48 | 86) && self.escaped(&process.vm)) {
                process.wait = Some((call, left - 1));
                return Ok(Status::Running);
            }
            process.vm.complete_library(
                call,
                match call.id {
                    48 => 0,
                    106 => 1,
                    _ => 2,
                },
                None,
            )?;
            process.wait = None;
        }
        let bytes = scene::script_bytes(resources, process.code)?;
        for _ in 0..10000 {
            let previous_escape = (process.vm.escape_enabled, process.vm.escape_generation);
            let call = match process.vm.step(bytes, &mut self.globals)? {
                Step::Continue => continue,
                Step::Escape(enabled) => {
                    if self.intro_active() {
                        (process.vm.escape_enabled, process.vm.escape_generation) = previous_escape;
                    } else {
                        Self::capture_escape(&mut self.escape_events, &mut process.vm, enabled);
                    }
                    continue;
                }
                Step::Halted => {
                    if process.conversation_started {
                        if process.conversation_took_control {
                            self.control.apply_at(
                                1,
                                self.control_inventory_active(),
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
                34 => {
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
                    let id = u32::try_from(actor).map_err(|_| Error("invalid hidden actor"))?;
                    self.actors.get(id)?;
                    if let Some(mover) = self.movers.get_mut(&actor) {
                        mover.hidden = true;
                    } else if !self.declarations.movers.contains_key(&actor) {
                        self.actors.get_mut(id)?.hidden = true;
                    }
                    // A declared but uncreated V1 mover has no drawable object;
                    // creation resets its hidden flag before any movement.
                    process.vm.complete_library(call, 1, None)?;
                }
                114 => {
                    self.request_restart();
                    process.vm.complete_library(call, 0, None)?;
                }
                120 => {
                    process
                        .vm
                        .complete_library(call, 0, Some(i32::from(self.has_restarted)))?;
                }
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
                    if self.escaped(&process.vm) {
                        process.vm.complete_library(call, 1, None)?;
                        continue;
                    }
                    if !(1..=2).contains(&slot) {
                        return Err(Error("invalid inventory slot"));
                    }
                    if self.active_inventory.is_none() && self.top_window.is_none() {
                        self.active_inventory = Some(slot);
                        self.inventory_geometry.open(slot as usize);
                        self.inventory_drag = None;
                        self.inventory_slider = None;
                        self.inventory_resize = None;
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
                    if matches!(mode, 0 | 1 | 3) && self.escaped(&process.vm) {
                        process.vm.complete_library(call, 1, None)?;
                        continue;
                    }
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
                            self.inventory_geometry.open(0);
                            self.inventory_drag = None;
                            self.inventory_slider = None;
                            self.inventory_resize = None;
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
                36 => {
                    let a = args(&process.vm, 2)?;
                    let film = Handle(a[1] as u32);
                    if !self.inventory_objects.contains_key(&a[0]) {
                        return Err(Error("undefined inventory depiction object"));
                    }
                    crate::film::Film::read(resources, film)?;
                    // V1 Dialogs::setObjectFilm changes the persistent definition.
                    // A held item keeps its current cursor until it is held again.
                    self.inventory_objects.get_mut(&a[0]).unwrap().film = film;
                    self.inventory_dirty |= self.held_object != a[0];
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
                    self.inventory_geometry.windows[a[0] as usize].maximized = false;
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
                78 => {
                    let a = args(&process.vm, 3)?;
                    if !self.escaped(&process.vm) {
                        // Match V1's upper clamp and TINSEL_RGB casts, including
                        // the historical 16-bit green component conversion.
                        let packed = u32::from(a[0].min(255) as u8)
                            | (u32::from(a[1].min(255) as u16) << 8)
                            | (u32::from(a[2].min(255) as u8) << 16);
                        self.talk_color =
                            Some([packed as u8, (packed >> 8) as u8, (packed >> 16) as u8]);
                    }
                    process.vm.complete_library(call, 3, None)?;
                }
                77 => {
                    if self.escaped(&process.vm) {
                        args(&process.vm, 2)?;
                        process.vm.complete_library(call, 2, None)?;
                        continue;
                    }
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
                    let sample = if self.voice_enabled() && bank.sample(text_id)?.is_some() {
                        self.sample_player.play_speech(bank, text_id)?;
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
                        reel_finished: false,
                        mover_saved,
                        call,
                        actor,
                        remaining: self.text_time(text.len())?,
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
                    if self.escaped(&process.vm) {
                        self.scroll.supersede()?;
                        self.scroll.stop();
                        self.world_origin = target;
                        process.vm.complete_library(call, 4, None)?;
                        continue;
                    }
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
                17 | 18 => {
                    // DW1 CursorPos queries absolute world coordinates, even
                    // while CONTROL has hidden or frozen the cursor.
                    let axis = (call.id - 17) as usize;
                    let position = self.control.cursor[axis].wrapping_add(self.world_origin[axis]);
                    process.vm.complete_library(call, 1, Some(position))?;
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
                    let actor = if a[0] == -2 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead actor"))?
                            .id
                    } else {
                        a[0]
                    };
                    if self.escaped(&process.vm) {
                        self.stand_mover(resources, actor, [a[1], a[2]], Handle(0))?;
                        process.vm.complete_library(call, 4, Some(1))?;
                        continue;
                    }
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
                    let actor = if a[0] == -2 {
                        self.declarations
                            .lead
                            .as_ref()
                            .ok_or(Error("missing lead actor"))?
                            .id
                    } else {
                        a[0]
                    };
                    if self.escaped(&process.vm) {
                        self.stand_mover(resources, actor, [a[1], a[2]], Handle(0))?;
                        process.vm.complete_library(call, 5, None)?;
                        continue;
                    }
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
                    let serial = mover.begin_walk_film(
                        resources,
                        &self.declarations,
                        &self.polygons,
                        destination.unwrap_or(mover.placement.position),
                        cursor,
                        Handle(a[3] as u32),
                    )?;
                    let moving = mover.walk_target.is_some();
                    self.acquire_walk_token(actor)?;
                    self.scroll.follow_cursor = false;
                    self.reel_owners.remove(&actor);
                    self.reel_escape.remove(&actor);
                    self.films.retain(|a| a.object.id != actor);
                    if moving && a[4] != 2 {
                        process.walk_wait = Some((call, actor, serial, false));
                        return Ok(Status::Running);
                    }
                    process.vm.complete_library(call, 5, None)?;
                }
                90 | 91 => {
                    // Both V1 calls consume three operands, including the
                    // otherwise unused third WALKPOLY operand.
                    let a = args(&process.vm, 3)?;
                    let Some(polygon) = process.polygon else {
                        return Err(Error("WALKTAG requires polygon context"));
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
                    if self.escaped(&process.vm) {
                        self.escape_stand_tag(resources, actor, Some(polygon))?;
                        process.vm.complete_library(call, 3, None)?;
                        continue;
                    }
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
                    let serial = mover.begin_walk_film(
                        resources,
                        &self.declarations,
                        &self.polygons,
                        destination.unwrap_or(mover.placement.position),
                        cursor,
                        Handle(a[1] as u32),
                    )?;
                    let moving = mover.walk_target.is_some();
                    self.scroll.follow_cursor = true;
                    let entered = call.id == 90
                        && self.polygons.active[polygon]
                            .contains([mover.animation.object.x, mover.animation.object.y]);
                    if !moving && call.id == 91 {
                        mover.adopt_tag_pose(resources, &self.declarations, film)?;
                    }
                    self.acquire_walk_token(actor)?;
                    self.reel_owners.remove(&actor);
                    self.reel_escape.remove(&actor);
                    self.films.retain(|a| a.object.id != actor);
                    if moving && !entered {
                        process.tag_walk_wait = Some((call, actor, serial, film));
                        return Ok(Status::Running);
                    }
                    process.vm.complete_library(call, 3, None)?;
                }
                75 => {
                    let a = args(&process.vm, 6)?;
                    if self.escaped(&process.vm) {
                        process.vm.complete_library(call, 6, None)?;
                        continue;
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
                    if !self.declarations.movers.contains_key(&actor) {
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
                    if placement.path.is_none() {
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
                    if a[5] != 0 {
                        mover.alter_reel(
                            resources,
                            &self.declarations,
                            crate::mover_reel::State::default(),
                            crate::mover_reel::Operation::Walk(Handle(a[5] as u32)),
                        )?;
                    }
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
                    self.acquire_walk_token(actor)?;
                    self.scroll.follow_cursor = false;
                    self.actors.get_mut(actor as u32)?.position = position;
                    self.reel_owners.remove(&actor);
                    self.reel_escape.remove(&actor);
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
                        self.control.apply_at(
                            1,
                            self.control_inventory_active(),
                            self.world_origin,
                        )?;
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
                    self.acquire_walk_token(actor)?;
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
                    self.stand_mover(resources, actor, [a[1], a[2]], Handle(a[3] as u32))?;
                    process.vm.complete_library(call, consumed, None)?;
                }
                38 => {
                    let actor = args(&process.vm, 1)?[0];
                    self.disable_actor(
                        u32::try_from(actor).map_err(|_| Error("invalid actor ID"))?,
                    )?;
                    process.vm.complete_library(call, 1, None)?;
                }
                93 => {
                    self.actors.set_actors_on();
                    process.vm.complete_library(call, 0, None)?;
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
                    if text == -1 || text == -2 {
                        if text == -1 {
                            self.inventory_hover.monitor.suppress();
                        } else {
                            self.inventory_hover.monitor.allow();
                        }
                        process.vm.complete_library(call, 1, None)?;
                        continue;
                    }
                    if text < 0 {
                        return Err(Error("invalid inventory text"));
                    }
                    if process.event == 0 {
                        let Some((cell, position)) =
                            self.inventory_text_cell_at(self.control.cursor)
                        else {
                            process.vm.complete_library(call, 1, None)?;
                            continue;
                        };
                        let item = process.inventory_object.unwrap();
                        let mut label =
                            self.inventory_hover.monitor.begin_pointed_print(item, cell);
                        if self.escaped(&process.vm) {
                            process.vm.complete_library(call, 1, None)?;
                            continue;
                        }
                        let (Some(strings), Some(bank)) = (strings, bank) else {
                            return Ok(Status::Unsupported(call));
                        };
                        if bank.sample(text)?.is_some() {
                            return Err(Error("V1 pointed inventory text has speech"));
                        }
                        let font = crate::text::Font::read(
                            resources,
                            self.declarations
                                .tag_font
                                .ok_or(Error("missing tag font"))?,
                        )?;
                        let mut layout =
                            font.centered(resources, strings.get(text as u32)?, position)?;
                        layout.keep_inventory_horizontal(resources, 320)?;
                        if label.enter(
                            &self.inventory_hover.monitor,
                            (self.held_object != -1).then_some(self.held_object),
                        ) == crate::inventory_hover::LabelAction::Finish
                        {
                            process.vm.complete_library(call, 1, None)?;
                            continue;
                        }
                        let id = self
                            .processes
                            .current_id()
                            .ok_or(Error("pointed print outside scheduler"))?;
                        self.inventory_hover.waits.push((
                            id,
                            crate::inventory_hover::Wait::Label {
                                label,
                                call,
                                layout,
                                visible: true,
                            },
                        ));
                        return Ok(Status::Running);
                    }
                    let Some(position) = process.object_print_position.take() else {
                        let Some((_, position)) = self.inventory_text_cell_at(self.control.cursor)
                        else {
                            process.vm.complete_library(call, 1, None)?;
                            continue;
                        };
                        process.object_print_position = Some(position);
                        self.inventory_hover.monitor.suppress();
                        // V1 lets POINTED text die for one scheduler tick.
                        return Ok(Status::Running);
                    };
                    if self.escaped(&process.vm) {
                        self.inventory_hover.monitor.allow();
                        process.vm.complete_library(call, 1, None)?;
                        continue;
                    }
                    let (Some(strings), Some(bank)) = (strings, bank) else {
                        return Ok(Status::Unsupported(call));
                    };
                    let bytes = strings.get(text as u32)?;
                    let speech = self.voice_enabled() && bank.sample(text)?.is_some();
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
                        self.sample_player.play_speech(bank, text)?;
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
                        remaining: self.text_time(bytes.len())?,
                        sample_serial,
                        timeout: 480,
                        left_events: self.left_events,
                    });
                    return Ok(Status::Running);
                }
                53 | 98 | 104 => {
                    let consumed = match call.id {
                        53 => 6,
                        98 => 4,
                        _ => 5,
                    };
                    let a = args(&process.vm, consumed)?;
                    if call.id == 104 && a[4] != 2 {
                        return Err(Error("invalid TALKATS sustain"));
                    }
                    if self.escaped(&process.vm) {
                        process.vm.complete_library(call, consumed, None)?;
                        continue;
                    }
                    let (Some(strings), Some(bank)) = (strings, bank) else {
                        return Ok(Status::Unsupported(call));
                    };
                    let (x, y, text, time, sustain, actor) = if call.id == 53 {
                        (a[0], a[1], a[3] as u32, a[4], a[5] == 2, None)
                    } else {
                        (a[1], a[2], a[3] as u32, 0, call.id == 104, Some(a[0]))
                    };
                    if time < 0 {
                        return Err(Error("negative print time"));
                    }
                    let bytes = strings.get(text)?;
                    let speech = self.voice_enabled() && bank.sample(text as i32)?.is_some();
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
                        self.text_time(bytes.len())?
                    } else {
                        (time as u32)
                            .checked_mul(24)
                            .ok_or(Error("print time overflow"))?
                    };
                    let sample_serial = if speech {
                        self.sample_player.play_speech(bank, text as i32)?;
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
                        self.control_inventory_active(),
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
                    if self.escaped(&process.vm) {
                        process.vm.complete_library(call, 0, None)?;
                        continue;
                    }
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
                66 => {
                    let handle = args(&process.vm, 1)?[0];
                    if !self.escaped(&process.vm) {
                        self.change_palette(resources, Handle(handle as u32))?;
                    }
                    process.vm.complete_library(call, 1, None)?;
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
                    if self.actor_steps.get(&a[0]).copied().unwrap_or(0) < a[1] as u32
                        && !self.escaped(&process.vm)
                    {
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
                        Some(i32::from(
                            self.sample_player.id.is_some() && !self.escaped(&process.vm),
                        )),
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
                    // V1 PlaySample returns immediately while a speech
                    // channel is active. TRYPLAYSAMPLE's busy-channel yield
                    // above remains distinct from this speech-only guard.
                    if self.sample_player.id.is_some() && self.sample_player.is_speech() {
                        args(&process.vm, 2)?;
                        process.vm.complete_library(call, 2, None)?;
                        continue;
                    }
                    // V1 tests speech priority first, then stops existing
                    // effects for an already-escaped call, even at zero volume.
                    if self.escaped(&process.vm) {
                        args(&process.vm, 2)?;
                        self.sample_player.stop();
                        process.vm.complete_library(call, 2, None)?;
                        continue;
                    }
                    if self
                        .configuration
                        .as_ref()
                        .is_some_and(|menu| menu.settings().volumes[1] == 0)
                    {
                        args(&process.vm, 2)?;
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
                        process.sample_serial = Some(self.sample_player.serial);
                        return Ok(Status::Running);
                    }
                    process.vm.complete_library(call, 2, None)?;
                }
                11 => {
                    let a = args(&process.vm, 1)?;
                    self.start_background(resources, Handle(a[0] as u32))?;
                    process.vm.complete_library(call, 1, None)?;
                }
                49 | 71 | 81 => {
                    let splay = call.id == 71;
                    let consumed = if splay { 7 } else { 6 };
                    let raw = args(&process.vm, consumed)?;
                    // SPlay checks Escape before entering Play's null-film assertion.
                    if splay && self.escaped(&process.vm) {
                        process.vm.complete_library(call, consumed, None)?;
                        continue;
                    }
                    let a = if splay { &raw[1..] } else { raw };
                    if a[0] == 0 {
                        return Err(Error("null film handle"));
                    }
                    if self.escaped(&process.vm) {
                        process.vm.complete_library(call, consumed, None)?;
                        continue;
                    }
                    if !(splay && matches!(process.event, 4 | 5)) {
                        if let Some(id) = process.actor {
                            if !self.actors.get(id)?.alive {
                                return Ok(Status::Halted);
                            }
                        }
                    }
                    // The pinned PlayFilm/PlayFilmc APIs convert SPLAY's slow
                    // factor to bool; 0 and 1 both permit every hidden step.
                    if !splay && !matches!(a[5], 0 | 1) {
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
                    let blocking = if splay { a[5] != 0 } else { a[5] == 1 };
                    let mut primary = None;
                    for (column, mut reel) in reels.into_iter().enumerate() {
                        let captured = process
                            .vm
                            .escape_generation
                            .filter(|_| process.vm.escape_enabled);
                        // V1 updates actor escape flags before its duplicate
                        // reel and normal/talk arbitration checks.
                        if self.actors.get(reel.object.id as u32)?.alive {
                            self.capture_reel_escape(reel.object.id, captured);
                        }

                        if self.playback.start(reel.object.id, film)
                            && self.claim_reel(&mut reel)?
                        {
                            if splay {
                                self.splay_reels.insert(reel.playback);
                            }
                            if let Some(captured) = process
                                .vm
                                .escape_generation
                                .filter(|_| process.vm.escape_enabled)
                            {
                                self.reel_escape
                                    .insert(reel.object.id, (reel.playback, captured));
                            }
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
                        self.finish_reel(resources, &primary)?;
                    }
                    process.vm.complete_library(call, consumed, None)?;
                }
                69 | 70 | 84 => {
                    // Release V1 SHOWPOS/SHOWSTRING and VIBRATE have no effect.
                    process.vm.complete_library(call, 0, None)?;
                }
                106 => {
                    if !matches!(args(&process.vm, 1)?[0], 0 | 1) {
                        return Err(Error("invalid MIDI fade direction"));
                    }
                    process.wait = Some((call, 1));
                    return Ok(Status::Running);
                }
                48 => {
                    if self.escaped(&process.vm) {
                        process.vm.complete_library(call, 0, None)?;
                        continue;
                    }
                    process.wait = Some((call, 2));
                    return Ok(Status::Running);
                }
                86 => {
                    let a = args(&process.vm, 2)?;
                    if self.escaped(&process.vm) {
                        process.vm.complete_library(call, 2, None)?;
                        continue;
                    }
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
                    let request = self.intro.as_ref().map_or(request, |intro| {
                        intro.redirect(request, self.escape_events.unwrap_or(1))
                    });
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
            if let Some(palette) = &self.background_palette {
                result.set_scene_palette(palette)?;
            }
        }
        let hidden: Vec<_> = self
            .movers
            .iter()
            .filter(|(id, mover)| mover.hidden && !self.reel_owners.contains_key(id))
            .map(|(_, mover)| {
                let mut animation = mover.animation.clone();
                animation.object.z = -1;
                animation
            })
            .collect();
        result.draw_animations_ordered(resources, hidden.iter(), self.world_origin)?;
        if let Some(background) = &self.background {
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
                .filter(|(id, m)| !m.hidden && !self.reel_owners.contains_key(id))
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
                if let Some(resize) = &self.inventory_resize {
                    result.draw_animations_ordered(resources, resize.overlay.iter(), [0, 0])?;
                }
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
        for (_, wait) in &self.inventory_hover.waits {
            if let crate::inventory_hover::Wait::Label {
                layout,
                visible: true,
                ..
            } = wait
            {
                layout.draw(resources, &mut result)?;
            }
        }
        if let Some(menu) = &self.configuration {
            menu.draw(resources, &mut result)?;
        }
        if !self.control.hidden {
            if let Some(cursor) = &self.held_cursor.as_ref().filter(|_| {
                self.configuration
                    .as_ref()
                    .is_none_or(|menu| menu.page().is_none())
            }) {
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
    #[test]
    fn completed_pointed_script_waits_then_runs_fresh_unpoint_after_save() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        // EVENT(0) -> global zero, then halt. The same code observes UNPOINT
        // only after the owner has finished POINTED and the pointer departs.
        scene[32..39].copy_from_slice(&[2, 0x4e, 31, 0x4c, 0, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.tick(&resources).unwrap();
        engine.active_inventory = Some(1);
        engine.declarations.windows[1] = Some(crate::startup::Window {
            title: Handle(0),
            capacity: 1,
            min: [1, 1],
            size: [1, 1],
            max: [1, 1],
            position: [100, 100],
            first_display: 0,
            contents: vec![190],
        });
        engine.control.cursor = [118, 132];
        let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
        process.inventory_object = Some(190);
        process.event = 0;
        let id = engine
            .processes
            .spawn(Some(engine.generation), process)
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [0]);
        assert_eq!(engine.inventory_hover.waits.len(), 1);
        let encoded = crate::save::Snapshot::capture(&engine, [7; 32])
            .to_bytes()
            .unwrap();
        assert_eq!(u32::from_le_bytes(encoded[8..12].try_into().unwrap()), 9);
        let restored = crate::save::Snapshot::from_bytes(&encoded, [7; 32]).unwrap();
        restored.verify_scripts(&resources).unwrap();
        let mut resumed = restored.state().clone();
        for e in [&mut engine, &mut resumed] {
            e.tick(&resources).unwrap();
            assert_eq!(e.globals, [0]);
            assert_eq!(e.inventory_hover.waits.len(), 1);
            e.control.cursor = [0, 0];
            e.tick(&resources).unwrap();
            assert_eq!(e.globals, [8]);
            assert!(e.inventory_hover.waits.is_empty());
            assert!(!e.processes.entries().any(|(p, _)| p == id));
        }
        assert_eq!(engine, resumed);
        let mut cutaway = restored.state().clone();
        let scripts = cutaway.capture_scene_scripts();
        assert!(
            scripts
                .contexts
                .iter()
                .all(|p| p.inventory_object.is_none() || p.event != 0)
        );
        cutaway.restore_scene_scripts(&resources, &scripts).unwrap();
        assert!(cutaway.inventory_hover.waits.is_empty());
        let mut invalid = restored.state().clone();
        invalid.processes.state_mut(id).unwrap().vm = Machine::default();
        assert_eq!(
            invalid.validate_snapshot_structure(),
            Err(Error("inventory unpoint waiter has unfinished script"))
        );
        invalid = restored.state().clone();
        invalid
            .inventory_hover
            .waits
            .push(invalid.inventory_hover.waits[0].clone());
        assert_eq!(
            invalid.validate_snapshot_structure(),
            Err(Error("duplicate inventory hover process"))
        );
        invalid = restored.state().clone();
        invalid.processes.cancel(id).unwrap();
        assert_eq!(
            invalid.validate_snapshot_structure(),
            Err(Error("missing inventory hover process"))
        );
    }
    #[test]
    fn inventory_text_cells_use_declared_geometry_and_absolute_indices() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        for slot in 0..3 {
            engine.active_inventory = Some(slot);
            engine.declarations.windows[slot as usize] = Some(crate::startup::Window {
                title: Handle(0),
                capacity: 100,
                min: [1, 1],
                size: [2, 2],
                max: [10, 6],
                position: [87, 100],
                first_display: 3,
                contents: vec![],
            });
            // invItem includes empty cells and works independently of the old
            // frame/hidden flag. These differ from invItemId's visibility rules.
            engine.conversation.hidden = true;
            assert_eq!(
                engine.inventory_text_cell_at([93, 120]),
                Some((3, [105, 120]))
            );
            assert_eq!(
                engine.inventory_text_cell_at([143, 170]),
                Some((6, [131, 146]))
            );
            for point in [[118, 132], [105, 145], [92, 120], [i32::MIN, i32::MAX]] {
                assert_eq!(engine.inventory_text_cell_at(point), None);
            }
            engine.declarations.windows[slot as usize]
                .as_mut()
                .unwrap()
                .first_display = usize::MAX;
            assert_eq!(engine.inventory_text_cell_at([119, 120]), None);
        }
        engine.active_inventory = Some(-1);
        assert_eq!(engine.inventory_text_cell_at([93, 120]), None);
    }

    #[test]
    fn inventory_click_regions_precede_icons_and_gap_fallback_is_only_for_putdown() {
        use crate::conversation_window::{Content, Frame, Layout};
        use PolygonInteraction::{Action, Look, WalkTo};
        let (master, mut scene) = fixtures();
        // LOOK inserts a replacement item; POINTED and UNPOINT simply halt.
        scene[32..49].copy_from_slice(&[
            2, 0x4e, 31, 0x41, 3, 20, 0x52, 16, 1, 201, 0, 0, 0, 0x4e, 9, 0, 0,
        ]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.active_inventory = Some(1);
        engine.held_object = 190;
        engine.declarations.windows[1] = Some(crate::startup::Window {
            title: Handle(0),
            capacity: 100,
            min: [2, 1],
            size: [2, 2],
            max: [10, 6],
            position: [100, 100],
            first_display: 0,
            contents: vec![],
        });
        engine.inventory_display = Some((
            Frame {
                layout: Layout {
                    parts: vec![],
                    background: [101, 101, 92, 73],
                    title: [140, 110],
                    icons: vec![[106, 120], [132, 120], [106, 146], [132, 146]],
                    scrollbar: None,
                },
                parts: vec![],
            },
            Content {
                title: crate::text::Layout { glyphs: vec![] },
                icons: vec![],
            },
        ));
        for (point, slot) in [([131, 132], 1), ([118, 145], 2), ([131, 145], 3)] {
            assert_eq!(engine.inventory_click_slot_at(point, WalkTo), Some(slot));
            for event in [Action, Look] {
                assert_eq!(engine.inventory_click_slot_at(point, event), None);
            }
            engine.held_object = -1;
            assert_eq!(engine.inventory_click_slot_at(point, WalkTo), None);
            engine.held_object = 190;
        }
        // Even inconsistent saved icon geometry cannot steal a header/border
        // click: the original invArea dispatch precedes invItem lookup.
        engine.inventory_display.as_mut().unwrap().0.layout.icons = vec![[100, 100]];
        for point in [[100, 100], [110, 110], [99, 110], [i32::MAX, i32::MAX]] {
            for event in [WalkTo, Action, Look] {
                assert_eq!(engine.inventory_click_slot_at(point, event), None);
            }
        }
        engine.conversation.hidden = true;
        assert_eq!(engine.inventory_click_slot_at([110, 120], WalkTo), None);
        // A move and click in one host frame must discover POINTED first.
        // Its object ID is a different global glitter index from the click's
        // backing-array cell, which ADDOPENINV subsequently consumes.
        engine.conversation.hidden = false;
        engine.held_object = -1;
        engine.declarations.windows[1].as_mut().unwrap().contents = vec![190, 200];
        for id in [190, 200] {
            engine.inventory_objects.insert(
                id,
                crate::inventory::Object {
                    id,
                    film: Handle(0),
                    script: Handle((1 << 23) | 32),
                    attributes: 0,
                },
            );
        }
        engine.control.cursor = [144, 132];
        assert!(engine.interact_inventory(&resources, 1, Look).unwrap());
        assert_eq!(engine.inventory_event_index, 1);
        assert!(
            engine
                .processes
                .states()
                .any(|p| p.inventory_object == Some(200) && p.event == 0)
        );
        engine.update_inventory_hover(&resources).unwrap();
        assert_eq!(engine.inventory_event_index, 1);
        let mut look = engine
            .processes
            .states()
            .find(|p| p.inventory_object == Some(200) && p.event == 3)
            .unwrap()
            .clone();
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut look)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(
            engine.declarations.windows[1].as_ref().unwrap().contents,
            [190, 201, 200]
        );
    }

    #[test]
    fn drag_snapshot_rejects_invalid_owner_and_extreme_motion_is_atomic() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        for slot in [i32::MIN, -1, 0, 1, 2, 3, i32::MAX] {
            engine.inventory_drag = Some(slot);
            assert_eq!(
                engine.validate_snapshot_structure(),
                Err(Error("invalid inventory drag owner"))
            );
        }
        engine.inventory_drag = None;
        engine.declarations.windows[1] = Some(crate::startup::Window {
            title: Handle(0),
            capacity: 10,
            min: [1, 1],
            size: [2, 2],
            max: [3, 3],
            position: [100, 100],
            first_display: 0,
            contents: vec![],
        });
        let layout = crate::conversation_window::Layout::inventory(
            engine.declarations.windows[1].as_ref().unwrap(),
            [8, 24],
            40,
            8,
        )
        .unwrap();
        engine.inventory_display = Some((
            crate::conversation_window::Frame {
                layout,
                parts: vec![],
            },
            crate::conversation_window::Content {
                title: crate::text::Layout { glyphs: vec![] },
                icons: vec![],
            },
        ));
        engine.active_inventory = Some(1);
        engine.control.enabled = true;
        engine.move_cursor([140, 110]);
        engine.ticks = 10;
        engine
            .mouse_button(&resources, crate::mouse::Button::Left, true)
            .unwrap();
        assert!(engine.inventory_dragging());
        let snapshot = crate::save::Snapshot::capture(&engine, [3; 32]);
        let bytes = snapshot.to_bytes().unwrap();
        assert_eq!(&bytes[8..12], &5u32.to_le_bytes());
        assert_eq!(
            crate::save::Snapshot::from_bytes(&bytes, [3; 32]).unwrap(),
            snapshot
        );
        for hidden in [false, true] {
            let mut invalid = engine.clone();
            if hidden {
                invalid.conversation.hidden = true;
            } else {
                invalid.active_inventory = Some(2);
            }
            assert!(
                crate::save::Snapshot::capture(&invalid, [3; 32])
                    .to_bytes()
                    .is_err()
            );
        }
        let display = engine.inventory_display.clone();
        engine.move_cursor([i32::MIN, i32::MAX]);
        assert!(!engine.inventory_dragging());
        assert_eq!(engine.inventory_display, display);
        assert_eq!(
            engine.declarations.windows[1].as_ref().unwrap().position,
            [100, 100]
        );
    }

    #[test]
    #[ignore = "requires DISCWORLD_TEST_ISO"]
    fn original_menu_blocks_scripted_control_on_tag_restoration() {
        let data = std::fs::read(std::env::var_os("DISCWORLD_TEST_ISO").unwrap()).unwrap();
        let iso = crate::iso::Iso::parse(&data).unwrap();
        let file = |name: &str| {
            let entry = iso
                .entries
                .iter()
                .find(|entry| entry.path.eq_ignore_ascii_case(name))
                .unwrap();
            &data[entry.range.clone()]
        };
        let index = crate::resource::parse_index(file("/DISCWLD/INDEX")).unwrap();
        let mut resources = Resources(
            index
                .iter()
                .map(|entry| file(&format!("/DISCWLD/{}", entry.name)))
                .collect(),
        );
        let strings = crate::text::Strings::new(file("/DISCWLD/ENGLISH.TXT")).unwrap();
        let bank =
            SampleBank::new(file("/DISCWLD/ENGLISH.IDX"), file("/DISCWLD/ENGLISH.SMP")).unwrap();
        let tracks = crate::music::archive(file("/DISCWLD/MIDI.DAT")).unwrap();
        let mut engine = Engine::bootstrap(&resources).unwrap();
        while engine.ticks < 500 {
            let mut menu_cycle = engine.clone();
            let expected = engine
                .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
                .unwrap();
            let (actual, dispatch) = menu_cycle
                .tick_with_configuration(&resources, Some(&bank), Some(&tracks), &strings, &[])
                .unwrap();
            assert_eq!(actual, expected);
            assert_eq!(menu_cycle, engine);
            assert_eq!(dispatch.outcome, crate::menu_navigation::Outcome::default());
            assert!(dispatch.selection.is_none());
        }
        engine.configuration = Some(
            crate::menu_controller::Controller::new(crate::menu_controller::Settings {
                volumes: [255; 3],
                double_click: 10,
                swap_buttons: false,
                text_speed: 50,
                subtitles: true,
                flipped: false,
            })
            .unwrap(),
        );
        engine
            .open_configuration(&resources, &strings, &[], crate::menu_navigation::Key::Main)
            .unwrap();
        let mut editing = engine.clone();
        editing.close_inventory();
        editing
            .open_configuration(&resources, &strings, &[], crate::menu_navigation::Key::Save)
            .unwrap();
        let mut expected = editing.clone();
        expected
            .configuration
            .as_mut()
            .unwrap()
            .edit(&resources, b'R', false)
            .unwrap();
        assert!(editing.edit_configuration(&resources, b'R', false).unwrap());
        assert_eq!(editing, expected); // No extra PlayerEvent counter or idle reset.
        expected.user_event().unwrap();
        expected
            .configuration
            .as_mut()
            .unwrap()
            .scroll(crate::save_menu::Scroll::PageDown);
        assert!(
            editing
                .scroll_configuration(crate::save_menu::Scroll::PageDown)
                .unwrap()
        );
        assert_eq!(editing, expected);
        let mut input = engine.clone();
        let processes = input.processes.clone();
        let before = input.render(&resources).unwrap();
        input.move_cursor([90, 59]);
        let mut expected_menu = engine.configuration.clone().unwrap();
        expected_menu.point_at(Some([90, 59]));
        assert_eq!(input.configuration.as_ref(), Some(&expected_menu));
        assert_ne!(input.render(&resources).unwrap(), before);
        input.control.frozen = true;
        input.move_cursor([200, 100]);
        assert_eq!(input.control.cursor, [90, 59]);
        assert_eq!(input.configuration.as_ref(), Some(&expected_menu));
        input.control.frozen = false;
        for event in [
            PolygonInteraction::WalkTo,
            PolygonInteraction::Look,
            PolygonInteraction::Action,
        ] {
            assert!(!input.interact_world(&resources, [200, 100], event).unwrap());
            assert!(!input.world_click(&resources, [200, 100], event).unwrap());
        }
        input.primary_click(&resources).unwrap();
        for button in [crate::mouse::Button::Left, crate::mouse::Button::Right] {
            input.mouse_button(&resources, button, true).unwrap();
            input.mouse_button(&resources, button, false).unwrap();
        }
        assert_eq!(input.processes, processes);
        // Cursor monitoring also sees changes made without host move_cursor.
        input.control.cursor = [200, 100];
        expected_menu.point_at(Some([200, 100]));
        input
            .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
            .unwrap();
        assert_eq!(input.configuration.as_ref(), Some(&expected_menu));
        // Execute an actual CONTROL_ON bytecode call alongside original menu resources.
        let code = [0x0a, 0x00, 0x34, 0x33, 0, 0, 0, 0, 3, 0x4e, 14, 0];
        let handle = Handle(((resources.0.len() as u32) << 23) | 8);
        resources.0.push(&code);
        for window_open in [true, false] {
            let mut instance = engine.clone();
            if !window_open {
                instance.close_inventory();
            }
            instance
                .control
                .apply_at(0, window_open, instance.world_origin)
                .unwrap();
            let mut process = ScriptProcess::new(handle);
            assert_eq!(
                instance
                    .run_process(
                        &resources,
                        Some(&bank),
                        Some(&tracks),
                        Some(&strings),
                        &mut process
                    )
                    .unwrap(),
                Status::Halted
            );
            assert!(instance.control.enabled);
            assert_eq!(instance.control.tags_enabled, !window_open);
            assert_eq!(
                instance.configuration.as_ref().unwrap().page().is_some(),
                window_open
            );
        }
    }

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
    fn intro_scene_escape_ignores_opcodes_redirects_and_restores_exactly() {
        let (master, mut scene) = fixtures();
        // ESCOFF/ESCON cannot disarm or recapture an intro scene's generation.
        let code = [
            41, 40, 0x41, 100, 3, 0x4e, 86, 1, 0, 0, 128, 0, 3, 2, 0x4e, 44, 0,
        ];
        scene[8..8 + code.len()].copy_from_slice(&code);
        let mut title = scene.clone();
        title[8..14].copy_from_slice(&[41, 0x41, 77, 0x4c, 0, 0]);
        title[88..92].copy_from_slice(&((2u32 << 23) | 8).to_le_bytes());
        title[92..96].copy_from_slice(&((2u32 << 23) | 112).to_le_bytes());
        let resources = Resources(vec![&master, &scene, &title]);
        let mut engine = Engine::bootstrap_with_title(&resources, Some(Handle(2 << 23))).unwrap();
        assert!(engine.intro_active());
        engine.tick(&resources).unwrap();
        let scene_vm = &engine
            .processes
            .states()
            .find(|p| p.wait.is_some())
            .unwrap()
            .vm;
        assert!(scene_vm.escape_enabled);
        assert_eq!(scene_vm.escape_generation, Some(1));
        // Intro redirect is applied after hook interception.
        engine
            .scene_hooks
            .set(crate::scene_hook::Request::new(Handle(1 << 23), 1, 1).unwrap());
        engine.script_escape_event().unwrap();
        for _ in 0..50 {
            let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
                .to_bytes()
                .unwrap();
            assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 17);
            let snapshot = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
            assert_eq!(snapshot.to_bytes().unwrap(), bytes);
            let mut restored = snapshot.state().clone();
            engine.tick(&resources).unwrap();
            restored.tick(&resources).unwrap();
            assert_eq!(engine, restored);
            if engine.globals[0] == 77 {
                break;
            }
        }
        assert_eq!(engine.scene, Handle(2 << 23));
        assert_eq!(engine.globals[0], 77);
        assert!(!engine.intro_active());
        engine.request_restart();
        engine.tick(&resources).unwrap();
        assert!(engine.intro_active());
        assert_eq!(engine.scene, Handle(1 << 23));
        assert_eq!(engine.escape_events, Some(1));
        engine.end_intro();
        assert!(!engine.intro_active());
        let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
            .to_bytes()
            .unwrap();
        let decoded = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
        assert!(!decoded.state().intro_active());
        // A nested v17 envelope must be rejected before recursive decoding.
        use sha2::{Digest, Sha256};
        let payload =
            postcard::to_allocvec(&(bytes.clone(), engine.intro.as_ref().unwrap())).unwrap();
        let mut bad = bytes[..80].to_vec();
        bad[44..48].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        bad[48..80].copy_from_slice(&Sha256::digest(&payload));
        bad.extend(payload);
        assert!(crate::save::Snapshot::from_bytes(&bad, [7; 32]).is_err());
        let legacy = Engine::bootstrap(&resources).unwrap();
        let fallback = Engine::bootstrap_with_title(&resources, None).unwrap();
        assert_eq!(legacy, fallback);
    }

    #[test]
    fn player_escape_counts_once_before_control_gate_and_preserves_hidden_inventory() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let strings = crate::text::Strings::new(&[1, 0, 52, 51, 0, 0, 0, 0, 0, 1, b'W']).unwrap();
        for enabled in [false, true] {
            for hidden in [false, true] {
                let mut engine = Engine::bootstrap(&resources).unwrap();
                engine.control.enabled = enabled;
                engine.active_inventory = Some(2);
                engine.conversation.hidden = hidden;
                let before = (engine.user_events, engine.left_events);
                engine.escape_input(&resources, &strings, &[]).unwrap();
                assert_eq!(engine.escape_events, Some(2));
                assert_eq!(
                    (engine.user_events, engine.left_events),
                    (before.0 + 1, before.1 + 1)
                );
                assert_eq!(
                    engine.active_inventory,
                    if enabled && !hidden { None } else { Some(2) }
                );
                assert_eq!(engine.control.enabled, enabled);
            }
        }
        let mut conversation = Engine::bootstrap(&resources).unwrap();
        conversation.control.enabled = true;
        conversation.active_inventory = Some(0);
        conversation.conversation.polygon = Some(0);
        let before = conversation.clone();
        assert!(
            conversation
                .escape_input(&resources, &strings, &[])
                .is_err()
        );
        assert_eq!(
            conversation, before,
            "bad postamble must not partially close"
        );
        conversation.polygons = crate::polygon::Polygons::new(
            vec![crate::polygon::Polygon {
                kind: crate::polygon::Kind::Tag,
                corners: [[0, 0], [10, 0], [10, 10], [0, 10]],
                tag_position: [0; 2],
                tag_text: Handle(0),
                node_position: [0; 2],
                film: Handle(0),
                refer_type: 0,
                id: 1,
                scales: [0; 2],
                reel: 0,
                z_factor: 0,
                nodes: vec![],
                lines: vec![],
                script: Handle((1 << 23) | 8),
            }],
            0,
        );
        conversation
            .escape_input(&resources, &strings, &[])
            .unwrap();
        assert_eq!(conversation.active_inventory, None);
        assert_eq!(conversation.conversation.icon, -1);
        assert_eq!(conversation.conversation.polygon, None);
        assert!(
            conversation
                .processes
                .states()
                .any(|p| p.event == 7 && p.polygon == Some(0))
        );
        let mut overflow = Engine::bootstrap(&resources).unwrap();
        overflow.left_events = u64::MAX;
        let before = overflow.clone();
        assert!(overflow.escape_input(&resources, &strings, &[]).is_err());
        assert_eq!(overflow, before);
    }

    #[test]
    fn startup_menu_key_enables_control_without_incrementing_waitkey_counter() {
        use crate::menu_navigation::Key;
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.control.enabled = false;
        engine.menu_enabled = true;
        let before = (engine.user_events, engine.left_events);
        assert!(
            engine
                .configuration_key(&resources, Key::Main, false)
                .unwrap()
        );
        assert!(engine.control.enabled && !engine.control.hidden);
        assert!(!engine.menu_enabled);
        assert_eq!((engine.user_events, engine.left_events), before);
        assert!(
            !engine
                .configuration_key(&resources, Key::Main, true)
                .unwrap()
        );
        assert_eq!(engine.user_events, before.0 + 1);
        assert_eq!(engine.left_events, before.1);
        for key in [Key::Save, Key::Load, Key::Quit] {
            engine.control.enabled = false;
            engine.menu_enabled = true;
            let count = engine.user_events;
            assert!(!engine.configuration_key(&resources, key, false).unwrap());
            assert_eq!(engine.user_events, count + 1);
            assert!(engine.menu_enabled); // only Main uses the startup exception
        }
        for slot in [0, 1, 2] {
            engine.active_inventory = Some(slot);
            engine.control.enabled = false;
            engine.menu_enabled = true;
            assert!(
                !engine
                    .configuration_key(&resources, Key::Main, false)
                    .unwrap()
            );
            assert!(engine.control.enabled); // control gate precedes inventory gate
            assert!(!engine.menu_enabled);
        }
    }
    #[test]
    fn menu_key_failures_do_not_partially_change_engine_state() {
        use crate::menu_navigation::Key;
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.user_events = u64::MAX;
        let before = engine.clone();
        assert!(
            engine
                .configuration_key(&resources, Key::Save, false)
                .is_err()
        );
        assert_eq!(engine, before);
        engine.menu_enabled = true;
        engine.declarations.cursor = Some((
            Handle(u32::MAX),
            crate::film::Film {
                rate: 1,
                reels: vec![],
            },
        ));
        let before = engine.clone();
        assert!(
            engine
                .configuration_key(&resources, Key::Main, false)
                .is_err()
        );
        assert_eq!(engine, before);
    }
    #[test]
    fn primary_click_during_hidden_conversation_wakes_waits_without_world_dispatch() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        for enabled in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.active_inventory = Some(0);
            engine.conversation.polygon = Some(0);
            engine.conversation.hidden = true;
            engine.control.enabled = enabled;
            let mut expected = engine.clone();
            expected.left_event().unwrap();
            engine.primary_click(&resources).unwrap();
            assert_eq!(engine, expected);
        }
    }
    #[test]
    fn restart_reboots_game_state_preserves_host_clock_and_reports_restart() {
        let (master, mut scene) = fixtures();
        // Startup asks HASRESTARTED and writes its result to global zero.
        scene[8..13].copy_from_slice(&[0x4e, 120, 0x4c, 0, 0]);
        let restart = [0x0a, 0x00, 0x34, 0x33, 0, 0, 0, 0, 0x4e, 114, 0];
        let resources = Resources(vec![&master, &scene, &restart]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [0]);
        assert!(!engine.has_restarted());
        engine.globals[0] = 99;
        engine.held_object = 260;
        engine.menu_enabled = true;
        engine.no_blocking = true;
        engine.subtitles = false;
        let settings = crate::menu_controller::Settings {
            volumes: [120, 80, 200],
            double_click: 17,
            swap_buttons: true,
            text_speed: 42,
            subtitles: false,
            flipped: true,
        };
        engine.configuration =
            Some(crate::menu_controller::Controller::new(settings.clone()).unwrap());
        engine.mouse = Some(crate::mouse::MouseInput::new(0, 17));
        engine.random.range(0, 100, 0);
        let random = engine.random.clone();
        let tracks = [crate::music::Track {
            offset: 4,
            events: &[0x90, 60, 100, 30, 40, 0xff, 0x2f, 0],
        }];
        engine.music.play(&tracks, 4, false).unwrap();
        engine.sample_player.id = Some(5);
        engine.timers.start(1, 12, false, false).unwrap();
        engine.scene_calls = Some(std::collections::VecDeque::new());
        engine
            .processes
            .spawn(None, ScriptProcess::new(Handle((2 << 23) | 8)))
            .unwrap();
        let tick = engine.ticks;
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        assert_eq!(engine.ticks, tick + 1);
        assert!(engine.has_restarted());
        assert!(!engine.restart_pending);
        assert_eq!(engine.configuration.as_ref().unwrap().settings(), settings);
        assert!(engine.configuration.as_ref().unwrap().page().is_none());
        assert_eq!(
            engine.mouse,
            Some(crate::mouse::MouseInput::new(engine.ticks as u32, 17))
        );
        assert!(!engine.subtitles);
        assert_eq!(engine.held_object, -1);
        assert_eq!(engine.random, random);
        assert!(!engine.music.playing());
        assert_eq!(engine.sample_player.id, None);
        assert_eq!(engine.globals, [0]);
        assert!(!engine.menu_enabled);
        assert!(!engine.no_blocking);
        assert_eq!(engine.timers, crate::timers::Timers::default());
        assert!(
            engine
                .scene_calls
                .as_ref()
                .unwrap()
                .iter()
                .any(|c| c.call.id == 114)
        );
        // Compare all game state, including scheduler, scene hooks, actors and
        // inventory definitions, against a fresh startup at the same host time.
        let mut fresh = Engine::bootstrap_after_restart(&resources, true, None).unwrap();
        fresh.ticks = engine.ticks;
        fresh.configuration = Some(crate::menu_controller::Controller::new(settings).unwrap());
        fresh.mouse = Some(crate::mouse::MouseInput::new(engine.ticks as u32, 17));
        fresh.subtitles = false;
        fresh.random = random;
        fresh.music = engine.music.clone();
        fresh.music_frame = engine.music_frame.clone();
        fresh.scene_calls = engine.scene_calls.clone();
        assert_eq!(engine, fresh);
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [1]);
        assert!(engine.music_frame.iter().any(|m| m.status == 0x80));
        assert!(
            engine
                .music_frame
                .iter()
                .any(|m| m.status == 0xb0 && m.data == [123, 0])
        );
    }

    #[test]
    fn restart_preserves_pcm_blocks_and_fractional_audio_clock() {
        let (master, scene) = fixtures();
        let restart = [0x0a, 0x00, 0x34, 0x33, 0, 0, 0, 0, 0x4e, 114, 0];
        let resources = Resources(vec![&master, &scene, &restart]);
        let bank = SampleBank::new(&[0; 4], &[]).unwrap();
        let mut engine = Engine::bootstrap(&resources).unwrap();
        let mut samples = 0;
        for tick in 1..=48 {
            if tick % 3 == 1 {
                engine
                    .processes
                    .spawn(None, ScriptProcess::new(Handle((2 << 23) | 8)))
                    .unwrap();
            }
            engine.tick_audio(&resources, Some(&bank)).unwrap();
            samples += engine.audio_frame.len() as u64;
            assert_eq!(samples, tick * u64::from(crate::audio::SAMPLE_RATE) / 24);
            assert_eq!(engine.music.clock, tick * 5);
            assert_eq!(engine.sample_player.id, None);
            assert!(engine.has_restarted());
        }
    }

    #[test]
    fn failed_restart_bootstrap_does_not_commit_partial_reset() {
        let (master, scene) = fixtures();
        let restart = [0x0a, 0x00, 0x34, 0x33, 0, 0, 0, 0, 0x4e, 114, 0];
        let resources = Resources(vec![&master, &scene, &restart]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine
            .processes
            .spawn(None, ScriptProcess::new(Handle((2 << 23) | 8)))
            .unwrap();
        let before = engine.clone();
        let mut bad_master = master.clone();
        bad_master[8..12].copy_from_slice(&65537u32.to_le_bytes());
        let bad_resources = Resources(vec![&bad_master, &scene, &restart]);
        assert_eq!(
            engine.tick(&bad_resources),
            Err(Error("global allocation limit"))
        );
        assert_eq!(engine, before);
    }

    #[test]
    fn hideactor_keeps_mover_separate_and_draws_it_behind_background() {
        use crate::save::Snapshot;
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        scene[32..40].copy_from_slice(&[0x41, 77, 0x41, 254, 0x4e, 34, 0x4c, 0]);
        scene[40] = 0;
        let base = 2u32 << 23;
        let mut art = vec![0; 256];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, base + 32),
            (12, base + 136),
            (16, 116),
            (32, base + 64),
            (40, 1),
            (44, 640),
            (64, base + 80),
            (80, 0x00040004),
            (88, base + 96),
            (92, base + 100),
            (100, 2),
            (136, 10),
            (170, 1),
            (200, base + 208),
            (208, 0x00040004),
            (216, base + 96),
            (220, base + 170),
        ] {
            art[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        art[104..107].copy_from_slice(&[90, 80, 70]);
        art[108..111].copy_from_slice(&[20, 30, 40]);
        art[116..132].fill(1);
        art[174..177].copy_from_slice(&[60, 70, 80]);
        let resources = Resources(vec![&master, &scene, &art]);
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
        let mut mover = crate::mover::Mover::new(
            &resources,
            crate::mover::Placement {
                actor: 1,
                position: [640, 0],
                direction: crate::mover::Direction::Forward,
                path: None,
                node: None,
                scale: 1,
                standing: Handle(base),
                base_walk: Handle(base),
                z_factor: 0,
            },
        )
        .unwrap();
        mover.animation.frame = Handle(base + 200);
        engine.movers.insert(1, mover);
        engine.background = Some(
            Animation::new(
                &resources,
                &crate::film::Reel {
                    object: Handle(base + 32),
                    script: Handle(base + 136),
                },
                1,
            )
            .unwrap(),
        );
        engine.world_origin = [640, 0];
        engine.control.tags_enabled = true;
        engine.actor_tags.insert(1, (Handle(0), 0));
        assert_eq!(engine.actor_at(&resources, [1, 1]).unwrap(), Some(1));
        assert_eq!(
            &engine.render(&resources).unwrap().rgba[..4],
            &[60, 70, 80, 255]
        );
        let before = engine.movers[&1].clone();
        let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(engine.globals[0], 77);
        assert!(engine.movers[&1].hidden);
        assert!(!engine.actors.get(1).unwrap().hidden);
        assert_eq!(engine.actor_at(&resources, [1, 1]).unwrap(), None);
        assert!(engine.actors.get(1).unwrap().alive);
        assert_eq!(engine.movers[&1].animation, before.animation);
        assert_eq!(
            &engine.render(&resources).unwrap().rgba[..4],
            &[90, 80, 70, 255]
        );
        let bytes = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
        let restored = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
        assert_eq!(restored.state(), &engine);
        assert_eq!(
            restored.preview(&resources).unwrap(),
            engine.render(&resources).unwrap()
        );
        let background = engine.background.take();
        assert_eq!(
            &engine.render(&resources).unwrap().rgba[..4],
            &[60, 70, 80, 255],
            "hidden means behind background, not discarded"
        );
        engine.background = background;
        // A replacement PLAY owns the hidden mover. Finishing the superseded
        // reel must not expose it underneath the replacement.
        let mut first = before.animation.clone();
        first.reel = Some(Handle(base + 8));
        engine.movers.get_mut(&1).unwrap().hidden = false;
        assert!(engine.claim_reel(&mut first).unwrap());
        assert!(engine.movers[&1].hidden);
        let mut replacement = first.clone();
        replacement.reel = Some(Handle(base + 16));
        assert!(engine.claim_reel(&mut replacement).unwrap());
        engine.release_reel(&first);
        assert!(engine.movers[&1].hidden);
        assert!(engine.owns_reel(&replacement));
        engine.release_reel(&replacement);
        assert!(!engine.movers[&1].hidden);
        assert!(engine.reel_owners.is_empty());

        // Continuing the same walk does not reveal a hidden mover. Explicitly
        // selecting a new walking reel does. STAND preserves hidden state.
        let mover = engine.movers.get_mut(&1).unwrap();
        mover.hidden = true;
        mover
            .select_walk_reel(
                &resources,
                &engine.declarations,
                crate::mover::Direction::Forward,
                1,
                false,
            )
            .unwrap();
        assert!(mover.hidden);
        mover
            .select_walk_reel(
                &resources,
                &engine.declarations,
                crate::mover::Direction::Forward,
                1,
                true,
            )
            .unwrap();
        assert!(!mover.hidden);
        mover.hidden = true;
        mover
            .stand(
                &resources,
                &engine.declarations,
                &engine.polygons,
                [-1, -1],
                Handle(2),
            )
            .unwrap();
        assert!(mover.hidden, "direction-only STAND does not position mover");
        for film in [Handle(0), Handle(2), Handle(base)] {
            mover
                .stand(
                    &resources,
                    &engine.declarations,
                    &engine.polygons,
                    [644, 0],
                    film,
                )
                .unwrap();
            assert!(mover.hidden, "explicit STAND preserves hidden state");
            assert_eq!(mover.placement.position, [644, 0]);
        }
        // An uncreated declared mover resets visibility on creation. Ordinary
        // actors use their own flag; hiding never changes alive state.
        engine.movers.clear();
        for declared in [true, false] {
            if !declared {
                engine.declarations.movers.clear();
            }
            let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap();
            assert_eq!(engine.actors.get(1).unwrap().hidden, !declared);
            assert!(engine.actors.get(1).unwrap().alive);
        }
    }

    #[test]
    fn setpalette_recolors_live_background_preserves_stack_and_retains_unwritten_colors() {
        let (master, scene) = fixtures();
        let base = 2u32 << 23;
        let mut art = vec![0; 200];
        for (at, value) in [
            (16, 116),
            (32, base + 64),
            (44, 640),
            (64, base + 80),
            (80, 0x00040004),
            (88, base + 96),
            (92, base + 100),
            (100, 2),
            (140, 1),
            (170, 1),
        ] {
            art[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        art[104..107].copy_from_slice(&[90, 80, 70]);
        art[108..111].copy_from_slice(&[20, 30, 40]);
        art[116..132].fill(1);
        art[144..147].copy_from_slice(&[5, 6, 7]);
        art[174..177].copy_from_slice(&[60, 70, 80]);
        let mut code = vec![0x0a, 0x00, 0x34, 0x33, 0, 0, 0, 0, 0x41, 77, 1];
        code.extend((base + 140).to_le_bytes());
        code.extend([0x4e, 66, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene, &art, &code]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.background = Some(
            Animation::new(
                &resources,
                &crate::film::Reel {
                    object: Handle(base + 32),
                    script: Handle(base + 136),
                },
                1,
            )
            .unwrap(),
        );
        engine.world_origin = [640, 0];
        let before_animation = engine.background.clone();
        assert_eq!(
            &engine.render(&resources).unwrap().rgba[..4],
            &[90, 80, 70, 255]
        );
        let mut process = ScriptProcess::new(Handle((3 << 23) | 8));
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(engine.globals, [77]);
        assert_eq!(engine.background, before_animation);
        let rendered = engine.render(&resources).unwrap();
        assert_eq!(&rendered.rgba[..4], &[5, 6, 7, 255]);
        assert_eq!(
            rendered
                .image_palette(&resources, Handle(base + 100))
                .unwrap(),
            [[5, 6, 7], [20, 30, 40]]
        );
        assert_eq!(
            rendered
                .image_palette(&resources, Handle(base + 170))
                .unwrap(),
            [[60, 70, 80]]
        );
        assert_eq!(
            rendered
                .font_palette(&resources, Handle(base + 170))
                .unwrap(),
            [[5, 6, 7], [20, 30, 40]]
        );
        assert_eq!(engine.clone().render(&resources).unwrap(), rendered);
        let before = engine.clone();
        assert!(
            engine
                .change_palette(&resources, Handle(base + 199))
                .is_err()
        );
        assert_eq!(engine, before);
        assert_eq!(
            crate::graphics::read_palette(&resources, Handle(base + 100)).unwrap()[0],
            [90, 80, 70]
        );
        engine
            .enter_scene(&resources, Handle(1 << 23), Some(1), 1)
            .unwrap();
        assert!(engine.background_palette.is_none());
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
    fn inventory_depiction_persists_and_preserves_held_cursor() {
        let (master, mut scene) = fixtures();
        scene[8] = 0;
        let base = 3 << 23;
        let mut art = vec![0u8; 256];
        for (at, value) in [
            (0, 12u32),
            (4, 1),
            (8, base + 32),
            (12, base + 80),
            (16, 12),
            (20, 1),
            (24, base + 56),
            (28, base + 88),
            (32, base + 96),
            (56, base + 104),
            (80, base + 96),
            (88, base + 104),
            (96, base + 128),
            (104, base + 160),
        ] {
            art[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        art[128..132].copy_from_slice(&[4, 0, 4, 0]);
        art[160..164].copy_from_slice(&[8, 0, 8, 0]);
        let code_for = |item: i32, film: u32| {
            let mut code = vec![0x0a, 0x00, 0x34, 0x33, 0, 0, 0, 0, 0x41, 77, 1];
            code.extend(item.to_le_bytes());
            code.push(1);
            code.extend(film.to_le_bytes());
            code.extend([0x4e, 36, 0x4c, 0, 0]);
            code
        };
        let code = code_for(287, base + 16);
        let resources = Resources(vec![&master, &scene, &code, &art]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.inventory_objects.insert(
            287,
            crate::inventory::Object {
                id: 287,
                film: Handle(base),
                script: Handle((2 << 23) | 8),
                attributes: 4,
            },
        );
        engine.declarations.windows[1] = Some(crate::startup::Window {
            title: Handle(0),
            capacity: 6,
            min: [1, 1],
            size: [1, 1],
            max: [10, 6],
            position: [0; 2],
            first_display: 0,
            contents: vec![287],
        });
        let initial = engine.clone();
        let mut process = ScriptProcess::new(Handle((2 << 23) | 8));
        assert_eq!(
            engine
                .run_process(&resources, None, None, None, &mut process)
                .unwrap(),
            Status::Halted
        );
        assert_eq!(engine.globals, [77]); // Exactly two operands consumed.
        assert!(engine.inventory_dirty);
        assert_eq!(engine.declarations.windows, initial.declarations.windows);
        let object = &engine.inventory_objects[&287];
        assert_eq!(object.film, Handle(base + 16));
        assert_eq!(object.script, initial.inventory_objects[&287].script);
        assert_eq!(object.attributes, 4);

        // Rebuilding visible inventory content uses the replacement animation.
        let window = engine.declarations.windows[1].as_ref().unwrap();
        let layout = crate::conversation_window::Layout::new(window, [8, 16], 12, 8).unwrap();
        let strings = crate::text::Strings::new(&[1, 0, 52, 51, 0, 0, 0, 0, 0]).unwrap();
        let font = crate::text::Font {
            x_spacing: 0,
            y_spacing: 0,
            space: 2,
            flags: 0,
            glyphs: [Handle(base + 128); 256],
        };
        let content = crate::conversation_window::Content::new(
            &resources,
            window,
            &layout,
            &font,
            &strings,
            &engine.inventory_objects,
            -1,
        )
        .unwrap();
        assert_eq!(content.icons[0].animation.object.frame, Handle(base + 104));
        engine
            .enter_scene(&resources, Handle(1 << 23), Some(1), 1)
            .unwrap();
        assert_eq!(engine.inventory_objects[&287].film, Handle(base + 16));
        assert_eq!(engine.clone().inventory_objects, engine.inventory_objects);

        let mut held = initial.clone();
        held.hold_object(&resources, 287).unwrap();
        held.inventory_dirty = false;
        let cursor = held.held_cursor.clone();
        let mut process = ScriptProcess::new(Handle((2 << 23) | 8));
        held.run_process(&resources, None, None, None, &mut process)
            .unwrap();
        assert_eq!(held.held_cursor, cursor);
        assert!(!held.inventory_dirty);
        assert_eq!(held.inventory_objects[&287].film, Handle(base + 16));
        held.hold_object(&resources, -1).unwrap();
        held.hold_object(&resources, 287).unwrap();
        assert_eq!(
            held.held_cursor.as_ref().unwrap().animation.object.frame,
            Handle(base + 104)
        );

        for (item, film) in [(999, base + 16), (287, base + 252)] {
            let bad_code = code_for(item, film);
            let bad_resources = Resources(vec![&master, &scene, &bad_code, &art]);
            let mut invalid = initial.clone();
            let before = invalid.clone();
            let mut process = ScriptProcess::new(Handle((2 << 23) | 8));
            assert!(
                invalid
                    .run_process(&bad_resources, None, None, None, &mut process)
                    .is_err()
            );
            assert_eq!(invalid, before);
        }
    }

    #[test]
    fn timed_inventory_drop_waits_roundtrips_and_can_be_cancelled() {
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
        engine.control.apply(1, true).unwrap();
        let mut scripted = engine.clone();
        assert!(
            scripted
                .inventory_click(&resources, 1, PolygonInteraction::WalkTo)
                .unwrap()
        );
        for _ in 0..11 {
            scripted.tick(&resources).unwrap();
            assert_eq!(scripted.globals, [0]);
        }
        scripted.tick(&resources).unwrap();
        assert_eq!(scripted.globals, [9]);
        assert_eq!(scripted.held_object, 284);

        engine.inventory_objects.get_mut(&284).unwrap().attributes = 0;
        engine.inventory_objects.insert(
            285,
            crate::inventory::Object {
                id: 285,
                film: Handle(0),
                script: Handle((1 << 23) | 32),
                attributes: 0,
            },
        );
        engine.declarations.windows[2].as_mut().unwrap().contents = vec![285];
        let mut full = engine.clone();
        full.declarations.windows[2].as_mut().unwrap().capacity = 1;
        assert!(
            full.inventory_click(&resources, 1, PolygonInteraction::WalkTo)
                .unwrap()
        );
        full.tick(&resources).unwrap();
        full.declarations.windows[2].as_mut().unwrap().capacity = 2;
        full.conversation.hidden = true;
        for _ in 0..11 {
            full.tick(&resources).unwrap();
        }
        assert_eq!(full.held_object, -1);
        assert_eq!(
            full.declarations.windows[2].as_ref().unwrap().contents,
            [285, 284]
        );
        assert!(
            engine
                .inventory_click(&resources, 1, PolygonInteraction::WalkTo)
                .unwrap()
        );
        assert_eq!(engine.held_object, 284);
        assert_eq!(
            engine.declarations.windows[2].as_ref().unwrap().contents,
            [285]
        );
        engine.tick(&resources).unwrap();
        crate::save::Snapshot::capture(&engine, [3; 32])
            .verify_scripts(&resources)
            .unwrap();
        let bytes = crate::save::Snapshot::capture(&engine, [3; 32])
            .to_bytes()
            .unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 3);
        let mut wrong_version = bytes.clone();
        wrong_version[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert!(crate::save::Snapshot::from_bytes(&wrong_version, [3; 32]).is_err());
        let mut restored = crate::save::Snapshot::from_bytes(&bytes, [3; 32])
            .unwrap()
            .state()
            .clone();
        assert_eq!(engine, restored);
        let mut cancelled = restored.clone();
        assert!(
            cancelled
                .inventory_click(&resources, 0, PolygonInteraction::Action)
                .unwrap()
        );
        cancelled.tick(&resources).unwrap();
        assert_eq!(cancelled.globals, [2]);
        assert_eq!(cancelled.held_object, 284);
        assert_eq!(cancelled.clicks, crate::click::State::default());
        // Hide the synthetic window so its intentionally absent display/font
        // resources do not obscure the putdown state transition under test.
        engine.conversation.hidden = true;
        restored.conversation.hidden = true;
        for _ in 0..10 {
            engine.tick(&resources).unwrap();
            restored.tick(&resources).unwrap();
            assert_eq!(engine.held_object, 284);
            assert_eq!(engine, restored);
        }
        engine.tick(&resources).unwrap();
        restored.tick(&resources).unwrap();
        assert_eq!(engine, restored);
        assert_eq!(engine.held_object, -1);
        assert_eq!(
            engine.declarations.windows[2].as_ref().unwrap().contents,
            [285, 284]
        );
        assert_eq!(engine.clicks, crate::click::State::default());
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
        let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
            .to_bytes()
            .unwrap();
        let decoded = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
        assert_eq!(decoded.state(), &engine);
        let mut restored = decoded.state().clone();
        engine.last_interaction_tick = None;
        restored.last_interaction_tick = None;
        for instance in [&mut engine, &mut restored] {
            instance
                .interact_inventory(&resources, 0, PolygonInteraction::WalkTo)
                .unwrap();
        }
        assert_eq!(restored, engine);
    }

    #[test]
    fn escaped_inventory_and_conversation_open_skip_but_end_still_closes() {
        for (opcode, modes) in [(37, vec![1, 2, 99]), (15, vec![0, 1, 2, 3])] {
            for mode in modes {
                for active in [None, Some(0), Some(1)] {
                    let (master, mut scene) = fixtures();
                    scene[8] = 0;
                    let code = [0x41, 77, 0x41, mode, 0x4e, opcode, 0x4c, 0, 0];
                    scene[32..32 + code.len()].copy_from_slice(&code);
                    let resources = Resources(vec![&master, &scene]);
                    let mut engine = Engine::bootstrap(&resources).unwrap();
                    engine.active_inventory = active;
                    engine.control.tags_enabled = false;
                    if active == Some(0) {
                        engine.conversation.polygon = Some(40);
                    }
                    engine.escape_events = Some(2);
                    let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
                    process.polygon = Some(40);
                    process.vm.escape_enabled = true;
                    process.vm.escape_generation = Some(1);
                    let before = engine.clone();
                    assert_eq!(
                        engine
                            .run_process(&resources, None, None, None, &mut process)
                            .unwrap(),
                        Status::Halted
                    );
                    assert_eq!(engine.globals[0], 77);
                    let mut expected = before;
                    if opcode == 15 && mode == 2 {
                        expected.close_conversation();
                    }
                    expected.globals[0] = 77;
                    assert_eq!(engine, expected);
                }
            }
        }
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
    fn queued_walk_snapshot_rejects_missing_lead_and_mover() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        let mut task = ScriptProcess::new(Handle(0));
        task.player_walk = Some((999, [10, 20]));
        engine
            .processes
            .spawn(Some(engine.generation), task)
            .unwrap();
        assert_eq!(
            crate::save::Snapshot::capture(&engine, [0; 32])
                .to_bytes()
                .unwrap_err()
                .0,
            "invalid queued player walk"
        );
    }
    #[test]
    fn physical_click_pair_survives_save_and_cancels_single_script() {
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
        for _ in 0..10 {
            engine.tick(&resources).unwrap();
        }
        engine.control.apply(1, false).unwrap();
        engine.move_cursor([10, 10]);
        engine
            .mouse_button(&resources, crate::mouse::Button::Left, true)
            .unwrap();
        engine
            .mouse_button(&resources, crate::mouse::Button::Left, false)
            .unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [0]);
        let bytes = crate::save::Snapshot::capture(&engine, [4; 32])
            .to_bytes()
            .unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 4);
        let mut restored = crate::save::Snapshot::from_bytes(&bytes, [4; 32])
            .unwrap()
            .state()
            .clone();
        assert_eq!(engine, restored);
        for instance in [&mut engine, &mut restored] {
            instance
                .mouse_button(&resources, crate::mouse::Button::Left, true)
                .unwrap();
            instance
                .mouse_button(&resources, crate::mouse::Button::Left, false)
                .unwrap();
            instance.tick(&resources).unwrap();
            assert_eq!(instance.globals, [2]);
            assert_eq!(instance.clicks, crate::click::State::default());
        }
        assert_eq!(engine, restored);
    }
    #[test]
    fn timed_world_click_delays_cancels_and_roundtrips_pending_save() {
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
        engine.control.apply(1, false).unwrap();
        let legacy = crate::save::Snapshot::capture(&engine, [9; 32])
            .to_bytes()
            .unwrap();
        for speed in [0, 3, 30] {
            let mut configured = engine.clone();
            let mut settings = crate::menu_controller::Settings {
                volumes: [255; 3],
                double_click: speed,
                swap_buttons: false,
                text_speed: 0,
                subtitles: true,
                flipped: false,
            };
            configured.configuration =
                Some(crate::menu_controller::Controller::new(settings.clone()).unwrap());
            assert!(
                configured
                    .world_click(&resources, [10, 10], PolygonInteraction::WalkTo)
                    .unwrap()
            );
            configured.tick(&resources).unwrap();
            assert_eq!(configured.globals, [0]);
            // A sleep already entered uses the value captured on acquisition.
            settings.double_click = 30 - speed;
            configured.configuration =
                Some(crate::menu_controller::Controller::new(settings).unwrap());
            for _ in 0..speed {
                configured.tick(&resources).unwrap();
                assert_eq!(configured.globals, [0]);
            }
            configured.tick(&resources).unwrap();
            assert_eq!(configured.globals, [1]);
            assert_eq!(configured.clicks, crate::click::State::default());
        }
        assert_eq!(u32::from_le_bytes(legacy[8..12].try_into().unwrap()), 1);
        assert!(
            engine
                .world_click(&resources, [10, 10], PolygonInteraction::WalkTo)
                .unwrap()
        );
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [0]);
        let bytes = crate::save::Snapshot::capture(&engine, [9; 32])
            .to_bytes()
            .unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 2);
        let mut restored = crate::save::Snapshot::from_bytes(&bytes, [9; 32])
            .unwrap()
            .state()
            .clone();
        assert_eq!(engine, restored);
        let mut double = restored.clone();
        assert!(
            double
                .world_click(&resources, [10, 10], PolygonInteraction::Action)
                .unwrap()
        );
        double.tick(&resources).unwrap();
        assert_eq!(double.globals, [2]);
        assert_eq!(double.clicks, crate::click::State::default());
        let mut disabled = restored.clone();
        disabled.control.apply(0, false).unwrap();
        for _ in 0..10 {
            engine.tick(&resources).unwrap();
            restored.tick(&resources).unwrap();
            disabled.tick(&resources).unwrap();
            assert_eq!(engine.globals, [0]);
            assert_eq!(engine, restored);
        }
        engine.tick(&resources).unwrap();
        restored.tick(&resources).unwrap();
        disabled.tick(&resources).unwrap();
        assert_eq!(engine.globals, [1]);
        assert_eq!(disabled.globals, [0]);
        assert_eq!(engine, restored);
        assert_eq!(engine.clicks, crate::click::State::default());
        let legacy_restored = crate::save::Snapshot::from_bytes(&legacy, [9; 32]).unwrap();
        assert_eq!(legacy_restored.to_bytes().unwrap(), legacy);
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
        assert!(engine.scene_transition_active());
        cut.pending_scene.as_mut().unwrap().2 = 1;
        assert_eq!(cut.tick(&resources).unwrap(), Status::Running);
        assert_eq!(cut.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(cut.scene, Handle(2 << 23));
        assert!(!cut.fade_on_background);
        assert!(!cut.scene_transition_active());
        let mut restored = engine.clone();
        for multiplier in [0xd000, 0xa000, 0x7000, 0x4000, 0x1000, 0] {
            assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
            assert!(engine.scene_transition_active());
            assert_eq!(engine.fade_multiplier, multiplier);
            restored.tick(&resources).unwrap();
            assert_eq!(engine, restored);
        }
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.scene, Handle(2 << 23));
        assert_eq!(engine.globals, [9]);
        assert!(engine.processes.is_empty());
        assert!(
            engine.scene_transition_active(),
            "waiting for new background"
        );
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
    fn walkpoly_returns_on_entry_without_stopping_and_restores_pending_context() {
        use crate::save::Snapshot;
        // inside, crossing, never entering, Escape before/after start, stopped.
        for mode in 0..8 {
            for custom in [false, true] {
                for lead in [false, true] {
                    let (master, mut scene) = fixtures();
                    scene[8] = 0;
                    scene[16..27]
                        .copy_from_slice(&[3, 0x41, 20, 0x41, 70, 2, 0x41, 2, 0x4e, 87, 0]);
                    scene[56..60].copy_from_slice(&[3, 0x4e, 74, 0]);
                    let base = 2u32 << 23;
                    let mut code = vec![0x41, 77, 3, 1];
                    code.extend_from_slice(&(if custom { base } else { 0 }).to_le_bytes());
                    code.extend([0x41, 99, 0x4e, 90, 0x4c, 0, 0]);
                    scene[32..32 + code.len()].copy_from_slice(&code);
                    let mut film = vec![0; 300];
                    for (at, value) in [
                        (0, 24u32),
                        (4, 1),
                        (8, base + 32),
                        (12, base + 256),
                        (32, base + 200),
                        (40, 1),
                        (200, base + 220),
                        (220, 8 | (16 << 16)),
                        (256, base + 200),
                        (260, 1),
                        (264, (-2i32) as u32),
                    ] {
                        film[at..at + 4].copy_from_slice(&value.to_le_bytes());
                    }
                    let resources = Resources(vec![&master, &scene, &film]);
                    let mut engine = Engine::bootstrap(&resources).unwrap();
                    engine.actors = crate::actor::Actors::new(1).unwrap();
                    if lead {
                        engine.declarations.lead = Some(crate::startup::Lead {
                            id: 1,
                            text: Handle(0),
                            tagged: false,
                        });
                    }
                    engine
                        .declarations
                        .movers
                        .insert(1, [[Handle(base); 12]; 10]);
                    let path = crate::polygon::Polygon {
                        kind: crate::polygon::Kind::Path,
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
                        nodes: vec![],
                        lines: vec![],
                        script: Handle(0),
                    };
                    let mut polygon = path.clone();
                    polygon.kind = crate::polygon::Kind::Tag;
                    let left = match mode {
                        0 => 40,
                        2 => 90,
                        _ => 60,
                    };
                    polygon.corners = [[left, 60], [100, 60], [100, 80], [left, 80]];
                    polygon.node_position = [80, 70];
                    polygon.film = Handle(1); // normal WALKPOLY must not adopt this pose
                    engine.polygons = crate::polygon::Polygons::new(vec![path, polygon], 0);
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
                    if mode == 0 && !custom && !lead {
                        use sha2::{Digest, Sha256};
                        let mut walking = engine.movers[&1].clone();
                        walking
                            .begin_direct_walk(
                                &resources,
                                &engine.declarations,
                                &engine.polygons,
                                [90, 70],
                                [90, 70],
                            )
                            .unwrap();
                        let mut hidden_walk = walking.clone();
                        hidden_walk.hidden = true;
                        for _ in 0..3 {
                            for mover in [&mut walking, &mut hidden_walk] {
                                mover
                                    .advance_direct_walk(
                                        &resources,
                                        &engine.declarations,
                                        &engine.polygons,
                                        &[],
                                    )
                                    .unwrap();
                            }
                            assert!(hidden_walk.hidden);
                            let mut comparable = hidden_walk.clone();
                            comparable.hidden = false;
                            assert_eq!(walking, comparable);
                        }
                        assert_ne!(walking.placement.position, [50, 70]);
                        for constrained in [false, true] {
                            for target in [
                                None,
                                Some([90, 70]),
                                Some([-1, 70]),
                                Some([90, -1]),
                                Some([-1, -1]),
                            ] {
                                let mut restored_mover = walking.clone();
                                restored_mover.walk_target = target;
                                restored_mover.constrained_walk = constrained;
                                restored_mover.hidden = true;
                                let before = restored_mover.clone();
                                // Equal actor/mover positions do not touch any state.
                                restored_mover
                                    .restore_after_splay(
                                        &resources,
                                        &engine.declarations,
                                        &engine.polygons,
                                        before.placement.position,
                                        [90, 70],
                                    )
                                    .unwrap();
                                assert_eq!(restored_mover, before);
                                restored_mover
                                    .restore_after_splay(
                                        &resources,
                                        &engine.declarations,
                                        &engine.polygons,
                                        [60, 70],
                                        [90, 70],
                                    )
                                    .unwrap();
                                let restarted = target.is_some_and(|t| !t.contains(&-1));
                                assert_eq!(restored_mover.placement.position, [60, 70]);
                                assert_eq!(restored_mover.walk_target, target);
                                assert_eq!(
                                    restored_mover.walk_serial,
                                    before.walk_serial + u64::from(restarted)
                                );
                                assert_eq!(restored_mover.hidden, !restarted);
                                assert_eq!(restored_mover.special_reel(), !restarted);
                                assert_eq!(restored_mover.constrained_walk, constrained);
                                let mut checkpoint = engine.clone();
                                checkpoint.movers.insert(1, restored_mover.clone());
                                let bytes =
                                    Snapshot::capture(&checkpoint, [7; 32]).to_bytes().unwrap();
                                assert_eq!(
                                    Snapshot::from_bytes(&bytes, [7; 32]).unwrap().state(),
                                    &checkpoint
                                );
                                let mut invalid = engine.declarations.clone();
                                invalid.movers.clear();
                                assert!(
                                    restored_mover
                                        .restore_after_splay(
                                            &resources,
                                            &invalid,
                                            &engine.polygons,
                                            [61, 70],
                                            [90, 70]
                                        )
                                        .is_err()
                                );
                                assert_eq!(restored_mover, checkpoint.movers[&1]);
                            }
                        }
                        // PLAY covers the mover without suspending its route.
                        for completion in [0u8, 1, 2, 255] {
                            let blocking = completion != 0;
                            for exit in 0..3 {
                                let mut splay_scene = scene.clone();
                                let mut code = vec![0x41, 77, 0x41, 7, 1];
                                code.extend((base + 100).to_le_bytes());
                                code.extend([
                                    0x41, 10, 0x41, 10, 2, 2, 0x41, completion, 0x4e, 71, 0x4c, 0,
                                    0,
                                ]);
                                splay_scene[32..32 + code.len()].copy_from_slice(&code);
                                let mut splay_film = film.clone();
                                splay_film.resize(400, 0);
                                for (at, word) in [
                                    (100, 24u32),
                                    (104, 1),
                                    (108, base + 32),
                                    (112, base + 320),
                                    (320, base + 200),
                                    (324, base + 200),
                                ] {
                                    splay_film[at..at + 4].copy_from_slice(&word.to_le_bytes());
                                }
                                let r = Resources(vec![&master, &splay_scene, &splay_film]);
                                let mut live = engine.clone();
                                live.processes = Scheduler::default();
                                live.master_stopped = true;
                                live.escape_events = Some(1);
                                live.movers.insert(1, walking.clone());
                                let serial = walking.walk_serial;
                                let mut caller = ScriptProcess::new(Handle((1 << 23) | 32));
                                caller.vm.escape_enabled = true;
                                caller.vm.escape_generation = Some(1);
                                let status =
                                    live.run_process(&r, None, None, None, &mut caller).unwrap();
                                assert_eq!(
                                    status,
                                    if blocking {
                                        Status::Running
                                    } else {
                                        Status::Halted
                                    }
                                );
                                if blocking {
                                    live.processes.spawn(None, caller).unwrap();
                                }
                                live.tick(&r).unwrap();
                                assert_eq!(live.splay_reels.len(), 1);
                                assert_eq!(live.actors.get(1).unwrap().position, [10, 10]);
                                if exit == 1 {
                                    live.script_escape_event().unwrap();
                                }
                                if exit == 2 {
                                    let mut replacement = walking.animation.clone();
                                    replacement.reel = Some(Handle(base + 8));
                                    replacement.object.x = 20;
                                    replacement.object.y = 30;
                                    assert!(live.claim_reel(&mut replacement).unwrap());
                                    live.actors.get_mut(1).unwrap().position = [20, 30];
                                    live.films.push(replacement);
                                }
                                for _ in 0..3 {
                                    let bytes =
                                        Snapshot::capture(&live, [7; 32]).to_bytes().unwrap();
                                    let snapshot = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                                    assert_eq!(snapshot.to_bytes().unwrap(), bytes);
                                    let mut restored = snapshot.state().clone();
                                    live.tick(&r).unwrap();
                                    restored.tick(&r).unwrap();
                                    assert_eq!(live, restored);
                                }
                                assert!(live.splay_reels.is_empty());
                                assert_eq!(live.globals[0], 77);
                                assert_eq!(live.movers[&1].walk_serial, serial + 1);
                                assert_eq!(live.movers[&1].walk_target, Some([90, 70]));
                                assert!(!live.movers[&1].hidden);
                                assert!(live.movers[&1].placement.position[0] < 50);
                            }
                        }
                        // Retain separate film coordinates until walking selects
                        // a new reel (here a path scale change).
                        for blocking in [false, true] {
                            for legacy in [false, true] {
                                let mut play_scene = scene.clone();
                                let mut code = vec![0x41, 77, 1];
                                code.extend(base.to_le_bytes());
                                code.extend([
                                    0x41,
                                    10,
                                    0x41,
                                    10,
                                    2,
                                    2,
                                    if blocking { 3 } else { 2 },
                                    0x4e,
                                    49,
                                    0x4c,
                                    0,
                                    0,
                                ]);
                                play_scene[32..32 + code.len()].copy_from_slice(&code);
                                let play_resources = Resources(vec![&master, &play_scene, &film]);
                                let mut playing = engine.clone();
                                playing.processes = Scheduler::default();
                                playing.master_stopped = true;
                                playing.movers.insert(1, walking.clone());
                                let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
                                let status = playing
                                    .run_process(&play_resources, None, None, None, &mut process)
                                    .unwrap();
                                assert_eq!(
                                    status,
                                    if blocking {
                                        Status::Running
                                    } else {
                                        Status::Halted
                                    }
                                );
                                let reel = if blocking {
                                    let reel = process.play_wait.as_ref().unwrap().1.clone();
                                    playing.processes.spawn(None, process).unwrap();
                                    reel
                                } else {
                                    playing.films[0].clone()
                                };
                                if legacy {
                                    playing.movers.get_mut(&1).unwrap().hidden = false;
                                }
                                {
                                    use sha2::{Digest, Sha256};
                                    let original =
                                        Snapshot::capture(&playing, [7; 32]).to_bytes().unwrap();
                                    let mut flagged = playing.clone();
                                    flagged.splay_reels.insert(reel.playback);
                                    let bytes =
                                        Snapshot::capture(&flagged, [7; 32]).to_bytes().unwrap();
                                    assert_eq!(
                                        u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
                                        19
                                    );
                                    let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                                    assert_eq!(decoded.state(), &flagged);
                                    assert_eq!(decoded.to_bytes().unwrap(), bytes);
                                    let (inner, ids): (Vec<u8>, Vec<u64>) =
                                        postcard::from_bytes(&bytes[80..]).unwrap();
                                    assert_eq!(inner, original);
                                    assert_eq!(ids, vec![reel.playback]);
                                    let wrap = |inner: Vec<u8>, ids: Vec<u64>, trailing: bool| {
                                        let mut payload =
                                            postcard::to_allocvec(&(inner, ids)).unwrap();
                                        if trailing {
                                            payload.push(0);
                                        }
                                        let mut bytes = bytes[..80].to_vec();
                                        bytes[44..48]
                                            .copy_from_slice(&(payload.len() as u32).to_le_bytes());
                                        bytes[48..80].copy_from_slice(&Sha256::digest(&payload));
                                        bytes.extend(payload);
                                        bytes
                                    };
                                    for bad in [
                                        wrap(inner.clone(), vec![], false),
                                        wrap(inner.clone(), vec![0], false),
                                        wrap(inner.clone(), vec![reel.playback; 2], false),
                                        wrap(
                                            inner.clone(),
                                            vec![reel.playback + 1, reel.playback],
                                            false,
                                        ),
                                        wrap(inner.clone(), vec![reel.playback + 1], false),
                                        wrap(inner, ids.clone(), true),
                                        wrap(bytes.clone(), ids, false),
                                    ] {
                                        assert!(Snapshot::from_bytes(&bad, [7; 32]).is_err());
                                    }
                                    // Cleanup belongs to a live reel, even when it
                                    // no longer owns the actor. Multiple generations
                                    // must survive together without merging them.
                                    let mut superseded = flagged.clone();
                                    superseded.reel_owners.clear();
                                    superseded.reel_escape.clear();
                                    superseded.next_playback += 1;
                                    let mut second = reel.clone();
                                    second.playback = superseded.next_playback;
                                    superseded.splay_reels.insert(second.playback);
                                    superseded.films.push(second);
                                    let saved =
                                        Snapshot::capture(&superseded, [7; 32]).to_bytes().unwrap();
                                    assert_eq!(
                                        Snapshot::from_bytes(&saved, [7; 32]).unwrap().state(),
                                        &superseded
                                    );
                                    flagged.films.push(reel.clone());
                                    assert!(
                                        Snapshot::capture(&flagged, [7; 32]).to_bytes().is_err()
                                    );
                                    flagged.films.pop();
                                    flagged.splay_reels.clear();
                                    assert_eq!(
                                        Snapshot::capture(&flagged, [7; 32]).to_bytes().unwrap(),
                                        original
                                    );
                                }
                                let start = playing.movers[&1].placement.position;
                                for _ in 0..2 {
                                    let bytes =
                                        Snapshot::capture(&playing, [7; 32]).to_bytes().unwrap();
                                    let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                                    assert_eq!(decoded.to_bytes().unwrap(), bytes);
                                    let mut restored = decoded.state().clone();
                                    playing.tick(&play_resources).unwrap();
                                    restored.tick(&play_resources).unwrap();
                                    assert_eq!(playing, restored);
                                    assert!(playing.movers[&1].hidden);
                                    assert!(playing.owns_reel(&reel));
                                    assert_eq!(playing.actors.get(1).unwrap().position, [10, 10]);
                                }
                                assert_ne!(playing.movers[&1].placement.position, start);
                                playing.polygons.active[0].definition.scales = [2, 0];
                                playing.tick(&play_resources).unwrap();
                                assert!(!playing.movers[&1].hidden);
                                assert!(!playing.owns_reel(&reel));
                                assert_eq!(playing.movers[&1].placement.scale, 2);
                                assert_eq!(playing.globals[0], 77);
                                assert_eq!(
                                    playing.actors.get(1).unwrap().position,
                                    playing.movers[&1].placement.position
                                );
                            }
                        }
                        let mut scene_restore = engine.clone();
                        scene_restore.movers.get_mut(&1).unwrap().hidden = true;
                        for film in [Handle(0), Handle(2), Handle(base)] {
                            scene_restore
                                .stand_mover(&resources, 1, [60, 70], film)
                                .unwrap();
                            assert!(scene_restore.movers[&1].hidden);
                            assert_eq!(scene_restore.movers[&1].placement.position, [60, 70]);
                            let bytes = Snapshot::capture(&scene_restore, [7; 32])
                                .to_bytes()
                                .unwrap();
                            assert_eq!(
                                Snapshot::from_bytes(&bytes, [7; 32]).unwrap().state(),
                                &scene_restore
                            );
                        }
                        let saved = scene_restore.capture_scene_movers();
                        scene_restore
                            .restore_scene_movers(&resources, &saved)
                            .unwrap();
                        assert!(!scene_restore.movers[&1].hidden);
                        let original = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
                        let mut hidden = engine.clone();
                        hidden.movers.get_mut(&1).unwrap().hidden = true;
                        let bytes = Snapshot::capture(&hidden, [7; 32]).to_bytes().unwrap();
                        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 18);
                        let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                        assert_eq!(decoded.state(), &hidden);
                        assert_eq!(decoded.to_bytes().unwrap(), bytes);
                        assert!(!decoded.state().actors.get(1).unwrap().hidden);
                        let mut invalid_identity = hidden.clone();
                        invalid_identity.movers.get_mut(&1).unwrap().placement.actor = 2;
                        assert!(
                            Snapshot::capture(&invalid_identity, [7; 32])
                                .to_bytes()
                                .is_err()
                        );
                        let (inner, ids): (Vec<u8>, Vec<i32>) =
                            postcard::from_bytes(&bytes[80..]).unwrap();
                        assert_eq!(inner, original);
                        assert_eq!(ids, vec![1]);
                        let wrap = |inner: Vec<u8>, ids: Vec<i32>, trailing: bool| {
                            let mut payload = postcard::to_allocvec(&(inner, ids)).unwrap();
                            if trailing {
                                payload.push(0);
                            }
                            let mut malformed = bytes[..80].to_vec();
                            malformed[44..48]
                                .copy_from_slice(&(payload.len() as u32).to_le_bytes());
                            malformed[48..80].copy_from_slice(&Sha256::digest(&payload));
                            malformed.extend(payload);
                            malformed
                        };
                        for malformed in [
                            wrap(inner.clone(), vec![], false),
                            wrap(inner.clone(), vec![0], false),
                            wrap(inner.clone(), vec![1, 1], false),
                            wrap(inner.clone(), vec![2, 1], false),
                            wrap(inner.clone(), vec![99], false),
                            wrap(inner.clone(), vec![1], true),
                            wrap(bytes.clone(), vec![1], false),
                        ] {
                            assert!(Snapshot::from_bytes(&malformed, [7; 32]).is_err());
                        }
                        hidden.movers.get_mut(&1).unwrap().hidden = false;
                        assert_eq!(
                            Snapshot::capture(&hidden, [7; 32]).to_bytes().unwrap(),
                            original
                        );
                    }
                    let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
                    process.polygon = Some(1);
                    engine.escape_events = Some(1);
                    process.vm.escape_enabled = true;
                    process.vm.escape_generation = Some(1);
                    if mode == 3 {
                        engine.script_escape_event().unwrap();
                    }
                    let status = engine
                        .run_process(&resources, None, None, None, &mut process)
                        .unwrap();
                    if matches!(mode, 0 | 3) {
                        assert_eq!(status, Status::Halted);
                        assert_eq!(engine.globals[0], 77);
                    } else {
                        assert_eq!(status, Status::Running);
                        engine.processes.spawn(None, process).unwrap();
                        let mut invalid = engine.clone();
                        for p in invalid.processes.states_mut() {
                            if p.tag_walk_wait.is_some() {
                                p.polygon = None;
                            }
                        }
                        assert!(Snapshot::capture(&invalid, [7; 32]).to_bytes().is_err());
                    }
                    let mut returned_while_moving = mode == 0;
                    for tick in 0..30 {
                        if tick == 1 && mode == 4 {
                            engine.script_escape_event().unwrap();
                        }
                        if tick == 1 && mode == 5 {
                            engine.movers.get_mut(&1).unwrap().stop_requested = true;
                        }
                        if tick == 1 && mode >= 6 {
                            let mut competitor = ScriptProcess::new(Handle(
                                (1 << 23) | if mode == 6 { 56 } else { 16 },
                            ));
                            engine
                                .run_process(&resources, None, None, None, &mut competitor)
                                .unwrap();
                            assert!(engine.processes.states().all(|p| p.tag_walk_wait.is_none()));
                        }
                        let bytes = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
                        let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                        assert_eq!(decoded.to_bytes().unwrap(), bytes);
                        let mut restored = decoded.state().clone();
                        engine.tick(&resources).unwrap();
                        restored.tick(&resources).unwrap();
                        assert_eq!(engine, restored);
                        let mover = &engine.movers[&1];
                        if engine.globals[0] == 77 && mover.walk_target.is_some() {
                            returned_while_moving = true;
                            assert!(
                                engine.polygons.active[1]
                                    .contains([mover.animation.object.x, mover.animation.object.y])
                            );
                        }
                    }
                    assert_eq!(
                        engine.globals[0],
                        if mode >= 6 { 0 } else { 77 },
                        "all three arguments consumed; killed owners must not continue"
                    );
                    assert_eq!(returned_while_moving, matches!(mode, 0 | 1));
                    assert!(engine.movers[&1].walk_target.is_none());
                    if mode == 7 {
                        assert_eq!(engine.movers[&1].placement.position, [20, 70]);
                    } else if !matches!(mode, 5 | 6) {
                        assert_eq!(engine.movers[&1].placement.position, [80, 70]);
                    }
                    if matches!(mode, 0..=2) {
                        assert_eq!(
                            engine.movers[&1].placement.direction,
                            if custom {
                                crate::mover::Direction::Forward
                            } else {
                                crate::mover::Direction::Right
                            }
                        );
                    }
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
    fn scripted_custom_walk_films_preserve_delay_hold_and_snapshot_continuation() {
        use crate::save::Snapshot;
        for node in [false, true] {
            for (call, hold, start_x) in [
                (87, 0, 50),
                (87, 1, 50),
                (87, 2, 50),
                (88, 0, 50),
                (91, 0, 50),
                (75, 0, 50),
                (75, 1, 50),
                (75, 0, 110),
                (75, 1, 110),
            ] {
                for (target_x, interrupt, custom) in [
                    (80, 0, true),
                    (80, 1, true),
                    (50, 0, true),
                    (50, 0, false),
                    (80, 0, false),
                    (80, 2, true),
                    (80, 3, true),
                    (80, 2, false),
                    (80, 4, true),
                    (255, 0, true), // signed byte -1 disables horizontal movement
                    (255, 0, false),
                    (255, 1, true),
                    (255, 2, true),
                    (255, 2, false),
                ] {
                    let disabled_axis = target_x == 255;
                    if disabled_axis && (call != 75 || start_x != 50) {
                        continue;
                    }
                    if interrupt == 4 && call != 88 {
                        continue;
                    }
                    let stop = interrupt == 1;
                    let (master, mut scene) = fixtures();
                    let base = 2u32 << 23;
                    let mut code = if call == 75 {
                        vec![3, 0x41, start_x, 0x41, 70, 0x41, target_x, 0x41, 70, 1]
                    } else if call == 91 {
                        vec![3, 1]
                    } else {
                        vec![3, 0x41, target_x, 0x41, 70, 1]
                    };
                    code.extend_from_slice(&(if custom { base + 64 } else { 0 }).to_le_bytes());
                    if call == 91 {
                        code.push(3); // escape disabled
                    }
                    if call == 87 {
                        code.extend([0x41, hold]);
                    }
                    code.extend([0x4e, call]);
                    if call == 88 {
                        code.extend([0x4c, 1]);
                    }
                    code.extend([0x41, 77, 0x4c, 0, 0]);
                    scene[8..8 + code.len()].copy_from_slice(&code);
                    scene[56..60].copy_from_slice(&[3, 0x4e, 74, 0]); // STOPWALK(lead)
                    scene[32..41].copy_from_slice(&[3, 0x41, 80, 0x41, 70, 2, 0x4e, 88, 0]);
                    let mut film = vec![0; 400];
                    for (at, value) in [
                        (0, 24u32),
                        (4, 1),
                        (8, base + 32),
                        (12, base + 256),
                        (32, base + 200),
                        (40, 1),
                        (64, 12),
                        (68, 1),
                        (72, base + 32),
                        (76, base + 320),
                        (200, base + 220),
                        (220, 8 | (16 << 16)),
                        (256, base + 200),
                        (260, 1),
                        (264, (-2i32) as u32),
                        (320, base + 200),
                    ] {
                        film[at..at + 4].copy_from_slice(&value.to_le_bytes());
                    }
                    let resources = Resources(vec![&master, &scene, &film]);
                    let mut engine = Engine::bootstrap(&resources).unwrap();
                    engine.globals.resize(2, 0);
                    engine.actors = crate::actor::Actors::new(1).unwrap();
                    engine.no_blocking = true;
                    if call == 75 || (call == 91 && interrupt >= 2) {
                        engine.declarations.lead = Some(crate::startup::Lead {
                            id: 1,
                            text: Handle(0),
                            tagged: false,
                        });
                        engine.control.enabled = hold == 1;
                    }
                    engine
                        .declarations
                        .movers
                        .insert(1, [[Handle(base); 12]; 10]);
                    engine.polygons = crate::polygon::Polygons::new(
                        vec![crate::polygon::Polygon {
                            kind: if node {
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
                            nodes: if node {
                                vec![[50, 70], [80, 70]]
                            } else {
                                vec![]
                            },
                            lines: if node {
                                vec![[0, 30, -2100, 0, 0, 0, 0, 0, 0, 0]]
                            } else {
                                vec![]
                            },
                            script: Handle(0),
                        }],
                        0,
                    );
                    if call == 91 {
                        let mut tag = engine.polygons.active[0].definition.clone();
                        tag.kind = crate::polygon::Kind::Tag;
                        tag.node_position = [i32::from(target_x), 70];
                        tag.film = Handle(1); // face up after arrival
                        let path = engine.polygons.active[0].definition.clone();
                        engine.polygons = crate::polygon::Polygons::new(vec![path, tag], 0);
                        let ids: Vec<_> = engine.processes.entries().map(|(id, _)| id).collect();
                        for id in ids {
                            engine.processes.state_mut(id).unwrap().polygon = Some(1);
                        }
                    }
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
                    if interrupt == 0
                        && target_x == 80
                        && (call == 75 || call == 91 || (call == 87 && hold != 2))
                    {
                        let mut competition = engine.clone();
                        let mut owner = ScriptProcess::new(Handle((1 << 23) | 8));
                        if call == 91 {
                            owner.polygon = Some(1);
                        }
                        assert_eq!(
                            competition
                                .run_process(&resources, None, None, None, &mut owner)
                                .unwrap(),
                            Status::Running
                        );
                        let owner_id = competition.processes.spawn(None, owner.clone()).unwrap();
                        let bytes = Snapshot::capture(&competition, [7; 32]).to_bytes().unwrap();
                        let mut delayed = competition.clone();
                        let mut walked = ScriptProcess::new(Handle((1 << 23) | 32));
                        delayed
                            .run_process(&resources, None, None, None, &mut walked)
                            .unwrap();
                        for _ in 0..23 {
                            delayed
                                .run_process(&resources, None, None, None, &mut walked)
                                .unwrap();
                            assert!(delayed.processes.entries().any(|(id, _)| id == owner_id));
                        }
                        delayed
                            .run_process(&resources, None, None, None, &mut walked)
                            .unwrap();
                        assert!(!delayed.processes.entries().any(|(id, _)| id == owner_id));
                        assert!(walked.walked_wait.as_ref().unwrap().serial.is_some());
                        let mut restored = Snapshot::from_bytes(&bytes, [7; 32])
                            .unwrap()
                            .state()
                            .clone();
                        for state in [&mut competition, &mut restored] {
                            let mut replacement = ScriptProcess::new(Handle((1 << 23) | 8));
                            replacement.polygon = owner.polygon;
                            state
                                .run_process(&resources, None, None, None, &mut replacement)
                                .unwrap();
                            assert!(
                                !state.processes.entries().any(|(id, _)| id == owner_id),
                                "new walk must kill the owner before its next scheduler turn"
                            );
                        }
                        assert_eq!(competition, restored);
                    }
                    if interrupt >= 2 {
                        engine.escape_events = Some(1);
                        for vm in engine.escape_contexts_mut() {
                            vm.escape_enabled = true;
                            vm.escape_generation = Some(1);
                        }
                        if interrupt == 3 {
                            engine.script_escape_event().unwrap();
                        }
                    }
                    let mut escaped = interrupt == 3;
                    let mut saw_custom = false;
                    let mut stopped_at = None;
                    for tick in 0..80 {
                        if interrupt == 4 && tick == 1 {
                            engine.script_escape_event().unwrap();
                            escaped = true;
                        }
                        if interrupt == 2
                            && !escaped
                            && tick > 0
                            && (engine.movers[&1].placement.position[0] != i32::from(start_x)
                                || (disabled_axis && tick == 5))
                            && engine.movers[&1].walk_target.is_some()
                        {
                            engine.script_escape_event().unwrap();
                            escaped = true;
                        }
                        if stop
                            && stopped_at.is_none()
                            && tick > 0
                            && (engine.movers[&1].placement.position[0] != i32::from(start_x)
                                || (disabled_axis && tick == 5))
                        {
                            stopped_at = Some(engine.movers[&1].placement.position);
                            if disabled_axis {
                                let mut stopper = ScriptProcess::new(Handle((1 << 23) | 56));
                                engine
                                    .run_process(&resources, None, None, None, &mut stopper)
                                    .unwrap();
                                assert!(
                                    engine.processes.is_empty(),
                                    "STOPWALK kills its token owner before movement advances"
                                );
                                assert_eq!(engine.movers[&1].walk_target, Some([-1, 70]));
                            } else {
                                engine.movers.get_mut(&1).unwrap().stop_requested = true;
                            }
                        }
                        let bytes = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
                        let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                        assert_eq!(decoded.to_bytes().unwrap(), bytes);
                        let mut restored = decoded.state().clone();
                        let a = engine.tick(&resources).unwrap();
                        let b = restored.tick(&resources).unwrap();
                        assert_eq!(a, b);
                        assert_eq!(
                            engine, restored,
                            "call={call},hold={hold},node={node},tick={tick}"
                        );
                        let mover = &engine.movers[&1];
                        saw_custom |= mover.last_film == Handle(base + 64);
                        if call == 88 && tick < 24 && interrupt != 3 {
                            assert_eq!(mover.placement.position, [50, 70]);
                            assert_eq!(mover.last_film, Handle(base));
                            assert_eq!(mover.walk_serial, 0);
                        }
                        if call == 87 && hold == 2 {
                            assert_eq!(engine.globals[0], 77);
                        }
                        if call == 75 && mover.walk_target.is_some() && !(disabled_axis && escaped)
                        {
                            assert!(!engine.control.enabled, "SWALK holds lead control");
                        }
                        if ((call == 87 && hold != 2) || call == 91 || call == 75)
                            && mover.walk_target.is_some()
                            && !(disabled_axis && escaped)
                        {
                            assert_eq!(engine.globals[0], 0, "blocking WALK must not return early");
                        }
                        if mover.walk_target.is_none() && engine.processes.is_empty() {
                            break;
                        }
                    }
                    let mover = &engine.movers[&1];
                    if disabled_axis {
                        assert_eq!(mover.placement.position, [50, 70]);
                        assert_eq!(mover.walk_target, if stop { None } else { Some([-1, 70]) });
                        assert_eq!(mover.walk_serial, 1 + u64::from(stop));
                        assert_eq!(engine.globals[0], if escaped { 77 } else { 0 });
                        assert_eq!(engine.control.enabled, escaped && hold == 1);
                        if !stop {
                            assert_eq!(
                                mover.last_film,
                                Handle(if custom { base + 64 } else { base })
                            );
                            assert_eq!(engine.processes.is_empty(), escaped);
                        }
                        continue;
                    }
                    assert_eq!(
                        saw_custom,
                        custom && target_x != start_x && !matches!(interrupt, 3 | 4)
                    );
                    assert_eq!(mover.walk_target, None);
                    assert_eq!(
                        mover.placement.position,
                        if call == 75 && interrupt == 3 {
                            [50, 70]
                        } else {
                            stopped_at.unwrap_or([i32::from(target_x), 70])
                        }
                    );
                    assert_eq!(mover.last_film, Handle(base));
                    assert_eq!(
                        mover.walk_serial,
                        if interrupt == 3 {
                            0
                        } else {
                            1 + u64::from(stop)
                        }
                    );
                    assert_eq!(
                        engine.globals[0],
                        if stop && ((call == 87 && hold != 2) || call == 91 || call == 75) {
                            0
                        } else {
                            77
                        }
                    );
                    if call == 75 {
                        assert_eq!(engine.control.enabled, hold == 1 && !stop);
                    }
                    if call == 91 && !stop {
                        assert_eq!(mover.placement.direction, crate::mover::Direction::Away);
                    }
                    if call == 88 {
                        assert_eq!(
                            engine.globals[1],
                            i32::from(!stop && target_x != 50),
                            "SetActorDest returns zero when no walk was needed"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn escaped_films_release_primary_and_secondary_reels_with_saved_ownership() {
        for opcode in [49, 81] {
            for blocking in [0, 1] {
                // 0: unescapable, 1: escape during playback, 2: already escaped,
                // 3: duplicate request disables escape without restarting.
                // 4: replacement playback survives the old owner's escape.
                // 5: null film remains invalid even after escape.
                for mode in 0..6 {
                    let (master, mut scene) = fixtures();
                    let base = 2u32 << 23;
                    let handle = match mode {
                        2 => u32::MAX,
                        5 => 0,
                        _ => base,
                    };
                    let mut code = vec![1];
                    code.extend_from_slice(&handle.to_le_bytes());
                    code.extend([
                        4, 4, 2, 2, 0x41, blocking, 0x4e, opcode, 0x41, 77, 0x4c, 0, 0,
                    ]);
                    scene[8..8 + code.len()].copy_from_slice(&code);
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
                        (80, 10),
                        (84, 1),
                        (88, (-2i32) as u32),
                    ] {
                        film[at..at + 4].copy_from_slice(&value.to_le_bytes());
                    }
                    let resources = Resources(vec![&master, &scene, &film]);
                    let mut engine = Engine::bootstrap(&resources).unwrap();
                    engine.actors = crate::actor::Actors::new(2).unwrap();
                    engine.escape_events = Some(1);
                    if mode != 0 {
                        for vm in engine.escape_contexts_mut() {
                            vm.escape_enabled = true;
                            vm.escape_generation = Some(1);
                        }
                    }
                    if matches!(mode, 2 | 5) {
                        engine.script_escape_event().unwrap();
                    }
                    if mode == 5 {
                        assert_eq!(engine.tick(&resources), Err(Error("null film handle")));
                        assert!(engine.reel_owners.is_empty());
                        continue;
                    }
                    engine.tick(&resources).unwrap();
                    if mode == 2 {
                        assert!(engine.reel_owners.is_empty());
                        assert_eq!(engine.globals[0], 77);
                        continue;
                    }
                    assert_eq!(engine.reel_owners.len(), 2);
                    assert_eq!(engine.reel_escape.len(), if mode == 0 { 0 } else { 2 });
                    if mode == 3 {
                        let owners = engine.reel_owners.clone();
                        let mut duplicate = ScriptProcess::new(Handle((1 << 23) | 8));
                        engine
                            .run_process(&resources, None, None, None, &mut duplicate)
                            .unwrap();
                        assert_eq!(engine.reel_owners, owners);
                        assert!(engine.reel_escape.is_empty());
                    }
                    if mode == 4 {
                        let mut replacement = engine
                            .films
                            .iter()
                            .find(|a| a.object.id == 2)
                            .unwrap()
                            .clone();
                        replacement.reel = Some(Handle(base + 100));
                        assert!(engine.claim_reel(&mut replacement).unwrap());
                        engine.films.push(replacement);
                    }
                    let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
                        .to_bytes()
                        .unwrap();
                    if mode == 1 {
                        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 15);
                    }
                    if mode == 1 {
                        use sha2::{Digest, Sha256};
                        type ReelCaptures = Vec<(i32, (u64, u64))>;
                        let (inner, captures): (Vec<u8>, ReelCaptures) =
                            postcard::from_bytes(&bytes[80..]).unwrap();
                        let wrap = |inner: Vec<u8>, captures: Vec<(i32, (u64, u64))>| {
                            let payload = postcard::to_allocvec(&(inner, captures)).unwrap();
                            let mut bad = bytes[..80].to_vec();
                            bad[44..48].copy_from_slice(&(payload.len() as u32).to_le_bytes());
                            bad[48..80].copy_from_slice(&Sha256::digest(&payload));
                            bad.extend(payload);
                            bad
                        };
                        for bad in [
                            wrap(inner.clone(), vec![]),
                            wrap(inner.clone(), vec![captures[0], captures[0]]),
                            wrap(inner.clone(), vec![(99, captures[0].1)]),
                            wrap(inner.clone(), vec![(1, (u64::MAX, 1))]),
                            wrap(inner.clone(), vec![(1, (captures[0].1.0, 2))]),
                            wrap(bytes.clone(), captures.clone()),
                        ] {
                            assert!(crate::save::Snapshot::from_bytes(&bad, [7; 32]).is_err());
                        }
                    }
                    let decoded = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                    assert_eq!(decoded.to_bytes().unwrap(), bytes);
                    let mut restored = decoded.state().clone();
                    engine.script_escape_event().unwrap();
                    restored.script_escape_event().unwrap();
                    engine.tick(&resources).unwrap();
                    restored.tick(&resources).unwrap();
                    assert_eq!(engine, restored);
                    if mode == 1 {
                        assert!(engine.reel_owners.is_empty());
                        assert!(engine.reel_escape.is_empty());
                        assert!(engine.films.is_empty());
                        assert!(engine.processes.states().all(|p| p.play_wait.is_none()));
                        assert_eq!(engine.globals[0], 77);
                    } else if mode == 4 {
                        assert_eq!(engine.reel_owners.len(), 1);
                        assert!(engine.reel_owners.contains_key(&2));
                        assert!(engine.reel_escape.is_empty());
                    } else {
                        assert_eq!(engine.reel_owners.len(), 2);
                    }
                }
            }
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
    fn returned_control_does_not_end_background_transition() {
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
        engine.enter(&resources, Handle(1 << 23), 1, 0).unwrap();
        assert!(engine.scene_transition_active());
        engine.start_background(&resources, Handle(base)).unwrap();
        engine.control.apply(1, false).unwrap();
        assert_eq!(engine.fade_multiplier, 0);
        assert!(engine.control.enabled);
        assert!(
            engine.scene_transition_active(),
            "do not pause on black frame"
        );
        for _ in 0..5 {
            engine.tick(&resources).unwrap();
            assert!(engine.scene_transition_active());
        }
        engine.tick(&resources).unwrap();
        assert!(engine.control.enabled);
        assert!(!engine.scene_transition_active());
        assert_eq!(engine.fade_multiplier, 0x10000);
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
    fn splay_checks_stale_escape_before_null_film() {
        let (master, mut scene) = fixtures();
        let code = [0x41, 77, 3, 2, 2, 2, 2, 2, 3, 0x4e, 71, 0x4c, 0, 0];
        scene[32..32 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        for escaped in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.escape_events = Some(if escaped { 2 } else { 1 });
            let mut caller = ScriptProcess::new(Handle((1 << 23) | 32));
            caller.vm.escape_enabled = true;
            caller.vm.escape_generation = Some(1);
            let result = engine.run_process(&resources, None, None, None, &mut caller);
            if escaped {
                assert_eq!(result, Ok(Status::Halted));
                assert_eq!(engine.globals[0], 77);
            } else {
                assert_eq!(result, Err(Error("null film handle")));
            }
            assert!(engine.splay_reels.is_empty());
            assert!(engine.films.is_empty());
        }
    }

    #[test]
    fn v1_diagnostic_calls_and_fademidi_preserve_stack_and_wait_once() {
        use crate::save::Snapshot;
        for master_context in [false, true] {
            for direction in [0u8, 1, 2, 255] {
                let (master, mut scene) = fixtures();
                let code = [
                    0x41, 77, 0x4e, 69, 0x4e, 70, 0x4e, 84, 0x41, direction, 0x4e, 106, 0x4c, 0, 0,
                ];
                scene[8..8 + code.len()].copy_from_slice(&code);
                let resources = Resources(vec![&master, &scene]);
                let mut engine = Engine::bootstrap(&resources).unwrap();
                engine.processes = Scheduler::default();
                engine.master_stopped = !master_context;
                engine.escape_events = Some(2);
                let mut process = ScriptProcess::new(Handle((1 << 23) | 8));
                process.vm.escape_enabled = true;
                process.vm.escape_generation = Some(1); // FADEMIDI ignores stale Escape.
                if master_context {
                    engine.master = process.vm;
                    engine.master_code = process.code;
                } else {
                    engine.processes.spawn(None, process).unwrap();
                }
                if direction > 1 {
                    let before = engine.clone();
                    assert_eq!(
                        engine.tick(&resources),
                        Err(Error("invalid MIDI fade direction"))
                    );
                    assert_eq!(engine, before);
                    continue;
                }
                engine.tick(&resources).unwrap();
                assert_eq!(engine.globals[0], 0);
                let (pending, stack) = if master_context {
                    (engine.master_wait, engine.master.operands())
                } else {
                    let process = engine.processes.states().next().unwrap();
                    (process.wait, process.vm.operands())
                };
                assert_eq!(pending.map(|(call, left)| (call.id, left)), Some((106, 1)));
                assert_eq!(stack, &[77, i32::from(direction)]);
                let bytes = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
                let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                assert_eq!(decoded.to_bytes().unwrap(), bytes);
                let mut restored = decoded.state().clone();
                engine.script_escape_event().unwrap();
                restored.script_escape_event().unwrap();
                engine.tick(&resources).unwrap();
                restored.tick(&resources).unwrap();
                assert_eq!(engine, restored);
                assert_eq!(engine.globals[0], 77);
            }
        }
    }

    #[test]
    fn pause_preserves_stack_waits_two_resumptions_and_restores_escape() {
        use crate::save::Snapshot;
        let (master, mut scene) = fixtures();
        scene[8..15].copy_from_slice(&[0x41, 77, 0x4e, 48, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        for master_context in [false, true] {
            for scenario in 0..4 {
                let mut engine = Engine::bootstrap(&resources).unwrap();
                engine.processes = Scheduler::default();
                engine.master_stopped = !master_context;
                engine.escape_events = Some(1);
                let mut process = ScriptProcess::new(Handle((1 << 23) | 8));
                process.vm.escape_enabled = scenario != 3;
                process.vm.escape_generation = (scenario != 3).then_some(1);
                if master_context {
                    engine.master = process.vm;
                    engine.master_code = process.code;
                } else {
                    engine.processes.spawn(None, process).unwrap();
                }
                if scenario == 1 {
                    engine.script_escape_event().unwrap();
                }
                let finished_at = match scenario {
                    1 => 1,
                    2 => 2,
                    _ => 3,
                };
                for tick in 1..=3 {
                    if tick == 2 && matches!(scenario, 2 | 3) {
                        engine.script_escape_event().unwrap();
                    }
                    let bytes = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
                    let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                    assert_eq!(decoded.to_bytes().unwrap(), bytes);
                    let mut restored = decoded.state().clone();
                    engine.tick(&resources).unwrap();
                    restored.tick(&resources).unwrap();
                    assert_eq!(engine, restored);
                    assert_eq!(engine.globals[0], if tick >= finished_at { 77 } else { 0 });
                    if tick == 1 && scenario != 1 {
                        let pending = if master_context {
                            engine.master_wait
                        } else {
                            engine.processes.states().next().unwrap().wait
                        };
                        assert_eq!(pending.map(|(call, left)| (call.id, left)), Some((48, 2)));
                    }
                }
            }
        }
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
    fn escaped_frame_and_key_waits_complete_at_entry_and_after_restore() {
        for opcode in [85, 100] {
            for at_entry in [false, true] {
                for enabled in [false, true] {
                    let (master, mut scene) = fixtures();
                    let mut code = vec![0x41, 77];
                    if opcode == 100 {
                        code.extend([3, 0x41, 20]);
                    }
                    code.extend([0x4e, opcode, 0x4c, 0, 0]);
                    scene[8..8 + code.len()].copy_from_slice(&code);
                    let resources = Resources(vec![&master, &scene]);
                    let mut engine = Engine::bootstrap(&resources).unwrap();
                    engine.escape_events = Some(1);
                    for vm in engine.escape_contexts_mut() {
                        vm.escape_enabled = enabled;
                        vm.escape_generation = enabled.then_some(1);
                    }
                    if at_entry {
                        engine.script_escape_event().unwrap();
                    }
                    engine.tick(&resources).unwrap();
                    assert_eq!(engine.globals[0], if at_entry && enabled { 77 } else { 0 });
                    if !at_entry {
                        engine.script_escape_event().unwrap();
                    }
                    let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
                        .to_bytes()
                        .unwrap();
                    let snapshot = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                    assert_eq!(snapshot.to_bytes().unwrap(), bytes);
                    let mut restored = snapshot.state().clone();
                    engine.tick(&resources).unwrap();
                    restored.tick(&resources).unwrap();
                    assert_eq!(engine, restored);
                    // Suspended WAITKEY observes Escape as an ordinary input
                    // even with ESCOFF. WAITFRAME requires an enabled capture.
                    let done = enabled || (opcode == 85 && !at_entry);
                    assert_eq!(engine.globals[0], if done { 77 } else { 0 });
                    assert!(engine.actor_steps.is_empty());
                }
            }
        }
    }

    #[test]
    fn talkattr_matches_v1_color_casts_and_escape_guard() {
        for (rgb, expected) in [
            ([12i32, 34, 56], [12, 34, 56]),
            ([300, 17, 32], [255, 17, 32]),
            ([-1, -2, 0], [255, 254, 255]),
        ] {
            for escaped in [false, true] {
                let (master, mut scene) = fixtures();
                let mut code = vec![0x41, 77];
                for component in rgb {
                    code.push(1);
                    code.extend(component.to_le_bytes());
                }
                code.extend([0x4e, 78, 0x4c, 0, 0]);
                scene[8..8 + code.len()].copy_from_slice(&code);
                let resources = Resources(vec![&master, &scene]);
                let mut engine = Engine::bootstrap(&resources).unwrap();
                engine.talk_color = Some([9, 8, 7]);
                engine.escape_events = Some(1);
                for vm in engine.escape_contexts_mut() {
                    vm.escape_enabled = true;
                    vm.escape_generation = Some(1);
                }
                if escaped {
                    engine.script_escape_event().unwrap();
                }
                engine.tick(&resources).unwrap();
                assert_eq!(engine.globals[0], 77);
                assert_eq!(
                    engine.talk_color,
                    Some(if escaped { [9, 8, 7] } else { expected })
                );
                let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
                    .to_bytes()
                    .unwrap();
                let loaded = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                assert_eq!(loaded.state(), &engine);
                assert_eq!(loaded.to_bytes().unwrap(), bytes);
            }
        }
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
    fn escaped_inventory_text_preserves_arbitration_delay_and_audio() {
        for pointed in [false, true] {
            for after_delay in [false, true] {
                if pointed && after_delay {
                    continue;
                }
                let (master, mut scene) = fixtures();
                scene[8] = 0;
                scene[32..40].copy_from_slice(&[0x41, 77, 3, 0x4e, 54, 0x4c, 0, 0]);
                let resources = Resources(vec![&master, &scene]);
                let index = [0, 0, 0, 0, 4, 0, 0, 0];
                let mut samples = vec![0; 8];
                samples[4..8].copy_from_slice(&10000u32.to_le_bytes());
                samples.extend([128; 10000]);
                let bank = SampleBank::new(&index, &samples).unwrap();
                let mut engine = Engine::bootstrap(&resources).unwrap();
                engine.active_inventory = Some(2);
                engine.declarations.windows[2] = Some(crate::startup::Window {
                    title: Handle(0),
                    capacity: 10,
                    min: [1, 1],
                    size: [1, 1],
                    max: [1, 1],
                    position: [94, 100],
                    first_display: 0,
                    contents: vec![284],
                });
                engine.control.cursor = [112, 132];
                engine.inventory_hover.monitor.suppress();
                engine.sample_player.play_speech(&bank, 1).unwrap();
                engine.escape_events = Some(1);
                let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
                process.inventory_object = Some(284);
                process.event = if pointed { 0 } else { 3 };
                process.vm.escape_enabled = true;
                process.vm.escape_generation = Some(1);
                engine
                    .processes
                    .spawn(Some(engine.generation), process)
                    .unwrap();
                if !after_delay {
                    engine.script_escape_event().unwrap();
                }
                let audio = engine.sample_player.clone();
                let mut control = engine.control.clone();
                // No font, strings or sample bank: skipping must not need them.
                engine.tick(&resources).unwrap();
                assert_eq!(engine.sample_player, audio);
                assert_eq!(engine.control, control);
                if !pointed {
                    assert_eq!(engine.globals[0], 0);
                    assert!(engine.inventory_hover.monitor.suppressed());
                    assert!(
                        engine
                            .processes
                            .states()
                            .any(|p| p.object_print_position.is_some())
                    );
                    if after_delay {
                        engine.script_escape_event().unwrap();
                        control = engine.control.clone();
                    }
                    let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
                        .to_bytes()
                        .unwrap();
                    let snapshot = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                    assert_eq!(snapshot.to_bytes().unwrap(), bytes);
                    let mut restored = snapshot.state().clone();
                    engine.tick(&resources).unwrap();
                    restored.tick(&resources).unwrap();
                    assert_eq!(engine, restored);
                }
                assert_eq!(engine.globals[0], 77);
                assert!(!engine.inventory_hover.monitor.suppressed());
                assert_eq!(engine.sample_player, audio);
                assert_eq!(engine.control, control);
                assert!(engine.processes.states().all(|p| p.print_wait.is_none()));
            }
        }
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
            engine.declarations.windows[2] = Some(crate::startup::Window {
                title: Handle(0),
                capacity: 10,
                min: [1, 1],
                size: [1, 1],
                max: [1, 1],
                position: [94, 100],
                first_display: 0,
                contents: vec![284],
            });
            engine.control.cursor = [112, 132];
            engine
                .processes
                .spawn(Some(engine.generation), process)
                .unwrap();
            engine
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap();
            // PRINTOBJ obtains its anchor from declarations without requiring
            // a current rendered frame, then yields before taking control.
            assert_eq!(engine.control.enabled, initially_enabled);
            assert!(engine.sample_player.id.is_none());
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
    fn zero_voice_volume_uses_text_timing_and_does_not_replace_effects() {
        let (master, mut scene) = fixtures();
        scene[8..18].copy_from_slice(&[2, 2, 2, 3, 2, 0x41, 2, 0x4e, 53, 0]);
        let base = 2u32 << 23;
        let mut font = vec![0; 1120];
        font[24..28].copy_from_slice(&1u32.to_le_bytes());
        let glyph = 44 + usize::from(b'W') * 4;
        font[glyph..glyph + 4].copy_from_slice(&(base + 1100).to_le_bytes());
        font[1100..1108].copy_from_slice(&[4, 0, 6, 0, 0, 0, 0, 0]);
        let resources = Resources(vec![&master, &scene, &font]);
        let strings = crate::text::Strings::new(&[1, 0, 52, 51, 0, 0, 0, 0, 0, 1, b'W']).unwrap();
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let mut samples = vec![0; 8];
        samples[4..8].copy_from_slice(&1000u32.to_le_bytes());
        samples.extend([129; 1000]);
        let bank = SampleBank::new(&index, &samples).unwrap();
        for volume in [0, 255] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.subtitles = false;
            engine.declarations.talk_font = Some(Handle(base));
            engine.configuration = Some(
                crate::menu_controller::Controller::new(crate::menu_controller::Settings {
                    volumes: [255, 255, volume],
                    double_click: 10,
                    swap_buttons: false,
                    text_speed: 0,
                    subtitles: false,
                    flipped: false,
                })
                .unwrap(),
            );
            if volume == 0 {
                let mut ambient = engine.clone();
                ambient.sample_player.play(&bank, 1).unwrap();
                ambient
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                assert_eq!(ambient.sample_player.serial, 1);
                assert_eq!(ambient.sample_player.position, 918);
                assert!(!ambient.sample_player.is_speech());
            }
            assert_eq!(
                engine
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap(),
                Status::Running
            );
            let wait = engine
                .processes
                .states()
                .find_map(|p| p.print_wait.as_ref())
                .unwrap();
            assert_eq!(
                wait.layout.is_some(),
                volume == 0,
                "disabled voice forces text even with subtitles off"
            );
            assert_eq!(wait.sample_serial.is_some(), volume != 0);
            assert_eq!(engine.sample_player.is_speech(), volume != 0);
            let mut status = Status::Running;
            for _ in 0..30 {
                status = engine
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                if status == Status::Halted {
                    break;
                }
            }
            assert_eq!(status, Status::Halted);
            assert_eq!(engine.ticks, if volume == 0 { 27 } else { 3 });
        }
    }

    #[test]
    fn escaped_print_and_positioned_speech_preserve_replacement_audio() {
        for opcode in [53, 98, 104] {
            for at_entry in [false, true] {
                for mode in 0..3 {
                    let (master, mut scene) = fixtures();
                    let mut code = vec![0x41, 77];
                    if opcode == 53 {
                        // Timed, sustained PRINT cannot be dismissed by clicks.
                        code.extend([2, 2, 2, 3, 0x41, 5, 0x41, 2]);
                    } else {
                        code.extend([4, 2, 2, 3]);
                        if opcode == 104 {
                            code.extend([0x41, 2]);
                        }
                    }
                    code.extend([0x4e, opcode, 0x4c, 0, 0]);
                    scene[8..8 + code.len()].copy_from_slice(&code);
                    let resources = Resources(vec![&master, &scene]);
                    let strings =
                        crate::text::Strings::new(&[1, 0, 52, 51, 0, 0, 0, 0, 0, 1, b'W']).unwrap();
                    let index = [0, 0, 0, 0, 4, 0, 0, 0];
                    let mut samples = vec![0; 8];
                    samples[4..8].copy_from_slice(&5000u32.to_le_bytes());
                    samples.extend(vec![129; 5000]);
                    let bank = SampleBank::new(&index, &samples).unwrap();
                    let mut engine = Engine::bootstrap(&resources).unwrap();
                    engine.subtitles = false;
                    engine.escape_events = Some(1);
                    for vm in engine.escape_contexts_mut() {
                        vm.escape_enabled = true;
                        vm.escape_generation = Some(1);
                    }
                    if at_entry {
                        engine.sample_player.play_speech(&bank, 1).unwrap();
                        let before = engine.sample_player.clone();
                        engine.script_escape_event().unwrap();
                        // Missing strings, bank and actor/font resources must
                        // not be accessed by an already-escaped text call.
                        engine.tick(&resources).unwrap();
                        assert_eq!(engine.sample_player, before);
                        assert_eq!(engine.globals[0], 77);
                        assert!(engine.processes.states().all(|p| p.print_wait.is_none()));
                        continue;
                    }
                    engine
                        .tick_content(&resources, Some(&bank), None, Some(&strings))
                        .unwrap();
                    for p in engine.processes.states_mut() {
                        if let Some(wait) = &mut p.print_wait {
                            // Isolate escape from the ordinary left-click path.
                            wait.left_events = 0;
                            if mode == 2 {
                                wait.sample_serial = None;
                                wait.layout = Some(crate::text::Layout { glyphs: vec![] });
                            }
                        }
                    }
                    if mode == 1 {
                        engine.sample_player.play_speech(&bank, 1).unwrap();
                    }
                    if mode == 2 {
                        engine.sample_player.stop();
                    }
                    let before = engine.sample_player.clone();
                    let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
                        .to_bytes()
                        .unwrap();
                    let snapshot = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                    assert_eq!(snapshot.to_bytes().unwrap(), bytes);
                    let mut restored = snapshot.state().clone();
                    if mode != 1 {
                        let mut ordinary = engine.clone();
                        ordinary.tick(&resources).unwrap();
                        assert_eq!(ordinary.globals[0], 0);
                        for vm in ordinary.escape_contexts_mut() {
                            vm.escape_enabled = false;
                            vm.escape_generation = None;
                        }
                        ordinary.script_escape_event().unwrap();
                        ordinary.tick(&resources).unwrap();
                        assert_eq!(ordinary.globals[0], 0);
                    }
                    engine.script_escape_event().unwrap();
                    restored.script_escape_event().unwrap();
                    engine.tick(&resources).unwrap();
                    restored.tick(&resources).unwrap();
                    assert_eq!(engine, restored);
                    assert_eq!(engine.globals[0], 77);
                    assert!(engine.processes.states().all(|p| p.print_wait.is_none()));
                    if mode == 0 {
                        let mut stopped = before;
                        stopped.stop();
                        assert_eq!(engine.sample_player, stopped);
                    } else {
                        assert_eq!(engine.sample_player, before);
                    }
                }
            }
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
        samples.extend([129; 1000]);
        let bank = SampleBank::new(&index, &samples).unwrap();
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.subtitles = false;
        assert_eq!(
            engine
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap(),
            Status::Running
        );
        assert!(engine.sample_player.is_speech());
        let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
            .to_bytes()
            .unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 12);
        let decoded = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
        assert_eq!(decoded.to_bytes().unwrap(), bytes);
        assert_eq!(decoded.state(), &engine);
        for (speech, legacy) in [(false, false), (true, false), (true, true)] {
            for voice_volume in [0, 255] {
                let mut mixed = engine.clone();
                mixed.configuration = Some(
                    crate::menu_controller::Controller::new(crate::menu_controller::Settings {
                        volumes: [255, 255 - voice_volume, voice_volume],
                        double_click: 10,
                        swap_buttons: false,
                        text_speed: 0,
                        subtitles: false,
                        flipped: false,
                    })
                    .unwrap(),
                );
                if !speech {
                    mixed.sample_player.play(&bank, 1).unwrap();
                }
                if legacy {
                    // Old envelopes contain only SamplePlayer's base fields.
                    mixed.sample_player =
                        postcard::from_bytes(&postcard::to_allocvec(&mixed.sample_player).unwrap())
                            .unwrap();
                    assert!(!mixed.sample_player.is_speech());
                }
                let saved = crate::save::Snapshot::capture(&mixed, [7; 32])
                    .to_bytes()
                    .unwrap();
                let mut resumed = crate::save::Snapshot::from_bytes(&saved, [7; 32])
                    .unwrap()
                    .restore_with_strings(&resources, &bank, &[], &strings)
                    .unwrap();
                mixed
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                resumed
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                assert_eq!(resumed, mixed);
                let volume = if speech {
                    voice_volume
                } else {
                    255 - voice_volume
                };
                assert_eq!(mixed.audio_frame[0], if volume == 0 { 0 } else { 256 });
                assert_eq!(mixed.sample_player.position, 919);
            }
        }
        let mut slow = engine.clone();
        slow.configuration = Some(
            crate::menu_controller::Controller::new(crate::menu_controller::Settings {
                volumes: [255; 3],
                double_click: 10,
                swap_buttons: false,
                text_speed: 100,
                subtitles: true,
                flipped: false,
            })
            .unwrap(),
        );
        let id = slow
            .processes
            .entries()
            .find(|(_, p)| p.print_wait.is_some())
            .unwrap()
            .0;
        slow.processes
            .state_mut(id)
            .unwrap()
            .print_wait
            .as_mut()
            .unwrap()
            .layout = Some(crate::text::Layout { glyphs: vec![] });
        for _ in 0..2 {
            assert_eq!(
                slow.tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap(),
                Status::Running
            );
            assert_eq!(
                slow.processes
                    .state_mut(id)
                    .unwrap()
                    .print_wait
                    .as_ref()
                    .unwrap()
                    .remaining,
                25,
                "PRINT retains its captured duration throughout speech"
            );
        }
        assert!(
            slow.processes
                .state_mut(id)
                .unwrap()
                .print_wait
                .as_ref()
                .unwrap()
                .sample_serial
                .is_none()
        );
        for _ in 0..25 {
            assert_eq!(
                slow.tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap(),
                Status::Running
            );
        }
        assert_eq!(
            slow.tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap(),
            Status::Halted
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
    fn implicit_film_position_uses_mover_animation_or_uncreated_zero() {
        let (master, scene) = fixtures();
        let base = 2u32 << 23;
        let mut film = vec![0; 64];
        for (at, word) in [
            (0, 24u32),
            (4, 1),
            (8, base + 24),
            (12, base + 48),
            (32, 1),
            (48, 10),
        ] {
            film[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(1).unwrap();
        engine.actors.get_mut(1).unwrap().position = [90, 100];
        let position = |engine: &Engine, at| {
            let reels = engine
                .prepare_actor_film(&resources, Handle(base), at)
                .unwrap();
            assert_eq!(
                reels[0].source.as_ref().unwrap().position,
                [reels[0].object.x, reels[0].object.y]
            );
            [reels[0].object.x, reels[0].object.y]
        };
        assert_eq!(position(&engine, [-1, -1]), [90, 100]);
        engine
            .declarations
            .movers
            .insert(1, [[Handle(base); 12]; 10]);
        assert_eq!(position(&engine, [-1, -1]), [0, 0]);
        let placement = crate::mover::Placement::initial(
            &engine.declarations,
            &engine.polygons,
            1,
            [40, 50],
            Handle(base),
        )
        .unwrap();
        let mut mover = crate::mover::Mover::new(&resources, placement).unwrap();
        mover.animation.object.x = 45;
        mover.animation.object.y = 55;
        engine.movers.insert(1, mover);
        assert_eq!(position(&engine, [-1, -1]), [45, 55]);
        assert_eq!(position(&engine, [1, -1]), [1, -1]);
        assert_eq!(engine.actors.get(1).unwrap().position, [90, 100]);
        assert_eq!(engine.movers[&1].placement.position, [40, 50]);
    }

    #[test]
    fn splay_secondary_mover_cleanup_outlives_primary_and_restores_independently() {
        use crate::save::Snapshot;
        let (master, mut scene) = fixtures();
        let code = [
            3, 1, 0, 0, 0, 1, 0x41, 12, 0x41, 13, 2, 2, 3, 0x4e, 71, 0x41, 7, 0x4c, 0, 0,
        ];
        scene[8..8 + code.len()].copy_from_slice(&code);
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
        for exit in 0..3 {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.actors = crate::actor::Actors::new(2).unwrap();
            engine.escape_events = Some(1);
            for process in engine.processes.states_mut() {
                process.vm.escape_enabled = true;
                process.vm.escape_generation = Some(1);
            }
            for actor in [1, 2] {
                engine
                    .declarations
                    .movers
                    .insert(actor, [[Handle(base); 12]; 10]);
                let placement = crate::mover::Placement::initial(
                    &engine.declarations,
                    &engine.polygons,
                    actor,
                    [100 + actor * 10, 100],
                    Handle(base),
                )
                .unwrap();
                engine.movers.insert(
                    actor,
                    crate::mover::Mover::new(&resources, placement).unwrap(),
                );
            }
            for tick in 1..=3 {
                let bytes = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
                let snapshot = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                assert_eq!(snapshot.to_bytes().unwrap(), bytes);
                let mut restored = snapshot.state().clone();
                engine.tick(&resources).unwrap();
                restored.tick(&resources).unwrap();
                assert_eq!(engine, restored);
                assert_eq!(engine.globals[0], if tick == 3 { 7 } else { 0 });
                assert_eq!(engine.splay_reels.len(), if tick == 3 { 1 } else { 2 });
            }
            assert_eq!(engine.movers[&1].placement.position, [12, 13]);
            assert!(!engine.movers[&1].hidden);
            assert_eq!(engine.movers[&2].placement.position, [120, 100]);
            assert!(engine.movers[&2].hidden);
            match exit {
                0 => engine.script_escape_event().unwrap(),
                1 => {
                    let mut replacement = engine.films[0].clone();
                    replacement.reel = Some(Handle(base + 8));
                    replacement.object.x = 20;
                    replacement.object.y = 21;
                    assert!(engine.claim_reel(&mut replacement).unwrap());
                    engine.actors.get_mut(2).unwrap().position = [20, 21];
                    engine.films.push(replacement);
                    assert_eq!(engine.films.len(), 2, "superseded SPLAY retains cleanup");
                }
                _ => engine.disable_actor(2).unwrap(),
            }
            let bytes = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
            let mut restored = Snapshot::from_bytes(&bytes, [7; 32])
                .unwrap()
                .state()
                .clone();
            engine.tick(&resources).unwrap();
            restored.tick(&resources).unwrap();
            assert_eq!(engine, restored);
            assert!(engine.splay_reels.is_empty());
            match exit {
                0 => {
                    assert_eq!(engine.movers[&2].placement.position, [5, 0]);
                    assert!(!engine.movers[&2].hidden);
                }
                1 => {
                    assert_eq!(engine.movers[&2].placement.position, [20, 21]);
                    assert!(
                        engine.movers[&2].hidden,
                        "replacement keeps its mover hidden"
                    );
                    assert!(engine.reel_owners.contains_key(&2));
                }
                _ => assert!(!engine.movers.contains_key(&2)),
            }
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
    fn actorson_revives_scene_actors_on_future_entries_only() {
        let (master, mut scene) = fixtures();
        // ACTORSON takes no arguments and must preserve the lower stack.
        scene[8..16].copy_from_slice(&[0x41, 42, 0x4e, 93, 0x4c, 0, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(2).unwrap();
        engine.actors.disable(1).unwrap();
        engine.actors.disable(2).unwrap();
        let definition = scene::Actor {
            id: 1,
            masking: 0,
            script: Handle((1 << 23) | 32),
        };
        assert!(
            engine
                .actors
                .enter(std::slice::from_ref(&definition))
                .unwrap()
                .is_empty()
        );
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.globals, [42]);
        assert!(!engine.actors.get(1).unwrap().alive);
        let mut restored = engine.clone();
        for state in [&mut engine, &mut restored] {
            assert!(
                state
                    .actors
                    .enter_with_startup(std::slice::from_ref(&definition), false)
                    .unwrap()
                    .is_empty()
            );
            assert!(!state.actors.get(1).unwrap().alive);
            for _ in 0..2 {
                assert_eq!(
                    state
                        .actors
                        .enter(std::slice::from_ref(&definition))
                        .unwrap(),
                    [(1, definition.script)]
                );
                assert!(state.actors.get(1).unwrap().alive);
                assert!(!state.actors.get(2).unwrap().alive);
                state.actors.disable(1).unwrap();
            }
        }
        assert_eq!(engine, restored);
    }
    #[test]
    fn cursor_queries_restore_world_position_while_control_is_frozen() {
        let (master, mut scene) = fixtures();
        let code = [
            0x50, 1, 0x4e, 17, 0x4c, 0, // reserve return slot, CURSORXPOS, store
            0x50, 1, 0x4e, 18, 0x4c, 1, // CURSORYPOS
            2, 2, 0x4e, 43, // temporarily move to world [0, 0]
            0x4a, 0, 0x4a, 1, 0x4e, 43, // restore the queried position
            3, 0x4e, 14, 0, // CONTROL_ON
        ];
        scene[8..8 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.globals.resize(2, 0);
        engine.world_origin = [320, -10];
        engine.control.cursor = [23, 34];
        engine
            .control
            .apply_at(2, false, engine.world_origin)
            .unwrap();
        let mut restored = engine.clone();
        assert_eq!(engine.tick(&resources).unwrap(), Status::Halted);
        assert_eq!(engine.globals, [343, 24]);
        assert_eq!(engine.control.cursor, [23, 34]);
        assert!(engine.control.enabled);
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
    fn escaped_scroll_offsets_before_supersession_and_roundtrips() {
        for at_entry in [false, true] {
            for blocking in [0, 1] {
                for replaced in [false, true] {
                    let (master, mut scene) = fixtures();
                    // Raw target deliberately lies outside normal scroll limits.
                    let code = [
                        0x41, 77, 0x41, 20, 0x41, 30, 2, 0x41, blocking, 0x4e, 61, 0x4c, 0, 0,
                    ];
                    scene[8..8 + code.len()].copy_from_slice(&code);
                    let resources = Resources(vec![&master, &scene]);
                    let mut engine = Engine::bootstrap(&resources).unwrap();
                    engine.control.cursor = [12, 34];
                    engine.escape_events = Some(1);
                    for vm in engine.escape_contexts_mut() {
                        vm.escape_enabled = true;
                        vm.escape_generation = Some(1);
                    }
                    if at_entry {
                        engine.script_escape_event().unwrap();
                    }
                    engine.tick(&resources).unwrap();
                    if at_entry {
                        assert_eq!(engine.world_origin, [20, 30]);
                        assert_eq!(engine.globals, [77]);
                        assert_eq!(engine.scroll.serial, 1);
                        assert!(!engine.scroll.is_scrolling());
                        continue;
                    }
                    if replaced {
                        engine.scroll.start([0, 0], [40, 50], 1).unwrap();
                    }
                    engine.script_escape_event().unwrap();
                    let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
                        .to_bytes()
                        .unwrap();
                    let snapshot = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                    assert_eq!(snapshot.to_bytes().unwrap(), bytes);
                    let mut restored = snapshot.state().clone();
                    engine.tick(&resources).unwrap();
                    restored.tick(&resources).unwrap();
                    assert_eq!(engine, restored);
                    assert_eq!(engine.globals, [77]);
                    assert_eq!(engine.control.cursor, [12, 34]);
                    assert_eq!(engine.scroll.serial, if replaced { 2 } else { 1 });
                    if blocking == 1 {
                        assert_eq!(engine.world_origin, [20, 30]);
                        assert!(!engine.scroll.is_scrolling());
                    } else {
                        // V1 does not add V2's asynchronous escape monitor.
                        assert_eq!(engine.world_origin, [0, 0]);
                        assert!(engine.scroll.is_scrolling());
                    }
                }
            }
        }
    }

    #[test]
    fn escaped_setpalette_skips_resource_access() {
        let (master, mut scene) = fixtures();
        scene[8..19].copy_from_slice(&[1, 255, 255, 255, 255, 0x4e, 66, 0x41, 9, 0x4c, 0]);
        scene[19] = 0;
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.escape_events = Some(2);
        for vm in engine.escape_contexts_mut() {
            vm.escape_enabled = true;
            vm.escape_generation = Some(1);
        }
        let palette = engine.background_palette.clone();
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals, [9]);
        assert_eq!(engine.background_palette, palette);
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
    fn talking_interrupts_live_walk_and_roundtrips_route_continuation() {
        use crate::save::Snapshot;
        for node in [false, true] {
            let (master, mut scene) = fixtures();
            scene[8..20].copy_from_slice(&[1, 0, 0, 0, 1, 3, 0x4e, 77, 0x41, 9, 0x4c, 0]);
            scene[20] = 0;
            let base = 2u32 << 23;
            let mut film = vec![0; 400];
            for (at, value) in [
                (0, 24u32),
                (4, 1),
                (8, base + 32),
                (12, base + 256),
                (32, base + 200),
                (40, 1),
                (64, 24),
                (68, 1),
                (72, base + 32),
                (76, base + 256),
                (200, base + 220),
                (220, 8 | (16 << 16)),
                (256, base + 200),
                (260, 1),
                (264, (-2i32) as u32),
            ] {
                film[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            let resources = Resources(vec![&master, &scene, &film]);
            let strings =
                crate::text::Strings::new(&[1, 0, 52, 51, 0, 0, 0, 0, 0, 1, b'W']).unwrap();
            let index = [0, 0, 0, 0, 4, 0, 0, 0];
            let mut samples = vec![0; 8];
            samples[4..8].copy_from_slice(&10000u32.to_le_bytes());
            samples.extend([128; 10000]);
            let bank = SampleBank::new(&index, &samples).unwrap();
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.actors = crate::actor::Actors::new(1).unwrap();
            engine.subtitles = false;
            engine.no_blocking = true;
            engine
                .declarations
                .movers
                .insert(1, [[Handle(base + 64); 12]; 10]);
            engine.polygons = crate::polygon::Polygons::new(
                vec![crate::polygon::Polygon {
                    kind: if node {
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
                    nodes: if node {
                        vec![[50, 70], [80, 70]]
                    } else {
                        vec![]
                    },
                    lines: if node {
                        vec![[0, 30, -2100, 0, 0, 0, 0, 0, 0, 0]]
                    } else {
                        vec![]
                    },
                    script: Handle(0),
                }],
                0,
            );
            let placement = crate::mover::Placement::initial(
                &engine.declarations,
                &engine.polygons,
                1,
                [50, 70],
                Handle(base + 64),
            )
            .unwrap();
            let mut mover = crate::mover::Mover::new(&resources, placement).unwrap();
            let mut stationary = engine.clone();
            stationary.movers.insert(1, mover.clone());
            stationary
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap();
            assert_eq!(
                stationary.movers[&1].reel_state.unwrap().pushed(),
                Handle(base + 64)
            );
            // A second TALK replaces the one shared pushed-film slot. The
            // final POP must restore the first talk film, not the old stand.
            let mut overlapping = stationary.clone();
            overlapping
                .processes
                .spawn(
                    Some(overlapping.generation),
                    ScriptProcess::new(Handle((1 << 23) | 8)),
                )
                .unwrap();
            overlapping
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap();
            assert_eq!(
                overlapping.movers[&1].reel_state.unwrap().pushed(),
                Handle(base)
            );
            // Conflicting old per-coroutine slots cannot identify a shared
            // slot. Reject the ambiguous migration without changing movers.
            let mut ambiguous = overlapping.clone();
            ambiguous.movers.get_mut(&1).unwrap().reel_state = None;
            let before = ambiguous.clone();
            assert_eq!(
                ambiguous.migrate_legacy_talk_reels(&resources),
                Err(Error("ambiguous legacy talk reel slot"))
            );
            assert_eq!(ambiguous, before);
            for _ in 0..40 {
                let bytes = Snapshot::capture(&overlapping, [7; 32]).to_bytes().unwrap();
                let mut restored = Snapshot::from_bytes(&bytes, [7; 32])
                    .unwrap()
                    .state()
                    .clone();
                let a = overlapping
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                let b = restored
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                assert_eq!(a, b);
                assert_eq!(overlapping, restored);
            }
            assert!(
                overlapping
                    .processes
                    .states()
                    .all(|p| p.talk_wait.is_none())
            );
            assert_eq!(overlapping.movers[&1].last_film, Handle(base));
            let mut legacy = stationary.clone();
            legacy.movers.get_mut(&1).unwrap().reel_state = None;
            let old_bytes = Snapshot::capture(&legacy, [7; 32]).to_bytes().unwrap();
            assert_eq!(u32::from_le_bytes(old_bytes[8..12].try_into().unwrap()), 12);
            let decoded = Snapshot::from_bytes(&old_bytes, [7; 32]).unwrap();
            assert_eq!(decoded.to_bytes().unwrap(), old_bytes);
            let mut legacy = decoded.state().clone();
            let mut moved_during_speech = false;
            for tick in 0..40 {
                if tick == 2 {
                    for instance in [&mut stationary, &mut legacy] {
                        instance
                            .movers
                            .get_mut(&1)
                            .unwrap()
                            .begin_walk(
                                &resources,
                                &instance.declarations,
                                &instance.polygons,
                                [80, 70],
                                [80, 70],
                            )
                            .unwrap();
                    }
                }
                if tick == 3 {
                    // Old runtimes could save after a new walk replaced a
                    // stationary talk. A pending wait alone must not freeze it.
                    legacy.movers.get_mut(&1).unwrap().reel_state = None;
                    let bytes = Snapshot::capture(&legacy, [7; 32]).to_bytes().unwrap();
                    let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                    assert_eq!(decoded.to_bytes().unwrap(), bytes);
                    legacy = decoded.state().clone();
                }
                let a = stationary
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                let b = legacy
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                assert_eq!(a, b);
                assert_eq!(
                    stationary, legacy,
                    "legacy migration node={node} tick={tick}"
                );
                moved_during_speech |= stationary.sample_player.id.is_some()
                    && stationary.movers[&1].placement.position[0] > 50;
            }
            assert!(moved_during_speech);
            assert_eq!(stationary.movers[&1].placement.position, [80, 70]);
            assert_eq!(stationary.movers[&1].walk_target, None);
            assert_eq!(stationary.movers[&1].last_film, Handle(base + 64));
            assert_eq!(stationary.globals, [9]);
            let serial = mover
                .begin_walk(
                    &resources,
                    &engine.declarations,
                    &engine.polygons,
                    [80, 70],
                    [80, 70],
                )
                .unwrap();
            engine.movers.insert(1, mover);
            engine
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap();
            assert!(
                engine.movers[&1].special_reel(),
                "TALK must run while actor is moving"
            );
            let frozen = engine.movers[&1].placement.position;
            assert!(frozen[0] < 80);
            // Scene replacement must discard the suspended route and TALK
            // coroutine together. Re-enter without startup scripts so the
            // previous coroutine cannot be mistaken for a fresh scene call.
            let mut transitioned = engine.clone();
            let generation = transitioned.generation;
            transitioned
                .enter_scene(&resources, Handle(1 << 23), None, 1)
                .unwrap();
            assert_eq!(transitioned.generation, generation + 1);
            assert!(transitioned.movers.is_empty());
            assert!(
                transitioned
                    .processes
                    .states()
                    .all(|p| p.talk_wait.is_none())
            );
            assert!(transitioned.sample_player.id.is_none());
            let bytes = Snapshot::capture(&transitioned, [7; 32])
                .to_bytes()
                .unwrap();
            let mut loaded = Snapshot::from_bytes(&bytes, [7; 32])
                .unwrap()
                .restore(&resources, &bank, &[])
                .unwrap();
            assert_eq!(loaded, transitioned);
            for _ in 0..3 {
                let a = transitioned
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                let b = loaded
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap();
                assert_eq!(a, b);
                assert_eq!(loaded, transitioned);
                assert!(loaded.movers.is_empty());
                assert_eq!(
                    loaded.globals,
                    [0],
                    "cancelled TALK must not resume its script"
                );
            }
            for scenario in [
                "natural", "click", "escape", "stop", "replace", "hidden", "early",
            ] {
                let mut live = engine.clone();
                if scenario == "escape" {
                    live.escape_events = Some(1);
                    for vm in live.escape_contexts_mut() {
                        vm.escape_enabled = true;
                        vm.escape_generation = Some(1);
                    }
                }
                if scenario == "stop" {
                    live.movers.get_mut(&1).unwrap().stop_requested = true;
                }
                if scenario == "replace" {
                    live.movers
                        .get_mut(&1)
                        .unwrap()
                        .begin_walk(
                            &resources,
                            &live.declarations,
                            &live.polygons,
                            [50, 70],
                            [50, 70],
                        )
                        .unwrap();
                    assert!(!live.movers[&1].special_reel());
                }
                if scenario == "hidden" {
                    live.movers.get_mut(&1).unwrap().hidden = true;
                }
                if scenario == "early" {
                    live.configuration = Some(
                        crate::menu_controller::Controller::new(crate::menu_controller::Settings {
                            volumes: [255; 3],
                            double_click: 10,
                            swap_buttons: false,
                            text_speed: 100,
                            subtitles: true,
                            flipped: false,
                        })
                        .unwrap(),
                    );
                    let id = live
                        .processes
                        .entries()
                        .find(|(_, p)| p.talk_wait.is_some())
                        .unwrap()
                        .0;
                    let wait = live
                        .processes
                        .state_mut(id)
                        .unwrap()
                        .talk_wait
                        .as_mut()
                        .unwrap();
                    wait.initialized = true;
                    wait.layout = Some(crate::text::Layout { glyphs: vec![] });
                    wait.remaining = 145;
                }
                let mut paused_ticks = 0;
                for tick in 0..50 {
                    if scenario == "click" && tick == 3 {
                        live.left_event().unwrap();
                    }
                    if scenario == "escape" && tick == 3 {
                        live.script_escape_event().unwrap();
                        for p in live.processes.states_mut() {
                            if let Some(wait) = &mut p.talk_wait {
                                wait.left_events = live.left_events;
                            }
                        }
                    }
                    let bytes = Snapshot::capture(&live, [7; 32]).to_bytes().unwrap();
                    let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                    assert_eq!(decoded.to_bytes().unwrap(), bytes);
                    let mut restored = decoded.state().clone();
                    // This synthetic route is not in the scene's polygon
                    // archive, so full resource restore belongs to the
                    // original-asset test. Rebuild the closed preferences
                    // controller separately; it is deferred by from_bytes.
                    if let Some(menu) = &live.configuration {
                        restored.configuration = Some(
                            crate::menu_controller::Controller::from_checkpoint(
                                &menu.checkpoint().unwrap(),
                                &crate::menu_controller::Assets {
                                    resources: &resources,
                                    strings: &strings,
                                    declarations: &restored.declarations,
                                    saves: &[],
                                },
                            )
                            .unwrap(),
                        );
                    }
                    let previous = live.movers[&1].clone();
                    let a = live
                        .tick_content(&resources, Some(&bank), None, Some(&strings))
                        .unwrap();
                    let b = restored
                        .tick_content(&resources, Some(&bank), None, Some(&strings))
                        .unwrap();
                    assert_eq!(a, b);
                    assert_eq!(
                        live, restored,
                        "node={node}, scenario={scenario}, tick={tick}"
                    );
                    if previous.special_reel() {
                        assert_eq!(
                            live.movers[&1].placement.position,
                            previous.placement.position
                        );
                        if scenario != "replace" {
                            assert_eq!(live.movers[&1].placement.position, frozen);
                        }
                        if scenario == "hidden" && live.movers[&1].special_reel() {
                            assert_eq!(live.movers[&1].animation, previous.animation);
                        }
                        paused_ticks += 1;
                    }
                    if live.movers[&1].walk_target.is_none()
                        && (scenario == "early"
                            || live.processes.states().all(|p| p.talk_wait.is_none()))
                    {
                        break;
                    }
                }
                if scenario == "early" {
                    assert!(
                        !live.finished_talks().is_empty(),
                        "walk resumes before remaining subtitle expires"
                    );
                } else {
                    assert!(live.processes.states().all(|p| p.talk_wait.is_none()));
                }
                assert!(paused_ticks > 0 || scenario == "replace");
                assert_eq!(live.movers[&1].walk_target, None, "{scenario}");
                let expected = match scenario {
                    "stop" => frozen,
                    "replace" => [50, 70],
                    _ => [80, 70],
                };
                assert_eq!(live.movers[&1].placement.position, expected, "{scenario}");
                assert_eq!(
                    live.movers[&1].walk_serial,
                    serial + u64::from(matches!(scenario, "stop" | "replace"))
                );
            }
        }
    }

    #[test]
    fn escaped_talk_entry_skips_resources_and_preserves_audio_control() {
        let (master, mut scene) = fixtures();
        scene[8..21].copy_from_slice(&[1, 255, 255, 255, 255, 3, 0x4e, 77, 0x41, 9, 0x4c, 0, 0]);
        let resources = Resources(vec![&master, &scene]);
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let mut samples = vec![0; 8];
        samples[4..8].copy_from_slice(&1000u32.to_le_bytes());
        samples.extend([128; 1000]);
        let bank = SampleBank::new(&index, &samples).unwrap();
        for control in [false, true] {
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.control.enabled = control;
            engine.sample_player.play_speech(&bank, 1).unwrap();
            engine.escape_events = Some(2);
            for vm in engine.escape_contexts_mut() {
                vm.escape_enabled = true;
                vm.escape_generation = Some(1);
            }
            let before = engine.sample_player.clone();
            let controls = engine.control.clone();
            engine.tick(&resources).unwrap();
            assert_eq!(engine.globals, [9]);
            assert_eq!(engine.control, controls);
            assert_eq!(engine.sample_player, before);
            assert!(engine.processes.states().all(|p| p.talk_wait.is_none()));
        }
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
        let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
            .to_bytes()
            .unwrap();
        let decoded = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
        assert_eq!(decoded.state(), &engine);
        let mut resumed = decoded.state().clone();
        let mut natural = engine.clone();
        let mut status = Status::Running;
        for _ in 0..12 {
            status = natural
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap();
            assert_eq!(
                resumed
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap(),
                status
            );
            assert_eq!(resumed, natural);
            if status == Status::Halted {
                break;
            }
        }
        assert_eq!(status, Status::Halted);
        assert_eq!(natural.globals, [9]);
        assert!(natural.control.tags_enabled);
        assert_eq!(natural.films[0].reel, Some(Handle(base + 72)));
        // Exercise the post-speech phase with a pre-laid-out subtitle. Font
        // rasterization is independent of the speech/reel scheduler tested here.
        let mut slow = engine.clone();
        slow.configuration = Some(
            crate::menu_controller::Controller::new(crate::menu_controller::Settings {
                volumes: [255; 3],
                double_click: 10,
                swap_buttons: false,
                text_speed: 100,
                subtitles: true,
                flipped: false,
            })
            .unwrap(),
        );
        assert_eq!(engine.text_time(1).unwrap(), 25);
        assert_eq!(slow.text_time(1).unwrap(), 145);
        let talk_id = slow
            .processes
            .entries()
            .find(|(_, process)| process.talk_wait.is_some())
            .unwrap()
            .0;
        let wait = slow
            .processes
            .state_mut(talk_id)
            .unwrap()
            .talk_wait
            .as_mut()
            .unwrap();
        assert!(wait.initialized);
        wait.layout = Some(crate::text::Layout { glyphs: vec![] });
        wait.remaining = 145;
        for _ in 0..20 {
            assert_eq!(
                slow.tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap(),
                Status::Running
            );
            if !slow.finished_talks().is_empty() {
                break;
            }
        }
        assert_eq!(slow.finished_talks(), [talk_id]);
        assert!(slow.sample_player.id.is_none());
        assert!(
            !slow.control.tags_enabled,
            "text still owns tags after speech"
        );
        assert_eq!(slow.films[0].reel, Some(Handle(base + 72)));
        let bytes = crate::save::Snapshot::capture(&slow, [7; 32])
            .to_bytes()
            .unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 11);
        let decoded = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
        assert_eq!(decoded.to_bytes().unwrap(), bytes);
        let mut resumed = decoded
            .restore_with_strings(&resources, &bank, &[], &strings)
            .unwrap();
        assert_eq!(resumed, slow);
        let mut invalid = slow.clone();
        assert!(
            invalid.restore_finished_talks(&[talk_id]).is_err(),
            "duplicate phase marker"
        );
        for source in [&engine, &slow] {
            for replacement in [false, true] {
                let mut escaped = source.clone();
                escaped.escape_events = Some(1);
                for vm in escaped.escape_contexts_mut() {
                    vm.escape_enabled = true;
                    vm.escape_generation = Some(1);
                }
                if replacement {
                    escaped.sample_player.play_speech(&bank, 1).unwrap();
                }
                let audio = escaped.sample_player.clone();
                escaped.script_escape_event().unwrap();
                // Isolate the generation test from TALK's click cancellation.
                for p in escaped.processes.states_mut() {
                    if let Some(wait) = &mut p.talk_wait {
                        wait.left_events = escaped.left_events;
                    }
                }
                let bytes = crate::save::Snapshot::capture(&escaped, [7; 32])
                    .to_bytes()
                    .unwrap();
                let snapshot = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                assert_eq!(snapshot.to_bytes().unwrap(), bytes);
                let mut restored = snapshot
                    .restore_with_strings(&resources, &bank, &[], &strings)
                    .unwrap();
                for instance in [&mut escaped, &mut restored] {
                    instance.tick(&resources).unwrap();
                    assert_eq!(instance.globals, [9]);
                    assert!(instance.control.tags_enabled);
                    assert!(instance.processes.states().all(|p| p.talk_wait.is_none()));
                    assert_eq!(instance.films[0].reel, Some(Handle(base + 72)));
                    if replacement {
                        assert_eq!(instance.sample_player, audio);
                    } else {
                        assert!(instance.sample_player.id.is_none());
                    }
                }
                assert_eq!(escaped, restored);
            }
        }
        let mut cancelled = slow.clone();
        cancelled.left_event().unwrap();
        assert_eq!(
            cancelled
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap(),
            Status::Halted
        );
        assert!(cancelled.control.tags_enabled);
        let mut ended = false;
        for _ in 0..160 {
            let status = slow
                .tick_content(&resources, Some(&bank), None, Some(&strings))
                .unwrap();
            assert_eq!(
                resumed
                    .tick_content(&resources, Some(&bank), None, Some(&strings))
                    .unwrap(),
                status
            );
            assert_eq!(resumed, slow);
            if status == Status::Halted {
                ended = true;
                break;
            }
        }
        assert!(ended);
        assert!(slow.control.tags_enabled);
        assert!(slow.finished_talks().is_empty());
        assert_eq!(slow.globals, [9]);
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
    fn explicit_mover_reels_roundtrip_and_reject_invalid_extensions() {
        use crate::mover_reel::{Operation, State as ReelState};
        use crate::save::Snapshot;
        use sha2::Digest;
        let (master, scene) = fixtures();
        let base = 2u32 << 23;
        let mut film = vec![0; 88];
        for (at, value) in [
            (0, 24u32),
            (4, 1),
            (8, base + 32),
            (12, base + 56),
            (40, 1),
            (56, 10),
            (60, 1),
            (64, (-2i32) as u32),
        ] {
            film[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resources = Resources(vec![&master, &scene, &film]);
        let bank = SampleBank::new(&[0; 4], &[0; 4]).unwrap();
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.actors = crate::actor::Actors::new(1).unwrap();
        engine
            .declarations
            .movers
            .insert(1, [[Handle(base); 12]; 10]);
        let mut mover = crate::mover::Mover::new(
            &resources,
            crate::mover::Placement {
                actor: 1,
                position: [10, 20],
                direction: crate::mover::Direction::Left,
                path: None,
                node: None,
                scale: 1,
                standing: Handle(base),
                base_walk: Handle(base),
                z_factor: 0,
            },
        )
        .unwrap();
        mover.walk_target = Some([100, 20]);
        mover.walk_serial = 7;
        engine.movers.insert(1, mover);
        let legacy = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
        assert_eq!(
            Snapshot::from_bytes(&legacy, [7; 32])
                .unwrap()
                .to_bytes()
                .unwrap(),
            legacy
        );
        engine
            .movers
            .get_mut(&1)
            .unwrap()
            .alter_reel(
                &resources,
                &engine.declarations,
                ReelState::default(),
                Operation::Push(Handle(base)),
            )
            .unwrap();
        let bytes = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 13);
        let decoded = Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
        assert_eq!(decoded.to_bytes().unwrap(), bytes);
        let mut resumed = decoded.restore(&resources, &bank, &[]).unwrap();
        assert_eq!(resumed, engine);
        for operation in [
            Operation::Normal(Handle(base)),
            Operation::Pop,
            Operation::Walk(Handle(base)),
        ] {
            for instance in [&mut engine, &mut resumed] {
                instance
                    .movers
                    .get_mut(&1)
                    .unwrap()
                    .alter_reel(
                        &resources,
                        &instance.declarations,
                        ReelState::default(),
                        operation,
                    )
                    .unwrap();
            }
            assert_eq!(engine, resumed);
            assert_eq!(engine.movers[&1].walk_target, Some([100, 20]));
            assert_eq!(engine.movers[&1].walk_serial, 7);
            assert_eq!(
                Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap(),
                Snapshot::capture(&resumed, [7; 32]).to_bytes().unwrap()
            );
        }
        // Forge checksummed envelopes so failures exercise the extension
        // validator, not merely the outer checksum.
        let envelope = |inner: Vec<u8>, reels: Vec<(i32, ReelState)>| {
            let payload = postcard::to_allocvec(&(inner, reels)).unwrap();
            let mut result = bytes[..80].to_vec();
            result[44..48].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            result[48..80].copy_from_slice(&sha2::Sha256::digest(&payload));
            result.extend(payload);
            result
        };
        let state = engine.movers[&1].reel_state.unwrap();
        for invalid in [
            envelope(legacy.clone(), vec![]),
            envelope(legacy.clone(), vec![(1, state), (1, state)]),
            envelope(legacy.clone(), vec![(2, state)]),
            envelope(bytes.clone(), vec![(1, state)]),
        ] {
            assert!(Snapshot::from_bytes(&invalid, [7; 32]).is_err());
        }
        let bad = ReelState::default()
            .transition(Handle(base + 87), Operation::Normal(Handle(base + 87)))
            .state
            .transition(Handle(base + 87), Operation::Push(Handle(base)))
            .state;
        let invalid = envelope(legacy, vec![(1, bad)]);
        let prepared = Snapshot::from_bytes(&invalid, [7; 32]).unwrap();
        assert!(
            prepared.restore(&resources, &bank, &[]).is_err(),
            "invalid pushed film must not activate"
        );
    }

    #[test]
    fn scene_wait_and_snapshot_resume() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        assert_eq!(engine.scene, Handle(1 << 23));
        assert_eq!(engine.tick(&resources).unwrap(), Status::Running);
        let snapshot = crate::save::Snapshot::capture(&engine, [7; 32]);
        let bytes = snapshot.to_bytes().unwrap();
        let decoded = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
        decoded.verify_scripts(&resources).unwrap();
        assert_eq!(decoded, snapshot);
        assert_eq!(decoded.tick(), engine.ticks);
        assert_eq!(decoded.to_bytes().unwrap(), bytes);
        let bank = SampleBank::new(&[0; 4], &[]).unwrap();
        let mut restored = decoded.restore(&resources, &bank, &[]).unwrap();
        assert_eq!(restored.tick(&resources).unwrap(), Status::Running);
        assert_eq!(restored.tick(&resources).unwrap(), Status::Halted);
        engine.tick(&resources).unwrap();
        engine.tick(&resources).unwrap();
        assert_eq!(restored, engine);
    }
    #[test]
    fn snapshot_restore_rejects_invalid_indices_without_changing_live_engine() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let bank = SampleBank::new(&[0; 4], &[]).unwrap();
        let engine = Engine::bootstrap(&resources).unwrap();
        let before = engine.clone();
        for field in 0..4 {
            let mut invalid = engine.clone();
            match field {
                0 => invalid.active_inventory = Some(3),
                1 => invalid.fade_in_step = Some(7),
                2 => invalid.pending_scene = Some((invalid.scene, 0, 0, Some(0))),
                _ => invalid.pending_scene = Some((invalid.scene, 0, 0, Some(7))),
            }
            assert!(
                crate::save::Snapshot::capture(&invalid, [7; 32])
                    .restore(&resources, &bank, &[])
                    .is_err()
            );
            assert_eq!(engine, before);
        }
    }
    #[test]
    fn escape_cancels_timed_waits_rearms_and_survives_save_load() {
        let (master, mut scene) = fixtures();
        let mut code = vec![40];
        for value in 1..=4 {
            if value == 3 {
                code.push(40);
            }
            if value == 4 {
                code.push(41);
            }
            code.extend([0x41, 10, 3, 0x4e, 86, 0x41, value, 0x4c, 0]);
        }
        code.push(0);
        scene[8..8 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.control.enabled = false;
        let tick = |engine: &mut Engine| {
            let bytes = crate::save::Snapshot::capture(engine, [7; 32])
                .to_bytes()
                .unwrap();
            let decoded = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
            assert_eq!(decoded.to_bytes().unwrap(), bytes);
            let mut restored = decoded.state().clone();
            assert_eq!(
                engine.tick(&resources).unwrap(),
                restored.tick(&resources).unwrap()
            );
            assert_eq!(*engine, restored);
        };
        tick(&mut engine);
        assert_eq!(engine.globals[0], 0);
        assert_eq!(engine.escape_events, Some(1));
        let old_events = (engine.user_events, engine.left_events);
        engine.script_escape_event().unwrap();
        assert_eq!(
            (engine.user_events, engine.left_events),
            (old_events.0 + 1, old_events.1 + 1)
        );
        assert!(!engine.control.enabled);
        tick(&mut engine);
        assert_eq!(
            engine.globals[0], 2,
            "stale generation skips a new WAITTIME too"
        );
        assert!(
            engine
                .processes
                .states()
                .any(|p| p.vm.escape_generation == Some(2))
        );
        tick(&mut engine);
        assert_eq!(
            engine.globals[0], 2,
            "repeated ESCON captures the new generation"
        );
        engine.script_escape_event().unwrap();
        tick(&mut engine);
        assert_eq!(engine.globals[0], 3);
        engine.script_escape_event().unwrap();
        tick(&mut engine);
        assert_eq!(
            engine.globals[0], 3,
            "ESCOFF makes the final wait unskippable"
        );
        for _ in 0..12 {
            tick(&mut engine);
        }
        assert_eq!(engine.globals[0], 4);
        engine.escape_events = Some(u64::MAX);
        let before = engine.clone();
        assert!(engine.script_escape_event().is_err());
        assert_eq!(engine, before);
    }

    #[test]
    fn escape_event_cancels_master_wait_and_migrates_legacy_contexts() {
        let (master, mut scene) = fixtures();
        let code = [40, 0x41, 5, 3, 0x4e, 86, 0x41, 77, 0x4c, 0, 0];
        scene[8..8 + code.len()].copy_from_slice(&code);
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.master = Machine::default();
        engine.master_code = Handle((1 << 23) | 8);
        engine.master_stopped = false;
        engine.processes = Scheduler::default();
        engine.tick(&resources).unwrap();
        assert!(engine.master_wait.is_some());
        assert_eq!(engine.master.escape_generation, Some(1));
        // An older save recorded enabled=true but no generation. Its first
        // newly delivered escape must cancel this already-suspended wait.
        engine.clear_escape_checkpoint();
        let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
            .to_bytes()
            .unwrap();
        engine = crate::save::Snapshot::from_bytes(&bytes, [7; 32])
            .unwrap()
            .state()
            .clone();
        engine.script_escape_event().unwrap();
        assert_eq!(engine.master.escape_generation, Some(1));
        assert_eq!(engine.escape_events, Some(2));
        engine.tick(&resources).unwrap();
        assert_eq!(engine.globals[0], 77);
        assert!(engine.master_wait.is_none());
    }

    #[test]
    fn escape_snapshot_covers_master_live_saved_and_restoring_contexts() {
        use crate::save::Snapshot;
        use sha2::{Digest, Sha256};
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.tick(&resources).unwrap();
        let saved = engine.capture_scene().unwrap();
        engine.saved_scenes.push(saved.clone());
        engine.scene_restore = Some(SceneRestore {
            snapshot: saved,
            remaining: 5,
            fade: false,
        });
        assert!(engine.escape_contexts().count() >= 4);
        for vm in engine.escape_contexts_mut() {
            vm.escape_enabled = true;
        }
        engine.master.escape_enabled = false;
        let legacy = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
        let count = engine.escape_contexts().count();
        let mut captures = vec![Some(3); count];
        captures[0] = None;
        engine.restore_escape_checkpoint(5, &captures).unwrap();
        let encoded = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
        assert_eq!(u32::from_le_bytes(encoded[8..12].try_into().unwrap()), 14);
        let decoded = Snapshot::from_bytes(&encoded, [7; 32]).unwrap();
        assert_eq!(decoded.to_bytes().unwrap(), encoded);
        assert_eq!(decoded.state(), &engine);
        let mut resumed = decoded.state().clone();
        engine.tick(&resources).unwrap();
        resumed.tick(&resources).unwrap();
        assert_eq!(engine, resumed);
        let ((inner, event, stored), tail) =
            postcard::take_from_bytes::<(Vec<u8>, u64, Vec<Option<u64>>)>(&encoded[80..]).unwrap();
        assert!(tail.is_empty());
        assert_eq!(inner, legacy);
        assert_eq!(stored, captures);
        let legacy_decoded = Snapshot::from_bytes(&legacy, [7; 32]).unwrap();
        assert_eq!(legacy_decoded.to_bytes().unwrap(), legacy);
        assert!(legacy_decoded.state().escape_checkpoint().is_none());
        let wrap = |inner: Vec<u8>, event, captures: Vec<Option<u64>>| {
            let payload = postcard::to_allocvec(&(inner, event, captures)).unwrap();
            let mut bytes = encoded[..80].to_vec();
            bytes[44..48].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes[48..80].copy_from_slice(&Sha256::digest(&payload));
            bytes.extend(payload);
            bytes
        };
        for bad in [
            wrap(inner.clone(), 0u64, captures.clone()),
            wrap(inner.clone(), event, vec![]),
            wrap(inner.clone(), event, vec![Some(6); count]),
            wrap(inner.clone(), event, vec![Some(3); count]), // disabled master
            wrap(encoded.clone(), event, captures.clone()),   // no nested v14
        ] {
            assert!(Snapshot::from_bytes(&bad, [7; 32]).is_err());
        }
        let before = engine.clone();
        assert!(engine.restore_escape_checkpoint(0, &captures).is_err());
        assert_eq!(engine, before);
    }

    #[test]
    fn snapshot_script_validation_includes_saved_cutaways_and_host_waits() {
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.tick(&resources).unwrap();
        engine.validate_snapshot_scripts(&resources).unwrap();
        let saved = engine.capture_scene().unwrap();
        assert!(!saved.scripts.contexts.is_empty());
        engine.saved_scenes.push(saved.clone());
        engine.validate_snapshot_scripts(&resources).unwrap();
        engine.saved_scenes[0].scripts.contexts[0].code = Handle(u32::MAX);
        assert!(engine.validate_snapshot_scripts(&resources).is_err());
        engine.saved_scenes.clear();
        engine.scene_restore = Some(SceneRestore {
            snapshot: saved,
            remaining: 5,
            fade: false,
        });
        engine.validate_snapshot_scripts(&resources).unwrap();
        engine
            .scene_restore
            .as_mut()
            .unwrap()
            .snapshot
            .scripts
            .contexts[0]
            .code = Handle(u32::MAX);
        assert!(engine.validate_snapshot_scripts(&resources).is_err());
        engine.scene_restore = None;
        let id = engine.processes.entries().next().unwrap().0;
        let process = engine.processes.state_mut(id).unwrap();
        let call = process.vm.pending_call().unwrap();
        process.wait = Some((
            LibraryCall {
                id: call.id + 1,
                ..call
            },
            1,
        ));
        assert!(engine.validate_snapshot_structure().is_err());
        let process = engine.processes.state_mut(id).unwrap();
        process.wait = Some((call, 1));
        process.sample_wait = Some(call);
        assert!(engine.validate_snapshot_structure().is_err());
    }
    #[test]
    fn snapshot_preview_checks_hidden_cursor_resources_without_mutation() {
        let (master, scene) = fixtures();
        let film = [0u8; 64];
        let resources = Resources(vec![&master, &scene, &film]);
        let mut engine = Engine::bootstrap(&resources).unwrap();
        engine.control.hidden = true;
        engine.cursor_animation = Some(
            Animation::new(
                &resources,
                &crate::film::Reel {
                    object: Handle(2 << 23),
                    script: Handle((2 << 23) | 48),
                },
                1,
            )
            .unwrap(),
        );
        let snapshot = crate::save::Snapshot::capture(&engine, [7; 32]);
        assert_eq!(
            snapshot.preview(&resources).unwrap(),
            engine.render(&resources).unwrap()
        );
        let before = engine.clone();
        engine.cursor_animation.as_mut().unwrap().frame = Handle(u32::MAX);
        // The hidden cursor is skipped by rendering but must be checked on load.
        assert_eq!(
            engine.render(&resources).unwrap(),
            before.render(&resources).unwrap()
        );
        let snapshot = crate::save::Snapshot::capture(&engine, [7; 32]);
        let malformed = engine.clone();
        assert!(snapshot.preview(&resources).is_err());
        assert_eq!(engine, malformed);
    }
    #[test]
    fn snapshot_envelope_rejects_corruption_truncation_wrong_assets_and_trailing_data() {
        use crate::save::Snapshot;
        use sha2::{Digest, Sha256};
        let (master, scene) = fixtures();
        let resources = Resources(vec![&master, &scene]);
        let engine = Engine::bootstrap(&resources).unwrap();
        let bytes = Snapshot::capture(&engine, [7; 32]).to_bytes().unwrap();
        for end in 0..bytes.len() {
            assert!(Snapshot::from_bytes(&bytes[..end], [7; 32]).is_err());
        }
        for index in 0..bytes.len() {
            let mut corrupted = bytes.clone();
            corrupted[index] ^= 1;
            assert!(
                Snapshot::from_bytes(&corrupted, [7; 32]).is_err(),
                "byte {index}"
            );
        }
        assert_eq!(
            Snapshot::from_bytes(&bytes, [8; 32]),
            Err(Error("save belongs to different game assets"))
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        let payload_len = trailing.len() - 80;
        trailing[44..48].copy_from_slice(&(payload_len as u32).to_le_bytes());
        let digest = Sha256::digest(&trailing[80..]);
        trailing[48..80].copy_from_slice(&digest);
        assert_eq!(
            Snapshot::from_bytes(&trailing, [7; 32]),
            Err(Error("trailing snapshot data"))
        );
        // A correct checksum must not permit a mid-dispatch scheduler to load.
        let mut mid_pass = engine.clone();
        mid_pass.processes.begin().unwrap();
        assert!(Snapshot::capture(&mid_pass, [7; 32]).to_bytes().is_err());
        let payload = postcard::to_allocvec(&mid_pass).unwrap();
        let mut forged = bytes[..80].to_vec();
        forged[44..48].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        forged[48..80].copy_from_slice(&Sha256::digest(&payload));
        forged.extend_from_slice(&payload);
        assert_eq!(
            Snapshot::from_bytes(&forged, [7; 32]),
            Err(Error("snapshot scheduler is not idle"))
        );
        assert_eq!(engine, Engine::bootstrap(&resources).unwrap());
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
            for enabled in [false, true] {
                engine.escape_events = Some(2);
                let mut escaped = ScriptProcess::new(Handle((1 << 23) | 32));
                escaped.vm.escape_enabled = enabled;
                escaped.vm.escape_generation = enabled.then_some(1);
                assert_eq!(
                    engine
                        .run_process(&resources, None, None, None, &mut escaped)
                        .unwrap(),
                    Status::Halted
                );
                assert_eq!(
                    engine.globals,
                    [i32::from(!enabled && matches!(phase, 1 | 2)), 77]
                );
                assert_eq!(engine.sample_player, before);
            }
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
    fn playsample_preserves_active_speech_without_yielding_or_retrying() {
        for blocking in [0, 1] {
            let (master, mut scene) = fixtures();
            scene[8] = 0;
            let code = [0x41, 77, 3, 0x41, blocking, 0x4e, 51, 0x4c, 0, 0];
            scene[32..32 + code.len()].copy_from_slice(&code);
            let resources = Resources(vec![&master, &scene]);
            let index = [0, 0, 0, 0, 4, 0, 0, 0];
            let mut data = vec![0; 8];
            data[4..8].copy_from_slice(&1000u32.to_le_bytes());
            data.extend(vec![129; 1000]);
            let bank = SampleBank::new(&index, &data).unwrap();
            let mut engine = Engine::bootstrap(&resources).unwrap();
            engine.sample_player.play_speech(&bank, 1).unwrap();
            engine.sample_player.tick(&bank).unwrap();
            let speech = engine.sample_player.clone();
            let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
            assert_eq!(
                engine
                    .run_process(&resources, Some(&bank), None, None, &mut process)
                    .unwrap(),
                Status::Halted
            );
            assert_eq!(engine.sample_player, speech);
            assert_eq!(engine.globals[0], 77);
            assert!(process.sample_wait.is_none() && process.wait.is_none());
            engine.sample_player.stop();
            let mut fresh = ScriptProcess::new(Handle((1 << 23) | 32));
            let status = engine
                .run_process(&resources, Some(&bank), None, None, &mut fresh)
                .unwrap();
            assert_eq!(
                status,
                if blocking == 1 {
                    Status::Running
                } else {
                    Status::Halted
                }
            );
            assert!(!engine.sample_player.is_speech());
            assert_eq!(engine.sample_player.serial, speech.serial + 1);
            assert_eq!(engine.sample_player.position, 0);
        }
    }

    #[test]
    fn escaped_sample_entry_preserves_speech_and_try_busy_priority() {
        for opcode in [51, 112] {
            for channel in 0..3 {
                for blocking in [0, 1] {
                    for muted in [false, true] {
                        let (master, mut scene) = fixtures();
                        scene[8] = 0;
                        let code = [0x41, 77, 3, 0x41, blocking, 0x4e, opcode, 0x4c, 0, 0];
                        scene[32..32 + code.len()].copy_from_slice(&code);
                        let resources = Resources(vec![&master, &scene]);
                        let index = [0, 0, 0, 0, 4, 0, 0, 0];
                        let mut data = vec![0; 8];
                        data[4..8].copy_from_slice(&1000u32.to_le_bytes());
                        data.extend(vec![129; 1000]);
                        let bank = SampleBank::new(&index, &data).unwrap();
                        let mut engine = Engine::bootstrap(&resources).unwrap();
                        if muted {
                            let settings = crate::menu_controller::Settings {
                                volumes: [255, 0, 255],
                                double_click: 10,
                                swap_buttons: false,
                                text_speed: 0,
                                subtitles: true,
                                flipped: false,
                            };
                            engine.configuration =
                                Some(crate::menu_controller::Controller::new(settings).unwrap());
                        }
                        if channel == 1 {
                            engine.sample_player.play(&bank, 1).unwrap();
                        } else if channel == 2 {
                            engine.sample_player.play_speech(&bank, 1).unwrap();
                        }
                        engine.sample_player.tick(&bank).unwrap();
                        let before = engine.sample_player.clone();
                        engine.escape_events = Some(2);
                        let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
                        process.vm.escape_enabled = true;
                        process.vm.escape_generation = Some(1);
                        // No sample bank: an escaped call must not access assets.
                        let status = engine
                            .run_process(&resources, None, None, None, &mut process)
                            .unwrap();
                        let busy_try = opcode == 112 && channel != 0;
                        assert_eq!(
                            status,
                            if busy_try {
                                Status::Running
                            } else {
                                Status::Halted
                            }
                        );
                        assert!(process.sample_wait.is_none());
                        assert_eq!(process.wait.is_some(), busy_try);
                        if busy_try || channel == 2 {
                            assert_eq!(engine.sample_player, before);
                        } else {
                            let mut stopped = before.clone();
                            stopped.stop();
                            assert_eq!(engine.sample_player, stopped);
                        }
                        if busy_try {
                            assert_eq!(engine.globals[0], 0);
                            assert_eq!(
                                engine
                                    .run_process(&resources, None, None, None, &mut process)
                                    .unwrap(),
                                Status::Halted
                            );
                            assert_eq!(engine.sample_player, before);
                        }
                        assert_eq!(engine.globals[0], 77);
                    }
                }
            }
        }
    }

    #[test]
    fn zero_effect_volume_yields_once_without_starting_or_replacing_samples() {
        for opcode in [51, 112] {
            for active in [false, true] {
                for blocking in [0, 1] {
                    let (master, mut scene) = fixtures();
                    scene[8] = 0;
                    let code = [0x41, 77, 3, 0x41, blocking, 0x4e, opcode, 0x4c, 0, 0];
                    scene[32..32 + code.len()].copy_from_slice(&code);
                    let resources = Resources(vec![&master, &scene]);
                    let index = [0, 0, 0, 0, 4, 0, 0, 0];
                    let mut data = vec![0; 8];
                    data[4..8].copy_from_slice(&1000u32.to_le_bytes());
                    data.extend(vec![129; 1000]);
                    let bank = SampleBank::new(&index, &data).unwrap();
                    let mut engine = Engine::bootstrap(&resources).unwrap();
                    let mut settings = crate::menu_controller::Settings {
                        volumes: [255, 0, 255],
                        double_click: 10,
                        swap_buttons: false,
                        text_speed: 0,
                        subtitles: true,
                        flipped: false,
                    };
                    engine.configuration =
                        Some(crate::menu_controller::Controller::new(settings.clone()).unwrap());
                    if active {
                        engine.sample_player.play(&bank, 1).unwrap();
                        engine.sample_player.tick(&bank).unwrap();
                    }
                    let before = engine.sample_player.clone();
                    let mut process = ScriptProcess::new(Handle((1 << 23) | 32));
                    assert_eq!(
                        engine
                            .run_process(&resources, Some(&bank), None, None, &mut process)
                            .unwrap(),
                        Status::Running
                    );
                    assert_eq!(engine.sample_player, before);
                    assert!(process.sample_wait.is_none());
                    assert_eq!(engine.globals[0], 0);
                    // Restore the suspended operation, then enable effects.
                    // The declined request must complete, not retry playback.
                    let bytes = postcard::to_allocvec(&process).unwrap();
                    process = postcard::from_bytes(&bytes).unwrap();
                    settings.volumes[1] = 255;
                    engine.configuration =
                        Some(crate::menu_controller::Controller::new(settings).unwrap());
                    assert_eq!(
                        engine
                            .run_process(&resources, Some(&bank), None, None, &mut process)
                            .unwrap(),
                        Status::Halted
                    );
                    assert_eq!(engine.sample_player, before);
                    assert_eq!(engine.globals[0], 77);
                    assert!(process.wait.is_none());
                }
            }
        }
    }

    #[test]
    fn sample_wait_ownership_survives_saves_escape_and_replacement() {
        for opcode in [51, 112] {
            for escaped in [false, true] {
                // 0 original; 1 replaced effect; 2 replaced speech; 3 ended;
                // 4 legacy wait without a recorded playback handle.
                for mode in 0..5 {
                    let (master, mut scene) = fixtures();
                    let code = [0x41, 77, 3, 3, 0x4e, opcode, 0x4c, 0, 0];
                    scene[8..8 + code.len()].copy_from_slice(&code);
                    let resources = Resources(vec![&master, &scene]);
                    let index = [0, 0, 0, 0, 4, 0, 0, 0];
                    let mut data = vec![0; 8];
                    data[4..8].copy_from_slice(&5000u32.to_le_bytes());
                    data.extend(vec![129; 5000]);
                    let bank = SampleBank::new(&index, &data).unwrap();
                    let mut engine = Engine::bootstrap(&resources).unwrap();
                    engine.escape_events = Some(1);
                    for vm in engine.escape_contexts_mut() {
                        vm.escape_enabled = true;
                        vm.escape_generation = Some(1);
                    }
                    engine.tick_audio(&resources, Some(&bank)).unwrap();
                    assert!(engine.sample_wait_checkpoint().contains(&Some(1)));
                    match mode {
                        1 => {
                            engine.sample_player.play(&bank, 1).unwrap();
                        }
                        2 => {
                            engine.sample_player.play_speech(&bank, 1).unwrap();
                        }
                        3 => engine.sample_player.stop(),
                        4 => engine.clear_sample_wait_checkpoint(),
                        _ => {}
                    }
                    let before = engine.sample_player.clone();
                    let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
                        .to_bytes()
                        .unwrap();
                    if mode != 4 {
                        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 16);
                        let (inner, serials): (Vec<u8>, Vec<Option<u64>>) =
                            postcard::from_bytes(&bytes[80..]).unwrap();
                        use sha2::{Digest, Sha256};
                        let wrap = |inner: Vec<u8>, serials: Vec<Option<u64>>| {
                            let payload = postcard::to_allocvec(&(inner, serials)).unwrap();
                            let mut bad = bytes[..80].to_vec();
                            bad[44..48].copy_from_slice(&(payload.len() as u32).to_le_bytes());
                            bad[48..80].copy_from_slice(&Sha256::digest(&payload));
                            bad.extend(payload);
                            bad
                        };
                        let mut zero = serials.clone();
                        *zero.iter_mut().find(|s| s.is_some()).unwrap() = Some(0);
                        let mut future = serials.clone();
                        *future.iter_mut().find(|s| s.is_some()).unwrap() = Some(u64::MAX);
                        for bad in [
                            wrap(inner.clone(), vec![]),
                            wrap(inner.clone(), vec![None; serials.len()]),
                            wrap(inner.clone(), zero),
                            wrap(inner, future),
                            wrap(bytes.clone(), serials),
                        ] {
                            assert!(crate::save::Snapshot::from_bytes(&bad, [7; 32]).is_err());
                        }
                    }
                    let snapshot = crate::save::Snapshot::from_bytes(&bytes, [7; 32]).unwrap();
                    assert_eq!(snapshot.to_bytes().unwrap(), bytes);
                    let mut restored = snapshot.state().clone();
                    if escaped {
                        engine.script_escape_event().unwrap();
                        restored.script_escape_event().unwrap();
                    }
                    engine.tick(&resources).unwrap();
                    restored.tick(&resources).unwrap();
                    assert_eq!(engine, restored);
                    let pending = !escaped && matches!(mode, 0 | 4);
                    assert_eq!(engine.globals[0], if pending { 0 } else { 77 });
                    assert_eq!(
                        engine.processes.states().any(|p| p.sample_wait.is_some()),
                        pending
                    );
                    if mode == 0 && escaped {
                        let mut stopped = before;
                        stopped.stop();
                        assert_eq!(engine.sample_player, stopped);
                    } else {
                        assert_eq!(engine.sample_player, before);
                    }
                    if !pending {
                        assert!(engine.sample_wait_checkpoint().iter().all(Option::is_none));
                    }
                }
            }
        }
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
