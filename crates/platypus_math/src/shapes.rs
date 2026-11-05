// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Vector2Int`, `Vector3Int`, `Rect`, `RectInt`, `Ray`, `Plane`, `Bounds` and `Pose` as
//! `js/engine/src/math.ts` has them. The classes with vector members (`Ray.origin`, `Bounds.size`)
//! keep them as flat numbers and give the vectors back from getters, so `ray.origin` is a new
//! `Vector3` each time.
//!
//! Differences: `Plane.raycast` / `intersect` answer NaN (a vector of NaNs) where the TypeScript
//! returned `null`; `RectInt.positions()` (a generator) is not there; the `...Like` parameters are
//! the classes.

use crate::jsnum;
use crate::math::{V2, V3, v2, v3};
use crate::quat::{Q, q};
use ts_api::{ValueType, ts_api};

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
    /// `Vector2Int`: whole numbers.
    V2I { x, y } 2
);
value!(
    /// `Vector3Int`.
    V3I { x, y, z } 3
);
value!(
    /// `Rect`: top-left and size.
    Rect { x, y, width, height } 4
);
value!(
    /// `Ray`: origin and unit direction.
    Ray { ox, oy, oz, dx, dy, dz } 6
);
value!(
    /// `Plane`: unit normal and distance.
    Plane { nx, ny, nz, distance } 4
);
value!(
    /// `Bounds`: centre and size.
    Bounds { cx, cy, cz, sx, sy, sz } 6
);
value!(
    /// `Pose`: position and rotation.
    Pose { px, py, pz, qx, qy, qz, qw } 7
);

impl Ray {
    fn origin(self) -> V3 {
        v3(self.ox, self.oy, self.oz)
    }

    fn direction(self) -> V3 {
        v3(self.dx, self.dy, self.dz)
    }

    fn point(self, distance: f64) -> V3 {
        self.origin().add(self.direction().scale(distance))
    }
}

impl Plane {
    fn normal(self) -> V3 {
        v3(self.nx, self.ny, self.nz)
    }

    fn make(normal: V3, distance: f64) -> Plane {
        let n = normal.normalized();
        Plane {
            nx: n.x,
            ny: n.y,
            nz: n.z,
            distance,
        }
    }

    fn from_point(normal: V3, point: V3) -> Plane {
        let n = normal.normalized();
        Plane::make(n, -n.dot(point))
    }

    fn distance_to(self, p: V3) -> f64 {
        self.normal().dot(p) + self.distance
    }
}

impl Bounds {
    fn center(self) -> V3 {
        v3(self.cx, self.cy, self.cz)
    }

    fn size(self) -> V3 {
        v3(self.sx, self.sy, self.sz)
    }

    fn extents(self) -> V3 {
        self.size().scale(0.5)
    }

    fn min(self) -> V3 {
        self.center().sub(self.extents())
    }

    fn max(self) -> V3 {
        self.center().add(self.extents())
    }

    fn min_max(min: V3, max: V3) -> Bounds {
        let c = min.add(max).scale(0.5);
        let s = max.sub(min);
        Bounds {
            cx: c.x,
            cy: c.y,
            cz: c.z,
            sx: s.x,
            sy: s.y,
            sz: s.z,
        }
    }

    fn make(center: V3, size: V3) -> Bounds {
        Bounds {
            cx: center.x,
            cy: center.y,
            cz: center.z,
            sx: size.x,
            sy: size.y,
            sz: size.z,
        }
    }
}

impl Pose {
    fn position(self) -> V3 {
        v3(self.px, self.py, self.pz)
    }

    fn rotation(self) -> Q {
        q(self.qx, self.qy, self.qz, self.qw)
    }

    fn make(position: V3, rotation: Q) -> Pose {
        Pose {
            px: position.x,
            py: position.y,
            pz: position.z,
            qx: rotation.x,
            qy: rotation.y,
            qz: rotation.z,
            qw: rotation.w,
        }
    }
}

fn dflt(x: f64, d: f64) -> f64 {
    if x.is_nan() { d } else { x }
}

fn trunc(x: f64) -> f64 {
    x.trunc()
}

fn n(x: f64) -> String {
    jsnum::to_string(x)
}

// ---- Vector2Int / Vector3Int ----

#[ts_api(namespace = "Vector2Int", values(
    Vector2Int = V2I [x: f64, y: f64],
    Vector2 = V2 [x: f64, y: f64]))]
pub mod vector2_int_api {
    use super::{V2, V2I, dflt, jsnum, n, trunc, v2};

