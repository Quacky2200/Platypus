// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What acoustics keeps per thread: the zones, occluders and sensors on objects, the noise
//! events, the settings and what the last update sent to the mixer (so only changes are sent).

use crate::model::{Curve, FxTarget, Shape};
use platypus_engine_api::audio::{PlaybackRef, SoundRef};
use platypus_scene::ObjectId;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use ts_api::Callback;
use ts_api::Handle;
use ts_api::callback::HandleArg;

macro_rules! numbered_handle {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub u32);

        impl Handle for $name {
            fn to_raw(self) -> u64 {
                u64::from(self.0)
            }
            fn from_raw(raw: u64) -> Self {
                $name(raw as u32)
            }
        }
    };
}

numbered_handle!(
    /// An `AudioZone` by number (1-based: 0 is "none").
    AudioZoneRef
);
numbered_handle!(
    /// An `AudioOccluder` by number (1-based: 0 is "none").
    AudioOccluderRef
);
numbered_handle!(
    /// An `AudioPortal` by number (1-based: 0 is "none").
    AudioPortalRef
);
numbered_handle!(
    /// A `SoundSensor` by number (1-based: 0 is "none").
    SoundSensorRef
);
numbered_handle!(
    /// A noise in the world (1-based: 0 is "none"); gone once it fades.
    SoundEventRef
);

/// A numbered list whose removed items leave a hole, so handles stay valid.
pub(crate) struct Slots<T>(pub Vec<Option<T>>);

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Slots(Vec::new())
    }
}

impl<T> Slots<T> {
    pub fn add(&mut self, item: T) -> u32 {
        self.0.push(Some(item));
        self.0.len() as u32
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        self.0.get_mut((id as usize).checked_sub(1)?)?.as_mut()
    }

