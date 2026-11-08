// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `AudioSource` and `AudioListener`, the Unity-style components over [`crate::audio`]
//! (`js/engine/src/audio-components.ts`). An `AudioSource` plays sounds from its GameObject (an
//! emitter that follows it); the `AudioListener` puts the player's ears on its object.
//!
//! The host calls [`pre_render`] once a frame after the transforms settled (what `onPreRender` did)
//! and [`crate::audio::ended`] when playbacks finish. `process_destroyed` removes the components
//! of destroyed objects. Both components register scene-file kinds with [`register_kinds`].

use crate::audio::{
    self, BusRef, EmitterRef, PlayOptions, PlaybackRef, SoundRef, create_emitter, move_emitter,
    play_sound, remove_track, set_playback,
};
use platypus_math::V3;
use platypus_scene::ObjectId;
use platypus_scene::api::with_scene;
use std::cell::RefCell;
use ts_api::{Handle, ts_api};

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
    /// An `AudioSource` by number (1-based: 0 is "none").
    AudioSourceRef
);
numbered_handle!(
    /// An `AudioListener` by number (1-based: 0 is "none").
    AudioListenerRef
);

/// What an `AudioSource` is made of (its `@property` fields and the playback state).
#[derive(Clone, Debug)]
pub struct SourceData {
    pub object: ObjectId,
    /// The sound `play()` plays (0: none).
    pub clip: SoundRef,
    pub play_on_awake: bool,
    /// 0 plays flat (2D), above 0 plays from the object (3D).
    pub spatial_blend: f64,
    pub min_distance: f64,
    pub max_distance: f64,
    pub output_bus: Option<BusRef>,
    pub volume: f64,
    pub pitch: f64,
    pub pan_stereo: f64,
    pub mute: bool,
    pub looped: bool,
    /// The emitter gets the full effect chain (high-pass, reverb, echo), not only the low-pass
    /// occlusion drives. Applies when the emitter is made, so set it before playing.
    pub effects: bool,
    /// How much the pitch follows the source's speed towards or away from the listener
    /// (0 none, 1 as in life). Acoustics applies it.
    pub doppler_level: f64,
    /// How far (metres) a `SoundSensor` can hear this source at full volume; 0 is silent to sensors.
    pub loudness: f64,
    /// How much occluders in the way dull this source (0 none, 1 fully). Acoustics applies it.
    pub occlusion: f64,
    /// The pitch factor acoustics found for the source's speed (1: none): playbacks run at
    /// `pitch * doppler_factor`.
    doppler_factor: f64,
    main: Option<PlaybackRef>,
    paused: bool,
    all: Vec<PlaybackRef>,
    emitter: Option<EmitterRef>,
    emitter_key: String,
    sent_position: Option<[f64; 3]>,
}

#[derive(Default)]
struct State {
    sources: Vec<Option<SourceData>>,
    listeners: Vec<Option<(ObjectId, Option<[f64; 7]>)>>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn source<R>(s: AudioSourceRef, f: impl FnOnce(&mut SourceData) -> R) -> Option<R> {
    st(|st| {
        st.sources
            .get_mut((s.0 as usize).checked_sub(1)?)?
            .as_mut()
            .map(f)
    })
}

/// Runs `f` on an `AudioSource`'s data (the scene-file kind reads and writes through this).
pub fn with_source<R>(s: AudioSourceRef, f: impl FnOnce(&mut SourceData) -> R) -> Option<R> {
    source(s, f)
}

/// Forgets every component (a test, or a reload).
pub fn reset() {
    st(|s| *s = State::default());
}

fn world_position(object: ObjectId) -> [f64; 3] {
    with_scene(|s| {
        let p = s.world(object).position;
        [f64::from(p.x), f64::from(p.y), f64::from(p.z)]
    })
    .unwrap_or([0.0; 3])
}

fn world_pose(object: ObjectId) -> [f64; 7] {
    with_scene(|s| {
        let t = s.world(object);
        let (p, r) = (t.position, t.rotation);
        [
            f64::from(p.x),
            f64::from(p.y),
            f64::from(p.z),
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.z),
            f64::from(r.w),
        ]
    })
    .unwrap_or([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0])
}