    /// `new Vector2Int(x, y)`: the components are truncated to whole numbers.
    #[optional(x, y)]
    pub fn create(x: f64, y: f64) -> V2I {
        V2I {
            x: trunc(dflt(x, 0.0)),
            y: trunc(dflt(y, 0.0)),
        }
    }

    #[property]
    pub fn zero() -> V2I {
        V2I { x: 0.0, y: 0.0 }
    }

    #[property]
    pub fn one() -> V2I {
        V2I { x: 1.0, y: 1.0 }
    }

    #[property]
    pub fn up() -> V2I {
        V2I { x: 0.0, y: 1.0 }
    }

    #[property]
    pub fn down() -> V2I {
        V2I { x: 0.0, y: -1.0 }
    }

    #[property]
    pub fn left() -> V2I {
        V2I { x: -1.0, y: 0.0 }
    }

    #[property]
    pub fn right() -> V2I {
        V2I { x: 1.0, y: 0.0 }
    }

    pub fn floor(v: V2) -> V2I {
        V2I {
            x: trunc(v.x.floor()),
            y: trunc(v.y.floor()),
        }
    }

    pub fn round(v: V2) -> V2I {
        V2I {
            x: trunc(jsnum::round(v.x)),
            y: trunc(jsnum::round(v.y)),
        }
    }

    #[method(Vector2Int)]
    pub fn add(a: V2I, b: V2) -> V2I {
        V2I {
            x: trunc(a.x + b.x),
            y: trunc(a.y + b.y),
        }
    }

    #[method(Vector2Int)]
    pub fn sub(a: V2I, b: V2) -> V2I {
        V2I {
            x: trunc(a.x - b.x),
            y: trunc(a.y - b.y),
        }
    }

    #[method(Vector2Int)]
    pub fn scale(a: V2I, s: f64) -> V2I {
        V2I {
            x: trunc(a.x * s),
            y: trunc(a.y * s),
        }
    }

    /// Grid distance in steps along the axes.
    #[method(Vector2Int)]
    pub fn manhattan(a: V2I, b: V2) -> f64 {
        (a.x - b.x).abs() + (a.y - b.y).abs()
    }

    #[method(Vector2Int)]
    pub fn equals(a: V2I, b: V2) -> bool {
        a.x == b.x && a.y == b.y
    }

    #[method(Vector2Int)]
    pub fn to_vector2(a: V2I) -> V2 {
        v2(a.x, a.y)
    }

    #[method(Vector2Int)]
    pub fn to_string(a: V2I) -> String {
        format!("({}, {})", n(a.x), n(a.y))
    }
}

#[ts_api(namespace = "Vector3Int", values(
    Vector3Int = V3I [x: f64, y: f64, z: f64],
    Vector3 = V3 [x: f64, y: f64, z: f64]))]
pub mod vector3_int_api {
    use super::{V3, V3I, dflt, jsnum, n, trunc, v3};

    /// `new Vector3Int(x, y, z)`: the components are truncated to whole numbers.
    #[optional(x, y, z)]
    pub fn create(x: f64, y: f64, z: f64) -> V3I {
        V3I {
            x: trunc(dflt(x, 0.0)),
            y: trunc(dflt(y, 0.0)),
            z: trunc(dflt(z, 0.0)),
        }
    }

    #[property]
    pub fn zero() -> V3I {
        V3I {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    #[property]
    pub fn one() -> V3I {
        V3I {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        }
    }

    pub fn floor(v: V3) -> V3I {
        V3I {
            x: trunc(v.x.floor()),
            y: trunc(v.y.floor()),
            z: trunc(v.z.floor()),
        }
    }

    pub fn round(v: V3) -> V3I {
        V3I {
            x: trunc(jsnum::round(v.x)),
            y: trunc(jsnum::round(v.y)),
            z: trunc(jsnum::round(v.z)),
        }
    }

    #[method(Vector3Int)]
    pub fn add(a: V3I, b: V3) -> V3I {
        V3I {
            x: trunc(a.x + b.x),
            y: trunc(a.y + b.y),
            z: trunc(a.z + b.z),
        }
    }

    #[method(Vector3Int)]
    pub fn sub(a: V3I, b: V3) -> V3I {
        V3I {
            x: trunc(a.x - b.x),
            y: trunc(a.y - b.y),
            z: trunc(a.z - b.z),
        }
    }

    #[method(Vector3Int)]
    pub fn equals(a: V3I, b: V3) -> bool {
        a.x == b.x && a.y == b.y && a.z == b.z
    }

    #[method(Vector3Int)]
    pub fn to_vector3(a: V3I) -> V3 {
        v3(a.x, a.y, a.z)
    }

    #[method(Vector3Int)]
    pub fn to_string(a: V3I) -> String {
        format!("({}, {}, {})", n(a.x), n(a.y), n(a.z))
    }
}

// ---- Rect / RectInt ----

#[ts_api(namespace = "Rect", values(
    Rect = Rect [x: f64, y: f64, width: f64, height: f64],
    Vector2 = V2 [x: f64, y: f64]))]
