// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Steering behaviours. Each returns an acceleration (a force on a unit mass), clamped to
//! the agent's `max_force`; sum them and call [`Agent::integrate`].

use glam::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Agent {
    pub position: Vec3,
    pub velocity: Vec3,
    pub max_speed: f32,
    pub max_force: f32,
}

impl Agent {
    pub fn new(position: Vec3, max_speed: f32, max_force: f32) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            max_speed,
            max_force,
        }
    }

    /// Steers toward a desired velocity.
    fn toward(&self, desired: Vec3) -> Vec3 {
        (desired - self.velocity).clamp_length_max(self.max_force)
    }

    /// Runs flat out at `target`.
    pub fn seek(&self, target: Vec3) -> Vec3 {
        self.toward((target - self.position).normalize_or_zero() * self.max_speed)
    }

    /// Runs flat out away from `threat`.
    pub fn flee(&self, threat: Vec3) -> Vec3 {
        self.toward((self.position - threat).normalize_or_zero() * self.max_speed)
    }

    /// Seeks `target` and slows to a stop within `slow_radius` of it.
    pub fn arrive(&self, target: Vec3, slow_radius: f32) -> Vec3 {
        let to = target - self.position;
        let dist = to.length();
        if dist < 1e-6 {
            return self.toward(Vec3::ZERO);
        }
        let speed = if dist < slow_radius {
            self.max_speed * dist / slow_radius.max(1e-6)
        } else {
            self.max_speed
        };
        self.toward(to / dist * speed)
    }

    /// Seeks where a moving target will be, looking ahead by the time to reach it.
    pub fn pursue(&self, target_pos: Vec3, target_vel: Vec3) -> Vec3 {
        let t = (target_pos - self.position).length() / self.max_speed.max(1e-6);
        self.seek(target_pos + target_vel * t)
    }

    /// Flees from where a moving threat will be.
    pub fn evade(&self, threat_pos: Vec3, threat_vel: Vec3) -> Vec3 {
        let t = (threat_pos - self.position).length() / self.max_speed.max(1e-6);
        self.flee(threat_pos + threat_vel * t)
    }

    /// Pushes away from `neighbours` closer than `radius`, harder the closer they are.
    pub fn separation(&self, neighbours: &[Vec3], radius: f32) -> Vec3 {
        let mut push = Vec3::ZERO;
        for &n in neighbours {
            let away = self.position - n;
            let d = away.length();
            if d > 1e-6 && d < radius {
                push += away / d * (1.0 - d / radius);
            }
        }
        self.toward(push.normalize_or_zero() * self.max_speed * push.length().min(1.0))
    }

    /// Wanders: a point on a sphere of `radius` ahead by `distance`, picked by the caller's
    /// `jitter` direction (keep it from frame to frame and nudge it for smooth wandering).
    pub fn wander(&self, distance: f32, radius: f32, jitter: Vec3) -> Vec3 {
        let ahead = self.velocity.normalize_or(Vec3::Z) * distance;
        self.seek(self.position + ahead + jitter.normalize_or_zero() * radius)
    }

    /// Adds `accel` over `dt`, caps the speed and moves.
    pub fn integrate(&mut self, accel: Vec3, dt: f32) {
        self.velocity = (self.velocity + accel * dt).clamp_length_max(self.max_speed);
        self.position += self.velocity * dt;
    }
}

/// Follows a list of waypoints, moving on when one is within `reach`.
#[derive(Clone, Debug)]
pub struct PathFollower {
    pub points: Vec<Vec3>,
    pub next: usize,
    pub reach: f32,
}

impl PathFollower {
    pub fn new(points: Vec<Vec3>, reach: f32) -> Self {
        Self {
            points,
            next: 0,
            reach,
        }
    }

    pub fn finished(&self) -> bool {
        self.next >= self.points.len()
    }

    /// Steering toward the current waypoint (arriving at the last), `None` once finished.
    pub fn steer(&mut self, agent: &Agent, slow_radius: f32) -> Option<Vec3> {
        while self.next < self.points.len()
            && (self.points[self.next] - agent.position).length() <= self.reach
        {
            self.next += 1;
        }
        let target = *self.points.get(self.next)?;
        Some(if self.next + 1 == self.points.len() {
            agent.arrive(target, slow_radius)
        } else {
            agent.seek(target)
        })
    }
}
