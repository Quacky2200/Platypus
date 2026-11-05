// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The scene's script API, declared once with `#[ts_api]` (see `docs/platypus-api.md`): the
//! `Scene` namespace of `GameObject` handles and `Vector3` / `Quaternion` values. It sits next to
//! the op-based `ScriptScene`, which keeps working; this is what compiled scripts call directly.
//!
//! The functions work on the scene the host made current with [`enter`]. With none (or for a
//! stale id) they answer with a default: no object, the origin, the identity rotation.

use crate::{ObjectId, Scene, Transform};
use glam::{Quat, Vec3};
use platypus_math::{M4, Q, V3};
use std::cell::Cell;
use ts_api::{Handle, context, ts_api};

thread_local! {
    static SCENE: Cell<*mut Scene> = const { Cell::new(std::ptr::null_mut()) };
    /// Objects script destroyed (with their children) that the host has not been told about.
    static DESTROYED: std::cell::RefCell<Vec<ObjectId>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The objects destroyed through the script API since the last call (each object once, children
/// included): the host removes their components and anything else attached to them.
pub fn take_destroyed() -> Vec<ObjectId> {
    DESTROYED.with(|d| std::mem::take(&mut *d.borrow_mut()))
}

fn destroyed(list: Vec<ObjectId>) {
    DESTROYED.with(|d| d.borrow_mut().extend(list));
}

/// Makes `scene` the one the `Scene` API functions work on, on this thread, until the scope is
/// dropped. The host does this around running scripts.
pub fn enter(scene: &mut Scene) -> context::Scope<'_, Scene> {
    context::enter(&SCENE, scene)
}

/// A handle is the object's index and generation, offset so that **no live object is 0** (0 means
/// "no object" in a script's `GameObject | null`).
impl Handle for ObjectId {
    fn to_raw(self) -> u64 {
        ((self.generation as u64 + 1) << 32) | self.index as u64
    }
    fn from_raw(raw: u64) -> Self {
        ObjectId {
            index: raw as u32,
            generation: ((raw >> 32) as u32).wrapping_sub(1),
        }
    }
}

/// The `Transform` of a `GameObject`: the same object seen as its transform. Scripts get one with
/// `go.transform`, and it shares the object's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransformRef(pub ObjectId);

impl Handle for TransformRef {
    fn to_raw(self) -> u64 {
        self.0.to_raw()
    }
    fn from_raw(raw: u64) -> Self {
        TransformRef(ObjectId::from_raw(raw))
    }
}

fn with<R>(f: impl FnOnce(&mut Scene) -> R) -> Option<R> {
    context::with(&SCENE, f)
}

/// Runs `f` on the scene the host made current with [`enter`] (`None` when there is none): for
/// engine systems (animation) that work on the scene next to the script API.
pub fn with_scene<R>(f: impl FnOnce(&mut Scene) -> R) -> Option<R> {
    with(f)
}

/// The world position, rotation and scale of an object, composed down its parent chain the way
/// `scene.ts` does it: rotations multiply and scales multiply component-wise (so a rotated
/// non-uniformly scaled parent gives no shear, as in Unity).
fn world_pose(scene: &Scene, object: ObjectId) -> (V3, Q, V3) {
    let one = V3 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    let mut chain = vec![object];
    while let Some(p) = scene.parent(*chain.last().expect("not empty")) {
        chain.push(p);
    }
    let (mut pos, mut rot, mut scale) = (V3::ZERO, Q::IDENTITY, one);
    for id in chain.into_iter().rev() {
        let Some(t) = scene.local(id) else {
            continue;
        };
        let (lp, lq, ls) = (v3_of(t.position), q_of(t.rotation), v3_of(t.scale));
        pos = pos.add(rot.rotate(V3 {
            x: scale.x * lp.x,
            y: scale.y * lp.y,
            z: scale.z * lp.z,
        }));
        rot = rot.mul(lq);
        scale = V3 {
            x: scale.x * ls.x,
            y: scale.y * ls.y,
            z: scale.z * ls.z,
        };
    }
    (pos, rot, scale)
}

/// The pose of the parent chain above an object (the identity for a root).
fn parent_pose(scene: &Scene, object: ObjectId) -> (V3, Q, V3) {
    match scene.parent(object) {
        Some(p) => world_pose(scene, p),
        None => (
            V3::ZERO,
            Q::IDENTITY,
            V3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        ),
    }
}

/// The script's doubles as the scene's floats, and back.
pub fn vec3(v: V3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

pub fn v3_of(v: Vec3) -> V3 {
    V3 {
        x: f64::from(v.x),
        y: f64::from(v.y),
        z: f64::from(v.z),
    }
}

pub fn quat(q: Q) -> Quat {
    Quat::from_xyzw(q.x as f32, q.y as f32, q.z as f32, q.w as f32)
}

pub fn q_of(q: Quat) -> Q {
    Q {
        x: f64::from(q.x),
        y: f64::from(q.y),
        z: f64::from(q.z),
        w: f64::from(q.w),
    }
}

fn none() -> ObjectId {
    // `from_raw(0)` wraps to a generation no slot has, so it misses everywhere.
    ObjectId::from_raw(0)
}

#[ts_api(
    namespace = "Scene",
    handles(GameObject = ObjectId, Transform = TransformRef),
    values(
        Vector3 = V3 [x: f64, y: f64, z: f64],
        Quaternion = Q [x: f64, y: f64, z: f64, w: f64],
        Matrix4x4 = M4 [m0: f64, m1: f64, m2: f64, m3: f64, m4: f64, m5: f64, m6: f64, m7: f64,
                        m8: f64, m9: f64, m10: f64, m11: f64, m12: f64, m13: f64, m14: f64, m15: f64]
    )
)]
pub mod scene_api {
    use super::{
        M4, ObjectId, Q, Transform, TransformRef, V3, none, parent_pose, q_of, quat, v3_of, vec3,
        with, world_pose,
    };
    use glam::{Quat, Vec3};
    use ts_api::Handle;

