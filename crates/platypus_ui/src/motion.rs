// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Transitions and animations (M3d group 7): timing functions, interpolation of
//! computed CSS values, and the `transition*` / `animation*` properties.
//!
//! Values are interpolated as CSS text, token by token: numbers with the same unit
//! blend, numbers with different units blend through `calc()`, colours blend in
//! premultiplied sRGB, and everything else must match exactly. That covers lengths,
//! colours, transforms and filters with the same functions, shadows with the same
//! layers, and so on; `none` blends with a function list or a shadow as its neutral
//! form (zero translations, unit scales, transparent shadows). Anything else isn't
//! interpolable: transitions skip it and animations flip halfway, as in CSS.

use crate::Rgba;

/// An easing function (`transition-timing-function`, `animation-timing-function`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timing {
    Linear,
    CubicBezier(f32, f32, f32, f32),
    /// `steps(n, position)`.
    Steps(u32, StepPosition),
}

/// Where a `steps()` jump happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepPosition {
    Start,
    End,
    None,
    Both,
}

impl Default for Timing {
    /// `ease`, the CSS initial value.
    fn default() -> Self {
        Self::EASE
    }
}

impl Timing {
    pub const EASE: Timing = Timing::CubicBezier(0.25, 0.1, 0.25, 1.0);

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        Some(match s {
            "linear" => Timing::Linear,
            "ease" => Timing::EASE,
            "ease-in" => Timing::CubicBezier(0.42, 0.0, 1.0, 1.0),
            "ease-out" => Timing::CubicBezier(0.0, 0.0, 0.58, 1.0),
            "ease-in-out" => Timing::CubicBezier(0.42, 0.0, 0.58, 1.0),
            "step-start" => Timing::Steps(1, StepPosition::Start),
            "step-end" => Timing::Steps(1, StepPosition::End),
            _ => {
                let (name, args) = s.strip_suffix(')')?.split_once('(')?;
                let args: Vec<&str> = args.split(',').map(str::trim).collect();
                match (name.trim(), &args[..]) {
                    ("cubic-bezier", [a, b, c, d]) => {
                        let n = |v: &str| v.parse::<f32>().ok().filter(|n| n.is_finite());
                        let (x1, y1, x2, y2) = (n(a)?, n(b)?, n(c)?, n(d)?);
                        // x must stay in [0, 1] so time keeps moving forward.
                        if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                            return None;
                        }
                        Timing::CubicBezier(x1, y1, x2, y2)
                    }
                    ("steps", [n, rest @ ..]) if rest.len() <= 1 => {
                        let n: u32 = n.parse().ok()?;
                        let position = match rest.first().copied() {
                            None | Some("end" | "jump-end") => StepPosition::End,
                            Some("start" | "jump-start") => StepPosition::Start,
                            Some("jump-none") => StepPosition::None,
                            Some("jump-both") => StepPosition::Both,
                            _ => return None,
                        };
                        let min = if position == StepPosition::None { 2 } else { 1 };
                        if n < min {
                            return None;
                        }
                        Timing::Steps(n, position)
                    }
                    _ => return None,
                }
            }
        })
    }

    /// The eased progress at input progress `t` (0..=1).
    pub fn eval(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Timing::Linear => t,
            Timing::CubicBezier(x1, y1, x2, y2) => {
                let bezier = |a: f32, b: f32, s: f32| {
                    let u = 1.0 - s;
                    3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
                };
                // Solve x(s) = t: Newton's method, then bisection if it stalls.
                let mut s = t;
                for _ in 0..8 {
                    let x = bezier(x1, x2, s) - t;
                    if x.abs() < 1e-6 {
                        return bezier(y1, y2, s);
                    }
                    let u = 1.0 - s;
                    let dx = 3.0 * u * u * x1 + 6.0 * u * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2);
                    if dx.abs() < 1e-6 {
                        break;
                    }
                    s = (s - x / dx).clamp(0.0, 1.0);
                }
                let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
                s = t;
                for _ in 0..30 {
                    let x = bezier(x1, x2, s);
                    if (x - t).abs() < 1e-6 {
                        break;
                    }
                    if x < t {
                        lo = s;
                    } else {
                        hi = s;
                    }
                    s = (lo + hi) / 2.0;
                }
                bezier(y1, y2, s)
            }
            Timing::Steps(n, position) => {
                let n = n as f32;
                let mut step = (t * n).floor();
                if matches!(position, StepPosition::Start | StepPosition::Both) {
                    step += 1.0;
                }
                if t >= 1.0 && position != StepPosition::Start && position != StepPosition::Both {
                    step = n;
                }
                let jumps = match position {
                    StepPosition::Start | StepPosition::End => n,
                    StepPosition::None => n - 1.0,
                    StepPosition::Both => n + 1.0,
                };
                (step / jumps).clamp(0.0, 1.0)
            }
        }
    }
}

