// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Quaternion` (Euler angles in degrees) and `Matrix4x4` (column-major, right-handed, depth 0 to
//! 1) as `js/engine/src/math.ts` has them, computed in the same order.
//!
//! Differences: `new Matrix4x4(array)` and `m` are not there (`Matrix4x4.fromColumns` and `get`
//! are); `Quaternion.from` and `toArray` are not there.

use crate::jsnum::{self, clamp, clamp01, hypot};
use crate::math::{DEG, V3, V4, v3, v4};
use ts_api::{ValueType, ts_api};

/// `Quaternion`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Q {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl ValueType<4> for Q {
    fn from_parts(p: [f64; 4]) -> Self {
        Q {
            x: p[0],
            y: p[1],
            z: p[2],
            w: p[3],
        }
    }

    fn to_parts(self) -> [f64; 4] {
        [self.x, self.y, self.z, self.w]
    }
}

pub const fn q(x: f64, y: f64, z: f64, w: f64) -> Q {
    Q { x, y, z, w }
}

impl Q {
    pub const IDENTITY: Q = q(0.0, 0.0, 0.0, 1.0);

    pub fn angle_axis(degrees: f64, axis: V3) -> Q {
        let a = axis.normalized();
        let half = (degrees * DEG) / 2.0;
        let s = half.sin();
        q(a.x * s, a.y * s, a.z * s, half.cos())
    }

    pub fn euler(x: f64, y: f64, z: f64) -> Q {
        Q::angle_axis(y, V3::UP)
            .mul(Q::angle_axis(x, V3::RIGHT))
            .mul(Q::angle_axis(z, v3(0.0, 0.0, 1.0)))
    }

    /// `this * o`: `o` applies first.
    pub fn mul(self, o: Q) -> Q {
        q(
            self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
            self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
        )
    }

    pub fn inverse(self) -> Q {
        q(-self.x, -self.y, -self.z, self.w)
    }

    pub fn rotate(self, v: V3) -> V3 {
        // v + 2w(q × v) + 2(q × (q × v))
        let qv = v3(self.x, self.y, self.z);
        let t = qv.cross(v).scale(2.0);
        v.add(t.scale(self.w)).add(qv.cross(t))
    }

    pub fn normalized(self) -> Q {
        let len = hypot(&[self.x, self.y, self.z, self.w]);
        if len > 0.0 {
            q(self.x / len, self.y / len, self.z / len, self.w / len)
        } else {
            Q::IDENTITY
        }
    }

    pub fn slerp(self, o: Q, t: f64) -> Q {
        let k = clamp01(t);
        let (mut x, mut y, mut z, mut w) = (o.x, o.y, o.z, o.w);
        let mut cos = self.x * x + self.y * y + self.z * z + self.w * w;
        if cos < 0.0 {
            (x, y, z, w, cos) = (-x, -y, -z, -w, -cos);
        }
        if cos > 0.9995 {
            return q(
                self.x + (x - self.x) * k,
                self.y + (y - self.y) * k,
                self.z + (z - self.z) * k,
                self.w + (w - self.w) * k,
            )
            .normalized();
        }
        let angle = cos.acos();
        let a = ((1.0 - k) * angle).sin() / angle.sin();
        let b = (k * angle).sin() / angle.sin();
        q(
            self.x * a + x * b,
            self.y * a + y * b,
            self.z * a + z * b,
            self.w * a + w * b,
        )
    }

    pub fn angle(a: Q, b: Q) -> f64 {
        let d = (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w).abs();
        (2.0 * jsnum::min(1.0, d).acos()) / DEG
    }

