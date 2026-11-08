// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Audio for scripts: kira driven by ops (DESIGN.md §8.2).
//!
//! Like physics, scripts pick numeric IDs for sounds, playing instances, buses and
//! emitters, and queue `audio.*` ops; [`Audio::apply`] runs them. Sounds are decoded
//! files from the game folder (the app reads the bytes); `stream` sounds decode as they
//! play (music). Buses are mixer sub-tracks (`music`, `sfx`, ...) whose volume applies to
//! everything played on them. Emitters are spatial tracks positioned in the world:
//! sounds played on one pan and fade with distance from the listener.
//!
//! [`Audio::poll`] reports instances that finished, so scripts can drop them.

mod fx;
mod live;
mod offline;

pub use fx::FxLevel;
use fx::{Fx, add_fx};
pub use live::{Capture, LiveFeed};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use kira::backend::Backend;
use kira::listener::ListenerHandle;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{FromFileError, PlaybackState};
use kira::track::{
    SpatialTrackBuilder, SpatialTrackDistances, SpatialTrackHandle, TrackBuilder, TrackHandle,
};
use kira::{AudioManager, AudioManagerSettings, Decibels, Easing, Panning, PlaybackRate, Tween};
use platypus_core::{RangeRead, Value};

pub use kira::DefaultBackend;
pub use offline::OfflineBackend;

/// What a sound, instance, bus or emitter is called by scripts.
pub type ScriptId = u32;

enum Sound {
    /// Decoded up front: short effects, played many times at once.
    Static(Box<StaticSoundData>),
    /// The file, read in pieces and decoded while playing (music, ambience): from memory, or
    /// from a pack's chunks or a folder without reading it all first.
    Stream(Arc<dyn RangeRead>),
}

/// An in-memory file as a [`RangeRead`].
struct Memory(Vec<u8>);

impl RangeRead for Memory {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        let from = (offset as usize).min(self.0.len());
        let n = buf.len().min(self.0.len() - from);
        buf[..n].copy_from_slice(&self.0[from..from + n]);
        Ok(n)
    }
}

/// A [`RangeRead`] as the `Read + Seek` source a decoder wants.
struct Reader {
    src: Arc<dyn RangeRead>,
    pos: u64,
}

impl std::io::Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.src.read_at(self.pos, buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for Reader {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        let len = self.src.len();
        let target = match to {
            std::io::SeekFrom::Start(n) => Some(n),
            std::io::SeekFrom::End(d) => len.checked_add_signed(d),
            std::io::SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = target.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before the start")
        })?;
        Ok(self.pos)
    }
}

impl symphonia::core::io::MediaSource for Reader {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.src.len())
    }
}

fn stream_data(
    src: &Arc<dyn RangeRead>,
) -> Result<StreamingSoundData<FromFileError>, FromFileError> {
    StreamingSoundData::from_media_source(Reader {
        src: src.clone(),
        pos: 0,
    })
}

enum Instance {
    Static(StaticSoundHandle),
    Stream(StreamingSoundHandle<FromFileError>),
    /// A microphone (or other live feed): only its volume and stopping are controlled.
    Live(LiveFeed),
}

/// A bus (plain sub-track) or an emitter (spatial sub-track).
enum Track {
    Bus(TrackHandle),
    Emitter(SpatialTrackHandle),
}

/// Settings for one `play`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayOptions {
    /// Linear gain, 1 = as recorded.
    pub volume: f32,
    /// Speed and pitch, 1 = normal.
    pub rate: f32,
    /// -1 left, 0 centre, 1 right.
    pub pan: f32,
    pub looped: bool,
    /// Seconds to fade in over.
    pub fade_in: f32,
    /// Seconds into the sound to start at.
    pub start: f32,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            volume: 1.0,
            rate: 1.0,
            pan: 0.0,
            looped: false,
            fade_in: 0.0,
            start: 0.0,
        }
    }
}

/// Linear gain to decibels; 0 (or less) is silence.
pub fn gain_to_db(gain: f32) -> Decibels {
    if gain <= 0.0 {
        Decibels::SILENCE
    } else {
        Decibels((20.0 * gain.log10()).max(Decibels::SILENCE.0))
    }
}

