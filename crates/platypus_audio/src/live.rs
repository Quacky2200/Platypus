// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Live input: a microphone (or any feed of samples) played through the mixer like a sound, so
//! it can be put on a bus and have zones, filters, echo and reverb applied to it. No VoIP system
//! is involved: this is the local voice. [`LiveFeed`] is the buffer a producer pushes samples
//! into (the capture thread, or a test); [`Capture`] opens the default input device with cpal
//! and feeds one.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use kira::Frame;
use kira::info::Info;
use kira::sound::{Sound, SoundData};

/// How much input is kept (seconds): a reader that falls behind skips ahead.
const KEEP: f64 = 1.0;
/// Where a reader starts behind the newest sample, and returns to when it falls too far behind
/// (seconds): small enough to talk to, big enough not to underrun.
const LATENCY: f64 = 0.06;
/// Falling this far behind skips ahead.
const MAX_LAG: f64 = 0.3;

struct Ring {
    samples: VecDeque<f32>,
    /// The absolute index of `samples[0]`.
    base: u64,
    rate: f64,
}

/// The samples a live sound plays, and its controls.
pub struct LiveShared {
    ring: Mutex<Ring>,
    stopped: AtomicBool,
    /// Linear gain, as `f32` bits.
    gain: AtomicU32,
    /// RMS of the latest pushed block, as `f32` bits.
    level: AtomicU32,
}

/// A handle to feed (and stop) a live sound.
#[derive(Clone)]
pub struct LiveFeed(pub(crate) Arc<LiveShared>);

impl LiveFeed {
    /// A feed of mono samples at `rate` Hz.
    pub fn new(rate: u32) -> LiveFeed {
        LiveFeed(Arc::new(LiveShared {
            ring: Mutex::new(Ring {
                samples: VecDeque::new(),
                base: 0,
                rate: f64::from(rate.max(1)),
            }),
            stopped: AtomicBool::new(false),
            gain: AtomicU32::new(1.0f32.to_bits()),
            level: AtomicU32::new(0),
        }))
    }

    /// Adds samples (mono, the feed's rate).
    pub fn push(&self, block: &[f32]) {
        if block.is_empty() {
            return;
        }
        let rms = (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt();
        self.0.level.store(rms.to_bits(), Ordering::Relaxed);
        let Ok(mut ring) = self.0.ring.lock() else {
            return;
        };
        ring.samples.extend(block.iter().copied());
        let keep = (ring.rate * KEEP) as usize;
        while ring.samples.len() > keep {
            ring.samples.pop_front();
            ring.base += 1;
        }
    }

    /// Ends the sound that plays this feed.
    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.0.stopped.load(Ordering::Relaxed)
    }

    /// RMS (0 to 1) of the latest block.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.0.level.load(Ordering::Relaxed))
    }

    pub fn set_gain(&self, gain: f32) {
        self.0
            .gain
            .store(gain.max(0.0).to_bits(), Ordering::Relaxed);
    }
}

/// The kira sound that plays a feed.
pub(crate) struct LiveData(pub(crate) LiveFeed);

impl SoundData for LiveData {
    type Error = std::convert::Infallible;
    type Handle = ();

    fn into_sound(self) -> Result<(Box<dyn Sound>, ()), Self::Error> {
        Ok((
            Box::new(LiveSound {
                feed: self.0,
                pos: None,
            }),
            (),
        ))
    }
}

struct LiveSound {
    feed: LiveFeed,
    /// The absolute (fractional) index of the next sample to play.
    pos: Option<f64>,
}

