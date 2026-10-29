// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Paint-only style values (no effect on layout) and their CSS parsers:
//! corner radii, box shadows, blur filters and backgrounds.

use std::sync::Arc;

use crate::Rgba;
use crate::color::parse_angle_degrees;
use crate::draw::Radii;

/// A pixel length or a percentage (stored as a fraction, `50%` → `0.5`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Px(f32),
    Percent(f32),
}

impl Len {
    /// Parses `12px`, `12` (only `0` is valid unitless CSS, but numbers are pixels
    /// everywhere else in Platypus, so any number is accepted) or `50%`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(p) = s.strip_suffix('%') {
            return Some(Len::Percent(p.trim().parse::<f32>().ok()? / 100.0));
        }
        let n = s.strip_suffix("px").unwrap_or(s).trim();
        Some(Len::Px(n.parse().ok()?))
    }

    pub fn resolve(self, basis: f32) -> f32 {
        match self {
            Len::Px(px) => px,
            Len::Percent(f) => f * basis,
        }
    }
}

/// Per-corner values in CSS order: top-left, top-right, bottom-right, bottom-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corners<T> {
    pub top_left: T,
    pub top_right: T,
    pub bottom_right: T,
    pub bottom_left: T,
}

impl<T: Copy> Corners<T> {
    pub const fn all(v: T) -> Self {
        Self {
            top_left: v,
            top_right: v,
            bottom_right: v,
            bottom_left: v,
        }
    }

    pub fn map<U>(self, f: impl Fn(T) -> U) -> Corners<U> {
        Corners {
            top_left: f(self.top_left),
            top_right: f(self.top_right),
            bottom_right: f(self.bottom_right),
            bottom_left: f(self.bottom_left),
        }
    }

    pub fn to_array(self) -> [T; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }
}

/// One corner's radius: horizontal and vertical (equal for a circular corner).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CornerRadius {
    pub x: Len,
    pub y: Len,
}

impl CornerRadius {
    pub const ZERO: Self = Self::circular(Len::Px(0.0));

    pub const fn circular(r: Len) -> Self {
        Self { x: r, y: r }
    }

    /// One corner's longhand: `10px` or `10px 20px` (horizontal, vertical).
    pub fn parse(s: &str) -> Option<Self> {
        match parse_radius_list(s)?[..] {
            [r] => Some(Self::circular(r)),
            [x, y] => Some(Self { x, y }),
            _ => None,
        }
    }
}

fn parse_radius_list(s: &str) -> Option<Vec<Len>> {
    let values: Vec<Len> = s
        .split_whitespace()
        .map(Len::parse)
        .collect::<Option<_>>()?;
    let negative = values.iter().any(|v| match v {
        Len::Px(n) | Len::Percent(n) => *n < 0.0,
    });
    (!negative && !values.is_empty()).then_some(values)
}

/// `borderRadius`: 1–4 values in CSS order, optionally `/` and 1–4 vertical radii
/// for elliptical corners (`10px / 20px`).
pub fn parse_radii(s: &str) -> Option<Corners<CornerRadius>> {
    let four = |part: &str| -> Option<[Len; 4]> {
        Some(match parse_radius_list(part)?[..] {
            [a] => [a, a, a, a],
            [a, b] => [a, b, a, b],
            [a, b, c] => [a, b, c, b],
            [a, b, c, d] => [a, b, c, d],
            _ => return None,
        })
    };
    let (x, y) = match s.split_once('/') {
        Some((x, y)) => (four(x)?, four(y)?),
        None => {
            let x = four(s)?;
            (x, x)
        }
    };
    let corner = |i: usize| CornerRadius { x: x[i], y: y[i] };
    Some(Corners {
        top_left: corner(0),
        top_right: corner(1),
        bottom_right: corner(2),
        bottom_left: corner(3),
    })
}

/// Resolves radii for a `width` × `height` box (horizontal percentages of the
/// width, vertical of the height) and scales them all down, like CSS, so adjacent
/// corners never overlap.
pub fn resolve_radii(radii: &Corners<CornerRadius>, width: f32, height: f32) -> Radii {
    let corners = radii.to_array();
    let x = corners.map(|c| c.x.resolve(width).max(0.0));
    let y = corners.map(|c| c.y.resolve(height).max(0.0));
    let fit = |side: f32, a: f32, b: f32| if a + b > side { side / (a + b) } else { 1.0 };
    let f = fit(width, x[0], x[1])
        .min(fit(width, x[3], x[2]))
        .min(fit(height, y[0], y[3]))
        .min(fit(height, y[1], y[2]));
    Radii {
        x: x.map(|v| v * f),
        y: y.map(|v| v * f),
    }
}

/// One `boxShadow` (or, in M3, `textShadow`) entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Rgba,
    /// The colour is `currentColor` (given explicitly, or omitted as in CSS) and
    /// is resolved from the element's text colour when drawn.
    pub current_color: bool,
    pub inset: bool,
}

/// A comma-separated shadow list, e.g. `"0 4px 12px rgba(0,0,0,.4), inset 0 0 0 1px #fff"`.
/// `none` gives an empty list. A missing colour means `currentColor`, as in CSS.
pub fn parse_shadows(s: &str) -> Option<Vec<Shadow>> {
    if s.trim() == "none" {
        return Some(Vec::new());
    }
    split_top_level(s, ',')
        .into_iter()
        .map(parse_shadow)
        .collect()
}

fn parse_shadow(s: &str) -> Option<Shadow> {
    let mut inset = false;
    let mut color = None;
    let mut lengths = Vec::new();
    for token in split_top_level(s, ' ') {
        if token == "inset" && !inset {
            inset = true;
        } else if let Some(Len::Px(px)) = Len::parse(token) {
            lengths.push(px);
        } else if color.is_none() && token.eq_ignore_ascii_case("currentcolor") {
            color = Some(None);
        } else if color.is_none() {
            color = Some(Some(Rgba::parse(token)?));
        } else {
            return None;
        }
    }
    let (offset_x, offset_y, blur, spread) = match lengths[..] {
        [x, y] => (x, y, 0.0, 0.0),
        [x, y, b] => (x, y, b, 0.0),
        [x, y, b, s] => (x, y, b, s),
        _ => return None,
    };
    if blur < 0.0 {
        return None;
    }
    Some(Shadow {
        offset_x,
        offset_y,
        blur,
        spread,
        color: color.flatten().unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0)),
        current_color: !matches!(color, Some(Some(_))),
        inset,
    })
}

/// A 4×5 colour matrix, row-major (rows r, g, b, a; the last column is an offset),
/// applied to straight-alpha sRGB colours, as CSS filter functions are.
pub type ColorMatrix = [f32; 20];

const IDENTITY: ColorMatrix = [
    1.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, 0.0,
];

/// `b` after `a`: the matrix that applies `a`, then `b`.
fn then(a: &ColorMatrix, b: &ColorMatrix) -> ColorMatrix {
    let mut out = [0.0; 20];
    for row in 0..4 {
        for col in 0..5 {
            let mut v = if col == 4 { b[row * 5 + 4] } else { 0.0 };
            for k in 0..4 {
                v += b[row * 5 + k] * a[k * 5 + col];
            }
            out[row * 5 + col] = v;
        }
    }
    out
}

/// A `drop-shadow()`: like a box shadow, but of the element's drawn shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    /// Blur radius (twice the Gaussian's standard deviation, as for box shadows).
    pub blur: f32,
    pub color: Rgba,
    /// No colour given: `currentColor`.
    pub current_color: bool,
}

/// `filter` / `backdropFilter`: a chain of functions. Colour functions combine into
/// one matrix; blurs combine (their deviations add in quadrature). Platypus applies
/// the blur, then the colours, then the drop shadows, whatever the order written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// Gaussian standard deviation (px); 0 for none.
    pub blur: f32,
    pub matrix: Option<ColorMatrix>,
    pub drop_shadows: Vec<DropShadow>,
}

impl Filter {
    pub fn is_none(&self) -> bool {
        self.blur <= 0.0 && self.matrix.is_none() && self.drop_shadows.is_empty()
    }