    // `GameObject`: creating, destroying, flags and the transform.
    impl ObjectId {
        /// Creates an enabled root object at the origin.
        pub fn create() -> ObjectId {
            with(|s| s.create("GameObject")).unwrap_or_else(none)
        }

        /// Destroys the object and everything under it.
        pub fn destroy(self) {
            super::destroyed(with(|s| s.destroy(self)).unwrap_or_default());
        }

        /// Does the object exist (it may have been destroyed)?
        pub fn exists(self) -> bool {
            with(|s| s.contains(self)).unwrap_or(false)
        }

        /// The first object with this name, or no object.
        pub fn find(name: &str) -> ObjectId {
            with(|s| s.find(name)).flatten().unwrap_or_else(none)
        }

        /// Its name.
        #[property]
        pub fn name(self) -> String {
            with(|s| s.name(self).map(str::to_owned))
                .flatten()
                .unwrap_or_default()
        }

        pub fn set_name(self, name: &str) {
            with(|s| s.set_name(self, name));
        }

        /// Its transform.
        #[property]
        pub fn transform(self) -> TransformRef {
            TransformRef(self)
        }

        /// Is it switched on? (Its parents may still be off: see `activeInHierarchy`.)
        #[property]
        pub fn enabled(self) -> bool {
            with(|s| s.enabled(self)).unwrap_or(false)
        }

        pub fn set_enabled(self, enabled: bool) {
            with(|s| s.set_enabled(self, enabled));
        }

        /// Enabled, and so are all its parents.
        #[property]
        pub fn active_in_hierarchy(self) -> bool {
            with(|s| s.active(self)).unwrap_or(false)
        }

        /// Render group 0 to 31.
        #[property]
        pub fn layer(self) -> u32 {
            with(|s| s.layer(self) as u32).unwrap_or(0)
        }

        pub fn set_layer(self, layer: u32) {
            with(|s| s.set_layer(self, layer.min(31) as u8));
        }

