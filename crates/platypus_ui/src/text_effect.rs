//! Per-glyph text effects (`text-effect`, `text-reveal`): the typewriter look of text
//! adventures without one animated element per character.
//!
//! `text-reveal: N` shows the first N glyphs of a text element; `text-effect: wavy rainbow`
//! (names, space separated, they combine) moves, scales, fades and colours each glyph. Entrances
//! and one-shot animations run from the moment the renderer first sees a glyph revealed, for as
//! many seconds as the original text adventure's CSS gave them, whatever the typing does next
//! (a pause at a comma doesn't hold them). Always-on effects use the renderer's clock. Glyphs
//! revealed all at once (skipping ahead, or `text-reveal` left at `all`) start settled.

/// The original typed a glyph every 25 ms: always-on waves and sweeps trail a stride per glyph.
const GLYPHS_PER_SECOND: f32 = 40.0;

macro_rules! effects {
    ($($bit:literal $name:literal),* $(,)?) => {
        const NAMES: &[(&str, u32)] = &[$(($name, 1 << $bit)),*];
    };
}
effects! {
    0 "fade", 1 "default", 2 "drop", 3 "zoom-in", 4 "zoom-out", 5 "slide-left", 6 "slide-right",
    7 "slide-up", 8 "slide-down", 9 "blink", 10 "bounce", 11 "pixar-bounce", 12 "hop", 13 "rope",
    14 "wavy", 15 "pulse", 16 "haunted", 17 "faulty", 18 "rainbow", 19 "shake-x", 20 "shake-y",
    21 "swing", 22 "insanity", 23 "cursor",
}
const FADE: u32 = 1 << 0;
const DEFAULT: u32 = 1 << 1;
const DROP: u32 = 1 << 2;
const ZOOM_IN: u32 = 1 << 3;
const ZOOM_OUT: u32 = 1 << 4;
const SLIDE_LEFT: u32 = 1 << 5;
const SLIDE_RIGHT: u32 = 1 << 6;
const SLIDE_UP: u32 = 1 << 7;
const SLIDE_DOWN: u32 = 1 << 8;
const BLINK: u32 = 1 << 9;
const BOUNCE: u32 = 1 << 10;
const PIXAR: u32 = 1 << 11;
const HOP: u32 = 1 << 12;
const ROPE: u32 = 1 << 13;
const WAVY: u32 = 1 << 14;
const PULSE: u32 = 1 << 15;
const HAUNTED: u32 = 1 << 16;
const FAULTY: u32 = 1 << 17;
const RAINBOW: u32 = 1 << 18;
const SHAKE_X: u32 = 1 << 19;
const SHAKE_Y: u32 = 1 << 20;
const SWING: u32 = 1 << 21;
const INSANITY: u32 = 1 << 22;
const CURSOR: u32 = 1 << 23;

/// A set of effect names, as bit flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextEffect(u32);

/// Effects that run on the renderer's clock after a glyph has appeared (so the app keeps
/// drawing); the rest are over within seconds of a glyph's reveal.
const AMBIENT: u32 = WAVY | FAULTY | RAINBOW | SHAKE_X | SHAKE_Y | SWING | INSANITY | BLINK;

impl TextEffect {
    pub const NONE: TextEffect = TextEffect(0);

    /// `none`, or names separated by spaces (`hue-rotate` is `rainbow`); `None` when one isn't
    /// known.
    pub fn parse(s: &str) -> Option<TextEffect> {
        let mut bits = 0;
        for word in s.split_whitespace() {
            bits |= match word {
                "none" => 0,
                "hue-rotate" => RAINBOW,
                _ => NAMES.iter().find(|(n, _)| *n == word)?.1,
            };
        }
        Some(TextEffect(bits))
    }

