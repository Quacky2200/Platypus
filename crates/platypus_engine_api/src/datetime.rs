// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! C#-style `TimeSpan` and `DateTime` (as in s&box), as `js/engine/src/time.ts` has them: whole
//! milliseconds, months 1 to 12, local time through the OS zone (or [`set_zone`]) with the same
//! daylight-saving rules as JavaScript's `Date` (a time in a gap moves forward, in an overlap it
//! is the earlier one).
//!
//! `new TimeSpan(ms)` and `new DateTime(year, month, ...)` call the static `create`, so they round
//! and normalise as in TypeScript. Differences: parse failures log an error and give an invalid
//! value (NaN) instead of throwing, and `DateTime.parse` knows the ISO 8601 forms only.

use jiff::{Timestamp, tz::TimeZone};
use std::cell::RefCell;
use ts_api::{ValueType, ts_api};

const MS_PER_SECOND: f64 = 1000.0;
const MS_PER_MINUTE: f64 = 60_000.0;
const MS_PER_HOUR: f64 = 3_600_000.0;
const MS_PER_DAY: f64 = 86_400_000.0;

/// A length of time in whole milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub ms: f64,
}

impl ValueType<1> for Span {
    fn from_parts(p: [f64; 1]) -> Self {
        Span { ms: p[0] }
    }

    fn to_parts(self) -> [f64; 1] {
        [self.ms]
    }
}

/// A point in time: milliseconds since the Unix epoch (UTC) and whether it is shown as UTC (1) or
/// local time (0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dt {
    pub ms: f64,
    pub utc: f64,
}

impl ValueType<2> for Dt {
    fn from_parts(p: [f64; 2]) -> Self {
        Dt {
            ms: p[0],
            utc: p[1],
        }
    }

    fn to_parts(self) -> [f64; 2] {
        [self.ms, self.utc]
    }
}

thread_local! {
    static ZONE: RefCell<Option<TimeZone>> = const { RefCell::new(None) };
}

/// Sets the zone local times use (an IANA name), instead of the OS zone. `None` goes back to it.
pub fn set_zone(name: Option<&str>) -> Result<(), String> {
    let tz = match name {
        Some(n) => Some(TimeZone::get(n).map_err(|e| e.to_string())?),
        None => None,
    };
    ZONE.with(|z| *z.borrow_mut() = tz);
    Ok(())
}

fn local_zone() -> TimeZone {
    ZONE.with(|z| z.borrow_mut().get_or_insert_with(TimeZone::system).clone())
}

fn zone_for(utc: f64) -> TimeZone {
    if utc != 0.0 {
        TimeZone::UTC
    } else {
        local_zone()
    }
}

/// `Math.round`: ties go up.
fn js_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let r = x.floor();
    if x - r >= 0.5 { r + 1.0 } else { r }
}

/// JavaScript's `ToIntegerOrInfinity`; NaN stays NaN here (callers check).
fn trunc(x: f64) -> f64 {
    x.trunc()
}

fn span(ms: f64) -> Span {
    Span { ms: js_round(ms) }
}

fn invalid_span(what: &str, text: &str) -> Span {
    crate::log::emit(
        crate::log::Level::Error,
        &format!("not a {what}: \"{text}\""),
    );
    Span { ms: f64::NAN }
}

// ---- formatting helpers shared by both ----

/// `String(n)` of a whole number.
fn int(n: f64) -> String {
    if n == 0.0 { "0".into() } else { format!("{n}") }
}

/// `String(n).padStart(w, "0")`.
fn pad(n: f64, w: usize) -> String {
    let s = int(n);
    if s.len() >= w {
        s
    } else {
        format!("{}{s}", "0".repeat(w - s.len()))
    }
}

/// Fraction-of-a-second digits from milliseconds: `f` keeps trailing zeros, `F` drops them.
fn fraction(ms: f64, digits: usize, trim: bool) -> String {
    let mut s = pad(ms, 3);
    while s.len() < digits {
        s.push('0');
    }
    s.truncate(digits);
    if trim {
        s = s.trim_end_matches('0').to_string();
    }
    s
}

