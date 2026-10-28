// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Physics for scripts: a rapier world driven by ops (DESIGN.md §8.1).
//!
//! Scripts never hold rapier handles. They pick numeric IDs for bodies and colliders
//! (like UI nodes) and queue `physics.*` ops; [`Physics::apply`] runs them, [`Physics::step`]
//! advances the world one fixed tick, and [`Physics::take_batch`] returns what scripts
//! need back (moved bodies, collision events, query results) as one batch per tick.

pub mod cloth;

use std::collections::HashMap;
use std::sync::Mutex;

use platypus_core::Value;
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::prelude::*;

/// What a body or collider is called by scripts.
pub type ScriptId = u32;

/// A collision between two colliders, by script ID.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub started: bool,
    pub a: ScriptId,
    pub b: ScriptId,
    /// How fast they were moving towards or past each other (m/s), going into the
    /// contact: for impact sounds and damage. Zero when it stops.
    pub speed: f32,
    /// One of them is a sensor (a trigger: reports overlaps, doesn't push).
    pub sensor: bool,
    /// `b`'s velocity minus `a`'s going into the contact (its length is `speed`). Zero when
    /// it stops.
    pub velocity: [f32; 3],
}

/// One point of the contact between two colliders after a step, as seen from one of them
/// ("this"; [`Physics::contact_points`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactPoint {
    /// World position: halfway between the two surfaces.
    pub point: [f32; 3],
    /// World normal, pointing from the other collider towards this one.
    pub normal: [f32; 3],
    /// Distance between the surfaces along the normal (negative: overlapping).
    pub separation: f32,
    /// The solver's impulse along the normal at this point in the last step (N·s, pushing
    /// this collider's body along `normal`).
    pub normal_impulse: f32,
    /// The friction impulse on this collider's body (world, N·s), as the solver keeps it for
    /// warm starting the next step.
    pub tangent_impulse: [f32; 3],
    /// The other body's velocity at the point minus this body's.
    pub relative_velocity: [f32; 3],
}

/// A body's pose and velocity after a step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyState {
    pub id: ScriptId,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub velocity: [f32; 3],
    /// Radians per second about each world axis.
    pub angular_velocity: [f32; 3],
}

/// The answer to a `physics.raycast` query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub query: u32,
    /// The collider hit, its distance along the ray and the surface normal; `None`
    /// when nothing was hit.
    pub hit: Option<(ScriptId, f32, [f32; 3])>,
}

/// Everything scripts get back from one tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Batch {
    pub bodies: Vec<BodyState>,
    pub contacts: Vec<Contact>,
    pub rays: Vec<RayHit>,
    /// Character controllers moved this tick, and whether they ended on the ground.
    pub characters: Vec<(ScriptId, bool)>,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
            && self.contacts.is_empty()
            && self.rays.is_empty()
            && self.characters.is_empty()
    }

    /// The batch as `__platypus_physics(bodies, contacts, rays, characters)` arguments: flat
    /// number arrays, so a tick crosses the boundary as four arrays rather than objects.
    /// - bodies: `id, px, py, pz, qx, qy, qz, qw, vx, vy, vz, wx, wy, wz` each
    /// - contacts: `started (1/0), a, b, sensor (1/0), speed` each
    /// - rays: `query, collider (-1: miss), distance, nx, ny, nz` each
    /// - characters: `id, grounded (1/0)` each
    pub fn to_args(&self) -> Vec<Value> {
        let num = |v: f32| Value::Number(v as f64);
        let flag = |b: bool| Value::Number(if b { 1.0 } else { 0.0 });
        let mut bodies = Vec::with_capacity(self.bodies.len() * 14);
        for b in &self.bodies {
            bodies.push(b.id.into());
            bodies.extend(b.position.map(num));
            bodies.extend(b.rotation.map(num));
            bodies.extend(b.velocity.map(num));
            bodies.extend(b.angular_velocity.map(num));
        }
        let mut contacts = Vec::with_capacity(self.contacts.len() * 5);
        for c in &self.contacts {
            contacts.extend([
                flag(c.started),
                c.a.into(),
                c.b.into(),
                flag(c.sensor),
                num(c.speed),
            ]);
        }
        let mut rays = Vec::with_capacity(self.rays.len() * 6);
        for r in &self.rays {
            rays.push(r.query.into());
            match r.hit {
                Some((id, dist, n)) => {
                    rays.extend([id.into(), num(dist)]);
                    rays.extend(n.map(num));
                }
                None => rays.extend([-1.0, 0.0, 0.0, 0.0, 0.0].map(Value::Number)),
            }
        }
        let characters = self
            .characters
            .iter()
            .flat_map(|&(id, grounded)| [id.into(), flag(grounded)])
            .collect();
        vec![
            Value::Array(bodies),
            Value::Array(contacts),
            Value::Array(rays),
            Value::Array(characters),
        ]
    }
}

/// Collects rapier's collision events during a step, with the colliders' relative
/// velocity (events fire before the solver, so it's the velocity going into the contact).
#[derive(Default)]
struct Events(Mutex<Vec<(CollisionEvent, Vector)>>);

impl EventHandler for Events {
    fn handle_collision_event(
        &self,
        bodies: &RigidBodySet,
        colliders: &ColliderSet,
        event: CollisionEvent,
        _pair: Option<&ContactPair>,
    ) {
        let velocity = |h: ColliderHandle| {
            colliders
                .get(h)
                .and_then(|c| bodies.get(c.parent()?))
                .map_or(Vector::ZERO, |b| b.linvel())
        };
        let relative = if event.started() {
            velocity(event.collider2()) - velocity(event.collider1())
        } else {
            Vector::ZERO
        };
        if let Ok(mut events) = self.0.lock() {
            events.push((event, relative));
        }
    }

    fn handle_contact_force_event(
        &self,
        _dt: Real,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        _pair: &ContactPair,
        _total_force_magnitude: Real,
    ) {
    }

    fn handle_soft_body_tear_event(&self, _soft_bodies: &SoftBodySet, _event: &SoftBodyTearEvent) {}
}

