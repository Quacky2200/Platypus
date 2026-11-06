// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `ParticleSystem` components (Unity's main, emission, shape, over-lifetime and
//! texture-sheet modules), simulated here each frame and drawn as camera-facing quads.
//!
//! Scripts send settings as key/value pairs (`render.particles component object [key,
//! value, ...]`) and control a system with `render.particleControl component action
//! [count]`. Shapes emit along the object's up (+y), unlike Unity's +z cones, so a
//! fountain needs no rotation.

use platypus_core::Value;
use platypus_core::glam::{Affine3A, Mat4, Quat, Vec3};
use platypus_render::{WorldQuad, WorldTexture};

/// The soft round texture particles use when they're given none.
pub const DEFAULT_TEXTURE: u32 = platypus_render::DEFAULT_PARTICLE_TEXTURE;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Point,
    Sphere {
        radius: f32,
    },
    Hemisphere {
        radius: f32,
    },
    /// Spreads by `angle` degrees from straight up, from a disc of `radius`.
    Cone {
        angle: f32,
        radius: f32,
    },
    Box {
        size: Vec3,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    /// Seconds into each cycle.
    pub time: f32,
    pub count: u32,
    /// How many times it fires (0: every `interval` for the rest of the cycle).
    pub cycles: u32,
    pub interval: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub duration: f32,
    pub looping: bool,
    pub start_delay: f32,
    /// Ranges: a random value between the two, per particle.
    pub lifetime: [f32; 2],
    pub speed: [f32; 2],
    pub size: [f32; 2],
    /// Degrees.
    pub rotation: [f32; 2],
    /// Degrees per second.
    pub angular_velocity: [f32; 2],
    /// Linear RGBA; each particle picks a random mix of the two.
    pub color: [[f32; 4]; 2],
    /// Multiplies world gravity (9.81 m/s² down).
    pub gravity: f32,
    /// Fraction of speed lost per second.
    pub drag: f32,
    /// Particles stay where they were emitted when the object moves; otherwise they
    /// move with it.
    pub world_space: bool,
    pub max_particles: usize,
    pub rate: f32,
    pub bursts: Vec<Burst>,
    pub shape: Shape,
    /// (life 0..1, multiplier) keys, linear between them.
    pub size_over_life: Vec<[f32; 2]>,
    /// (life 0..1, r, g, b, a multipliers) keys.
    pub color_over_life: Vec<[f32; 5]>,
    pub texture: u32,
    pub additive: bool,
    /// Texture sheet: columns, rows, and how many times it plays over a lifetime.
    pub sheet: [f32; 3],
    pub seed: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            duration: 5.0,
            looping: true,
            start_delay: 0.0,
            lifetime: [5.0; 2],
            speed: [5.0; 2],
            size: [1.0; 2],
            rotation: [0.0; 2],
            angular_velocity: [0.0; 2],
            color: [[1.0; 4]; 2],
            gravity: 0.0,
            drag: 0.0,
            world_space: false,
            max_particles: 1000,
            rate: 10.0,
            bursts: Vec::new(),
            shape: Shape::Cone {
                angle: 25.0,
                radius: 1.0,
            },
            size_over_life: Vec::new(),
            color_over_life: Vec::new(),
            texture: DEFAULT_TEXTURE,
            additive: false,
            sheet: [1.0, 1.0, 1.0],
            seed: 1,
        }
    }
}

fn floats<const N: usize>(v: &Value) -> Option<[f32; N]> {
    let Value::Array(items) = v else {
        return None;
    };
    let mut out = [0.0; N];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = items.get(i)?.as_f64()? as f32;
    }
    Some(out)
}

/// A number, or a `[min, max]` range.
fn range(v: &Value) -> Option<[f32; 2]> {
    match v {
        Value::Number(n) => Some([*n as f32; 2]),
        _ => floats::<2>(v),
    }
}

/// Rows of `N` numbers from a flat array.
fn rows<const N: usize>(v: &Value) -> Option<Vec<[f32; N]>> {
    let Value::Array(items) = v else {
        return None;
    };
    let nums: Option<Vec<f32>> = items.iter().map(|x| x.as_f64().map(|n| n as f32)).collect();
    Some(nums?.as_chunks::<N>().0.to_vec())
}