        /// Unity's tag, a name for finding objects ("Untagged" by default).
        #[property]
        pub fn tag(self) -> String {
            with(|s| s.tag_name(self).to_owned()).unwrap_or_else(|| "Untagged".to_owned())
        }

        pub fn set_tag(self, tag: &str) {
            with(|s| s.set_tag_name(self, tag));
        }

        /// Has it this tag?
        pub fn compare_tag(self, tag: &str) -> bool {
            with(|s| s.tag_name(self) == tag).unwrap_or(false)
        }

        /// The first active object with this tag, or `null`.
        #[nullable]
        pub fn find_with_tag(tag: &str) -> ObjectId {
            with(|s| s.find_with_tag(tag).first().copied())
                .flatten()
                .unwrap_or_else(none)
        }

        /// Every active object with this tag.
        pub fn find_game_objects_with_tag(tag: &str) -> Vec<ObjectId> {
            with(|s| s.find_with_tag(tag)).unwrap_or_default()
        }

        /// The object it is under (`null` at the root).
        #[property]
        #[nullable]
        pub fn parent(self) -> ObjectId {
            parent(self)
        }

        /// Moves the object under `parent` (no object: to the world root). It keeps its place in the
        /// world unless `keepWorld` is false, in which case it keeps its local transform.
        #[optional(keep_world = true)]
        pub fn set_parent(self, parent: ObjectId, keep_world: bool) {
            set_parent(self, parent, keep_world);
        }

        /// How many children it has.
        #[property]
        pub fn child_count(self) -> f64 {
            with(|s| s.children(self).len() as f64).unwrap_or(0.0)
        }

        /// The i-th child (no object past the end).
        pub fn child_at(self, index: f64) -> ObjectId {
            with(|s| s.children(self).get(index as usize).copied())
                .flatten()
                .unwrap_or_else(none)
        }

        /// A child by name (direct children only; no object if none).
        pub fn find_child(self, name: &str) -> ObjectId {
            with(|s| {
                s.children(self)
                    .iter()
                    .copied()
                    .find(|c| s.name(*c) == Some(name))
            })
            .flatten()
            .unwrap_or_else(none)
        }

        /// The topmost object above it (itself at the root).
        #[property]
        pub fn root(self) -> ObjectId {
            with(|s| {
                let mut o = self;
                while let Some(p) = s.parent(o) {
                    o = p;
                }
                o
            })
            .unwrap_or(self)
        }

        /// True once the object was destroyed.
        #[property]
        pub fn destroyed(self) -> bool {
            !with(|s| s.contains(self)).unwrap_or(false)
        }

        /// Flags that keep it out of the editor views and saved scenes (`HideFlags` bits).
        #[property]
        pub fn hide_flags(self) -> f64 {
            with(|s| f64::from(s.hide_flags(self))).unwrap_or(0.0)
        }

        pub fn set_hide_flags(self, flags: f64) {
            with(|s| s.set_hide_flags(self, flags as u32));
        }

        /// A number that names the object while it lives (the same object, the same number).
        #[property]
        pub fn id(self) -> f64 {
            self.to_raw() as f64
        }

        /// An object by its `id` (no object if it is gone).
        pub fn by_id(id: f64) -> ObjectId {
            let o = ObjectId::from_raw(id as u64);
            if with(|s| s.contains(o)).unwrap_or(false) {
                o
            } else {
                none()
            }
        }

        /// How many objects are at the world root.
        pub fn root_count() -> f64 {
            with(|s| s.roots().len() as f64).unwrap_or(0.0)
        }

        /// The i-th root object (no object past the end).
        pub fn root_at(index: f64) -> ObjectId {
            with(|s| s.roots().get(index as usize).copied())
                .flatten()
                .unwrap_or_else(none)
        }
    }

    // `Transform`: position, rotation and scale, and the hierarchy.
    impl TransformRef {
        /// The object this is the transform of.
        #[property]
        pub fn game_object(self) -> ObjectId {
            self.0
        }