fn gain(d: &SourceData) -> f64 {
    if d.mute { 0.0 } else { d.volume }
}

impl SourceData {
    fn new(object: ObjectId) -> SourceData {
        SourceData {
            object,
            clip: SoundRef(0),
            play_on_awake: true,
            spatial_blend: 0.0,
            min_distance: 1.0,
            max_distance: 50.0,
            output_bus: None,
            volume: 1.0,
            pitch: 1.0,
            pan_stereo: 0.0,
            mute: false,
            looped: false,
            effects: false,
            doppler_level: 0.0,
            loudness: 0.0,
            occlusion: 1.0,
            doppler_factor: 1.0,
            main: None,
            paused: false,
            all: Vec::new(),
            emitter: None,
            emitter_key: String::new(),
            sent_position: None,
        }
    }

    /// Where its sounds go: its emitter (made or remade when the 3D settings change) or bus.
    fn output(&mut self) -> (Option<BusRef>, Option<EmitterRef>) {
        if self.spatial_blend <= 0.0 {
            return (self.output_bus, None);
        }
        let key = format!(
            "{},{},{},{}",
            self.min_distance,
            self.max_distance,
            self.output_bus.map_or(String::new(), |b| b.0.to_string()),
            self.effects
        );
        if self.emitter.is_none() || key != self.emitter_key {
            // Remaking it stops what played on the old one, as moving the source would in Unity.
            if let Some(e) = self.emitter.take() {
                remove_track(e.0);
            }
            self.all.clear();
            self.main = None;
            let p = world_position(self.object);
            self.sent_position = Some(p);
            self.emitter = Some(create_emitter(
                p,
                self.min_distance,
                self.max_distance,
                self.output_bus,
                self.effects,
            ));
            self.emitter_key = key;
        }
        (None, self.emitter)
    }

    fn start(&mut self, sound: SoundRef, volume: f64, looped: bool) -> PlaybackRef {
        let (bus, emitter) = self.output();
        let p = play_sound(
            sound,
            PlayOptions {
                volume,
                rate: self.pitch * self.doppler_factor,
                pan: self.pan_stereo,
                looped,
                bus,
                emitter,
                ..PlayOptions::default()
            },
        );
        self.all.push(p);
        p
    }

    pub fn play(&mut self) {
        if !audio::sound_exists(self.clip) {
            platypus_log("AudioSource has no clip");
            return;
        }
        if let Some(m) = self.main {
            audio::stop_playback(m, 0.0);
        }
        self.paused = false;
        let (g, looped, clip) = (gain(self), self.looped, self.clip);
        self.main = Some(self.start(clip, g, looped));
    }

    fn each_playing(&self, f: impl Fn(PlaybackRef)) {
        for p in &self.all {
            f(*p);
        }
    }

    fn destroy(&mut self) {
        for p in &self.all {
            audio::stop_playback(*p, 0.0);
        }
        self.all.clear();
        self.main = None;
        if let Some(e) = self.emitter.take() {
            remove_track(e.0);
        }
    }
}

fn platypus_log(message: &str) {
    crate::log::debug_api::log_warning(message);
}

