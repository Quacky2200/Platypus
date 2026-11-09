// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The maths of acoustics, with no engine in it: zone gradients, how walls dull a sound, how
//! loud a sensor hears something, the Doppler pitch. Everything here is a function of numbers,
//! so the tests need no scene, physics or mixer.

use platypus_engine_api::audio_components::Directivity;

/// A zone's volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Sphere,
    Box,
}

/// How a zone's effect grows from its outer edge to its full-effect core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    Linear,
    /// Slow at both ends (smoothstep): no audible corner at the edge.
    Smooth,
    /// Stays weak until near the core.
    EaseIn,
    /// Rises fast just inside the edge.
    EaseOut,
}

/// Maps 0 (outer edge) .. 1 (core) to the effect's strength.
pub fn curve(c: Curve, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match c {
        Curve::Linear => t,
        Curve::Smooth => t * t * (3.0 - 2.0 * t),
        Curve::EaseIn => t * t,
        Curve::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
    }
}

/// How strongly a zone applies at `local` (a point in the zone's space, metres, scale applied
/// to `extent`): 1 within the core, 0 at and beyond the edge, `curve` between.
///
/// `extent` is the sphere's radius (x) or the box's half sizes; `inner` (0 to 1) is how much of
/// that reaches full strength. `invert` flips the result, for "everywhere but here" (the
/// nature outside a cave).
pub fn zone_weight(
    shape: Shape,
    local: [f64; 3],
    extent: [f64; 3],
    inner: f64,
    c: Curve,
    invert: bool,
) -> f64 {
    let d = match shape {
        Shape::Sphere => {
            let r = extent[0].max(1e-6);
            (local[0] * local[0] + local[1] * local[1] + local[2] * local[2]).sqrt() / r
        }
        Shape::Box => (0..3)
            .map(|i| local[i].abs() / extent[i].max(1e-6))
            .fold(0.0, f64::max),
    };
    let inner = inner.clamp(0.0, 0.999);
    let w = if d <= inner {
        1.0
    } else if d >= 1.0 {
        0.0
    } else {
        curve(c, (1.0 - d) / (1.0 - inner))
    };
    if invert { 1.0 - w } else { w }
}

/// The effect settings of a track (a bus or the master), by what a zone can ask for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxTarget {
    /// Hz; at the top (22000) the filter is open.
    pub low_pass: f64,
    /// Hz; at the bottom (0) the filter is open.
    pub high_pass: f64,
    pub reverb: f64,
    pub reverb_size: f64,
    pub reverb_damping: f64,
    pub echo: f64,
    /// Linear gain.
    pub volume: f64,
}

impl FxTarget {
    /// Nothing applied: what a bus is outside every zone.
    pub const OPEN: FxTarget = FxTarget {
        low_pass: 22000.0,
        high_pass: 0.0,
        reverb: 0.0,
        reverb_size: 0.5,
        reverb_damping: 0.5,
        echo: 0.0,
        volume: 1.0,
    };

    /// `self` moved `w` of the way to `to`. Frequencies move on a log scale (equal steps sound
    /// equal), so half way from open to 500 Hz is about 3 kHz, not 11 kHz.
    pub fn lerp(&self, to: &FxTarget, w: f64) -> FxTarget {
        let w = w.clamp(0.0, 1.0);
        let l = |a: f64, b: f64| a + (b - a) * w;
        FxTarget {
            low_pass: log_lerp(self.low_pass, to.low_pass, w),
            high_pass: {
                // Open is 0, but the glide runs on a log scale from 20 Hz: below 25 it is open.
                let hp = log_lerp(self.high_pass.max(20.0), to.high_pass.max(20.0), w);
                if hp < 25.0 { 0.0 } else { hp }
            },
            reverb: l(self.reverb, to.reverb),
            reverb_size: l(self.reverb_size, to.reverb_size),
            reverb_damping: l(self.reverb_damping, to.reverb_damping),
            echo: l(self.echo, to.echo),
            volume: l(self.volume, to.volume),
        }
    }
}