/// One `transition` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionSpec {
    /// camelCase property name, or `all`.
    pub property: String,
    pub duration_ms: f64,
    pub delay_ms: f64,
    pub timing: Timing,
}

impl TransitionSpec {
    /// Whether this entry applies to a change of `prop` (camelCase). A shorthand and
    /// its longhands match each other (`background` ↔ `backgroundColor`).
    pub fn matches(&self, prop: &str) -> bool {
        self.property == "all" || related(&self.property, prop)
    }
}

/// Whether two camelCase property names are the same property or a shorthand and one
/// of its longhands.
pub fn related(a: &str, b: &str) -> bool {
    let longhand_of = |long: &str, short: &str| {
        long.strip_prefix(short)
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_uppercase()))
    };
    a == b || longhand_of(a, b) || longhand_of(b, a)
}

/// The `transition*` longhands as set on an element (lists, combined by index).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransitionLists {
    pub property: Vec<String>,
    pub duration: Vec<f64>,
    pub delay: Vec<f64>,
    pub timing: Vec<Timing>,
}

impl TransitionLists {
    /// The entries: one per property, the other lists repeating to match (CSS).
    pub fn specs(&self) -> Vec<TransitionSpec> {
        // `transition-property` defaults to `all`.
        let all = ["all".to_owned()];
        let properties = if self.property.is_empty() {
            &all[..]
        } else {
            &self.property[..]
        };
        let pick = |list: &[f64], i: usize| {
            if list.is_empty() {
                0.0
            } else {
                list[i % list.len()]
            }
        };
        properties
            .iter()
            .enumerate()
            .filter(|(_, p)| p.as_str() != "none")
            .map(|(i, p)| TransitionSpec {
                property: p.clone(),
                duration_ms: pick(&self.duration, i),
                delay_ms: pick(&self.delay, i),
                timing: if self.timing.is_empty() {
                    Timing::EASE
                } else {
                    self.timing[i % self.timing.len()]
                },
            })
            .filter(|s| s.duration_ms > 0.0)
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.specs().is_empty()
    }
}

/// A CSS time (`0.3s`, `150ms`) in milliseconds.
pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    let (n, scale) = if let Some(n) = s.strip_suffix("ms") {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1000.0)
    } else if s == "0" {
        ("0", 1.0)
    } else {
        return None;
    };
    let v: f64 = n.trim().parse().ok()?;
    v.is_finite().then_some(v * scale)
}

/// A comma list, each item parsed by `f`.
pub fn parse_list<T>(s: &str, f: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    crate::paint::split_top_level(s, ',')
        .into_iter()
        .map(|p| f(p.trim()))
        .collect()
}

/// `transition-property`: `none`, `all` or property names (kebab-case in CSS).
pub fn parse_property(s: &str) -> Option<String> {
    let s = s.trim();
    let ok = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| crate::css::camel_case(s))
}

/// Parses the `transition` shorthand into its longhand lists.
pub fn parse_transition(s: &str) -> Option<TransitionLists> {
    let mut out = TransitionLists::default();
    for item in crate::paint::split_top_level(s, ',') {
        let (mut property, mut times, mut timing) = (None, Vec::new(), None);
        for part in split_space(item) {
            if let Some(t) = parse_time(part) {
                times.push(t);
            } else if let Some(t) = Timing::parse(part).filter(|_| timing.is_none()) {
                timing = Some(t);
            } else if part == "normal" || part == "allow-discrete" {
                // `transition-behavior`: discrete properties never transition here.
            } else if property.is_none() {
                property = Some(parse_property(part)?);
            } else {
                return None;
            }
        }
        if times.len() > 2 {
            return None;
        }
        out.property.push(property.unwrap_or_else(|| "all".into()));
        out.duration.push(times.first().copied().unwrap_or(0.0));
        out.delay.push(times.get(1).copied().unwrap_or(0.0));
        out.timing.push(timing.unwrap_or(Timing::EASE));
    }
    Some(out)
}

/// Splits on top-level spaces (keeping `cubic-bezier(0, 0, 1, 1)` whole).
pub fn split_space(s: &str) -> Vec<&str> {
    crate::paint::split_top_level(s.trim(), ' ')
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

// --- Animations ----------------------------------------------------------------------

/// `animation-direction`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayDirection {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

/// `animation-fill-mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillMode {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}

/// One `animation` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationSpec {
    pub name: String,
    pub duration_ms: f64,
    pub delay_ms: f64,
    /// `infinite` is `f64::INFINITY`.
    pub iterations: f64,
    pub direction: PlayDirection,
    pub fill: FillMode,
    pub paused: bool,
    pub timing: Timing,
}

/// The `animation*` longhands as set on an element (lists, combined by index).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationLists {
    pub name: Vec<String>,
    pub duration: Vec<f64>,
    pub delay: Vec<f64>,
    pub iterations: Vec<f64>,
    pub direction: Vec<PlayDirection>,
    pub fill: Vec<FillMode>,
    pub paused: Vec<bool>,
    pub timing: Vec<Timing>,
}