fn tween(seconds: f32) -> Tween {
    Tween {
        duration: Duration::from_secs_f32(seconds.max(0.0)),
        ..Default::default()
    }
}

fn mint3(v: [f32; 3]) -> mint::Vector3<f32> {
    mint::Vector3 {
        x: v[0],
        y: v[1],
        z: v[2],
    }
}

/// The mixer scripts drive.
pub struct Audio<B: Backend = DefaultBackend> {
    manager: AudioManager<B>,
    sounds: HashMap<ScriptId, Sound>,
    instances: HashMap<ScriptId, Instance>,
    tracks: HashMap<ScriptId, Track>,
    /// The microphone, once started, and why starting it failed last.
    capture: Option<Capture>,
    mic_error: String,
    /// Effect chains of the tracks made with them (see [`FxLevel`]).
    fx: HashMap<ScriptId, Fx>,
    listener: Option<ListenerHandle>,
    /// The master volume scripts set; the main track plays it unless muted.
    master: Decibels,
    /// Silenced while the game is paused in the background (`runInBackground`).
    muted: bool,
    /// The highest filter cutoff (Hz): a filter set past the sample rate's limit blows up.
    max_cutoff: f64,
}

impl Audio<DefaultBackend> {
    /// Opens the default output device.
    pub fn new() -> Result<Self, String> {
        Self::with_settings(AudioManagerSettings::default())
    }
}