/// Runs a C#-style custom format: runs of the same letter are tokens handed to `render` (which
/// returns `None` for letters it does not know, printed as they are); `'...'` / `"..."` are literals
/// and `\c` escapes one character.
fn format_tokens(format: &str, mut render: impl FnMut(&str) -> Option<String>) -> String {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '\'' || c == '"' {
            let end = chars[i + 1..]
                .iter()
                .position(|x| *x == c)
                .map(|p| p + i + 1);
            let stop = end.unwrap_or(chars.len());
            out.extend(&chars[i + 1..stop]);
            i = stop + 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && chars[j] == c {
            j += 1;
        }
        let token: String = chars[i..j].iter().collect();
        let rendered = if c.is_ascii_alphabetic() {
            render(&token)
        } else {
            None
        };
        out.push_str(&rendered.unwrap_or(token));
        i = j;
    }
    out
}

// ---- TimeSpan ----

fn span_part(ms: f64, unit: f64, modulo: f64) -> f64 {
    (ms / unit).trunc() % modulo
}

fn span_to_string(ms: f64, format: &str) -> String {
    let abs = ms.abs();
    let days = (abs / MS_PER_DAY).trunc();
    let hours = span_part(abs, MS_PER_HOUR, 24.0);
    let minutes = span_part(abs, MS_PER_MINUTE, 60.0);
    let seconds = span_part(abs, MS_PER_SECOND, 60.0);
    let millis = abs % 1000.0;
    if format.is_empty() || format == "c" {
        let sign = if ms < 0.0 { "-" } else { "" };
        let day = if days != 0.0 {
            format!("{}.", int(days))
        } else {
            String::new()
        };
        let frac = if millis != 0.0 {
            format!(".{}", pad(millis, 3))
        } else {
            String::new()
        };
        return format!(
            "{sign}{day}{}:{}:{}{frac}",
            pad(hours, 2),
            pad(minutes, 2),
            pad(seconds, 2)
        );
    }
    format_tokens(format, |token| {
        let c = token.chars().next()?;
        let n = token.len();
        match c {
            'd' => Some(pad(days, n)),
            'h' => Some(pad(hours, n.min(2))),
            'm' => Some(pad(minutes, n.min(2))),
            's' => Some(pad(seconds, n.min(2))),
            'f' | 'F' => Some(fraction(millis, n, c == 'F')),
            _ => None,
        }
    })
}

/// `[-][d.]hh:mm[:ss[.fff]]`, C#'s constant format. `None` when it does not match.
fn parse_span(text: &str) -> Option<f64> {
    let t = text.trim_matches(char::is_whitespace);
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let digits = |s: &str, min: usize, max: usize| -> Option<(f64, usize)> {
        let n = s.bytes().take_while(u8::is_ascii_digit).count().min(max);
        (n >= min).then(|| (s[..n].parse::<f64>().unwrap_or(0.0), n))
    };
    // Days: digits and a dot, tried first; without them if that does not fit.
    let attempt = |body: &str, days: f64| -> Option<f64> {
        let (h, n) = digits(body, 1, 2)?;
        let body = body[n..].strip_prefix(':')?;
        let (m, n) = digits(body, 1, 2)?;
        let mut body = &body[n..];
        let (mut s, mut frac) = (0.0, 0.0);
        if let Some(b) = body.strip_prefix(':') {
            let (sv, n) = digits(b, 1, 2)?;
            s = sv;
            body = &b[n..];
            if let Some(f) = body.strip_prefix('.') {
                let n = f.bytes().take_while(u8::is_ascii_digit).count();
                if !(1..=7).contains(&n) {
                    return None;
                }
                // Fractions finer than a millisecond are dropped.
                let mut d: String = f[..n].to_string();
                while d.len() < 3 {
                    d.push('0');
                }
                frac = d[..3].parse::<f64>().unwrap_or(0.0);
                body = &f[n..];
            }
        }
        if !body.is_empty() || h > 23.0 || m > 59.0 || s > 59.0 {
            return None;
        }
        Some(days * MS_PER_DAY + h * MS_PER_HOUR + m * MS_PER_MINUTE + s * MS_PER_SECOND + frac)
    };
    let with_days = rest.bytes().take_while(u8::is_ascii_digit).count();
    let ms = if with_days > 0 && rest[with_days..].starts_with('.') {
        let d = rest[..with_days].parse::<f64>().unwrap_or(0.0);
        attempt(&rest[with_days + 1..], d).or_else(|| attempt(rest, 0.0))
    } else {
        attempt(rest, 0.0)
    }?;
    Some(if neg { -ms } else { ms })
}

#[ts_api(namespace = "TimeSpan", values(TimeSpan = Span [totalMilliseconds: f64]))]
pub mod time_span_api {
    use super::{
        MS_PER_DAY, MS_PER_HOUR, MS_PER_MINUTE, MS_PER_SECOND, Span, invalid_span, parse_span,
        span, span_part, span_to_string,
    };