    /// Euler angles in degrees (each 0 to 360), the inverse of `euler`.
    pub fn euler_angles(self) -> V3 {
        let (x, y, z, w) = (self.x, self.y, self.z, self.w);
        // Rotation matrix terms for the YXZ order.
        let m12 = 2.0 * (y * z - w * x);
        let sx = clamp(-m12, -1.0, 1.0);
        let ax = sx.asin();
        let (ay, az);
        if sx.abs() < 0.9999 {
            ay = (2.0 * (x * z + w * y)).atan2(1.0 - 2.0 * (x * x + y * y));
            az = (2.0 * (x * y + w * z)).atan2(1.0 - 2.0 * (x * x + z * z));
        } else {
            // Gimbal lock: put it all in yaw.
            ay = (-2.0 * (x * z - w * y)).atan2(1.0 - 2.0 * (y * y + z * z));
            az = 0.0;
        }
        let repeat = |t: f64, length: f64| t - (t / length).floor() * length;
        let wrap = |r: f64| repeat(r / DEG, 360.0);
        v3(wrap(ax), wrap(ay), wrap(az))
    }

    pub fn look_rotation(direction: V3, up: V3) -> Q {
        let f = direction.normalized();
        if f.sqr_magnitude() == 0.0 {
            return Q::IDENTITY;
        }
        let mut r = f.cross(up).normalized();
        if r.sqr_magnitude() == 0.0 {
            r = f
                .cross(if f.x.abs() < 0.9 { V3::RIGHT } else { V3::UP })
                .normalized();
        }
        let u = r.cross(f);
        // Matrix columns: right, up, back (-forward).
        let (m00, m01, m02) = (r.x, u.x, -f.x);
        let (m10, m11, m12) = (r.y, u.y, -f.y);
        let (m20, m21, m22) = (r.z, u.z, -f.z);
        let trace = m00 + m11 + m22;
        if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            return q((m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, s / 4.0);
        }
        if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            return q(s / 4.0, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s);
        }
        if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            return q((m01 + m10) / s, s / 4.0, (m12 + m21) / s, (m02 - m20) / s);
        }
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        q((m02 + m20) / s, (m12 + m21) / s, s / 4.0, (m10 - m01) / s)
    }

    pub fn from_to_rotation(from: V3, to: V3) -> Q {
        let a = from.normalized();
        let b = to.normalized();
        let d = a.dot(b);
        if d < -0.999999 {
            let mut axis = V3::RIGHT.cross(a);
            if axis.sqr_magnitude() < 1e-6 {
                axis = V3::UP.cross(a);
            }
            return Q::angle_axis(180.0, axis);
        }
        let c = a.cross(b);
        q(c.x, c.y, c.z, 1.0 + d).normalized()
    }
}

#[ts_api(namespace = "Quaternion", values(
    Quaternion = Q [x: f64, y: f64, z: f64, w: f64],
    Vector3 = V3 [x: f64, y: f64, z: f64]))]
pub mod quaternion_api {
    use super::{Q, V3, jsnum, q};

    /// `new Quaternion(x, y, z, w)`: a missing component is 0, except `w` which is 1.
    #[optional(x, y, z, w)]
    pub fn create(x: f64, y: f64, z: f64, w: f64) -> Q {
        let d = |a: f64, dflt: f64| if a.is_nan() { dflt } else { a };
        q(d(x, 0.0), d(y, 0.0), d(z, 0.0), d(w, 1.0))
    }

    #[property]
    pub fn identity() -> Q {
        Q::IDENTITY
    }

    /// A rotation of `degrees` about `axis`.
    pub fn angle_axis(degrees: f64, axis: V3) -> Q {
        Q::angle_axis(degrees, axis)
    }

    /// Euler angles in degrees, like Unity: `z` about the forward axis, then `x` about the right
    /// axis, then `y` about the up axis (roll, pitch, yaw).
    #[js_name = "euler"]
    pub fn euler_xyz(x: f64, y: f64, z: f64) -> Q {
        Q::euler(x, y, z)
    }

    #[js_name = "euler"]
    pub fn euler_vector(angles: V3) -> Q {
        Q::euler(angles.x, angles.y, angles.z)
    }

    /// A rotation that points forward (-z) along `direction`, keeping `up` as close to up as it
    /// can.
    #[js_name = "lookRotation"]
    pub fn look_rotation_up(direction: V3, up: V3) -> Q {
        Q::look_rotation(direction, up)
    }

    #[js_name = "lookRotation"]
    pub fn look_rotation(direction: V3) -> Q {
        Q::look_rotation(direction, V3::UP)
    }

    /// The rotation from one direction to another.
    pub fn from_to_rotation(from: V3, to: V3) -> Q {
        Q::from_to_rotation(from, to)
    }