impl<B: Backend> Audio<B>
where
    B::Error: std::fmt::Debug,
{
    pub fn with_settings(mut settings: AudioManagerSettings<B>) -> Result<Self, String> {
        // The master mix has the full chain, as track 0: a zone can colour everything heard.
        let master_fx = add_fx!(settings.main_track_builder, FxLevel::Full, 19_500.0);
        let manager = AudioManager::new(settings).map_err(|e| format!("{e:?}"))?;
        Ok(Self {
            manager,
            sounds: HashMap::new(),
            instances: HashMap::new(),
            tracks: HashMap::new(),
            fx: master_fx.map(|f| (0, f)).into_iter().collect(),
            capture: None,
            mic_error: String::new(),
            listener: None,
            master: Decibels::IDENTITY,
            muted: false,
            max_cutoff: 19_500.0,
        })
    }

    /// Tells the mixer the output's sample rate, so filters stay below its limit (a device
    /// is assumed to run at 44.1 kHz or more).
    pub fn set_sample_rate(&mut self, rate: u32) {
        self.max_cutoff = (f64::from(rate) * 0.45).min(19_500.0);
        for fx in self.fx.values_mut() {
            fx.set_max_cutoff(self.max_cutoff);
        }
    }

    pub fn backend_mut(&mut self) -> &mut B {
        self.manager.backend_mut()
    }

    /// Registers a sound from an audio file's bytes (WAV, OGG, MP3, FLAC). `stream`
    /// sounds are decoded while they play instead of up front.
    pub fn load(&mut self, id: ScriptId, bytes: Vec<u8>, stream: bool) -> Result<(), String> {
        let sound = if stream {
            return self.load_stream(id, Arc::new(Memory(bytes)));
        } else {
            Sound::Static(Box::new(
                StaticSoundData::from_cursor(Cursor::new(bytes)).map_err(|e| e.to_string())?,
            ))
        };
        self.sounds.insert(id, sound);
        Ok(())
    }

    /// Registers a streamed sound read in pieces from `src` (a pack entry, a file).
    pub fn load_stream(&mut self, id: ScriptId, src: Arc<dyn RangeRead>) -> Result<(), String> {
        // Check it decodes now, so a bad file fails at load like a static one.
        stream_data(&src).map_err(|e| e.to_string())?;
        self.sounds.insert(id, Sound::Stream(src));
        Ok(())
    }

    /// Registers a sound from raw stereo frames (generated audio, tests).
    pub fn load_frames(&mut self, id: ScriptId, sample_rate: u32, frames: Vec<[f32; 2]>) {
        let frames: Arc<[kira::Frame]> = frames
            .into_iter()
            .map(|[left, right]| kira::Frame { left, right })
            .collect();
        self.sounds.insert(
            id,
            Sound::Static(Box::new(StaticSoundData {
                sample_rate,
                frames,
                settings: Default::default(),
                slice: None,
            })),
        );
    }

    /// Plays sound `sound` as instance `id`, on `track` (a bus or emitter) or the master.
    pub fn play(
        &mut self,
        id: ScriptId,
        sound: ScriptId,
        track: Option<ScriptId>,
        options: PlayOptions,
    ) -> Result<(), String> {
        let sound = self
            .sounds
            .get(&sound)
            .ok_or_else(|| format!("no sound {sound}"))?;
        let fade = (options.fade_in > 0.0).then(|| tween(options.fade_in));
        let track = match track {
            Some(t) => Some(
                self.tracks
                    .get_mut(&t)
                    .ok_or_else(|| format!("no bus or emitter {t}"))?,
            ),
            None => None,
        };
        let instance = match sound {
            Sound::Static(data) => {
                let mut data = data
                    .volume(gain_to_db(options.volume))
                    .playback_rate(PlaybackRate(options.rate.max(0.0) as f64))
                    .panning(Panning(options.pan.clamp(-1.0, 1.0)))
                    .fade_in_tween(fade)
                    .start_position(options.start.max(0.0) as f64);
                if options.looped {
                    data = data.loop_region(..);
                }
                let handle = match track {
                    Some(Track::Bus(t)) => t.play(data),
                    Some(Track::Emitter(t)) => t.play(data),
                    None => self.manager.play(data),
                }
                .map_err(|e| e.to_string())?;
                Instance::Static(handle)
            }
            Sound::Stream(src) => {
                let mut data = stream_data(src)
                    .map_err(|e| e.to_string())?
                    .volume(gain_to_db(options.volume))
                    .playback_rate(PlaybackRate(options.rate.max(0.0) as f64))
                    .panning(Panning(options.pan.clamp(-1.0, 1.0)))
                    .fade_in_tween(fade)
                    .start_position(options.start.max(0.0) as f64);
                if options.looped {
                    data = data.loop_region(..);
                }
                let handle = match track {
                    Some(Track::Bus(t)) => t.play(data),
                    Some(Track::Emitter(t)) => t.play(data),
                    None => self.manager.play(data),
                }
                .map_err(|e| e.to_string())?;
                Instance::Stream(handle)
            }
        };
        if let Some(old) = self.instances.insert(id, instance) {
            stop(old, Tween::default());
        }
        Ok(())
    }

    /// Plays a live feed (a microphone, or a test's samples) as instance `id`, on `track` (a bus
    /// or emitter) or the master.
    pub fn play_live(
        &mut self,
        id: ScriptId,
        feed: LiveFeed,
        track: Option<ScriptId>,
        volume: f32,
    ) -> Result<(), String> {
        feed.set_gain(volume);
        let data = live::LiveData(feed.clone());
        let played = match track {
            Some(t) => match self.tracks.get_mut(&t) {
                Some(Track::Bus(h)) => h.play(data),
                Some(Track::Emitter(h)) => h.play(data),
                None => return Err(format!("no bus or emitter {t}")),
            },
            None => self.manager.play(data),
        };
        played.map_err(|e| format!("{e:?}"))?;
        if let Some(old) = self.instances.insert(id, Instance::Live(feed)) {
            stop(old, Tween::default());
        }
        Ok(())
    }

    /// The microphone's level (RMS, 0 to 1), whether it is running, and why it last failed to
    /// start (empty if it did not).
    pub fn mic_status(&self) -> (f32, bool, String) {
        match &self.capture {
            Some(c) if !c.feed().stopped() => (c.feed().level(), true, String::new()),
            _ => (0.0, false, self.mic_error.clone()),
        }
    }

    /// Runs one `audio.*` op (the name without the prefix). `load` needs the file's
    /// bytes, so the app handles it and calls [`Self::load`].
    pub fn apply(&mut self, name: &str, args: &[Value]) -> Result<(), String> {
        let a = Args(args);
        match name {
            // `play id sound track|null [volume rate pan loop fadeIn start]`
            "play" => {
                let o = a.list(3).unwrap_or(&[]);
                let n = |i: usize, d: f32| o.get(i).and_then(Value::as_f64).map_or(d, |v| v as f32);
                let options = PlayOptions {
                    volume: n(0, 1.0),
                    rate: n(1, 1.0),
                    pan: n(2, 0.0),
                    looped: matches!(o.get(3), Some(Value::Bool(true))),
                    fade_in: n(4, 0.0),
                    start: n(5, 0.0),
                };
                let track = a.opt_id(2);
                self.play(a.id(0)?, a.id(1)?, track, options)?;
            }
            // `set id property value tweenSeconds`: volume, rate, pan.
            "set" => {
                let id = a.id(0)?;
                let t = tween(a.opt_num(3).unwrap_or(0.0));
                let v = a.num(2)?;
                let instance = self
                    .instances
                    .get_mut(&id)
                    .ok_or_else(|| format!("no instance {id}"))?;
                match (a.str(1)?, instance) {
                    ("volume", Instance::Static(h)) => h.set_volume(gain_to_db(v), t),
                    ("volume", Instance::Stream(h)) => h.set_volume(gain_to_db(v), t),
                    ("rate", Instance::Static(h)) => {
                        h.set_playback_rate(PlaybackRate(v.max(0.0) as f64), t)
                    }
                    ("rate", Instance::Stream(h)) => {
                        h.set_playback_rate(PlaybackRate(v.max(0.0) as f64), t)
                    }
                    ("pan", Instance::Static(h)) => h.set_panning(Panning(v.clamp(-1.0, 1.0)), t),
                    ("pan", Instance::Stream(h)) => h.set_panning(Panning(v.clamp(-1.0, 1.0)), t),
                    ("volume", Instance::Live(feed)) => feed.set_gain(v),
                    // A live input has no speed or pan of its own: bus and emitter place it.
                    (_, Instance::Live(_)) => {}
                    (other, _) => return Err(format!("unknown sound property `{other}`")),
                }
            }
            // `pause|resume|stop id fadeSeconds`
            "pause" | "resume" | "stop" => {
                let id = a.id(0)?;
                let t = tween(a.opt_num(1).unwrap_or(0.0));
                let Some(instance) = self.instances.get_mut(&id) else {
                    // Already finished (and reported): nothing to do.
                    return Ok(());
                };
                match (name, instance) {
                    ("pause", Instance::Static(h)) => h.pause(t),
                    ("pause", Instance::Stream(h)) => h.pause(t),
                    ("resume", Instance::Static(h)) => h.resume(t),
                    ("resume", Instance::Stream(h)) => h.resume(t),
                    // Kept until it has faded out; `poll` reports it then.
                    (_, Instance::Static(h)) => h.stop(t),
                    (_, Instance::Stream(h)) => h.stop(t),
                    ("stop", Instance::Live(feed)) => feed.stop(),
                    (_, Instance::Live(_)) => {}
                }
            }
            // `micStart id track|null [volume]`: the default microphone, played on a bus or
            // emitter (the master if none) as instance `id`. It reads the input device, which
            // can fail (no microphone, permission), so the error comes back.
            "micStart" => {
                let id = a.id(0)?;
                let volume = a.opt_num(2).unwrap_or(1.0);
                let feed = match &self.capture {
                    Some(c) if !c.feed().stopped() => c.feed(),
                    _ => {
                        let c = Capture::start().inspect_err(|e| self.mic_error = e.clone())?;
                        self.mic_error.clear();
                        let feed = c.feed();
                        self.capture = Some(c);
                        feed
                    }
                };
                self.play_live(id, feed, a.opt_id(1), volume)?;
            }
            "seek" => {
                let id = a.id(0)?;
                let to = a.num(1)?.max(0.0) as f64;
                match self.instances.get_mut(&id) {
                    Some(Instance::Static(h)) => h.seek_to(to),
                    Some(Instance::Stream(h)) => h.seek_to(to),
                    Some(Instance::Live(_)) | None => {}
                }
            }
            "unload" => {
                self.sounds.remove(&a.id(0)?);
            }
            // `bus id parent|null volume [fx]`: `fx` 1 adds a low-pass, 2 the full chain.
            "bus" => {
                let id = a.id(0)?;
                let mut builder =
                    TrackBuilder::new().volume(gain_to_db(a.opt_num(2).unwrap_or(1.0)));
                let level = FxLevel::from_number(a.opt_num(3).map_or(0.0, f64::from));
                let fx = add_fx!(builder, level, self.max_cutoff);
                let handle = match a.opt_id(1) {
                    Some(p) => match self.tracks.get_mut(&p) {
                        Some(Track::Bus(parent)) => parent.add_sub_track(builder),
                        Some(Track::Emitter(parent)) => parent.add_sub_track(builder),
                        None => return Err(format!("no bus {p}")),
                    },
                    None => self.manager.add_sub_track(builder),
                }
                .map_err(|e| e.to_string())?;
                self.tracks.insert(id, Track::Bus(handle));
                self.fx.remove(&id);
                if let Some(fx) = fx {
                    self.fx.insert(id, fx);
                }
            }
            // `fx id property value tweenSeconds`: `lowpass` / `highpass` (Hz), `reverb` /
            // `echo` (wet 0..1), `reverbSize`, `reverbDamping`, on a track made with effects.
            "fx" => {
                let id = a.id(0)?;
                let t = tween(a.opt_num(3).unwrap_or(0.0));
                let value = f64::from(a.num(2)?);
                let property = a.str(1)?;
                match self.fx.get_mut(&id) {
                    Some(fx) => fx.set(property, value, t)?,
                    None if self.tracks.contains_key(&id) => {
                        return Err(format!("track {id} was made without effects"));
                    }
                    None => return Err(format!("no bus or emitter {id}")),
                }
            }
            // `emitter id [x y z] [minDistance maxDistance] parent|null [fx]`
            "emitter" => {
                let id = a.id(0)?;
                let position = mint3(a.floats::<3>(1)?);
                let d = a.floats::<2>(2).unwrap_or([1.0, 50.0]);
                let mut builder = SpatialTrackBuilder::new()
                    .distances(SpatialTrackDistances {
                        min_distance: d[0].max(0.0),
                        max_distance: d[1].max(d[0] + 0.01),
                    })
                    // Inverse-square-like falloff between the distances.
                    .attenuation_function(Easing::OutPowi(2));
                let level = FxLevel::from_number(a.opt_num(4).map_or(0.0, f64::from));
                let fx = add_fx!(builder, level, self.max_cutoff);
                let listener = self.listener()?;
                let handle = match a.opt_id(3) {
                    Some(p) => match self.tracks.get_mut(&p) {
                        Some(Track::Bus(parent)) => {
                            parent.add_spatial_sub_track(listener, position, builder)
                        }
                        Some(Track::Emitter(parent)) => {
                            parent.add_spatial_sub_track(listener, position, builder)
                        }
                        None => return Err(format!("no bus {p}")),
                    },
                    None => self
                        .manager
                        .add_spatial_sub_track(listener, position, builder),
                }
                .map_err(|e| e.to_string())?;
                self.tracks.insert(id, Track::Emitter(handle));
                self.fx.remove(&id);
                if let Some(fx) = fx {
                    self.fx.insert(id, fx);
                }
            }
            // `trackVolume id|null volume tweenSeconds` (null: the master)
            "trackVolume" => {
                let db = gain_to_db(a.num(1)?);
                let t = tween(a.opt_num(2).unwrap_or(0.0));
                match a.opt_id(0) {
                    None => {
                        self.master = db;
                        if !self.muted {
                            self.manager.main_track().set_volume(db, t);
                        }
                    }
                    Some(id) => match self.tracks.get_mut(&id) {
                        Some(Track::Bus(h)) => h.set_volume(db, t),
                        Some(Track::Emitter(h)) => h.set_volume(db, t),
                        None => return Err(format!("no bus or emitter {id}")),
                    },
                }
            }
            // `trackPause|trackResume id fadeSeconds`
            "trackPause" | "trackResume" => {
                let id = a.id(0)?;
                let t = tween(a.opt_num(1).unwrap_or(0.0));
                match (name, self.tracks.get_mut(&id)) {
                    ("trackPause", Some(Track::Bus(h))) => h.pause(t),
                    ("trackPause", Some(Track::Emitter(h))) => h.pause(t),
                    (_, Some(Track::Bus(h))) => h.resume(t),
                    (_, Some(Track::Emitter(h))) => h.resume(t),
                    (_, None) => return Err(format!("no bus or emitter {id}")),
                }
            }
            // `emitterPosition id [x y z] tweenSeconds`
            "emitterPosition" => {
                let id = a.id(0)?;
                let t = tween(a.opt_num(2).unwrap_or(0.0));
                match self.tracks.get_mut(&id) {
                    Some(Track::Emitter(h)) => h.set_position(mint3(a.floats::<3>(1)?), t),
                    _ => return Err(format!("no emitter {id}")),
                }
            }
            // `removeTrack id`: its sounds stop with it.
            "removeTrack" => {
                let id = a.id(0)?;
                self.tracks.remove(&id);
                self.fx.remove(&id);
            }
            // `listener [x y z] [qx qy qz qw]`: where the player's ears are.
            "listener" => {
                let p = mint3(a.floats::<3>(0)?);
                let [x, y, z, w] = a.floats::<4>(1).unwrap_or([0.0, 0.0, 0.0, 1.0]);
                let listener = self.listener_mut()?;
                listener.set_position(p, Tween::default());
                listener.set_orientation(
                    mint::Quaternion {
                        v: mint::Vector3 { x, y, z },
                        s: w,
                    },
                    Tween::default(),
                );
            }
            other => return Err(format!("unknown audio op `{other}`")),
        }
        Ok(())
    }

    fn listener_mut(&mut self) -> Result<&mut ListenerHandle, String> {
        if self.listener.is_none() {
            let origin = mint3([0.0; 3]);
            let facing = mint::Quaternion {
                v: mint::Vector3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                s: 1.0,
            };
            let handle = self
                .manager
                .add_listener(origin, facing)
                .map_err(|e| e.to_string())?;
            self.listener = Some(handle);
        }
        Ok(self.listener.as_mut().expect("just set"))
    }

    fn listener(&mut self) -> Result<kira::listener::ListenerId, String> {
        Ok(self.listener_mut()?.id())
    }

    /// Instances that finished since the last call (played out or stopped), which are
    /// forgotten here; scripts get them as `ended` events.
    pub fn poll(&mut self) -> Vec<ScriptId> {
        let mut ended: Vec<ScriptId> = self
            .instances
            .iter()
            .filter(|(_, i)| match i {
                Instance::Static(h) => h.state() == PlaybackState::Stopped,
                Instance::Stream(h) => h.state() == PlaybackState::Stopped,
                Instance::Live(feed) => feed.stopped(),
            })
            .map(|(&id, _)| id)
            .collect();
        ended.sort_unstable();
        for id in &ended {
            self.instances.remove(id);
        }
        ended
    }

    pub fn playing(&self) -> usize {
        self.instances.len()
    }

    /// Fades everything out (or back to the scripts' master volume) without touching
    /// what scripts set: for a game paused in the background.
    pub fn set_muted(&mut self, muted: bool) {
        if muted == self.muted {
            return;
        }
        self.muted = muted;
        let db = if muted {
            Decibels::SILENCE
        } else {
            self.master
        };
        self.manager.main_track().set_volume(db, tween(0.1));
    }
}