fn log_lerp(a: f64, b: f64, w: f64) -> f64 {
    let (a, b) = (a.max(1.0), b.max(1.0));
    (a.ln() + (b.ln() - a.ln()) * w).exp()
}

/// Blends zones into one setting: from `OPEN`, each zone (lowest priority first, so a later one
/// wins where they overlap) pulls it towards its target by its weight.
pub fn blend_zones(zones: &[(f64, FxTarget)]) -> FxTarget {
    zones
        .iter()
        .fold(FxTarget::OPEN, |acc, (w, t)| acc.lerp(t, *w))
}

/// What one occluder does to a sound passing through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OccluderSpec {
    /// 0 shut (a closed door), 1 open (an open door, a window): how much gets through.
    pub openness: f64,
    /// The gain lost when shut (0 none, 1 everything).
    pub absorption: f64,
    /// The low-pass cutoff (Hz) a shut one leaves: thick walls keep only the lows.
    pub low_pass: f64,
}

/// What a ray's crossings leave of a sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    pub gain: f64,
    pub low_pass: f64,
}

impl Probe {
    pub const CLEAR: Probe = Probe {
        gain: 1.0,
        low_pass: 22000.0,
    };
}

/// The lowest cutoff walls stack down to: past it the sound is a rumble.
pub const MIN_CUTOFF: f64 = 180.0;

/// Crossing several occluders: gains multiply, each cutoff ratio multiplies in.
pub fn through(hits: &[OccluderSpec]) -> Probe {
    let mut p = Probe::CLEAR;
    for h in hits {
        let shut = (1.0 - h.openness).clamp(0.0, 1.0);
        p.gain *= 1.0 - h.absorption.clamp(0.0, 1.0) * shut;
        p.low_pass *= (h.low_pass.max(1.0) / 22000.0).powf(shut);
    }
    p.low_pass = p.low_pass.clamp(MIN_CUTOFF, 22000.0);
    p
}

/// Several rays (the centre and some to the sides) averaged: a doorway the centre ray misses
/// still lets some through. Gains average, cutoffs average on a log scale.
pub fn average(rays: &[Probe]) -> Probe {
    if rays.is_empty() {
        return Probe::CLEAR;
    }
    let n = rays.len() as f64;
    Probe {
        gain: rays.iter().map(|r| r.gain).sum::<f64>() / n,
        low_pass: (rays.iter().map(|r| r.low_pass.ln()).sum::<f64>() / n).exp(),
    }
}

/// How an occlusion strength (0 none, 1 full) scales a probe: lets a source ask for less.
pub fn scaled(p: Probe, strength: f64) -> Probe {
    let s = strength.clamp(0.0, 1.0);
    Probe {
        gain: 1.0 + (p.gain - 1.0) * s,
        low_pass: log_lerp(22000.0, p.low_pass, s),
    }
}

/// The low-pass cutoff (Hz) a head puts on a sound: behind the listener the ears shade it, and
/// above or below the pinnae do. Panning alone cannot tell front from back or up from down;
/// this dullness can. `forward` and `to_source` are unit vectors (world space, y up);
/// `strength` 0 leaves it open, 1 takes a source directly behind down to 4.5 kHz and one
/// straight above or below to 9 kHz.
pub fn head_cue(forward: [f64; 3], to_source: [f64; 3], strength: f64) -> f64 {
    let s = strength.clamp(0.0, 1.0);
    let ahead = forward[0] * to_source[0] + forward[1] * to_source[1] + forward[2] * to_source[2];
    let behind = (-ahead).clamp(0.0, 1.0);
    let elevation = to_source[1].abs().clamp(0.0, 1.0);
    let back = log_lerp(22000.0, 4500.0, behind.powf(1.5) * s);
    let up = log_lerp(22000.0, 9000.0, elevation * s);
    back.min(up)
}

