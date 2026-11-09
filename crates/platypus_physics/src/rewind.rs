// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Rewindable physics: a [`Snapshot`] is a copy of everything that makes the world what it is
//! (every body with its pose, velocity and sleep state, the colliders, joints, contact cache and
//! broad phase), and [`Physics::restore`] puts the world back to it. Stepping on from a restored
//! world gives, step for step, what stepping on from the moment of the snapshot gave (a test
//! checks it), so a game can rewind time as a mechanic, rewind to check a shot against where
//! things were (lag compensation of physics props), or re-run a few steps with a different input
//! (client-side prediction of bodies).
//!
//! A snapshot is a copy of memory, not a file: it is quick to make (a clone of the world's
//! arrays) and not meant to be saved. It holds the world as the scripts' ids know it: bodies,
//! colliders and joints made after the snapshot are gone after a restore, those removed since
//! are back. Parameters (gravity, the timestep) are not part of it: a rewind does not change them.

use super::*;

/// The world at one moment. Make one with [`Physics::snapshot`].
#[derive(Clone)]
pub struct Snapshot {
    islands: IslandManager,
    broad_phase: BroadPhaseBvh,
    narrow_phase: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    soft_bodies: SoftBodySet,
    body_ids: HashMap<ScriptId, RigidBodyHandle>,
    collider_ids: HashMap<ScriptId, ColliderHandle>,
    joint_ids: HashMap<ScriptId, ImpulseJointHandle>,
    pins: HashMap<ScriptId, [Option<f32>; 3]>,
    world_anchor: Option<RigidBodyHandle>,
    ignored: IgnoredPairs,
    dirty: bool,
}

impl Snapshot {
    /// How many bodies the world had.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }
}

impl Physics {
    /// A copy of the world as it is now.
    pub fn snapshot(&self) -> Snapshot {
        let w = &self.world;
        Snapshot {
            islands: w.islands.clone(),
            broad_phase: w.broad_phase.clone(),
            narrow_phase: w.narrow_phase.clone(),
            bodies: w.bodies.clone(),
            colliders: w.colliders.clone(),
            impulse_joints: w.impulse_joints.clone(),
            multibody_joints: w.multibody_joints.clone(),
            soft_bodies: w.soft_bodies.clone(),
            body_ids: self.bodies.clone(),
            collider_ids: self.colliders.clone(),
            joint_ids: self.joints.clone(),
            pins: self.pins.clone(),
            world_anchor: self.world_anchor,
            ignored: self.ignored.clone(),
            dirty: self.dirty,
        }
    }