/// Colliders (by script id, smaller first) that must not touch each other
/// (`Physics.IgnoreCollision`).
type IgnoredPairs = std::collections::HashSet<(ScriptId, ScriptId)>;

/// Applies the ignored pairs. Setting the hook flag on a collider replaces rapier's default
/// pair filter (no contacts between two bodies that can't move), so that is repeated here.
struct IgnoreHooks<'a>(&'a IgnoredPairs);

impl IgnoreHooks<'_> {
    fn ignored(&self, colliders: &ColliderSet, a: ColliderHandle, b: ColliderHandle) -> bool {
        let (Some(a), Some(b)) = (colliders.get(a), colliders.get(b)) else {
            return false;
        };
        let (a, b) = (a.user_data as ScriptId, b.user_data as ScriptId);
        self.0.contains(&(a.min(b), a.max(b)))
    }
}

impl PhysicsHooks for IgnoreHooks<'_> {
    fn filter_contact_pair(&self, c: &PairFilterContext) -> Option<SolverFlags> {
        if self.ignored(c.colliders, c.collider1, c.collider2) {
            return None;
        }
        let dynamic = |h: Option<RigidBodyHandle>| {
            h.and_then(|h| c.bodies.get(h))
                .is_some_and(|b| b.is_dynamic())
        };
        (dynamic(c.rigid_body1) || dynamic(c.rigid_body2))
            .then_some(SolverFlags::COMPUTE_RIGID_IMPULSES)
    }

    fn filter_intersection_pair(&self, c: &PairFilterContext) -> bool {
        !self.ignored(c.colliders, c.collider1, c.collider2)
    }
}

/// The physics world scripts drive.
pub struct Physics {
    world: PhysicsWorld,
    bodies: HashMap<ScriptId, RigidBodyHandle>,
    colliders: HashMap<ScriptId, ColliderHandle>,
    /// Bodies moved by a character controller: the controller and the move asked for
    /// this tick.
    characters: HashMap<ScriptId, (KinematicCharacterController, Option<Vector>)>,
    /// Joints by script id.
    joints: HashMap<ScriptId, ImpulseJointHandle>,
    /// Movement axes Platypus holds in place itself (the value each is held at), instead of
    /// rapier's lock: see [`Self::set_constraints`].
    pins: HashMap<ScriptId, [Option<f32>; 3]>,
    /// A fixed body for joints to the world (`null` for the second body), made when
    /// first needed.
    world_anchor: Option<RigidBodyHandle>,
    /// Collider pairs that don't collide (`physics.ignore`).
    ignored: IgnoredPairs,
    /// Raycasts queued this tick, answered after the step.
    rays: Vec<(u32, Ray, f32)>,
    /// Colliders were added or removed since the last step: they aren't in the query
    /// structures until a step runs, so character moves wait a step (moving through a
    /// floor that was just added would be wrong).
    dirty: bool,
    batch: Batch,
}

impl Default for Physics {
    fn default() -> Self {
        Self::new()
    }
}

impl Physics {
    /// An empty world with Earth gravity (y up, metres).
    pub fn new() -> Self {
        let mut world = PhysicsWorld::new();
        world.gravity = Vector::new(0.0, -9.81, 0.0);
        Self {
            world,
            bodies: HashMap::new(),
            colliders: HashMap::new(),
            characters: HashMap::new(),
            rays: Vec::new(),
            joints: HashMap::new(),
            pins: HashMap::new(),
            world_anchor: None,
            ignored: IgnoredPairs::new(),
            dirty: false,
            batch: Batch::default(),
        }
    }

    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    pub fn collider_count(&self) -> usize {
        self.colliders.len()
    }

    /// A body's position, if it exists.
    pub fn position(&self, id: ScriptId) -> Option<[f32; 3]> {
        let body = self.world.bodies.get(*self.bodies.get(&id)?)?;
        Some(body.translation().to_array())
    }

    /// A body's linear velocity, if it exists.
    pub fn velocity(&self, id: ScriptId) -> Option<[f32; 3]> {
        let body = self.world.bodies.get(*self.bodies.get(&id)?)?;
        Some(body.linvel().to_array())
    }

    /// A body's angular velocity (radians per second about each world axis).
    pub fn angular_velocity(&self, id: ScriptId) -> Option<[f32; 3]> {
        let body = self.world.bodies.get(*self.bodies.get(&id)?)?;
        Some(body.angvel().to_array())
    }

    /// The body a collider is attached to (none for a world collider or an unknown id).
    pub fn collider_body(&self, collider: ScriptId) -> Option<ScriptId> {
        let parent = self
            .world
            .colliders
            .get(*self.colliders.get(&collider)?)?
            .parent()?;
        self.bodies
            .iter()
            .find_map(|(&id, &h)| (h == parent).then_some(id))
    }

    /// The linear velocity of a collider's body (zero for a world collider or an unknown id).
    pub fn collider_velocity(&self, collider: ScriptId) -> [f32; 3] {
        self.colliders
            .get(&collider)
            .and_then(|h| self.world.colliders.get(*h))
            .and_then(|c| self.world.bodies.get(c.parent()?))
            .map_or([0.0; 3], |b| b.linvel().to_array())
    }

    /// A collider's world-space bounding box (min, max).
    pub fn collider_aabb(&self, collider: ScriptId) -> Option<([f32; 3], [f32; 3])> {
        let c = self.world.colliders.get(*self.colliders.get(&collider)?)?;
        let aabb = c.compute_aabb();
        Some((aabb.mins.to_array(), aabb.maxs.to_array()))
    }

    /// The point of a collider closest to `point` (the point itself when it is inside).
    pub fn closest_point(&self, collider: ScriptId, point: [f32; 3]) -> Option<[f32; 3]> {
        let c = self.world.colliders.get(*self.colliders.get(&collider)?)?;
        let p = c
            .shape()
            .project_point(c.position(), Vector::from_array(point), true);
        Some(p.point.to_array())
    }