        /// Position in the world.
        #[property]
        pub fn position(self) -> V3 {
            position(self.0)
        }

        /// Puts the object at a world position, keeping its world rotation.
        pub fn set_position(self, position: V3) {
            set_position(self.0, position);
        }

        /// Rotation in the world.
        #[property]
        pub fn rotation(self) -> Q {
            rotation(self.0)
        }

        /// Rotates the object in the world, keeping its world position.
        pub fn set_rotation(self, rotation: Q) {
            set_rotation(self.0, rotation);
        }

        /// Position relative to the parent.
        #[property]
        pub fn local_position(self) -> V3 {
            local_position(self.0)
        }

        pub fn set_local_position(self, position: V3) {
            set_local_position(self.0, position);
        }

        #[property]
        pub fn local_rotation(self) -> Q {
            local_rotation(self.0)
        }

        pub fn set_local_rotation(self, rotation: Q) {
            set_local_rotation(self.0, rotation);
        }

        #[property]
        pub fn local_scale(self) -> V3 {
            local_scale(self.0)
        }

        pub fn set_local_scale(self, scale: V3) {
            set_local_scale(self.0, scale);
        }

        /// Local rotation as Euler angles in degrees.
        #[property]
        pub fn local_euler_angles(self) -> V3 {
            local_rotation(self.0).euler_angles()
        }

        pub fn set_local_euler_angles(self, v: V3) {
            set_local_rotation(self.0, Q::euler(v.x, v.y, v.z));
        }

        /// World rotation as Euler angles in degrees.
        #[property]
        pub fn euler_angles(self) -> V3 {
            rotation(self.0).euler_angles()
        }

        pub fn set_euler_angles(self, v: V3) {
            set_rotation(self.0, Q::euler(v.x, v.y, v.z));
        }

        /// World scale (approximate under rotated, non-uniformly scaled parents, like Unity).
        #[property]
        pub fn lossy_scale(self) -> V3 {
            lossy_scale(self.0)
        }

        /// The object forward (-z) direction in the world.
        #[property]
        pub fn forward(self) -> V3 {
            rotation(self.0).rotate(V3 {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            })
        }

        /// The object right (+x) direction in the world.
        #[property]
        pub fn right(self) -> V3 {
            rotation(self.0).rotate(V3::RIGHT)
        }

        /// The object up (+y) direction in the world.
        #[property]
        pub fn up(self) -> V3 {
            rotation(self.0).rotate(V3::UP)
        }

        /// Moves by `delta`, in world space (or along the object own axes with `"self"`).
        #[optional(space)]
        pub fn translate(self, delta: V3, space: &str) {
            let d = if space == "self" {
                rotation(self.0).rotate(delta)
            } else {
                delta
            };
            let p = position(self.0);
            set_position(self.0, p.add(d));
        }

        /// Rotates by Euler angles in degrees, about the object own axes (or the world with
        /// `"world"`).
        #[optional(space)]
        pub fn rotate(self, euler: V3, space: &str) {
            let q = Q::euler(euler.x, euler.y, euler.z);
            let r = rotation(self.0);
            set_rotation(self.0, if space == "world" { q.mul(r) } else { r.mul(q) });
        }

        /// Turns the object so forward points at `target`.
        pub fn look_at(self, target: V3) {
            let d = target.sub(position(self.0));
            set_rotation(self.0, Q::look_rotation(d, V3::UP));
        }

        /// Like `lookAt`, with the direction that should stay up.
        pub fn look_at_up(self, target: V3, up: V3) {
            let d = target.sub(position(self.0));
            set_rotation(self.0, Q::look_rotation(d, up));
        }

        /// A point from the object local space to the world.
        pub fn transform_point(self, p: V3) -> V3 {
            let s = lossy_scale(self.0);
            position(self.0).add(rotation(self.0).rotate(V3 {
                x: s.x * p.x,
                y: s.y * p.y,
                z: s.z * p.z,
            }))
        }