    /// `none`, or space-separated `blur()`, `brightness()`, `contrast()`,
    /// `saturate()`, `grayscale()`, `sepia()`, `hue-rotate()`, `invert()`,
    /// `opacity()` and `drop-shadow()`.
    pub fn parse(s: &str) -> Option<Self> {
        let mut f = Filter::default();
        if s.trim() == "none" {
            return Some(f);
        }
        let mut matrix = IDENTITY;
        let mut colour = false;
        for call in split_top_level(s, ' ') {
            let (name, arg) = call.strip_suffix(')')?.split_once('(')?;
            let arg = arg.trim();
            // An amount: a number or percentage, defaulting to 1; never negative.
            let amount = |max: Option<f32>| -> Option<f32> {
                let v = if arg.is_empty() {
                    1.0
                } else if let Some(p) = arg.strip_suffix('%') {
                    p.trim().parse::<f32>().ok()? / 100.0
                } else {
                    arg.parse::<f32>().ok()?
                };
                (v >= 0.0).then(|| max.map_or(v, |m| v.min(m)))
            };
            let m = match name.trim() {
                "blur" => {
                    let px = if arg.is_empty() {
                        0.0
                    } else {
                        match Len::parse(arg)? {
                            Len::Px(px) if px >= 0.0 => px,
                            _ => return None,
                        }
                    };
                    f.blur = (f.blur * f.blur + px * px).sqrt();
                    continue;
                }
                "drop-shadow" => {
                    let shadow = parse_shadow(arg)?;
                    if shadow.inset || shadow.spread != 0.0 {
                        return None;
                    }
                    f.drop_shadows.push(DropShadow {
                        offset_x: shadow.offset_x,
                        offset_y: shadow.offset_y,
                        blur: shadow.blur,
                        color: shadow.color,
                        current_color: shadow.current_color,
                    });
                    continue;
                }
                "brightness" => {
                    let a = amount(None)?;
                    scale_offset([a, a, a, 1.0], [0.0; 4])
                }
                "contrast" => {
                    let a = amount(None)?;
                    let o = 0.5 - 0.5 * a;
                    scale_offset([a, a, a, 1.0], [o, o, o, 0.0])
                }
                "invert" => {
                    let a = amount(Some(1.0))?;
                    let d = 1.0 - 2.0 * a;
                    scale_offset([d, d, d, 1.0], [a, a, a, 0.0])
                }
                "opacity" => scale_offset([1.0, 1.0, 1.0, amount(Some(1.0))?], [0.0; 4]),
                "saturate" => saturate(amount(None)?),
                "grayscale" => saturate(1.0 - amount(Some(1.0))?),
                "sepia" => {
                    let a = 1.0 - amount(Some(1.0))?;
                    [
                        0.393 + 0.607 * a,
                        0.769 - 0.769 * a,
                        0.189 - 0.189 * a,
                        0.0,
                        0.0,
                        0.349 - 0.349 * a,
                        0.686 + 0.314 * a,
                        0.168 - 0.168 * a,
                        0.0,
                        0.0,
                        0.272 - 0.272 * a,
                        0.534 - 0.534 * a,
                        0.131 + 0.869 * a,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        1.0,
                        0.0,
                    ]
                }
                "hue-rotate" => {
                    let deg = if arg.is_empty() || arg == "0" {
                        0.0
                    } else {
                        crate::color::parse_angle_degrees(arg)
                            .filter(|_| arg.ends_with(|c: char| c.is_ascii_alphabetic()))?
                    };
                    hue_rotate(deg)
                }
                _ => return None,
            };
            matrix = then(&matrix, &m);
            colour = true;
        }
        f.matrix = colour.then_some(matrix);
        Some(f)
    }
}

fn scale_offset(s: [f32; 4], o: [f32; 4]) -> ColorMatrix {
    let mut m = [0.0; 20];
    for i in 0..4 {
        m[i * 5 + i] = s[i];
        m[i * 5 + 4] = o[i];
    }
    m
}

/// The Filter Effects spec's `saturate` matrix (grayscale is `saturate(1 - a)`).
fn saturate(s: f32) -> ColorMatrix {
    [
        0.213 + 0.787 * s,
        0.715 - 0.715 * s,
        0.072 - 0.072 * s,
        0.0,
        0.0,
        0.213 - 0.213 * s,
        0.715 + 0.285 * s,
        0.072 - 0.072 * s,
        0.0,
        0.0,
        0.213 - 0.213 * s,
        0.715 - 0.715 * s,
        0.072 + 0.928 * s,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

/// The Filter Effects spec's `hueRotate` matrix.
fn hue_rotate(deg: f32) -> ColorMatrix {
    let (s, c) = deg.to_radians().sin_cos();
    [
        0.213 + c * 0.787 - s * 0.213,
        0.715 - c * 0.715 - s * 0.715,
        0.072 - c * 0.072 + s * 0.928,
        0.0,
        0.0,
        0.213 - c * 0.213 + s * 0.143,
        0.715 + c * 0.285 + s * 0.140,
        0.072 - c * 0.072 - s * 0.283,
        0.0,
        0.0,
        0.213 - c * 0.213 - s * 0.787,
        0.715 - c * 0.715 + s * 0.715,
        0.072 + c * 0.928 + s * 0.072,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

/// A colour stop in a gradient. `position` is `None` when the stop should be
/// spaced evenly between its neighbours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStop {
    pub color: Rgba,
    pub position: Option<Len>,
}

/// Direction of a linear gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientDirection {
    /// CSS angle: `0deg` points up, `90deg` right (`to right`), `180deg` down (the default).
    Angle(f32),
    /// `to top right` and friends: the angle depends on the box's aspect ratio,
    /// so it's resolved at draw time.
    Corner { right: bool, bottom: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadialShape {
    Circle,
    Ellipse,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundImage {
    /// An image file, loaded through the asset system by the renderer.
    Url(String),
    Linear {
        direction: GradientDirection,
        stops: Vec<ColorStop>,
        /// `repeating-linear-gradient`: the stops repeat beyond the first and last.
        repeating: bool,
    },
    /// Sized to the farthest corner (the CSS default), centred unless `at` places it.
    Radial {
        shape: RadialShape,
        /// The centre (`at X Y`), as fractions or lengths of the box.
        at: BackgroundPosition,
        stops: Vec<ColorStop>,
        repeating: bool,
    },
    /// `conic-gradient`: colours sweep clockwise around `at`, starting `from` degrees
    /// (0 is up). Stop positions are fractions of a turn.
    Conic {
        from: f32,
        at: BackgroundPosition,
        stops: Vec<ColorStop>,
        repeating: bool,
    },
}

/// `url(...)` or a gradient: `linear-gradient(...)`, `radial-gradient(...)`,
/// `conic-gradient(...)` and their `repeating-` forms.
pub fn parse_background_image(s: &str) -> Option<Option<BackgroundImage>> {
    let s = s.trim();
    if s == "none" {
        return Some(None);
    }
    let (repeating, s) = match s.strip_prefix("repeating-") {
        Some(rest) if rest.contains("-gradient(") => (true, rest),
        _ => (false, s),
    };
    let call = |name: &str| s.strip_prefix(name)?.strip_suffix(')');
    if let Some(arg) = call("url(").filter(|_| !repeating) {
        let path = arg.trim().trim_matches(['"', '\'']);
        return (!path.is_empty()).then(|| Some(BackgroundImage::Url(path.to_owned())));
    }
    if let Some(args) = call("linear-gradient(") {
        let mut parts = split_top_level(args, ',');
        let first = *parts.first()?;
        let direction = if let Some(to) = first.strip_prefix("to ") {
            parts.remove(0);
            let words: Vec<&str> = to.split_whitespace().collect();
            let has = |w: &str| words.contains(&w);
            match (
                words.len(),
                has("left") || has("right"),
                has("top") || has("bottom"),
            ) {
                (1, true, false) => {
                    GradientDirection::Angle(if has("right") { 90.0 } else { 270.0 })
                }
                (1, false, true) => {
                    GradientDirection::Angle(if has("bottom") { 180.0 } else { 0.0 })
                }
                (2, true, true) => GradientDirection::Corner {
                    right: has("right"),
                    bottom: has("bottom"),
                },
                _ => return None,
            }
        } else if Rgba::parse(split_top_level(first, ' ').first()?).is_none() {
            let angle = parse_angle_degrees(first)?;
            parts.remove(0);
            GradientDirection::Angle(angle)
        } else {
            GradientDirection::Angle(180.0)
        };
        return Some(Some(BackgroundImage::Linear {
            direction,
            stops: parse_stops(&parts, Len::parse)?,
            repeating,
        }));
    }
    if let Some(args) = call("radial-gradient(") {
        let mut parts = split_top_level(args, ',');
        // The first part is `[shape] [at <position>]`; anything else is a colour stop.
        let first = parts.first()?.trim();
        let (shape_word, at_word) = match first.split_once(" at ") {
            Some((shape, at)) => (shape.trim(), Some(at.trim())),
            None => match first.strip_prefix("at ") {
                Some(at) => ("", Some(at.trim())),
                None => (first, None),
            },
        };
        let shape = match shape_word {
            "circle" => Some(RadialShape::Circle),
            "ellipse" => Some(RadialShape::Ellipse),
            _ => None,
        };
        if shape.is_none() && at_word.is_some() && !shape_word.is_empty() {
            return None;
        }
        let at = match at_word {
            Some(position) => parse_background_position(position)?,
            None => BackgroundPosition {
                x: Len::Percent(0.5),
                y: Len::Percent(0.5),
            },
        };
        if shape.is_some() || at_word.is_some() {
            parts.remove(0);
        }
        return Some(Some(BackgroundImage::Radial {
            shape: shape.unwrap_or(RadialShape::Ellipse),
            at,
            stops: parse_stops(&parts, Len::parse)?,
            repeating,
        }));
    }
    if let Some(args) = call("conic-gradient(") {
        let mut parts = split_top_level(args, ',');
        let mut from = 0.0;
        let mut at = BackgroundPosition {
            x: Len::Percent(0.5),
            y: Len::Percent(0.5),
        };
        let first = parts.first()?.trim();
        if first.starts_with("from ") || first.starts_with("at ") {
            let (from_part, at_part) = match first.find("at ") {
                Some(i) => (&first[..i], Some(&first[i + 3..])),
                None => (first, None),
            };
            if let Some(angle) = from_part.trim().strip_prefix("from ") {
                from = parse_angle_degrees(angle)?;
            } else if !from_part.trim().is_empty() {
                return None;
            }
            if let Some(position) = at_part {
                at = parse_background_position(position)?;
            }
            parts.remove(0);
        }
        // Stop positions are angles or percentages of a turn.
        let turn = |t: &str| match Len::parse(t) {
            Some(Len::Percent(f)) => Some(Len::Percent(f)),
            Some(Len::Px(0.0)) => Some(Len::Percent(0.0)),
            _ => parse_angle_degrees(t)
                .filter(|_| t.ends_with(|c: char| c.is_ascii_alphabetic()))
                .map(|deg| Len::Percent(deg / 360.0)),
        };
        return Some(Some(BackgroundImage::Conic {
            from,
            at,
            stops: parse_stops(&parts, turn)?,
            repeating,
        }));
    }
    None
}

/// Colour stops: `color`, `color pos` or `color pos1 pos2` (two stops of one colour).
fn parse_stops(parts: &[&str], position: impl Fn(&str) -> Option<Len>) -> Option<Vec<ColorStop>> {
    let mut stops = Vec::new();
    for p in parts {
        let tokens = split_top_level(p, ' ');
        let (color, positions) = tokens.split_first()?;
        let color = Rgba::parse(color)?;
        match positions {
            [] => stops.push(ColorStop {
                color,
                position: None,
            }),
            [_] | [_, _] => {
                for pos in positions {
                    stops.push(ColorStop {
                        color,
                        position: Some(position(pos)?),
                    });
                }
            }
            _ => return None,
        }
    }
    (stops.len() >= 2).then_some(stops)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackgroundSize {
    Cover,
    Contain,
    /// Width and height; `None` means `auto` (keep the image's aspect ratio).
    Explicit(Option<Len>, Option<Len>),
}

impl Default for BackgroundSize {
    fn default() -> Self {
        BackgroundSize::Explicit(None, None)
    }
}

pub fn parse_background_size(s: &str) -> Option<BackgroundSize> {
    let len_or_auto = |t: &str| match t {
        "auto" => Some(None),
        other => Len::parse(other).map(Some),
    };
    match s.split_whitespace().collect::<Vec<_>>()[..] {
        ["cover"] => Some(BackgroundSize::Cover),
        ["contain"] => Some(BackgroundSize::Contain),
        [w] => Some(BackgroundSize::Explicit(len_or_auto(w)?, None)),
        [w, h] => Some(BackgroundSize::Explicit(len_or_auto(w)?, len_or_auto(h)?)),
        _ => None,
    }
}

/// Where the background sits in the box: percentages align the image's point with
/// the box's point (CSS semantics), pixels offset from the top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackgroundPosition {
    pub x: Len,
    pub y: Len,
}

impl Default for BackgroundPosition {
    fn default() -> Self {
        Self {
            x: Len::Percent(0.0),
            y: Len::Percent(0.0),
        }
    }
}

/// `center`, `top`, `left top`, `right 20%`, `10px 50%`, ...
/// `object-fit`: how an image fills its element's content box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObjectFit {
    #[default]
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

impl ObjectFit {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "fill" => Self::Fill,
            "contain" => Self::Contain,
            "cover" => Self::Cover,
            "none" => Self::None,
            "scale-down" => Self::ScaleDown,
            _ => return None,
        })
    }

    /// The equivalent `background-size` for an image of `natural` size in `area`
    /// (`scale-down` is `none` or `contain`, whichever is smaller).
    pub fn background_size(self, natural: (f32, f32), area: (f32, f32)) -> BackgroundSize {
        let full = Some(Len::Percent(1.0));
        match self {
            Self::Fill => BackgroundSize::Explicit(full, full),
            Self::Contain => BackgroundSize::Contain,
            Self::Cover => BackgroundSize::Cover,
            Self::None => BackgroundSize::Explicit(None, None),
            Self::ScaleDown if natural.0 <= area.0 && natural.1 <= area.1 => {
                BackgroundSize::Explicit(None, None)
            }
            Self::ScaleDown => BackgroundSize::Contain,
        }
    }
}

pub fn parse_background_position(s: &str) -> Option<BackgroundPosition> {
    let horizontal = |t: &str| match t {
        "left" => Some(Len::Percent(0.0)),
        "center" => Some(Len::Percent(0.5)),
        "right" => Some(Len::Percent(1.0)),
        _ => None,
    };
    let vertical = |t: &str| match t {
        "top" => Some(Len::Percent(0.0)),
        "center" => Some(Len::Percent(0.5)),
        "bottom" => Some(Len::Percent(1.0)),
        _ => None,
    };
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let centre = Len::Percent(0.5);
    let (x, y) = match tokens[..] {
        [t] => match (horizontal(t), vertical(t)) {
            (Some(x), _) if t != "center" => (x, centre),
            (_, Some(y)) => (centre, y),
            _ => (Len::parse(t)?, centre),
        },
        [a, b] => {
            // Keywords may come in either order (`top left`); lengths are x then y.
            if let (Some(y), Some(x)) = (vertical(a), horizontal(b))
                && a != "center"
            {
                (x, y)
            } else {
                (
                    horizontal(a).or_else(|| Len::parse(a))?,
                    vertical(b).or_else(|| Len::parse(b))?,
                )
            }
        }
        _ => return None,
    };
    Some(BackgroundPosition { x, y })
}

/// How a background image repeats along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatAxis {
    #[default]
    Repeat,
    NoRepeat,
    /// Repeat, scaled so a whole number of tiles fits.
    Round,
    /// Repeat whole tiles only, spreading the leftover space between them.
    Space,
}

/// `background-repeat`, per axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackgroundRepeat {
    pub x: RepeatAxis,
    pub y: RepeatAxis,
}

