// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The script side: the `AudioZone`, `AudioOccluder` and `SoundSensor` components, the
//! `SoundEvent` handle and the `Acoustics` namespace.

use crate::model::{Curve, Shape};
use crate::state::{
    AudioOccluderRef, AudioPortalRef, AudioZoneRef, Occluder, Portal, Sensor, SoundEventRef,
    SoundSensorRef, Zone, push_event, st,
};
use crate::update::{events, probe, stop_ambience};
use platypus_engine_api::audio::{BusRef, SoundRef};
use platypus_engine_api::audio_components::{Directivity, Rolloff};
use platypus_math::V3;
use platypus_scene::ObjectId;
use platypus_scene::api::with_scene;
use ts_api::callback::HandleArg;
use ts_api::{Callback, ts_api};

fn none_object() -> ObjectId {
    ObjectId {
        index: u32::MAX,
        generation: u32::MAX,
    }
}

fn arr(v: V3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn vec3(a: [f64; 3]) -> V3 {
    V3 {
        x: a[0],
        y: a[1],
        z: a[2],
    }
}

pub(crate) fn zone<R>(z: AudioZoneRef, f: impl FnOnce(&mut Zone) -> R) -> Option<R> {
    st(|s| s.zones.get_mut(z.0).map(f))
}

pub(crate) fn portal<R>(p: AudioPortalRef, f: impl FnOnce(&mut Portal) -> R) -> Option<R> {
    st(|s| s.portals.get_mut(p.0).map(f))
}

pub(crate) fn attach_portal(object: ObjectId) -> AudioPortalRef {
    platypus_kinds::components_changed();
    AudioPortalRef(st(|s| s.portals.add(Portal::new(object))))
}

pub(crate) fn remove_portal(p: AudioPortalRef) {
    platypus_kinds::components_changed();
    st(|s| s.portals.remove(p.0));
}

pub(crate) fn portals_on(object: ObjectId) -> Vec<AudioPortalRef> {
    st(|s| {
        s.portals
            .iter()
            .filter(|(_, p)| p.object == object)
            .map(|(i, _)| AudioPortalRef(i))
            .collect()
    })
}

fn occluder<R>(o: AudioOccluderRef, f: impl FnOnce(&mut Occluder) -> R) -> Option<R> {
    st(|s| s.occluders.get_mut(o.0).map(f))
}

fn sensor<R>(x: SoundSensorRef, f: impl FnOnce(&mut Sensor) -> R) -> Option<R> {
    st(|s| s.sensors.get_mut(x.0).map(f))
}

pub(crate) fn attach_zone(object: ObjectId) -> AudioZoneRef {
    platypus_kinds::components_changed();
    AudioZoneRef(st(|s| s.zones.add(Zone::new(object))))
}

pub(crate) fn attach_occluder(object: ObjectId) -> AudioOccluderRef {
    platypus_kinds::components_changed();
    AudioOccluderRef(st(|s| s.occluders.add(Occluder::new(object))))
}

pub(crate) fn attach_sensor(object: ObjectId) -> SoundSensorRef {
    platypus_kinds::components_changed();
    SoundSensorRef(st(|s| s.sensors.add(Sensor::new(object))))
}

pub(crate) fn remove_zone(z: AudioZoneRef) {
    platypus_kinds::components_changed();
    if let Some(Some(p)) = st(|s| s.zones.remove(z.0)).map(|z| z.playback) {
        stop_ambience(p);
    }
}

pub(crate) fn remove_occluder(o: AudioOccluderRef) {
    platypus_kinds::components_changed();
    st(|s| s.occluders.remove(o.0));
}

pub(crate) fn remove_sensor(x: SoundSensorRef) {
    platypus_kinds::components_changed();
    st(|s| s.sensors.remove(x.0));
}

pub(crate) fn zones_on(object: ObjectId) -> Vec<AudioZoneRef> {
    st(|s| {
        s.zones
            .iter()
            .filter(|(_, z)| z.object == object)
            .map(|(i, _)| AudioZoneRef(i))
            .collect()
    })
}

pub(crate) fn occluders_on(object: ObjectId) -> Vec<AudioOccluderRef> {
    st(|s| {
        s.occluders
            .iter()
            .filter(|(_, o)| o.object == object)
            .map(|(i, _)| AudioOccluderRef(i))
            .collect()
    })
}

pub(crate) fn sensors_on(object: ObjectId) -> Vec<SoundSensorRef> {
    st(|s| {
        s.sensors
            .iter()
            .filter(|(_, x)| x.object == object)
            .map(|(i, _)| SoundSensorRef(i))
            .collect()
    })
}

#[cfg(test)]
pub(crate) fn occluder_open(o: AudioOccluderRef, openness: f64) {
    occluder(o, |o| o.openness = openness);
}

/// Registers a Rust closure as a sensor's `onHear` (tests).
#[cfg(test)]
pub(crate) fn test_on_hear(x: SoundSensorRef, f: impl Fn(f64) + 'static) {
    sensor(x, |x| {
        x.callbacks.push(Callback::from_fn(
            move |(_, level): (HandleArg<SoundEventRef>, f64)| f(level),
        ))
    });
}

pub(crate) fn surface<R>(
    s: crate::state::AudioSurfaceRef,
    f: impl FnOnce(&mut crate::state::Surface) -> R,
) -> Option<R> {
    st(|st| st.surfaces.get_mut(s.0).map(f))
}

pub(crate) fn attach_surface(object: ObjectId) -> crate::state::AudioSurfaceRef {
    platypus_kinds::components_changed();
    crate::state::AudioSurfaceRef(st(|s| {
        s.surfaces.add(crate::state::Surface {
            object,
            name: "default".to_string(),
        })
    }))
}

pub(crate) fn remove_surface(s: crate::state::AudioSurfaceRef) {
    platypus_kinds::components_changed();
    st(|st| st.surfaces.remove(s.0));
}

pub(crate) fn surfaces_on(object: ObjectId) -> Vec<crate::state::AudioSurfaceRef> {
    st(|s| {
        s.surfaces
            .iter()
            .filter(|(_, x)| x.object == object)
            .map(|(i, _)| crate::state::AudioSurfaceRef(i))
            .collect()
    })
}

/// Removes the acoustics components of destroyed objects.
pub fn object_destroyed(object: ObjectId) {
    for s in surfaces_on(object) {
        remove_surface(s);
    }
    for z in zones_on(object) {
        remove_zone(z);
    }
    for p in portals_on(object) {
        remove_portal(p);
    }
    for o in occluders_on(object) {
        remove_occluder(o);
    }
    for x in sensors_on(object) {
        remove_sensor(x);
    }
}

#[ts_api(
    namespace = "AudioZone",
    handles(
        AudioZone = AudioZoneRef,
        GameObject = ObjectId,
        Sound = SoundRef,
        Bus = BusRef
    ),
    values(Vector3 = V3 [x: f64, y: f64, z: f64]),
    enums(
        /// The volume a zone covers: a `sphere` (its `radius`) or a `box` (its `size`).
        ZoneShape = Shape [Sphere, Box],
        /// How the effect grows from the zone's edge to its core: `linear`, `smooth` (no corner
        /// at the edge), `easeIn` (weak until near the core) or `easeOut` (strong just inside).
        ZoneCurve = Curve [Linear, Smooth, EaseIn, EaseOut]
    )
)]
pub mod audio_zone_api {
    use super::*;