fn cycle<T: Copy>(list: &[T], i: usize, default: T) -> T {
    if list.is_empty() {
        default
    } else {
        list[i % list.len()]
    }
}

impl AnimationLists {
    /// The entries: one per name (`none` skipped), the other lists repeating.
    pub fn specs(&self) -> Vec<AnimationSpec> {
        self.name
            .iter()
            .enumerate()
            .filter(|(_, n)| n.as_str() != "none")
            .map(|(i, name)| AnimationSpec {
                name: name.clone(),
                duration_ms: cycle(&self.duration, i, 0.0),
                delay_ms: cycle(&self.delay, i, 0.0),
                iterations: cycle(&self.iterations, i, 1.0),
                direction: cycle(&self.direction, i, PlayDirection::Normal),
                fill: cycle(&self.fill, i, FillMode::None),
                paused: cycle(&self.paused, i, false),
                timing: cycle(&self.timing, i, Timing::EASE),
            })
            .collect()
    }
}

pub fn parse_iterations(s: &str) -> Option<f64> {
    match s.trim() {
        "infinite" => Some(f64::INFINITY),
        n => n.parse::<f64>().ok().filter(|n| *n >= 0.0 && n.is_finite()),
    }
}

pub fn parse_direction(s: &str) -> Option<PlayDirection> {
    Some(match s.trim() {
        "normal" => PlayDirection::Normal,
        "reverse" => PlayDirection::Reverse,
        "alternate" => PlayDirection::Alternate,
        "alternate-reverse" => PlayDirection::AlternateReverse,
        _ => return None,
    })
}

pub fn parse_fill(s: &str) -> Option<FillMode> {
    Some(match s.trim() {
        "none" => FillMode::None,
        "forwards" => FillMode::Forwards,
        "backwards" => FillMode::Backwards,
        "both" => FillMode::Both,
        _ => return None,
    })
}

pub fn parse_play_state(s: &str) -> Option<bool> {
    match s.trim() {
        "running" => Some(false),
        "paused" => Some(true),
        _ => None,
    }
}

/// An animation name: an identifier or a quoted string.
pub fn parse_name(s: &str) -> Option<String> {
    let s = s.trim();
    let quoted = s
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')));
    if let Some(q) = quoted {
        return Some(q.to_owned());
    }
    let ok = !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    ok.then(|| s.to_owned())
}

/// Parses the `animation` shorthand into its longhand lists.
pub fn parse_animation(s: &str) -> Option<AnimationLists> {
    let mut out = AnimationLists::default();
    for item in crate::paint::split_top_level(s, ',') {
        let mut times = Vec::new();
        let (mut name, mut timing, mut iterations, mut direction, mut fill, mut paused) =
            (None, None, None, None, None, None);
        for part in split_space(item) {
            if let Some(t) = parse_time(part) {
                times.push(t);
            } else if timing.is_none()
                && let Some(t) = Timing::parse(part)
            {
                timing = Some(t);
            } else if iterations.is_none()
                && let Some(n) = parse_iterations(part)
            {
                iterations = Some(n);
            } else if direction.is_none()
                && let Some(d) = parse_direction(part)
            {
                direction = Some(d);
            } else if fill.is_none()
                && part != "none"
                && let Some(f) = parse_fill(part)
            {
                fill = Some(f);
            } else if paused.is_none()
                && let Some(p) = parse_play_state(part)
            {
                paused = Some(p);
            } else if name.is_none() {
                name = Some(parse_name(part)?);
            } else {
                return None;
            }
        }
        if times.len() > 2 {
            return None;
        }
        out.name.push(name.unwrap_or_else(|| "none".into()));
        out.duration.push(times.first().copied().unwrap_or(0.0));
        out.delay.push(times.get(1).copied().unwrap_or(0.0));
        out.timing.push(timing.unwrap_or(Timing::EASE));
        out.iterations.push(iterations.unwrap_or(1.0));
        out.direction.push(direction.unwrap_or_default());
        out.fill.push(fill.unwrap_or_default());
        out.paused.push(paused.unwrap_or(false));
    }
    Some(out)
}

/// A running animation on one element.
#[derive(Clone, Debug)]
pub(crate) struct Animation {
    pub name: String,
    /// When it was applied, in engine milliseconds (before its delay).
    pub start_ms: f64,
    /// Set while `animation-play-state: paused`: when it paused.
    pub paused_at: Option<f64>,
    /// The last iteration reported (for `animationiteration`).
    pub iteration: u64,
    pub started: bool,
    pub ended: bool,
}

/// Where an animation is at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Phase {
    /// Before its delay ends.
    Before,
    /// Running: the iteration and the directed progress through it (0..=1).
    Active(u64, f64),
    /// Finished.
    After,
}

