// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `audio`: sounds, playbacks, buses, emitters and the listener, as `js/engine/src/audio.ts` has
//! them. Scripts hold handles; every change is recorded as an [`AudioCommand`] that the host takes
//! with [`take_commands`] and runs on the mixer (`platypus_audio`): [`AudioCommand::to_op`] gives the
//! `audio.*` op name and arguments `Mixer::apply` takes (a `Load` needs the file's bytes, so the
//! host reads the file itself). When playbacks finish the host calls [`ended`].
//!
//! Options of `play` are trailing parameters, written as an object:
//! `sound.play({ volume: 0.5, loop: true, bus: music })`. Differences from the TypeScript: `on:
//! Bus | Emitter` is `bus` and `emitter`; `playback.onEnded = f` is `playback.onEnded(f)`; the
//! `Sound` has `unload()` as before.
//!
//! The Unity-style components over these (`AudioSource`, `AudioListener`) are in
//! [`crate::audio_components`].

use crate::assets;
use platypus_math::{Q, V3};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use ts_api::{Callback, Handle, ts_api};

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
    /// A loaded sound (1-based; 0 is "no sound").
    SoundRef
);
numbered_handle!(
    /// A playing (or paused) sound (1-based).
    PlaybackRef
);
numbered_handle!(
    /// A mixer bus (1-based, numbered with the emitters: a track id).
    BusRef
);
numbered_handle!(
    /// A point sounds play from (1-based, numbered with the buses: a track id).
    EmitterRef
);

/// What a script asked the audio system to do. The numbers are the ids scripts see.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioCommand {
    Load {
        sound: SoundRef,
        path: String,
        stream: bool,
    },
    Unload(SoundRef),
    Play {
        sound: SoundRef,
        playback: PlaybackRef,
        /// The bus or emitter (a track id) it plays on.
        track: Option<u32>,
        options: PlayOptions,
    },
    /// `volume`, `rate` or `pan`, over `fade` seconds.
    Set {
        playback: PlaybackRef,
        property: &'static str,
        value: f64,
        fade: f64,
    },
    Pause(PlaybackRef, f64),
    Resume(PlaybackRef, f64),
    Stop(PlaybackRef, f64),
    Seek(PlaybackRef, f64),
    Bus {
        bus: BusRef,
        parent: Option<u32>,
        volume: f64,
        /// 0 none, 1 low-pass only, 2 the full chain (low-pass, high-pass, reverb, echo).
        fx: u8,
    },
    Emitter {
        emitter: EmitterRef,
        position: [f64; 3],
        distances: [f64; 2],
        bus: Option<u32>,
        /// As for `Bus`.
        fx: u8,
    },
    /// An effect parameter of a track made with effects (`lowpass`, `highpass`, `reverb`,
    /// `reverbSize`, `reverbDamping`, `echo`), over `fade` seconds.
    Fx {
        track: u32,
        property: String,
        value: f64,
        fade: f64,
    },
    /// `None` is the master volume.
    TrackVolume {
        track: Option<u32>,
        volume: f64,
        fade: f64,
    },
    TrackPause(u32, f64),
    TrackResume(u32, f64),
    EmitterPosition {
        emitter: EmitterRef,
        position: [f64; 3],
        glide: f64,
    },
    RemoveTrack(u32),
    Listener {
        position: [f64; 3],
        rotation: [f64; 4],
    },
}