fn stop(instance: Instance, t: Tween) {
    match instance {
        Instance::Static(mut h) => h.stop(t),
        Instance::Stream(mut h) => h.stop(t),
        Instance::Live(feed) => feed.stop(),
    }
}

/// Reads op arguments with errors that say what was wrong.
struct Args<'a>(&'a [Value]);

impl Args<'_> {
    fn num(&self, i: usize) -> Result<f32, String> {
        self.0
            .get(i)
            .and_then(Value::as_f64)
            .map(|v| v as f32)
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("argument {i} must be a number"))
    }

    fn opt_num(&self, i: usize) -> Option<f32> {
        self.num(i).ok()
    }

    /// Ids go through `f64`: `f32` can't hold integers past 2^24 (script component
    /// and engine object ids are bigger).
    fn id(&self, i: usize) -> Result<ScriptId, String> {
        self.0
            .get(i)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0 && *v <= u32::MAX as f64)
            .map(|v| v as ScriptId)
            .ok_or_else(|| format!("argument {i} must be an id"))
    }

    fn opt_id(&self, i: usize) -> Option<ScriptId> {
        self.id(i).ok()
    }

    fn str(&self, i: usize) -> Result<&str, String> {
        self.0
            .get(i)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("argument {i} must be a string"))
    }

    fn list(&self, i: usize) -> Result<&[Value], String> {
        match self.0.get(i) {
            Some(Value::Array(items)) => Ok(items),
            _ => Err(format!("argument {i} must be an array")),
        }
    }

    fn floats<const N: usize>(&self, i: usize) -> Result<[f32; N], String> {
        let items = self.list(i)?;
        let mut out = [0.0; N];
        for (k, slot) in out.iter_mut().enumerate() {
            *slot = items
                .get(k)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("argument {i} must hold {N} numbers"))?;
        }
        Ok(out)
    }
}

