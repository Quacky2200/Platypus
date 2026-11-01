// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Constraint validation of text fields, the part behind `:valid`, `:invalid`, `:required` and
//! `:optional`. A pure function of a field's constraint attributes and its text, after the
//! HTML spec's "constraint validation" for `<input>` and `<textarea>`.
//!
//! `pattern` is a `regex-lite` expression, anchored like the spec's `^(?:...)$`: the common
//! JavaScript syntax works (classes, groups, alternation, repeats), but look-around and
//! back-references do not, and a pattern that does not compile is ignored, as browsers do.

use std::sync::OnceLock;

use regex_lite::Regex;

/// Which constraints a field's text breaks (the DOM's `ValidityState`, less `customError`,
/// `tooLong` and `valueMissing` for a checkbox).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Validity {
    /// `required` and nothing in it.
    pub value_missing: bool,
    /// An `email` or `url` field with text that is not one.
    pub type_mismatch: bool,
    /// Text that does not match `pattern`.
    pub pattern_mismatch: bool,
    /// Shorter than `minLength`.
    pub too_short: bool,
    /// A `number` field with text that is not a number.
    pub bad_input: bool,
    /// Below `min`.
    pub range_underflow: bool,
    /// Above `max`.
    pub range_overflow: bool,
    /// Not on the `step` grid that starts at `min` (or 0).
    pub step_mismatch: bool,
}

impl Validity {
    /// Whether no constraint is broken.
    pub fn valid(&self) -> bool {
        *self == Self::default()
    }
}

/// What a field's attributes ask of its text.
#[derive(Clone, Copy, Debug, Default)]
pub struct Constraints<'a> {
    /// `type` of an `<input>` (lower-cased); `None` for a `<textarea>` or no type.
    pub input_type: Option<&'a str>,
    /// A `<textarea>` has no `pattern`.
    pub textarea: bool,
    pub required: bool,
    pub pattern: Option<&'a str>,
    pub min_length: Option<usize>,
    pub min: Option<&'a str>,
    pub max: Option<&'a str>,
    pub step: Option<&'a str>,
}

/// Checks `value` against `c`. Empty text breaks only `required`.
pub fn check(c: &Constraints, value: &str) -> Validity {
    let mut v = Validity::default();
    if value.is_empty() {
        v.value_missing = c.required;
        return v;
    }
    match c.input_type {
        Some("number") => number(c, value, &mut v),
        Some("email") => v.type_mismatch = !email().is_match(value),
        Some("url") => v.type_mismatch = !url().is_match(value),
        _ => {}
    }
    if c.input_type != Some("number") {
        if let Some(min) = c.min_length {
            v.too_short = value.chars().count() < min;
        }
        if let Some(pattern) = c.pattern.filter(|_| !c.textarea) {
            // Anchored as a whole, like the spec's `^(?:pattern)$`.
            if let Ok(re) = Regex::new(&format!("^(?:{pattern})$")) {
                v.pattern_mismatch = !re.is_match(value);
            }
        }
    }
    v
}

fn number(c: &Constraints, value: &str, v: &mut Validity) {
    let text = value.trim();
    // `parse` would also take `inf` and `nan`.
    let numeric = text
        .chars()
        .all(|ch| ch.is_ascii_digit() || "+-.eE".contains(ch));
    let Some(n) = numeric
        .then(|| text.parse::<f64>().ok())
        .flatten()
        .filter(|n| n.is_finite())
    else {
        v.bad_input = true;
        return;
    };
    let parse = |s: Option<&str>| s.and_then(|s| s.trim().parse::<f64>().ok());
    let min = parse(c.min);
    if let Some(min) = min {
        v.range_underflow = n < min;
    }
    if let Some(max) = parse(c.max) {
        v.range_overflow = n > max;
    }
    if !c.step.is_some_and(|s| s.trim().eq_ignore_ascii_case("any")) {
        let step = parse(c.step).filter(|s| *s > 0.0).unwrap_or(1.0);
        let cells = (n - min.unwrap_or(0.0)) / step;
        v.step_mismatch = (cells - cells.round()).abs() > 1e-9 * cells.abs().max(1.0);
    }
}

/// The spec's "valid e-mail address" expression.
fn email() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$",
        )
        .expect("the e-mail expression compiles")
    })
}

/// An absolute URL: a scheme, a colon and something after it, no spaces.
fn url() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*:\S+$").expect("the URL expression compiles")
    })
}