    /// `new TimeSpan(milliseconds)`: whole milliseconds (rounded).
    pub fn create(milliseconds: f64) -> Span {
        span(milliseconds)
    }

    #[property]
    pub fn zero() -> Span {
        Span { ms: 0.0 }
    }

    /// From days and, optionally, hours, minutes, seconds and milliseconds.
    #[optional(hours, minutes, seconds, milliseconds)]
    pub fn of(days: f64, hours: f64, minutes: f64, seconds: f64, milliseconds: f64) -> Span {
        let n = |x: f64| if x.is_nan() { 0.0 } else { x };
        span(
            days * MS_PER_DAY
                + n(hours) * MS_PER_HOUR
                + n(minutes) * MS_PER_MINUTE
                + n(seconds) * MS_PER_SECOND
                + n(milliseconds),
        )
    }

    pub fn from_days(d: f64) -> Span {
        span(d * MS_PER_DAY)
    }

    pub fn from_hours(h: f64) -> Span {
        span(h * MS_PER_HOUR)
    }

    pub fn from_minutes(m: f64) -> Span {
        span(m * MS_PER_MINUTE)
    }

    pub fn from_seconds(s: f64) -> Span {
        span(s * MS_PER_SECOND)
    }

    pub fn from_milliseconds(ms: f64) -> Span {
        span(ms)
    }

    /// Parses `[-][d.]hh:mm[:ss[.fff]]` (C#'s constant format). Anything else logs an error and
    /// gives an invalid (NaN) span.
    pub fn parse(text: &str) -> Span {
        match parse_span(text) {
            Some(ms) => Span { ms },
            None => invalid_span("TimeSpan", text),
        }
    }

    /// Whole days.
    #[getter(TimeSpan)]
    pub fn days(t: Span) -> f64 {
        (t.ms / MS_PER_DAY).trunc()
    }

    #[getter(TimeSpan)]
    pub fn hours(t: Span) -> f64 {
        span_part(t.ms, MS_PER_HOUR, 24.0)
    }

    #[getter(TimeSpan)]
    pub fn minutes(t: Span) -> f64 {
        span_part(t.ms, MS_PER_MINUTE, 60.0)
    }

    #[getter(TimeSpan)]
    pub fn seconds(t: Span) -> f64 {
        span_part(t.ms, MS_PER_SECOND, 60.0)
    }

    #[getter(TimeSpan)]
    pub fn milliseconds(t: Span) -> f64 {
        t.ms % 1000.0
    }

    #[getter(TimeSpan)]
    pub fn total_days(t: Span) -> f64 {
        t.ms / MS_PER_DAY
    }

    #[getter(TimeSpan)]
    pub fn total_hours(t: Span) -> f64 {
        t.ms / MS_PER_HOUR
    }

    #[getter(TimeSpan)]
    pub fn total_minutes(t: Span) -> f64 {
        t.ms / MS_PER_MINUTE
    }

    #[getter(TimeSpan)]
    pub fn total_seconds(t: Span) -> f64 {
        t.ms / MS_PER_SECOND
    }

    #[method(TimeSpan)]
    pub fn add(t: Span, other: Span) -> Span {
        span(t.ms + other.ms)
    }

    #[method(TimeSpan)]
    pub fn subtract(t: Span, other: Span) -> Span {
        span(t.ms - other.ms)
    }

    #[method(TimeSpan)]
    pub fn multiply(t: Span, factor: f64) -> Span {
        span(t.ms * factor)
    }

    #[method(TimeSpan)]
    pub fn divide(t: Span, divisor: f64) -> Span {
        span(t.ms / divisor)
    }

    #[method(TimeSpan)]
    pub fn negate(t: Span) -> Span {
        span(-t.ms)
    }

    /// The absolute length.
    #[method(TimeSpan)]
    pub fn duration(t: Span) -> Span {
        span(t.ms.abs())
    }

    #[method(TimeSpan)]
    pub fn compare_to(t: Span, other: Span) -> f64 {
        let d = t.ms - other.ms;
        if d > 0.0 {
            1.0
        } else if d < 0.0 {
            -1.0
        } else if d.is_nan() {
            f64::NAN
        } else {
            0.0
        }
    }

    #[method(TimeSpan)]
    pub fn equals(t: Span, other: Span) -> bool {
        t.ms == other.ms
    }