    impl AudioZoneRef {
        /// Adds an `AudioZone` to a GameObject. While the listener is inside it, the buses it
        /// names (the master mix if none) take on its filter, reverb and echo settings, more
        /// strongly the deeper the listener is: entering a cave, a tunnel, a room.
        pub fn attach(object: ObjectId) -> AudioZoneRef {
            super::attach_zone(object)
        }

        pub fn find_on(object: ObjectId) -> AudioZoneRef {
            super::zones_on(object)
                .first()
                .copied()
                .unwrap_or(AudioZoneRef(0))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            super::zone(self, |z| z.object).unwrap_or_else(super::none_object)
        }

        /// A zone that is not enabled does nothing.
        #[property]
        pub fn enabled(self) -> bool {
            super::zone(self, |z| z.enabled).unwrap_or(false)
        }

        pub fn set_enabled(self, on: bool) {
            super::zone(self, |z| z.enabled = on);
        }

        #[property]
        pub fn shape(self) -> Shape {
            super::zone(self, |z| z.shape).unwrap_or(Shape::Sphere)
        }

        pub fn set_shape(self, v: Shape) {
            super::zone(self, |z| z.shape = v);
        }

        /// The sphere's outer radius in metres (times the object's scale): the effect is gone
        /// here.
        #[property]
        pub fn radius(self) -> f64 {
            super::zone(self, |z| z.radius).unwrap_or(10.0)
        }