        /// A point from the world to the object local space.
        pub fn inverse_transform_point(self, p: V3) -> V3 {
            let s = lossy_scale(self.0);
            let q = rotation(self.0).inverse().rotate(p.sub(position(self.0)));
            V3 {
                x: q.x / s.x,
                y: q.y / s.y,
                z: q.z / s.z,
            }
        }

        /// A direction from local space to the world (rotation only).
        pub fn transform_direction(self, d: V3) -> V3 {
            rotation(self.0).rotate(d)
        }

        /// A direction from the world to local space (rotation only).
        pub fn inverse_transform_direction(self, d: V3) -> V3 {
            rotation(self.0).inverse().rotate(d)
        }

        /// A vector from local space to the world: rotated and scaled, not moved.
        pub fn transform_vector(self, v: V3) -> V3 {
            let s = lossy_scale(self.0);
            rotation(self.0).rotate(V3 {
                x: s.x * v.x,
                y: s.y * v.y,
                z: s.z * v.z,
            })
        }

        /// A vector from the world to local space: unrotated and unscaled, not moved.
        pub fn inverse_transform_vector(self, v: V3) -> V3 {
            let s = lossy_scale(self.0);
            let q = rotation(self.0).inverse().rotate(v);
            V3 {
                x: q.x / s.x,
                y: q.y / s.y,
                z: q.z / s.z,
            }
        }

        /// Puts the object at a world position and rotation at once.
        pub fn set_position_and_rotation(self, position: V3, rotation: Q) {
            set_position(self.0, position);
            set_rotation(self.0, rotation);
        }

        /// Puts the object at a position and rotation relative to its parent at once.
        pub fn set_local_position_and_rotation(self, position: V3, rotation: Q) {
            set_local_position(self.0, position);
            set_local_rotation(self.0, rotation);
        }

        /// Turns the object about an axis through a world point, by `angle` degrees: it orbits the
        /// point and turns with it.
        pub fn rotate_around(self, point: V3, axis: V3, angle: f64) {
            let q = Q::angle_axis(angle, axis);
            let at = position(self.0);
            set_position(self.0, point.add(q.rotate(at.sub(point))));
            set_rotation(self.0, q.mul(rotation(self.0)));
        }

        /// The parent transform (`null` for a root).
        #[property]
        #[nullable]
        pub fn parent(self) -> TransformRef {
            TransformRef(parent(self.0))
        }

        /// Moves the object under a parent (none: to the root), optionally keeping its world pose.
        pub fn set_parent(self, parent: TransformRef, keep_world: bool) {
            set_parent(self.0, parent.0, keep_world);
        }

        /// How many children it has.
        #[property]
        pub fn child_count(self) -> f64 {
            with(|s| s.children(self.0).len() as f64).unwrap_or(0.0)
        }

        /// The i-th child (null past the end).
        #[nullable]
        pub fn get_child(self, index: f64) -> TransformRef {
            TransformRef(
                with(|s| s.children(self.0).get(index as usize).copied())
                    .flatten()
                    .unwrap_or_else(none),
            )
        }

        /// The topmost transform above it (itself at the root).
        #[property]
        pub fn root(self) -> TransformRef {
            TransformRef(
                with(|s| {
                    let mut o = self.0;
                    while let Some(p) = s.parent(o) {
                        o = p;
                    }
                    o
                })
                .unwrap_or(self.0),
            )
        }

        /// A transform by a path of names ("arm/hand"), null if there is none. Each step is a child
        /// of the one before; `..` goes up to the parent and `.` stays; a leading `/` starts at
        /// the world root, where the first name is a root object.
        #[nullable]
        pub fn find(self, path: &str) -> TransformRef {
            TransformRef(
                with(|s| {
                    let (absolute, rest) = match path.strip_prefix('/') {
                        Some(rest) => (true, rest),
                        None => (false, path),
                    };
                    let mut at = (!absolute).then_some(self.0);
                    for name in rest.split('/').filter(|n| !n.is_empty()) {
                        at = match name {
                            "." => at,
                            ".." => Some(at.and_then(|a| s.parent(a))?),
                            _ => {
                                let list = match at {
                                    Some(a) => s.children(a),
                                    None => s.roots(),
                                };
                                Some(list.iter().copied().find(|c| s.name(*c) == Some(name))?)
                            }
                        };
                    }
                    at
                })
                .flatten()
                .unwrap_or_else(none),
            )
        }