/// An [`Audio`] on any backend, so the app can pick a device or offline mixing at run
/// time.
pub trait Mixer {
    fn load(&mut self, id: ScriptId, bytes: Vec<u8>, stream: bool) -> Result<(), String>;
    /// A streamed sound read in pieces (see [`Audio::load_stream`]).
    fn load_stream(&mut self, id: ScriptId, src: Arc<dyn RangeRead>) -> Result<(), String>;
    fn apply(&mut self, name: &str, args: &[Value]) -> Result<(), String>;
    fn poll(&mut self) -> Vec<ScriptId>;
    /// Lets `seconds` of audio play: a device plays on its own; an offline mixer renders
    /// (and drops) that much, so sounds still progress and end.
    fn advance(&mut self, seconds: f64);
    /// See [`Audio::set_muted`].
    fn set_muted(&mut self, muted: bool);
    /// The microphone's level, whether it runs, and its last error (see [`Audio::mic_status`]).
    fn mic_status(&self) -> (f32, bool, String);
}

impl Mixer for Audio<DefaultBackend> {
    fn load(&mut self, id: ScriptId, bytes: Vec<u8>, stream: bool) -> Result<(), String> {
        Audio::load(self, id, bytes, stream)
    }
    fn load_stream(&mut self, id: ScriptId, src: Arc<dyn RangeRead>) -> Result<(), String> {
        Audio::load_stream(self, id, src)
    }
    fn apply(&mut self, name: &str, args: &[Value]) -> Result<(), String> {
        Audio::apply(self, name, args)
    }
    fn poll(&mut self) -> Vec<ScriptId> {
        Audio::poll(self)
    }
    fn advance(&mut self, _seconds: f64) {}
    fn set_muted(&mut self, muted: bool) {
        Audio::set_muted(self, muted)
    }
    fn mic_status(&self) -> (f32, bool, String) {
        Audio::mic_status(self)
    }
}