/// How far out of a source's cone the listener is, 0 (inside the inner cone: full) to 1 (outside the
/// outer cone), linear between. `angle` is how far the listener is from the source's forward
/// direction (degrees); the cone angles are full widths (360 is all round: no cone), as in Web Audio.
pub fn cone_amount(angle: f64, inner: f64, outer: f64) -> f64 {
    let (inner, outer) = (inner.clamp(0.0, 360.0), outer.clamp(0.0, 360.0));
    let outer = outer.max(inner);
    if inner >= 360.0 {
        return 0.0;
    }
    let (a, i, o) = (angle.abs(), inner * 0.5, outer * 0.5);
    if a <= i {
        0.0
    } else if a >= o {
        1.0
    } else {
        (a - i) / (o - i)
    }
}

/// The gain and low-pass cutoff (Hz) a source's cone gives a listener `amount` (0 to 1, see
/// [`cone_amount`]) out of it: quieter by `outer_gain` and duller towards `outer_low_pass`.
pub fn cone_effect(amount: f64, outer_gain: f64, outer_low_pass: f64) -> (f64, f64) {
    let t = amount.clamp(0.0, 1.0);
    (
        1.0 + (outer_gain.clamp(0.0, 1.0) - 1.0) * t,
        log_lerp(22000.0, outer_low_pass.clamp(MIN_CUTOFF, 22000.0), t),
    )
}

/// How loud a source of this polar pattern is at `angle` degrees off its forward direction, 0 to 1
/// (1 on axis). The first-order patterns are `a + (1 - a) cos(angle)`, absolute: cardioid
/// `a` 0.5, supercardioid 0.37, hypercardioid 0.25, figure-eight 0. The ultra-directional one is a
/// narrow beam, `(0.5 + 0.5 cos)^6`, with side and rear lobes of 18% (a shotgun's interference).
pub fn pattern_gain(pattern: Directivity, angle: f64) -> f64 {
    let t = angle.to_radians();
    let first_order = |a: f64| (a + (1.0 - a) * t.cos()).abs();
    match pattern {
        Directivity::Omni => 1.0,
        Directivity::Cardioid => first_order(0.5),
        Directivity::Supercardioid => first_order(0.37),
        Directivity::Hypercardioid => first_order(0.25),
        Directivity::FigureEight => first_order(0.0),
        Directivity::UltraDirectional => {
            let beam = (0.5 + 0.5 * t.cos()).powi(6);
            // Lobes grow in from 30 to 70 degrees: none on the axis.
            let ramp = ((angle.abs() - 30.0) / 40.0).clamp(0.0, 1.0);
            beam.max(0.18 * (4.0 * t).cos().abs() * ramp)
        }
    }
}

/// The gain and low-pass cutoff (Hz) a pattern gives where it is `g` as loud as on axis (see
/// [`pattern_gain`]), blended with all-round by `strength` (0 to 1). Nulls are held at 3% (-30 dB)
/// rather than silent; the quieter the direction, the duller (towards `low_pass`).
pub fn pattern_effect(g: f64, low_pass: f64, strength: f64) -> (f64, f64) {
    let raw = g.clamp(0.0, 1.0);
    let s = strength.clamp(0.0, 1.0);
    // The dullness follows the pattern itself; only the gain is kept off silence.
    let cutoff = log_lerp(22000.0, low_pass.clamp(MIN_CUTOFF, 22000.0), 1.0 - raw);
    (
        1.0 + (raw.max(0.03) - 1.0) * s,
        log_lerp(22000.0, cutoff, s),
    )
}

/// The cutoff (Hz) air leaves of a sound that travelled `distance` metres: the highs fade the
/// further it goes, so far sounds are soft and dull (`strength` 1 is a lot of air: 11 kHz at 100 m,
/// 3 kHz at 300 m; 0 is none). Held above 2.5 kHz, where speech still carries.
pub fn air_cutoff(distance: f64, strength: f64) -> f64 {
    let s = strength.max(0.0);
    (22000.0 * (-distance.max(0.0) * s / 150.0).exp()).clamp(2500.0, 22000.0)
}