/// Keeps emitters on their objects (only sending real moves) and the listener on its object.
/// Call once a frame after transforms are final.
pub fn pre_render() {
    let sources: Vec<(usize, ObjectId, EmitterRef, Option<[f64; 3]>)> = st(|s| {
        s.sources
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let d = d.as_ref()?;
                Some((i, d.object, d.emitter?, d.sent_position))
            })
            .collect()
    });
    for (i, object, emitter, sent) in sources {
        let p = world_position(object);
        if sent == Some(p) {
            continue;
        }
        move_emitter(emitter, p, 0.0);
        st(|s| {
            if let Some(Some(d)) = s.sources.get_mut(i) {
                d.sent_position = Some(p);
            }
        });
    }
    let listeners: Vec<(usize, ObjectId, Option<[f64; 7]>)> = st(|s| {
        s.listeners
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.map(|(o, sent)| (i, o, sent)))
            .collect()
    });
    for (i, object, sent) in listeners {
        let pose = world_pose(object);
        if sent == Some(pose) {
            continue;
        }
        audio::set_listener(
            [pose[0], pose[1], pose[2]],
            [pose[3], pose[4], pose[5], pose[6]],
        );
        st(|s| {
            if let Some(Some(l)) = s.listeners.get_mut(i) {
                l.1 = Some(pose);
            }
        });
    }
}

/// What the acoustics system reads of an `AudioSource`.
#[derive(Clone, Copy, Debug)]
pub struct SourceView {
    pub source: AudioSourceRef,
    pub object: ObjectId,
    /// The emitter it plays from (3D sources that have played).
    pub emitter: Option<EmitterRef>,
    /// Something it started is still playing.
    pub playing: bool,
    pub max_distance: f64,
    pub volume: f64,
    pub doppler_level: f64,
    pub loudness: f64,
    pub occlusion: f64,
    pub effects: bool,
}

/// Every `AudioSource` (those on destroyed objects are gone already).
pub fn source_views() -> Vec<SourceView> {
    st(|s| {
        s.sources
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let d = d.as_ref()?;
                Some(SourceView {
                    source: AudioSourceRef(i as u32 + 1),
                    object: d.object,
                    emitter: d.emitter,
                    playing: d.all.iter().any(|p| audio::is_playing(*p)) && !d.paused,
                    max_distance: d.max_distance,
                    volume: gain(d),
                    doppler_level: d.doppler_level,
                    loudness: d.loudness,
                    occlusion: d.occlusion,
                    effects: d.effects,
                })
            })
            .collect()
    })
}

/// Gives a source the pitch factor of its speed (see `SourceData`): its playbacks change rate
/// when the factor does.
pub fn set_doppler_factor(s: AudioSourceRef, factor: f64) {
    source(s, |d| {
        if (d.doppler_factor - factor).abs() < 1e-4 {
            return;
        }
        d.doppler_factor = factor;
        let rate = d.pitch * factor;
        // A short glide so the pitch slides between frames instead of stepping.
        d.each_playing(|p| set_playback(p, "rate", rate, 0.05));
    });
}

/// Playbacks the mixer finished: sources forget them.
pub fn playbacks_ended(ids: &[PlaybackRef]) {
    st(|s| {
        for d in s.sources.iter_mut().flatten() {
            d.all.retain(|p| !ids.contains(p));
            if d.main.is_some_and(|m| ids.contains(&m)) {
                // `main` stays set (a finished clip is "not playing"), as in the TypeScript.
            }
        }
    });
}

/// Removes the components of a destroyed object (stopping what its sources play).
pub fn remove_on(object: ObjectId) {
    platypus_kinds::components_changed();
    st(|s| {
        for slot in s.sources.iter_mut() {
            if slot.as_ref().is_some_and(|d| d.object == object) {
                if let Some(d) = slot.as_mut() {
                    d.destroy();
                }
                *slot = None;
            }
        }
        for slot in s.listeners.iter_mut() {
            if slot.is_some_and(|(o, _)| o == object) {
                *slot = None;
            }
        }
    });
}

/// The `AudioSource`s on an object, in the order they were added.
pub fn sources_on(object: ObjectId) -> Vec<AudioSourceRef> {
    st(|s| {
        s.sources
            .iter()
            .enumerate()
            .filter(|(_, d)| d.as_ref().is_some_and(|d| d.object == object))
            .map(|(i, _)| AudioSourceRef(i as u32 + 1))
            .collect()
    })
}

/// Every `AudioSource`'s object, in the order they were added.
pub fn source_objects() -> Vec<ObjectId> {
    st(|s| s.sources.iter().flatten().map(|d| d.object).collect())
}