impl Animation {
    /// The phase at `now`, and the directed progress to show (if any: fill modes
    /// decide whether the before and after phases show a frame).
    pub fn sample(&self, spec: &AnimationSpec, now: f64) -> (Phase, Option<f64>) {
        let elapsed = self.paused_at.unwrap_or(now) - self.start_ms - spec.delay_ms;
        let directed = |iteration: u64, f: f64| {
            let odd = iteration % 2 == 1;
            match spec.direction {
                PlayDirection::Normal => f,
                PlayDirection::Reverse => 1.0 - f,
                PlayDirection::Alternate if odd => 1.0 - f,
                PlayDirection::Alternate => f,
                PlayDirection::AlternateReverse if odd => f,
                PlayDirection::AlternateReverse => 1.0 - f,
            }
        };
        if elapsed < 0.0 {
            let fill = matches!(spec.fill, FillMode::Backwards | FillMode::Both);
            return (Phase::Before, fill.then(|| directed(0, 0.0)));
        }
        let total = spec.duration_ms * spec.iterations;
        if spec.duration_ms <= 0.0 || elapsed >= total {
            let fill = matches!(spec.fill, FillMode::Forwards | FillMode::Both);
            // The end of the last iteration (part-way for fractional counts).
            let (iteration, f) = if spec.iterations > 0.0 && spec.iterations.fract() == 0.0 {
                (spec.iterations as u64 - 1, 1.0)
            } else {
                (spec.iterations.floor() as u64, spec.iterations.fract())
            };
            return (Phase::After, fill.then(|| directed(iteration, f)));
        }
        let progress = elapsed / spec.duration_ms;
        let iteration = progress.floor() as u64;
        let f = progress - progress.floor();
        (
            Phase::Active(iteration, directed(iteration, f)),
            Some(directed(iteration, f)),
        )
    }
}

/// Keyframes with resolved values: (offset, [(property, value)]), sorted by offset.
pub(crate) type Frames = Vec<(f32, Vec<(String, String)>)>;

/// Each animated property's value at progress `f` through `frames` (sorted). Missing
/// 0% / 100% frames take `base(prop)` (the element's own value). Each segment eases
/// with `timing` or its start frame's `animation-timing-function`.
pub(crate) fn keyframe_values(
    frames: &[(f32, Vec<(String, String)>)],
    f: f64,
    timing: Timing,
    base: &dyn Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut props: Vec<&String> = frames
        .iter()
        .flat_map(|(_, decls)| decls.iter().map(|(p, _)| p))
        .filter(|p| p.as_str() != "animationTimingFunction")
        .collect();
    props.sort();
    props.dedup();
    let mut out = Vec::new();
    for prop in props {
        // This property's frames, with the implicit ends.
        let mut points: Vec<(f64, String, Timing)> = frames
            .iter()
            .filter_map(|(offset, decls)| {
                let value = decls.iter().rev().find(|(p, _)| p == prop)?.1.clone();
                let ease = decls
                    .iter()
                    .rev()
                    .find(|(p, _)| p == "animationTimingFunction")
                    .and_then(|(_, v)| Timing::parse(v))
                    .unwrap_or(timing);
                Some((f64::from(*offset), value, ease))
            })
            .collect();
        if points.first().is_none_or(|p| p.0 > 0.0)
            && let Some(b) = base(prop)
        {
            points.insert(0, (0.0, b, timing));
        }
        if points.last().is_none_or(|p| p.0 < 1.0)
            && let Some(b) = base(prop)
        {
            points.push((1.0, b, timing));
        }
        let Some(first) = points.first() else {
            continue;
        };
        let value = if f <= first.0 {
            first.1.clone()
        } else if let Some(i) = points.windows(2).position(|w| f >= w[0].0 && f <= w[1].0) {
            let (a, b) = (&points[i], &points[i + 1]);
            let span = b.0 - a.0;
            let local = if span > 0.0 { (f - a.0) / span } else { 1.0 };
            let eased = f64::from(a.2.eval(local as f32));
            interpolate(&a.1, &b.1, eased).unwrap_or_else(|| {
                // Discrete values flip halfway.
                if eased < 0.5 {
                    a.1.clone()
                } else {
                    b.1.clone()
                }
            })
        } else {
            points.last().expect("non-empty").1.clone()
        };
        out.push((prop.clone(), value));
    }
    out
}

// --- Running state -----------------------------------------------------------------

/// What runs on one element: its transitions, and the values they start from.
#[derive(Clone, Debug, Default)]
pub(crate) struct Motion {
    /// Each property's winning computed value at the last restyle (before any
    /// transition is applied); `None` until the element was styled once.
    pub values: Option<std::collections::HashMap<String, String>>,
    pub transitions: Vec<Transition>,
    pub animations: Vec<Animation>,
}

impl Motion {
    /// Whether anything still changes over time (so the element restyles each frame).
    pub fn is_moving(&self) -> bool {
        !self.transitions.is_empty()
            || self
                .animations
                .iter()
                .any(|a| !a.ended && a.paused_at.is_none())
    }
}