impl Settings {
    /// Reads `[key, value, ...]` over the defaults.
    pub fn parse(pairs: &[Value]) -> Result<Self, String> {
        let mut s = Self::default();
        for pair in pairs.chunks(2) {
            let [Value::String(key), v] = pair else {
                return Err("particle settings must be [key, value, ...]".into());
            };
            let bad = || format!("particle setting `{key}`: bad value {v:?}");
            let num = || v.as_f64().map(|n| n as f32).ok_or_else(bad);
            match key.as_str() {
                "duration" => s.duration = num()?.max(0.01),
                "looping" => s.looping = matches!(v, Value::Bool(true)),
                "startDelay" => s.start_delay = num()?.max(0.0),
                "lifetime" => s.lifetime = range(v).ok_or_else(bad)?,
                "speed" => s.speed = range(v).ok_or_else(bad)?,
                "size" => s.size = range(v).ok_or_else(bad)?,
                "rotation" => s.rotation = range(v).ok_or_else(bad)?,
                "angularVelocity" => s.angular_velocity = range(v).ok_or_else(bad)?,
                "color" => {
                    let c = rows::<4>(v).ok_or_else(bad)?;
                    let a = *c.first().ok_or_else(bad)?;
                    s.color = [a, *c.get(1).unwrap_or(&a)];
                }
                "gravity" => s.gravity = num()?,
                "drag" => s.drag = num()?.max(0.0),
                "worldSpace" => s.world_space = matches!(v, Value::Bool(true)),
                "maxParticles" => s.max_particles = num()?.max(0.0) as usize,
                "rate" => s.rate = num()?.max(0.0),
                "bursts" => {
                    s.bursts = rows::<4>(v)
                        .ok_or_else(bad)?
                        .into_iter()
                        .map(|[time, count, cycles, interval]| Burst {
                            time,
                            count: count.max(0.0) as u32,
                            cycles: cycles.max(0.0) as u32,
                            interval: interval.max(0.01),
                        })
                        .collect();
                }
                "shape" => {
                    let Value::Array(items) = v else {
                        return Err(bad());
                    };
                    let n = |i: usize, d: f32| {
                        items.get(i).and_then(Value::as_f64).map_or(d, |x| x as f32)
                    };
                    s.shape = match items.first().and_then(Value::as_str) {
                        Some("point") => Shape::Point,
                        Some("sphere") => Shape::Sphere { radius: n(1, 1.0) },
                        Some("hemisphere") => Shape::Hemisphere { radius: n(1, 1.0) },
                        Some("cone") => Shape::Cone {
                            angle: n(1, 25.0),
                            radius: n(2, 1.0),
                        },
                        Some("box") => Shape::Box {
                            size: Vec3::new(n(1, 1.0), n(2, 1.0), n(3, 1.0)),
                        },
                        _ => return Err(bad()),
                    };
                }
                "sizeOverLifetime" => s.size_over_life = rows::<2>(v).ok_or_else(bad)?,
                "colorOverLifetime" => s.color_over_life = rows::<5>(v).ok_or_else(bad)?,
                "texture" => {
                    s.texture = match v {
                        Value::Null => DEFAULT_TEXTURE,
                        _ => num()? as u32,
                    }
                }
                "additive" => s.additive = matches!(v, Value::Bool(true)),
                "sheet" => {
                    let [c, r, n] = floats::<3>(v).ok_or_else(bad)?;
                    s.sheet = [c.max(1.0).floor(), r.max(1.0).floor(), n.max(0.0)];
                }
                "seed" => s.seed = num()? as u32,
                other => return Err(format!("unknown particle setting `{other}`")),
            }
        }
        Ok(s)
    }
}

/// Linear interpolation through `(t, values...)` keys; 1s with no keys.
fn curve<const N: usize, const K: usize>(keys: &[[f32; K]], t: f32) -> [f32; N] {
    debug_assert_eq!(K, N + 1);
    let value = |k: &[f32; K]| -> [f32; N] { std::array::from_fn(|i| k[i + 1]) };
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
        return [1.0; N];
    };
    if t <= first[0] {
        return value(first);
    }
    if t >= last[0] {
        return value(last);
    }
    for w in keys.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if t <= b[0] {
            let f = if b[0] > a[0] {
                (t - a[0]) / (b[0] - a[0])
            } else {
                1.0
            };
            return std::array::from_fn(|i| a[i + 1] + (b[i + 1] - a[i + 1]) * f);
        }
    }
    value(last)
}

