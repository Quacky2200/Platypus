// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Mathf`, `Vector2`, `Vector3` and `Vector4` as `js/engine/src/math.ts` has them. The values are
//! doubles (like the TypeScript's), computed in the same order, so results are bit for bit the
//! same. Units: metres, degrees for angles, y up, right-handed (-z forward).
//!
//! Differences: a `...Like` parameter is the class itself; `toArray()` and `from(v)` are not
//! there; `Mathf.smoothDamp` returns a `Vector2` of (value, velocity) instead of an array.

use crate::jsnum::{self, clamp, clamp01, hypot};
use ts_api::{ValueType, ts_api};

pub const DEG: f64 = std::f64::consts::PI / 180.0;

macro_rules! value {
    ($(#[$m:meta])* $name:ident { $($f:ident),+ } $n:literal) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct $name { $(pub $f: f64),+ }
        impl ValueType<$n> for $name {
            fn from_parts(p: [f64; $n]) -> Self {
                let mut it = p.into_iter();
                $name { $($f: it.next().unwrap_or(0.0)),+ }
            }
            fn to_parts(self) -> [f64; $n] {
                [$(self.$f),+]
            }
        }
    };
}

value!(
    /// `Vector2`.
    V2 { x, y } 2
);
value!(
    /// `Vector3`.
    V3 { x, y, z } 3
);
value!(
    /// `Vector4`.
    V4 { x, y, z, w } 4
);

pub const fn v2(x: f64, y: f64) -> V2 {
    V2 { x, y }
}

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

pub const fn v4(x: f64, y: f64, z: f64, w: f64) -> V4 {
    V4 { x, y, z, w }
}

impl V3 {
    pub const ZERO: V3 = v3(0.0, 0.0, 0.0);
    pub const UP: V3 = v3(0.0, 1.0, 0.0);
    pub const RIGHT: V3 = v3(1.0, 0.0, 0.0);

    pub fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    pub fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    pub fn scale(self, s: f64) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }

    pub fn negate(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }

    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn magnitude(self) -> f64 {
        hypot(&[self.x, self.y, self.z])
    }

    pub fn sqr_magnitude(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// Length 1 in the same direction (zero stays zero).
    pub fn normalized(self) -> V3 {
        let m = self.magnitude();
        if m > 0.0 {
            self.scale(1.0 / m)
        } else {
            V3::ZERO
        }
    }

    pub fn lerp(self, o: V3, t: f64) -> V3 {
        let k = clamp01(t);
        v3(
            self.x + (o.x - self.x) * k,
            self.y + (o.y - self.y) * k,
            self.z + (o.z - self.z) * k,
        )
    }
}

impl V2 {
    pub const ZERO: V2 = v2(0.0, 0.0);

    pub fn add(self, o: V2) -> V2 {
        v2(self.x + o.x, self.y + o.y)
    }

    pub fn sub(self, o: V2) -> V2 {
        v2(self.x - o.x, self.y - o.y)
    }

    pub fn scale(self, s: f64) -> V2 {
        v2(self.x * s, self.y * s)
    }

    pub fn magnitude(self) -> f64 {
        hypot(&[self.x, self.y])
    }

    pub fn dot(self, o: V2) -> f64 {
        self.x * o.x + self.y * o.y
    }
}

impl V4 {
    pub const ZERO: V4 = v4(0.0, 0.0, 0.0, 0.0);

    pub fn add(self, o: V4) -> V4 {
        v4(self.x + o.x, self.y + o.y, self.z + o.z, self.w + o.w)
    }

    pub fn sub(self, o: V4) -> V4 {
        v4(self.x - o.x, self.y - o.y, self.z - o.z, self.w - o.w)
    }

    pub fn scale(self, s: f64) -> V4 {
        v4(self.x * s, self.y * s, self.z * s, self.w * s)
    }

    pub fn magnitude(self) -> f64 {
        hypot(&[self.x, self.y, self.z, self.w])
    }
}

/// `${x}` of a number, as `String(x)`.
fn n(x: f64) -> String {
    jsnum::to_string(x)
}

fn opt_eps(e: f64) -> f64 {
    if e.is_nan() { 1e-6 } else { e }
}

// ---- Perlin noise (Ken Perlin's reference table) ----

const PERM_HALF: [u8; 256] = [
    151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69,
    142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219,
    203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175,
    74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230,
    220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76,
    132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173,
    186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206,
    59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163,
    70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232,
    178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162,
    241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204,
    176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141,
    128, 195, 78, 66, 215, 61, 156, 180,
];

fn perm(i: usize) -> usize {
    usize::from(PERM_HALF[i % 256])
}

fn perlin2(x: f64, y: f64) -> f64 {
    // `Math.floor(x) & 255` (ToInt32 of the floor, then the low byte).
    let xi = (jsnum::to_int32(x.floor()) & 255) as usize;
    let yi = (jsnum::to_int32(y.floor()) & 255) as usize;
    let xf = x - x.floor();
    let yf = y - y.floor();
    let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let grad = |h: usize, gx: f64, gy: f64| {
        (if h & 1 == 0 { gx } else { -gx }) + (if h & 2 == 0 { gy } else { -gy })
    };
    let u = fade(xf);
    let v = fade(yf);
    let aa = perm(perm(xi) + yi);
    let ab = perm(perm(xi) + yi + 1);
    let ba = perm(perm(xi + 1) + yi);
    let bb = perm(perm(xi + 1) + yi + 1);
    let x1 = grad(aa, xf, yf) + (grad(ba, xf - 1.0, yf) - grad(aa, xf, yf)) * u;
    let x2 = grad(ab, xf, yf - 1.0) + (grad(bb, xf - 1.0, yf - 1.0) - grad(ab, xf, yf - 1.0)) * u;
    x1 + (x2 - x1) * v
}

#[ts_api(namespace = "Mathf", values(Vector2 = V2 [x: f64, y: f64]))]
pub mod mathf_api {
    use super::{DEG, V2, clamp, clamp01, jsnum, perlin2, v2};
    use std::f64::consts::PI;

    #[property]
    #[js_name = "PI"]
    pub fn pi() -> f64 {
        PI
    }

    #[property]
    #[js_name = "Deg2Rad"]
    pub fn deg2_rad() -> f64 {
        DEG
    }

    #[property]
    #[js_name = "Rad2Deg"]
    pub fn rad2_deg() -> f64 {
        1.0 / DEG
    }

    #[property]
    #[js_name = "Epsilon"]
    pub fn epsilon() -> f64 {
        1e-6
    }

    #[js_name = "clamp"]
    pub fn clamp_value(v: f64, min: f64, max: f64) -> f64 {
        clamp(v, min, max)
    }

    #[js_name = "clamp01"]
    pub fn clamp_01(v: f64) -> f64 {
        clamp01(v)
    }

    pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
        a + (b - a) * clamp01(t)
    }

    pub fn lerp_unclamped(a: f64, b: f64, t: f64) -> f64 {
        a + (b - a) * t
    }

    pub fn inverse_lerp(a: f64, b: f64, v: f64) -> f64 {
        if a == b {
            0.0
        } else {
            clamp01((v - a) / (b - a))
        }
    }

    pub fn move_towards(current: f64, target: f64, max_delta: f64) -> f64 {
        if (target - current).abs() <= max_delta {
            target
        } else {
            current + jsnum::sign(target - current) * max_delta
        }
    }

    pub fn smooth_step(from: f64, to: f64, t: f64) -> f64 {
        let x = clamp01(t);
        from + (to - from) * x * x * (3.0 - 2.0 * x)
    }

    /// The shortest difference between two angles in degrees, -180 to 180.
    pub fn delta_angle(a: f64, b: f64) -> f64 {
        let d = (((b - a) % 360.0) + 540.0) % 360.0 - 180.0;
        if d == -180.0 { 180.0 } else { d }
    }

    pub fn repeat(t: f64, length: f64) -> f64 {
        t - (t / length).floor() * length
    }

    pub fn ping_pong(t: f64, length: f64) -> f64 {
        let r = t - (t / (length * 2.0)).floor() * length * 2.0;
        length - (r - length).abs()
    }

    pub fn approximately(a: f64, b: f64) -> bool {
        (a - b).abs() < jsnum::max(1e-6 * jsnum::max(a.abs(), b.abs()), 1e-6)
    }

    /// 1 for zero or more, -1 below (Unity's `Sign`, which is never 0).
    pub fn sign(v: f64) -> f64 {
        if v >= 0.0 { 1.0 } else { -1.0 }
    }

    /// Lerps between angles in degrees the short way round.
    pub fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
        a + delta_angle(a, b) * clamp01(t)
    }

    /// Turns from `current` towards `target` (degrees) by at most `maxDelta`.
    pub fn move_towards_angle(current: f64, target: f64, max_delta: f64) -> f64 {
        let d = delta_angle(current, target);
        if d.abs() <= max_delta {
            current + d
        } else {
            current + jsnum::sign(d) * max_delta
        }
    }

    /// Eases `current` towards `target` like a critically damped spring, taking about
    /// `smoothTime` seconds. Returns (value, velocity) as a `Vector2`: pass the velocity back in
    /// on the next call.
    #[optional(max_speed)]
    pub fn smooth_damp(
        current: f64,
        target: f64,
        velocity: f64,
        smooth_time: f64,
        delta_time: f64,
        max_speed: f64,
    ) -> V2 {
        let max_speed = if max_speed.is_nan() {
            f64::INFINITY
        } else {
            max_speed
        };
        let st = jsnum::max(0.0001, smooth_time);
        let omega = 2.0 / st;
        let x = omega * delta_time;
        let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
        let max_change = max_speed * st;
        let change = jsnum::min(max_change, jsnum::max(-max_change, current - target));
        let goal = current - change;
        let temp = (velocity + omega * change) * delta_time;
        let mut v = (velocity - omega * temp) * exp;
        let mut out = goal + (change + temp) * exp;
        // Do not overshoot.
        if (target - current > 0.0) == (out > target) {
            out = target;
            v = (out - target) / delta_time;
        }
        v2(out, v)
    }

    /// Smooth 2D noise, about 0 to 1. Same inputs, same output.
    pub fn perlin_noise(x: f64, y: f64) -> f64 {
        perlin2(x, y) * 0.5 + 0.5
    }

    pub fn is_power_of_two(v: f64) -> bool {
        v > 0.0 && (jsnum::to_int32(v) & jsnum::to_int32(v - 1.0)) == 0
    }

    pub fn next_power_of_two(v: f64) -> f64 {
        if v <= 1.0 {
            1.0
        } else {
            2f64.powf(v.log2().ceil())
        }
    }

    pub fn closest_power_of_two(v: f64) -> f64 {
        if v <= 1.0 {
            1.0
        } else {
            2f64.powf(jsnum::round(v.log2()))
        }
    }

    /// sRGB (as written in colour pickers) to linear, per channel.
    pub fn gamma_to_linear_space(c: f64) -> f64 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    pub fn linear_to_gamma_space(c: f64) -> f64 {
        if c <= 0.0031308 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    }
}