    /// The angle between two rotations, in degrees.
    pub fn angle(a: Q, b: Q) -> f64 {
        Q::angle(a, b)
    }

    /// This rotation as Euler angles in degrees (each 0 to 360), the inverse of `euler`.
    #[getter(Quaternion)]
    pub fn euler_angles(a: Q) -> V3 {
        a.euler_angles()
    }

    /// Rotation `b` followed by this one (like multiplying matrices: `this * b`).
    #[method(Quaternion)]
    pub fn mul(a: Q, b: Q) -> Q {
        a.mul(b)
    }

    /// The opposite rotation (for unit quaternions).
    #[method(Quaternion)]
    pub fn inverse(a: Q) -> Q {
        a.inverse()
    }

    /// Rotates a vector.
    #[method(Quaternion)]
    pub fn rotate(a: Q, v: V3) -> V3 {
        a.rotate(v)
    }

    #[getter(Quaternion)]
    pub fn normalized(a: Q) -> Q {
        a.normalized()
    }

    /// Spherical interpolation, the short way round.
    #[method(Quaternion)]
    pub fn slerp(a: Q, b: Q, t: f64) -> Q {
        a.slerp(b, t)
    }

    /// Turns towards `target` by at most `maxDegrees`.
    #[method(Quaternion)]
    pub fn rotate_towards(a: Q, target: Q, max_degrees: f64) -> Q {
        let angle = Q::angle(a, target);
        if angle == 0.0 {
            target
        } else {
            a.slerp(target, jsnum::min(1.0, max_degrees / angle))
        }
    }
}

// ---- Matrix4x4 ----

/// `Matrix4x4`: 16 numbers, column by column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M4 {
    pub m: [f64; 16],
}

impl ValueType<16> for M4 {
    fn from_parts(p: [f64; 16]) -> Self {
        M4 { m: p }
    }

    fn to_parts(self) -> [f64; 16] {
        self.m
    }
}

impl M4 {
    pub const IDENTITY: M4 = M4 {
        m: [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    };

    pub fn trs(position: V3, rotation: Q, scale: V3) -> M4 {
        let Q { x, y, z, w } = rotation;
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz, wx, wy, wz) = (x * y, x * z, y * z, w * x, w * y, w * z);
        M4 {
            m: [
                (1.0 - 2.0 * (yy + zz)) * scale.x,
                2.0 * (xy + wz) * scale.x,
                2.0 * (xz - wy) * scale.x,
                0.0,
                2.0 * (xy - wz) * scale.y,
                (1.0 - 2.0 * (xx + zz)) * scale.y,
                2.0 * (yz + wx) * scale.y,
                0.0,
                2.0 * (xz + wy) * scale.z,
                2.0 * (yz - wx) * scale.z,
                (1.0 - 2.0 * (xx + yy)) * scale.z,
                0.0,
                position.x,
                position.y,
                position.z,
                1.0,
            ],
        }
    }

    /// `self * o`: `o` applies first.
    pub fn mul(self, o: M4) -> M4 {
        let (a, b) = (self.m, o.m);
        let mut out = [0.0; 16];
        for c in 0..4 {
            for r in 0..4 {
                out[c * 4 + r] = a[r] * b[c * 4]
                    + a[4 + r] * b[c * 4 + 1]
                    + a[8 + r] * b[c * 4 + 2]
                    + a[12 + r] * b[c * 4 + 3];
            }
        }
        M4 { m: out }
    }