/// The `AudioListener`s on an object.
pub fn listeners_on(object: ObjectId) -> Vec<AudioListenerRef> {
    st(|s| {
        s.listeners
            .iter()
            .enumerate()
            .filter(|(_, l)| l.is_some_and(|(o, _)| o == object))
            .map(|(i, _)| AudioListenerRef(i as u32 + 1))
            .collect()
    })
}

pub fn remove_source(s: AudioSourceRef) {
    platypus_kinds::components_changed();
    st(|st| {
        if let Some(slot) = st.sources.get_mut((s.0 as usize).wrapping_sub(1)) {
            if let Some(d) = slot.as_mut() {
                d.destroy();
            }
            *slot = None;
        }
    });
}

pub fn remove_listener(l: AudioListenerRef) {
    st(|s| {
        if let Some(slot) = s.listeners.get_mut((l.0 as usize).wrapping_sub(1)) {
            *slot = None;
        }
    });
}

pub fn object_of_source(s: AudioSourceRef) -> Option<ObjectId> {
    source(s, |d| d.object)
}

pub fn object_of_listener(l: AudioListenerRef) -> Option<ObjectId> {
    st(|s| {
        s.listeners
            .get((l.0 as usize).wrapping_sub(1))?
            .map(|(o, _)| o)
    })
}

pub fn attach_source(object: ObjectId) -> AudioSourceRef {
    platypus_kinds::components_changed();
    st(|s| {
        s.sources.push(Some(SourceData::new(object)));
        AudioSourceRef(s.sources.len() as u32)
    })
}

pub fn attach_listener(object: ObjectId) -> AudioListenerRef {
    st(|s| {
        s.listeners.push(Some((object, None)));
        AudioListenerRef(s.listeners.len() as u32)
    })
}

/// Wakes sources made by a scene load: those with `playOnAwake` and a clip play.
pub fn awake_sources(list: &[AudioSourceRef]) {
    for s in list {
        source(*s, |d| {
            if d.play_on_awake && audio::sound_exists(d.clip) {
                d.play();
            }
        });
    }
}

/// Wakes listeners: the first position is sent.
pub fn awake_listeners() {
    pre_render();
}

fn none_object() -> ObjectId {
    ObjectId {
        index: u32::MAX,
        generation: u32::MAX,
    }
}

#[ts_api(
    namespace = "AudioSource", handles(
        AudioSource = AudioSourceRef,
        GameObject = ObjectId,
        Sound = SoundRef,
        Playback = PlaybackRef,
        Bus = BusRef,
        Emitter = EmitterRef
    ),
    values(Vector3 = V3 [x: f64, y: f64, z: f64])
)]
pub mod audio_source_api {
    use super::*;

    impl AudioSourceRef {
        /// Adds an `AudioSource` to a GameObject (what `go.addComponent(AudioSource)` does).
        pub fn attach(object: ObjectId) -> AudioSourceRef {
            super::attach_source(object)
        }

        /// The first `AudioSource` on an object, or none.
        pub fn find_on(object: ObjectId) -> AudioSourceRef {
            super::sources_on(object)
                .first()
                .copied()
                .unwrap_or(AudioSourceRef(0))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            super::object_of_source(self).unwrap_or_else(super::none_object)
        }

        /// The sound `play()` plays.
        #[property]
        pub fn clip(self) -> SoundRef {
            super::source(self, |d| d.clip).unwrap_or(SoundRef(0))
        }

        pub fn set_clip(self, clip: SoundRef) {
            super::source(self, |d| d.clip = clip);
        }

        /// The clip as a path in the game folder (loaded once and shared); the TypeScript's
        /// `clip = "path"`.
        #[property]
        pub fn clip_path(self) -> String {
            audio::sound_path(super::source(self, |d| d.clip).unwrap_or(SoundRef(0)))
        }