/// A running transition of one property.
#[derive(Clone, Debug)]
pub(crate) struct Transition {
    pub prop: String,
    pub from: String,
    pub to: String,
    /// When it starts moving (after its delay), in engine milliseconds.
    pub start_ms: f64,
    pub duration_ms: f64,
    pub timing: Timing,
    /// `transitionstart` was sent.
    pub started: bool,
}

impl Transition {
    /// The value at `now`, and whether the transition has finished.
    pub fn value_at(&self, now: f64) -> (String, bool) {
        let t = ((now - self.start_ms) / self.duration_ms).max(0.0);
        if t >= 1.0 {
            return (self.to.clone(), true);
        }
        let eased = f64::from(self.timing.eval(t as f32));
        let value = interpolate(&self.from, &self.to, eased).unwrap_or_else(|| self.to.clone());
        (value, false)
    }
}

/// A transition or animation event for scripts.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionEvent {
    pub kind: crate::events::EventKind,
    pub target: web_ui_core::NodeId,
    /// The property (`transition*`, kebab-case like the DOM's `propertyName`) or the
    /// animation name.
    pub name: String,
    /// Seconds the transition or animation has run (`elapsedTime`).
    pub elapsed: f64,
}

/// camelCase → kebab-case (`backgroundColor` → `background-color`).
pub fn kebab_case(prop: &str) -> String {
    let mut out = String::with_capacity(prop.len() + 4);
    for c in prop.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

// --- Interpolation ------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64, String),
    Color(Rgba),
    Text(String),
}

const COLOR_FUNCTIONS: &[&str] = &[
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "hwb",
    "lab",
    "lch",
    "oklab",
    "oklch",
    "color",
    "color-mix",
];