// ---- Vector2 ----

#[ts_api(namespace = "Vector2", values(Vector2 = V2 [x: f64, y: f64], Vector3 = V3 [x: f64, y: f64, z: f64]))]
pub mod vector2_api {
    use super::{DEG, V2, V3, clamp, clamp01, hypot, n, opt_eps, v2, v3};

    /// `new Vector2(x, y)`: a missing component is 0.
    #[optional(x, y)]
    pub fn create(x: f64, y: f64) -> V2 {
        v2(
            if x.is_nan() { 0.0 } else { x },
            if y.is_nan() { 0.0 } else { y },
        )
    }

    #[property]
    pub fn zero() -> V2 {
        v2(0.0, 0.0)
    }

    #[property]
    pub fn one() -> V2 {
        v2(1.0, 1.0)
    }

    #[property]
    pub fn up() -> V2 {
        v2(0.0, 1.0)
    }

    #[property]
    pub fn down() -> V2 {
        v2(0.0, -1.0)
    }

    #[property]
    pub fn left() -> V2 {
        v2(-1.0, 0.0)
    }

    #[property]
    pub fn right() -> V2 {
        v2(1.0, 0.0)
    }

    /// The angle between two directions, 0 to 180 degrees.
    pub fn angle(a: V2, b: V2) -> f64 {
        let d = a.magnitude() * b.magnitude();
        if d == 0.0 {
            0.0
        } else {
            clamp(a.dot(b) / d, -1.0, 1.0).acos() / DEG
        }
    }