        pub fn set_clip_path(self, path: &str) {
            let clip = if path.is_empty() {
                SoundRef(0)
            } else {
                audio::cached_sound(path)
            };
            super::source(self, |d| d.clip = clip);
        }

        /// Plays `clip` when the component wakes (default true).
        #[property]
        pub fn play_on_awake(self) -> bool {
            super::source(self, |d| d.play_on_awake).unwrap_or(true)
        }

        pub fn set_play_on_awake(self, on: bool) {
            super::source(self, |d| d.play_on_awake = on);
        }

        /// 0 plays flat (2D), above 0 plays from the object (3D; there is no partial blend).
        #[property]
        pub fn spatial_blend(self) -> f64 {
            super::source(self, |d| d.spatial_blend).unwrap_or(0.0)
        }

        pub fn set_spatial_blend(self, v: f64) {
            super::source(self, |d| d.spatial_blend = v);
        }

        /// Full volume within this distance (metres, 3D only).
        #[property]
        pub fn min_distance(self) -> f64 {
            super::source(self, |d| d.min_distance).unwrap_or(1.0)
        }

        pub fn set_min_distance(self, v: f64) {
            super::source(self, |d| d.min_distance = v);
        }

        /// Silent beyond this distance (metres, 3D only).
        #[property]
        pub fn max_distance(self) -> f64 {
            super::source(self, |d| d.max_distance).unwrap_or(50.0)
        }

        pub fn set_max_distance(self, v: f64) {
            super::source(self, |d| d.max_distance = v);
        }

        /// The bus it plays on (or its emitter mixes into); none plays on the main mix.
        #[property]
        pub fn output_bus(self) -> BusRef {
            super::source(self, |d| d.output_bus)
                .flatten()
                .unwrap_or(BusRef(0))
        }

        pub fn set_output_bus(self, bus: BusRef) {
            super::source(self, |d| d.output_bus = (bus.0 != 0).then_some(bus));
        }

        /// Linear gain (1 = as recorded).
        #[property]
        pub fn volume(self) -> f64 {
            super::source(self, |d| d.volume).unwrap_or(1.0)
        }

        pub fn set_volume(self, v: f64) {
            super::source(self, |d| {
                d.volume = v;
                let g = super::gain(d);
                d.each_playing(|p| set_playback(p, "volume", g, 0.0));
            });
        }

        /// Speed and pitch together (1 = as recorded).
        #[property]
        pub fn pitch(self) -> f64 {
            super::source(self, |d| d.pitch).unwrap_or(1.0)
        }

        pub fn set_pitch(self, v: f64) {
            super::source(self, |d| {
                d.pitch = v;
                let rate = v * d.doppler_factor;
                d.each_playing(|p| set_playback(p, "rate", rate, 0.0));
            });
        }

        /// The emitter has the full effect chain (high-pass, reverb, echo) as well as the
        /// low-pass. Set it before the first `play()`.
        #[property]
        pub fn effects(self) -> bool {
            super::source(self, |d| d.effects).unwrap_or(false)
        }

        pub fn set_effects(self, on: bool) {
            super::source(self, |d| d.effects = on);
        }

        /// How much pitch follows the source's speed towards or away from the listener (0 none,
        /// 1 as in life, above 1 exaggerated): a passing car, someone shouting from a moving
        /// truck. Needs `Acoustics` (the host runs it each frame).
        #[property]
        pub fn doppler_level(self) -> f64 {
            super::source(self, |d| d.doppler_level).unwrap_or(0.0)
        }

        pub fn set_doppler_level(self, v: f64) {
            super::source(self, |d| d.doppler_level = v.max(0.0));
        }

        /// How far (metres) a `SoundSensor` hears this source at full volume; 0 (default) means
        /// sensors do not hear it. It counts while the source is playing.
        #[property]
        pub fn loudness(self) -> f64 {
            super::source(self, |d| d.loudness).unwrap_or(0.0)
        }

        pub fn set_loudness(self, v: f64) {
            super::source(self, |d| d.loudness = v.max(0.0));
        }