    /// Appends the points where `this` touches `other` after the last step, as seen from `this`
    /// (normals point towards it, impulses push it). Nothing when they do not touch, one of them
    /// is a sensor, or an id is unknown. Reads the narrow phase: no copy is kept per step.
    pub fn contact_points(&self, this: ScriptId, other: ScriptId, out: &mut Vec<ContactPoint>) {
        let (Some(&h_this), Some(&h_other)) =
            (self.colliders.get(&this), self.colliders.get(&other))
        else {
            return;
        };
        let Some(pair) = self.world.narrow_phase.contact_pair(h_this, h_other) else {
            return;
        };
        let (Some(c1), Some(c2)) = (
            self.world.colliders.get(pair.collider1),
            self.world.colliders.get(pair.collider2),
        ) else {
            return;
        };
        // rapier's normal points from the pair's first collider to its second (flipped when
        // "this" is the first), and its friction impulse is the one on the first collider's body
        // (flipped when "this" is the second).
        let flip = pair.collider1 == h_this;
        let sign = if flip { -1.0 } else { 1.0 };
        let point_velocity = |c: &Collider, p: Vector| {
            c.parent()
                .and_then(|b| self.world.bodies.get(b))
                .map_or(Vector::ZERO, |b| b.velocity_at_point(p))
        };
        for m in pair.solver_manifolds() {
            if m.data.solver_contacts.is_empty() {
                continue;
            }
            let (pose1, pose2) = match &m.subshape_poses {
                Some(s) => (
                    s.pos1.map_or(*c1.position(), |p| *c1.position() * p),
                    s.pos2.map_or(*c2.position(), |p| *c2.position() * p),
                ),
                None => (*c1.position(), *c2.position()),
            };
            let normal = m.data.normal * sign;
            for p in &m.points {
                let w1 = pose1.transform_point(p.local_p1);
                let w2 = pose2.transform_point(p.local_p2);
                let at = (w1 + w2) * 0.5;
                let (mine, theirs) = if flip { (c1, c2) } else { (c2, c1) };
                let relative = point_velocity(theirs, at) - point_velocity(mine, at);
                out.push(ContactPoint {
                    point: at.to_array(),
                    normal: normal.to_array(),
                    separation: p.dist,
                    normal_impulse: p.data.impulse,
                    tangent_impulse: (p.data.warmstart_tangent_world * -sign).to_array(),
                    relative_velocity: relative.to_array(),
                });
            }
        }
    }

