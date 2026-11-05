// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Keyframes and curves, as `AnimationCurve` in `js/engine/src/animation.ts`: keys joined by cubic
//! Hermite segments shaped by their tangents, wrap modes before and after.

use serde_json::{Value, json};
use ts_api::ValueType;

/// One key on a curve: tangents are slopes (value per second). An infinite tangent holds a value
/// (a step).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    pub time: f64,
    pub value: f64,
    pub in_tangent: f64,
    pub out_tangent: f64,
}

impl ValueType<4> for Keyframe {
    fn from_parts(p: [f64; 4]) -> Self {
        Keyframe {
            time: p[0],
            value: p[1],
            in_tangent: p[2],
            out_tangent: p[3],
        }
    }

    fn to_parts(self) -> [f64; 4] {
        [self.time, self.value, self.in_tangent, self.out_tangent]
    }
}

impl Keyframe {
    pub fn new(time: f64, value: f64) -> Self {
        Keyframe {
            time,
            value,
            in_tangent: 0.0,
            out_tangent: 0.0,
        }
    }
}

/// What happens past a curve's or clip's ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapMode {
    Once,
    Loop,
    PingPong,
    ClampForever,
}

impl WrapMode {
    pub fn name(self) -> &'static str {
        match self {
            WrapMode::Once => "once",
            WrapMode::Loop => "loop",
            WrapMode::PingPong => "pingPong",
            WrapMode::ClampForever => "clampForever",
        }
    }

    pub fn parse(s: &str) -> Option<WrapMode> {
        Some(match s {
            "once" => WrapMode::Once,
            "loop" => WrapMode::Loop,
            "pingPong" => WrapMode::PingPong,
            "clampForever" => WrapMode::ClampForever,
            _ => return None,
        })
    }
}

/// Time `t` folded into 0..length by a wrap mode. `once` and `clampForever` clamp.
pub fn wrap(t: f64, length: f64, mode: WrapMode) -> f64 {
    if length <= 0.0 {
        return 0.0;
    }
    match mode {
        WrapMode::Loop => ((t % length) + length) % length,
        WrapMode::PingPong => {
            let p = ((t % (2.0 * length)) + 2.0 * length) % (2.0 * length);
            if p > length { 2.0 * length - p } else { p }
        }
        _ => platypus_math::jsnum::min(length, platypus_math::jsnum::max(0.0, t)),
    }
}

/// How `from_points` makes tangents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointMode {
    Linear,
    Smooth,
    Step,
}

impl PointMode {
    pub fn parse(s: &str) -> Option<PointMode> {
        Some(match s {
            "linear" => PointMode::Linear,
            "smooth" => PointMode::Smooth,
            "step" => PointMode::Step,
            _ => return None,
        })
    }
}

/// Where a time falls on a curve ([`Curve::locate`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Spot {
    t: f64,
    lo: usize,
    hi: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub keys: Vec<Keyframe>,
    pub pre_wrap: WrapMode,
    pub post_wrap: WrapMode,
}

impl Default for Curve {
    fn default() -> Self {
        Curve::new(Vec::new())
    }
}