    /// Signed angle from `a` to `b`, -180 to 180 degrees (counter-clockwise positive).
    pub fn signed_angle(a: V2, b: V2) -> f64 {
        (a.x * b.y - a.y * b.x).atan2(a.x * b.x + a.y * b.y) / DEG
    }

    #[method(Vector2)]
    pub fn add(a: V2, b: V2) -> V2 {
        a.add(b)
    }

    #[method(Vector2)]
    pub fn sub(a: V2, b: V2) -> V2 {
        a.sub(b)
    }

    #[method(Vector2)]
    pub fn scale(a: V2, s: f64) -> V2 {
        a.scale(s)
    }

    /// Component-wise product.
    #[method(Vector2)]
    pub fn mul(a: V2, b: V2) -> V2 {
        v2(a.x * b.x, a.y * b.y)
    }

    #[method(Vector2)]
    pub fn dot(a: V2, b: V2) -> f64 {
        a.dot(b)
    }

    #[getter(Vector2)]
    pub fn magnitude(a: V2) -> f64 {
        a.magnitude()
    }

    #[getter(Vector2)]
    pub fn sqr_magnitude(a: V2) -> f64 {
        a.x * a.x + a.y * a.y
    }

    /// Length 1 in the same direction (zero stays zero).
    #[getter(Vector2)]
    pub fn normalized(a: V2) -> V2 {
        let m = a.magnitude();
        if m > 0.0 { a.scale(1.0 / m) } else { V2::ZERO }
    }