impl AudioCommand {
    /// The `audio.*` op for `Mixer::apply` (the name without `audio.`), or `None` for `Load`,
    /// which the host does itself.
    pub fn to_op(&self) -> Option<(&'static str, Vec<Value>)> {
        let id = |t: Option<u32>| t.map_or(Value::Null, |t| json!(t));
        Some(match self {
            AudioCommand::Load { .. } => return None,
            AudioCommand::Unload(s) => ("unload", vec![json!(s.0)]),
            AudioCommand::Play {
                sound,
                playback,
                track,
                options: o,
            } => (
                "play",
                vec![
                    json!(playback.0),
                    json!(sound.0),
                    id(*track),
                    json!([o.volume, o.rate, o.pan, o.looped, o.fade_in, o.start]),
                ],
            ),
            AudioCommand::Set {
                playback,
                property,
                value,
                fade,
            } => (
                "set",
                vec![
                    json!(playback.0),
                    json!(property),
                    json!(value),
                    json!(fade),
                ],
            ),
            AudioCommand::Pause(p, f) => ("pause", vec![json!(p.0), json!(f)]),
            AudioCommand::Resume(p, f) => ("resume", vec![json!(p.0), json!(f)]),
            AudioCommand::Stop(p, f) => ("stop", vec![json!(p.0), json!(f)]),
            AudioCommand::Seek(p, s) => ("seek", vec![json!(p.0), json!(s)]),
            AudioCommand::Bus {
                bus,
                parent,
                volume,
                fx,
            } => (
                "bus",
                vec![json!(bus.0), id(*parent), json!(volume), json!(fx)],
            ),
            AudioCommand::Emitter {
                emitter,
                position,
                distances,
                bus,
                fx,
            } => (
                "emitter",
                vec![
                    json!(emitter.0),
                    json!(position),
                    json!(distances),
                    id(*bus),
                    json!(fx),
                ],
            ),
            AudioCommand::Fx {
                track,
                property,
                value,
                fade,
            } => (
                "fx",
                vec![json!(track), json!(property), json!(value), json!(fade)],
            ),
            AudioCommand::TrackVolume {
                track,
                volume,
                fade,
            } => ("trackVolume", vec![id(*track), json!(volume), json!(fade)]),
            AudioCommand::TrackPause(t, f) => ("trackPause", vec![json!(t), json!(f)]),
            AudioCommand::TrackResume(t, f) => ("trackResume", vec![json!(t), json!(f)]),
            AudioCommand::EmitterPosition {
                emitter,
                position,
                glide,
            } => (
                "emitterPosition",
                vec![json!(emitter.0), json!(position), json!(glide)],
            ),
            AudioCommand::RemoveTrack(t) => ("removeTrack", vec![json!(t)]),
            AudioCommand::Listener { position, rotation } => {
                ("listener", vec![json!(position), json!(rotation)])
            }
        })
    }
}

/// How a playback starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayOptions {
    /// Linear gain.
    pub volume: f64,
    /// Speed and pitch.
    pub rate: f64,
    /// -1 left, 0 centre, 1 right.
    pub pan: f64,
    pub looped: bool,
    /// Seconds to fade in over.
    pub fade_in: f64,
    /// Seconds into the sound to start.
    pub start: f64,
    pub bus: Option<BusRef>,
    pub emitter: Option<EmitterRef>,
}

impl Default for PlayOptions {
    fn default() -> Self {
        PlayOptions {
            volume: 1.0,
            rate: 1.0,
            pan: 0.0,
            looped: false,
            fade_in: 0.0,
            start: 0.0,
            bus: None,
            emitter: None,
        }
    }
}

struct SoundData {
    path: String,
}

struct PlaybackData {
    sound: SoundRef,
    playing: bool,
    paused: bool,
    on_ended: Vec<Callback<()>>,
}

#[derive(Default)]
struct State {
    sounds: Vec<SoundData>,
    playbacks: Vec<PlaybackData>,
    next_track: u32,
    commands: Vec<AudioCommand>,
    /// `audio.play(path)` loads a path once.
    cache: HashMap<String, SoundRef>,
    master: f64,
    listener: Option<([f64; 3], [f64; 4])>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { master: 1.0, ..State::default() });
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Where the listener last was set (position and rotation), if a script set one.
pub fn listener() -> Option<([f64; 3], [f64; 4])> {
    st(|s| s.listener)
}

/// The commands since the last call, in order.
pub fn take_commands() -> Vec<AudioCommand> {
    st(|s| std::mem::take(&mut s.commands))
}

/// Forgets every sound, playback and command (a test, or a reload).
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
    st(|s| s.master = 1.0);
    crate::audio_components::reset();
}

/// Playbacks that finished (played out or were stopped), as `Mixer::poll` reports them:
/// `onEnded` callbacks run, and the `AudioSource`s forget them.
pub fn ended(ids: &[PlaybackRef]) {
    let mut callbacks = Vec::new();
    st(|s| {
        for id in ids {
            if let Some(p) = s.playbacks.get_mut((id.0 as usize).wrapping_sub(1))
                && p.playing
            {
                p.playing = false;
                callbacks.extend(std::mem::take(&mut p.on_ended));
            }
        }
    });
    crate::audio_components::playbacks_ended(ids);
    crate::audio_components::finished_temp(ids);
    for c in callbacks {
        c.call(());
    }
}