impl BackgroundRepeat {
    pub const REPEAT: Self = Self {
        x: RepeatAxis::Repeat,
        y: RepeatAxis::Repeat,
    };
    pub const NO_REPEAT: Self = Self {
        x: RepeatAxis::NoRepeat,
        y: RepeatAxis::NoRepeat,
    };
}

/// One layer's `background-repeat`: `repeat-x`, `repeat-y`, or one or two of
/// `repeat`, `no-repeat`, `round`, `space`.
pub fn parse_background_repeat(s: &str) -> Option<BackgroundRepeat> {
    let axis = |t: &str| {
        Some(match t {
            "repeat" => RepeatAxis::Repeat,
            "no-repeat" => RepeatAxis::NoRepeat,
            "round" => RepeatAxis::Round,
            "space" => RepeatAxis::Space,
            _ => return None,
        })
    };
    let words: Vec<&str> = s.split_whitespace().collect();
    let (x, y) = match words[..] {
        ["repeat-x"] => (RepeatAxis::Repeat, RepeatAxis::NoRepeat),
        ["repeat-y"] => (RepeatAxis::NoRepeat, RepeatAxis::Repeat),
        [both] => (axis(both)?, axis(both)?),
        [x, y] => (axis(x)?, axis(y)?),
        _ => return None,
    };
    Some(BackgroundRepeat { x, y })
}

/// `background-clip`: the area a background layer paints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackgroundClip {
    #[default]
    BorderBox,
    PaddingBox,
    ContentBox,
    /// Only inside the glyphs of the element's text (gradient text).
    Text,
}

impl BackgroundClip {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "border-box" => Self::BorderBox,
            "padding-box" => Self::PaddingBox,
            "content-box" => Self::ContentBox,
            "text" => Self::Text,
            _ => return None,
        })
    }
}

/// `border-image-repeat` along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderImageRepeat {
    #[default]
    Stretch,
    Repeat,
    Round,
    Space,
}

impl BorderImageRepeat {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "stretch" => Self::Stretch,
            "repeat" => Self::Repeat,
            "round" => Self::Round,
            "space" => Self::Space,
            _ => return None,
        })
    }
}

/// A `border-image-width` / `-outset` value: a multiple of the border width, a
/// length, a percentage of the border image area (widths only) or `auto` (the
/// slice's own size; widths only).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderImageLength {
    Number(f32),
    Px(f32),
    Percent(f32),
    Auto,
}

impl BorderImageLength {
    fn parse(s: &str, allow_percent_and_auto: bool) -> Option<Self> {
        let s = s.trim();
        if s == "auto" {
            return allow_percent_and_auto.then_some(Self::Auto);
        }
        if let Ok(n) = s.parse::<f32>() {
            return (n >= 0.0).then_some(Self::Number(n));
        }
        match Len::parse(s)? {
            Len::Px(px) if px >= 0.0 => Some(Self::Px(px)),
            Len::Percent(f) if f >= 0.0 && allow_percent_and_auto => Some(Self::Percent(f)),
            _ => None,
        }
    }

