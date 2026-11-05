// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What the contact hooks get, in Unity's shape: `Collision` / `Collision2D` (the other side of a
//! contact, its velocity, impulse and contact points), `ContactPoint` / `ContactPoint2D`, and the
//! collider classes every collider component also is, `Collider` / `Collider2D` (a `Collider` is the
//! number of its `BoxCollider`, `SphereCollider`, ... component).
//!
//! The host makes a collision with [`collision`] before it calls the hooks of one contact event;
//! the handles stay valid until the next dispatch ([`clear_collisions`]), like Unity's reused
//! `Collision` objects. Contact points are read from the rapier narrow phase the first time a
//! script asks for them, into one buffer that is kept between dispatches.

use crate::comp::{self, Data, with_col};
use crate::*;
use platypus_math::{Bounds, V2};
use ts_api::{Handle, ts_api};

/// One collision handed to the hooks.
pub(crate) struct Slot {
    /// The rapier collider on the side being told, and the other side's.
    own: u32,
    other: u32,
    object: ObjectId,
    /// The other side's velocity minus this side's.
    velocity: V3,
    /// False for a contact that ended or a trigger: no contact points then.
    touching: bool,
    /// Where its points are in `State::points`, once read.
    points: Option<(u32, u32)>,
}

/// A collision for the contact hooks: `own` is the rapier collider of the side being told, `other`
/// the other one, `object` the raw GameObject on the other side (0: none). `velocity` is the
/// other side's velocity minus this one's (`None`: as the bodies move now). The number is the
/// `Collision` / `Collision2D` handle.
pub fn collision(
    own: u32,
    other: u32,
    object: u64,
    velocity: Option<[f64; 3]>,
    touching: bool,
) -> u64 {
    st(|s| {
        let velocity = velocity.unwrap_or_else(|| {
            let (a, b) = (
                s.physics.collider_velocity(own),
                s.physics.collider_velocity(other),
            );
            [0, 1, 2].map(|i| f64::from(b[i] - a[i]))
        });
        s.collisions.push(Slot {
            own,
            other,
            object: ObjectId::from_raw(object),
            velocity: V3 {
                x: velocity[0],
                y: velocity[1],
                z: velocity[2],
            },
            touching,
            points: None,
        });
        s.collisions.len() as u64
    })
}

/// Forgets the collisions handed out (their handles read as empty from now on); the buffers keep
/// their room.
pub fn clear_collisions() {
    st(|s| {
        s.collisions.clear();
        s.points.clear();
    });
}

/// The collider component (`Collider` handle) behind a rapier collider, or 0 (a collider made
/// without a component, such as `PhysicsBody.addBox`).
pub fn collider_component(collider: u32) -> u64 {
    st(|s| s.col_owner.get(&collider).copied()).map_or(0, u64::from)
}

fn none_object() -> ObjectId {
    ObjectId::from_raw(0)
}

/// Reads a slot (`h`: the handle, its place plus one).
fn slot<R: Default>(h: u32, f: impl FnOnce(&Slot) -> R) -> R {
    st(|s| {
        s.collisions
            .get((h as usize).wrapping_sub(1))
            .map(f)
            .unwrap_or_default()
    })
}

/// The range of a slot's points in `State::points`, reading them from the world the first time.
fn points_of(h: u32) -> (u32, u32) {
    st(|s| {
        let State {
            physics,
            collisions,
            points,
            ..
        } = s;
        let Some(c) = collisions.get_mut((h as usize).wrapping_sub(1)) else {
            return (0, 0);
        };
        if let Some(r) = c.points {
            return r;
        }
        let start = points.len();
        if c.touching {
            physics.contact_points(c.own, c.other, points);
        }
        let r = (start as u32, (points.len() - start) as u32);
        c.points = Some(r);
        r
    })
}

fn point_of<R: Default>(raw: u64, f: impl FnOnce(&platypus_physics::ContactPoint) -> R) -> R {
    let (h, i) = ((raw >> 32) as u32, raw as u32);
    let (start, len) = points_of(h);
    if i >= len {
        return R::default();
    }
    st(|s| {
        s.points
            .get((start + i) as usize)
            .map(f)
            .unwrap_or_default()
    })
}