    /// C#'s constant format, `[-][d.]hh:mm:ss[.fff]` (milliseconds only when non-zero), or a
    /// custom format with `d`, `h`/`hh`, `m`/`mm`, `s`/`ss`, `f`..`fffffff`, `'literal'` and `\c`
    /// (a custom format does not print the sign, as in C#).
    #[method(TimeSpan)]
    #[optional(format)]
    pub fn to_string(t: Span, format: &str) -> String {
        span_to_string(t.ms, format)
    }
}

// ---- DateTime ----

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Days from 1970-01-01 to a civil date (proleptic Gregorian); `month` 1 to 12.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn is_leap(year: f64) -> bool {
    (year % 4.0 == 0.0 && year % 100.0 != 0.0) || year % 400.0 == 0.0
}

fn days_in_month(year: f64, month: f64) -> f64 {
    let m = month as usize;
    if !(1..=12).contains(&m) {
        return f64::NAN;
    }
    f64::from(
        [
            31,
            if is_leap(year) { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ][m - 1],
    )
}

/// The epoch milliseconds of calendar fields (`new Date(y, m - 1, ...)`): out-of-range fields
/// carry into the next (month 13 is January of the next year); local times in a gap move forward.
/// NaN when a field is not a number or the date is out of range.
#[allow(clippy::too_many_arguments)]
fn from_fields(y: f64, mo: f64, d: f64, h: f64, mi: f64, s: f64, ms: f64, utc: bool) -> f64 {
    let all = [y, mo, d, h, mi, s, ms];
    if all.iter().any(|x| !x.is_finite()) {
        return f64::NAN;
    }
    let [y, mo, d, h, mi, s, ms] = all.map(trunc);
    let month0 = mo - 1.0;
    let ym = y + (month0 / 12.0).floor();
    let mn = month0.rem_euclid(12.0);
    if ym.abs() > 400_000.0 {
        return f64::NAN;
    }
    let days = days_from_civil(ym as i64, mn as i64 + 1, 1) as f64 + d - 1.0;
    let local = days * MS_PER_DAY + h * MS_PER_HOUR + mi * MS_PER_MINUTE + s * MS_PER_SECOND + ms;
    if utc || !local.is_finite() {
        return local;
    }
    // Local time: read the same wall clock in the zone.
    let Ok(wall) = Timestamp::from_millisecond(local as i64) else {
        return f64::NAN;
    };
    let civil = wall.to_zoned(TimeZone::UTC).datetime();
    match civil.to_zoned(local_zone()) {
        Ok(z) => z.timestamp().as_millisecond() as f64,
        Err(_) => f64::NAN,
    }
}

/// Calendar fields of an instant in its kind.
struct Parts {
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    millisecond: f64,
    /// 0 (Sunday) to 6.
    weekday: f64,
    /// Minutes east of UTC.
    offset_minutes: f64,
}

fn parts(t: Dt) -> Option<Parts> {
    let ts = Timestamp::from_millisecond(t.ms as i64).ok()?;
    let z = ts.to_zoned(zone_for(t.utc));
    Some(Parts {
        year: f64::from(z.year()),
        month: f64::from(z.month()),
        day: f64::from(z.day()),
        hour: f64::from(z.hour()),
        minute: f64::from(z.minute()),
        second: f64::from(z.second()),
        millisecond: f64::from(z.millisecond()),
        weekday: f64::from(z.weekday().to_sunday_zero_offset()),
        offset_minutes: f64::from(z.offset().seconds()) / 60.0,
    })
}

fn day_of_year(p: &Parts) -> f64 {
    let mut n = p.day;
    let mut m = 1.0;
    while m < p.month {
        n += days_in_month(p.year, m);
        m += 1.0;
    }
    n
}

fn from_parts(p: &Parts, utc: f64) -> Dt {
    Dt {
        ms: from_fields(
            p.year,
            p.month,
            p.day,
            p.hour,
            p.minute,
            p.second,
            p.millisecond,
            utc != 0.0,
        ),
        utc,
    }
}

fn kind_flag(kind: &str) -> f64 {
    f64::from(u8::from(kind == "utc"))
}

fn dt_to_string(t: Dt, format: &str) -> String {
    let format = if format.is_empty() { "G" } else { format };
    if format == "u" {
        let u = Dt { ms: t.ms, utc: 1.0 };
        return dt_to_string(u, "yyyy'-'MM'-'dd HH':'mm':'ss'Z'");
    }
    let standard = |c: &str| -> Option<&'static str> {
        Some(match c {
            "d" => "MM/dd/yyyy",
            "D" => "dddd, dd MMMM yyyy",
            "t" => "HH:mm",
            "T" => "HH:mm:ss",
            "g" => "MM/dd/yyyy HH:mm",
            "G" => "MM/dd/yyyy HH:mm:ss",
            "f" => "dddd, dd MMMM yyyy HH:mm",
            "F" => "dddd, dd MMMM yyyy HH:mm:ss",
            "s" => "yyyy'-'MM'-'dd'T'HH':'mm':'ss",
            "o" | "O" => "yyyy'-'MM'-'dd'T'HH':'mm':'ss'.'fffK",
            _ => return None,
        })
    };
    let pattern = if format.chars().count() == 1 {
        standard(format).unwrap_or(format)
    } else {
        format
    };
    let Some(p) = parts(t) else {
        return "Invalid".to_string();
    };
    let offset = || {
        let m = p.offset_minutes;
        let sign = if m < 0.0 { "-" } else { "+" };
        let a = m.abs();
        (sign, (a / 60.0).trunc(), a % 60.0)
    };
    format_tokens(pattern, |token| {
        let c = token.chars().next()?;
        let n = token.len();
        match c {
            'y' => Some(match n {
                1 => int(p.year % 100.0),
                2 => pad(p.year % 100.0, 2),
                _ => pad(p.year, n),
            }),
            'M' => Some(match n {
                n if n >= 4 => MONTHS[p.month as usize - 1].to_string(),
                3 => MONTHS[p.month as usize - 1][..3].to_string(),
                _ => pad(p.month, n),
            }),
            'd' => Some(match n {
                n if n >= 4 => DAYS[p.weekday as usize].to_string(),
                3 => DAYS[p.weekday as usize][..3].to_string(),
                _ => pad(p.day, n),
            }),
            'H' => Some(pad(p.hour, n.min(2))),
            'h' => {
                let h = p.hour % 12.0;
                Some(pad(if h == 0.0 { 12.0 } else { h }, n.min(2)))
            }
            'm' => Some(pad(p.minute, n.min(2))),
            's' => Some(pad(p.second, n.min(2))),
            'f' | 'F' => Some(fraction(p.millisecond, n, c == 'F')),
            't' => {
                let s = if p.hour < 12.0 { "AM" } else { "PM" };
                Some(s[..if n == 1 { 1 } else { 2 }].to_string())
            }
            'z' => {
                let (sign, h, m) = offset();
                Some(match n {
                    1 => format!("{sign}{}", int(h)),
                    2 => format!("{sign}{}", pad(h, 2)),
                    _ => format!("{sign}{}:{}", pad(h, 2), pad(m, 2)),
                })
            }
            'K' => {
                if t.utc != 0.0 {
                    return Some("Z".into());
                }
                let (sign, h, m) = offset();
                Some(format!("{sign}{}:{}", pad(h, 2), pad(m, 2)))
            }
            _ => None,
        }
    })
}