/// One way round a wall: through a portal. The two legs (listener to portal, portal to source)
/// each left a probe; `portal_gain` and `portal_low_pass` are the portal's own.
/// The result is what reaches the listener by that way, and the path's length.
pub fn through_portal(
    to_portal: f64,
    from_portal: f64,
    first: Probe,
    second: Probe,
    portal_gain: f64,
    portal_low_pass: f64,
) -> (Probe, f64) {
    let gain = first.gain * second.gain * portal_gain.clamp(0.0, 1.0);
    let ratio = (first.low_pass / 22000.0)
        * (second.low_pass / 22000.0)
        * (portal_low_pass.clamp(MIN_CUTOFF, 22000.0) / 22000.0);
    (
        Probe {
            gain,
            low_pass: (22000.0 * ratio).clamp(MIN_CUTOFF, 22000.0),
        },
        to_portal + from_portal,
    )
}

/// Whether the way round (gain over path length, the inverse-distance rule) beats going
/// straight through: it must win by a tenth, so the choice does not flicker.
pub fn detour_wins(direct: Probe, direct_distance: f64, round: Probe, path: f64) -> bool {
    let metric = |p: Probe, d: f64| p.gain / d.max(1.0);
    metric(round, path) > metric(direct, direct_distance) * 1.1
}

/// How loud something sounds to a sensor, 0 to 1: full at the source, falling to nothing at
/// `radius` metres (squared, as sound spreads), times the gain the walls left.
pub fn heard_level(radius: f64, distance: f64, gain: f64, sensitivity: f64) -> f64 {
    if radius <= 0.0 {
        return 0.0;
    }
    let reach = radius * sensitivity.max(0.0);
    if distance >= reach {
        return 0.0;
    }
    let t = 1.0 - distance / reach.max(1e-6);
    (t * t * gain).clamp(0.0, 1.0)
}