        /// How much occluders (walls, doors, floors) between the source and the listener
        /// muffle it: 0 not at all, 1 fully (default).
        #[property]
        pub fn occlusion(self) -> f64 {
            super::source(self, |d| d.occlusion).unwrap_or(1.0)
        }

        pub fn set_occlusion(self, v: f64) {
            super::source(self, |d| d.occlusion = v.clamp(0.0, 1.0));
        }

        /// The emitter this source plays from (0 for a flat 2D source or before it plays):
        /// for `setLowPass`, `setReverb` and the rest of an emitter's effects.
        #[property]
        pub fn emitter(self) -> EmitterRef {
            super::source(self, |d| d.emitter)
                .flatten()
                .unwrap_or(EmitterRef(0))
        }

        /// -1 left to 1 right, for 2D sounds.
        #[property]
        pub fn pan_stereo(self) -> f64 {
            super::source(self, |d| d.pan_stereo).unwrap_or(0.0)
        }

        pub fn set_pan_stereo(self, v: f64) {
            super::source(self, |d| {
                d.pan_stereo = v;
                d.each_playing(|p| set_playback(p, "pan", v, 0.0));
            });
        }

        #[property]
        pub fn mute(self) -> bool {
            super::source(self, |d| d.mute).unwrap_or(false)
        }

        pub fn set_mute(self, m: bool) {
            super::source(self, |d| {
                d.mute = m;
                let g = super::gain(d);
                d.each_playing(|p| set_playback(p, "volume", g, 0.0));
            });
        }

        /// Whether `play()` repeats the clip. Applies from the next `play()`.
        #[property]
        pub fn r#loop(self) -> bool {
            super::source(self, |d| d.looped).unwrap_or(false)
        }

        pub fn set_loop(self, l: bool) {
            super::source(self, |d| d.looped = l);
        }

        /// Whether the clip from `play()` is playing (paused counts as not).
        #[property]
        pub fn is_playing(self) -> bool {
            super::source(self, |d| d.main.is_some_and(audio::is_playing) && !d.paused)
                .unwrap_or(false)
        }

        /// Plays `clip` from the start, replacing what `play()` played before.
        pub fn play(self) {
            super::source(self, |d| d.play());
        }

        /// Stops the clip from `play()` (one-shots carry on).
        pub fn stop(self) {
            super::source(self, |d| {
                if let Some(m) = d.main.take() {
                    audio::stop_playback(m, 0.0);
                }
                d.paused = false;
            });
        }

        pub fn pause(self) {
            super::source(self, |d| {
                if let Some(m) = d.main
                    && audio::is_playing(m)
                    && !d.paused
                {
                    audio::pause_playback(m, 0.0);
                    d.paused = true;
                }
            });
        }

        pub fn un_pause(self) {
            super::source(self, |d| {
                if d.paused
                    && let Some(m) = d.main
                {
                    audio::resume_playback(m, 0.0);
                    d.paused = false;
                }
            });
        }

        /// Plays `clip` once over whatever else plays (footsteps, hits), at `volumeScale` x `volume`.
        #[optional(volume_scale = 1)]
        pub fn play_one_shot(self, clip: SoundRef, volume_scale: f64) {
            super::source(self, |d| {
                let g = super::gain(d) * volume_scale;
                d.start(clip, g, false);
            });
        }

        /// `playOneShot` with a path in the game folder (a separate name: calls are not chosen by argument type).
        #[optional(volume_scale = 1)]
        pub fn play_one_shot_path(self, clip: &str, volume_scale: f64) {
            let clip = audio::cached_sound(clip);
            super::source(self, |d| {
                let g = super::gain(d) * volume_scale;
                d.start(clip, g, false);
            });
        }