pub mod rect_api {
    use super::{Rect, V2, dflt, v2};

    #[optional(x, y, width, height)]
    pub fn create(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x: dflt(x, 0.0),
            y: dflt(y, 0.0),
            width: dflt(width, 0.0),
            height: dflt(height, 0.0),
        }
    }

    pub fn min_max(min: V2, max: V2) -> Rect {
        Rect {
            x: min.x,
            y: min.y,
            width: max.x - min.x,
            height: max.y - min.y,
        }
    }

    #[getter(Rect)]
    pub fn x_min(r: Rect) -> f64 {
        r.x
    }

    #[getter(Rect)]
    pub fn y_min(r: Rect) -> f64 {
        r.y
    }

    #[getter(Rect)]
    pub fn x_max(r: Rect) -> f64 {
        r.x + r.width
    }

    #[getter(Rect)]
    pub fn y_max(r: Rect) -> f64 {
        r.y + r.height
    }

    #[getter(Rect)]
    pub fn center(r: Rect) -> V2 {
        v2(r.x + r.width / 2.0, r.y + r.height / 2.0)
    }

    #[getter(Rect)]
    pub fn size(r: Rect) -> V2 {
        v2(r.width, r.height)
    }

    #[method(Rect)]
    pub fn contains(r: Rect, p: V2) -> bool {
        p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height
    }

    #[method(Rect)]
    pub fn overlaps(r: Rect, o: Rect) -> bool {
        o.x + o.width > r.x && o.x < r.x + r.width && o.y + o.height > r.y && o.y < r.y + r.height
    }
}

#[ts_api(namespace = "RectInt", values(
    RectInt = Rect [x: f64, y: f64, width: f64, height: f64],
    Vector2 = V2 [x: f64, y: f64]))]
pub mod rect_int_api {
    use super::{Rect, V2, dflt, trunc, v2};

    /// `new RectInt(x, y, width, height)`: whole numbers.
    #[optional(x, y, width, height)]
    pub fn create(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x: trunc(dflt(x, 0.0)),
            y: trunc(dflt(y, 0.0)),
            width: trunc(dflt(width, 0.0)),
            height: trunc(dflt(height, 0.0)),
        }
    }

    pub fn min_max(min: V2, max: V2) -> Rect {
        Rect {
            x: min.x,
            y: min.y,
            width: max.x - min.x,
            height: max.y - min.y,
        }
    }

    #[getter(RectInt)]
    pub fn x_min(r: Rect) -> f64 {
        r.x
    }

    #[getter(RectInt)]
    pub fn y_min(r: Rect) -> f64 {
        r.y
    }

    #[getter(RectInt)]
    pub fn x_max(r: Rect) -> f64 {
        r.x + r.width
    }

    #[getter(RectInt)]
    pub fn y_max(r: Rect) -> f64 {
        r.y + r.height
    }

    #[getter(RectInt)]
    pub fn center(r: Rect) -> V2 {
        v2(r.x + r.width / 2.0, r.y + r.height / 2.0)
    }

    #[getter(RectInt)]
    pub fn size(r: Rect) -> V2 {
        v2(r.width, r.height)
    }

    #[method(RectInt)]
    pub fn contains(r: Rect, p: V2) -> bool {
        p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height
    }

    #[method(RectInt)]
    pub fn overlaps(r: Rect, o: Rect) -> bool {
        o.x + o.width > r.x && o.x < r.x + r.width && o.y + o.height > r.y && o.y < r.y + r.height
    }
}

// ---- Ray / Plane ----

#[ts_api(namespace = "Ray", values(
    Ray = Ray [ox: f64, oy: f64, oz: f64, dx: f64, dy: f64, dz: f64],
    Vector3 = V3 [x: f64, y: f64, z: f64]))]
pub mod ray_api {
    use super::{Ray, V3};

    /// A ray: an origin and a direction (made unit length).
    pub fn create(origin: V3, direction: V3) -> Ray {
        let d = direction.normalized();
        Ray {
            ox: origin.x,
            oy: origin.y,
            oz: origin.z,
            dx: d.x,
            dy: d.y,
            dz: d.z,
        }
    }