        /// The matrix from the object's local space to the world (position, rotation and world
        /// scale, no shear).
        #[property]
        pub fn local_to_world_matrix(self) -> M4 {
            with(|s| {
                s.contains(self.0).then(|| {
                    let (p, r, sc) = world_pose(s, self.0);
                    M4::trs(p, r, sc)
                })
            })
            .flatten()
            .unwrap_or(M4::IDENTITY)
        }

        /// The matrix from the world to the object's local space.
        #[property]
        pub fn world_to_local_matrix(self) -> M4 {
            self.local_to_world_matrix().inverse()
        }

        /// Is it `parent` or anywhere under it?
        pub fn is_child_of(self, parent: TransformRef) -> bool {
            with(|s| s.is_descendant(self.0, parent.0)).unwrap_or(false)
        }

        /// Frees every child to the world root, where it is.
        pub fn detach_children(self) {
            let kids = with(|s| s.children(self.0).to_vec()).unwrap_or_default();
            for k in kids {
                set_parent(k, none(), true);
            }
        }

        /// Where it stands among its siblings, first is 0.
        #[property]
        pub fn sibling_index(self) -> f64 {
            with(|s| s.sibling_index(self.0) as f64).unwrap_or(0.0)
        }

        pub fn set_sibling_index(self, index: f64) {
            with(|s| s.set_sibling_index(self.0, index.max(0.0) as usize));
        }

        /// Moves it before its siblings.
        pub fn set_as_first_sibling(self) {
            with(|s| s.set_sibling_index(self.0, 0));
        }

        /// Moves it after its siblings.
        pub fn set_as_last_sibling(self) {
            with(|s| s.set_sibling_index(self.0, usize::MAX));
        }
    }

    /// Creates an enabled root object at the origin.
    pub fn create() -> ObjectId {
        with(|s| s.create("GameObject")).unwrap_or_else(none)
    }

    /// Destroys an object and everything under it.
    pub fn destroy(object: ObjectId) {
        super::destroyed(with(|s| s.destroy(object)).unwrap_or_default());
    }

    /// Does the object exist (it may have been destroyed)?
    pub fn exists(object: ObjectId) -> bool {
        with(|s| s.contains(object)).unwrap_or(false)
    }

    /// How many objects there are.
    pub fn count() -> u32 {
        with(|s| s.len() as u32).unwrap_or(0)
    }

    /// The parent, or no object for a root.
    pub fn parent(object: ObjectId) -> ObjectId {
        with(|s| s.parent(object)).flatten().unwrap_or_else(none)
    }

    /// Moves an object under a parent (no object: to the root), optionally keeping its world pose.
    pub fn set_parent(object: ObjectId, parent: ObjectId, keep_world: bool) {
        // The world pose is read and put back the way `scene.ts` does it (a rotated, non-uniformly
        // scaled parent gives no shear), not through the matrices.
        let pose = with(|s| s.contains(object).then(|| world_pose(s, object))).flatten();
        with(|s| {
            let p = s.contains(parent).then_some(parent);
            s.set_parent(object, p, false)
        });
        if keep_world && let Some((pos, rot, scale)) = pose {
            set_position(object, pos);
            set_rotation(object, rot);
            // Unity keeps the world scale too: the local scale is what the parents leave over.
            let (_, _, ps) =
                with(|s| parent_pose(s, object)).unwrap_or((V3::ZERO, Q::IDENTITY, scale));
            let div = |a: f64, b: f64| if b == 0.0 { a } else { a / b };
            set_local_scale(
                object,
                V3 {
                    x: div(scale.x, ps.x),
                    y: div(scale.y, ps.y),
                    z: div(scale.z, ps.z),
                },
            );
        }
    }