    #[method(Vector2)]
    pub fn distance(a: V2, b: V2) -> f64 {
        hypot(&[a.x - b.x, a.y - b.y])
    }

    #[method(Vector2)]
    pub fn lerp(a: V2, b: V2, t: f64) -> V2 {
        let k = clamp01(t);
        v2(a.x + (b.x - a.x) * k, a.y + (b.y - a.y) * k)
    }

    #[method(Vector2)]
    pub fn move_towards(a: V2, target: V2, max_distance: f64) -> V2 {
        let d = target.sub(a);
        let m = d.magnitude();
        if m <= max_distance || m == 0.0 {
            target
        } else {
            a.add(d.scale(max_distance / m))
        }
    }

    /// Rotated counter-clockwise by `degrees`.
    #[method(Vector2)]
    pub fn rotate(a: V2, degrees: f64) -> V2 {
        let c = (degrees * DEG).cos();
        let s = (degrees * DEG).sin();
        v2(a.x * c - a.y * s, a.x * s + a.y * c)
    }

    #[method(Vector2)]
    #[optional(epsilon)]
    pub fn equals(a: V2, b: V2, epsilon: f64) -> bool {
        let e = opt_eps(epsilon);
        (a.x - b.x).abs() <= e && (a.y - b.y).abs() <= e
    }

    #[method(Vector2)]
    #[optional(z)]
    pub fn to_vector3(a: V2, z: f64) -> V3 {
        v3(a.x, a.y, if z.is_nan() { 0.0 } else { z })
    }

    #[method(Vector2)]
    pub fn to_string(a: V2) -> String {
        format!("({}, {})", n(a.x), n(a.y))
    }
}

// ---- Vector3 ----

#[ts_api(namespace = "Vector3", values(Vector3 = V3 [x: f64, y: f64, z: f64], Vector2 = V2 [x: f64, y: f64]))]
pub mod vector3_api {
    use super::{DEG, V2, V3, clamp, hypot, n, opt_eps, v2, v3};

    /// `new Vector3(x, y, z)`: a missing component is 0.
    #[optional(x, y, z)]
    pub fn create(x: f64, y: f64, z: f64) -> V3 {
        let d = |a: f64| if a.is_nan() { 0.0 } else { a };
        v3(d(x), d(y), d(z))
    }