impl Sound for LiveSound {
    fn process(&mut self, out: &mut [Frame], dt: f64, _info: &Info) {
        let gain = f32::from_bits(self.feed.0.gain.load(Ordering::Relaxed));
        // The audio thread never waits for the capture thread: no lock, no sound this block.
        let Ok(ring) = self.feed.0.ring.try_lock() else {
            out.fill(Frame::ZERO);
            return;
        };
        let end = ring.base + ring.samples.len() as u64;
        let step = ring.rate * dt;
        let mut pos = self.pos.unwrap_or(end as f64 - LATENCY * ring.rate);
        if end as f64 - pos > MAX_LAG * ring.rate || pos < ring.base as f64 {
            pos = (end as f64 - LATENCY * ring.rate).max(ring.base as f64);
        }
        for frame in out.iter_mut() {
            let i = pos.floor();
            let at = |k: u64| {
                ring.samples
                    .get((k.saturating_sub(ring.base)) as usize)
                    .copied()
            };
            // Underrun: nothing new to play yet; wait.
            let (Some(a), Some(b)) = (at(i as u64), at(i as u64 + 1)) else {
                *frame = Frame::ZERO;
                continue;
            };
            let f = (pos - i) as f32;
            let s = (a + (b - a) * f) * gain;
            *frame = Frame { left: s, right: s };
            pos += step;
        }
        self.pos = Some(pos);
    }

    fn finished(&self) -> bool {
        self.feed.stopped()
    }
}

/// The default input device, feeding a [`LiveFeed`] from its own thread (a cpal stream is not
/// `Send` everywhere, so the thread owns it).
pub struct Capture {
    feed: LiveFeed,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Opens the default microphone and starts capturing; an error says why not (no device,
    /// permission refused, busy).
    pub fn start() -> Result<Capture, String> {
        let (tx, rx) = std::sync::mpsc::channel::<Result<LiveFeed, String>>();
        let quit = Arc::new(AtomicBool::new(false));
        let quit_thread = quit.clone();
        let thread = std::thread::Builder::new()
            .name("platypus-microphone".into())
            .spawn(move || capture_thread(tx, quit_thread))
            .map_err(|e| e.to_string())?;
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(feed)) => Ok(Capture {
                feed,
                quit,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the microphone did not start in time".into()),
        }
    }

    pub fn feed(&self) -> LiveFeed {
        self.feed.clone()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.feed.stop();
        self.quit.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn capture_thread(tx: std::sync::mpsc::Sender<Result<LiveFeed, String>>, quit: Arc<AtomicBool>) {
    let setup = || -> Result<(cpal::Stream, LiveFeed), String> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("no microphone found")?;
        let supported = device
            .default_input_config()
            .map_err(|e| format!("microphone: {e}"))?;
        let channels = usize::from(supported.channels()).max(1);
        let format = supported.sample_format();
        let config = supported.config();
        let feed = LiveFeed::new(supported.sample_rate());
        let sink = feed.clone();
        let err = |e| tracing::warn!("microphone: {e}");
        // Down-mix to mono as the samples arrive.
        fn mono<T: Copy>(data: &[T], channels: usize, convert: impl Fn(T) -> f32) -> Vec<f32> {
            data.chunks(channels)
                .map(|f| f.iter().map(|s| convert(*s)).sum::<f32>() / f.len() as f32)
                .collect()
        }
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_input_stream::<f32, _, _>(
                config,
                move |d, _| sink.push(&mono(d, channels, |s| s)),
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
                config,
                move |d, _| sink.push(&mono(d, channels, |s| f32::from(s) / 32768.0)),
                err,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream::<u16, _, _>(
                config,
                move |d, _| sink.push(&mono(d, channels, |s| (f32::from(s) - 32768.0) / 32768.0)),
                err,
                None,
            ),
            other => {
                return Err(format!(
                    "microphone sample format {other:?} is not supported"
                ));
            }
        }
        .map_err(|e| format!("microphone: {e}"))?;
        stream.play().map_err(|e| format!("microphone: {e}"))?;
        Ok((stream, feed))
    };
    match setup() {
        Ok((stream, feed)) => {
            let _ = tx.send(Ok(feed));
            while !quit.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(50));
            }
            drop(stream);
        }
        Err(e) => {
            let _ = tx.send(Err(e));
        }
    }
}