    /// In px, for a side with `border` width, `basis` for percentages and `slice`
    /// (the slice's size, already scaled to px) for `auto`.
    fn resolve(self, border: f32, basis: f32, slice: f32) -> f32 {
        match self {
            Self::Number(n) => n * border,
            Self::Px(px) => px,
            Self::Percent(f) => f * basis,
            Self::Auto => slice,
        }
    }
}

/// `border-image`: a 9-slice image (or gradient) drawn over the border area.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderImage {
    /// `None` (the default) draws the ordinary border.
    pub source: Option<Arc<BackgroundImage>>,
    /// Inward offsets into the image (top, right, bottom, left): px of the image
    /// (`Len::Px`) or fractions of its size.
    pub slice: [Len; 4],
    /// `fill`: also draw the middle piece.
    pub fill: bool,
    pub width: [BorderImageLength; 4],
    pub outset: [BorderImageLength; 4],
    /// Horizontal, vertical.
    pub repeat: [BorderImageRepeat; 2],
}

impl Default for BorderImage {
    fn default() -> Self {
        Self {
            source: None,
            slice: [Len::Percent(1.0); 4],
            fill: false,
            width: [BorderImageLength::Number(1.0); 4],
            outset: [BorderImageLength::Number(0.0); 4],
            repeat: [BorderImageRepeat::Stretch; 2],
        }
    }
}

/// One of the nine pieces of a border image: where it comes from in the image
/// (`src`, image px) and where it goes (`dst`, logical px), and how it fills `dst`
/// along each axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderImagePiece {
    pub src: crate::Rect,
    pub dst: crate::Rect,
    pub repeat: [BorderImageRepeat; 2],
    /// The size of one tile (the piece scaled to the border it fills).
    pub tile: (f32, f32),
}

/// Expands 1–4 values to top, right, bottom, left.
fn four<T: Copy>(values: &[T]) -> Option<[T; 4]> {
    Some(match *values {
        [a] => [a; 4],
        [a, b] => [a, b, a, b],
        [a, b, c] => [a, b, c, b],
        [a, b, c, d] => [a, b, c, d],
        _ => return None,
    })
}

impl BorderImage {
    /// `border-image-slice`: 1–4 numbers (image px) or percentages, and `fill`.
    pub fn parse_slice(s: &str) -> Option<([Len; 4], bool)> {
        let mut fill = false;
        let mut values = Vec::new();
        for token in s.split_whitespace() {
            if token == "fill" && !fill {
                fill = true;
            } else if let Ok(n) = token.parse::<f32>() {
                (n >= 0.0).then_some(())?;
                values.push(Len::Px(n));
            } else {
                match Len::parse(token)? {
                    Len::Percent(f) if f >= 0.0 => values.push(Len::Percent(f)),
                    _ => return None,
                }
            }
        }
        Some((four(&values)?, fill))
    }

    /// `border-image-width` (`outset: false`) or `border-image-outset`: 1–4 values.
    pub fn parse_lengths(s: &str, outset: bool) -> Option<[BorderImageLength; 4]> {
        let values: Vec<_> = s
            .split_whitespace()
            .map(|t| BorderImageLength::parse(t, !outset))
            .collect::<Option<_>>()?;
        four(&values)
    }

    /// `border-image-repeat`: one or two of `stretch`, `repeat`, `round`, `space`.
    pub fn parse_repeat(s: &str) -> Option<[BorderImageRepeat; 2]> {
        let values: Vec<_> = s
            .split_whitespace()
            .map(BorderImageRepeat::parse)
            .collect::<Option<_>>()?;
        match values[..] {
            [a] => Some([a, a]),
            [a, b] => Some([a, b]),
            _ => None,
        }
    }

    /// The `border-image` shorthand: `<source> <slice> [/ <width> [/ <outset>]] <repeat>`,
    /// any part optional except that `/` parts follow the slice.
    pub fn parse(s: &str) -> Option<Self> {
        let mut out = Self::default();
        if s.trim() == "none" {
            return Some(out);
        }
        let tokens = split_top_level(s, ' ');
        let mut i = 0;
        let mut slice_tokens = Vec::new();
        let mut repeat_tokens = Vec::new();
        let mut slash_parts: Vec<Vec<&str>> = Vec::new();
        while i < tokens.len() {
            let t = tokens[i];
            if t.contains('(') || t == "none" {
                (out.source.is_none()).then_some(())?;
                out.source = parse_background_image(t)?.map(Arc::new);
            } else if BorderImageRepeat::parse(t).is_some() {
                repeat_tokens.push(t);
            } else if t == "/" || t.starts_with('/') {
                // `/ width` or `/width`; a second slash starts the outset.
                slash_parts.push(Vec::new());
                let rest = t.trim_start_matches('/');
                if !rest.is_empty() {
                    slash_parts.last_mut()?.push(rest);
                }
            } else if let Some(part) = slash_parts.last_mut() {
                part.push(t);
            } else {
                slice_tokens.push(t);
            }
            i += 1;
        }
        if !slice_tokens.is_empty() {
            (out.slice, out.fill) = Self::parse_slice(&slice_tokens.join(" "))?;
        }
        match slash_parts.len() {
            0 => {}
            1 | 2 => {
                (!slice_tokens.is_empty()).then_some(())?;
                if !slash_parts[0].is_empty() {
                    out.width = Self::parse_lengths(&slash_parts[0].join(" "), false)?;
                }
                if let Some(outset) = slash_parts.get(1) {
                    out.outset = Self::parse_lengths(&outset.join(" "), true)?;
                }
            }
            _ => return None,
        }
        if !repeat_tokens.is_empty() {
            out.repeat = Self::parse_repeat(&repeat_tokens.join(" "))?;
        }
        Some(out)
    }

    /// The border image area: the border box grown by the outset.
    pub fn area(&self, border_box: crate::Rect, border: [f32; 4]) -> crate::Rect {
        let o = [0, 1, 2, 3].map(|i| self.outset[i].resolve(border[i], 0.0, 0.0));
        crate::Rect::new(
            border_box.x - o[3],
            border_box.y - o[0],
            border_box.width + o[1] + o[3],
            border_box.height + o[0] + o[2],
        )
    }

    /// The nine pieces for an image of `natural` size (a gradient's is the area's
    /// size) drawn over `area`, with the element's `border` widths. Pieces that
    /// would be empty are left out.
    pub fn pieces(
        &self,
        area: crate::Rect,
        border: [f32; 4],
        natural: (f32, f32),
    ) -> Vec<BorderImagePiece> {
        use crate::Rect;
        let (iw, ih) = natural;
        // Slices in image px, clamped so opposite slices don't cross.
        let s = [
            self.slice[0].resolve(ih).min(ih),
            self.slice[1].resolve(iw).min(iw),
            self.slice[2].resolve(ih).min(ih),
            self.slice[3].resolve(iw).min(iw),
        ];
        let basis = [area.height, area.width, area.height, area.width];
        let mut w = [0, 1, 2, 3].map(|i| self.width[i].resolve(border[i], basis[i], s[i]));
        // Like CSS, scale all widths down if opposite ones overlap.
        let f = (area.width / (w[1] + w[3]).max(f32::EPSILON))
            .min(area.height / (w[0] + w[2]).max(f32::EPSILON))
            .min(1.0);
        w = w.map(|v| v * f);
        let [t, r, b, l] = w;
        let xs = [
            area.x,
            area.x + l,
            area.x + area.width - r,
            area.x + area.width,
        ];
        let ys = [
            area.y,
            area.y + t,
            area.y + area.height - b,
            area.y + area.height,
        ];
        let sx = [0.0, s[3], iw - s[1], iw];
        let sy = [0.0, s[0], ih - s[2], ih];
        // Scale of the edge pieces: the image slice to the border width.
        let scale = |dst: f32, src: f32| if src > 0.0 { dst / src } else { 1.0 };
        let (top_scale, bottom_scale) = (scale(t, s[0]), scale(b, s[2]));
        let (left_scale, right_scale) = (scale(l, s[3]), scale(r, s[1]));
        let mut out = Vec::new();
        for row in 0..3 {
            for col in 0..3 {
                if row == 1 && col == 1 && !self.fill {
                    continue;
                }
                let src = Rect::new(
                    sx[col],
                    sy[row],
                    sx[col + 1] - sx[col],
                    sy[row + 1] - sy[row],
                );
                let dst = Rect::new(
                    xs[col],
                    ys[row],
                    xs[col + 1] - xs[col],
                    ys[row + 1] - ys[row],
                );
                if src.width <= 0.0 || src.height <= 0.0 || dst.width <= 0.0 || dst.height <= 0.0 {
                    continue;
                }
                // Corners stretch; edges repeat along their length; the middle
                // follows the top and left edges' scales.
                let horizontal = if col == 1 {
                    self.repeat[0]
                } else {
                    BorderImageRepeat::Stretch
                };
                let vertical = if row == 1 {
                    self.repeat[1]
                } else {
                    BorderImageRepeat::Stretch
                };
                let x_scale = match row {
                    0 => top_scale,
                    2 => bottom_scale,
                    _ => top_scale,
                };
                let y_scale = match col {
                    0 => left_scale,
                    2 => right_scale,
                    _ => left_scale,
                };
                let tile = (
                    if horizontal == BorderImageRepeat::Stretch {
                        dst.width
                    } else {
                        src.width * x_scale
                    },
                    if vertical == BorderImageRepeat::Stretch {
                        dst.height
                    } else {
                        src.height * y_scale
                    },
                );
                out.push(BorderImagePiece {
                    src,
                    dst,
                    repeat: [horizontal, vertical],
                    tile,
                });
            }
        }
        out
    }
}