    #[property]
    pub fn zero() -> V3 {
        v3(0.0, 0.0, 0.0)
    }

    #[property]
    pub fn one() -> V3 {
        v3(1.0, 1.0, 1.0)
    }

    #[property]
    pub fn up() -> V3 {
        v3(0.0, 1.0, 0.0)
    }

    #[property]
    pub fn down() -> V3 {
        v3(0.0, -1.0, 0.0)
    }

    #[property]
    pub fn left() -> V3 {
        v3(-1.0, 0.0, 0.0)
    }

    #[property]
    pub fn right() -> V3 {
        v3(1.0, 0.0, 0.0)
    }

    /// -z: where cameras and objects look with no rotation.
    #[property]
    pub fn forward() -> V3 {
        v3(0.0, 0.0, -1.0)
    }

    #[property]
    pub fn back() -> V3 {
        v3(0.0, 0.0, 1.0)
    }

    /// The angle between two directions, 0 to 180 degrees.
    pub fn angle(a: V3, b: V3) -> f64 {
        let d = a.magnitude() * b.magnitude();
        if d == 0.0 {
            0.0
        } else {
            clamp(a.dot(b) / d, -1.0, 1.0).acos() / DEG
        }
    }

    #[method(Vector3)]
    pub fn add(a: V3, b: V3) -> V3 {
        a.add(b)
    }

    #[method(Vector3)]
    pub fn sub(a: V3, b: V3) -> V3 {
        a.sub(b)
    }

    #[method(Vector3)]
    pub fn scale(a: V3, s: f64) -> V3 {
        a.scale(s)
    }

    /// Component-wise product.
    #[method(Vector3)]
    pub fn mul(a: V3, b: V3) -> V3 {
        v3(a.x * b.x, a.y * b.y, a.z * b.z)
    }

    /// Component-wise quotient.
    #[method(Vector3)]
    pub fn div(a: V3, b: V3) -> V3 {
        v3(a.x / b.x, a.y / b.y, a.z / b.z)
    }

    #[method(Vector3)]
    pub fn negate(a: V3) -> V3 {
        a.negate()
    }

    #[method(Vector3)]
    pub fn dot(a: V3, b: V3) -> f64 {
        a.dot(b)
    }

    #[method(Vector3)]
    pub fn cross(a: V3, b: V3) -> V3 {
        a.cross(b)
    }

    #[getter(Vector3)]
    pub fn magnitude(a: V3) -> f64 {
        a.magnitude()
    }

    #[getter(Vector3)]
    pub fn sqr_magnitude(a: V3) -> f64 {
        a.sqr_magnitude()
    }

    /// Length 1 in the same direction (zero stays zero).
    #[getter(Vector3)]
    pub fn normalized(a: V3) -> V3 {
        a.normalized()
    }

    #[method(Vector3)]
    pub fn distance(a: V3, b: V3) -> f64 {
        hypot(&[a.x - b.x, a.y - b.y, a.z - b.z])
    }

    #[method(Vector3)]
    pub fn lerp(a: V3, b: V3, t: f64) -> V3 {
        a.lerp(b, t)
    }

    #[method(Vector3)]
    pub fn move_towards(a: V3, target: V3, max_distance: f64) -> V3 {
        let d = target.sub(a);
        let m = d.magnitude();
        if m <= max_distance || m == 0.0 {
            target
        } else {
            a.add(d.scale(max_distance / m))
        }
    }

    /// This vector projected onto `onto`'s direction.
    #[method(Vector3)]
    pub fn project(a: V3, onto: V3) -> V3 {
        let sq = onto.sqr_magnitude();
        if sq == 0.0 {
            V3::ZERO
        } else {
            onto.scale(a.dot(onto) / sq)
        }
    }

    /// This vector with its component along `normal` removed.
    #[method(Vector3)]
    pub fn project_on_plane(a: V3, normal: V3) -> V3 {
        let sq = normal.sqr_magnitude();
        let p = if sq == 0.0 {
            V3::ZERO
        } else {
            normal.scale(a.dot(normal) / sq)
        };
        a.sub(p)
    }