/// Where the sound came from, whether it is a GUID or a path.
fn source_of(src: &str) -> String {
    assets::resolve(src)
}

/// Loads a sound (a GUID source becomes its path; the file's `.meta` can ask for streaming).
pub fn load_sound(src: &str, stream: Option<bool>) -> SoundRef {
    let path = source_of(src);
    let stream = stream.unwrap_or_else(|| {
        serde_json::from_str::<Value>(&assets_importer(&path))
            .ok()
            .and_then(|v| v.get("stream").and_then(Value::as_bool))
            .unwrap_or(false)
    });
    st(|s| {
        s.sounds.push(SoundData { path: path.clone() });
        let sound = SoundRef(s.sounds.len() as u32);
        s.commands.push(AudioCommand::Load {
            sound,
            path,
            stream,
        });
        sound
    })
}

fn assets_importer(path: &str) -> String {
    assets::assets_api::importer(path)
}

/// A path loaded once and shared (what `audio.play(path)` and an `AudioSource` clip path use).
pub fn cached_sound(path: &str) -> SoundRef {
    if let Some(s) = st(|s| s.cache.get(path).copied()) {
        return s;
    }
    let sound = load_sound(path, None);
    st(|s| s.cache.insert(path.to_string(), sound));
    sound
}

/// The path of a sound ("" for none).
pub fn sound_path(sound: SoundRef) -> String {
    st(|s| {
        s.sounds
            .get((sound.0 as usize).wrapping_sub(1))
            .map(|d| d.path.clone())
            .unwrap_or_default()
    })
}

pub fn sound_exists(sound: SoundRef) -> bool {
    sound.0 != 0 && st(|s| (sound.0 as usize) <= s.sounds.len())
}

/// Starts a sound.
pub fn play_sound(sound: SoundRef, o: PlayOptions) -> PlaybackRef {
    st(|s| {
        s.playbacks.push(PlaybackData {
            sound,
            playing: true,
            paused: false,
            on_ended: Vec::new(),
        });
        let playback = PlaybackRef(s.playbacks.len() as u32);
        // An emitter wins over a bus when both are given.
        let track = o.emitter.map(|e| e.0).or(o.bus.map(|b| b.0));
        s.commands.push(AudioCommand::Play {
            sound,
            playback,
            track,
            options: o,
        });
        playback
    })
}

/// Whether the playback is still playing (paused counts as playing, as in the TypeScript).
pub fn is_playing(p: PlaybackRef) -> bool {
    st(|s| {
        s.playbacks
            .get((p.0 as usize).wrapping_sub(1))
            .is_some_and(|d| d.playing)
    })
}

pub fn is_paused(p: PlaybackRef) -> bool {
    st(|s| {
        s.playbacks
            .get((p.0 as usize).wrapping_sub(1))
            .is_some_and(|d| d.paused)
    })
}

pub fn set_playback(p: PlaybackRef, property: &'static str, value: f64, fade: f64) {
    st(|s| {
        s.commands.push(AudioCommand::Set {
            playback: p,
            property,
            value,
            fade,
        })
    });
}

pub fn pause_playback(p: PlaybackRef, fade: f64) {
    st(|s| {
        if let Some(d) = s.playbacks.get_mut((p.0 as usize).wrapping_sub(1)) {
            d.paused = true;
            s.commands.push(AudioCommand::Pause(p, fade));
        }
    });
}

pub fn resume_playback(p: PlaybackRef, fade: f64) {
    st(|s| {
        if let Some(d) = s.playbacks.get_mut((p.0 as usize).wrapping_sub(1)) {
            d.paused = false;
            s.commands.push(AudioCommand::Resume(p, fade));
        }
    });
}

pub fn stop_playback(p: PlaybackRef, fade: f64) {
    st(|s| s.commands.push(AudioCommand::Stop(p, fade)));
}

pub fn create_bus(volume: f64, parent: Option<BusRef>, effects: bool) -> BusRef {
    st(|s| {
        s.next_track += 1;
        let bus = BusRef(s.next_track);
        s.commands.push(AudioCommand::Bus {
            bus,
            parent: parent.map(|p| p.0),
            volume,
            fx: if effects { 2 } else { 0 },
        });
        bus
    })
}