/// A small, fast, seedable generator (xorshift32): particles don't need more.
#[derive(Clone, Debug)]
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
    fn range(&mut self, r: [f32; 2]) -> f32 {
        r[0] + (r[1] - r[0]) * self.next()
    }
    /// A uniformly random point inside the unit sphere.
    fn in_sphere(&mut self) -> Vec3 {
        loop {
            let p = Vec3::new(self.next(), self.next(), self.next()) * 2.0 - Vec3::ONE;
            if p.length_squared() <= 1.0 {
                return p;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    /// World space, or the object's space when not simulating in world space.
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub size: f32,
    /// Degrees.
    pub rotation: f32,
    pub angular_velocity: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct ParticleSystem {
    pub object: u32,
    pub settings: Settings,
    pub particles: Vec<Particle>,
    /// Seconds since `play`.
    time: f32,
    /// The cycle the bursts' counts are for, and how often each has fired in it.
    cycle: u32,
    fired: Vec<u32>,
    owed: f32,
    playing: bool,
    emitting: bool,
    rng: Rng,
    /// The particle count and liveness scripts last heard.
    reported: (usize, bool),
}

impl ParticleSystem {
    pub fn new(object: u32, settings: Settings) -> Self {
        let rng = Rng::new(settings.seed);
        Self {
            object,
            fired: vec![0; settings.bursts.len()],
            settings,
            particles: Vec::new(),
            time: 0.0,
            cycle: 0,
            owed: 0.0,
            playing: false,
            emitting: false,
            rng,
            reported: (0, false),
        }
    }

    /// New settings keep the live particles and the clock.
    pub fn set_settings(&mut self, settings: Settings) {
        self.fired = vec![0; settings.bursts.len()];
        self.settings = settings;
    }

    pub fn play(&mut self) {
        if !self.emitting {
            self.time = 0.0;
            self.cycle = 0;
            self.owed = 0.0;
            self.fired.iter_mut().for_each(|f| *f = 0);
        }
        self.playing = true;
        self.emitting = true;
    }

    /// Stops emitting; live particles finish their lives.
    pub fn stop(&mut self) {
        self.emitting = false;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn clear(&mut self) {
        self.particles.clear();
    }

    /// Whether it's emitting or has particles left (Unity's `IsAlive`).
    pub fn is_alive(&self) -> bool {
        self.emitting || !self.particles.is_empty()
    }

    /// The count and liveness, if they changed since last asked.
    pub fn take_change(&mut self) -> Option<(usize, bool)> {
        let now = (self.particles.len(), self.is_alive());
        (now != self.reported).then(|| {
            self.reported = now;
            now
        })
    }

    /// Emits `count` particles now, from `world` (the object's world transform).
    pub fn emit(&mut self, count: u32, world: &Affine3A) {
        let room = self
            .settings
            .max_particles
            .saturating_sub(self.particles.len());
        for _ in 0..(count as usize).min(room) {
            let p = self.spawn(world);
            self.particles.push(p);
        }
    }

    fn spawn(&mut self, world: &Affine3A) -> Particle {
        let s = &self.settings;
        let rng = &mut self.rng;
        let (position, direction) = match s.shape {
            Shape::Point => (Vec3::ZERO, Vec3::Y),
            Shape::Sphere { radius } => {
                let p = rng.in_sphere();
                (p * radius, p.try_normalize().unwrap_or(Vec3::Y))
            }
            Shape::Hemisphere { radius } => {
                let mut p = rng.in_sphere();
                p.y = p.y.abs();
                (p * radius, p.try_normalize().unwrap_or(Vec3::Y))
            }
            Shape::Cone { angle, radius } => {
                // A point on the base disc; its direction leans out by the angle in
                // proportion to how far out it is (straight up at the centre).
                let a = rng.next() * std::f32::consts::TAU;
                let r = rng.next().sqrt();
                let disc = Vec3::new(a.cos() * r, 0.0, a.sin() * r);
                let tilt = angle.clamp(0.0, 89.0).to_radians().tan();
                let lean = if radius > 0.0 {
                    disc
                } else {
                    disc.normalize_or_zero() * rng.next()
                };
                (disc * radius, (Vec3::Y + lean * tilt).normalize())
            }
            Shape::Box { size } => {
                let p = Vec3::new(rng.next(), rng.next(), rng.next()) - Vec3::splat(0.5);
                (p * size, Vec3::Y)
            }
        };
        let speed = rng.range(s.speed);
        let mix = rng.next();
        let [a, b] = s.color;
        let color = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * mix);
        let (position, velocity) = if s.world_space {
            (
                world.transform_point3(position),
                world.transform_vector3(direction).normalize_or_zero() * speed,
            )
        } else {
            (position, direction * speed)
        };
        Particle {
            position,
            velocity,
            age: 0.0,
            lifetime: rng.range(s.lifetime).max(0.001),
            size: rng.range(s.size),
            rotation: rng.range(s.rotation),
            angular_velocity: rng.range(s.angular_velocity),
            color,
        }
    }

    /// Advances `dt` seconds: emission (rate and bursts) while emitting, then motion.
    pub fn update(&mut self, dt: f32, world: &Affine3A) {
        if !self.playing || dt <= 0.0 {
            return;
        }
        self.time += dt;
        if self.emitting {
            self.emit_due(dt, world);
        }
        let s = &self.settings;
        let mut gravity = Vec3::new(0.0, -9.81 * s.gravity, 0.0);
        if !s.world_space {
            // Local particles fall down in the world, whichever way the object faces.
            gravity = world.inverse().transform_vector3(gravity);
        }
        let keep = (1.0 - s.drag * dt).max(0.0);
        for p in &mut self.particles {
            p.age += dt;
            p.velocity = (p.velocity + gravity * dt) * keep;
            p.position += p.velocity * dt;
            p.rotation += p.angular_velocity * dt;
        }
        self.particles.retain(|p| p.age < p.lifetime);
    }

    fn emit_due(&mut self, dt: f32, world: &Affine3A) {
        let s = &self.settings;
        let t = self.time - s.start_delay;
        if t < 0.0 {
            return;
        }
        let cycle = (t / s.duration) as u32;
        if !s.looping && cycle >= 1 {
            self.emitting = false;
            return;
        }
        if cycle != self.cycle {
            self.cycle = cycle;
            self.fired.iter_mut().for_each(|f| *f = 0);
        }
        let in_cycle = t - cycle as f32 * s.duration;
        let mut count = 0;
        for (b, fired) in s.bursts.iter().zip(&mut self.fired) {
            // Every firing due by now in this cycle that hasn't happened yet.
            while b.cycles == 0 || *fired < b.cycles {
                let at = b.time + *fired as f32 * b.interval;
                if at > in_cycle || at >= s.duration {
                    break;
                }
                count += b.count;
                *fired += 1;
            }
        }
        // Only the part of this frame after the start delay counts towards the rate.
        self.owed += s.rate * dt.min(t);
        let due = self.owed.floor();
        self.owed -= due;
        self.emit(count + due as u32, world);
    }

    /// The particles as quads facing `camera_rotation`, given the object's world
    /// transform.
    pub fn quads(&self, world: &Affine3A, camera_rotation: Quat, out: &mut Vec<WorldQuad>) {
        let s = &self.settings;
        let [cols, rows, cycles] = s.sheet;
        let frames = (cols * rows) as u32;
        for p in &self.particles {
            let life = (p.age / p.lifetime).clamp(0.0, 1.0);
            let [k] = curve::<1, 2>(&s.size_over_life, life);
            let [r, g, b, a] = curve::<4, 5>(&s.color_over_life, life);
            let size = p.size * k;
            if size <= 0.0 {
                continue;
            }
            let position = if s.world_space {
                p.position
            } else {
                world.transform_point3(p.position)
            };
            let rotation = camera_rotation * Quat::from_rotation_z(-p.rotation.to_radians());
            let uv = if frames > 1 {
                let f = (life * cycles * frames as f32) as u32 % frames;
                let (c, r) = ((f % cols as u32) as f32, (f / cols as u32) as f32);
                [c / cols, r / rows, 1.0 / cols, 1.0 / rows]
            } else {
                [0.0, 0.0, 1.0, 1.0]
            };
            out.push(WorldQuad {
                model: Mat4::from_scale_rotation_translation(
                    Vec3::new(size, size, 1.0),
                    rotation,
                    position,
                ),
                color: [
                    p.color[0] * r,
                    p.color[1] * g,
                    p.color[2] * b,
                    p.color[3] * a,
                ],
                texture: Some(WorldTexture::Asset(s.texture)),
                uv,
                additive: s.additive,
                flat: false,
                order: 0,
                layer: 0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(pairs: Vec<Value>) -> ParticleSystem {
        let mut p = ParticleSystem::new(1, Settings::parse(&pairs).unwrap());
        p.play();
        p
    }

    fn s(k: &str) -> Value {
        Value::String(k.into())
    }

    fn n(v: f64) -> Value {
        Value::from(v)
    }

    fn arr(v: &[f64]) -> Value {
        Value::Array(v.iter().map(|x| Value::from(*x)).collect())
    }

    // M5-PRT-01
    #[test]
    fn rate_and_bursts_emit_on_time_and_particles_die_after_their_lifetime() {
        let world = Affine3A::IDENTITY;
        let mut p = system(vec![
            s("rate"),
            n(10.0),
            s("lifetime"),
            n(0.55),
            s("bursts"),
            arr(&[0.25, 5.0, 2.0, 0.1]),
        ]);
        for _ in 0..30 {
            p.update(1.0 / 60.0, &world);
        }
        // Half a second: 5 from the rate, and both bursts (at 0.25 and 0.35 s).
        assert_eq!(p.particles.len(), 15);
        for _ in 0..30 {
            p.update(1.0 / 60.0, &world);
        }
        // Rate particles live 0.55 s: those from the first half second are mostly gone,
        // and the bursts are.
        assert!(
            (5..=7).contains(&p.particles.len()),
            "{}",
            p.particles.len()
        );
        p.stop();
        for _ in 0..60 {
            p.update(1.0 / 60.0, &world);
        }
        assert!(p.particles.is_empty());
    }

    #[test]
    fn one_shot_systems_stop_after_their_duration_and_respect_max_particles() {
        let world = Affine3A::IDENTITY;
        let mut p = system(vec![
            s("duration"),
            n(0.5),
            s("looping"),
            Value::Bool(false),
            s("rate"),
            n(100.0),
            s("maxParticles"),
            n(20.0),
            s("lifetime"),
            n(10.0),
        ]);
        for _ in 0..60 {
            p.update(1.0 / 60.0, &world);
        }
        assert_eq!(p.particles.len(), 20);
        assert!(!p.emitting && p.is_alive());
    }

    #[test]
    fn gravity_pulls_world_down_and_cones_spray_upwards_from_the_object() {
        // An object 5 m up, upside down: local particles still fall in world -y.
        let world = Affine3A::from_rotation_translation(
            Quat::from_rotation_x(std::f32::consts::PI),
            Vec3::new(0.0, 5.0, 0.0),
        );
        let mut p = system(vec![
            s("shape"),
            Value::Array(vec![s("cone"), n(20.0), n(0.0)]),
            s("speed"),
            n(0.0),
            s("gravity"),
            n(1.0),
            s("rate"),
            n(0.0),
        ]);
        p.emit(1, &world);
        for _ in 0..60 {
            p.update(1.0 / 60.0, &world);
        }
        let at = world.transform_point3(p.particles[0].position);
        assert!((at.y - (5.0 - 0.5 * 9.81)).abs() < 0.2, "{at}");

        let mut cone = system(vec![
            s("shape"),
            Value::Array(vec![s("cone"), n(30.0), n(0.5)]),
            s("rate"),
            n(0.0),
        ]);
        cone.emit(200, &Affine3A::IDENTITY);
        let max_tilt = 30f32.to_radians().tan();
        for q in &cone.particles {
            let d = q.velocity.normalize();
            assert!(d.y > 0.0 && Vec3::new(d.x, 0.0, d.z).length() / d.y <= max_tilt + 1e-3);
            assert!(q.position.y == 0.0 && q.position.length() <= 0.5 + 1e-4);
        }
    }

    #[test]
    fn curves_shape_size_and_colour_over_life_and_sheets_pick_frames() {
        let mut p = system(vec![
            s("rate"),
            n(0.0),
            s("lifetime"),
            n(1.0),
            s("speed"),
            n(0.0),
            s("size"),
            n(2.0),
            s("sizeOverLifetime"),
            arr(&[0.0, 0.0, 1.0, 1.0]),
            s("colorOverLifetime"),
            arr(&[0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.0, 0.0]),
            s("sheet"),
            arr(&[2.0, 2.0, 1.0]),
            s("additive"),
            Value::Bool(true),
        ]);
        p.emit(1, &Affine3A::IDENTITY);
        p.update(0.5, &Affine3A::IDENTITY);
        let mut quads = Vec::new();
        p.quads(&Affine3A::IDENTITY, Quat::IDENTITY, &mut quads);
        let q = quads[0];
        // Half way: size 2 × 0.5, alpha 1 → 0 half way, frame 2 of 4 (bottom-left).
        assert!((q.model.x_axis.x - 1.0).abs() < 1e-5);
        assert_eq!(q.color, [1.0, 0.75, 0.5, 0.5]);
        assert_eq!(q.uv, [0.0, 0.5, 0.5, 0.5]);
        assert!(q.additive);
        assert!(Settings::parse(&[s("nope"), n(1.0)]).is_err());
    }
}