    pub fn inverse(self) -> M4 {
        let m = self.m;
        let mut inv = [0.0; 16];
        inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
            + m[9] * m[7] * m[14]
            + m[13] * m[6] * m[11]
            - m[13] * m[7] * m[10];
        inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
            - m[8] * m[7] * m[14]
            - m[12] * m[6] * m[11]
            + m[12] * m[7] * m[10];
        inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
            + m[8] * m[7] * m[13]
            + m[12] * m[5] * m[11]
            - m[12] * m[7] * m[9];
        inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
            - m[8] * m[6] * m[13]
            - m[12] * m[5] * m[10]
            + m[12] * m[6] * m[9];
        inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
            - m[9] * m[3] * m[14]
            - m[13] * m[2] * m[11]
            + m[13] * m[3] * m[10];
        inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
            + m[8] * m[3] * m[14]
            + m[12] * m[2] * m[11]
            - m[12] * m[3] * m[10];
        inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
            - m[8] * m[3] * m[13]
            - m[12] * m[1] * m[11]
            + m[12] * m[3] * m[9];
        inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
            + m[8] * m[2] * m[13]
            + m[12] * m[1] * m[10]
            - m[12] * m[2] * m[9];
        inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
            + m[5] * m[3] * m[14]
            + m[13] * m[2] * m[7]
            - m[13] * m[3] * m[6];
        inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
            - m[4] * m[3] * m[14]
            - m[12] * m[2] * m[7]
            + m[12] * m[3] * m[6];
        inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
            + m[4] * m[3] * m[13]
            + m[12] * m[1] * m[7]
            - m[12] * m[3] * m[5];
        inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
            - m[4] * m[2] * m[13]
            - m[12] * m[1] * m[6]
            + m[12] * m[2] * m[5];
        inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
            - m[5] * m[3] * m[10]
            - m[9] * m[2] * m[7]
            + m[9] * m[3] * m[6];
        inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
            + m[4] * m[3] * m[10]
            + m[8] * m[2] * m[7]
            - m[8] * m[3] * m[6];
        inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
            - m[4] * m[3] * m[9]
            - m[8] * m[1] * m[7]
            + m[8] * m[3] * m[5];
        inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
            + m[4] * m[2] * m[9]
            + m[8] * m[1] * m[6]
            - m[8] * m[2] * m[5];
        let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
        if det.abs() < 1e-12 {
            return M4::IDENTITY;
        }
        M4 {
            m: inv.map(|v| v / det),
        }
    }

    pub fn determinant(self) -> f64 {
        let m = self.m;
        let sub = |a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64, i: f64| {
            a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
        };
        m[0] * sub(m[5], m[9], m[13], m[6], m[10], m[14], m[7], m[11], m[15])
            - m[4] * sub(m[1], m[9], m[13], m[2], m[10], m[14], m[3], m[11], m[15])
            + m[8] * sub(m[1], m[5], m[13], m[2], m[6], m[14], m[3], m[7], m[15])
            - m[12] * sub(m[1], m[5], m[9], m[2], m[6], m[10], m[3], m[7], m[11])
    }

    pub fn transpose(self) -> M4 {
        let mut out = [0.0; 16];
        for r in 0..4 {
            for c in 0..4 {
                out[r * 4 + c] = self.m[c * 4 + r];
            }
        }
        M4 { m: out }
    }
}

#[ts_api(namespace = "Matrix4x4", values(
    Matrix4x4 = M4 [m0: f64, m1: f64, m2: f64, m3: f64, m4: f64, m5: f64, m6: f64, m7: f64,
                    m8: f64, m9: f64, m10: f64, m11: f64, m12: f64, m13: f64, m14: f64, m15: f64],
    Vector3 = V3 [x: f64, y: f64, z: f64],
    Vector4 = V4 [x: f64, y: f64, z: f64, w: f64],
    Quaternion = Q [x: f64, y: f64, z: f64, w: f64]))]
pub mod matrix4x4_api {
    use super::{DEG, M4, Q, V3, V4, v3, v4};

    #[property]
    pub fn identity() -> M4 {
        M4::IDENTITY
    }

    /// The matrix with these four columns.
    pub fn from_columns(c0: V4, c1: V4, c2: V4, c3: V4) -> M4 {
        M4 {
            m: [
                c0.x, c0.y, c0.z, c0.w, c1.x, c1.y, c1.z, c1.w, c2.x, c2.y, c2.z, c2.w, c3.x, c3.y,
                c3.z, c3.w,
            ],
        }
    }

    /// Translation, rotation and scale (applied scale, then rotation, then translation).
    #[js_name = "trs"]
    pub fn trs_scaled(position: V3, rotation: Q, scale: V3) -> M4 {
        M4::trs(position, rotation, scale)
    }

