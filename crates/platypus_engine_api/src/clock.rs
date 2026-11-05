// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `TimeSince` and `TimeUntil` (s&box's), measured on the game clock of [`crate::time`]:
//! `lastShot = TimeSince.now(); ... if (lastShot.seconds > 0.5) fire();` and
//! `cooldown = TimeUntil.in(2); ... if (cooldown.done) ...`.

use crate::time;
use ts_api::{ValueType, ts_api};

/// Seconds since something happened: remembers when.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Since {
    pub at: f64,
}

impl ValueType<1> for Since {
    fn from_parts(p: [f64; 1]) -> Self {
        Since { at: p[0] }
    }

    fn to_parts(self) -> [f64; 1] {
        [self.at]
    }
}

/// Seconds until something happens: remembers when and how long it was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Until {
    pub at: f64,
    pub length: f64,
}

impl ValueType<2> for Until {
    fn from_parts(p: [f64; 2]) -> Self {
        Until {
            at: p[0],
            length: p[1],
        }
    }

    fn to_parts(self) -> [f64; 2] {
        [self.at, self.length]
    }
}

fn game_time() -> f64 {
    time::state().time
}

#[ts_api(namespace = "TimeSince", values(TimeSince = Since [at: f64]))]
pub mod time_since_api {
    use super::{Since, game_time};

    /// Starting now.
    pub fn now() -> Since {
        Since { at: game_time() }
    }

    /// As if it happened `seconds` ago.
    pub fn ago(seconds: f64) -> Since {
        Since {
            at: game_time() - seconds,
        }
    }

    /// Seconds of game time since then.
    #[getter(TimeSince)]
    pub fn seconds(t: Since) -> f64 {
        game_time() - t.at
    }

    /// Shorthand for `seconds > s`.
    #[method(TimeSince)]
    pub fn over(t: Since, s: f64) -> bool {
        game_time() - t.at > s
    }
}

#[ts_api(namespace = "TimeUntil", values(TimeUntil = Until [at: f64, length: f64]))]
pub mod time_until_api {
    use super::{Until, game_time};

    /// Done in `seconds` seconds of game time.
    pub fn r#in(seconds: f64) -> Until {
        Until {
            at: game_time() + seconds,
            length: seconds,
        }
    }

    /// Seconds left (0 once done).
    #[getter(TimeUntil)]
    pub fn seconds(t: Until) -> f64 {
        (t.at - game_time()).max(0.0)
    }

    #[getter(TimeUntil)]
    pub fn done(t: Until) -> bool {
        game_time() >= t.at
    }

    /// 0 at the start, 1 when done.
    #[getter(TimeUntil)]
    pub fn fraction(t: Until) -> f64 {
        if t.length <= 0.0 {
            1.0
        } else {
            (1.0 - (t.at - game_time()) / t.length).min(1.0)
        }
    }
}