    pub fn remove(&mut self, id: u32) -> Option<T> {
        self.0.get_mut((id as usize).checked_sub(1)?)?.take()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &T)> {
        self.0
            .iter()
            .enumerate()
            .filter_map(|(i, t)| Some((i as u32 + 1, t.as_ref()?)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (u32, &mut T)> {
        self.0
            .iter_mut()
            .enumerate()
            .filter_map(|(i, t)| Some((i as u32 + 1, t.as_mut()?)))
    }
}

/// A volume that colours what the listener hears.
#[derive(Clone)]
pub struct Zone {
    pub object: ObjectId,
    pub enabled: bool,
    pub shape: Shape,
    /// The sphere's outer radius (metres, before the object's scale).
    pub radius: f64,
    /// The box's outer size (before the object's scale).
    pub size: [f64; 3],
    /// How much of the volume is at full strength (0 to 1).
    pub inner: f64,
    pub curve: Curve,
    /// Strongest outside, not inside.
    pub invert: bool,
    /// Where zones overlap, the higher one wins.
    pub priority: f64,
    pub target: FxTarget,
    /// The buses it processes; none means the master mix.
    pub buses: Vec<u32>,
    pub ambience: SoundRef,
    pub ambience_volume: f64,
    /// The bus the ambience plays on (0: the master).
    pub ambience_bus: u32,
    /// How strongly it applies at the listener, as of the last update.
    pub weight: f64,
    /// Also colours the `AudioSource`s that stand in it (their emitters), not only the listener.
    pub affects_sources: bool,
    pub(crate) playback: Option<PlaybackRef>,
    pub(crate) paused: bool,
    pub(crate) sent_volume: f64,
}

impl Zone {
    pub fn new(object: ObjectId) -> Zone {
        Zone {
            object,
            enabled: true,
            shape: Shape::Sphere,
            radius: 10.0,
            size: [10.0, 10.0, 10.0],
            inner: 0.5,
            curve: Curve::Smooth,
            invert: false,
            priority: 0.0,
            target: FxTarget::OPEN,
            buses: Vec::new(),
            ambience: SoundRef(0),
            ambience_volume: 1.0,
            ambience_bus: 0,
            weight: 0.0,
            affects_sources: false,
            playback: None,
            paused: false,
            sent_volume: -1.0,
        }
    }
}

/// Something sound passes through badly: a wall, a door, a floor.
#[derive(Clone, Copy)]
pub struct Occluder {
    pub object: ObjectId,
    pub openness: f64,
    pub absorption: f64,
    pub low_pass: f64,
}

impl Occluder {
    pub fn new(object: ObjectId) -> Occluder {
        Occluder {
            object,
            openness: 0.0,
            absorption: 0.55,
            low_pass: 900.0,
        }
    }
}

/// An opening sound can bend through (a doorway, a window, a stairwell): when a wall is in the
/// way, a source is heard as coming from the portal, by the way round.
#[derive(Clone, Copy)]
pub struct Portal {
    pub object: ObjectId,
    pub enabled: bool,
    /// How much gets through, 0 to 1 (a window that can be shut by a script).
    pub gain: f64,
    /// Metres short of the portal the source-side leg stops, so a door in the opening is
    /// counted once.
    pub radius: f64,
    /// The low-pass cutoff (Hz) of sound that bends through: edges take the highs off.
    pub low_pass: f64,
}

impl Portal {
    pub fn new(object: ObjectId) -> Portal {
        Portal {
            object,
            enabled: true,
            gain: 1.0,
            radius: 0.25,
            low_pass: 9000.0,
        }
    }
}

/// Ears: hears noises and sounds loud enough where it stands.
#[derive(Clone)]
pub struct Sensor {
    pub object: ObjectId,
    pub enabled: bool,
    pub sensitivity: f64,
    pub threshold: f64,
    /// Walls between it and a noise quiet the noise.
    pub occlusion: bool,
    /// The loudest thing it hears now, 0 to 1 (a gunshot spikes it, then it falls).
    pub level: f64,
    pub(crate) spike: f64,
    pub(crate) heard: HashSet<u32>,
    pub(crate) hearing: HashSet<u32>,
    pub(crate) callbacks: Vec<HearCallback>,
}

pub type HearCallback = Callback<(HandleArg<SoundEventRef>, f64)>;

impl Sensor {
    pub fn new(object: ObjectId) -> Sensor {
        Sensor {
            object,
            enabled: true,
            sensitivity: 1.0,
            threshold: 0.03,
            occlusion: true,
            level: 0.0,
            spike: 0.0,
            heard: HashSet::new(),
            hearing: HashSet::new(),
            callbacks: Vec::new(),
        }
    }
}

/// A noise at a place: a gunshot, a footstep, a pulse of something playing.
#[derive(Clone, Debug)]
pub struct Event {
    pub id: u32,
    pub position: [f64; 3],
    /// Metres at which it can still be heard (at sensitivity 1).
    pub loudness: f64,
    pub kind: String,
    pub source: Option<ObjectId>,
    pub born: f64,
    /// Made by a playing source on a timer (for the picture, and to wake sensors once), not by
    /// a game's `emit`.
    pub pulse: bool,
}

/// Global acoustics settings.
#[derive(Clone, Copy)]
pub struct Settings {
    pub enabled: bool,
    pub speed_of_sound: f64,
    /// Layers whose colliders muffle sound even without an `AudioOccluder`.
    pub occlusion_layers: u32,
    /// How far (metres) to the sides of the straight line the extra probe rays go.
    pub ray_spread: f64,
    /// How much sounds behind or above the listener lose their highs, 0 to 1.
    pub head_shadow: f64,
    /// How much distance takes the highs off sounds, 0 to 2: see `model::air_cutoff`.
    pub air_absorption: f64,
    pub event_lifetime: f64,
    pub pulse_interval: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: true,
            speed_of_sound: 343.0,
            occlusion_layers: 0,
            ray_spread: 0.4,
            head_shadow: 0.5,
            air_absorption: 0.5,
            event_lifetime: 3.0,
            pulse_interval: 0.8,
        }
    }
}

/// What the update remembers about one `AudioSource`.
#[derive(Clone, Copy, Default)]
pub(crate) struct SrcState {
    /// The gain and cutoff last sent to its emitter (the combined result).
    pub gain: f64,
    pub low_pass: f64,
    pub checked: u64,
    pub doppler: f64,
    pub last_pulse: f64,
    pub started: bool,
    /// What occlusion (or the way round through a portal) left of it at the last probe.
    pub occ_gain: f64,
    pub occ_low_pass: f64,
    /// Where it is heard from when the way round is through a portal.
    pub virtual_pos: Option<[f64; 3]>,
}

#[derive(Default)]
pub(crate) struct State {
    pub zones: Slots<Zone>,
    pub occluders: Slots<Occluder>,
    pub portals: Slots<Portal>,
    pub sensors: Slots<Sensor>,
    pub events: Vec<Event>,
    pub next_event: u32,
    pub settings: Settings,
    pub time: f64,
    pub frame: u64,
    pub listener_prev: Option<[f64; 3]>,
    pub listener_velocity: [f64; 3],
    pub prev_pos: HashMap<ObjectId, [f64; 3]>,
    /// The last value sent for each (track, property), so only changes go to the mixer.
    pub sent: HashMap<(u32, &'static str), f64>,
    /// Tracks a zone drove last update: those no zone names now go back to open.
    pub driven: HashSet<u32>,
    pub src: HashMap<u32, SrcState>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub(crate) fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Forgets everything (a test, or a reload).
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}

/// Adds an event now; the number is its handle.
pub(crate) fn push_event(
    position: [f64; 3],
    loudness: f64,
    kind: &str,
    source: Option<ObjectId>,
    pulse: bool,
) -> u32 {
    st(|s| {
        s.next_event += 1;
        let id = s.next_event;
        let born = s.time;
        s.events.push(Event {
            id,
            position,
            loudness,
            kind: kind.to_string(),
            source,
            born,
            pulse,
        });
        id
    })
}