impl Mixer for Audio<OfflineBackend> {
    fn load(&mut self, id: ScriptId, bytes: Vec<u8>, stream: bool) -> Result<(), String> {
        Audio::load(self, id, bytes, stream)
    }
    fn load_stream(&mut self, id: ScriptId, src: Arc<dyn RangeRead>) -> Result<(), String> {
        Audio::load_stream(self, id, src)
    }
    fn apply(&mut self, name: &str, args: &[Value]) -> Result<(), String> {
        Audio::apply(self, name, args)
    }
    fn poll(&mut self) -> Vec<ScriptId> {
        Audio::poll(self)
    }
    fn advance(&mut self, seconds: f64) {
        let backend = self.backend_mut();
        let frames = (seconds.max(0.0) * backend.sample_rate() as f64).round() as usize;
        backend.render(frames);
    }
    fn set_muted(&mut self, muted: bool) {
        Audio::set_muted(self, muted)
    }
    fn mic_status(&self) -> (f32, bool, String) {
        Audio::mic_status(self)
    }
}

/// Opens the default output device, or (with `offline`, or when there's no device)
/// mixes offline: sounds play silently but still progress and end.
pub fn open_mixer(offline: bool) -> Box<dyn Mixer> {
    if !offline {
        match Audio::new() {
            Ok(audio) => return Box::new(audio),
            Err(e) => tracing::warn!("no audio output ({e}); mixing silently"),
        }
    }
    let settings = AudioManagerSettings {
        backend_settings: 48_000,
        ..Default::default()
    };
    let mut audio =
        Audio::<OfflineBackend>::with_settings(settings).expect("offline mixing can't fail");
    audio.set_sample_rate(48_000);
    Box::new(audio)
}