    /// Casts a ray at once: the first collider hit within `max` metres (its id, the distance and
    /// the surface normal). Colliders added since the last step are not in the world's query
    /// structures yet, so then every collider is tested directly. `accept` says which colliders
    /// count (by script id, e.g. a layer mask); a collider the origin is inside is skipped, as
    /// in Unity.
    pub fn raycast_now(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max: f32,
        accept: &dyn Fn(ScriptId) -> bool,
    ) -> Option<(ScriptId, f32, [f32; 3])> {
        let dir = Vector::from_array(direction);
        if dir.length_squared() == 0.0 {
            return None;
        }
        let ray = Ray::new(Vector::from_array(origin), dir.normalize());
        let id_of = |h: ColliderHandle| self.world.colliders.get(h).map(|c| c.user_data as u32);
        let usable = |c: &Collider| {
            accept(c.user_data as u32) && !c.shape().contains_point(c.position(), ray.origin)
        };
        if !self.dirty {
            let accepted = |_: ColliderHandle, c: &Collider| usable(c);
            let filter = QueryFilter::default().predicate(&accepted);
            let (h, hit) = self
                .world
                .cast_ray_and_get_normal(&ray, max, true, filter)?;
            return Some((id_of(h)?, hit.time_of_impact, hit.normal.to_array()));
        }
        self.colliders
            .values()
            .filter_map(|&h| {
                let c = self.world.colliders.get(h)?;
                if !usable(c) {
                    return None;
                }
                let hit = c
                    .shape()
                    .cast_ray_and_get_normal(c.position(), &ray, max, true)?;
                Some((
                    c.user_data as u32,
                    hit.time_of_impact,
                    hit.normal.to_array(),
                ))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Casts a ray at once and answers every collider it hits within `max` metres, nearest first
    /// (id, distance, surface normal). `accept` and the start-inside rule are those of
    /// [`Self::raycast_now`].
    pub fn raycast_all(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max: f32,
        accept: &dyn Fn(ScriptId) -> bool,
    ) -> Vec<(ScriptId, f32, [f32; 3])> {
        let dir = Vector::from_array(direction);
        if dir.length_squared() == 0.0 {
            return Vec::new();
        }
        let ray = Ray::new(Vector::from_array(origin), dir.normalize());
        let usable = |c: &Collider| {
            accept(c.user_data as u32) && !c.shape().contains_point(c.position(), ray.origin)
        };
        let mut hits: Vec<(ScriptId, f32, [f32; 3])> = if self.dirty {
            self.colliders
                .values()
                .filter_map(|&h| {
                    let c = self.world.colliders.get(h)?;
                    if !usable(c) {
                        return None;
                    }
                    let hit = c
                        .shape()
                        .cast_ray_and_get_normal(c.position(), &ray, max, true)?;
                    Some((
                        c.user_data as u32,
                        hit.time_of_impact,
                        hit.normal.to_array(),
                    ))
                })
                .collect()
        } else {
            let accepted = |_: ColliderHandle, c: &Collider| usable(c);
            let filter = QueryFilter::default().predicate(&accepted);
            self.world
                .intersect_ray(ray, max, true, filter)
                .map(|(_, c, hit)| {
                    (
                        c.user_data as u32,
                        hit.time_of_impact,
                        hit.normal.to_array(),
                    )
                })
                .collect()
        };
        hits.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        hits
    }

    /// Every collider a sphere touches (inside it counts), by id, ascending. `accept` says which
    /// colliders count; sensors are included, as in Unity.
    pub fn overlap_sphere(
        &self,
        center: [f32; 3],
        radius: f32,
        accept: &dyn Fn(ScriptId) -> bool,
    ) -> Vec<ScriptId> {
        let ball = rapier3d::parry::shape::Ball::new(radius.max(0.0));
        let pose = Pose::from_parts(Vector::from_array(center), Rotation::IDENTITY);
        let touches = |c: &Collider| {
            accept(c.user_data as u32)
                && rapier3d::parry::query::intersection_test(&pose, &ball, c.position(), c.shape())
                    .is_ok_and(|i| i.intersecting)
        };
        let mut ids: Vec<ScriptId> = if self.dirty {
            self.colliders
                .values()
                .filter_map(|&h| self.world.colliders.get(h))
                .filter(|c| touches(c))
                .map(|c| c.user_data as u32)
                .collect()
        } else {
            let accepted = |_: ColliderHandle, c: &Collider| touches(c);
            let filter = QueryFilter::default().predicate(&accepted);
            self.world
                .intersect_shape(pose, &ball, filter)
                .map(|(_, c)| c.user_data as u32)
                .collect()
        };
        ids.sort_unstable();
        ids
    }

    /// Runs one `physics.*` op (the name without the prefix). Returns an error message
    /// for a bad op; nothing is changed then.
    pub fn apply(&mut self, name: &str, args: &[Value]) -> Result<(), String> {
        let a = Args(args);
        self.dirty |= matches!(name, "collider" | "removeBody" | "removeCollider");
        match name {
            "gravity" => self.world.gravity = a.vec3(0)?,

            "timestep" => self.world.integration_parameters.dt = a.num(0)?.max(1e-4),
            "body" => self.add_body(&a)?,
            "collider" => self.add_collider(&a)?,
            "removeBody" => {
                let id = a.id(0)?;
                self.characters.remove(&id);
                self.pins.remove(&id);
                if let Some(h) = self.bodies.remove(&id)
                    && let Some(body) = self.world.remove_body(h)
                {
                    // Its colliders went with it.
                    self.colliders.retain(|_, c| !body.colliders().contains(c));
                }
            }
            "removeCollider" => {
                let id = a.id(0)?;
                self.ignored.retain(|&(x, y)| x != id && y != id);
                if let Some(h) = self.colliders.remove(&id) {
                    self.world.remove_collider(h);
                }
            }
            "ignore" => {
                let (x, y) = (a.id(0)?, a.id(1)?);
                let pair = (x.min(y), x.max(y));
                if matches!(args.get(2), Some(Value::Bool(false))) {
                    self.ignored.remove(&pair);
                } else {
                    self.ignored.insert(pair);
                    self.flag_hooks(x);
                    self.flag_hooks(y);
                }
            }
            "set" => self.set(&a)?,
            "colliderSet" => self.collider_set(&a)?,
            "joint" => self.add_joint(&a)?,
            "removeJoint" => {
                if let Some(h) = self.joints.remove(&a.id(0)?) {
                    // Gone already if one of its bodies was removed.
                    self.world.remove_impulse_joint(h);
                }
            }
            "force" => {
                let (body, v) = (self.body_mut(a.id(0)?)?, a.vec3(2)?);
                match a.str(1)? {
                    "force" => body.add_force(v, true),
                    "impulse" => body.apply_impulse(v, true),
                    "torque" => body.add_torque(v, true),
                    "torqueImpulse" => body.apply_torque_impulse(v, true),
                    // `force id impulseAt [impulse] [world point]`
                    "impulseAt" => body.apply_impulse_at_point(v, a.vec3(3)?, true),
                    other => return Err(format!("unknown force kind `{other}`")),
                }
            }
            "clearForces" => {
                let body = self.body_mut(a.id(0)?)?;
                body.reset_forces(true);
                body.reset_torques(true);
            }
            "character" => {
                let id = a.id(0)?;
                if !self.bodies.contains_key(&id) {
                    return Err(format!("no body {id}"));
                }
                let controller = KinematicCharacterController {
                    max_slope_climb_angle: a.opt_num(1).unwrap_or(45.0).to_radians(),
                    snap_to_ground: a.opt_num(2).map(CharacterLength::Absolute),
                    autostep: a.opt_num(3).map(|h| CharacterAutostep {
                        max_height: CharacterLength::Absolute(h),
                        min_width: CharacterLength::Absolute(0.2),
                        include_dynamic_bodies: false,
                    }),
                    ..Default::default()
                };
                self.characters.insert(id, (controller, None));
            }
            "move" => {
                let id = a.id(0)?;
                let delta = a.vec3(1)?;
                let Some((_, pending)) = self.characters.get_mut(&id) else {
                    return Err(format!("body {id} has no character controller"));
                };
                *pending = Some(pending.unwrap_or(Vector::ZERO) + delta);
            }
            "raycast" => {
                // `raycast query [origin] [direction] maxDistance`
                let dir = a.vec3(2)?;
                if dir.length_squared() == 0.0 {
                    return Err("raycast needs a direction".into());
                }
                let ray = Ray::new(a.vec3(1)?, dir.normalize());
                self.rays.push((a.id(0)?, ray, a.num(3)?.max(0.0)));
            }
            other => return Err(format!("unknown physics op `{other}`")),
        }
        Ok(())
    }

    /// `body id kind [px py pz] [qx qy qz qw] [vx vy vz] options`: options is
    /// `[gravityScale, linearDamping, angularDamping, ccd, lockRotations]`.
    fn add_body(&mut self, a: &Args) -> Result<(), String> {
        let id = a.id(0)?;
        let builder = match a.str(1)? {
            "dynamic" => RigidBodyBuilder::dynamic(),
            "fixed" => RigidBodyBuilder::fixed(),
            "kinematic" => RigidBodyBuilder::kinematic_position_based(),
            other => return Err(format!("unknown body kind `{other}`")),
        };
        let opts = a.list(5).unwrap_or(&[]);
        let opt = |i: usize| opts.get(i).and_then(Value::as_f64).map(|v| v as f32);
        let flag = |i: usize| matches!(opts.get(i), Some(Value::Bool(true)));
        let mut builder = builder
            .translation(a.vec3(2).unwrap_or(Vector::ZERO))
            .linvel(a.vec3(4).unwrap_or(Vector::ZERO))
            .gravity_scale(opt(0).unwrap_or(1.0))
            .linear_damping(opt(1).unwrap_or(0.0))
            .angular_damping(opt(2).unwrap_or(0.0))
            .ccd_enabled(flag(3));
        if flag(4) {
            builder = builder.lock_rotations();
        }
        let mut body = builder.build();
        if let Ok(q) = a.quat(3) {
            body.set_rotation(q, false);
        }
        if let Some(old) = self.bodies.remove(&id) {
            self.world.remove_body(old);
        }
        let handle = self.world.insert_body(body);
        self.bodies.insert(id, handle);
        Ok(())
    }

    /// `collider id body|null shape [dims] [px py pz] [qx qy qz qw] options`: options is
    /// `[friction, restitution, density, sensor, events]`. Shapes: `box` (half extents),
    /// `ball` (radius), `capsule` (half height, radius; along y), `cylinder` (half height,
    /// radius), `convex` (points, flat), `mesh` (points flat, then `indices` in the
    /// options' slot 5).
    fn add_collider(&mut self, a: &Args) -> Result<(), String> {
        let id = a.id(0)?;
        let parent = match a.0.get(1) {
            Some(Value::Null) | None => None,
            _ => Some(
                *self
                    .bodies
                    .get(&a.id(1)?)
                    .ok_or_else(|| format!("collider {id}: no body {}", a.id(1).unwrap_or(0)))?,
            ),
        };
        let dims: Vec<f32> = a
            .list(3)?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();
        let dim = |i: usize| dims.get(i).copied().unwrap_or(0.5).max(0.0);
        let points = || -> Vec<Vector> {
            dims.as_chunks::<3>()
                .0
                .iter()
                .map(|&p| Vector::from_array(p))
                .collect()
        };
        let opts = a.list(6).unwrap_or(&[]);
        let builder = match a.str(2)? {
            "box" => ColliderBuilder::cuboid(dim(0), dim(1), dim(2)),
            "ball" => ColliderBuilder::ball(dim(0)),
            "capsule" => ColliderBuilder::capsule_y(dim(0), dim(1)),
            "cylinder" => ColliderBuilder::cylinder(dim(0), dim(1)),
            "convex" => ColliderBuilder::convex_hull(&points())
                .ok_or_else(|| format!("collider {id}: the points have no convex hull"))?,
            "mesh" => {
                let indices: Vec<[u32; 3]> = opts
                    .get(5)
                    .and_then(|v| match v {
                        Value::Array(items) => Some(items),
                        _ => None,
                    })
                    .map(|items| {
                        let index = |v: &Value| v.as_f64().unwrap_or(0.0) as u32;
                        items
                            .as_chunks::<3>()
                            .0
                            .iter()
                            .map(|t| t.each_ref().map(index))
                            .collect()
                    })
                    .unwrap_or_default();
                let count = dims.len() as u32 / 3;
                if indices.is_empty() || indices.iter().flatten().any(|&i| i >= count) {
                    return Err(format!(
                        "collider {id}: mesh indices are missing or out of range"
                    ));
                }
                ColliderBuilder::trimesh(points(), indices)
                    .map_err(|e| format!("collider {id}: {e}"))?
            }
            other => return Err(format!("unknown collider shape `{other}`")),
        };
        let opt = |i: usize| opts.get(i).and_then(Value::as_f64).map(|v| v as f32);
        let flag = |i: usize| matches!(opts.get(i), Some(Value::Bool(true)));
        let mut builder = builder
            .position(Pose::from_parts(
                a.vec3(4).unwrap_or(Vector::ZERO),
                a.quat(5).unwrap_or(Rotation::IDENTITY),
            ))
            .friction(opt(0).unwrap_or(0.5))
            .restitution(opt(1).unwrap_or(0.0))
            .density(opt(2).unwrap_or(1.0))
            .sensor(flag(3))
            .user_data(id as u128);
        if flag(4) {
            builder = builder.active_events(ActiveEvents::COLLISION_EVENTS);
        }
        let collider = builder.build();
        if let Some(old) = self.colliders.remove(&id) {
            self.world.remove_collider(old);
        }
        let handle = self.world.insert_collider(collider, parent);
        if self.ignored.iter().any(|&(x, y)| x == id || y == id) {
            self.flag_hooks_of(handle);
        }
        self.colliders.insert(id, handle);
        Ok(())
    }

    /// `set id property values`: `position` [x y z], `rotation` [x y z w], `velocity`,
    /// `angularVelocity`, `target` (a kinematic body's next position), `targetRotation`,
    /// `enabled` (bool), `kind` (`dynamic` / `fixed` / `kinematic`), `mass` (kg, shared
    /// by its colliders), `gravityScale`, `linearDamping`, `angularDamping`, `ccd`
    /// (bool), `constraints` ([x y z] translations then [x y z] rotations allowed, 6
    /// bools), `sleep`, `wake`.
    fn set(&mut self, a: &Args) -> Result<(), String> {
        let id = a.id(0)?;
        let flag = |i: usize| matches!(a.0.get(i), Some(Value::Bool(true)));
        match a.str(1)? {
            "mass" => return self.set_mass(id, a.num(2)?.max(0.0), a.vec3(3).ok()),
            "constraints" => return self.set_constraints(id, a.list(2)?),
            _ => {}
        }
        if a.str(1)? == "position"
            && let Some(pin) = self.pins.get_mut(&id)
        {
            // Moved on purpose: held where it was put.
            let to = a.vec3(2)?.to_array();
            for axis in 0..3 {
                if pin[axis].is_some() {
                    pin[axis] = Some(to[axis]);
                }
            }
        }
        let body = self.body_mut(id)?;
        match a.str(1)? {
            "position" => body.set_translation(a.vec3(2)?, true),
            "rotation" => body.set_rotation(a.quat(2)?, true),
            "velocity" => body.set_linvel(a.vec3(2)?, true),
            "angularVelocity" => body.set_angvel(a.vec3(2)?, true),
            "target" => body.set_next_kinematic_translation(a.vec3(2)?),
            "targetRotation" => body.set_next_kinematic_rotation(a.quat(2)?),
            "enabled" => body.set_enabled(flag(2)),
            "kind" => body.set_body_type(
                match a.str(2)? {
                    "dynamic" => RigidBodyType::Dynamic,
                    "fixed" => RigidBodyType::Fixed,
                    "kinematic" => RigidBodyType::KinematicPositionBased,
                    other => return Err(format!("unknown body kind `{other}`")),
                },
                true,
            ),
            "gravityScale" => {
                body.set_gravity_scale(a.num(2)?, true);
                // A body that fell asleep floating (scale 0) must wake when gravity returns;
                // rapier's own wake isn't enough.
                body.wake_up(true);
            }
            "linearDamping" => body.set_linear_damping(a.num(2)?.max(0.0)),
            "angularDamping" => body.set_angular_damping(a.num(2)?.max(0.0)),
            "ccd" => body.enable_ccd(flag(2)),
            "sleep" => body.sleep(),
            "wake" => body.wake_up(true),
            other => return Err(format!("unknown body property `{other}`")),
        }
        Ok(())
    }

    /// Gives a body `mass` kg, split evenly over its colliders (their shapes still set
    /// how it turns); a body without colliders gets it as extra mass.
    ///
    /// A body without colliders has no shape to turn, so `size` (the box it stands for, from its
    /// object's scale) gives it the inertia of a box of that much mass: without one it could
    /// move but never rotate, and a hinged chain or door would stay put when pushed.
    fn set_mass(&mut self, id: ScriptId, mass: f32, size: Option<Vector>) -> Result<(), String> {
        let handle = *self
            .bodies
            .get(&id)
            .ok_or_else(|| format!("no body {id}"))?;
        let colliders: Vec<ColliderHandle> = self.world.bodies[handle].colliders().to_vec();
        if colliders.is_empty() {
            let body = self.world.bodies.get_mut(handle).ok_or("no body")?;
            match size {
                Some(s) if mass > 0.0 => {
                    let (x, y, z) = (
                        s.x.abs().max(0.05),
                        s.y.abs().max(0.05),
                        s.z.abs().max(0.05),
                    );
                    let inertia = Vector::new(
                        mass * (y * y + z * z) / 12.0,
                        mass * (x * x + z * z) / 12.0,
                        mass * (x * x + y * y) / 12.0,
                    );
                    body.set_additional_mass_properties(
                        MassProperties::new(Vector::ZERO, mass, inertia),
                        true,
                    );
                }
                _ => body.set_additional_mass(mass, true),
            }
            // Rapier works the body's mass out on the next step; an impulse before it (a script's
            // `Awake`) would meet zero mass and do nothing.
            body.recompute_mass_properties_from_colliders(&self.world.colliders);
            return Ok(());
        }
        let share = mass / colliders.len() as f32;
        for c in colliders {
            if let Some(collider) = self.world.colliders.get_mut(c) {
                collider.set_mass(share);
            }
        }
        self.world.bodies[handle].wake_up(true);
        Ok(())
    }

    /// `joint id kind bodyA bodyB|null [anchorA] [anchorB] [params]`: joins two bodies at
    /// anchors in their own spaces (`null`: to the world, anchor B in world space).
    /// Kinds and params: `fixed`; `ball` (turns freely about the anchor); `hinge` [axis x
    /// y z, use limits (0/1), min, max (degrees), use motor (0/1), target speed (degrees
    /// per second), motor strength]; `spring` [rest length, stiffness, damping]; `rope`
    /// [max length]. Joined bodies don't collide with each other.
    fn add_joint(&mut self, a: &Args) -> Result<(), String> {
        let id = a.id(0)?;
        let body1 = *self
            .bodies
            .get(&a.id(2)?)
            .ok_or_else(|| format!("joint {id}: no body {}", a.id(2).unwrap_or(0)))?;
        let body2 = match a.0.get(3) {
            Some(Value::Null) | None => self.world_anchor(),
            _ => *self
                .bodies
                .get(&a.id(3)?)
                .ok_or_else(|| format!("joint {id}: no body {}", a.id(3).unwrap_or(0)))?,
        };
        let anchor1 = a.vec3(4).unwrap_or(Vector::ZERO);
        let anchor2 = a.vec3(5).unwrap_or(Vector::ZERO);
        let p = a.list(6).unwrap_or(&[]);
        let n = |i: usize, d: f32| p.get(i).and_then(Value::as_f64).map_or(d, |v| v as f32);
        let joint: GenericJoint = match a.str(1)? {
            "fixed" => FixedJointBuilder::new()
                .local_anchor1(anchor1)
                .local_anchor2(anchor2)
                .contacts_enabled(false)
                .into(),
            "ball" => SphericalJointBuilder::new()
                .local_anchor1(anchor1)
                .local_anchor2(anchor2)
                .contacts_enabled(false)
                .into(),
            "hinge" => {
                let axis = Vector::new(n(0, 0.0), n(1, 1.0), n(2, 0.0)).normalize_or(Vector::Y);
                let mut b = RevoluteJointBuilder::new(axis)
                    .local_anchor1(anchor1)
                    .local_anchor2(anchor2)
                    .contacts_enabled(false);
                if n(3, 0.0) > 0.5 {
                    b = b.limits([n(4, -90.0).to_radians(), n(5, 90.0).to_radians()]);
                }
                if n(6, 0.0) > 0.5 {
                    b = b.motor_velocity(n(7, 0.0).to_radians(), n(8, 1.0));
                }
                b.into()
            }
            "spring" => SpringJointBuilder::new(n(0, 1.0).max(0.0), n(1, 10.0), n(2, 0.5))
                .local_anchor1(anchor1)
                .local_anchor2(anchor2)
                .contacts_enabled(false)
                .into(),
            "rope" => RopeJointBuilder::new(n(0, 1.0).max(0.0))
                .local_anchor1(anchor1)
                .local_anchor2(anchor2)
                .contacts_enabled(false)
                .into(),
            other => return Err(format!("unknown joint kind `{other}`")),
        };
        if let Some(old) = self.joints.remove(&id) {
            self.world.remove_impulse_joint(old);
        }
        let handle = self.world.insert_impulse_joint(body1, body2, joint);
        self.joints.insert(id, handle);
        Ok(())
    }

    /// The fixed body joints to the world hang from.
    fn world_anchor(&mut self) -> RigidBodyHandle {
        if let Some(h) = self.world_anchor {
            return h;
        }
        let h = self.world.insert_body(RigidBodyBuilder::fixed());
        self.world_anchor = Some(h);
        h
    }

    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    /// `colliderSet id property value`: `friction`, `restitution`, `sensor` (bool),
    /// `offset` ([x y z] [x y z w] from its body).
    fn collider_set(&mut self, a: &Args) -> Result<(), String> {
        let id = a.id(0)?;
        let collider = self
            .colliders
            .get(&id)
            .and_then(|h| self.world.colliders.get_mut(*h))
            .ok_or_else(|| format!("no collider {id}"))?;
        match a.str(1)? {
            "friction" => collider.set_friction(a.num(2)?.max(0.0)),
            "restitution" => collider.set_restitution(a.num(2)?.clamp(0.0, 1.0)),
            "sensor" => collider.set_sensor(matches!(a.0.get(2), Some(Value::Bool(true)))),
            "offset" => collider.set_position_wrt_parent(Pose::from_parts(
                a.vec3(2)?,
                a.quat(3).unwrap_or(Rotation::IDENTITY),
            )),
            other => return Err(format!("unknown collider property `{other}`")),
        }
        Ok(())
    }

    /// Moves a body to where a script put its GameObject: a kinematic body travels there
    /// over the next step (pushing things on the way); others teleport (keeping their
    /// velocity, like setting a Unity transform).
    /// The world's gravity (cloth uses it too).
    pub fn gravity(&self) -> [f32; 3] {
        let g = self.world.gravity;
        [g.x, g.y, g.z]
    }

    /// Arrays rather than vectors: callers may use another glam version than rapier's.
    pub fn set_pose(&mut self, id: ScriptId, position: [f32; 3], rotation: [f32; 4]) {
        let (position, rotation) = (Vector::from_array(position), Rotation::from_array(rotation));
        let Ok(body) = self.body_mut(id) else { return };
        if body.is_kinematic() {
            body.set_next_kinematic_translation(position);
            body.set_next_kinematic_rotation(rotation);
        } else {
            body.set_translation(position, true);
            body.set_rotation(rotation, true);
        }
    }

    /// A body's position and rotation, if it exists.
    pub fn pose(&self, id: ScriptId) -> Option<([f32; 3], [f32; 4])> {
        let body = self.world.bodies.get(*self.bodies.get(&id)?)?;
        Some((body.translation().to_array(), body.rotation().to_array()))
    }

    fn body_mut(&mut self, id: ScriptId) -> Result<&mut RigidBody, String> {
        self.bodies
            .get(&id)
            .and_then(|h| self.world.bodies.get_mut(*h))
            .ok_or_else(|| format!("no body {id}"))
    }

    /// `constraints`: which axes a body may move along and turn about (`[x y z]`
    /// translations, then rotations; missing entries are allowed).
    ///
    /// A movement axis locked together with both turns across it (the usual way to keep a
    /// body in a plane) isn't given to rapier as a lock: friction along that axis would have
    /// no mass, and rapier 0.36's 3D friction solve then drops friction along the other
    /// direction too, so the body slides as if on ice. Platypus holds that axis in place itself
    /// instead, after every step ([`Self::hold_pins`]).
    fn set_constraints(&mut self, id: ScriptId, c: &[Value]) -> Result<(), String> {
        let allow = |i: usize| !matches!(c.get(i), Some(Value::Bool(false)));
        let moves = [allow(0), allow(1), allow(2)];
        let turns = [allow(3), allow(4), allow(5)];
        // Locked movement with both turns across it locked: pinned by Platypus.
        let pinned: [bool; 3] =
            std::array::from_fn(|i| !moves[i] && !turns[(i + 1) % 3] && !turns[(i + 2) % 3]);
        let body = self.body_mut(id)?;
        let at = body.translation().to_array();
        body.set_enabled_translations(
            moves[0] || pinned[0],
            moves[1] || pinned[1],
            moves[2] || pinned[2],
            true,
        );
        body.set_enabled_rotations(turns[0], turns[1], turns[2], true);
        if pinned.contains(&true) {
            self.pins
                .insert(id, std::array::from_fn(|i| pinned[i].then_some(at[i])));
        } else {
            self.pins.remove(&id);
        }
        Ok(())
    }

    /// Puts pinned axes back where they're held, with no speed along them.
    fn hold_pins(&mut self) {
        for (id, pin) in &self.pins {
            let Some(body) = self
                .bodies
                .get(id)
                .and_then(|h| self.world.bodies.get_mut(*h))
            else {
                continue;
            };
            let mut p = body.translation().to_array();
            let mut v = body.linvel().to_array();
            let mut moved = false;
            for axis in 0..3 {
                if let Some(held) = pin[axis] {
                    moved |= p[axis] != held || v[axis] != 0.0;
                    p[axis] = held;
                    v[axis] = 0.0;
                }
            }
            // Untouched bodies stay asleep.
            if moved {
                body.set_translation(Vector::from_array(p), false);
                body.set_linvel(Vector::from_array(v), false);
            }
        }
    }

    /// Makes rapier ask the ignore hooks about a collider's pairs.
    fn flag_hooks(&mut self, id: ScriptId) {
        if let Some(&h) = self.colliders.get(&id) {
            self.flag_hooks_of(h);
        }
    }

    fn flag_hooks_of(&mut self, h: ColliderHandle) {
        if let Some(c) = self.world.colliders.get_mut(h) {
            c.set_active_hooks(
                ActiveHooks::FILTER_CONTACT_PAIRS | ActiveHooks::FILTER_INTERSECTION_PAIR,
            );
        }
    }

    /// Advances the world by one timestep (`physics.timestep`, 1/60 s by default):
    /// moves character controllers, steps, then answers raycasts and records what moved.
    pub fn step(&mut self) {
        self.move_characters();
        self.hold_pins();
        let events = Events::default();
        self.world
            .step_with_events(&IgnoreHooks(&self.ignored), &events);
        self.hold_pins();

        let script_id = |h: ColliderHandle| self.world.colliders.get(h).map(|c| c.user_data as u32);
        for (event, relative) in events.0.into_inner().unwrap_or_default() {
            let (Some(a), Some(b)) = (script_id(event.collider1()), script_id(event.collider2()))
            else {
                continue;
            };
            self.batch.contacts.push(Contact {
                started: event.started(),
                a,
                b,
                sensor: event.sensor(),
                speed: relative.length(),
                velocity: relative.to_array(),
            });
        }

        let ids: HashMap<RigidBodyHandle, ScriptId> =
            self.bodies.iter().map(|(&id, &h)| (h, id)).collect();
        for (handle, body) in self.world.active_bodies() {
            if !body.is_dynamic() && !body.is_kinematic() {
                continue;
            }
            let Some(&id) = ids.get(&handle) else {
                continue;
            };
            self.batch.bodies.push(BodyState {
                id,
                position: body.translation().to_array(),
                rotation: body.rotation().to_array(),
                velocity: body.linvel().to_array(),
                angular_velocity: body.angvel().to_array(),
            });
        }

        for (query, ray, max) in std::mem::take(&mut self.rays) {
            let hit = self
                .world
                .cast_ray_and_get_normal(&ray, max, true, QueryFilter::default())
                .and_then(|(h, hit)| {
                    Some((script_id(h)?, hit.time_of_impact, hit.normal.to_array()))
                });
            self.batch.rays.push(RayHit { query, hit });
        }
    }

    /// Applies each character controller's pending move as its body's next kinematic
    /// position, sliding along and stepping over what it hits.
    fn move_characters(&mut self) {
        // New colliders aren't in the query structures until this step runs: their moves
        // stay pending until the next one. (Running collision detection here instead
        // trips rapier's island bookkeeping for bodies that just changed type.)
        if std::mem::take(&mut self.dirty) {
            return;
        }
        let dt = self.world.integration_parameters.dt;
        let mut moves = Vec::new();
        for (&id, (controller, pending)) in &mut self.characters {
            let Some(delta) = pending.take() else {
                continue;
            };
            let Some(body) = self.bodies.get(&id).and_then(|h| self.world.bodies.get(*h)) else {
                continue;
            };
            // The body's first collider is the character's shape.
            let Some(collider) = body
                .colliders()
                .first()
                .and_then(|c| self.world.colliders.get(*c))
            else {
                continue;
            };
            let handle = self.bodies[&id];
            let filter = QueryFilter::default()
                .exclude_rigid_body(handle)
                .exclude_sensors();
            let queries = self.world.query_pipeline_with_filter(filter);
            let movement = controller.move_shape(
                dt,
                &queries,
                collider.shape(),
                collider.position(),
                delta,
                |_| {},
            );
            moves.push((
                id,
                handle,
                body.translation() + movement.translation,
                movement.grounded,
            ));
        }
        for (id, handle, target, grounded) in moves {
            if let Some(body) = self.world.bodies.get_mut(handle) {
                if body.is_kinematic() {
                    body.set_next_kinematic_translation(target);
                } else {
                    body.set_translation(target, true);
                }
            }
            self.batch.characters.push((id, grounded));
        }
    }

    /// Takes what scripts need from the ticks since the last call.
    pub fn take_batch(&mut self) -> Batch {
        std::mem::take(&mut self.batch)
    }
}

/// Reads op arguments with errors that say what was wrong.
struct Args<'a>(&'a [Value]);

impl Args<'_> {
    fn num(&self, i: usize) -> Result<f32, String> {
        self.0
            .get(i)
            .and_then(Value::as_f64)
            .map(|v| v as f32)
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("argument {i} must be a number"))
    }