        pub fn set_radius(self, v: f64) {
            super::zone(self, |z| z.radius = v.max(0.0));
        }

        /// The box's outer size in metres (times the object's scale).
        #[property]
        pub fn size(self) -> V3 {
            super::zone(self, |z| super::vec3(z.size)).unwrap_or(super::vec3([10.0; 3]))
        }

        pub fn set_size(self, v: V3) {
            super::zone(self, |z| z.size = super::arr(v));
        }

        /// How much of the volume has the full effect, 0 to 1: the core. Between the core and
        /// the edge the effect follows `curve`.
        #[property]
        pub fn inner_fraction(self) -> f64 {
            super::zone(self, |z| z.inner).unwrap_or(0.5)
        }

        pub fn set_inner_fraction(self, v: f64) {
            super::zone(self, |z| z.inner = v.clamp(0.0, 1.0));
        }

        #[property]
        pub fn curve(self) -> Curve {
            super::zone(self, |z| z.curve).unwrap_or(Curve::Smooth)
        }

        pub fn set_curve(self, v: Curve) {
            super::zone(self, |z| z.curve = v);
        }

        /// Strongest everywhere outside the volume instead of inside it: the nature a cave's
        /// exit lets in.
        #[property]
        pub fn invert(self) -> bool {
            super::zone(self, |z| z.invert).unwrap_or(false)
        }

        pub fn set_invert(self, on: bool) {
            super::zone(self, |z| z.invert = on);
        }

        /// Where zones overlap on a bus the higher priority wins.
        #[property]
        pub fn priority(self) -> f64 {
            super::zone(self, |z| z.priority).unwrap_or(0.0)
        }

        pub fn set_priority(self, v: f64) {
            super::zone(self, |z| z.priority = v);
        }

        /// Low-pass cutoff in Hz at full strength (22000 leaves it open): a cave takes the
        /// harshness off a voice.
        #[property]
        pub fn low_pass(self) -> f64 {
            super::zone(self, |z| z.target.low_pass).unwrap_or(22000.0)
        }

        pub fn set_low_pass(self, hz: f64) {
            super::zone(self, |z| z.target.low_pass = hz.clamp(20.0, 22000.0));
        }

        /// High-pass cutoff in Hz at full strength (0 leaves it open): thin and tinny.
        #[property]
        pub fn high_pass(self) -> f64 {
            super::zone(self, |z| z.target.high_pass).unwrap_or(0.0)
        }

        pub fn set_high_pass(self, hz: f64) {
            super::zone(self, |z| z.target.high_pass = hz.clamp(0.0, 22000.0));
        }

        /// Reverb wet level at full strength, 0 to 1.
        #[property]
        pub fn reverb(self) -> f64 {
            super::zone(self, |z| z.target.reverb).unwrap_or(0.0)
        }

        pub fn set_reverb(self, v: f64) {
            super::zone(self, |z| z.target.reverb = v.clamp(0.0, 1.0));
        }

        /// How big the reverberating room sounds, 0 to 1.
        #[property]
        pub fn reverb_size(self) -> f64 {
            super::zone(self, |z| z.target.reverb_size).unwrap_or(0.5)
        }

        pub fn set_reverb_size(self, v: f64) {
            super::zone(self, |z| z.target.reverb_size = v.clamp(0.0, 1.0));
        }

        /// How fast the reverb loses its highs, 0 bright to 1 dull.
        #[property]
        pub fn reverb_damping(self) -> f64 {
            super::zone(self, |z| z.target.reverb_damping).unwrap_or(0.5)
        }

        pub fn set_reverb_damping(self, v: f64) {
            super::zone(self, |z| z.target.reverb_damping = v.clamp(0.0, 1.0));
        }