/// `border-style` for one side. Platypus's default is `solid` (a width alone draws a
/// border, as in s&box); CSS's is `none`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    Hidden,
    #[default]
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl BorderStyle {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "none" => Self::None,
            "hidden" => Self::Hidden,
            "solid" => Self::Solid,
            "dashed" => Self::Dashed,
            "dotted" => Self::Dotted,
            "double" => Self::Double,
            "groove" => Self::Groove,
            "ridge" => Self::Ridge,
            "inset" => Self::Inset,
            "outset" => Self::Outset,
            _ => return None,
        })
    }

    /// Whether the side is drawn at all (`none` and `hidden` aren't, and take no space).
    pub fn is_visible(self) -> bool {
        !matches!(self, Self::None | Self::Hidden)
    }

    /// The style's number in the quad shader.
    pub fn index(self) -> u32 {
        self as u32
    }
}

/// `mix-blend-mode` / `background-blend-mode`: how colours combine with what's
/// beneath them (the W3C compositing modes, blended in sRGB).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    const NAMES: [&str; 16] = [
        "normal",
        "multiply",
        "screen",
        "overlay",
        "darken",
        "lighten",
        "color-dodge",
        "color-burn",
        "hard-light",
        "soft-light",
        "difference",
        "exclusion",
        "hue",
        "saturation",
        "color",
        "luminosity",
    ];
    const ALL: [BlendMode; 16] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Darken,
        Self::Lighten,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::HardLight,
        Self::SoftLight,
        Self::Difference,
        Self::Exclusion,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        let i = Self::NAMES.iter().position(|n| *n == s.trim())?;
        Some(Self::ALL[i])
    }

    /// The mode's number in the composite shader (`normal` is 0).
    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0) as u32
    }
}

/// `mask-mode`: what of the mask image counts. `match-source` is alpha for images
/// and gradients (Platypus has no SVG `<mask>` elements).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaskMode {
    #[default]
    Alpha,
    Luminance,
}

impl MaskMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "alpha" | "match-source" => Self::Alpha,
            "luminance" => Self::Luminance,
            _ => return None,
        })
    }
}

/// A comma-separated list of background layer values (`a, b, c`), top layer first.
pub fn parse_layers<T>(s: &str, item: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    split_top_level(s, ',').into_iter().map(item).collect()
}

/// Splits on `sep` outside parentheses, trimming parts and dropping empty ones
/// (so `' '` also collapses repeated spaces).
pub(crate) fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

/// `overflow` on one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    /// Like `hidden`, but can't be scrolled even by scripts.
    Clip,
    /// Scrollable, with a scrollbar always shown.
    Scroll,
    /// Scrollable, with a scrollbar when the content overflows.
    Auto,
}

impl Overflow {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "visible" => Overflow::Visible,
            "hidden" => Overflow::Hidden,
            "clip" => Overflow::Clip,
            "scroll" => Overflow::Scroll,
            "auto" | "overlay" => Overflow::Auto,
            _ => return None,
        })
    }

    /// `overflow` with one or two values (x then y).
    pub fn parse_pair(s: &str) -> Option<[Self; 2]> {
        match s.split_whitespace().collect::<Vec<_>>()[..] {
            [a] => Overflow::parse(a).map(|o| [o, o]),
            [a, b] => Some([Overflow::parse(a)?, Overflow::parse(b)?]),
            _ => None,
        }
    }

    /// Computed values: when one axis scrolls or hides, `visible` on the other
    /// becomes `auto` and `clip` becomes `hidden` (CSS Overflow 3).
    pub fn computed(pair: [Self; 2]) -> [Self; 2] {
        let boxy = |o: Overflow| matches!(o, Overflow::Hidden | Overflow::Scroll | Overflow::Auto);
        if !boxy(pair[0]) && !boxy(pair[1]) {
            return pair;
        }
        pair.map(|o| match o {
            Overflow::Visible => Overflow::Auto,
            Overflow::Clip => Overflow::Hidden,
            o => o,
        })
    }

    /// Whether content is clipped on this axis.
    pub fn clips(self) -> bool {
        self != Overflow::Visible
    }

    /// Whether the user can scroll this axis.
    pub fn scrolls(self) -> bool {
        matches!(self, Overflow::Scroll | Overflow::Auto)
    }
}

/// The box a `clipPath` shape is measured against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GeometryBox {
    #[default]
    Border,
    Padding,
    Content,
}

/// A circle or ellipse radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeRadius {
    Len(Len),
    ClosestSide,
    FarthestSide,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipShapeSpec {
    /// `inset(top right bottom left round radii)`.
    Inset {
        top: Len,
        right: Len,
        bottom: Len,
        left: Len,
        radii: Corners<CornerRadius>,
    },
    /// `circle(r at x y)`.
    Circle { r: ShapeRadius, at: (Len, Len) },
    /// `ellipse(rx ry at x y)`.
    Ellipse {
        rx: ShapeRadius,
        ry: ShapeRadius,
        at: (Len, Len),
    },
    /// The reference box itself (`clipPath: padding-box`), with the element's corners.
    Box,
    /// `polygon([nonzero | evenodd,] x y, ...)`, relative to the reference box.
    Polygon {
        even_odd: bool,
        points: Vec<(Len, Len)>,
    },
    /// `path([nonzero | evenodd,] "M ...")`: SVG path data in px from the reference
    /// box's corner, flattened to contours when parsed.
    Path {
        even_odd: bool,
        contours: Arc<Vec<Vec<(f32, f32)>>>,
    },
}

/// `clipPath`: a basic shape and/or a reference box. `url()` (SVG `<clipPath>`
/// elements) isn't supported.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPath {
    pub shape: ClipShapeSpec,
    pub reference: GeometryBox,
}

impl ClipPath {
    /// Parses `inset(10px round 8px)`, `circle(40% at 50% 50%)`,
    /// `ellipse(closest-side farthest-side at left top)`, optionally with a box
    /// keyword (`border-box`, `padding-box`, `content-box`), or a box keyword alone.
    pub fn parse(s: &str) -> Option<Self> {
        let mut shape = None;
        let mut reference = None;
        let s = s.trim();
        let mut rest = s;
        while !rest.is_empty() {
            let end = match rest.find('(') {
                Some(open) if !rest[..open].contains(' ') => open + rest[open..].find(')')? + 1,
                _ => rest.find(' ').unwrap_or(rest.len()),
            };
            let token = rest[..end].trim();
            rest = rest[end..].trim_start();
            let geometry = match token {
                "border-box" | "margin-box" => Some(GeometryBox::Border),
                "padding-box" => Some(GeometryBox::Padding),
                "content-box" => Some(GeometryBox::Content),
                _ => None,
            };
            if let Some(g) = geometry {
                if reference.replace(g).is_some() {
                    return None;
                }
            } else if shape.replace(parse_shape(token)?).is_some() {
                return None;
            }
        }
        if shape.is_none() && reference.is_none() {
            return None;
        }
        Some(ClipPath {
            shape: shape.unwrap_or(ClipShapeSpec::Box),
            reference: reference.unwrap_or_default(),
        })
    }