impl Curve {
    /// A curve through these keys (sorted by time).
    pub fn new(mut keys: Vec<Keyframe>) -> Curve {
        keys.sort_by(|a, b| {
            (a.time - b.time)
                .partial_cmp(&0.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Curve {
            keys,
            pre_wrap: WrapMode::ClampForever,
            post_wrap: WrapMode::ClampForever,
        }
    }

    /// A straight line from (t0, v0) to (t1, v1).
    pub fn linear(t0: f64, v0: f64, t1: f64, v1: f64) -> Curve {
        let slope = if t1 == t0 { 0.0 } else { (v1 - v0) / (t1 - t0) };
        Curve::new(vec![
            Keyframe {
                time: t0,
                value: v0,
                in_tangent: 0.0,
                out_tangent: slope,
            },
            Keyframe {
                time: t1,
                value: v1,
                in_tangent: slope,
                out_tangent: 0.0,
            },
        ])
    }

    /// From (t0, v0) to (t1, v1), easing in and out (flat tangents).
    pub fn ease_in_out(t0: f64, v0: f64, t1: f64, v1: f64) -> Curve {
        Curve::new(vec![Keyframe::new(t0, v0), Keyframe::new(t1, v1)])
    }

    /// `value` from t0 to t1.
    pub fn constant(t0: f64, t1: f64, value: f64) -> Curve {
        Curve::new(vec![Keyframe::new(t0, value), Keyframe::new(t1, value)])
    }

    /// Keys through points with tangents made to match.
    pub fn from_points(points: &[(f64, f64)], mode: PointMode) -> Curve {
        let mut keys: Vec<Keyframe> = points.iter().map(|&(t, v)| Keyframe::new(t, v)).collect();
        let slope = |a: &Keyframe, b: &Keyframe| {
            if b.time == a.time {
                0.0
            } else {
                (b.value - a.value) / (b.time - a.time)
            }
        };
        for i in 0..keys.len() {
            let prev = i.checked_sub(1).map(|j| keys[j]);
            let next = keys.get(i + 1).copied();
            let k = keys[i];
            let (tin, tout) = match mode {
                PointMode::Step => (f64::INFINITY, f64::INFINITY),
                PointMode::Linear => (
                    prev.map_or(0.0, |p| slope(&p, &k)),
                    next.map_or(0.0, |n| slope(&k, &n)),
                ),
                PointMode::Smooth => {
                    let s = match (prev, next) {
                        (Some(p), Some(n)) => slope(&p, &n),
                        (Some(p), None) => slope(&p, &k),
                        (None, Some(n)) => slope(&k, &n),
                        (None, None) => 0.0,
                    };
                    (s, s)
                }
            };
            keys[i].in_tangent = tin;
            keys[i].out_tangent = tout;
        }
        Curve::new(keys)
    }

    /// Adds a key, keeping them in time order; returns its index.
    pub fn add_key(&mut self, key: Keyframe) -> usize {
        let i = self
            .keys
            .iter()
            .position(|k| k.time > key.time)
            .unwrap_or(self.keys.len());
        self.keys.insert(i, key);
        i
    }

    pub fn remove_key(&mut self, index: usize) {
        if index < self.keys.len() {
            self.keys.remove(index);
        }
    }

    /// The curve's value at `time`.
    pub fn evaluate(&self, time: f64) -> f64 {
        self.at(self.locate(time))
    }

    /// Whether `locate` gives the same for both at any time: the same key times (bit for bit) and
    /// wrap modes. glTF channels' curves (x, y, z, w of one bone) share them.
    pub(crate) fn same_times(&self, other: &Curve) -> bool {
        self.pre_wrap == other.pre_wrap
            && self.post_wrap == other.post_wrap
            && self.keys.len() == other.keys.len()
            && self
                .keys
                .iter()
                .zip(&other.keys)
                .all(|(a, b)| a.time.to_bits() == b.time.to_bits())
    }

    /// Where `time` falls: the time folded by the wrap modes and the segment's keys. It depends
    /// only on the key times and wrap modes, so curves with [`Self::same_times`] share it.
    pub(crate) fn locate(&self, time: f64) -> Spot {
        let keys = &self.keys;
        if keys.len() < 2 {
            return Spot {
                t: time,
                lo: 0,
                hi: 0,
            };
        }
        let first = keys[0].time;
        let last = keys[keys.len() - 1].time;
        let span = last - first;
        let mut t = time;
        if t < first {
            t = first + wrap(t - first, span, self.pre_wrap);
        } else if t > last {
            t = first + wrap(t - first, span, self.post_wrap);
        }
        // The segment holding t (binary search).
        let (mut lo, mut hi) = (0usize, keys.len() - 1);
        while hi - lo > 1 {
            let mid = (lo + hi) >> 1;
            if keys[mid].time <= t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Spot { t, lo, hi }
    }

    /// The value at a spot `locate` found (on this curve or one with the same times).
    pub(crate) fn at(&self, spot: Spot) -> f64 {
        let keys = &self.keys;
        if keys.is_empty() {
            return 0.0;
        }
        if keys.len() == 1 {
            return keys[0].value;
        }
        let Spot { t, lo, hi } = spot;
        let (a, b) = (keys[lo], keys[hi]);
        let dt = b.time - a.time;
        if dt <= 0.0 {
            return b.value;
        }
        if !a.out_tangent.is_finite() || !b.in_tangent.is_finite() {
            return if t >= b.time { b.value } else { a.value };
        }
        let s = (t - a.time) / dt;
        let s2 = s * s;
        let s3 = s2 * s;
        (2.0 * s3 - 3.0 * s2 + 1.0) * a.value
            + (s3 - 2.0 * s2 + s) * a.out_tangent * dt
            + (-2.0 * s3 + 3.0 * s2) * b.value
            + (s3 - s2) * b.in_tangent * dt
    }

    /// As plain data, for saving (an infinite tangent is the text `"Infinity"`).
    pub fn to_json(&self) -> Value {
        let n = |x: f64| {
            if x.is_finite() {
                json!(x)
            } else {
                json!("Infinity")
            }
        };
        json!({
            "keys": self.keys.iter().map(|k| json!([k.time, k.value, n(k.in_tangent), n(k.out_tangent)])).collect::<Vec<_>>(),
            "pre": self.pre_wrap.name(),
            "post": self.post_wrap.name(),
        })
    }

    pub fn from_json(data: &Value) -> Curve {
        let num = |v: &Value| match v {
            Value::String(s) if s == "Infinity" => f64::INFINITY,
            other => other.as_f64().unwrap_or(f64::NAN),
        };
        let keys = data["keys"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|k| Keyframe {
                        time: num(&k[0]),
                        value: num(&k[1]),
                        in_tangent: num(&k[2]),
                        out_tangent: num(&k[3]),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut c = Curve::new(keys);
        c.pre_wrap = data["pre"]
            .as_str()
            .and_then(WrapMode::parse)
            .unwrap_or(WrapMode::ClampForever);
        c.post_wrap = data["post"]
            .as_str()
            .and_then(WrapMode::parse)
            .unwrap_or(WrapMode::ClampForever);
        c
    }
}