/// The pitch factor of the Doppler effect. `to_source` is the unit vector from the listener to
/// the source; velocities in m/s. A source coming at you sounds higher, one leaving lower; a
/// listener moving towards the source hears it higher too. `level` scales how far from 1 it
/// goes (0 none, 1 physical). Speeds are held under 90% of the speed of sound.
pub fn doppler(
    to_source: [f64; 3],
    source_velocity: [f64; 3],
    listener_velocity: [f64; 3],
    speed_of_sound: f64,
    level: f64,
) -> f64 {
    let c = speed_of_sound.max(1.0);
    let along = |v: [f64; 3]| {
        (v[0] * to_source[0] + v[1] * to_source[1] + v[2] * to_source[2]).clamp(-0.9 * c, 0.9 * c)
    };
    // Receiver towards the source: +; source away from the receiver: +.
    let raw = (c + along(listener_velocity)) / (c + along(source_velocity));
    (1.0 + (raw - 1.0) * level.max(0.0)).clamp(0.25, 4.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ACO-MOD-01
    #[test]
    fn a_zone_is_full_in_its_core_and_fades_to_nothing_at_its_edge() {
        let w = |x: f64| {
            zone_weight(
                Shape::Sphere,
                [x, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                0.4,
                Curve::Linear,
                false,
            )
        };
        assert_eq!(w(0.0), 1.0);
        assert_eq!(w(4.0), 1.0, "core is 40% of the radius");
        assert!((w(7.0) - 0.5).abs() < 1e-9, "half way across the gradient");
        assert_eq!(w(10.0), 0.0);
        assert_eq!(w(25.0), 0.0);
        // Inverted: the nature outside the cave.
        let out = zone_weight(
            Shape::Sphere,
            [7.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            0.4,
            Curve::Linear,
            true,
        );
        assert!((out - 0.5).abs() < 1e-9);
    }

    // ACO-MOD-02
    #[test]
    fn a_box_zone_uses_its_own_half_sizes_and_curves_shape_the_gradient() {
        let b = |p: [f64; 3], c| zone_weight(Shape::Box, p, [5.0, 2.0, 10.0], 0.0, c, false);
        assert!((b([0.0, 0.0, 0.0], Curve::Linear) - 1.0).abs() < 1e-9);
        assert!((b([2.5, 0.0, 0.0], Curve::Linear) - 0.5).abs() < 1e-9);
        // The tight axis decides: 1.5 of 2 on y is 75% out.
        assert!((b([0.0, 1.5, 0.0], Curve::Linear) - 0.25).abs() < 1e-9);
        assert_eq!(b([0.0, 0.0, 10.5], Curve::Linear), 0.0);
        // At the same point the curves differ but agree at the ends.
        let p = [2.5, 0.0, 0.0];
        assert!(b(p, Curve::EaseIn) < b(p, Curve::Linear));
        assert!(b(p, Curve::EaseOut) > b(p, Curve::Linear));
        assert!((b(p, Curve::Smooth) - 0.5).abs() < 1e-9);
        for c in [Curve::Linear, Curve::Smooth, Curve::EaseIn, Curve::EaseOut] {
            assert_eq!(curve(c, 0.0), 0.0);
            assert_eq!(curve(c, 1.0), 1.0);
        }
    }

    // ACO-MOD-03
    #[test]
    fn overlapping_zones_blend_with_the_later_one_winning_and_frequencies_glide_on_a_log_scale() {
        let cave = FxTarget {
            low_pass: 4000.0,
            reverb: 0.8,
            echo: 0.4,
            ..FxTarget::OPEN
        };
        let none = blend_zones(&[]);
        assert_eq!(none, FxTarget::OPEN);
        let half = blend_zones(&[(0.5, cave)]);
        assert!((half.reverb - 0.4).abs() < 1e-9);
        assert!(
            half.low_pass > 8000.0 && half.low_pass < 10000.0,
            "geometric mean of 22000 and 4000: {}",
            half.low_pass
        );
        assert_eq!(
            half.high_pass, 0.0,
            "an untouched filter stays exactly open"
        );
        let tunnel = FxTarget {
            low_pass: 1000.0,
            ..FxTarget::OPEN
        };
        let both = blend_zones(&[(1.0, cave), (1.0, tunnel)]);
        assert!((both.low_pass - 1000.0).abs() < 1e-6, "the later zone wins");
        assert_eq!(both.reverb, 0.0, "and so does its (open) reverb");
    }

    // ACO-MOD-04
    #[test]
    fn walls_multiply_their_losses_and_an_open_door_lets_everything_through() {
        let wall = OccluderSpec {
            openness: 0.0,
            absorption: 0.5,
            low_pass: 1100.0,
        };
        let door_open = OccluderSpec {
            openness: 1.0,
            ..wall
        };
        assert_eq!(through(&[]), Probe::CLEAR);
        assert_eq!(through(&[door_open]), Probe::CLEAR);
        let one = through(&[wall]);
        assert!((one.gain - 0.5).abs() < 1e-9 && (one.low_pass - 1100.0).abs() < 1e-6);
        let two = through(&[wall, wall]);
        assert!((two.gain - 0.25).abs() < 1e-9);
        assert!(two.low_pass < 1100.0 && two.low_pass >= MIN_CUTOFF);
        // A door half open leaves half the effect (on a log scale for the cutoff).
        let ajar = through(&[OccluderSpec {
            openness: 0.5,
            ..wall
        }]);
        assert!((ajar.gain - 0.75).abs() < 1e-9);
        assert!(ajar.low_pass > 1100.0 && ajar.low_pass < 22000.0);
        // Rays that find the doorway pull the average open.
        let mixed = average(&[one, Probe::CLEAR, Probe::CLEAR, one, Probe::CLEAR]);
        assert!((mixed.gain - 0.8).abs() < 1e-9);
        assert!(mixed.low_pass > one.low_pass);
        // A source that asks for less occlusion is dulled less.
        let half = scaled(one, 0.5);
        assert!((half.gain - 0.75).abs() < 1e-9);
        assert_eq!(scaled(one, 0.0), Probe::CLEAR);
        let full = scaled(one, 1.0);
        assert!((full.gain - one.gain).abs() < 1e-9 && (full.low_pass - one.low_pass).abs() < 1e-6);
    }

    // ACO-MOD-05
    #[test]
    fn a_sensor_hears_less_far_away_and_through_walls() {
        assert_eq!(heard_level(20.0, 0.0, 1.0, 1.0), 1.0);
        assert!((heard_level(20.0, 10.0, 1.0, 1.0) - 0.25).abs() < 1e-9);
        assert_eq!(heard_level(20.0, 20.0, 1.0, 1.0), 0.0);
        assert!((heard_level(20.0, 10.0, 0.5, 1.0) - 0.125).abs() < 1e-9);
        assert!(
            heard_level(20.0, 25.0, 1.0, 2.0) > 0.0,
            "sensitive ears hear further"
        );
        assert_eq!(
            heard_level(0.0, 1.0, 1.0, 1.0),
            0.0,
            "silent sources are not heard"
        );
    }

    // ACO-MOD-06
    #[test]
    fn doppler_raises_the_pitch_coming_and_lowers_it_going() {
        let c = 343.0;
        let to = [1.0, 0.0, 0.0]; // the source is ahead of the listener
        let away = doppler(to, [30.0, 0.0, 0.0], [0.0; 3], c, 1.0);
        let toward = doppler(to, [-30.0, 0.0, 0.0], [0.0; 3], c, 1.0);
        assert!((away - c / (c + 30.0)).abs() < 1e-9 && away < 1.0);
        assert!((toward - c / (c - 30.0)).abs() < 1e-9 && toward > 1.0);
        // A listener running at the source hears it higher; standing still, unchanged.
        assert!(doppler(to, [0.0; 3], [20.0, 0.0, 0.0], c, 1.0) > 1.0);
        assert_eq!(doppler(to, [0.0; 3], [0.0; 3], c, 1.0), 1.0);
        // Moving across the line of sight changes nothing.
        assert_eq!(doppler(to, [0.0, 40.0, 0.0], [0.0; 3], c, 1.0), 1.0);
        // The level scales the effect; a bullet is held under the speed of sound.
        let exaggerated = doppler(to, [-30.0, 0.0, 0.0], [0.0; 3], c, 2.0);
        assert!(exaggerated > toward);
        assert_eq!(doppler(to, [-30.0, 0.0, 0.0], [0.0; 3], c, 0.0), 1.0);
        assert!(doppler(to, [-900.0, 0.0, 0.0], [0.0; 3], c, 1.0).is_finite());
    }

    // ACO-MOD-07
    #[test]
    fn the_head_dulls_what_is_behind_and_above() {
        let fwd = [0.0, 0.0, -1.0];
        let cue = |to: [f64; 3], s| head_cue(fwd, to, s);
        assert_eq!(cue([0.0, 0.0, -1.0], 1.0), 22000.0, "ahead is open");
        assert_eq!(cue([1.0, 0.0, 0.0], 1.0), 22000.0, "to the side is open");
        assert!(
            (cue([0.0, 0.0, 1.0], 1.0) - 4500.0).abs() < 1e-6,
            "behind is dull"
        );
        assert!(
            (cue([0.0, 1.0, 0.0], 1.0) - 9000.0).abs() < 1e-6,
            "above is a little dull"
        );
        assert!(cue([0.0, -1.0, 0.0], 1.0) < 22000.0, "and so is below");
        assert_eq!(
            cue([0.0, 0.0, 1.0], 0.0),
            22000.0,
            "strength 0 turns it off"
        );
        // Between: a source behind and to the right is not as dull as one directly behind.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let diag = cue([h, 0.0, h], 1.0);
        assert!(diag > 4500.0 && diag < 22000.0, "{diag}");
    }

    // ACO-MOD-09
    #[test]
    fn a_cone_is_full_inside_its_inner_angle_and_dull_and_quiet_outside_its_outer() {
        // A 90 degree inner cone (45 each side) inside a 180 degree outer (90 each side).
        let amount = |a| cone_amount(a, 90.0, 180.0);
        assert_eq!(amount(0.0), 0.0);
        assert_eq!(amount(45.0), 0.0, "the inner edge");
        assert!(
            (amount(67.5) - 0.5).abs() < 1e-9,
            "half way across the fade"
        );
        assert_eq!(amount(90.0), 1.0);
        assert_eq!(amount(180.0), 1.0, "directly behind");
        assert_eq!(amount(-67.5), amount(67.5), "left and right alike");
        // No cone: all round.
        assert_eq!(cone_amount(180.0, 360.0, 360.0), 0.0);
        assert_eq!(
            cone_amount(180.0, 360.0, 180.0),
            0.0,
            "an inner cone of 360 wins"
        );
        // An outer angle narrower than the inner one cannot make a hole: it is the inner.
        assert_eq!(cone_amount(30.0, 90.0, 40.0), 0.0);
        let (gain, cutoff) = cone_effect(0.0, 0.25, 3000.0);
        assert_eq!((gain, cutoff), (1.0, 22000.0));
        let (gain, cutoff) = cone_effect(1.0, 0.25, 3000.0);
        assert!((gain - 0.25).abs() < 1e-9 && (cutoff - 3000.0).abs() < 1e-6);
        let (gain, cutoff) = cone_effect(0.5, 0.25, 3000.0);
        assert!((gain - 0.625).abs() < 1e-9);
        assert!(cutoff > 3000.0 && cutoff < 22000.0);
    }

    // ACO-MOD-10
    #[test]
    fn air_takes_the_highs_off_with_distance() {
        assert_eq!(air_cutoff(0.0, 1.0), 22000.0);
        assert_eq!(air_cutoff(500.0, 0.0), 22000.0, "no air, no loss");
        let near = air_cutoff(10.0, 1.0);
        let mid = air_cutoff(100.0, 1.0);
        let far = air_cutoff(300.0, 1.0);
        assert!(near > mid && mid > far, "{near} {mid} {far}");
        assert!(
            (mid - 11_300.0).abs() < 200.0,
            "about 11 kHz at 100 m: {mid}"
        );
        assert!((far - 2975.0).abs() < 100.0, "about 3 kHz at 300 m: {far}");
        assert_eq!(
            air_cutoff(5000.0, 1.0),
            2500.0,
            "held where speech still carries"
        );
        assert!(air_cutoff(100.0, 2.0) < mid, "more air, more loss");
    }

    // ACO-MOD-11
    #[test]
    fn each_polar_pattern_has_its_own_shape() {
        let g = pattern_gain;
        for p in [
            Directivity::Omni,
            Directivity::Cardioid,
            Directivity::Supercardioid,
            Directivity::Hypercardioid,
            Directivity::FigureEight,
            Directivity::UltraDirectional,
        ] {
            assert!((g(p, 0.0) - 1.0).abs() < 1e-9, "{p:?} is full on axis");
            assert_eq!(g(p, 40.0), g(p, -40.0), "{p:?} is symmetric");
        }
        assert_eq!(
            g(Directivity::Omni, 137.0),
            1.0,
            "omni is the same everywhere"
        );
        // Cardioid: half to the sides, nothing behind.
        assert!((g(Directivity::Cardioid, 90.0) - 0.5).abs() < 1e-9);
        assert!(g(Directivity::Cardioid, 180.0) < 1e-9);
        // Supercardioid and hypercardioid are narrower at the sides and have a rear lobe.
        assert!((g(Directivity::Supercardioid, 90.0) - 0.37).abs() < 1e-9);
        assert!((g(Directivity::Supercardioid, 180.0) - 0.26).abs() < 1e-9);
        assert!((g(Directivity::Hypercardioid, 90.0) - 0.25).abs() < 1e-9);
        assert!((g(Directivity::Hypercardioid, 180.0) - 0.5).abs() < 1e-9);
        // Figure-eight: none to the sides, as loud behind as in front.
        assert!(g(Directivity::FigureEight, 90.0) < 1e-9);
        assert!((g(Directivity::FigureEight, 180.0) - 1.0).abs() < 1e-9);
        // A shotgun: a narrow beam, small lobes at the sides and behind, none right ahead of them.
        let u = Directivity::UltraDirectional;
        assert!(
            g(u, 20.0) < g(Directivity::Cardioid, 20.0),
            "narrower than a cardioid"
        );
        assert!(
            g(u, 90.0) > 0.1 && g(u, 90.0) < 0.2,
            "a side lobe: {}",
            g(u, 90.0)
        );
        assert!(
            g(u, 180.0) > 0.1 && g(u, 180.0) < 0.2,
            "a rear lobe: {}",
            g(u, 180.0)
        );
        // Narrowness: cardioid > supercardioid > hypercardioid > ultra at 60 degrees.
        let at60 = |p| g(p, 60.0);
        assert!(at60(Directivity::Cardioid) > at60(Directivity::Supercardioid));
        assert!(at60(Directivity::Supercardioid) > at60(Directivity::Hypercardioid));
        assert!(at60(Directivity::Hypercardioid) > at60(u));
    }

    // ACO-MOD-12
    #[test]
    fn a_pattern_makes_quiet_directions_duller_and_strength_blends_it_away() {
        assert_eq!(pattern_effect(1.0, 5000.0, 1.0), (1.0, 22000.0));
        let (gain, cutoff) = pattern_effect(0.5, 5000.0, 1.0);
        assert!((gain - 0.5).abs() < 1e-9);
        assert!(cutoff > 5000.0 && cutoff < 22000.0);
        let (gain, cutoff) = pattern_effect(0.0, 5000.0, 1.0);
        assert!((gain - 0.03).abs() < 1e-9, "a null is held at -30 dB");
        assert!((cutoff - 5000.0).abs() < 1e-6);
        // Strength 0 is all round; 0.5 is half way.
        assert_eq!(pattern_effect(0.1, 5000.0, 0.0), (1.0, 22000.0));
        let (half, _) = pattern_effect(0.1, 5000.0, 0.5);
        assert!((half - 0.55).abs() < 1e-9);
    }

    // ACO-MOD-08
    #[test]
    fn the_way_round_a_wall_wins_when_the_wall_costs_more_than_the_detour() {
        let wall = Probe {
            gain: 0.3,
            low_pass: 700.0,
        };
        // Through a doorway: both legs clear.
        let (round, path) = through_portal(4.0, 5.0, Probe::CLEAR, Probe::CLEAR, 1.0, 9000.0);
        assert_eq!(path, 9.0);
        assert!((round.gain - 1.0).abs() < 1e-9);
        assert!(
            (round.low_pass - 9000.0).abs() < 1e-6,
            "edges take the highs off"
        );
        // Straight through the wall: 8 m, a third of the gain. Round: 9 m, all of it.
        assert!(detour_wins(wall, 8.0, round, path));
        // A door that is open all the way is not worth a detour.
        assert!(!detour_wins(Probe::CLEAR, 8.0, round, path));
        // A shut door in the portal's leg takes it away again.
        let shut = Probe {
            gain: 0.2,
            low_pass: 600.0,
        };
        let (blocked, path) = through_portal(4.0, 5.0, shut, Probe::CLEAR, 1.0, 9000.0);
        assert!(!detour_wins(wall, 8.0, blocked, path));
        assert!(blocked.low_pass < round.low_pass && blocked.low_pass >= MIN_CUTOFF);
        // A closed portal passes nothing.
        let (closed, _) = through_portal(4.0, 5.0, Probe::CLEAR, Probe::CLEAR, 0.0, 9000.0);
        assert_eq!(closed.gain, 0.0);
    }
}