    /// The clip for an element whose border box is `border`, given its border and
    /// padding widths (top, right, bottom, left) and resolved corner radii.
    pub fn resolve(
        &self,
        border: crate::Rect,
        borders: [f32; 4],
        padding: [f32; 4],
        radii: Radii,
    ) -> crate::ClipShape {
        let inset = |r: crate::Rect, w: [f32; 4]| {
            crate::Rect::new(
                r.x + w[3],
                r.y + w[0],
                (r.width - w[1] - w[3]).max(0.0),
                (r.height - w[0] - w[2]).max(0.0),
            )
        };
        let reference = match self.reference {
            GeometryBox::Border => border,
            GeometryBox::Padding => inset(border, borders),
            GeometryBox::Content => inset(inset(border, borders), padding),
        };
        let (w, h) = (reference.width, reference.height);
        let center =
            |at: (Len, Len)| (reference.x + at.0.resolve(w), reference.y + at.1.resolve(h));
        match &self.shape {
            ClipShapeSpec::Box => {
                let shrink = match self.reference {
                    GeometryBox::Border => [0.0; 4],
                    GeometryBox::Padding => borders,
                    GeometryBox::Content => [0, 1, 2, 3].map(|i| borders[i] + padding[i]),
                };
                inner_corners(reference, radii, shrink)
            }
            ClipShapeSpec::Inset {
                top,
                right,
                bottom,
                left,
                radii,
            } => {
                let rect = crate::Rect::new(
                    reference.x + left.resolve(w),
                    reference.y + top.resolve(h),
                    (w - left.resolve(w) - right.resolve(w)).max(0.0),
                    (h - top.resolve(h) - bottom.resolve(h)).max(0.0),
                );
                crate::ClipShape::rounded(rect, resolve_radii(radii, rect.width, rect.height))
            }
            ClipShapeSpec::Circle { r, at } => {
                let (cx, cy) = center(*at);
                let sides = [
                    cx - reference.x,
                    reference.x + w - cx,
                    cy - reference.y,
                    reference.y + h - cy,
                ];
                let r = match r {
                    // Percentages are of the box's normalized diagonal.
                    ShapeRadius::Len(l) => {
                        l.resolve((w * w + h * h).sqrt() / std::f32::consts::SQRT_2)
                    }
                    ShapeRadius::ClosestSide => sides.iter().fold(f32::MAX, |a, b| a.min(b.abs())),
                    ShapeRadius::FarthestSide => sides.iter().fold(0.0_f32, |a, b| a.max(b.abs())),
                };
                ellipse(cx, cy, r, r)
            }
            ClipShapeSpec::Ellipse { rx, ry, at } => {
                let (cx, cy) = center(*at);
                let axis = |r: ShapeRadius, lo: f32, hi: f32, size: f32| match r {
                    ShapeRadius::Len(l) => l.resolve(size),
                    ShapeRadius::ClosestSide => lo.min(hi),
                    ShapeRadius::FarthestSide => lo.max(hi),
                };
                let rx = axis(
                    *rx,
                    (cx - reference.x).abs(),
                    (reference.x + w - cx).abs(),
                    w,
                );
                let ry = axis(
                    *ry,
                    (cy - reference.y).abs(),
                    (reference.y + h - cy).abs(),
                    h,
                );
                ellipse(cx, cy, rx, ry)
            }
            ClipShapeSpec::Polygon { even_odd, points } => {
                let contour = points
                    .iter()
                    .map(|(x, y)| (reference.x + x.resolve(w), reference.y + y.resolve(h)))
                    .collect();
                crate::ClipShape::polygon(crate::Polygon {
                    contours: vec![contour],
                    even_odd: *even_odd,
                })
            }
            ClipShapeSpec::Path { even_odd, contours } => {
                let contours = contours
                    .iter()
                    .map(|c| {
                        c.iter()
                            .map(|(x, y)| (reference.x + x, reference.y + y))
                            .collect()
                    })
                    .collect();
                crate::ClipShape::polygon(crate::Polygon {
                    contours,
                    even_odd: *even_odd,
                })
            }
        }
    }
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> crate::ClipShape {
    let (rx, ry) = (rx.max(0.0), ry.max(0.0));
    crate::ClipShape {
        rect: crate::Rect::new(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry),
        radii_x: [rx; 4],
        radii_y: [ry; 4],
        polygon: None,
    }
}

/// Splits an optional leading `nonzero` / `evenodd` off shape arguments.
fn fill_rule(args: &str) -> (bool, &str) {
    let parts = split_top_level(args, ',');
    match parts.first().map(|p| p.trim()) {
        Some("evenodd") => (true, args.split_once(',').map_or("", |(_, rest)| rest)),
        Some("nonzero") => (false, args.split_once(',').map_or("", |(_, rest)| rest)),
        _ => (false, args),
    }
}

/// `rect` (already shrunk by `shrink`: top, right, bottom, left) with the outer
/// `radii` reduced by the shrink on each side, as CSS does for inner edges: the
/// horizontal radius loses the left/right width, the vertical one the top/bottom.
pub fn inner_corners(rect: crate::Rect, radii: Radii, shrink: [f32; 4]) -> crate::ClipShape {
    crate::ClipShape::rounded(rect, radii.inner(shrink))
}

fn parse_shape(token: &str) -> Option<ClipShapeSpec> {
    let (name, args) = token.strip_suffix(')')?.split_once('(')?;
    let args = args.trim();
    match name.trim() {
        "inset" => {
            let (lengths, round) = match args.split_once(" round ") {
                Some((l, r)) => (l, Some(r)),
                None => (args, None),
            };
            let v: Vec<Len> = lengths
                .split_whitespace()
                .map(Len::parse)
                .collect::<Option<_>>()?;
            let (top, right, bottom, left) = match v[..] {
                [a] => (a, a, a, a),
                [a, b] => (a, b, a, b),
                [a, b, c] => (a, b, c, b),
                [a, b, c, d] => (a, b, c, d),
                _ => return None,
            };
            let radii = match round {
                Some(r) => parse_radii(r)?,
                None => Corners::all(CornerRadius::ZERO),
            };
            Some(ClipShapeSpec::Inset {
                top,
                right,
                bottom,
                left,
                radii,
            })
        }
        "circle" | "ellipse" => {
            let tokens: Vec<&str> = args.split_whitespace().collect();
            let (radii, at) = match tokens.iter().position(|t| *t == "at") {
                Some(i) => (&tokens[..i], Some(tokens[i + 1..].join(" "))),
                None => (&tokens[..], None),
            };
            let at = match at {
                Some(a) => {
                    let p = parse_background_position(&a)?;
                    (p.x, p.y)
                }
                None => (Len::Percent(0.5), Len::Percent(0.5)),
            };
            let radius = |t: &str| match t {
                "closest-side" => Some(ShapeRadius::ClosestSide),
                "farthest-side" => Some(ShapeRadius::FarthestSide),
                t => Len::parse(t).map(ShapeRadius::Len),
            };
            let r: Vec<ShapeRadius> = radii.iter().map(|t| radius(t)).collect::<Option<_>>()?;
            if name.trim() == "circle" {
                let r = match r[..] {
                    [] => ShapeRadius::ClosestSide,
                    [r] => r,
                    _ => return None,
                };
                Some(ClipShapeSpec::Circle { r, at })
            } else {
                let (rx, ry) = match r[..] {
                    [] => (ShapeRadius::ClosestSide, ShapeRadius::ClosestSide),
                    [rx, ry] => (rx, ry),
                    _ => return None,
                };
                Some(ClipShapeSpec::Ellipse { rx, ry, at })
            }
        }
        "polygon" => {
            let (even_odd, rest) = fill_rule(args);
            let points: Vec<(Len, Len)> = split_top_level(rest, ',')
                .into_iter()
                .map(|pair| match split_top_level(pair, ' ')[..] {
                    [x, y] => Some((Len::parse(x)?, Len::parse(y)?)),
                    _ => None,
                })
                .collect::<Option<_>>()?;
            (points.len() >= 3).then_some(ClipShapeSpec::Polygon { even_odd, points })
        }
        "path" => {
            let (even_odd, rest) = fill_rule(args);
            let d = rest.trim();
            let d = d
                .strip_prefix('"')
                .and_then(|d| d.strip_suffix('"'))
                .or_else(|| d.strip_prefix('\'')?.strip_suffix('\''))?;
            let contours = crate::svg_path::flatten(d)?;
            (!contours.is_empty()).then(|| ClipShapeSpec::Path {
                even_odd,
                contours: Arc::new(contours),
            })
        }
        _ => None,
    }
}

/// Legacy `clip: rect(top, right, bottom, left)` (commas optional, `auto` for an
/// edge of the border box). Offsets are from the border box's top-left corner.
pub fn parse_clip_rect(s: &str) -> Option<[Option<f32>; 4]> {
    let inner = s.trim().strip_prefix("rect(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    let [t, r, b, l] = parts[..] else {
        return None;
    };
    let edge = |p: &str| -> Option<Option<f32>> {
        if p == "auto" {
            return Some(None);
        }
        match Len::parse(p)? {
            Len::Px(px) => Some(Some(px)),
            Len::Percent(_) => None,
        }
    };
    Some([edge(t)?, edge(r)?, edge(b)?, edge(l)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    // M2-VIS-03
    #[test]
    fn radii_follow_css_corner_order() {
        let x = |s: &str| parse_radii(s).unwrap().to_array().map(|c| c.x);
        let px = |v: [f32; 4]| v.map(Len::Px);
        assert_eq!(x("1px 2px 3px 4px"), px([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(x("1px 2px"), px([1.0, 2.0, 1.0, 2.0]));
        assert_eq!(x("1px 2px 3px"), px([1.0, 2.0, 3.0, 2.0]));
        assert_eq!(parse_radii("-1px"), None);
        assert_eq!(parse_radii("1px 2px 3px 4px 5px"), None);
    }

    // M3-BRD-02
    #[test]
    fn elliptical_radii_parse_after_a_slash() {
        let r = parse_radii("10px 20px / 5px").unwrap().to_array();
        assert_eq!(r.map(|c| c.x), [10.0, 20.0, 10.0, 20.0].map(Len::Px));
        assert_eq!(r.map(|c| c.y), [Len::Px(5.0); 4]);
        assert_eq!(
            CornerRadius::parse("10px 50%"),
            Some(CornerRadius {
                x: Len::Px(10.0),
                y: Len::Percent(0.5)
            })
        );
        for bad in ["10px /", "/ 5px", "1px / 2px / 3px", "1px 2px 3px"] {
            assert!(parse_radii(bad).is_none() || bad == "1px 2px 3px", "{bad}");
        }
        assert_eq!(CornerRadius::parse("1px 2px 3px"), None);
    }

    // M2-VIS-03, M3-BRD-02
    #[test]
    fn radii_resolve_and_scale_like_css() {
        let circle = parse_radii("50%").unwrap();
        assert_eq!(
            resolve_radii(&circle, 40.0, 40.0),
            Radii::circular([20.0; 4])
        );
        // On a wide box, 50% is an ellipse: half the width across, half the height down.
        let ellipse = resolve_radii(&circle, 100.0, 40.0);
        assert_eq!((ellipse.x, ellipse.y), ([50.0; 4], [20.0; 4]));
        // 30px corners on a 40px-tall box overlap vertically: scaled by 40/60.
        let big = parse_radii("30px").unwrap();
        let r = resolve_radii(&big, 100.0, 40.0);
        assert!(
            r.x.iter().chain(&r.y).all(|v| (v - 20.0).abs() < 1e-4),
            "{r:?}"
        );
    }

    // M2-VIS-04
    #[test]
    fn parses_shadow_lists() {
        let s = parse_shadows("0 4px 12px rgba(0,0,0,.4), inset 0 0 0 1px #fff").unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[0].offset_x, s[0].offset_y, s[0].blur, s[0].spread),
            (0.0, 4.0, 12.0, 0.0)
        );
        assert!(!s[0].inset);
        assert!((s[0].color.a - 0.4).abs() < 1e-6);
        assert!(s[1].inset);
        assert_eq!(s[1].spread, 1.0);
        assert_eq!(s[1].color, Rgba::new(1.0, 1.0, 1.0, 1.0));

        let s = parse_shadows("red 2px 2px").unwrap();
        assert_eq!(s[0].color, Rgba::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(parse_shadows("none").unwrap(), []);
        for bad in [
            "2px",
            "1px 2px 3px 4px 5px",
            "1px 2px -3px",
            "1px 2px red blue",
            "1px 2px, ",
        ] {
            if bad == "1px 2px, " {
                // A trailing comma just drops the empty entry.
                assert_eq!(parse_shadows(bad).unwrap().len(), 1);
                continue;
            }
            assert_eq!(parse_shadows(bad), None, "{bad:?}");
        }
    }

    // M2-VIS-05
    #[test]
    fn parses_blur_filters() {
        let blur = |s: &str| Filter::parse(s).map(|f| f.blur);
        assert_eq!(blur("blur(4px)"), Some(4.0));
        assert_eq!(blur("blur(0)"), Some(0.0));
        assert_eq!(blur("none"), Some(0.0));
        assert_eq!(blur("blur(-1px)"), None);
        assert_eq!(blur("blur(10%)"), None);
        assert_eq!(
            blur("blur(3px) blur(4px)"),
            Some(5.0),
            "deviations add in quadrature"
        );
    }

    // M3-FX-02
    #[test]
    fn filter_chains_combine_colour_functions() {
        let apply = |s: &str, c: [f32; 4]| {
            let m = Filter::parse(s).unwrap().matrix.unwrap();
            [0, 1, 2, 3].map(|r| (0..4).map(|k| m[r * 5 + k] * c[k]).sum::<f32>() + m[r * 5 + 4])
        };
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
        let red = [1.0, 0.0, 0.0, 1.0];
        assert!(
            close(apply("grayscale(1)", red), [0.2126, 0.2126, 0.2126, 1.0]) || {
                let g = apply("grayscale(1)", red);
                (g[0] - 0.213).abs() < 1e-3 && g[0] == g[1] && g[1] == g[2]
            }
        );
        assert!(close(apply("brightness(50%)", red), [0.5, 0.0, 0.0, 1.0]));
        assert!(
            close(apply("invert()", red), [0.0, 1.0, 1.0, 1.0]),
            "amount defaults to 1"
        );
        assert!(close(apply("opacity(0.25)", red), [1.0, 0.0, 0.0, 0.25]));
        // Applied in order: brightness then invert differs from invert then brightness.
        assert!(close(
            apply("brightness(0.5) invert(1)", red),
            [0.5, 1.0, 1.0, 1.0]
        ));
        assert!(close(
            apply("invert(1) brightness(0.5)", red),
            [0.0, 0.5, 0.5, 1.0]
        ));
        let h = apply("hue-rotate(360deg)", red);
        assert!(close(h, red), "a full turn is identity: {h:?}");
        let f = Filter::parse("blur(2px) drop-shadow(3px 4px 6px red) contrast(1.2)").unwrap();
        assert_eq!(f.blur, 2.0);
        assert_eq!(f.drop_shadows.len(), 1);
        assert_eq!(
            (f.drop_shadows[0].offset_x, f.drop_shadows[0].blur),
            (3.0, 6.0)
        );
        assert!(f.matrix.is_some());
        assert!(Filter::parse("blur(2px)").unwrap().matrix.is_none());
        for bad in [
            "grayscale(-1)",
            "hue-rotate(10)",
            "frobnicate(1)",
            "drop-shadow(1px 1px 2px 3px red)",
            "drop-shadow(inset 1px 1px red)",
            "blur(2px",
        ] {
            assert_eq!(Filter::parse(bad), None, "{bad}");
        }
    }

    // M3-BRD-04
    #[test]
    fn border_image_shorthand_and_longhands_parse() {
        let b = BorderImage::parse("url(frame.png) 10 20% fill / 2 4px / 1 round stretch").unwrap();
        assert_eq!(*b.source.unwrap(), BackgroundImage::Url("frame.png".into()));
        assert_eq!(
            b.slice,
            [
                Len::Px(10.0),
                Len::Percent(0.2),
                Len::Px(10.0),
                Len::Percent(0.2)
            ]
        );
        assert!(b.fill);
        assert_eq!(
            b.width,
            [
                BorderImageLength::Number(2.0),
                BorderImageLength::Px(4.0),
                BorderImageLength::Number(2.0),
                BorderImageLength::Px(4.0)
            ]
        );
        assert_eq!(b.outset, [BorderImageLength::Number(1.0); 4]);
        assert_eq!(
            b.repeat,
            [BorderImageRepeat::Round, BorderImageRepeat::Stretch]
        );

        let g = BorderImage::parse("linear-gradient(red, blue) 1").unwrap();
        assert!(matches!(
            g.source.as_deref(),
            Some(BackgroundImage::Linear { .. })
        ));
        assert_eq!(g.slice, [Len::Px(1.0); 4]);
        assert_eq!(g.repeat, [BorderImageRepeat::Stretch; 2]);
        assert_eq!(BorderImage::parse("none"), Some(BorderImage::default()));
        for bad in [
            "url(a.png) / 2",
            "url(a.png) 10 / 2 / 3 / 4",
            "url(a.png) url(b.png) 1",
            "url(a.png) 1 sideways",
            "url(a.png) -1",
        ] {
            assert_eq!(BorderImage::parse(bad), None, "{bad}");
        }
        assert_eq!(
            BorderImage::parse_lengths("auto", true),
            None,
            "no auto outset"
        );
    }

    // M3-BRD-04
    #[test]
    fn border_image_pieces_map_slices_to_the_border() {
        let b = BorderImage::parse("url(a.png) 10 fill").unwrap();
        let area = crate::Rect::new(0.0, 0.0, 100.0, 60.0);
        let pieces = b.pieces(area, [5.0, 5.0, 5.0, 5.0], (30.0, 30.0));
        assert_eq!(pieces.len(), 9, "fill keeps the middle");
        let top_left = pieces[0];
        assert_eq!(top_left.src, crate::Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(top_left.dst, crate::Rect::new(0.0, 0.0, 5.0, 5.0));
        let top = pieces[1];
        assert_eq!(top.src, crate::Rect::new(10.0, 0.0, 10.0, 10.0));
        assert_eq!(top.dst, crate::Rect::new(5.0, 0.0, 90.0, 5.0));
        let middle = pieces[4];
        assert_eq!(middle.dst, crate::Rect::new(5.0, 5.0, 90.0, 50.0));

        // Repeating edges tile at the slice scaled to the border width (10px → 5px).
        let r = BorderImage::parse("url(a.png) 10 round").unwrap();
        let pieces = r.pieces(area, [5.0; 4], (30.0, 30.0));
        assert_eq!(pieces.len(), 8);
        assert_eq!(pieces[1].tile, (5.0, 5.0));
        assert_eq!(
            pieces[1].repeat,
            [BorderImageRepeat::Round, BorderImageRepeat::Stretch]
        );

        // Widths that don't fit are scaled down together.
        let wide = BorderImage::parse("url(a.png) 10 / 40px").unwrap();
        let pieces = wide.pieces(area, [5.0; 4], (30.0, 30.0));
        assert_eq!(pieces[0].dst.height, 30.0, "40+40 > 60: scaled by 60/80");
        // The outset grows the area.
        let out = BorderImage::parse("url(a.png) 10 / 1 / 2").unwrap();
        assert_eq!(
            out.area(area, [5.0; 4]),
            crate::Rect::new(-10.0, -10.0, 120.0, 80.0)
        );
    }

    // M3-BG-03
    #[test]
    fn blend_modes_parse_to_shader_indices() {
        assert_eq!(BlendMode::parse("normal").map(BlendMode::index), Some(0));
        assert_eq!(BlendMode::parse("multiply"), Some(BlendMode::Multiply));
        assert_eq!(
            BlendMode::parse(" color-dodge ").map(BlendMode::index),
            Some(6)
        );
        assert_eq!(
            BlendMode::parse("luminosity").map(BlendMode::index),
            Some(15)
        );
        assert_eq!(BlendMode::parse("plus-darker"), None);
    }

    // M3-BG-01
    #[test]
    fn parses_conic_and_repeating_gradients() {
        let Some(BackgroundImage::Conic {
            from,
            at,
            stops,
            repeating,
        }) = parse_background_image(
            "conic-gradient(from 0.25turn at 10px 20%, red, blue 90deg, lime 50% 75%)",
        )
        .unwrap()
        else {
            panic!("expected a conic gradient");
        };
        assert_eq!(from, 90.0);
        assert_eq!((at.x, at.y), (Len::Px(10.0), Len::Percent(0.2)));
        assert!(!repeating);
        let positions: Vec<_> = stops.iter().map(|s| s.position).collect();
        assert_eq!(
            positions,
            vec![
                None,
                Some(Len::Percent(0.25)),
                Some(Len::Percent(0.5)),
                Some(Len::Percent(0.75))
            ],
            "angles are fractions of a turn; two positions make two stops"
        );

        let Some(BackgroundImage::Conic { at, .. }) =
            parse_background_image("conic-gradient(red, blue)").unwrap()
        else {
            panic!()
        };
        assert_eq!(
            (at.x, at.y),
            (Len::Percent(0.5), Len::Percent(0.5)),
            "centred"
        );
        assert!(matches!(
            parse_background_image("repeating-linear-gradient(red 0 10px, blue 10px 20px)"),
            Some(Some(BackgroundImage::Linear { repeating: true, ref stops, .. })) if stops.len() == 4
        ));
        assert!(matches!(
            parse_background_image("repeating-radial-gradient(circle, red, blue 10%)"),
            Some(Some(BackgroundImage::Radial {
                repeating: true,
                ..
            }))
        ));
        assert!(matches!(
            parse_background_image("repeating-conic-gradient(red 0 10deg, blue 10deg 20deg)"),
            Some(Some(BackgroundImage::Conic {
                repeating: true,
                ..
            }))
        ));
        for bad in [
            "conic-gradient(from 10px, red, blue)",
            "conic-gradient(red 10px, blue)",
            "conic-gradient(red)",
            "repeating-url(a.png)",
            "linear-gradient(red 1px 2px 3px, blue)",
        ] {
            assert_eq!(parse_background_image(bad), None, "{bad}");
        }
    }

    // M2-VIS-06
    #[test]
    fn parses_gradients_and_urls() {
        assert_eq!(
            parse_background_image("url('ui/panel.png')").unwrap(),
            Some(BackgroundImage::Url("ui/panel.png".into()))
        );
        let Some(BackgroundImage::Linear {
            direction, stops, ..
        }) = parse_background_image("linear-gradient(to right, red, rgba(0, 0, 255, 0.5) 75%)")
            .unwrap()
        else {
            panic!("expected a linear gradient");
        };
        assert_eq!(direction, GradientDirection::Angle(90.0));
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[1].position, Some(Len::Percent(0.75)));

        let Some(BackgroundImage::Linear { direction, .. }) =
            parse_background_image("linear-gradient(45deg, red, blue)").unwrap()
        else {
            panic!()
        };
        assert_eq!(direction, GradientDirection::Angle(45.0));
        let Some(BackgroundImage::Linear { direction, .. }) =
            parse_background_image("linear-gradient(red, blue)").unwrap()
        else {
            panic!()
        };
        assert_eq!(
            direction,
            GradientDirection::Angle(180.0),
            "default is to bottom"
        );
        let Some(BackgroundImage::Linear { direction, .. }) =
            parse_background_image("linear-gradient(to top left, red, blue)").unwrap()
        else {
            panic!()
        };
        assert_eq!(
            direction,
            GradientDirection::Corner {
                right: false,
                bottom: false
            }
        );

        let Some(BackgroundImage::Radial { shape, stops, .. }) =
            parse_background_image("radial-gradient(circle, white 0%, black 100%)").unwrap()
        else {
            panic!()
        };
        assert_eq!(shape, RadialShape::Circle);
        assert_eq!(stops.len(), 2);
        // `circle at 100% 0` places the centre; a bare `at` keeps the default shape.
        let Some(BackgroundImage::Radial {
            shape, at, stops, ..
        }) = parse_background_image("radial-gradient(circle at 100% 0, red 0px, blue 6.5px)")
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(shape, RadialShape::Circle);
        assert_eq!(at.x, Len::Percent(1.0));
        assert_eq!(at.y, Len::Px(0.0));
        assert_eq!(stops.len(), 2, "the pixel stops are stops, not the header");
        assert!(matches!(
            parse_background_image("radial-gradient(at 5px 5px, red, blue)"),
            Some(Some(BackgroundImage::Radial { .. }))
        ));
        assert_eq!(
            parse_background_image("radial-gradient(red at 5px 5px, blue)"),
            None
        );

        assert_eq!(parse_background_image("none"), Some(None));
        for bad in [
            "linear-gradient(red)",
            "url()",
            "conic-gradient(red)",
            "linear-gradient(to middle, red, blue)",
        ] {
            assert_eq!(parse_background_image(bad), None, "{bad:?}");
        }
    }

    // M2-VIS-06
    #[test]
    fn parses_background_size_position_repeat() {
        assert_eq!(parse_background_size("cover"), Some(BackgroundSize::Cover));
        assert_eq!(
            parse_background_size("50% auto"),
            Some(BackgroundSize::Explicit(Some(Len::Percent(0.5)), None))
        );
        assert_eq!(
            parse_background_size("32px"),
            Some(BackgroundSize::Explicit(Some(Len::Px(32.0)), None))
        );

        let pos = |s| parse_background_position(s).unwrap();
        assert_eq!(
            pos("center"),
            BackgroundPosition {
                x: Len::Percent(0.5),
                y: Len::Percent(0.5)
            }
        );
        assert_eq!(
            pos("top"),
            BackgroundPosition {
                x: Len::Percent(0.5),
                y: Len::Percent(0.0)
            }
        );
        assert_eq!(
            pos("right"),
            BackgroundPosition {
                x: Len::Percent(1.0),
                y: Len::Percent(0.5)
            }
        );
        assert_eq!(
            pos("top left"),
            BackgroundPosition {
                x: Len::Percent(0.0),
                y: Len::Percent(0.0)
            }
        );
        assert_eq!(
            pos("left bottom"),
            BackgroundPosition {
                x: Len::Percent(0.0),
                y: Len::Percent(1.0)
            }
        );
        assert_eq!(
            pos("10px 25%"),
            BackgroundPosition {
                x: Len::Px(10.0),
                y: Len::Percent(0.25)
            }
        );
        assert_eq!(parse_background_position("middle"), None);

        assert_eq!(
            parse_background_repeat("no-repeat"),
            Some(BackgroundRepeat::NO_REPEAT)
        );
        assert_eq!(
            parse_background_repeat("repeat-x"),
            Some(BackgroundRepeat {
                x: RepeatAxis::Repeat,
                y: RepeatAxis::NoRepeat
            })
        );
        assert_eq!(
            parse_background_repeat("round space"),
            Some(BackgroundRepeat {
                x: RepeatAxis::Round,
                y: RepeatAxis::Space
            })
        );
        assert_eq!(parse_background_repeat("sideways"), None);
        assert_eq!(parse_background_repeat("repeat-x repeat"), None);
    }

    // M3d step 1: angle units and modern colour functions in gradients.
    #[test]
    fn gradients_take_any_angle_unit_and_colour_function() {
        for (angle, deg) in [
            ("0.25turn", 90.0),
            ("100grad", 90.0),
            ("3.14159265rad", 180.0),
        ] {
            let Some(BackgroundImage::Linear { direction, .. }) =
                parse_background_image(&format!("linear-gradient({angle}, red, blue)")).unwrap()
            else {
                panic!("{angle}");
            };
            let GradientDirection::Angle(a) = direction else {
                panic!("{angle}");
            };
            assert!((a - deg).abs() < 1e-3, "{angle}: {a}");
        }
        let Some(BackgroundImage::Linear { stops, .. }) = parse_background_image(
            "linear-gradient(oklch(0.7 0.1 200) 10%, color-mix(in srgb, red 25%, blue))",
        )
        .unwrap() else {
            panic!("expected a linear gradient");
        };
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].position, Some(Len::Percent(0.1)));
    }
}