fn point_raw(h: u32, i: u32) -> u64 {
    (u64::from(h) << 32) | u64::from(i)
}

fn f3(a: [f32; 3]) -> V3 {
    v3_from_f32(a)
}

fn scaled(v: V3, k: f64) -> V3 {
    V3 {
        x: v.x * k,
        y: v.y * k,
        z: v.z * k,
    }
}

fn xy(v: V3) -> V2 {
    V2 { x: v.x, y: v.y }
}

/// The Rigidbody (or Rigidbody2D) on the object of a collider component, else on `object`.
pub(crate) fn rigidbody_of(collider: u32, object: ObjectId, two_d: bool) -> u32 {
    if collider != 0 {
        return comp::attached_rigidbody(collider, two_d);
    }
    comp::find_rb(object, two_d)
}

/// The world bounds of a collider component (empty at its object without a shape).
fn bounds_of(h: u32) -> Bounds {
    let cid = with_col(h, |c| c.cid).unwrap_or(0);
    let (min, max) = match st(|s| s.physics.collider_aabb(cid)) {
        Some((min, max)) => (f3(min), f3(max)),
        None => {
            let p = comp::world_transform(comp::comp_object(h))
                .map_or(V3::ZERO, |t| f3(t.position.to_array()));
            (p, p)
        }
    };
    Bounds {
        cx: (min.x + max.x) / 2.0,
        cy: (min.y + max.y) / 2.0,
        cz: (min.z + max.z) / 2.0,
        sx: max.x - min.x,
        sy: max.y - min.y,
        sz: max.z - min.z,
    }
}

fn closest_of(h: u32, p: V3) -> V3 {
    let cid = with_col(h, |c| c.cid).unwrap_or(0);
    st(|s| {
        s.physics
            .closest_point(cid, [p.x as f32, p.y as f32, p.z as f32])
    })
    .map_or(p, f3)
}

fn first_collider(object: ObjectId, two_d: bool) -> u32 {
    comp::comps_on(
        object,
        |d| matches!(d, Data::Col(c) if c.shape.is_2d() == two_d),
    )
    .first()
    .copied()
    .unwrap_or(0)
}

/// `Collision.collider`: the other side's collider component.
fn other_collider(h: u32) -> u32 {
    collider_component(slot(h, |c| c.other)) as u32
}

fn own_collider(h: u32) -> u32 {
    collider_component(slot(h, |c| c.own)) as u32
}

fn other_object(h: u32) -> ObjectId {
    st(|s| {
        s.collisions
            .get((h as usize).wrapping_sub(1))
            .map(|c| c.object)
    })
    .unwrap_or_else(none_object)
}

fn own_object(h: u32) -> ObjectId {
    let own = slot(h, |c| c.own);
    let comp = collider_component(own) as u32;
    if comp != 0 {
        return comp::comp_object(comp);
    }
    object_of_collider(own).unwrap_or_else(none_object)
}

macro_rules! raw_handle {
    ($($name:ident($t:ty)),*) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name(pub $t);
        impl Handle for $name {
            fn to_raw(self) -> u64 {
                self.0 as u64
            }
            fn from_raw(raw: u64) -> Self {
                $name(raw as $t)
            }
        }
    )*};
}

raw_handle!(
    ColliderCompRef(u32),
    Collider2DRef(u32),
    CollisionRef(u32),
    Collision2DRef(u32),
    ContactPointRef(u64),
    ContactPoint2DRef(u64)
);

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[ts_api(
    namespace = "PhysicsContacts", handles(
        GameObject = ObjectId,
        Rigidbody = RigidbodyRef,
        Rigidbody2D = Rigidbody2DRef,
        Collider = ColliderCompRef,
        Collider2D = Collider2DRef,
        Collision = CollisionRef,
        Collision2D = Collision2DRef,
        ContactPoint = ContactPointRef,
        ContactPoint2D = ContactPoint2DRef
    ),
    values(
        Vector2 = V2 [x: f64, y: f64],
        Vector3 = V3 [x: f64, y: f64, z: f64],
        Bounds = Bounds [cx: f64, cy: f64, cz: f64, sx: f64, sy: f64, sz: f64]
    )
)]
pub mod contacts_api {
    use super::*;