/// An emitter always has a low-pass (what occlusion drives); `effects` adds the rest.
pub fn create_emitter(
    position: [f64; 3],
    min_distance: f64,
    max_distance: f64,
    bus: Option<BusRef>,
    effects: bool,
) -> EmitterRef {
    st(|s| {
        s.next_track += 1;
        let emitter = EmitterRef(s.next_track);
        s.commands.push(AudioCommand::Emitter {
            emitter,
            position,
            distances: [min_distance, max_distance],
            bus: bus.map(|b| b.0),
            fx: if effects { 2 } else { 1 },
        });
        emitter
    })
}

/// Sets the volume of a bus or emitter track (`None`: the master), over `fade` seconds.
pub fn set_track_volume(track: Option<u32>, volume: f64, fade: f64) {
    st(|s| {
        s.commands.push(AudioCommand::TrackVolume {
            track,
            volume,
            fade,
        })
    });
}

/// Sets an effect parameter of a bus or emitter (see [`AudioCommand::Fx`]); track 0 is the
/// master mix.
pub fn set_fx(track: u32, property: &str, value: f64, fade: f64) {
    st(|s| {
        s.commands.push(AudioCommand::Fx {
            track,
            property: property.to_string(),
            value,
            fade,
        })
    });
}

pub fn move_emitter(e: EmitterRef, position: [f64; 3], glide: f64) {
    st(|s| {
        s.commands.push(AudioCommand::EmitterPosition {
            emitter: e,
            position,
            glide,
        })
    });
}

pub fn remove_track(track: u32) {
    st(|s| s.commands.push(AudioCommand::RemoveTrack(track)));
}

pub fn set_listener(position: [f64; 3], rotation: [f64; 4]) {
    st(|s| {
        s.listener = Some((position, rotation));
        s.commands
            .push(AudioCommand::Listener { position, rotation });
    });
}

pub fn set_master_volume(volume: f64, fade: f64) {
    st(|s| {
        s.master = volume;
        s.commands.push(AudioCommand::TrackVolume {
            track: None,
            volume,
            fade,
        });
    });
}

fn opt<T: Handle>(h: T) -> Option<T> {
    (h.to_raw() != 0).then_some(h)
}

fn arr3(v: V3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

#[ts_api(
    namespace = "audio", handles(Sound = SoundRef, Playback = PlaybackRef, Bus = BusRef, Emitter = EmitterRef),
    values(Vector3 = V3 [x: f64, y: f64, z: f64], Quaternion = Q [x: f64, y: f64, z: f64, w: f64])
)]
pub mod audio_api {
    use super::*;

    /// Loads a sound from a path inside the game folder (or a `guid:` source). `stream` decodes
    /// while playing instead of up front: for music and long ambience (a `.meta` can ask for it).
    pub fn load(path: &str, stream: bool) -> SoundRef {
        super::load_sound(path, stream.then_some(true))
    }