/// Splits a value into numbers (with units), colours and other text.
fn tokenize(s: &str) -> Vec<Token> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let flush = |text: &mut String, out: &mut Vec<Token>| {
        if !text.is_empty() {
            out.push(Token::Text(std::mem::take(text)));
        }
    };
    while i < b.len() {
        let c = b[i] as char;
        let prev_word = i > 0 && {
            let p = b[i - 1] as char;
            p.is_ascii_alphanumeric()
                || p == '_'
                || p == '-' && i > 1 && (b[i - 2] as char).is_ascii_alphabetic()
        };
        let starts_number = !prev_word
            && (c.is_ascii_digit()
                || (c == '.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
                || ((c == '-' || c == '+')
                    && b.get(i + 1).is_some_and(|n| {
                        n.is_ascii_digit()
                            || (*n == b'.' && b.get(i + 2).is_some_and(u8::is_ascii_digit))
                    })));
        if starts_number {
            flush(&mut text, &mut out);
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            // Exponent (`1e-3`).
            if i + 1 < b.len()
                && (b[i] == b'e' || b[i] == b'E')
                && (b[i + 1].is_ascii_digit()
                    || ((b[i + 1] == b'-' || b[i + 1] == b'+')
                        && b.get(i + 2).is_some_and(u8::is_ascii_digit)))
            {
                i += 2;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let n: f64 = s[start..i].parse().unwrap_or(0.0);
            let unit_start = i;
            while i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'%') {
                i += 1;
            }
            out.push(Token::Number(n, s[unit_start..i].to_owned()));
            continue;
        }
        if c == '#' && !prev_word {
            let start = i;
            i += 1;
            while i < b.len() && b[i].is_ascii_hexdigit() {
                i += 1;
            }
            match Rgba::parse(&s[start..i]) {
                Some(color) => {
                    flush(&mut text, &mut out);
                    out.push(Token::Color(color));
                }
                None => text.push_str(&s[start..i]),
            }
            continue;
        }
        if c.is_ascii_alphabetic() && !prev_word {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'_') {
                i += 1;
            }
            let word = &s[start..i];
            if b.get(i) == Some(&b'(')
                && COLOR_FUNCTIONS.contains(&word.to_ascii_lowercase().as_str())
            {
                // The whole function, to its matching parenthesis.
                let mut depth = 0;
                let mut j = i;
                while j < b.len() {
                    match b[j] {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                j += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if let Some(color) = Rgba::parse(&s[start..j]) {
                    flush(&mut text, &mut out);
                    out.push(Token::Color(color));
                    i = j;
                    continue;
                }
                text.push_str(word);
                continue;
            }
            match Rgba::parse(word).filter(|_| b.get(i) != Some(&b'(')) {
                Some(color) if !word.eq_ignore_ascii_case("currentcolor") => {
                    flush(&mut text, &mut out);
                    out.push(Token::Color(color));
                }
                _ => text.push_str(word),
            }
            continue;
        }
        text.push(c);
        i += 1;
    }
    flush(&mut text, &mut out);
    // Collapse runs of white space so `a  b` matches `a b`.
    out.into_iter()
        .map(|t| match t {
            Token::Text(s) => {
                let mut collapsed = String::with_capacity(s.len());
                for ch in s.chars() {
                    if !ch.is_whitespace() {
                        collapsed.push(ch);
                    } else if !collapsed.ends_with(' ') {
                        collapsed.push(' ');
                    }
                }
                Token::Text(collapsed)
            }
            t => t,
        })
        .collect()
}

fn fmt_number(n: f64) -> String {
    let r = (n * 10_000.0).round() / 10_000.0;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

fn mix_colors(a: Rgba, b: Rgba, t: f64) -> Rgba {
    // Premultiplied, so a fade to `transparent` doesn't darken.
    let t = t as f32;
    let alpha = a.a + (b.a - a.a) * t;
    let pm = |ca: f32, cb: f32| ca * a.a + (cb * b.a - ca * a.a) * t;
    let un = |v: f32| {
        if alpha > 0.0 {
            (v / alpha).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    Rgba {
        r: un(pm(a.r, b.r)),
        g: un(pm(a.g, b.g)),
        b: un(pm(a.b, b.b)),
        a: alpha.clamp(0.0, 1.0),
    }
}

fn fmt_color(c: Rgba) -> String {
    format!(
        "rgba({}, {}, {}, {})",
        fmt_number(f64::from(c.r) * 255.0),
        fmt_number(f64::from(c.g) * 255.0),
        fmt_number(f64::from(c.b) * 255.0),
        fmt_number(f64::from(c.a))
    )
}

/// Interpolates CSS values `from` → `to` at `t` (0..=1, may overshoot for bouncy
/// easing). `None` if they can't be blended.
pub fn interpolate(from: &str, to: &str, t: f64) -> Option<String> {
    if from.trim() == to.trim() {
        return Some(to.trim().to_owned());
    }
    let (from, to) = match (from.trim(), to.trim()) {
        ("none", other) => (neutral(other)?, other.to_owned()),
        (other, "none") => (other.to_owned(), neutral(other)?),
        (a, b) => (a.to_owned(), b.to_owned()),
    };
    let (a, b) = (tokenize(&from), tokenize(&to));
    if a.len() != b.len() {
        return None;
    }
    let mut out = String::new();
    for (x, y) in a.iter().zip(&b) {
        match (x, y) {
            (Token::Number(m, u), Token::Number(n, v)) => {
                let blend = m + (n - m) * t;
                if u == v || (u.is_empty() && *m == 0.0) || (v.is_empty() && *n == 0.0) {
                    let unit = if u.is_empty() { v } else { u };
                    out.push_str(&fmt_number(blend));
                    out.push_str(unit);
                } else if u.is_empty() || v.is_empty() {
                    return None;
                } else {
                    // Mixed units (`10px` → `50%`): let `calc()` resolve them.
                    out.push_str(&format!(
                        "calc({}{u} * {} + {}{v} * {})",
                        fmt_number(*m),
                        fmt_number(1.0 - t),
                        fmt_number(*n),
                        fmt_number(t)
                    ));
                }
            }
            (Token::Color(c), Token::Color(d)) => out.push_str(&fmt_color(mix_colors(*c, *d, t))),
            (Token::Text(p), Token::Text(q)) if p == q => out.push_str(p),
            _ => return None,
        }
    }
    Some(out)
}

/// `none` as the neutral form of `other`: zero translations, rotations, skews and
/// offsets; unit scales and filter amounts; identity matrices; transparent colours.
fn neutral(other: &str) -> Option<String> {
    let tokens = tokenize(other);
    if !tokens.iter().any(|t| matches!(t, Token::Number(..))) {
        return None;
    }
    let mut out = String::new();
    let mut function = String::new();
    let mut arg = 0;
    for t in &tokens {
        match t {
            Token::Text(s) => {
                // Track the innermost function name and argument index.
                if let Some(open) = s.rfind('(') {
                    function = s[..open]
                        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    arg = 0;
                }
                if s.contains(',') {
                    arg += s.matches(',').count();
                }
                // `) scale(` closes one function and opens the next.
                if s.rfind(')') > s.rfind('(') {
                    function.clear();
                }
                out.push_str(s);
            }
            Token::Number(_, unit) => {
                let one = matches!(
                    function.as_str(),
                    "scale"
                        | "scaleX"
                        | "scaleY"
                        | "scaleZ"
                        | "scale3d"
                        | "brightness"
                        | "contrast"
                        | "saturate"
                        | "opacity"
                ) || (function == "matrix" && matches!(arg, 0 | 3))
                    || (function == "matrix3d" && matches!(arg, 0 | 5 | 10 | 15));
                out.push_str(if one { "1" } else { "0" });
                out.push_str(unit);
            }
            Token::Color(c) => out.push_str(&fmt_color(Rgba { a: 0.0, ..*c })),
        }
    }
    Some(out)
}

/// The value a property has when no declaration sets it, for transitions to and from
/// it. `None` where that isn't a fixed value (`auto`, inherited, `currentColor`).
pub fn initial_value(prop: &str) -> Option<&'static str> {
    Some(match prop {
        "opacity" => "1",
        "transform" | "translate" | "rotate" | "scale" | "filter" | "backdropFilter"
        | "boxShadow" | "textShadow" => "none",
        "backgroundColor" | "outlineColor" => "transparent",
        p if p.starts_with("margin") || p.starts_with("padding") => "0px",
        p if p.starts_with("border") && (p.ends_with("Width") || p.ends_with("Radius")) => "0px",
        "borderRadius" | "borderWidth" | "outlineWidth" | "outlineOffset" | "letterSpacing"
        | "wordSpacing" | "gap" | "rowGap" | "columnGap" | "flexGrow" => "0",
        "flexShrink" => "1",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // M3-ANI-01
    #[test]
    fn timing_functions_follow_css() {
        assert_eq!(Timing::Linear.eval(0.3), 0.3);
        let ease = Timing::parse("ease").unwrap();
        assert!(
            (ease.eval(0.5) - 0.8024).abs() < 0.002,
            "{}",
            ease.eval(0.5)
        );
        assert_eq!(ease.eval(0.0), 0.0);
        assert!((ease.eval(1.0) - 1.0).abs() < 1e-4);
        let ease_in = Timing::parse("cubic-bezier(0.42, 0, 1, 1)").unwrap();
        assert!(ease_in.eval(0.25) < 0.1);
        assert!(
            Timing::parse("cubic-bezier(2, 0, 1, 1)").is_none(),
            "x outside 0..1"
        );
        let steps = Timing::parse("steps(4)").unwrap();
        assert_eq!(
            (steps.eval(0.1), steps.eval(0.3), steps.eval(1.0)),
            (0.0, 0.25, 1.0)
        );
        let start = Timing::parse("steps(4, jump-start)").unwrap();
        assert_eq!(start.eval(0.1), 0.25);
        let none = Timing::parse("steps(3, jump-none)").unwrap();
        assert_eq!(
            (none.eval(0.0), none.eval(0.5), none.eval(1.0)),
            (0.0, 0.5, 1.0)
        );
        assert_eq!(
            Timing::parse("step-end"),
            Some(Timing::Steps(1, StepPosition::End))
        );
        assert!(Timing::parse("bouncy").is_none());
    }

    // M3-ANI-01
    #[test]
    fn values_interpolate_token_by_token() {
        assert_eq!(interpolate("0px", "100px", 0.25).unwrap(), "25px");
        assert_eq!(interpolate("1", "0", 0.5).unwrap(), "0.5");
        assert_eq!(
            interpolate("10px 20px", "20px 40px", 0.5).unwrap(),
            "15px 30px"
        );
        assert_eq!(
            interpolate(
                "translate(0px, 10px) rotate(0deg)",
                "translate(100px, 0px) rotate(90deg)",
                0.5
            )
            .unwrap(),
            "translate(50px, 5px) rotate(45deg)"
        );
        assert_eq!(
            interpolate("red", "#0000ff", 0.5).unwrap(),
            "rgba(127.5, 0, 127.5, 1)"
        );
        // Fading to transparent keeps the colour (premultiplied).
        assert_eq!(
            interpolate("rgb(255, 0, 0)", "transparent", 0.5).unwrap(),
            "rgba(255, 0, 0, 0.5)"
        );
        assert_eq!(
            interpolate("10px", "50%", 0.5).unwrap(),
            "calc(10px * 0.5 + 50% * 0.5)"
        );
        assert_eq!(interpolate("0", "10px", 0.5).unwrap(), "5px");
        assert!(interpolate("auto", "10px", 0.5).is_none());
        assert!(interpolate("solid", "dashed", 0.5).is_none());
        assert!(interpolate("translate(1px)", "scale(2)", 0.5).is_none());
        assert!(interpolate("1px 2px", "1px", 0.5).is_none());
        // Identifiers with digits aren't numbers.
        assert_eq!(
            interpolate(
                "translate3d(0px, 0px, 0px)",
                "translate3d(10px, 0px, 0px)",
                0.5
            )
            .unwrap(),
            "translate3d(5px, 0px, 0px)"
        );
    }

    // M3-ANI-01
    #[test]
    fn none_blends_as_the_neutral_value() {
        assert_eq!(
            interpolate("none", "translateX(100px) scale(2)", 0.5).unwrap(),
            "translateX(50px) scale(1.5)"
        );
        assert_eq!(
            interpolate("0px 4px 8px red", "none", 0.5).unwrap(),
            "0px 2px 4px rgba(255, 0, 0, 0.5)"
        );
        assert_eq!(interpolate("none", "blur(10px)", 0.5).unwrap(), "blur(5px)");
        assert_eq!(
            interpolate("none", "brightness(3)", 0.5).unwrap(),
            "brightness(2)"
        );
        assert!(interpolate("none", "block", 0.5).is_none());
    }

    // M3-ANI-03
    #[test]
    fn animation_shorthand() {
        let a = parse_animation(
            "spin 2s linear infinite, fade 300ms ease-out 1s 3 alternate both paused",
        )
        .unwrap();
        let s = a.specs();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "spin");
        assert_eq!((s[0].duration_ms, s[0].iterations), (2000.0, f64::INFINITY));
        assert_eq!(s[0].timing, Timing::Linear);
        assert_eq!(s[1].name, "fade");
        assert_eq!((s[1].delay_ms, s[1].iterations), (1000.0, 3.0));
        assert_eq!(s[1].direction, PlayDirection::Alternate);
        assert_eq!(s[1].fill, FillMode::Both);
        assert!(s[1].paused);
        assert!(parse_animation("none").unwrap().specs().is_empty());
        assert!(parse_animation("a b").is_none(), "two names");
    }

    // M3-ANI-03
    #[test]
    fn animations_sample_by_phase_and_direction() {
        let spec = |direction, fill, iterations| AnimationSpec {
            name: "a".into(),
            duration_ms: 1000.0,
            delay_ms: 500.0,
            iterations,
            direction,
            fill,
            paused: false,
            timing: Timing::Linear,
        };
        let a = Animation {
            name: "a".into(),
            start_ms: 0.0,
            paused_at: None,
            iteration: 0,
            started: false,
            ended: false,
        };
        let s = spec(PlayDirection::Normal, FillMode::None, 2.0);
        assert_eq!(a.sample(&s, 100.0), (Phase::Before, None));
        assert_eq!(a.sample(&s, 750.0), (Phase::Active(0, 0.25), Some(0.25)));
        assert_eq!(a.sample(&s, 1750.0), (Phase::Active(1, 0.25), Some(0.25)));
        assert_eq!(a.sample(&s, 2600.0), (Phase::After, None));
        let s = spec(PlayDirection::Alternate, FillMode::Both, 2.0);
        assert_eq!(a.sample(&s, 100.0).1, Some(0.0), "backwards fill");
        assert_eq!(a.sample(&s, 1750.0).1, Some(0.75), "second run goes back");
        assert_eq!(
            a.sample(&s, 9000.0).1,
            Some(0.0),
            "ends where the reversed run ends"
        );
        let s = spec(PlayDirection::Reverse, FillMode::Forwards, 1.5);
        assert_eq!(a.sample(&s, 9000.0).1, Some(0.5));
        let paused = Animation {
            paused_at: Some(1000.0),
            ..a.clone()
        };
        let s = spec(PlayDirection::Normal, FillMode::None, 1.0);
        assert_eq!(
            paused.sample(&s, 5000.0).1,
            Some(0.5),
            "frozen while paused"
        );
    }

    // M3-ANI-03
    #[test]
    fn keyframes_interpolate_with_implicit_ends() {
        let frames = vec![(
            0.5_f32,
            vec![
                ("opacity".to_owned(), "0".to_owned()),
                ("display".into(), "none".into()),
            ],
        )];
        let base = |p: &str| match p {
            "opacity" => Some("1".to_owned()),
            "display" => Some("flex".to_owned()),
            _ => None,
        };
        let at = |f| keyframe_values(&frames, f, Timing::Linear, &base);
        assert_eq!(
            at(0.2),
            [
                ("display".to_owned(), "flex".to_owned()),
                ("opacity".into(), "0.6".into())
            ]
        );
        assert_eq!(at(0.5)[1].1, "0");
        assert_eq!(at(0.75)[1].1, "0.5");
        assert_eq!(
            at(0.75)[0].1,
            "flex",
            "discrete: flips at the segment's middle"
        );
        assert_eq!(at(0.6)[0].1, "none");
    }

    // M3-ANI-01
    #[test]
    fn transition_shorthand_and_matching() {
        let t = parse_transition("opacity 0.3s ease-in, transform 200ms 1s, color 1s").unwrap();
        let specs = t.specs();
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].property, "opacity");
        assert_eq!(specs[0].duration_ms, 300.0);
        assert_eq!(specs[0].timing, Timing::parse("ease-in").unwrap());
        assert_eq!((specs[1].duration_ms, specs[1].delay_ms), (200.0, 1000.0));
        let t = parse_transition("background-color .5s").unwrap();
        assert_eq!(t.specs()[0].property, "backgroundColor");
        assert!(t.specs()[0].matches("background"));
        assert!(t.specs()[0].matches("backgroundColor"));
        assert!(!t.specs()[0].matches("color"));
        let all = parse_transition("1s linear").unwrap();
        assert!(all.specs()[0].matches("width"));
        assert!(parse_transition("opacity 1s 2s 3s").is_none());
        assert!(
            parse_transition("opacity 0s").unwrap().is_empty(),
            "no duration"
        );
        assert_eq!(parse_time("250ms"), Some(250.0));
        assert_eq!(parse_time("0"), Some(0.0));
        assert_eq!(parse_time("1"), None);
    }
}