    /// Whether a block cursor is drawn where typing has got to (the original's `cursor-bar`).
    pub fn has_cursor(self) -> bool {
        self.0 & CURSOR != 0
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    /// Whether the look changes with the clock once glyphs are shown.
    pub fn is_ambient(self) -> bool {
        self.0 & AMBIENT != 0
    }

    /// The longest a one-shot effect takes after a glyph is revealed, in seconds.
    pub fn tail_seconds(self) -> f32 {
        let f = self.0;
        let mut t = 0.5f32;
        for (bit, secs) in [
            (PIXAR, 5.0),
            (HOP, 2.0),
            (PULSE, 3.0),
            (HAUNTED, 3.0),
            (BOUNCE, 1.0),
            (ROPE, 0.8),
        ] {
            if f & bit != 0 {
                t = t.max(secs);
            }
        }
        t
    }

    /// The effect on glyph `index` (0-based over the element's text), `age` seconds after it
    /// was revealed (`f32::INFINITY` for one that starts settled), at `time` seconds.
    pub fn eval(self, index: usize, age: f32, time: f32) -> GlyphFx {
        let mut fx = GlyphFx::default();
        let f = self.0;
        let i = index as f32;
        let s = age;
        let upto = |secs: f32| (s / secs).clamp(0.0, 1.0);

        if f & FADE != 0 {
            fx.alpha *= ease(upto(0.5));
        }
        if f & DEFAULT != 0 {
            fx.alpha *= ease(upto(0.5));
            let d = 1.25 * (1.0 - upto(0.2));
            fx.dx += d;
            fx.dy += d;
        }
        if f & DROP != 0 {
            // Falls an em with the original's overshoot (cubic-bezier .175 .885 .32 1.275).
            fx.alpha *= upto(0.15);
            fx.dy -= 1.0 - bezier(0.175, 0.885, 0.32, 1.275, upto(0.3));
        }
        if f & ZOOM_IN != 0 {
            fx.alpha *= upto(0.15);
            fx.scale *= upto(0.3).max(0.02);
        }
        if f & ZOOM_OUT != 0 {
            fx.alpha *= upto(0.15);
            fx.scale *= 2.0 - upto(0.25);
        }
        for (bit, dx, dy) in [
            (SLIDE_LEFT, 0.5, 0.0),
            (SLIDE_RIGHT, -0.5, 0.0),
            (SLIDE_UP, 0.0, 0.5),
            (SLIDE_DOWN, 0.0, -0.5),
        ] {
            if f & bit != 0 {
                fx.alpha *= upto(0.15);
                let left = 1.0 - upto(0.25);
                fx.dx += dx * left;
                fx.dy += dy * left;
            }
        }
        if f & BLINK != 0 {
            // A neon tube catching: on arrival each glyph stutters on and off at random (the
            // same way every time for a given glyph), more and more often on, and stays lit;
            // afterwards one now and then goes out for a moment, as a tired tube does. A lit
            // glyph is always fully lit: the tube is on or off, never dim.
            let p = upto(0.5);
            if p < 1.0 {
                let h = hash01(index as u32, (s * 30.0) as u32);
                if h >= 0.3 + 0.7 * ease(p) {
                    fx.alpha = 0.0;
                }
            } else {
                let h = hash01(index as u32 ^ 0x9e37, (time * 12.0) as u32);
                if h < 0.03 {
                    fx.alpha = 0.0;
                }
            }
        }
        if f & BOUNCE != 0 {
            // Falls from two ems and bounces down to rest, over a second (linear).
            fx.alpha *= upto(0.1);
            fx.dy += keys(
                &[
                    (0.0, -2.0),
                    (0.1, -1.5),
                    (0.2, -1.0),
                    (0.3, -0.5),
                    (0.4, 0.0),
                    (0.5, -0.25),
                    (0.6, 0.0),
                    (0.7, -0.15),
                    (0.8, 0.0),
                ],
                upto(1.0),
            );
        }
        if f & PIXAR != 0 {
            // Hops in from the right, squashing between hops, over five seconds (the original
            // also turns each glyph about its vertical axis, which isn't done here).
            fx.alpha *= upto(0.1);
            let p = upto(5.0);
            fx.dx += keys(
                &[
                    (0.0, 1.5),
                    (0.1, 1.25),
                    (0.2, 0.75),
                    (0.3, 0.25),
                    (0.35, 0.0),
                ],
                p,
            );
            fx.dy += keys(
                &[
                    (0.0, 0.5),
                    (0.1, -0.5),
                    (0.15, 0.5),
                    (0.2, -0.5),
                    (0.25, 0.5),
                    (0.3, -0.5),
                    (0.35, 0.5),
                    (0.4, 0.5),
                    (0.5, -0.5),
                    (0.55, 0.5),
                    (0.6, -0.5),
                    (0.65, 0.5),
                    (0.7, -0.5),
                    (0.75, 0.0),
                ],
                p,
            );
            let squash: Vec<(f32, f32)> = (0..=16)
                .map(|n| (n as f32 * 0.05, if n % 2 == 1 { 0.75 } else { 1.0 }))
                .collect();
            fx.scale *= keys(&squash, p);
        }
        if f & HOP != 0 {
            fx.alpha *= upto(0.1);
            let p = upto(2.0);
            fx.dx += keys(
                &[
                    (0.0, 1.5),
                    (0.1, 1.5),
                    (0.2, 1.25),
                    (0.3, 1.0),
                    (0.4, 0.75),
                    (0.5, 0.5),
                    (0.6, 0.25),
                    (0.7, 0.0),
                ],
                p,
            );
            fx.dy += keys(
                &[
                    (0.0, 0.0),
                    (0.2, -0.5),
                    (0.3, 0.0),
                    (0.4, -0.5),
                    (0.5, 0.0),
                    (0.6, -0.5),
                    (0.7, 0.0),
                ],
                p,
            );
            fx.scale *= keys(
                &[
                    (0.0, 1.0),
                    (0.1, 0.75),
                    (0.2, 1.0),
                    (0.3, 0.75),
                    (0.4, 1.0),
                    (0.5, 0.75),
                    (0.6, 1.0),
                    (0.7, 0.75),
                    (0.8, 1.0),
                ],
                p,
            );
        }
        if f & ROPE != 0 {
            // One slow wriggle (750 ms after a 50 ms delay) as the glyph arrives.
            fx.alpha *= upto(0.25);
            let p = ((s - 0.05) / 0.75).clamp(0.0, 1.0);
            fx.dy += keys(&[(0.0, 0.0), (0.25, 0.5), (0.5, -0.5), (1.0, 0.0)], ease(p));
        }
        if f & (WAVY | INSANITY) != 0 {
            // A wave of half an em, one swell every 1.5 s, each glyph a stride behind the last.
            fx.alpha *= upto(0.25);
            let phase = (time + i / GLYPHS_PER_SECOND) / 1.5;
            fx.dy += 0.5 * (0.5 - 0.5 * (std::f32::consts::TAU * phase).cos());
        }
        if f & PULSE != 0 && s < 3.0 {
            // Opacity down to half and back, three times a second apart.
            fx.alpha *= 0.75 + 0.25 * (std::f32::consts::TAU * s).cos();
        }
        if f & HAUNTED != 0 && s < 3.0 {
            // Fades out and back, three times.
            fx.alpha *= 0.5 + 0.5 * (std::f32::consts::TAU * s).cos();
        }
        if f & FAULTY != 0 {
            // Ten steps a second through the original's pattern (out at the second, fifth and
            // ninth), played backwards every other second, each word of about six glyphs later.
            const PATTERN: [bool; 10] = [
                true, false, true, true, false, true, true, true, false, true,
            ];
            let t = time * 10.0 + (index / 6) as f32 * 3.0;
            let step = (t.floor() as i64).rem_euclid(10) as usize;
            let backwards = ((t / 10.0).floor() as i64).rem_euclid(2) == 1;
            let on = if backwards {
                PATTERN[9 - step]
            } else {
                PATTERN[step]
            };
            if !on {
                fx.alpha = 0.0;
            }
        }
        if f & (RAINBOW | INSANITY) != 0 {
            // A full sweep of hues each second, back and forth, a stride apart per glyph.
            let t = time + i / GLYPHS_PER_SECOND;
            let tri = 1.0 - (1.0 - (t / 2.0).fract() * 2.0).abs();
            fx.hue = Some(360.0 * tri);
        }
        // `shake-x`, `shake-y` and `swing` are named in the original's stylesheet without any
        // keyframes, so they did nothing there; these are small stand-ins.
        if f & SHAKE_X != 0 {
            fx.dx += (time * 55.0 + i * 7.3).sin() * 0.04;
        }
        if f & SHAKE_Y != 0 {
            fx.dy += (time * 61.0 + i * 5.1).sin() * 0.04;
        }
        if f & SWING != 0 {
            fx.dx += (time * 7.0 + i * 0.4).sin() * 0.08;
        }
        fx
    }
}

/// A repeatable pseudo-random number in 0..1 from two integers.
fn hash01(a: u32, b: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297a_2d39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

fn ease(p: f32) -> f32 {
    p * p * (3.0 - 2.0 * p)
}

/// Linear interpolation through `(position, value)` keys; before the first and after the last it
/// holds their values.
fn keys(keys: &[(f32, f32)], p: f32) -> f32 {
    let (first, last) = (keys[0], keys[keys.len() - 1]);
    if p <= first.0 {
        return first.1;
    }
    for w in keys.windows(2) {
        let ((p0, v0), (p1, v1)) = (w[0], w[1]);
        if p <= p1 {
            return v0 + (v1 - v0) * (p - p0) / (p1 - p0);
        }
    }
    last.1
}

/// CSS `cubic-bezier(x1, y1, x2, y2)` at progress `x` (0..1).
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 || x >= 1.0 {
        return x.clamp(0.0, 1.0);
    }
    let at = |t: f32, a: f32, b: f32| {
        3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t.powi(3)
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if at(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at((lo + hi) / 2.0, y1, y2)
}

/// What an effect does to one glyph: offsets in ems (right and down), a scale about the
/// glyph's centre, an alpha multiplier and a hue override (degrees, full saturation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphFx {
    pub dx: f32,
    pub dy: f32,
    pub scale: f32,
    pub alpha: f32,
    pub hue: Option<f32>,
}

impl Default for GlyphFx {
    fn default() -> Self {
        GlyphFx {
            dx: 0.0,
            dy: 0.0,
            scale: 1.0,
            alpha: 1.0,
            hue: None,
        }
    }
}

/// `text-reveal` meaning "all of it".
pub const REVEAL_ALL: f32 = f32::INFINITY;

#[cfg(test)]
mod tests {
    use super::*;

    // RDY-TXT-01
    #[test]
    fn effect_names_combine_and_unknown_ones_are_refused() {
        let e = TextEffect::parse("wavy rainbow").unwrap();
        assert!(e.is_ambient() && !e.is_none());
        assert_eq!(TextEffect::parse("none"), Some(TextEffect::NONE));
        assert_eq!(TextEffect::parse("wavy sparkly"), None);
        assert!(!TextEffect::parse("drop").unwrap().is_ambient());
        assert!(TextEffect::parse("fade cursor").unwrap().has_cursor());
        assert!(!TextEffect::parse("fade").unwrap().has_cursor());
        assert!(
            TextEffect::parse("blink").unwrap().is_ambient(),
            "dips follow the clock"
        );
        assert!(!TextEffect::parse("pixar-bounce").unwrap().is_ambient());
        assert_eq!(
            TextEffect::parse("hue-rotate"),
            TextEffect::parse("rainbow")
        );
    }

    // RDY-TXT-02
    #[test]
    fn arriving_glyphs_are_part_way_and_old_ones_settled() {
        for name in [
            "fade",
            "drop",
            "zoom-in",
            "slide-up",
            "blink",
            "bounce",
            "pixar-bounce",
            "hop",
            "rope",
        ] {
            let e = TextEffect::parse(name).unwrap();
            assert_ne!(e.eval(4, 0.05, 0.0), GlyphFx::default(), "{name}: arriving");
            // (A blink may dip at some moments; at time 0 glyphs 0 and 3 are lit.)
            // (A settled blink may be in one of its brief dips: only its alpha differs.)
            let settled = |g: GlyphFx| GlyphFx {
                alpha: if name == "blink" { 1.0 } else { g.alpha },
                ..g
            };
            assert_eq!(
                settled(e.eval(0, 9.0, 0.0)),
                GlyphFx::default(),
                "{name}: settled"
            );
            assert_eq!(
                settled(e.eval(3, f32::INFINITY, 0.0)),
                GlyphFx::default(),
                "{name}"
            );
            assert!(e.tail_seconds() <= 5.0);
        }
    }

    // RDY-TXT-03
    #[test]
    fn ambient_effects_follow_the_clock_and_index() {
        let e = TextEffect::parse("wavy").unwrap();
        let a = e.eval(0, f32::INFINITY, 1.0).dy;
        assert_ne!(a, e.eval(0, f32::INFINITY, 1.3).dy, "moves over time");
        assert_ne!(
            a,
            e.eval(30, f32::INFINITY, 1.0).dy,
            "differs along the line"
        );
        let rainbow = TextEffect::parse("rainbow")
            .unwrap()
            .eval(2, f32::INFINITY, 0.5);
        assert!(rainbow.hue.is_some());
    }

    // RDY-TXT-06
    #[test]
    fn entrances_match_the_originals_shapes() {
        let at = |name: &str, secs: f32| TextEffect::parse(name).unwrap().eval(0, secs, 0.0);
        // Fade is only a fade: no movement or scaling at any point.
        let f = at("fade", 0.0);
        assert!(f.alpha < 0.01 && f.dx == 0.0 && f.dy == 0.0 && f.scale == 1.0);
        let mid = at("fade", 0.25);
        assert!(mid.alpha > 0.2 && mid.alpha < 0.8 && mid.dx == 0.0 && mid.scale == 1.0);
        // Zoom in grows from nothing; zoom out shrinks from double.
        assert!(at("zoom-in", 0.01).scale < 0.2);
        assert!(at("zoom-out", 0.01).scale > 1.8);
        // Drop falls from above and overshoots the line before settling.
        assert!(at("drop", 0.01).dy < -0.5);
        assert!(at("drop", 0.2).dy > 0.0, "overshoot");
        // The pixar bounce arrives from the right and is still hopping at one second.
        assert!(at("pixar-bounce", 0.2).dx > 1.0);
        let hop = at("pixar-bounce", 1.0);
        assert!(hop.dy != 0.0 || hop.scale != 1.0);
        // Blink is a neon flicker: off at some moments and lit at others while arriving, and a
        // different pattern for each glyph; once done it is lit, with only rare brief dips.
        let blink = TextEffect::parse("blink").unwrap();
        let alphas = |i: usize| -> Vec<f32> {
            (0..20)
                .map(|n| blink.eval(i, n as f32 * 0.035, 0.0).alpha)
                .collect()
        };
        let first = alphas(0);
        assert!(first.contains(&0.0) && first.contains(&1.0));
        assert!(
            first.iter().all(|a| *a == 0.0 || *a == 1.0),
            "on or off, never dim"
        );
        assert!((1..8).any(|i| alphas(i) != first), "random per glyph");
        let dips = (0..400)
            .filter(|n| blink.eval(*n % 40, 5.0, *n as f32 * 0.083).alpha == 0.0)
            .count();
        assert!(dips > 0 && dips < 40, "rare dips: {dips} of 400");
    }
}