        /// Echo wet level at full strength, 0 to 1.
        #[property]
        pub fn echo(self) -> f64 {
            super::zone(self, |z| z.target.echo).unwrap_or(0.0)
        }

        pub fn set_echo(self, v: f64) {
            super::zone(self, |z| z.target.echo = v.clamp(0.0, 1.0));
        }

        /// Gain of the buses it names at full strength (linear). It leaves the master volume
        /// alone: that is the game's.
        #[property]
        pub fn volume(self) -> f64 {
            super::zone(self, |z| z.target.volume).unwrap_or(1.0)
        }

        pub fn set_volume(self, v: f64) {
            super::zone(self, |z| z.target.volume = v.max(0.0));
        }

        /// A looping sound that fades in with the zone's strength: the sea along a shore, birds
        /// outside a cave (with `invert`), the hum of a room.
        #[property]
        pub fn ambience(self) -> SoundRef {
            super::zone(self, |z| z.ambience).unwrap_or(SoundRef(0))
        }

        pub fn set_ambience(self, sound: SoundRef) {
            super::zone(self, |z| z.ambience = sound);
        }

        /// The ambience's volume at full strength (linear).
        #[property]
        pub fn ambience_volume(self) -> f64 {
            super::zone(self, |z| z.ambience_volume).unwrap_or(1.0)
        }

        pub fn set_ambience_volume(self, v: f64) {
            super::zone(self, |z| z.ambience_volume = v.max(0.0));
        }

        /// The bus the ambience plays on (none: the master mix).
        #[property]
        pub fn ambience_bus(self) -> BusRef {
            super::zone(self, |z| BusRef(z.ambience_bus)).unwrap_or(BusRef(0))
        }

        pub fn set_ambience_bus(self, bus: BusRef) {
            super::zone(self, |z| z.ambience_bus = bus.0);
        }

        /// Also colours the `AudioSource`s that stand in the zone (their emitters, made with
        /// `effects`), by where each stands, not only the listener: a radio in a cave echoes for
        /// someone outside it.
        #[property]
        pub fn affects_sources(self) -> bool {
            super::zone(self, |z| z.affects_sources).unwrap_or(false)
        }

        pub fn set_affects_sources(self, on: bool) {
            super::zone(self, |z| z.affects_sources = on);
        }

        /// Names a bus the zone processes (made with `effects`). With none named it processes
        /// the master mix.
        pub fn add_bus(self, bus: BusRef) {
            super::zone(self, |z| {
                if bus.0 != 0 && !z.buses.contains(&bus.0) {
                    z.buses.push(bus.0);
                }
            });
        }

        /// Forgets the named buses: the zone processes the master mix again.
        pub fn clear_buses(self) {
            super::zone(self, |z| z.buses.clear());
        }

        /// How strongly the zone applies at the listener now, 0 to 1 (as of the last frame).
        #[property]
        pub fn weight(self) -> f64 {
            super::zone(self, |z| z.weight).unwrap_or(0.0)
        }

        /// How strongly the zone would apply to a listener at `point`, 0 to 1.
        pub fn weight_at(self, point: V3) -> f64 {
            let Some(z) = super::zone(self, |z| z.clone()) else {
                return 0.0;
            };
            crate::update::weight_at(&z, super::arr(point))
        }

        pub fn remove(self) {
            super::remove_zone(self);
        }
    }
}

#[ts_api(
    namespace = "AudioPortal",
    handles(AudioPortal = AudioPortalRef, GameObject = ObjectId)
)]
pub mod audio_portal_api {
    use super::*;

    impl AudioPortalRef {
        /// Adds an `AudioPortal` to a GameObject standing in an opening (a doorway, a window, a
        /// stairwell). When a wall is between a source and the listener and the way round by a
        /// portal is better, the source is heard from the portal, quieter and duller, with the
        /// distance of the whole way round: sound seems to come through the door.
        pub fn attach(object: ObjectId) -> AudioPortalRef {
            super::attach_portal(object)
        }

        pub fn find_on(object: ObjectId) -> AudioPortalRef {
            super::portals_on(object)
                .first()
                .copied()
                .unwrap_or(AudioPortalRef(0))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            super::portal(self, |p| p.object).unwrap_or_else(super::none_object)
        }