    fn opt_num(&self, i: usize) -> Option<f32> {
        self.num(i).ok()
    }

    /// Ids go through `f64`: `f32` can't hold integers past 2^24 (script component
    /// and engine object ids are bigger).
    fn id(&self, i: usize) -> Result<ScriptId, String> {
        self.0
            .get(i)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0 && *v <= u32::MAX as f64)
            .map(|v| v as ScriptId)
            .ok_or_else(|| format!("argument {i} must be an id"))
    }

    fn str(&self, i: usize) -> Result<&str, String> {
        self.0
            .get(i)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("argument {i} must be a string"))
    }

    fn list(&self, i: usize) -> Result<&[Value], String> {
        match self.0.get(i) {
            Some(Value::Array(items)) => Ok(items),
            _ => Err(format!("argument {i} must be an array")),
        }
    }

    /// `N` numbers: an array, or the arguments from `i` on.
    fn floats<const N: usize>(&self, i: usize) -> Result<[f32; N], String> {
        let from: &[Value] = match self.0.get(i) {
            Some(Value::Array(items)) => items,
            _ => self.0.get(i..).unwrap_or(&[]),
        };
        let mut out = [0.0; N];
        for (k, slot) in out.iter_mut().enumerate() {
            *slot = from
                .get(k)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("argument {i} must hold {N} numbers"))?;
        }
        Ok(out)
    }

    fn vec3(&self, i: usize) -> Result<Vector, String> {
        self.floats::<3>(i).map(Vector::from_array)
    }

    fn quat(&self, i: usize) -> Result<Rotation, String> {
        let [x, y, z, w] = self.floats::<4>(i)?;
        let q = Rotation::from_xyzw(x, y, z, w);
        if q.length_squared() < 1e-12 {
            return Err(format!("argument {i} is not a rotation"));
        }
        Ok(q.normalize())
    }
}