/// The ISO 8601 forms `Date.parse` accepts: `YYYY`, `YYYY-MM`, `YYYY-MM-DD` (UTC), and with
/// `THH:mm[:ss[.fff]]` (local) and `Z` or `+HH:MM` (that instant). A space may replace the `T`.
/// Returns epoch milliseconds, NaN when the text is not one of them.
fn parse_iso(text: &str) -> f64 {
    let b = text.as_bytes();
    let mut i = 0;
    let num = |i: &mut usize, n: usize| -> Option<f64> {
        let s = b.get(*i..*i + n)?;
        if !s.iter().all(u8::is_ascii_digit) {
            return None;
        }
        *i += n;
        std::str::from_utf8(s).ok()?.parse().ok()
    };
    let mut go = || -> Option<f64> {
        let mut year_sign = 1.0;
        let year = if matches!(b.first(), Some(b'+' | b'-')) {
            year_sign = if b[0] == b'-' { -1.0 } else { 1.0 };
            i = 1;
            num(&mut i, 6)? * year_sign
        } else {
            num(&mut i, 4)?
        };
        let _ = year_sign;
        let mut month = 1.0;
        let mut day = 1.0;
        if b.get(i) == Some(&b'-') {
            i += 1;
            month = num(&mut i, 2)?;
            if b.get(i) == Some(&b'-') {
                i += 1;
                day = num(&mut i, 2)?;
            }
        }
        if !(1.0..=12.0).contains(&month) || day < 1.0 || day > days_in_month(year, month) {
            return None;
        }
        if i == b.len() {
            // A date alone is UTC.
            return Some(from_fields(year, month, day, 0.0, 0.0, 0.0, 0.0, true));
        }
        if !matches!(b.get(i), Some(b'T' | b't' | b' ')) {
            return None;
        }
        i += 1;
        let hour = num(&mut i, 2)?;
        if b.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        let minute = num(&mut i, 2)?;
        let (mut second, mut ms) = (0.0, 0.0);
        if b.get(i) == Some(&b':') {
            i += 1;
            second = num(&mut i, 2)?;
            if b.get(i) == Some(&b'.') {
                i += 1;
                let start = i;
                while b.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
                if i == start {
                    return None;
                }
                let mut digits = std::str::from_utf8(&b[start..i]).ok()?.to_string();
                while digits.len() < 3 {
                    digits.push('0');
                }
                ms = digits[..3].parse().ok()?;
            }
        }
        // 24:00:00 is the end of the day.
        if hour > 24.0
            || minute > 59.0
            || second > 59.0
            || (hour == 24.0 && (minute + second + ms) > 0.0)
        {
            return None;
        }
        let local = || from_fields(year, month, day, hour, minute, second, ms, false);
        let utc = || from_fields(year, month, day, hour, minute, second, ms, true);
        match b.get(i) {
            None => Some(local()),
            Some(b'Z' | b'z') if i + 1 == b.len() => Some(utc()),
            Some(sign @ (b'+' | b'-')) => {
                i += 1;
                let oh = num(&mut i, 2)?;
                if b.get(i) == Some(&b':') {
                    i += 1;
                }
                let om = num(&mut i, 2)?;
                if i != b.len() || oh > 23.0 || om > 59.0 {
                    return None;
                }
                let off = (oh * 60.0 + om) * MS_PER_MINUTE * if *sign == b'-' { -1.0 } else { 1.0 };
                Some(utc() - off)
            }
            _ => None,
        }
    };
    go().unwrap_or(f64::NAN)
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

/// The same wall clock fields in another kind, going through calendar fields.
fn add_days(t: Dt, days: f64) -> Dt {
    let days = trunc(days);
    if t.utc != 0.0 {
        return Dt {
            ms: t.ms + days * MS_PER_DAY,
            utc: t.utc,
        };
    }
    let Some(mut p) = parts(t) else {
        return Dt {
            ms: f64::NAN,
            utc: t.utc,
        };
    };
    p.day += days;
    from_parts(&p, t.utc)
}

fn add_months(t: Dt, months: f64) -> Dt {
    let Some(p) = parts(t) else {
        return Dt {
            ms: f64::NAN,
            utc: t.utc,
        };
    };
    let total = p.year * 12.0 + (p.month - 1.0) + trunc(months);
    let year = (total / 12.0).floor();
    let month = total.rem_euclid(12.0) + 1.0;
    let day = p.day.min(days_in_month(year, month));
    Dt {
        ms: from_fields(
            year,
            month,
            day,
            p.hour,
            p.minute,
            p.second,
            p.millisecond,
            t.utc != 0.0,
        ),
        utc: t.utc,
    }
}

#[ts_api(namespace = "DateTime", values(
    DateTime = Dt [epochMilliseconds: f64, utc: f64],
    TimeSpan = Span [totalMilliseconds: f64]))]