        #[property]
        pub fn enabled(self) -> bool {
            super::portal(self, |p| p.enabled).unwrap_or(false)
        }

        pub fn set_enabled(self, on: bool) {
            super::portal(self, |p| p.enabled = on);
        }

        /// How much gets through, 0 to 1: a window a script can shut.
        #[property]
        pub fn gain(self) -> f64 {
            super::portal(self, |p| p.gain).unwrap_or(1.0)
        }

        pub fn set_gain(self, v: f64) {
            super::portal(self, |p| p.gain = v.clamp(0.0, 1.0));
        }

        /// Metres short of the portal that the far leg stops (0.25), so a door standing in the
        /// opening is counted once.
        #[property]
        pub fn radius(self) -> f64 {
            super::portal(self, |p| p.radius).unwrap_or(0.25)
        }

        pub fn set_radius(self, v: f64) {
            super::portal(self, |p| p.radius = v.clamp(0.0, 5.0));
        }

        /// The low-pass cutoff in Hz of sound that bends through (9000): edges take the highs off.
        #[property]
        pub fn low_pass(self) -> f64 {
            super::portal(self, |p| p.low_pass).unwrap_or(9000.0)
        }

        pub fn set_low_pass(self, hz: f64) {
            super::portal(self, |p| p.low_pass = hz.clamp(100.0, 22000.0));
        }

        pub fn remove(self) {
            super::remove_portal(self);
        }
    }
}

#[ts_api(
    namespace = "AudioSurface",
    handles(AudioSurface = AudioSurfaceRef, GameObject = ObjectId)
)]
pub mod audio_surface_api {
    use super::*;
    use crate::state::AudioSurfaceRef;

    impl AudioSurfaceRef {
        /// Adds an `AudioSurface` to a GameObject: what it is made of, as a name (`"metal"`,
        /// `"wood"`, `"stone"`, `"grass"`...). Sounds that strike it look the name up: ray from a
        /// tool or a foot, read `getComponent(AudioSurface).surface` on what it hits, and play the
        /// sound set for that name. The name means nothing to the engine; games choose theirs.
        pub fn attach(object: ObjectId) -> AudioSurfaceRef {
            super::attach_surface(object)
        }

        pub fn find_on(object: ObjectId) -> AudioSurfaceRef {
            super::surfaces_on(object)
                .first()
                .copied()
                .unwrap_or(AudioSurfaceRef(0))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            super::surface(self, |s| s.object).unwrap_or_else(super::none_object)
        }

        /// The surface's name (`"default"` until set).
        #[property]
        pub fn surface(self) -> String {
            super::surface(self, |s| s.name.clone()).unwrap_or_default()
        }

        pub fn set_surface(self, name: &str) {
            super::surface(self, |s| s.name = name.to_string());
        }

        pub fn remove(self) {
            super::remove_surface(self);
        }
    }
}

#[ts_api(
    namespace = "AudioOccluder",
    handles(AudioOccluder = AudioOccluderRef, GameObject = ObjectId)
)]
pub mod audio_occluder_api {
    use super::*;

    impl AudioOccluderRef {
        /// Adds an `AudioOccluder` to a GameObject that has colliders. Sounds that pass through
        /// those colliders on their way to the listener (and to sensors) come out quieter and
        /// duller: a wall, a door, a floor.
        pub fn attach(object: ObjectId) -> AudioOccluderRef {
            super::attach_occluder(object)
        }

        pub fn find_on(object: ObjectId) -> AudioOccluderRef {
            super::occluders_on(object)
                .first()
                .copied()
                .unwrap_or(AudioOccluderRef(0))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            super::occluder(self, |o| o.object).unwrap_or_else(super::none_object)
        }

        /// 0 shut (the default: a wall, a closed door) to 1 open (an open door): how much gets
        /// through. Change it as a door swings.
        #[property]
        pub fn openness(self) -> f64 {
            super::occluder(self, |o| o.openness).unwrap_or(0.0)
        }

        pub fn set_openness(self, v: f64) {
            super::occluder(self, |o| o.openness = v.clamp(0.0, 1.0));
        }