    #[js_name = "trs"]
    pub fn trs(position: V3, rotation: Q) -> M4 {
        M4::trs(position, rotation, v3(1.0, 1.0, 1.0))
    }

    /// A perspective projection: vertical field of view in degrees.
    pub fn perspective(fov_degrees: f64, aspect: f64, near: f64, far: f64) -> M4 {
        let f = 1.0 / ((fov_degrees * DEG) / 2.0).tan();
        let r = far / (near - far);
        M4 {
            m: [
                f / aspect,
                0.0,
                0.0,
                0.0,
                0.0,
                f,
                0.0,
                0.0,
                0.0,
                0.0,
                r,
                -1.0,
                0.0,
                0.0,
                r * near,
                0.0,
            ],
        }
    }

    /// An orthographic projection of the box between the planes.
    pub fn orthographic(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) -> M4 {
        let rw = 1.0 / (right - left);
        let rh = 1.0 / (top - bottom);
        let r = 1.0 / (near - far);
        M4 {
            m: [
                2.0 * rw,
                0.0,
                0.0,
                0.0,
                0.0,
                2.0 * rh,
                0.0,
                0.0,
                0.0,
                0.0,
                r,
                0.0,
                -(left + right) * rw,
                -(top + bottom) * rh,
                r * near,
                1.0,
            ],
        }
    }

    /// Element at `row`, `col`.
    #[method(Matrix4x4)]
    pub fn get(a: M4, row: f64, col: f64) -> f64 {
        let i = col * 4.0 + row;
        if i >= 0.0 && i.fract() == 0.0 && i < 16.0 {
            a.m[i as usize]
        } else {
            f64::NAN
        }
    }

    /// `this * other`: `other` applies first.
    #[method(Matrix4x4)]
    pub fn mul(a: M4, b: M4) -> M4 {
        a.mul(b)
    }

    /// The inverse (the identity if it has none; check `determinant`).
    #[getter(Matrix4x4)]
    pub fn inverse(a: M4) -> M4 {
        a.inverse()
    }

    #[getter(Matrix4x4)]
    pub fn determinant(a: M4) -> f64 {
        a.determinant()
    }

    #[getter(Matrix4x4)]
    pub fn transpose(a: M4) -> M4 {
        a.transpose()
    }

    /// A point through the matrix, divided by w (for projections).
    #[method(Matrix4x4)]
    pub fn multiply_point(a: M4, p: V3) -> V3 {
        let m = a.m;
        let x = m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12];
        let y = m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13];
        let z = m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14];
        let w = m[3] * p.x + m[7] * p.y + m[11] * p.z + m[15];
        if w != 0.0 && w != 1.0 {
            v3(x / w, y / w, z / w)
        } else {
            v3(x, y, z)
        }
    }

    /// A point through the matrix, ignoring projection (faster, for transforms).
    #[method(Matrix4x4)]
    #[js_name = "multiplyPoint3x4"]
    pub fn multiply_point_3x4(a: M4, p: V3) -> V3 {
        let m = a.m;
        v3(
            m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12],
            m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
            m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14],
        )
    }

    /// A direction through the matrix (no translation).
    #[method(Matrix4x4)]
    pub fn multiply_vector(a: M4, v: V3) -> V3 {
        let m = a.m;
        v3(
            m[0] * v.x + m[4] * v.y + m[8] * v.z,
            m[1] * v.x + m[5] * v.y + m[9] * v.z,
            m[2] * v.x + m[6] * v.y + m[10] * v.z,
        )
    }

    /// The full 4D product (w kept).
    #[method(Matrix4x4)]
    #[js_name = "multiplyVector4"]
    pub fn multiply_vector4(a: M4, v: V4) -> V4 {
        let m = a.m;
        v4(
            m[0] * v.x + m[4] * v.y + m[8] * v.z + m[12] * v.w,
            m[1] * v.x + m[5] * v.y + m[9] * v.z + m[13] * v.w,
            m[2] * v.x + m[6] * v.y + m[10] * v.z + m[14] * v.w,
            m[3] * v.x + m[7] * v.y + m[11] * v.z + m[15] * v.w,
        )
    }
}