    /// Plays a file, loading it the first time (handy for UI clicks and one-off effects).
    #[optional(
        volume = 1,
        rate = 1,
        pan = 0,
        r#loop,
        fade_in = 0,
        start = 0,
        bus,
        emitter
    )]
    #[allow(clippy::too_many_arguments)]
    pub fn play(
        src: &str,
        volume: f64,
        rate: f64,
        pan: f64,
        r#loop: bool,
        fade_in: f64,
        start: f64,
        bus: BusRef,
        emitter: EmitterRef,
    ) -> PlaybackRef {
        super::play_sound(
            super::cached_sound(src),
            PlayOptions {
                volume,
                rate,
                pan,
                looped: r#loop,
                fade_in,
                start,
                bus: opt(bus),
                emitter: opt(emitter),
            },
        )
    }

    /// A mixer bus: its volume and pause apply to everything on it (and on its sub-buses).
    ///
    /// With `effects` the bus has a low-pass, a high-pass, a reverb and an echo that
    /// `setLowPass`, `setHighPass`, `setReverb` and `setEcho` drive (all off at first): a voice
    /// bus that a cave zone or a radio filter can colour, a bus a room's ambience plays on.
    #[optional(volume = 1, parent, effects)]
    pub fn create_bus(volume: f64, parent: BusRef, effects: bool) -> BusRef {
        super::create_bus(volume, opt(parent), effects)
    }

    /// A point in the world sounds play from: they pan and fade with its distance and direction
    /// from the listener. Full volume within `minDistance` (1), silent beyond `maxDistance` (50).
    /// It has a low-pass (`setLowPass`); `effects` adds the high-pass, reverb and echo of a bus.
    #[optional(min_distance = 1, max_distance = 50, bus, effects)]
    pub fn create_emitter(
        position: V3,
        min_distance: f64,
        max_distance: f64,
        bus: BusRef,
        effects: bool,
    ) -> EmitterRef {
        super::create_emitter(
            arr3(position),
            min_distance,
            max_distance,
            opt(bus),
            effects,
        )
    }

    /// Where the player hears from (usually the camera).
    pub fn set_listener(position: V3, rotation: Q) {
        super::set_listener(
            arr3(position),
            [rotation.x, rotation.y, rotation.z, rotation.w],
        );
    }

    /// The volume of everything, over `fade` seconds.
    #[optional(fade = 0)]
    pub fn set_master_volume(volume: f64, fade: f64) {
        super::set_master_volume(volume, fade);
    }

    /// The volume set by `setMasterVolume` (linear).
    #[property]
    pub fn master_volume() -> f64 {
        st(|s| s.master)
    }

    /// Sounds loaded so far.
    #[property]
    pub fn sound_count() -> f64 {
        st(|s| s.sounds.len() as f64)
    }

    impl SoundRef {
        /// A sound by path (what `playOneShot("assets/thud.wav")` means): the one already loaded
        /// from it, or a new one.
        pub fn from_string(path: &str) -> SoundRef {
            let src = super::source_of(path);
            let known = super::st(|s| s.sounds.iter().position(|d| d.path == src));
            match known {
                Some(i) => SoundRef(i as u32 + 1),
                None => super::load_sound(path, None),
            }
        }

        /// Starts the sound and returns the playback.
        #[optional(
            volume = 1,
            rate = 1,
            pan = 0,
            r#loop,
            fade_in = 0,
            start = 0,
            bus,
            emitter
        )]
        #[allow(clippy::too_many_arguments)]
        pub fn play(
            self,
            volume: f64,
            rate: f64,
            pan: f64,
            r#loop: bool,
            fade_in: f64,
            start: f64,
            bus: BusRef,
            emitter: EmitterRef,
        ) -> PlaybackRef {
            super::play_sound(
                self,
                PlayOptions {
                    volume,
                    rate,
                    pan,
                    looped: r#loop,
                    fade_in,
                    start,
                    bus: opt(bus),
                    emitter: opt(emitter),
                },
            )
        }

        /// The sound's path in the game folder.
        #[property]
        pub fn path(self) -> String {
            super::sound_path(self)
        }

        /// The same as `path` (the TypeScript's name).
        #[property]
        pub fn src(self) -> String {
            super::sound_path(self)
        }

        /// Frees the sound's data. Playing copies finish first.
        pub fn unload(self) {
            st(|s| s.commands.push(AudioCommand::Unload(self)));
        }
    }

    impl PlaybackRef {
        /// Changes the volume (linear gain), over `fade` seconds.
        #[optional(fade = 0)]
        pub fn set_volume(self, volume: f64, fade: f64) {
            super::set_playback(self, "volume", volume, fade);
        }

        /// Speed and pitch.
        #[optional(fade = 0)]
        pub fn set_rate(self, rate: f64, fade: f64) {
            super::set_playback(self, "rate", rate, fade);
        }

        /// -1 left, 0 centre, 1 right.
        #[optional(fade = 0)]
        pub fn set_pan(self, pan: f64, fade: f64) {
            super::set_playback(self, "pan", pan, fade);
        }

        #[optional(fade = 0)]
        pub fn pause(self, fade: f64) {
            super::pause_playback(self, fade);
        }

        #[optional(fade = 0)]
        pub fn resume(self, fade: f64) {
            super::resume_playback(self, fade);
        }

        /// Stops it (fading out over `fade` seconds); `onEnded` follows.
        #[optional(fade = 0)]
        pub fn stop(self, fade: f64) {
            super::stop_playback(self, fade);
        }

        /// Jumps to `seconds` into the sound.
        pub fn seek(self, seconds: f64) {
            st(|s| s.commands.push(AudioCommand::Seek(self, seconds)));
        }

        /// False once it has finished (played out or stopped).
        #[property]
        pub fn playing(self) -> bool {
            super::is_playing(self)
        }

        #[property]
        pub fn paused(self) -> bool {
            super::is_paused(self)
        }

        /// The sound it plays.
        #[property]
        pub fn sound(self) -> SoundRef {
            st(|s| {
                s.playbacks
                    .get((self.0 as usize).wrapping_sub(1))
                    .map_or(SoundRef(0), |d| d.sound)
            })
        }

        /// Calls `f()` when it finishes. A playback that has already finished calls it at once.
        pub fn on_ended(self, f: Callback<()>) {
            let done = st(
                |s| match s.playbacks.get_mut((self.0 as usize).wrapping_sub(1)) {
                    Some(d) if d.playing => {
                        d.on_ended.push(f.clone());
                        false
                    }
                    _ => true,
                },
            );
            if done {
                f.call(());
            }
        }
    }

    impl BusRef {
        #[optional(fade = 0)]
        pub fn set_volume(self, volume: f64, fade: f64) {
            st(|s| {
                s.commands.push(AudioCommand::TrackVolume {
                    track: Some(self.0),
                    volume,
                    fade,
                })
            });
        }

        #[optional(fade = 0)]
        pub fn pause(self, fade: f64) {
            st(|s| s.commands.push(AudioCommand::TrackPause(self.0, fade)));
        }

        #[optional(fade = 0)]
        pub fn resume(self, fade: f64) {
            st(|s| s.commands.push(AudioCommand::TrackResume(self.0, fade)));
        }

        /// Removes the bus; sounds on it stop.
        pub fn remove(self) {
            super::remove_track(self.0);
        }

        /// Low-pass cutoff in Hz (22000 is open): muffled behind a wall, under water.
        /// Needs a bus made with `effects`.
        #[optional(fade = 0)]
        pub fn set_low_pass(self, hz: f64, fade: f64) {
            super::set_fx(self.0, "lowpass", hz, fade);
        }

        /// High-pass cutoff in Hz (0 is open): thin and tinny, a telephone, a radio.
        #[optional(fade = 0)]
        pub fn set_high_pass(self, hz: f64, fade: f64) {
            super::set_fx(self.0, "highpass", hz, fade);
        }

        /// Reverb: how much of the wet signal is mixed in (0 to 1).
        #[optional(fade = 0)]
        pub fn set_reverb(self, wet: f64, fade: f64) {
            super::set_fx(self.0, "reverb", wet, fade);
        }

        /// How big the reverberating room sounds (0 to 1).
        #[optional(fade = 0)]
        pub fn set_reverb_size(self, size: f64, fade: f64) {
            super::set_fx(self.0, "reverbSize", size, fade);
        }

        /// How fast the reverb loses its highs (0 bright, 1 dull).
        #[optional(fade = 0)]
        pub fn set_reverb_damping(self, damping: f64, fade: f64) {
            super::set_fx(self.0, "reverbDamping", damping, fade);
        }

        /// Echo: how much of the repeats is mixed in (0 to 1).
        #[optional(fade = 0)]
        pub fn set_echo(self, wet: f64, fade: f64) {
            super::set_fx(self.0, "echo", wet, fade);
        }
    }

    impl EmitterRef {
        /// Moves the emitter, gliding over `glide` seconds.
        #[optional(glide = 0)]
        pub fn set_position(self, position: V3, glide: f64) {
            super::move_emitter(self, arr3(position), glide);
        }

        #[optional(fade = 0)]
        pub fn set_volume(self, volume: f64, fade: f64) {
            st(|s| {
                s.commands.push(AudioCommand::TrackVolume {
                    track: Some(self.0),
                    volume,
                    fade,
                })
            });
        }

        /// Removes the emitter; sounds on it stop.
        pub fn remove(self) {
            super::remove_track(self.0);
        }

        /// Low-pass cutoff in Hz (22000 is open): what occlusion turns down.
        #[optional(fade = 0)]
        pub fn set_low_pass(self, hz: f64, fade: f64) {
            super::set_fx(self.0, "lowpass", hz, fade);
        }

        /// High-pass cutoff in Hz (0 is open). Needs an emitter made with `effects`.
        #[optional(fade = 0)]
        pub fn set_high_pass(self, hz: f64, fade: f64) {
            super::set_fx(self.0, "highpass", hz, fade);
        }

        /// Reverb wet level (0 to 1). Needs `effects`.
        #[optional(fade = 0)]
        pub fn set_reverb(self, wet: f64, fade: f64) {
            super::set_fx(self.0, "reverb", wet, fade);
        }

        /// Echo wet level (0 to 1). Needs `effects`.
        #[optional(fade = 0)]
        pub fn set_echo(self, wet: f64, fade: f64) {
            super::set_fx(self.0, "echo", wet, fade);
        }
    }
}