        /// How much volume is lost when shut, 0 to 1.
        #[property]
        pub fn absorption(self) -> f64 {
            super::occluder(self, |o| o.absorption).unwrap_or(0.55)
        }

        pub fn set_absorption(self, v: f64) {
            super::occluder(self, |o| o.absorption = v.clamp(0.0, 1.0));
        }

        /// The low-pass cutoff in Hz a shut occluder leaves (thick walls keep only the lows).
        #[property]
        pub fn low_pass(self) -> f64 {
            super::occluder(self, |o| o.low_pass).unwrap_or(900.0)
        }

        pub fn set_low_pass(self, hz: f64) {
            super::occluder(self, |o| o.low_pass = hz.clamp(100.0, 22000.0));
        }

        pub fn remove(self) {
            super::remove_occluder(self);
        }
    }
}

#[ts_api(
    namespace = "SoundSensor",
    handles(
        SoundSensor = SoundSensorRef,
        SoundEvent = SoundEventRef,
        GameObject = ObjectId
    )
)]
pub mod sound_sensor_api {
    use super::*;

    impl SoundSensorRef {
        /// Adds a `SoundSensor` to a GameObject: ears that hear noises made with
        /// `Acoustics.emit` and `AudioSource`s with a `loudness`, through walls, as far as their
        /// loudness reaches. `onHear` runs for each noise it hears.
        pub fn attach(object: ObjectId) -> SoundSensorRef {
            super::attach_sensor(object)
        }

        pub fn find_on(object: ObjectId) -> SoundSensorRef {
            super::sensors_on(object)
                .first()
                .copied()
                .unwrap_or(SoundSensorRef(0))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            super::sensor(self, |x| x.object).unwrap_or_else(super::none_object)
        }

        #[property]
        pub fn enabled(self) -> bool {
            super::sensor(self, |x| x.enabled).unwrap_or(false)
        }

        pub fn set_enabled(self, on: bool) {
            super::sensor(self, |x| x.enabled = on);
        }

        /// Scales how far it hears (2 hears twice as far, 0.5 half).
        #[property]
        pub fn sensitivity(self) -> f64 {
            super::sensor(self, |x| x.sensitivity).unwrap_or(1.0)
        }

        pub fn set_sensitivity(self, v: f64) {
            super::sensor(self, |x| x.sensitivity = v.max(0.0));
        }

        /// The quietest level (0 to 1) it reacts to.
        #[property]
        pub fn threshold(self) -> f64 {
            super::sensor(self, |x| x.threshold).unwrap_or(0.03)
        }

        pub fn set_threshold(self, v: f64) {
            super::sensor(self, |x| x.threshold = v.clamp(0.0, 1.0));
        }

        /// Whether walls between it and a noise quieten the noise (default true).
        #[property]
        pub fn occlusion(self) -> bool {
            super::sensor(self, |x| x.occlusion).unwrap_or(true)
        }

        pub fn set_occlusion(self, on: bool) {
            super::sensor(self, |x| x.occlusion = on);
        }

        /// The loudest thing it hears now, 0 to 1: a noise makes it jump, then it falls away.
        #[property]
        pub fn level(self) -> f64 {
            super::sensor(self, |x| x.level).unwrap_or(0.0)
        }

        /// Calls `f(event, level)` for each noise it hears (a gunshot, a footstep, a source
        /// that starts being audible): `event` has `position`, `kind` and `source`; `level` is
        /// how loud it was here, 0 to 1.
        pub fn on_hear(self, f: Callback<(HandleArg<SoundEventRef>, f64)>) {
            super::sensor(self, |x| x.callbacks.push(f));
        }

        /// Drops every `onHear` callback.
        pub fn clear_on_hear(self) {
            super::sensor(self, |x| x.callbacks.clear());
        }

        pub fn remove(self) {
            super::remove_sensor(self);
        }
    }
}

fn event<R>(e: SoundEventRef, f: impl FnOnce(&crate::state::Event) -> R) -> Option<R> {
    events().iter().find(|x| x.id == e.0).map(f)
}

#[ts_api(
    namespace = "SoundEvent",
    handles(SoundEvent = SoundEventRef, GameObject = ObjectId),
    values(Vector3 = V3 [x: f64, y: f64, z: f64])
)]
pub mod sound_event_api {
    use super::*;