pub mod date_time_api {
    use super::{
        Dt, MS_PER_HOUR, MS_PER_MINUTE, MS_PER_SECOND, Span, add_days, add_months, day_of_year,
        days_in_month, dt_to_string, from_fields, from_parts, is_leap, js_round, kind_flag, now_ms,
        parse_iso, parts, span,
    };

    fn opt(x: f64, default: f64) -> f64 {
        if x.is_nan() { default } else { x }
    }

    /// `new DateTime(year, month?, day?, hour?, minute?, second?, millisecond?, kind?)`: calendar
    /// fields (`month` 1 to 12) in local time, or in UTC when `kind` is `"utc"`. Fields out of
    /// range carry over; an impossible date gives an invalid (NaN) value.
    #[optional(month, day, hour, minute, second, millisecond, kind)]
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        year: f64,
        month: f64,
        day: f64,
        hour: f64,
        minute: f64,
        second: f64,
        millisecond: f64,
        kind: &str,
    ) -> Dt {
        let utc = kind == "utc";
        Dt {
            ms: from_fields(
                year,
                opt(month, 1.0),
                opt(day, 1.0),
                opt(hour, 0.0),
                opt(minute, 0.0),
                opt(second, 0.0),
                opt(millisecond, 0.0),
                utc,
            ),
            utc: kind_flag(kind),
        }
    }

    /// From epoch milliseconds.
    #[optional(kind)]
    pub fn from_epoch(ms: f64, kind: &str) -> Dt {
        Dt {
            ms: js_round(ms),
            utc: kind_flag(kind),
        }
    }

    /// The current local time, from the system clock (milliseconds).
    #[property]
    pub fn now() -> Dt {
        Dt {
            ms: now_ms(),
            utc: 0.0,
        }
    }

    #[property]
    pub fn utc_now() -> Dt {
        Dt {
            ms: now_ms(),
            utc: 1.0,
        }
    }

    /// Midnight today, local time.
    #[property]
    pub fn today() -> Dt {
        let now = Dt {
            ms: now_ms(),
            utc: 0.0,
        };
        date(now)
    }

    /// Parses an ISO 8601 date or date and time. A `Z` or offset makes it UTC; a date alone is
    /// UTC midnight (as `Date.parse` has it). Anything else logs an error and gives an invalid
    /// (NaN) value.
    pub fn parse(text: &str) -> Dt {
        let t = text.trim();
        let ms = parse_iso(text);
        if ms.is_nan() {
            crate::log::emit(
                crate::log::Level::Error,
                &format!("not a DateTime: \"{text}\""),
            );
        }
        let ends_with_offset = {
            let lower = t.to_ascii_lowercase();
            let b = lower.as_bytes();
            let n = b.len();
            lower.ends_with('z')
                || (n >= 5 && {
                    // [+-]dd:?dd at the end
                    let tail = &b[n - 5..];
                    let colon = &b[n.saturating_sub(6)..];
                    let a = (tail[0] == b'+' || tail[0] == b'-')
                        && tail[1..].iter().all(u8::is_ascii_digit);
                    let c = n >= 6
                        && (colon[0] == b'+' || colon[0] == b'-')
                        && colon[1].is_ascii_digit()
                        && colon[2].is_ascii_digit()
                        && colon[3] == b':'
                        && colon[4].is_ascii_digit()
                        && colon[5].is_ascii_digit();
                    a || c
                })
        };
        Dt {
            ms,
            utc: f64::from(u8::from(ends_with_offset)),
        }
    }

    pub fn is_leap_year(year: f64) -> bool {
        is_leap(year)
    }

    #[js_name = "daysInMonth"]
    pub fn days_in_month_of(year: f64, month: f64) -> f64 {
        days_in_month(year, month)
    }

    /// `"local"` or `"utc"`.
    #[getter(DateTime)]
    pub fn kind(t: Dt) -> String {
        if t.utc != 0.0 { "utc" } else { "local" }.to_string()
    }

    #[getter(DateTime)]
    pub fn year(t: Dt) -> f64 {
        parts(t).map_or(f64::NAN, |p| p.year)
    }

    #[getter(DateTime)]
    pub fn month(t: Dt) -> f64 {
        parts(t).map_or(f64::NAN, |p| p.month)
    }

    #[getter(DateTime)]
    pub fn day(t: Dt) -> f64 {
        parts(t).map_or(f64::NAN, |p| p.day)
    }

    #[getter(DateTime)]
    pub fn hour(t: Dt) -> f64 {
        parts(t).map_or(f64::NAN, |p| p.hour)
    }

    #[getter(DateTime)]
    pub fn minute(t: Dt) -> f64 {
        parts(t).map_or(f64::NAN, |p| p.minute)
    }

    #[getter(DateTime)]
    pub fn second(t: Dt) -> f64 {
        parts(t).map_or(f64::NAN, |p| p.second)
    }

    #[getter(DateTime)]
    pub fn millisecond(t: Dt) -> f64 {
        parts(t).map_or(f64::NAN, |p| p.millisecond)
    }

    /// 0 (Sunday) to 6 (Saturday), like C#'s `DayOfWeek`.
    #[getter(DateTime)]
    pub fn day_of_week(t: Dt) -> f64 {
        parts(t).map_or(f64::NAN, |p| p.weekday)
    }

    /// 1 to 366.
    #[getter(DateTime)]
    #[js_name = "dayOfYear"]
    pub fn day_of_year_of(t: Dt) -> f64 {
        parts(t).map_or(f64::NAN, |p| day_of_year(&p))
    }

    /// The same day at midnight.
    #[getter(DateTime)]
    pub fn date(t: Dt) -> Dt {
        match parts(t) {
            Some(mut p) => {
                p.hour = 0.0;
                p.minute = 0.0;
                p.second = 0.0;
                p.millisecond = 0.0;
                from_parts(&p, t.utc)
            }
            None => Dt {
                ms: f64::NAN,
                utc: t.utc,
            },
        }
    }

    /// Time since midnight.
    #[getter(DateTime)]
    pub fn time_of_day(t: Dt) -> Span {
        match parts(t) {
            Some(p) => span(
                p.hour * MS_PER_HOUR
                    + p.minute * MS_PER_MINUTE
                    + p.second * MS_PER_SECOND
                    + p.millisecond,
            ),
            None => Span { ms: f64::NAN },
        }
    }

    #[method(DateTime)]
    pub fn add(t: Dt, s: Span) -> Dt {
        Dt {
            ms: t.ms + s.ms,
            utc: t.utc,
        }
    }

    #[method(DateTime)]
    pub fn add_milliseconds(t: Dt, ms: f64) -> Dt {
        Dt {
            ms: t.ms + span(ms).ms,
            utc: t.utc,
        }
    }

    #[method(DateTime)]
    pub fn add_seconds(t: Dt, s: f64) -> Dt {
        Dt {
            ms: t.ms + span(s * MS_PER_SECOND).ms,
            utc: t.utc,
        }
    }

    #[method(DateTime)]
    pub fn add_minutes(t: Dt, m: f64) -> Dt {
        Dt {
            ms: t.ms + span(m * MS_PER_MINUTE).ms,
            utc: t.utc,
        }
    }

    #[method(DateTime)]
    pub fn add_hours(t: Dt, h: f64) -> Dt {
        Dt {
            ms: t.ms + span(h * MS_PER_HOUR).ms,
            utc: t.utc,
        }
    }

    /// Calendar days (across a daylight-saving change, local times keep their clock time).
    #[method(DateTime)]
    #[js_name = "addDays"]
    pub fn add_days_to(t: Dt, days: f64) -> Dt {
        add_days(t, days)
    }

    /// Calendar months; the day is clamped to the month's length, as in C#.
    #[method(DateTime)]
    #[js_name = "addMonths"]
    pub fn add_months_to(t: Dt, months: f64) -> Dt {
        add_months(t, months)
    }

    #[method(DateTime)]
    pub fn add_years(t: Dt, years: f64) -> Dt {
        add_months(t, years * 12.0)
    }

    /// The time between two dates (`a.subtract(b)` is `a - b`).
    #[method(DateTime)]
    pub fn subtract(t: Dt, other: Dt) -> Span {
        span(t.ms - other.ms)
    }

    /// This date minus a span.
    #[method(DateTime)]
    pub fn subtract_time_span(t: Dt, s: Span) -> Dt {
        Dt {
            ms: t.ms - s.ms,
            utc: t.utc,
        }
    }

    #[method(DateTime)]
    pub fn to_universal_time(t: Dt) -> Dt {
        Dt { ms: t.ms, utc: 1.0 }
    }

    #[method(DateTime)]
    pub fn to_local_time(t: Dt) -> Dt {
        Dt { ms: t.ms, utc: 0.0 }
    }

    #[method(DateTime)]
    pub fn compare_to(t: Dt, other: Dt) -> f64 {
        let d = t.ms - other.ms;
        if d > 0.0 {
            1.0
        } else if d < 0.0 {
            -1.0
        } else if d.is_nan() {
            f64::NAN
        } else {
            0.0
        }
    }

    /// The same instant (kinds may differ).
    #[method(DateTime)]
    pub fn equals(t: Dt, other: Dt) -> bool {
        t.ms == other.ms
    }

    /// ISO 8601 with milliseconds and offset (C#'s `"o"`, with 3 fraction digits).
    #[method(DateTime)]
    #[js_name = "toISOString"]
    pub fn to_iso_string(t: Dt) -> String {
        dt_to_string(t, "o")
    }

    /// Formats like C#'s `DateTime.ToString` with the invariant culture.
    /// Standard: `o`/`O` round trip, `s` sortable, `u` universal sortable, `d` short date, `D` long
    /// date, `t`/`T` short/long time, `g`/`G` general (the default), `f`/`F` full.
    /// Custom: `yyyy yy y MMMM MMM MM M dddd ddd dd d HH H hh h mm m ss s fff ff f FFF tt t zzz zz
    /// z K`, `'literal'` and `\c`. Digits after the third `f` print as zeros.
    #[method(DateTime)]
    #[optional(format)]
    pub fn to_string(t: Dt, format: &str) -> String {
        dt_to_string(t, format)
    }
}