    /// A 3D collider component, whatever its shape (`BoxCollider`, `SphereCollider`,
    /// `CapsuleCollider`, `MeshCollider`, `CharacterController`): what `Collision.collider` is and
    /// `getComponent(Collider)` finds.
    impl ColliderCompRef {
        /// The first 3D collider on an object, or none.
        pub fn find_on(object: ObjectId) -> ColliderCompRef {
            ColliderCompRef(first_collider(object, false))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// A trigger reports overlaps (`onTriggerEnter`) instead of colliding.
        #[property]
        pub fn is_trigger(self) -> bool {
            with_col(self.0, |c| c.trigger).unwrap_or(false)
        }

        pub fn set_is_trigger(self, on: bool) {
            comp::col_set_sensor(self.0, on);
        }

        /// The Rigidbody moving this collider, if any.
        #[property]
        #[nullable]
        pub fn attached_rigidbody(self) -> RigidbodyRef {
            RigidbodyRef(comp::attached_rigidbody(self.0, false))
        }

        /// The world-space box around the shape (as of the last physics step).
        #[property]
        pub fn bounds(self) -> Bounds {
            bounds_of(self.0)
        }

        /// The point of the shape closest to `position` (`position` itself when it is inside).
        pub fn closest_point(self, position: V3) -> V3 {
            closest_of(self.0, position)
        }

        /// The point of `bounds` closest to `position`.
        pub fn closest_point_on_bounds(self, position: V3) -> V3 {
            let b = bounds_of(self.0);
            let (min, max) = (
                V3 {
                    x: b.cx - b.sx / 2.0,
                    y: b.cy - b.sy / 2.0,
                    z: b.cz - b.sz / 2.0,
                },
                V3 {
                    x: b.cx + b.sx / 2.0,
                    y: b.cy + b.sy / 2.0,
                    z: b.cz + b.sz / 2.0,
                },
            );
            V3 {
                x: position.x.clamp(min.x, max.x),
                y: position.y.clamp(min.y, max.y),
                z: position.z.clamp(min.z, max.z),
            }
        }
    }

