// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! 2D physics for compiled scripts: `Rigidbody2D`, `BoxCollider2D`, `CircleCollider2D`,
//! `CapsuleCollider2D`, `PolygonCollider2D`, `EdgeCollider2D` and `Physics2D`, as in
//! `physics2d.ts`. They run in the same rapier world as 3D: a Rigidbody2D's body moves in x and
//! y and turns about z, and 2D colliders are their outlines extruded into slabs `Physics2D.depth`
//! deep, centred on the object. Angles are degrees about z.
//!
//! Differences to the TypeScript: `points` of the polygon and edge colliders are `setPoints`,
//! `pointCount` and `pointAt(i)`; `Physics2D.raycast` answers at once and its hit has
//! `colliderObject` (the GameObject) rather than a collider component.

use crate::comp::{self, ColShape, Data, ShapeTag, with_col, with_rb};
use crate::contacts::Collider2DRef;
use crate::*;
use platypus_api::platypus_api;
use platypus_math::{Bounds, V2};

fn colg<R: Default>(h: u32, f: impl FnOnce(&comp::Col) -> R) -> R {
    with_col(h, |c| f(c)).unwrap_or_default()
}

fn rbg<R: Default>(h: u32, f: impl FnOnce(&comp::Rb) -> R) -> R {
    with_rb(h, |r| f(r)).unwrap_or_default()
}

fn v2(x: f64, y: f64) -> V2 {
    V2 { x, y }
}

fn body_id(h: u32) -> u32 {
    comp::rb_body(h)
}

fn force_scale(mode: f64) -> f64 {
    // ForceMode2D.Force (0) is a force over one fixed step; Impulse (1) is instant.
    if mode == 1.0 { 1.0 } else { 1.0 / 60.0 }
}

fn z_of(h: u32) -> f64 {
    comp::world_transform(comp::comp_object(h)).map_or(0.0, |t| f64::from(t.position.z))
}

fn rb_position(h: u32) -> V2 {
    comp::world_transform(comp::comp_object(h)).map_or(V2::ZERO, |t| {
        v2(f64::from(t.position.x), f64::from(t.position.y))
    })
}

fn rb_rotation(h: u32) -> f64 {
    comp::world_transform(comp::comp_object(h)).map_or(0.0, |t| {
        let q = t.rotation;
        let z = (2.0 * (q.w * q.z + q.x * q.y)).atan2(1.0 - 2.0 * (q.y * q.y + q.z * q.z));
        f64::from(z).to_degrees().rem_euclid(360.0)
    })
}

fn set_pose2d(h: u32, position: Option<V2>, degrees: Option<f64>) {
    let object = comp::comp_object(h);
    platypus_scene::api::with_scene(|sc| {
        if !sc.contains(object) {
            return;
        }
        let t = sc.world(object);
        let p = position.map_or(t.position, |p| {
            glam::Vec3::new(p.x as f32, p.y as f32, t.position.z)
        });
        let r = degrees.map_or(t.rotation, |d| {
            glam::Quat::from_rotation_z((d as f32).to_radians())
        });
        sc.set_world(object, p, r);
    });
}

fn polygon_points(h: u32) -> Vec<V2> {
    colg(h, |c| match &c.shape {
        ColShape::Polygon2D { points } | ColShape::Edge2D { points } => points.clone(),
        _ => Vec::new(),
    })
}

fn set_polygon_points(h: u32, pts: &[V2]) {
    comp::col_edit(h, |sh| match sh {
        ColShape::Polygon2D { points } | ColShape::Edge2D { points } => *points = pts.to_vec(),
        _ => {}
    });
}

fn hit_data<R: Default>(h: u32, f: impl FnOnce(&HitData) -> R) -> R {
    st(|s| s.hits.get((h as usize).wrapping_sub(1)).map(f)).unwrap_or_default()
}