    #[getter(Ray)]
    pub fn origin(r: Ray) -> V3 {
        r.origin()
    }

    #[getter(Ray)]
    pub fn direction(r: Ray) -> V3 {
        r.direction()
    }

    #[method(Ray)]
    pub fn get_point(r: Ray, distance: f64) -> V3 {
        r.point(distance)
    }
}

#[ts_api(namespace = "Plane", values(
    Plane = Plane [nx: f64, ny: f64, nz: f64, distance: f64],
    Ray = Ray [ox: f64, oy: f64, oz: f64, dx: f64, dy: f64, dz: f64],
    Vector3 = V3 [x: f64, y: f64, z: f64]))]
pub mod plane_api {
    use super::{Plane, Ray, V3, dflt};

    /// A plane: points `p` with `normal · p + distance = 0`.
    #[optional(distance)]
    pub fn create(normal: V3, distance: f64) -> Plane {
        Plane::make(normal, dflt(distance, 0.0))
    }

    /// The plane through `point` facing along `normal`.
    pub fn from_point(normal: V3, point: V3) -> Plane {
        Plane::from_point(normal, point)
    }

    /// The plane through three points (counter-clockwise seen from the side the normal faces).
    pub fn from_points(a: V3, b: V3, c: V3) -> Plane {
        Plane::from_point(b.sub(a).cross(c.sub(a)), a)
    }

    pub fn set_normal_and_position(normal: V3, point: V3) -> Plane {
        Plane::from_point(normal, point)
    }

    #[getter(Plane)]
    pub fn normal(p: Plane) -> V3 {
        p.normal()
    }

    /// Signed distance from the plane (positive on the side the normal faces).
    #[method(Plane)]
    pub fn get_distance_to_point(p: Plane, point: V3) -> f64 {
        p.distance_to(point)
    }

    #[method(Plane)]
    pub fn get_side(p: Plane, point: V3) -> bool {
        p.distance_to(point) > 0.0
    }

    #[method(Plane)]
    pub fn closest_point_on_plane(p: Plane, point: V3) -> V3 {
        point.sub(p.normal().scale(p.distance_to(point)))
    }

    /// How far along the ray it meets the plane; NaN if it is parallel or behind.
    #[method(Plane)]
    pub fn raycast(p: Plane, ray: Ray) -> f64 {
        let denom = p.normal().dot(ray.direction());
        if denom.abs() < 1e-9 {
            return f64::NAN;
        }
        let t = -p.distance_to(ray.origin()) / denom;
        if t >= 0.0 { t } else { f64::NAN }
    }

    /// Where the ray meets the plane; a vector of NaNs if it does not.
    #[method(Plane)]
    pub fn intersect(p: Plane, ray: Ray) -> V3 {
        let denom = p.normal().dot(ray.direction());
        if denom.abs() < 1e-9 {
            return V3 {
                x: f64::NAN,
                y: f64::NAN,
                z: f64::NAN,
            };
        }
        let t = -p.distance_to(ray.origin()) / denom;
        if t >= 0.0 {
            ray.point(t)
        } else {
            V3 {
                x: f64::NAN,
                y: f64::NAN,
                z: f64::NAN,
            }
        }
    }

    /// The same plane facing the other way.
    #[getter(Plane)]
    pub fn flipped(p: Plane) -> Plane {
        Plane::make(p.normal().negate(), -p.distance)
    }

    /// This plane moved by `translation`.
    #[method(Plane)]
    pub fn translate(p: Plane, translation: V3) -> Plane {
        Plane::make(p.normal(), p.distance - p.normal().dot(translation))
    }

    /// Whether two points are on the same side.
    #[method(Plane)]
    pub fn same_side(p: Plane, a: V3, b: V3) -> bool {
        let da = p.distance_to(a);
        let db = p.distance_to(b);
        (da > 0.0 && db > 0.0) || (da <= 0.0 && db <= 0.0)
    }

    /// A direction flattened onto the plane.
    #[method(Plane)]
    pub fn project_vector(p: Plane, v: V3) -> V3 {
        let n = p.normal();
        let sq = n.sqr_magnitude();
        let proj = if sq == 0.0 {
            V3::ZERO
        } else {
            n.scale(v.dot(n) / sq)
        };
        v.sub(proj)
    }
}

// ---- Bounds / Pose ----