    impl SoundEventRef {
        /// Where the noise was made.
        #[property]
        pub fn position(self) -> V3 {
            super::event(self, |e| super::vec3(e.position)).unwrap_or(super::vec3([0.0; 3]))
        }

        /// What it was (`"gunshot"`, `"footstep"`, `"source"` for something that is playing).
        #[property]
        pub fn kind(self) -> String {
            super::event(self, |e| e.kind.clone()).unwrap_or_default()
        }

        /// Metres it can be heard from.
        #[property]
        pub fn loudness(self) -> f64 {
            super::event(self, |e| e.loudness).unwrap_or(0.0)
        }

        /// Seconds since it was made (a large number once it has faded away).
        #[property]
        pub fn age(self) -> f64 {
            let now = st(|s| s.time);
            super::event(self, |e| now - e.born).unwrap_or(1e9)
        }

        /// The object that made it (none for a noise without one).
        #[property]
        pub fn source(self) -> ObjectId {
            super::event(self, |e| e.source)
                .flatten()
                .unwrap_or_else(super::none_object)
        }

        /// Made by a playing source on a timer, not by `Acoustics.emit`.
        #[property]
        pub fn pulse(self) -> bool {
            super::event(self, |e| e.pulse).unwrap_or(false)
        }

        /// Whether it is still in the list of recent noises.
        #[property]
        pub fn alive(self) -> bool {
            super::event(self, |_| true).unwrap_or(false)
        }
    }
}

#[ts_api(
    namespace = "Acoustics",
    handles(SoundEvent = SoundEventRef, GameObject = ObjectId),
    values(Vector3 = V3 [x: f64, y: f64, z: f64]),
    enums(
        extern Directivity = Directivity [Omni, SemiDirectional, Directional, Custom],
        extern Rolloff = Rolloff [Default, Inverse, Linear, Exponential]
    )
)]
pub mod acoustics_api {
    use super::*;
    use platypus_engine_api::audio_components::audio_source_api::{
        directivity_from_index, rolloff_from_index,
    };

    /// Makes a noise at `position` that sensors within `loudness` metres can hear (`kind`
    /// says what it was; `source` is the object that made it, if any). It stays in
    /// `Acoustics.eventAt` for `eventLifetime` seconds, for a radar or a map to draw.
    #[optional(source)]
    pub fn emit(position: V3, loudness: f64, kind: &str, source: ObjectId) -> SoundEventRef {
        let source = with_scene(|s| s.contains(source))
            .unwrap_or(false)
            .then_some(source);
        SoundEventRef(push_event(
            super::arr(position),
            loudness.max(0.0),
            kind,
            source,
            false,
        ))
    }

    /// How many noises are alive.
    #[property]
    pub fn event_count() -> f64 {
        events().len() as f64
    }

    /// The `index`th noise alive (oldest first), or none.
    #[nullable]
    pub fn event_at(index: u32) -> SoundEventRef {
        events()
            .get(index as usize)
            .map_or(SoundEventRef(0), |e| SoundEventRef(e.id))
    }

    /// How loud the world is at `position`, 0 to 1: the loudest playing source with a `loudness`
    /// and recent noise that reaches it, through walls. Sample it over a grid for a heat map.
    pub fn level_at(position: V3) -> f64 {
        crate::update::level_at(super::arr(position))
    }

    /// How much of a sound at `to` gets through to `from` (1 clear, 0 blocked): what
    /// occluders on the way let through.
    pub fn transmission(from: V3, to: V3) -> f64 {
        let (specs, layers, spread) = crate::update::probe_settings();
        probe(super::arr(from), super::arr(to), &specs, layers, spread).gain
    }

    /// The speed of sound in m/s for Doppler (343).
    #[property]
    pub fn speed_of_sound() -> f64 {
        st(|s| s.settings.speed_of_sound)
    }

    pub fn set_speed_of_sound(v: f64) {
        st(|s| s.settings.speed_of_sound = v.clamp(10.0, 100_000.0));
    }