pub(crate) fn count_rigidbodies_2d() -> u32 {
    st(|s| {
        s.comps
            .iter()
            .flatten()
            .filter(|c| matches!(&c.data, Data::Rb(r) if r.two_d))
            .count() as u32
    })
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[platypus_api(
    namespace = "Physics2D", handles(
        GameObject = ObjectId,
        Rigidbody2D = Rigidbody2DRef,
        BoxCollider2D = BoxCollider2DRef,
        CircleCollider2D = CircleCollider2DRef,
        CapsuleCollider2D = CapsuleCollider2DRef,
        PolygonCollider2D = PolygonCollider2DRef,
        EdgeCollider2D = EdgeCollider2DRef,
        Collider2D = Collider2DRef,
        RaycastHit2D = Hit2DRef
    ),
    values(
        Vector2 = V2 [x: f64, y: f64],
        Vector3 = V3 [x: f64, y: f64, z: f64],
        Bounds = Bounds [cx: f64, cy: f64, cz: f64, sx: f64, sy: f64, sz: f64]
    )
)]
pub mod physics2d_api {
    use super::*;

    /// Stops two colliders colliding with each other (or lets them again with `ignore` false),
    /// whatever their shapes; it holds when either is rebuilt. Triggers between them stop too.
    #[optional(ignore = true)]
    pub fn ignore_collision(a: Collider2DRef, b: Collider2DRef, ignore: bool) {
        comp::set_ignored(a.0, b.0, ignore);
    }

    /// How many Rigidbody2D components exist.
    pub fn rigidbody_2d_count() -> u32 {
        count_rigidbodies_2d()
    }

    /// How deep 2D colliders are along z (metres).
    #[property]
    pub fn depth() -> f64 {
        st(|s| s.depth_2d)
    }

    pub fn set_depth(d: f64) {
        st(|s| s.depth_2d = d);
    }

    /// World gravity, shared with 3D physics (its z stays 0).
    #[property]
    pub fn gravity() -> V2 {
        let g = st(|s| s.gravity_2d);
        V2 { x: g[0], y: g[1] }
    }

    pub fn set_gravity(g: V2) {
        st(|s| s.gravity_2d = [g.x, g.y]);
        op(
            "gravity",
            vec![crate::v3(V3 {
                x: g.x,
                y: g.y,
                z: 0.0,
            })],
        );
    }

    /// Casts a ray in the XY plane at once and answers the first collider hit within
    /// `distance`, or no hit.
    #[optional(distance = 1000)]
    pub fn raycast(origin: V2, direction: V2, distance: f64) -> Hit2DRef {
        Hit2DRef(crate::do_raycast(
            V3 {
                x: origin.x,
                y: origin.y,
                z: 0.0,
            },
            V3 {
                x: direction.x,
                y: direction.y,
                z: 0.0,
            },
            distance,
        ))
    }

    impl Hit2DRef {
        /// The GameObject of the collider that was hit.
        #[property]
        pub fn collider_object(self) -> ObjectId {
            let cid = hit_data(self.0, |h| h.collider);
            comp::collider_info(cid)
                .map(|i| i.object)
                .or_else(|| crate::object_of_collider(cid))
                .unwrap_or(ObjectId {
                    index: u32::MAX,
                    generation: u32::MAX,
                })
        }

        #[property]
        pub fn point(self) -> V2 {
            hit_data(self.0, |h| v2(h.point.x, h.point.y))
        }

        #[property]
        pub fn normal(self) -> V2 {
            hit_data(self.0, |h| v2(h.normal.x, h.normal.y))
        }

        #[property]
        pub fn distance(self) -> f64 {
            hit_data(self.0, |h| h.distance)
        }
    }

    impl Rigidbody2DRef {
        /// Adds a Rigidbody2D to a GameObject (what `go.addComponent(Rigidbody2D)` does).
        pub fn attach(object: ObjectId) -> Rigidbody2DRef {
            Rigidbody2DRef(comp::rb_attach(object, true))
        }

        pub fn find_on(object: ObjectId) -> Rigidbody2DRef {
            Rigidbody2DRef(comp::find_rb(object, true))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// In kg.
        #[property]
        pub fn mass(self) -> f64 {
            rbg(self.0, |r| r.mass)
        }

        pub fn set_mass(self, kg: f64) {
            comp::rb_set_mass(self.0, kg);
        }

        #[property]
        pub fn drag(self) -> f64 {
            rbg(self.0, |r| r.drag)
        }

        pub fn set_drag(self, d: f64) {
            comp::rb_set_damping(self.0, Some(d), None);
        }

        #[property]
        pub fn angular_drag(self) -> f64 {
            rbg(self.0, |r| r.angular_drag)
        }

        pub fn set_angular_drag(self, d: f64) {
            comp::rb_set_damping(self.0, None, Some(d));
        }

        /// Multiplies gravity (0: floats).
        #[property]
        pub fn gravity_scale(self) -> f64 {
            rbg(self.0, |r| r.gravity_scale)
        }

        pub fn set_gravity_scale(self, s: f64) {
            comp::rb_set_gravity_scale(self.0, s);
        }

        /// `"dynamic"` (simulated), `"kinematic"` (moved by scripts, pushes others) or `"static"`.
        #[property]
        pub fn body_type(self) -> String {
            match rbg(self.0, |r| r.kind.to_string()).as_str() {
                "fixed" => "static".into(),
                other => other.into(),
            }
        }

        pub fn set_body_type(self, t: &str) {
            comp::rb_set_kind(
                self.0,
                match t {
                    "static" => "fixed",
                    "kinematic" => "kinematic",
                    _ => "dynamic",
                },
            );
        }

        #[property]
        pub fn is_kinematic(self) -> bool {
            rbg(self.0, |r| r.kind == "kinematic")
        }

        pub fn set_is_kinematic(self, on: bool) {
            comp::rb_set_kind(self.0, if on { "kinematic" } else { "dynamic" });
        }

        /// Keeps it upright: no turning from hits.
        #[property]
        pub fn freeze_rotation(self) -> bool {
            rbg(self.0, |r| r.freeze_rotation)
        }

        pub fn set_freeze_rotation(self, on: bool) {
            with_rb(self.0, |r| r.freeze_rotation = on);
            comp::rb_resend_axes(self.0);
        }

        /// `"discrete"` or `"continuous"`.
        #[property]
        pub fn collision_detection_mode(self) -> String {
            if rbg(self.0, |r| r.continuous) {
                "continuous".into()
            } else {
                "discrete".into()
            }
        }

        pub fn set_collision_detection_mode(self, mode: &str) {
            comp::rb_set_continuous(self.0, mode == "continuous");
        }

        /// Velocity in m/s (as of the last physics step).
        #[property]
        pub fn velocity(self) -> V2 {
            let v = from3(
                {
                    let b = body_id(self.0);
                    st(|s| s.physics.velocity(b))
                }
                .unwrap_or_default(),
            );
            v2(v.x, v.y)
        }

        pub fn set_velocity(self, v: V2) {
            op(
                "set",
                vec![
                    num(f64::from(body_id(self.0))),
                    Value::String("velocity".into()),
                    crate::v3(V3 {
                        x: v.x,
                        y: v.y,
                        z: 0.0,
                    }),
                ],
            );
        }

        /// Spin in degrees per second, anticlockwise (as of the last physics step).
        #[property]
        pub fn angular_velocity(self) -> f64 {
            from3(
                {
                    let b = body_id(self.0);
                    st(|s| s.physics.angular_velocity(b))
                }
                .unwrap_or_default(),
            )
            .z
            .to_degrees()
        }

        pub fn set_angular_velocity(self, degrees: f64) {
            op(
                "set",
                vec![
                    num(f64::from(body_id(self.0))),
                    Value::String("angularVelocity".into()),
                    crate::v3(V3 {
                        x: 0.0,
                        y: 0.0,
                        z: degrees.to_radians(),
                    }),
                ],
            );
        }

        /// Where it is in the XY plane.
        #[property]
        pub fn position(self) -> V2 {
            rb_position(self.0)
        }

        pub fn set_position(self, p: V2) {
            set_pose2d(self.0, Some(p), None);
        }

        /// Its angle about z in degrees, anticlockwise.
        #[property]
        pub fn rotation(self) -> f64 {
            rb_rotation(self.0)
        }

        pub fn set_rotation(self, degrees: f64) {
            set_pose2d(self.0, None, Some(degrees));
        }

        /// `mode` 0 is a force over one fixed step (`ForceMode2D.Force`), 1 an impulse.
        #[optional(mode)]
        pub fn add_force(self, force: V2, mode: f64) {
            let k = force_scale(mode);
            force_op(
                body_id(self.0),
                "impulse",
                V3 {
                    x: force.x * k,
                    y: force.y * k,
                    z: 0.0,
                },
                None,
            );
        }

        /// Pushes at a world point, so it also spins.
        #[optional(mode)]
        pub fn add_force_at_position(self, force: V2, position: V2, mode: f64) {
            let k = force_scale(mode);
            force_op(
                body_id(self.0),
                "impulseAt",
                V3 {
                    x: force.x * k,
                    y: force.y * k,
                    z: 0.0,
                },
                Some(V3 {
                    x: position.x,
                    y: position.y,
                    z: z_of(self.0),
                }),
            );
        }

        /// Spins it (N·m anticlockwise).
        #[optional(mode)]
        pub fn add_torque(self, torque: f64, mode: f64) {
            let k = force_scale(mode);
            force_op(
                body_id(self.0),
                "torqueImpulse",
                V3 {
                    x: 0.0,
                    y: 0.0,
                    z: torque * k,
                },
                None,
            );
        }

        /// Moves a kinematic body to `position` over the next step, pushing things.
        pub fn move_position(self, position: V2) {
            set_pose2d(self.0, Some(position), None);
        }

        pub fn move_rotation(self, degrees: f64) {
            set_pose2d(self.0, None, Some(degrees));
        }

        pub fn sleep(self) {
            body_set(body_id(self.0), "sleep", None);
        }

        pub fn wake_up(self) {
            body_set(body_id(self.0), "wake", None);
        }

        pub fn destroy(self) {
            comp::rb_destroy(self.0);
        }
    }

    impl BoxCollider2DRef {
        /// Adds a BoxCollider2D to a GameObject.
        pub fn attach(object: ObjectId) -> BoxCollider2DRef {
            BoxCollider2DRef(comp::col_attach(
                object,
                ColShape::Box2D {
                    size: V2 { x: 1.0, y: 1.0 },
                },
            ))
        }

        pub fn find_on(object: ObjectId) -> BoxCollider2DRef {
            BoxCollider2DRef(comp::find_col(object, ShapeTag::Box2D))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The outline's offset from the object.
        #[property]
        pub fn offset(self) -> V2 {
            colg(self.0, |c| v2(c.center.x, c.center.y))
        }

        pub fn set_offset(self, o: V2) {
            comp::col_set_center(
                self.0,
                V3 {
                    x: o.x,
                    y: o.y,
                    z: 0.0,
                },
            );
        }

        #[property]
        pub fn is_trigger(self) -> bool {
            colg(self.0, |c| c.trigger)
        }

        pub fn set_is_trigger(self, on: bool) {
            comp::col_set_sensor(self.0, on);
        }

        #[property]
        pub fn dynamic_friction(self) -> f64 {
            colg(self.0, |c| c.friction)
        }

        pub fn set_dynamic_friction(self, f: f64) {
            comp::col_set_material(self.0, Some(f), None);
        }

        #[property]
        pub fn bounciness(self) -> f64 {
            colg(self.0, |c| c.bounciness)
        }

        pub fn set_bounciness(self, b: f64) {
            comp::col_set_material(self.0, None, Some(b));
        }

        /// The Rigidbody2D moving this collider, if any.
        #[property]
        pub fn attached_rigidbody(self) -> Rigidbody2DRef {
            Rigidbody2DRef(comp::attached_rigidbody(self.0, true))
        }

        /// The world-space box around the shape (its slab `Physics2D.depth` deep along z).
        #[property]
        pub fn bounds(self) -> Bounds {
            crate::contacts::collider_bounds(self.0)
        }

        /// The point of the shape closest to `position` (`position` itself when it is inside).
        pub fn closest_point(self, position: V2) -> V2 {
            crate::contacts::collider_closest_2d(self.0, position)
        }

        pub fn destroy(self) {
            comp::col_destroy(self.0);
        }

        /// The rectangle `size` across.
        #[property]
        pub fn size(self) -> V2 {
            colg(self.0, |c| match &c.shape {
                ColShape::Box2D { size } => *size,
                _ => V2::ZERO,
            })
        }

        pub fn set_size(self, s: V2) {
            comp::col_edit(self.0, |sh| *sh = ColShape::Box2D { size: s });
        }
    }

    impl CircleCollider2DRef {
        /// Adds a CircleCollider2D to a GameObject.
        pub fn attach(object: ObjectId) -> CircleCollider2DRef {
            CircleCollider2DRef(comp::col_attach(object, ColShape::Circle2D { radius: 0.5 }))
        }

        pub fn find_on(object: ObjectId) -> CircleCollider2DRef {
            CircleCollider2DRef(comp::find_col(object, ShapeTag::Circle2D))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The outline's offset from the object.
        #[property]
        pub fn offset(self) -> V2 {
            colg(self.0, |c| v2(c.center.x, c.center.y))
        }

        pub fn set_offset(self, o: V2) {
            comp::col_set_center(
                self.0,
                V3 {
                    x: o.x,
                    y: o.y,
                    z: 0.0,
                },
            );
        }

        #[property]
        pub fn is_trigger(self) -> bool {
            colg(self.0, |c| c.trigger)
        }

        pub fn set_is_trigger(self, on: bool) {
            comp::col_set_sensor(self.0, on);
        }

        #[property]
        pub fn dynamic_friction(self) -> f64 {
            colg(self.0, |c| c.friction)
        }

        pub fn set_dynamic_friction(self, f: f64) {
            comp::col_set_material(self.0, Some(f), None);
        }

        #[property]
        pub fn bounciness(self) -> f64 {
            colg(self.0, |c| c.bounciness)
        }

        pub fn set_bounciness(self, b: f64) {
            comp::col_set_material(self.0, None, Some(b));
        }

        /// The Rigidbody2D moving this collider, if any.
        #[property]
        pub fn attached_rigidbody(self) -> Rigidbody2DRef {
            Rigidbody2DRef(comp::attached_rigidbody(self.0, true))
        }

        /// The world-space box around the shape (its slab `Physics2D.depth` deep along z).
        #[property]
        pub fn bounds(self) -> Bounds {
            crate::contacts::collider_bounds(self.0)
        }

        /// The point of the shape closest to `position` (`position` itself when it is inside).
        pub fn closest_point(self, position: V2) -> V2 {
            crate::contacts::collider_closest_2d(self.0, position)
        }

        pub fn destroy(self) {
            comp::col_destroy(self.0);
        }

        /// Scaled by the object's larger of x and y.
        #[property]
        pub fn radius(self) -> f64 {
            colg(self.0, |c| match &c.shape {
                ColShape::Circle2D { radius } => *radius,
                _ => 0.0,
            })
        }

        pub fn set_radius(self, r: f64) {
            comp::col_edit(self.0, |sh| *sh = ColShape::Circle2D { radius: r });
        }
    }

    impl CapsuleCollider2DRef {
        /// Adds a CapsuleCollider2D to a GameObject.
        pub fn attach(object: ObjectId) -> CapsuleCollider2DRef {
            CapsuleCollider2DRef(comp::col_attach(
                object,
                ColShape::Capsule2D {
                    size: V2 { x: 1.0, y: 2.0 },
                    vertical: true,
                },
            ))
        }

        pub fn find_on(object: ObjectId) -> CapsuleCollider2DRef {
            CapsuleCollider2DRef(comp::find_col(object, ShapeTag::Capsule2D))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The outline's offset from the object.
        #[property]
        pub fn offset(self) -> V2 {
            colg(self.0, |c| v2(c.center.x, c.center.y))
        }

        pub fn set_offset(self, o: V2) {
            comp::col_set_center(
                self.0,
                V3 {
                    x: o.x,
                    y: o.y,
                    z: 0.0,
                },
            );
        }

        #[property]
        pub fn is_trigger(self) -> bool {
            colg(self.0, |c| c.trigger)
        }

        pub fn set_is_trigger(self, on: bool) {
            comp::col_set_sensor(self.0, on);
        }

        #[property]
        pub fn dynamic_friction(self) -> f64 {
            colg(self.0, |c| c.friction)
        }

        pub fn set_dynamic_friction(self, f: f64) {
            comp::col_set_material(self.0, Some(f), None);
        }

        #[property]
        pub fn bounciness(self) -> f64 {
            colg(self.0, |c| c.bounciness)
        }

        pub fn set_bounciness(self, b: f64) {
            comp::col_set_material(self.0, None, Some(b));
        }

        /// The Rigidbody2D moving this collider, if any.
        #[property]
        pub fn attached_rigidbody(self) -> Rigidbody2DRef {
            Rigidbody2DRef(comp::attached_rigidbody(self.0, true))
        }

        /// The world-space box around the shape (its slab `Physics2D.depth` deep along z).
        #[property]
        pub fn bounds(self) -> Bounds {
            crate::contacts::collider_bounds(self.0)
        }

        /// The point of the shape closest to `position` (`position` itself when it is inside).
        pub fn closest_point(self, position: V2) -> V2 {
            crate::contacts::collider_closest_2d(self.0, position)
        }

        pub fn destroy(self) {
            comp::col_destroy(self.0);
        }

        #[property]
        pub fn size(self) -> V2 {
            colg(self.0, |c| match &c.shape {
                ColShape::Capsule2D { size, .. } => *size,
                _ => V2::ZERO,
            })
        }

        pub fn set_size(self, s: V2) {
            comp::col_edit(self.0, |sh| {
                if let ColShape::Capsule2D { size, .. } = sh {
                    *size = s;
                }
            });
        }

        /// `"vertical"` or `"horizontal"`: the way it is rounded.
        #[property]
        pub fn direction(self) -> String {
            if colg(self.0, |c| {
                matches!(&c.shape, ColShape::Capsule2D { vertical: true, .. })
            }) {
                "vertical".into()
            } else {
                "horizontal".into()
            }
        }

        pub fn set_direction(self, d: &str) {
            comp::col_edit(self.0, |sh| {
                if let ColShape::Capsule2D { vertical, .. } = sh {
                    *vertical = d != "horizontal";
                }
            });
        }
    }

    impl PolygonCollider2DRef {
        /// Adds a PolygonCollider2D to a GameObject.
        pub fn attach(object: ObjectId) -> PolygonCollider2DRef {
            PolygonCollider2DRef(comp::col_attach(
                object,
                ColShape::Polygon2D {
                    points: vec![
                        V2 { x: -0.5, y: -0.5 },
                        V2 { x: 0.5, y: -0.5 },
                        V2 { x: 0.0, y: 0.5 },
                    ],
                },
            ))
        }

        pub fn find_on(object: ObjectId) -> PolygonCollider2DRef {
            PolygonCollider2DRef(comp::find_col(object, ShapeTag::Polygon2D))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The outline's offset from the object.
        #[property]
        pub fn offset(self) -> V2 {
            colg(self.0, |c| v2(c.center.x, c.center.y))
        }

        pub fn set_offset(self, o: V2) {
            comp::col_set_center(
                self.0,
                V3 {
                    x: o.x,
                    y: o.y,
                    z: 0.0,
                },
            );
        }

        #[property]
        pub fn is_trigger(self) -> bool {
            colg(self.0, |c| c.trigger)
        }

        pub fn set_is_trigger(self, on: bool) {
            comp::col_set_sensor(self.0, on);
        }

        #[property]
        pub fn dynamic_friction(self) -> f64 {
            colg(self.0, |c| c.friction)
        }

        pub fn set_dynamic_friction(self, f: f64) {
            comp::col_set_material(self.0, Some(f), None);
        }

        #[property]
        pub fn bounciness(self) -> f64 {
            colg(self.0, |c| c.bounciness)
        }

        pub fn set_bounciness(self, b: f64) {
            comp::col_set_material(self.0, None, Some(b));
        }

        /// The Rigidbody2D moving this collider, if any.
        #[property]
        pub fn attached_rigidbody(self) -> Rigidbody2DRef {
            Rigidbody2DRef(comp::attached_rigidbody(self.0, true))
        }

        /// The world-space box around the shape (its slab `Physics2D.depth` deep along z).
        #[property]
        pub fn bounds(self) -> Bounds {
            crate::contacts::collider_bounds(self.0)
        }

        /// The point of the shape closest to `position` (`position` itself when it is inside).
        pub fn closest_point(self, position: V2) -> V2 {
            crate::contacts::collider_closest_2d(self.0, position)
        }

        pub fn destroy(self) {
            comp::col_destroy(self.0);
        }

        /// The outline in the object's space. Concave outlines are filled in to their convex hull.
        #[property]
        pub fn points(self) -> Vec<V2> {
            polygon_points(self.0)
        }

        #[like]
        pub fn set_points(self, points: &[V2]) {
            set_polygon_points(self.0, points);
        }

        #[property]
        pub fn point_count(self) -> u32 {
            polygon_points(self.0).len() as u32
        }

        pub fn point_at(self, i: u32) -> V2 {
            polygon_points(self.0)
                .get(i as usize)
                .copied()
                .unwrap_or(V2::ZERO)
        }
    }

    impl EdgeCollider2DRef {
        /// Adds a EdgeCollider2D to a GameObject.
        pub fn attach(object: ObjectId) -> EdgeCollider2DRef {
            EdgeCollider2DRef(comp::col_attach(
                object,
                ColShape::Edge2D {
                    points: vec![V2 { x: -0.5, y: 0.0 }, V2 { x: 0.5, y: 0.0 }],
                },
            ))
        }

        pub fn find_on(object: ObjectId) -> EdgeCollider2DRef {
            EdgeCollider2DRef(comp::find_col(object, ShapeTag::Edge2D))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The outline's offset from the object.
        #[property]
        pub fn offset(self) -> V2 {
            colg(self.0, |c| v2(c.center.x, c.center.y))
        }

        pub fn set_offset(self, o: V2) {
            comp::col_set_center(
                self.0,
                V3 {
                    x: o.x,
                    y: o.y,
                    z: 0.0,
                },
            );
        }

        #[property]
        pub fn is_trigger(self) -> bool {
            colg(self.0, |c| c.trigger)
        }

        pub fn set_is_trigger(self, on: bool) {
            comp::col_set_sensor(self.0, on);
        }

        #[property]
        pub fn dynamic_friction(self) -> f64 {
            colg(self.0, |c| c.friction)
        }

        pub fn set_dynamic_friction(self, f: f64) {
            comp::col_set_material(self.0, Some(f), None);
        }

        #[property]
        pub fn bounciness(self) -> f64 {
            colg(self.0, |c| c.bounciness)
        }

        pub fn set_bounciness(self, b: f64) {
            comp::col_set_material(self.0, None, Some(b));
        }

        /// The Rigidbody2D moving this collider, if any.
        #[property]
        pub fn attached_rigidbody(self) -> Rigidbody2DRef {
            Rigidbody2DRef(comp::attached_rigidbody(self.0, true))
        }

        /// The world-space box around the shape (its slab `Physics2D.depth` deep along z).
        #[property]
        pub fn bounds(self) -> Bounds {
            crate::contacts::collider_bounds(self.0)
        }

        /// The point of the shape closest to `position` (`position` itself when it is inside).
        pub fn closest_point(self, position: V2) -> V2 {
            crate::contacts::collider_closest_2d(self.0, position)
        }

        pub fn destroy(self) {
            comp::col_destroy(self.0);
        }

        /// A line through the points (ground, walls, slopes): for static scenery and
        /// kinematic platforms, not falling bodies.
        #[property]
        pub fn points(self) -> Vec<V2> {
            polygon_points(self.0)
        }

        #[like]
        pub fn set_points(self, points: &[V2]) {
            set_polygon_points(self.0, points);
        }

        #[property]
        pub fn point_count(self) -> u32 {
            polygon_points(self.0).len() as u32
        }

        pub fn point_at(self, i: u32) -> V2 {
            polygon_points(self.0)
                .get(i as usize)
                .copied()
                .unwrap_or(V2::ZERO)
        }
    }
}