    /// A 2D collider component, whatever its shape (`BoxCollider2D`, `CircleCollider2D`,
    /// `CapsuleCollider2D`, `PolygonCollider2D`, `EdgeCollider2D`).
    impl Collider2DRef {
        /// The first 2D collider on an object, or none.
        pub fn find_on(object: ObjectId) -> Collider2DRef {
            Collider2DRef(first_collider(object, true))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        #[property]
        pub fn is_trigger(self) -> bool {
            with_col(self.0, |c| c.trigger).unwrap_or(false)
        }

        pub fn set_is_trigger(self, on: bool) {
            comp::col_set_sensor(self.0, on);
        }

        /// The Rigidbody2D moving this collider, if any.
        #[property]
        #[nullable]
        pub fn attached_rigidbody(self) -> Rigidbody2DRef {
            Rigidbody2DRef(comp::attached_rigidbody(self.0, true))
        }

        /// The shape's offset from the object, in its local space.
        #[property]
        pub fn offset(self) -> V2 {
            with_col(self.0, |c| xy(c.center)).unwrap_or_default()
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

        /// The world-space box around the shape (its slab `Physics2D.depth` deep along z).
        #[property]
        pub fn bounds(self) -> Bounds {
            bounds_of(self.0)
        }

        /// The point of the shape closest to `position` (`position` itself when it is inside).
        pub fn closest_point(self, position: V2) -> V2 {
            let z = comp::world_transform(comp::comp_object(self.0))
                .map_or(0.0, |t| f64::from(t.position.z));
            xy(closest_of(
                self.0,
                V3 {
                    x: position.x,
                    y: position.y,
                    z,
                },
            ))
        }
    }

    /// What `onCollisionEnter`, `onCollisionStay` and `onCollisionExit` get (and a trigger hook
    /// that asks for it): the other side of the contact. Read it in the hook: the engine reuses it
    /// for the next contacts, as Unity does.
    impl CollisionRef {
        /// The object hit.
        #[property]
        pub fn game_object(self) -> ObjectId {
            other_object(self.0)
        }

        /// The collider hit (none for a collider made without a component).
        #[property]
        pub fn collider(self) -> ColliderCompRef {
            ColliderCompRef(other_collider(self.0))
        }

        /// The Rigidbody hit (none for a collider without one).
        #[property]
        #[nullable]
        pub fn rigidbody(self) -> RigidbodyRef {
            RigidbodyRef(rigidbody_of(
                other_collider(self.0),
                other_object(self.0),
                false,
            ))
        }

        /// The other side's velocity relative to this one (m/s): going into the contact for
        /// `onCollisionEnter`, as they move now for the others.
        #[property]
        pub fn relative_velocity(self) -> V3 {
            slot(self.0, |c| c.velocity)
        }

        /// The total impulse (N·s) the contact gave this object in the last physics step, along
        /// the contact normals (divide by `Time.fixedDeltaTime` for the force).
        #[property]
        pub fn impulse(self) -> V3 {
            let (start, len) = points_of(self.0);
            st(|s| {
                s.points[start as usize..(start + len) as usize]
                    .iter()
                    .fold(V3::ZERO, |a, p| {
                        let i = scaled(f3(p.normal), f64::from(p.normal_impulse));
                        V3 {
                            x: a.x + i.x,
                            y: a.y + i.y,
                            z: a.z + i.z,
                        }
                    })
            })
        }

        /// How many contact points there are (0 in `onCollisionExit`).
        #[property]
        pub fn contact_count(self) -> u32 {
            points_of(self.0).1
        }

        /// The contact points (a new array each time; `getContact(i)` does not make one).
        #[property]
        pub fn contacts(self) -> Vec<ContactPointRef> {
            (0..points_of(self.0).1)
                .map(|i| ContactPointRef(point_raw(self.0, i)))
                .collect()
        }

        /// Contact point `index` (0 to `contactCount - 1`).
        pub fn get_contact(self, index: u32) -> ContactPointRef {
            ContactPointRef(point_raw(self.0, index))
        }
    }

    /// One point of a `Collision`.
    impl ContactPointRef {
        /// Where the two surfaces touch (world space).
        #[property]
        pub fn point(self) -> V3 {
            point_of(self.0, |p| f3(p.point))
        }

        /// The surface normal there, pointing away from the other collider towards this one.
        #[property]
        pub fn normal(self) -> V3 {
            point_of(self.0, |p| f3(p.normal))
        }

        /// The distance between the surfaces along the normal (negative: they overlap).
        #[property]
        pub fn separation(self) -> f64 {
            point_of(self.0, |p| f64::from(p.separation))
        }

        /// The impulse at this point in the last physics step (N·s, along `normal`).
        #[property]
        pub fn impulse(self) -> V3 {
            point_of(self.0, |p| {
                scaled(f3(p.normal), f64::from(p.normal_impulse))
            })
        }

        /// The collider of the object being told.
        #[property]
        pub fn this_collider(self) -> ColliderCompRef {
            ColliderCompRef(own_collider((self.0 >> 32) as u32))
        }

        /// The collider hit.
        #[property]
        pub fn other_collider(self) -> ColliderCompRef {
            ColliderCompRef(other_collider((self.0 >> 32) as u32))
        }
    }

    /// What `onCollisionEnter2D`, `onCollisionStay2D` and `onCollisionExit2D` get: the other
    /// side of a 2D contact, in x and y. Read it in the hook (the engine reuses it).
    impl Collision2DRef {
        /// The object hit.
        #[property]
        pub fn game_object(self) -> ObjectId {
            other_object(self.0)
        }

        /// The collider hit.
        #[property]
        pub fn collider(self) -> Collider2DRef {
            Collider2DRef(other_collider(self.0))
        }

        /// This object's collider in the contact.
        #[property]
        pub fn other_collider(self) -> Collider2DRef {
            Collider2DRef(own_collider(self.0))
        }

        /// The Rigidbody2D hit (none for a collider without one).
        #[property]
        #[nullable]
        pub fn rigidbody(self) -> Rigidbody2DRef {
            Rigidbody2DRef(rigidbody_of(
                other_collider(self.0),
                other_object(self.0),
                true,
            ))
        }

        /// This object's Rigidbody2D.
        #[property]
        #[nullable]
        pub fn other_rigidbody(self) -> Rigidbody2DRef {
            Rigidbody2DRef(rigidbody_of(own_collider(self.0), own_object(self.0), true))
        }

        /// The other side's velocity relative to this one (m/s): going into the contact for
        /// `onCollisionEnter2D`, as they move now for the others.
        #[property]
        pub fn relative_velocity(self) -> V2 {
            slot(self.0, |c| xy(c.velocity))
        }

        /// Always true: Platypus has no effectors or contact modification that turn a contact's
        /// response off.
        #[property]
        pub fn enabled(self) -> bool {
            true
        }

        /// How many contact points there are (0 in `onCollisionExit2D`).
        #[property]
        pub fn contact_count(self) -> u32 {
            points_of(self.0).1
        }

        /// The contact points (a new array each time; `getContact(i)` does not make one).
        #[property]
        pub fn contacts(self) -> Vec<ContactPoint2DRef> {
            (0..points_of(self.0).1)
                .map(|i| ContactPoint2DRef(point_raw(self.0, i)))
                .collect()
        }

        /// Contact point `index` (0 to `contactCount - 1`).
        pub fn get_contact(self, index: u32) -> ContactPoint2DRef {
            ContactPoint2DRef(point_raw(self.0, index))
        }
    }

    /// One point of a `Collision2D`.
    impl ContactPoint2DRef {
        /// Where the two surfaces touch.
        #[property]
        pub fn point(self) -> V2 {
            point_of(self.0, |p| xy(f3(p.point)))
        }

        /// The surface normal there, pointing away from the other collider towards this one.
        #[property]
        pub fn normal(self) -> V2 {
            point_of(self.0, |p| xy(f3(p.normal)))
        }

        /// The distance between the surfaces along the normal (negative: they overlap).
        #[property]
        pub fn separation(self) -> f64 {
            point_of(self.0, |p| f64::from(p.separation))
        }

        /// The impulse along the normal in the last physics step (N·s).
        #[property]
        pub fn normal_impulse(self) -> f64 {
            point_of(self.0, |p| f64::from(p.normal_impulse))
        }

        /// The friction impulse along the tangent (the normal turned a quarter clockwise), N·s.
        #[property]
        pub fn tangent_impulse(self) -> f64 {
            point_of(self.0, |p| {
                let (n, t) = (p.normal, p.tangent_impulse);
                f64::from(t[0] * n[1] - t[1] * n[0])
            })
        }

        /// The other body's velocity at the point minus this body's.
        #[property]
        pub fn relative_velocity(self) -> V2 {
            point_of(self.0, |p| xy(f3(p.relative_velocity)))
        }

        /// The collider hit.
        #[property]
        pub fn collider(self) -> Collider2DRef {
            Collider2DRef(other_collider((self.0 >> 32) as u32))
        }

        /// This object's collider.
        #[property]
        pub fn other_collider(self) -> Collider2DRef {
            Collider2DRef(own_collider((self.0 >> 32) as u32))
        }

        /// The Rigidbody2D hit.
        #[property]
        #[nullable]
        pub fn rigidbody(self) -> Rigidbody2DRef {
            let h = (self.0 >> 32) as u32;
            Rigidbody2DRef(rigidbody_of(other_collider(h), other_object(h), true))
        }

        /// This object's Rigidbody2D.
        #[property]
        #[nullable]
        pub fn other_rigidbody(self) -> Rigidbody2DRef {
            let h = (self.0 >> 32) as u32;
            Rigidbody2DRef(rigidbody_of(own_collider(h), own_object(h), true))
        }

        /// Always true (see `Collision2D.enabled`).
        #[property]
        pub fn enabled(self) -> bool {
            true
        }
    }
}