    pub fn enabled(object: ObjectId) -> bool {
        with(|s| s.enabled(object)).unwrap_or(false)
    }

    pub fn set_enabled(object: ObjectId, enabled: bool) {
        with(|s| s.set_enabled(object, enabled));
    }

    /// Enabled, and so are all its parents.
    pub fn active(object: ObjectId) -> bool {
        with(|s| s.active(object)).unwrap_or(false)
    }

    pub fn layer(object: ObjectId) -> u32 {
        with(|s| s.layer(object) as u32).unwrap_or(0)
    }

    pub fn set_layer(object: ObjectId, layer: u32) {
        with(|s| s.set_layer(object, layer.min(31) as u8));
    }

    /// Position relative to the parent.
    pub fn local_position(object: ObjectId) -> V3 {
        v3_of(
            with(|s| s.local(object).map(|t| t.position))
                .flatten()
                .unwrap_or_default(),
        )
    }

    pub fn set_local_position(object: ObjectId, position: V3) {
        let position = vec3(position);
        with(|s| {
            if let Some(t) = s.local(object) {
                s.set_local(object, Transform { position, ..t });
            }
        });
    }

    pub fn local_rotation(object: ObjectId) -> Q {
        q_of(
            with(|s| s.local(object).map(|t| t.rotation))
                .flatten()
                .unwrap_or(Quat::IDENTITY),
        )
    }

    pub fn set_local_rotation(object: ObjectId, rotation: Q) {
        let rotation = quat(rotation);
        with(|s| {
            if let Some(t) = s.local(object) {
                s.set_local(
                    object,
                    Transform {
                        rotation: rotation.normalize(),
                        ..t
                    },
                );
            }
        });
    }

    pub fn local_scale(object: ObjectId) -> V3 {
        v3_of(
            with(|s| s.local(object).map(|t| t.scale))
                .flatten()
                .unwrap_or(Vec3::ONE),
        )
    }

    pub fn set_local_scale(object: ObjectId, scale: V3) {
        let scale = vec3(scale);
        with(|s| {
            if let Some(t) = s.local(object) {
                s.set_local(object, Transform { scale, ..t });
            }
        });
    }

    /// World scale: the local scales up the parent chain, multiplied.
    pub fn lossy_scale(object: ObjectId) -> V3 {
        with(|s| world_pose(s, object).2).unwrap_or(V3 {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        })
    }

    /// Position in the world.
    pub fn position(object: ObjectId) -> V3 {
        with(|s| s.contains(object).then(|| world_pose(s, object).0))
            .flatten()
            .unwrap_or(V3::ZERO)
    }

    /// Puts the object at a world position, keeping its world rotation.
    pub fn set_position(object: ObjectId, position: V3) {
        with(|s| {
            if !s.contains(object) {
                return;
            }
            let (pp, pr, ps) = parent_pose(s, object);
            let d = pr.inverse().rotate(position.sub(pp));
            let local = vec3(V3 {
                x: d.x / ps.x,
                y: d.y / ps.y,
                z: d.z / ps.z,
            });
            if let Some(t) = s.local(object) {
                s.set_local(
                    object,
                    Transform {
                        position: local,
                        ..t
                    },
                );
            }
        });
    }

    /// Rotation in the world.
    pub fn rotation(object: ObjectId) -> Q {
        with(|s| s.contains(object).then(|| world_pose(s, object).1))
            .flatten()
            .unwrap_or(Q::IDENTITY)
    }

    /// Rotates the object in the world, keeping its world position.
    pub fn set_rotation(object: ObjectId, rotation: Q) {
        with(|s| {
            if !s.contains(object) {
                return;
            }
            let (_, pr, _) = parent_pose(s, object);
            let local = quat(pr.inverse().mul(rotation).normalized());
            if let Some(t) = s.local(object) {
                s.set_local(
                    object,
                    Transform {
                        rotation: local,
                        ..t
                    },
                );
            }
        });
    }
}