        /// Plays `clip` once at `position` in the world, from a temporary emitter that goes
        /// away with the sound.
        #[optional(volume = 1)]
        pub fn play_clip_at_point(clip: SoundRef, position: V3, volume: f64) -> PlaybackRef {
            let emitter =
                create_emitter([position.x, position.y, position.z], 1.0, 50.0, None, false);
            let p = play_sound(
                clip,
                PlayOptions {
                    volume,
                    emitter: Some(emitter),
                    ..PlayOptions::default()
                },
            );
            super::temp_emitter(p, emitter);
            p
        }
    }
}

thread_local! {
    /// Emitters made by `playClipAtPoint`, removed when their playback ends.
    static TEMP_EMITTERS: RefCell<Vec<(PlaybackRef, EmitterRef)>> = const { RefCell::new(Vec::new()) };
}

fn temp_emitter(p: PlaybackRef, e: EmitterRef) {
    TEMP_EMITTERS.with(|t| t.borrow_mut().push((p, e)));
}

/// Removes the temporary emitters of finished playbacks (called from [`crate::audio::ended`]).
pub(crate) fn finished_temp(ids: &[PlaybackRef]) {
    TEMP_EMITTERS.with(|t| {
        t.borrow_mut().retain(|(p, e)| {
            if ids.contains(p) {
                remove_track(e.0);
                false
            } else {
                true
            }
        });
    });
}

#[ts_api(
    namespace = "AudioListener", handles(AudioListener = AudioListenerRef, GameObject = ObjectId)
)]
pub mod audio_listener_api {
    use super::*;

    impl AudioListenerRef {
        /// Adds an `AudioListener` to a GameObject.
        pub fn attach(object: ObjectId) -> AudioListenerRef {
            let l = super::attach_listener(object);
            super::awake_listeners();
            l
        }

        /// The first `AudioListener` on an object, or none.
        pub fn find_on(object: ObjectId) -> AudioListenerRef {
            super::listeners_on(object)
                .first()
                .copied()
                .unwrap_or(AudioListenerRef(0))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            super::object_of_listener(self).unwrap_or_else(super::none_object)
        }

        /// The volume of everything (linear): the TypeScript's static `AudioListener.volume`.
        pub fn get_volume() -> f64 {
            audio::audio_api::master_volume()
        }

        pub fn set_volume(volume: f64) {
            audio::set_master_volume(volume, 0.0);
        }
    }
}

// ---- scene-file kinds ----

use platypus_kinds::Kind;
use serde_json::{Map, Value, json};