    /// Bounced off a surface with this `normal` (unit length).
    #[method(Vector3)]
    pub fn reflect(a: V3, normal: V3) -> V3 {
        a.sub(normal.scale(2.0 * a.dot(normal)))
    }

    /// At most `max` long.
    #[method(Vector3)]
    pub fn clamp_magnitude(a: V3, max: f64) -> V3 {
        let m = a.magnitude();
        if m > max { a.scale(max / m) } else { a }
    }

    #[method(Vector3)]
    #[optional(epsilon)]
    pub fn equals(a: V3, b: V3, epsilon: f64) -> bool {
        let e = opt_eps(epsilon);
        (a.x - b.x).abs() <= e && (a.y - b.y).abs() <= e && (a.z - b.z).abs() <= e
    }

    #[method(Vector3)]
    pub fn to_vector2(a: V3) -> V2 {
        v2(a.x, a.y)
    }

    #[method(Vector3)]
    pub fn to_string(a: V3) -> String {
        format!("({}, {}, {})", n(a.x), n(a.y), n(a.z))
    }
}

// ---- Vector4 ----

#[ts_api(namespace = "Vector4", values(Vector4 = V4 [x: f64, y: f64, z: f64, w: f64], Vector3 = V3 [x: f64, y: f64, z: f64]))]
pub mod vector4_api {
    use super::{V3, V4, clamp01, n, opt_eps, v3, v4};

    /// `new Vector4(x, y, z, w)`: a missing component is 0.
    #[optional(x, y, z, w)]
    pub fn create(x: f64, y: f64, z: f64, w: f64) -> V4 {
        let d = |a: f64| if a.is_nan() { 0.0 } else { a };
        v4(d(x), d(y), d(z), d(w))
    }

    #[property]
    pub fn zero() -> V4 {
        v4(0.0, 0.0, 0.0, 0.0)
    }

    #[property]
    pub fn one() -> V4 {
        v4(1.0, 1.0, 1.0, 1.0)
    }

    /// A point (`w` 1) or, with `w` 0, a direction.
    #[optional(w)]
    pub fn from_vector3(v: V3, w: f64) -> V4 {
        v4(v.x, v.y, v.z, if w.is_nan() { 1.0 } else { w })
    }

    #[method(Vector4)]
    pub fn add(a: V4, b: V4) -> V4 {
        a.add(b)
    }

    #[method(Vector4)]
    pub fn sub(a: V4, b: V4) -> V4 {
        a.sub(b)
    }

    #[method(Vector4)]
    pub fn scale(a: V4, s: f64) -> V4 {
        a.scale(s)
    }

    /// Component-wise product.
    #[method(Vector4)]
    pub fn mul(a: V4, b: V4) -> V4 {
        v4(a.x * b.x, a.y * b.y, a.z * b.z, a.w * b.w)
    }

    #[method(Vector4)]
    pub fn dot(a: V4, b: V4) -> f64 {
        a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w
    }

    #[getter(Vector4)]
    pub fn magnitude(a: V4) -> f64 {
        a.magnitude()
    }

    #[getter(Vector4)]
    pub fn normalized(a: V4) -> V4 {
        let m = a.magnitude();
        if m > 0.0 { a.scale(1.0 / m) } else { V4::ZERO }
    }

    #[method(Vector4)]
    pub fn lerp(a: V4, b: V4, t: f64) -> V4 {
        a.add(b.sub(a).scale(clamp01(t)))
    }

    #[method(Vector4)]
    #[optional(epsilon)]
    pub fn equals(a: V4, b: V4, epsilon: f64) -> bool {
        a.sub(b).magnitude() <= opt_eps(epsilon)
    }

    #[method(Vector4)]
    pub fn to_vector3(a: V4) -> V3 {
        v3(a.x, a.y, a.z)
    }

    #[method(Vector4)]
    pub fn to_string(a: V4) -> String {
        format!("({}, {}, {}, {})", n(a.x), n(a.y), n(a.z), n(a.w))
    }
}
