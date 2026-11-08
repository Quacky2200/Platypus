// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Effect chains on buses and emitters: what makes sound muffle behind a wall, echo in a cave
//! or go tinny on a radio. A track made with effects owns a low-pass filter, and (the full
//! chain) a high-pass filter, a reverb and an echo; scripts set them with `audio.fx` ops and
//! the tracks glide to the new values. The low-pass is the one occlusion drives, so every
//! emitter of an `AudioSource` has it (a biquad is cheap); the rest cost more and are asked for.

use std::time::Duration;

use kira::effect::delay::{DelayBuilder, DelayHandle};
use kira::effect::filter::{FilterBuilder, FilterHandle, FilterMode};
use kira::effect::reverb::ReverbHandle;
use kira::{Decibels, Mix, Tween};

/// How many effects a track gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FxLevel {
    #[default]
    None,
    /// The low-pass only.
    Low,
    /// Low-pass, high-pass, reverb and echo.
    Full,
}

impl FxLevel {
    /// Script numbers: 0 none, 1 low-pass only, 2 everything (`true` is 2).
    pub fn from_number(n: f64) -> FxLevel {
        match n {
            n if n >= 2.0 => FxLevel::Full,
            n if n >= 1.0 => FxLevel::Low,
            _ => FxLevel::None,
        }
    }
}

/// The handles of a track's effects.
pub(crate) struct Fx {
    low: FilterHandle,
    full: Option<Full>,
    /// Where "open" is: the filter is bypassed at or above it (and never set past the sample
    /// rate's limit, where it would blow up).
    max_cutoff: f64,
}

pub(crate) struct Full {
    pub(crate) high: FilterHandle,
    pub(crate) reverb: ReverbHandle,
    pub(crate) echo: DelayHandle,
}

/// The echo's repeat time in seconds: a cave's slap.
pub const ECHO_TIME: f32 = 0.27;
/// Each echo repeat's level against the one before.
const ECHO_FEEDBACK_DB: f32 = -9.0;

pub(crate) fn bypassed_filter(mode: FilterMode, cutoff: f64) -> FilterBuilder {
    FilterBuilder::new().mode(mode).cutoff(cutoff).mix(Mix::DRY)
}

/// Adds the chain for `level` to a track builder (a bus's or an emitter's: their `add_effect`
/// is generic per builder, so this is a macro rather than a function).
macro_rules! add_fx {
    ($builder:expr, $level:expr, $max:expr) => {{
        let (level, max_cutoff): (crate::fx::FxLevel, f64) = ($level, $max);
        if level == crate::fx::FxLevel::None {
            None
        } else {
            let low = $builder.add_effect(crate::fx::bypassed_filter(
                kira::effect::filter::FilterMode::LowPass,
                max_cutoff,
            ));
            let full = (level == crate::fx::FxLevel::Full).then(|| {
                let high = $builder.add_effect(crate::fx::bypassed_filter(
                    kira::effect::filter::FilterMode::HighPass,
                    20.0,
                ));
                let reverb = $builder
                    .add_effect(kira::effect::reverb::ReverbBuilder::new().mix(kira::Mix::DRY));
                let echo = $builder.add_effect(crate::fx::echo_builder());
                crate::fx::Full { high, reverb, echo }
            });
            Some(crate::fx::Fx::new(low, full, max_cutoff))
        }
    }};
}
pub(crate) use add_fx;

pub(crate) fn echo_builder() -> DelayBuilder {
    DelayBuilder::new()
        .delay_time(Duration::from_secs_f32(ECHO_TIME))
        .feedback(Decibels(ECHO_FEEDBACK_DB))
        .mix(Mix::DRY)
}

impl Fx {
    pub(crate) fn new(low: FilterHandle, full: Option<Full>, max_cutoff: f64) -> Fx {
        Fx {
            low,
            full,
            max_cutoff,
        }
    }

    /// Sets one effect parameter: `lowpass` / `highpass` (Hz), `reverb` / `echo` (wet, 0 to 1),
    /// `reverbSize` and `reverbDamping` (0 to 1). The ones the chain lacks are ignored, so a
    /// zone can drive a mixed set of tracks.
    pub(crate) fn set(&mut self, property: &str, v: f64, t: Tween) -> Result<(), String> {
        match property {
            "lowpass" => {
                let cutoff = v.clamp(20.0, self.max_cutoff);
                self.low.set_cutoff(cutoff, t);
                self.low.set_mix(
                    if v >= self.max_cutoff {
                        Mix::DRY
                    } else {
                        Mix::WET
                    },
                    t,
                );
            }
            "highpass" => {
                if let Some(f) = &mut self.full {
                    f.high.set_cutoff(v.clamp(20.0, self.max_cutoff), t);
                    f.high
                        .set_mix(if v <= 20.0 { Mix::DRY } else { Mix::WET }, t);
                }
            }
            "reverb" => {
                if let Some(f) = &mut self.full {
                    f.reverb.set_mix(Mix(v.clamp(0.0, 1.0) as f32), t);
                }
            }
            "reverbSize" => {
                if let Some(f) = &mut self.full {
                    f.reverb.set_feedback(v.clamp(0.0, 0.98), t);
                }
            }
            "reverbDamping" => {
                if let Some(f) = &mut self.full {
                    f.reverb.set_damping(v.clamp(0.0, 1.0), t);
                }
            }
            "echo" => {
                if let Some(f) = &mut self.full {
                    f.echo.set_mix(Mix(v.clamp(0.0, 1.0) as f32), t);
                }
            }
            other => return Err(format!("unknown effect property `{other}`")),
        }
        Ok(())
    }
}