#[ts_api(namespace = "Bounds", values(
    Bounds = Bounds [cx: f64, cy: f64, cz: f64, sx: f64, sy: f64, sz: f64],
    Vector3 = V3 [x: f64, y: f64, z: f64]))]
pub mod bounds_api {
    use super::{Bounds, V3, v3};

    /// An axis-aligned box from its centre and size.
    pub fn create(center: V3, size: V3) -> Bounds {
        Bounds::make(center, size)
    }

    pub fn min_max(min: V3, max: V3) -> Bounds {
        Bounds::min_max(min, max)
    }

    #[getter(Bounds)]
    pub fn center(b: Bounds) -> V3 {
        b.center()
    }

    #[getter(Bounds)]
    pub fn size(b: Bounds) -> V3 {
        b.size()
    }

    #[getter(Bounds)]
    pub fn extents(b: Bounds) -> V3 {
        b.extents()
    }

    #[getter(Bounds)]
    pub fn min(b: Bounds) -> V3 {
        b.min()
    }

    #[getter(Bounds)]
    pub fn max(b: Bounds) -> V3 {
        b.max()
    }

    #[method(Bounds)]
    pub fn contains(b: Bounds, p: V3) -> bool {
        let (a, c) = (b.min(), b.max());
        p.x >= a.x && p.x <= c.x && p.y >= a.y && p.y <= c.y && p.z >= a.z && p.z <= c.z
    }

    #[method(Bounds)]
    pub fn intersects(b: Bounds, o: Bounds) -> bool {
        let (a, c, d, e) = (b.min(), b.max(), o.min(), o.max());
        a.x <= e.x && c.x >= d.x && a.y <= e.y && c.y >= d.y && a.z <= e.z && c.z >= d.z
    }

    /// Grown to include `p`.
    #[method(Bounds)]
    pub fn encapsulate(b: Bounds, p: V3) -> Bounds {
        let (a, c) = (b.min(), b.max());
        Bounds::min_max(
            v3(
                crate::jsnum::min(a.x, p.x),
                crate::jsnum::min(a.y, p.y),
                crate::jsnum::min(a.z, p.z),
            ),
            v3(
                crate::jsnum::max(c.x, p.x),
                crate::jsnum::max(c.y, p.y),
                crate::jsnum::max(c.z, p.z),
            ),
        )
    }
}

#[ts_api(namespace = "Pose", values(
    Pose = Pose [px: f64, py: f64, pz: f64, qx: f64, qy: f64, qz: f64, qw: f64],
    Vector3 = V3 [x: f64, y: f64, z: f64],
    Quaternion = Q [x: f64, y: f64, z: f64, w: f64]))]
pub mod pose_api {
    use super::{Pose, Q, V3};

    /// A position and rotation together (spawn points, attachments).
    pub fn create(position: V3, rotation: Q) -> Pose {
        Pose::make(position, rotation)
    }

    #[property]
    pub fn identity() -> Pose {
        Pose::make(V3::ZERO, Q::IDENTITY)
    }

    #[getter(Pose)]
    pub fn position(p: Pose) -> V3 {
        p.position()
    }

    #[getter(Pose)]
    pub fn rotation(p: Pose) -> Q {
        p.rotation()
    }

    #[getter(Pose)]
    pub fn forward(p: Pose) -> V3 {
        p.rotation().rotate(V3 {
            x: 0.0,
            y: 0.0,
            z: -1.0,
        })
    }

    #[getter(Pose)]
    pub fn right(p: Pose) -> V3 {
        p.rotation().rotate(V3::RIGHT)
    }

    #[getter(Pose)]
    pub fn up(p: Pose) -> V3 {
        p.rotation().rotate(V3::UP)
    }

    /// This pose, given relative to `parent`, in the parent's space.
    #[method(Pose)]
    pub fn transformed_by(p: Pose, parent: Pose) -> Pose {
        Pose::make(
            parent
                .position()
                .add(parent.rotation().rotate(p.position())),
            parent.rotation().mul(p.rotation()),
        )
    }

    /// A point from this pose's local space to the world.
    #[method(Pose)]
    pub fn transform_point(p: Pose, point: V3) -> V3 {
        p.position().add(p.rotation().rotate(point))
    }

    #[method(Pose)]
    pub fn inverse_transform_point(p: Pose, point: V3) -> V3 {
        p.rotation().inverse().rotate(point.sub(p.position()))
    }

    #[method(Pose)]
    pub fn lerp(p: Pose, to: Pose, t: f64) -> Pose {
        Pose::make(
            p.position().lerp(to.position(), t),
            p.rotation().slerp(to.rotation(), t),
        )
    }
}