    /// Puts the world back as it was in `s`. What scripts see is told as a batch: every moving
    /// body's pose and speed, so the objects attached to bodies follow. Pending character moves,
    /// raycasts and contact events of the world being left are dropped.
    pub fn restore(&mut self, s: &Snapshot) {
        let w = &mut self.world;
        w.islands = s.islands.clone();
        w.broad_phase = s.broad_phase.clone();
        w.narrow_phase = s.narrow_phase.clone();
        w.bodies = s.bodies.clone();
        w.colliders = s.colliders.clone();
        w.impulse_joints = s.impulse_joints.clone();
        w.multibody_joints = s.multibody_joints.clone();
        w.soft_bodies = s.soft_bodies.clone();
        self.bodies = s.body_ids.clone();
        self.colliders = s.collider_ids.clone();
        self.joints = s.joint_ids.clone();
        self.pins = s.pins.clone();
        self.world_anchor = s.world_anchor;
        self.ignored = s.ignored.clone();
        self.dirty = s.dirty;
        // Characters keep their controller settings (they are not world state); a move asked for
        // in the world being left is not asked in this one.
        self.characters.retain(|id, _| self.bodies.contains_key(id));
        for (_, pending) in self.characters.values_mut() {
            *pending = None;
        }
        self.rays.clear();
        self.batch = Batch::default();
        let ids: HashMap<RigidBodyHandle, ScriptId> =
            self.bodies.iter().map(|(&id, &h)| (h, id)).collect();
        for (handle, body) in self.world.bodies.iter() {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(n: f64) -> Value {
        Value::Number(n)
    }

    fn arr(v: &[f64]) -> Value {
        Value::Array(v.iter().copied().map(num).collect())
    }

    fn op(p: &mut Physics, name: &str, args: Vec<Value>) {
        p.apply(name, &args)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    fn add_box(p: &mut Physics, id: f64, kind: &str, at: [f64; 3], half: [f64; 3]) {
        op(p, "body", vec![num(id), kind.into(), arr(&at)]);
        op(
            p,
            "collider",
            vec![num(id), num(id), "box".into(), arr(&half)],
        );
    }

    /// A floor and a leaning stack of boxes that is then knocked: enough contacts to matter.
    fn world() -> Physics {
        let mut p = Physics::new();
        add_box(&mut p, 1.0, "fixed", [0.0, -0.5, 0.0], [20.0, 0.5, 20.0]);
        for i in 0..6u32 {
            let y = 0.5 + 1.01 * f64::from(i);
            add_box(
                &mut p,
                f64::from(10 + i),
                "dynamic",
                [0.12 * f64::from(i), y, 0.0],
                [0.5, 0.5, 0.5],
            );
        }
        op(
            &mut p,
            "force",
            vec![num(15.0), "impulse".into(), arr(&[3.0, 0.0, 0.0])],
        );
        p
    }

    fn poses(p: &Physics) -> Vec<(ScriptId, [f32; 3], [f32; 4])> {
        let mut v: Vec<_> = (10..16)
            .filter_map(|id| p.pose(id).map(|(a, b)| (id, a, b)))
            .collect();
        v.sort_by_key(|e| e.0);
        v
    }

    // M8-PHY-01
    #[test]
    fn restoring_a_snapshot_and_stepping_on_repeats_the_same_steps_exactly() {
        let mut p = world();
        for _ in 0..30 {
            p.step();
        }
        let snap = p.snapshot();
        let at_snapshot = poses(&p);
        for _ in 0..120 {
            p.step();
        }
        let later = poses(&p);
        assert_ne!(at_snapshot, later, "the stack should have moved");

        p.restore(&snap);
        assert_eq!(poses(&p), at_snapshot, "restoring puts every body back");
        for _ in 0..120 {
            p.step();
        }
        // Bit for bit: a rewind is not an approximation.
        assert_eq!(poses(&p), later);
    }

    // M8-PHY-02
    #[test]
    fn a_restore_tells_scripts_where_every_moving_body_is_and_forgets_later_bodies() {
        let mut p = world();
        for _ in 0..10 {
            p.step();
        }
        let snap = p.snapshot();
        assert_eq!(snap.body_count(), 7);
        // A body made after the snapshot, and the world moving on.
        add_box(&mut p, 99.0, "dynamic", [5.0, 5.0, 5.0], [0.5, 0.5, 0.5]);
        for _ in 0..20 {
            p.step();
        }
        let _ = p.take_batch();
        p.restore(&snap);
        assert!(
            p.pose(99).is_none(),
            "a body made after the snapshot is gone"
        );
        assert_eq!(p.body_count(), 7);
        let batch = p.take_batch();
        let mut told: Vec<ScriptId> = batch.bodies.iter().map(|b| b.id).collect();
        told.sort_unstable();
        assert_eq!(told, (10..16).collect::<Vec<_>>());
        for b in &batch.bodies {
            let (pos, rot) = p.pose(b.id).unwrap();
            assert_eq!((b.position, b.rotation), (pos, rot));
        }
        // A snapshot can be restored again and again.
        for _ in 0..5 {
            p.step();
        }
        p.restore(&snap);
        assert_eq!(p.body_count(), 7);
    }
}