fn number(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn sound_value(clip: SoundRef) -> Value {
    if !audio::sound_exists(clip) {
        return Value::Null;
    }
    let path = audio::sound_path(clip);
    let guid = crate::assets::assets_api::path_to_guid(&path);
    let mut m = Map::new();
    m.insert("$type".into(), json!("Sound"));
    m.insert("src".into(), json!(path));
    if !guid.is_empty() {
        m.insert("guid".into(), json!(guid));
    }
    Value::Object(m)
}

/// A saved `clip`: a `Sound` object (by GUID when the file is known) or a plain path.
fn clip_of(v: &Value) -> Option<SoundRef> {
    match v {
        Value::Null => Some(SoundRef(0)),
        Value::String(path) => Some(audio::cached_sound(path)),
        Value::Object(o) if o.get("$type").and_then(Value::as_str) == Some("Sound") => {
            let src = o.get("src").and_then(Value::as_str).unwrap_or_default();
            let path = match o.get("guid").and_then(Value::as_str) {
                Some(g) => {
                    let p = crate::assets::assets_api::guid_to_path(g);
                    if p.is_empty() { src.to_string() } else { p }
                }
                None => src.to_string(),
            };
            Some(audio::cached_sound(&path))
        }
        _ => None,
    }
}

fn source_kind() -> Kind {
    Kind {
        name: "AudioSource".into(),
        create: Box::new(|o| Ok(u64::from(attach_source(ObjectId::from_raw(o)).0))),
        on: Box::new(|o| {
            sources_on(ObjectId::from_raw(o))
                .into_iter()
                .map(|s| u64::from(s.0))
                .collect()
        }),
        save: Box::new(|h| {
            let mut m = Map::new();
            with_source(AudioSourceRef(h as u32), |d| {
                m.insert("clip".into(), sound_value(d.clip));
                m.insert("playOnAwake".into(), json!(d.play_on_awake));
                m.insert("spatialBlend".into(), json!(d.spatial_blend));
                m.insert("minDistance".into(), json!(d.min_distance));
                m.insert("maxDistance".into(), json!(d.max_distance));
                m.insert("volume".into(), json!(d.volume));
                m.insert("pitch".into(), json!(d.pitch));
                m.insert("panStereo".into(), json!(d.pan_stereo));
                m.insert("mute".into(), json!(d.mute));
                m.insert("loop".into(), json!(d.looped));
                m.insert("effects".into(), json!(d.effects));
                m.insert("dopplerLevel".into(), json!(d.doppler_level));
                m.insert("loudness".into(), json!(d.loudness));
                m.insert("occlusion".into(), json!(d.occlusion));
            });
            m
        }),
        load: Box::new(|h, props| {
            let mut warnings = Vec::new();
            with_source(AudioSourceRef(h as u32), |d| {
                for (k, v) in props {
                    let ok = match k.as_str() {
                        "clip" => clip_of(v).map(|c| d.clip = c),
                        "playOnAwake" => v.as_bool().map(|x| d.play_on_awake = x),
                        "spatialBlend" => number(v).map(|x| d.spatial_blend = x),
                        "minDistance" => number(v).map(|x| d.min_distance = x),
                        "maxDistance" => number(v).map(|x| d.max_distance = x),
                        "volume" => number(v).map(|x| d.volume = x),
                        "pitch" => number(v).map(|x| d.pitch = x),
                        "panStereo" => number(v).map(|x| d.pan_stereo = x),
                        "mute" => v.as_bool().map(|x| d.mute = x),
                        "loop" => v.as_bool().map(|x| d.looped = x),
                        "effects" => v.as_bool().map(|x| d.effects = x),
                        "dopplerLevel" => number(v).map(|x| d.doppler_level = x),
                        "loudness" => number(v).map(|x| d.loudness = x),
                        "occlusion" => number(v).map(|x| d.occlusion = x),
                        other => {
                            warnings.push(format!("no property `{other}` yet; dropped"));
                            continue;
                        }
                    };
                    if ok.is_none() {
                        warnings.push(format!("property `{k}` has a value of the wrong shape"));
                    }
                }
            });
            warnings
        }),
        awake: Box::new(|hs| {
            let list: Vec<AudioSourceRef> = hs.iter().map(|h| AudioSourceRef(*h as u32)).collect();
            awake_sources(&list);
        }),
        object: Box::new(|h| object_of_source(AudioSourceRef(h as u32)).map(|o| o.to_raw())),
        destroy: Box::new(|h| remove_source(AudioSourceRef(h as u32))),
    }
}

fn listener_kind() -> Kind {
    Kind {
        name: "AudioListener".into(),
        create: Box::new(|o| Ok(u64::from(attach_listener(ObjectId::from_raw(o)).0))),
        on: Box::new(|o| {
            listeners_on(ObjectId::from_raw(o))
                .into_iter()
                .map(|l| u64::from(l.0))
                .collect()
        }),
        save: Box::new(|_| Map::new()),
        load: Box::new(|_, props| {
            props
                .keys()
                .map(|k| format!("no property `{k}`; dropped"))
                .collect()
        }),
        awake: Box::new(|_| awake_listeners()),
        object: Box::new(|h| object_of_listener(AudioListenerRef(h as u32)).map(|o| o.to_raw())),
        destroy: Box::new(|h| remove_listener(AudioListenerRef(h as u32))),
    }
}

/// Registers the `AudioSource` and `AudioListener` scene-file kinds on this thread.
pub fn register_kinds() {
    platypus_kinds::register(source_kind());
    platypus_kinds::register(listener_kind());
}