    /// Layers whose colliders muffle sound even with no `AudioOccluder` (a bit per layer,
    /// `LayerMask.getMask`). 0, the default, makes only occluders count.
    #[property]
    pub fn occlusion_layers() -> u32 {
        st(|s| s.settings.occlusion_layers)
    }

    pub fn set_occlusion_layers(mask: u32) {
        st(|s| s.settings.occlusion_layers = mask);
    }

    /// How far (metres) to the sides of the straight line occlusion also looks (0.4): a
    /// doorway the line misses still lets some sound in.
    #[property]
    pub fn ray_spread() -> f64 {
        st(|s| s.settings.ray_spread)
    }

    pub fn set_ray_spread(v: f64) {
        st(|s| s.settings.ray_spread = v.clamp(0.0, 10.0));
    }

    /// How much sounds behind or above the listener lose their highs (0 to 1, default 0.5):
    /// the cue that tells front from back and up from down, which panning cannot. It applies
    /// to `AudioSource`s while acoustics is in use (a zone, occluder, sensor, noise or
    /// Doppler source exists).
    #[property]
    pub fn head_shadow() -> f64 {
        st(|s| s.settings.head_shadow)
    }

    pub fn set_head_shadow(v: f64) {
        st(|s| s.settings.head_shadow = v.clamp(0.0, 1.0));
    }

    /// How much distance takes the highs off sounds (0 to 2, default 0.5): at 1, 11 kHz is left at
    /// 100 m and 3 kHz at 300 m. Each `AudioSource` scales it with its own `airAbsorption`. Applies
    /// while acoustics is in use.
    #[property]
    pub fn air_absorption() -> f64 {
        st(|s| s.settings.air_absorption)
    }

    pub fn set_air_absorption(v: f64) {
        st(|s| s.settings.air_absorption = v.clamp(0.0, 2.0));
    }

    /// The cutoff in Hz that air leaves of a sound that travelled `distance` metres, at the current
    /// `airAbsorption`.
    pub fn air_cutoff(distance: f64) -> f64 {
        crate::model::air_cutoff(distance, st(|s| s.settings.air_absorption))
    }

    /// How loud a source of this shape is `angle` degrees off its front, 0 to 1: the curve the
    /// mixer uses, for drawing a polar plot. `width` (degrees across) and `rear` (0 to 1) are used
    /// by `custom`; the other shapes have their own.
    #[optional(width = 120, rear = 0.25)]
    pub fn directivity_gain(pattern: Directivity, angle: f64, width: f64, rear: f64) -> f64 {
        crate::model::pattern_gain(pattern, angle, width, rear)
    }

    /// The gain a source's `rolloff` gives at `distance` metres (1 full, towards 0): for plotting how
    /// a source fades. `inverse` is `min / d` at factor 1 (6 dB less for each doubling); `default`
    /// is only the shape of the mixer's own curve.
    pub fn rolloff_gain(
        rolloff: Rolloff,
        distance: f64,
        min_distance: f64,
        max_distance: f64,
        factor: f64,
    ) -> f64 {
        crate::model::rolloff_gain(rolloff, distance, min_distance, max_distance, factor)
    }

    /// Seconds a noise stays in the list (3).
    #[property]
    pub fn event_lifetime() -> f64 {
        st(|s| s.settings.event_lifetime)
    }

    pub fn set_event_lifetime(v: f64) {
        st(|s| s.settings.event_lifetime = v.clamp(0.1, 60.0));
    }

    /// Seconds between the pulses a playing source with a `loudness` makes (0.8).
    #[property]
    pub fn pulse_interval() -> f64 {
        st(|s| s.settings.pulse_interval)
    }

    pub fn set_pulse_interval(v: f64) {
        st(|s| s.settings.pulse_interval = v.clamp(0.1, 10.0));
    }

    /// Turns the whole system off (zones stay as they were, nothing updates).
    #[property]
    pub fn enabled() -> bool {
        st(|s| s.settings.enabled)
    }

    pub fn set_enabled(on: bool) {
        st(|s| s.settings.enabled = on);
    }

    /// The listener's speed in m/s as acoustics measured it (for Doppler).
    #[property]
    pub fn listener_speed() -> f64 {
        st(|s| {
            let v = s.listener_velocity;
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
        })
    }
}
