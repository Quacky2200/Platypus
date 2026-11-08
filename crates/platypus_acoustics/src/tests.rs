// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the whole path: scene objects, physics rays and the audio commands that come out
//! (`ACO-SYS-*`). The maths alone is tested in `model.rs`.

use crate::components::{attach_occluder, attach_sensor, attach_zone, zone};
use crate::model::Shape;
use crate::state::{push_event, st};
use crate::update::update;
use glam::{Quat, Vec3};
use platypus_engine_api::audio::{self, AudioCommand};
use platypus_engine_api::audio_components::{attach_source, source_views};
use platypus_math::V3;
use platypus_physics_api::{BoxColliderRef, step};
use platypus_scene::{ObjectId, Scene};
use std::cell::Cell;
use std::rc::Rc;

fn setup() -> Scene {
    crate::reset();
    audio::reset();
    platypus_physics_api::reset();
    Scene::new()
}

fn make(name: &str, x: f32, y: f32, z: f32) -> ObjectId {
    platypus_scene::api::with_scene(|s| {
        let o = s.create(name);
        s.set_world(o, Vec3::new(x, y, z), Quat::IDENTITY);
        o
    })
    .unwrap()
}

fn fx(commands: &[AudioCommand], track: u32, property: &str) -> Option<f64> {
    commands.iter().rev().find_map(|c| match c {
        AudioCommand::Fx {
            track: t,
            property: p,
            value,
            ..
        } if *t == track && p == property => Some(*value),
        _ => None,
    })
}

// ACO-SYS-01
//
// A cave zone around the origin: the master gets dull and reverberant as the listener walks in,
// and open again when they leave.
#[test]
fn a_zone_processes_the_master_by_how_deep_the_listener_is() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let cave = make("cave", 0.0, 0.0, 0.0);
    let z = attach_zone(cave);
    zone(z, |z| {
        z.radius = 10.0;
        z.inner = 0.0;
        z.curve = crate::model::Curve::Linear;
        z.target.low_pass = 2000.0;
        z.target.reverb = 0.8;
    });
    audio::set_listener([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let inside = audio::take_commands();
    assert!((fx(&inside, 0, "reverb").unwrap() - 0.8).abs() < 1e-6);
    assert!((fx(&inside, 0, "lowpass").unwrap() - 2000.0).abs() < 1.0);
    assert_eq!(zone(z, |z| z.weight), Some(1.0));

    audio::set_listener([5.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let half = audio::take_commands();
    assert!((fx(&half, 0, "reverb").unwrap() - 0.4).abs() < 1e-6);

    audio::set_listener([30.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let out = audio::take_commands();
    assert_eq!(fx(&out, 0, "reverb"), Some(0.0));
    assert_eq!(fx(&out, 0, "lowpass"), Some(22000.0));

    // Nothing changes: nothing is sent.
    update(0.016);
    assert!(audio::take_commands().is_empty());

    // Removing the zone opens the track again.
    audio::set_listener([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    audio::take_commands();
    crate::object_destroyed(cave);
    update(0.016);
    assert_eq!(fx(&audio::take_commands(), 0, "reverb"), Some(0.0));
}

// ACO-SYS-02
//
// A box zone on a named bus drives that bus's volume too, and a box is measured in its own
// (scaled) space.
#[test]
fn a_box_zone_drives_its_bus_volume_and_follows_scale() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let room = make("room", 0.0, 0.0, 0.0);
    platypus_scene::api::with_scene(|s| {
        let mut t = s.local(room).unwrap();
        t.scale = Vec3::new(2.0, 1.0, 1.0);
        s.set_local(room, t);
    });
    let z = attach_zone(room);
    let bus = audio::create_bus(1.0, None, true);
    zone(z, |z| {
        z.shape = Shape::Box;
        z.size = [4.0, 4.0, 4.0];
        z.inner = 1.0;
        z.buses.push(bus.0);
        z.target.volume = 0.25;
    });
    // 3.5 m along x is inside: the box is 4 wide, doubled by the scale to 8, so 4 m each way.
    audio::set_listener([3.5, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let cmds = audio::take_commands();
    assert!(cmds.iter().any(|c| matches!(c,
        AudioCommand::TrackVolume { track: Some(t), volume, .. } if *t == bus.0 && (*volume - 0.25).abs() < 1e-9)));
    // The same distance along z is outside (2 m half size there).
    audio::set_listener([0.0, 0.0, 3.5], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    assert!(audio::take_commands().iter().any(|c| matches!(c,
        AudioCommand::TrackVolume { track: Some(t), volume, .. } if *t == bus.0 && (*volume - 1.0).abs() < 1e-9)));
}

// ACO-SYS-03
//
// A radio behind a wall is dulled and quietened; opening the occluder lets it through again.
#[test]
fn a_wall_dulls_a_source_and_opening_it_lets_the_sound_through() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let wall = make("wall", 5.0, 0.0, 0.0);
    BoxColliderRef::attach(wall).set_size(V3 {
        x: 0.5,
        y: 10.0,
        z: 10.0,
    });
    let occ = attach_occluder(wall);
    let radio = make("radio", 10.0, 0.0, 0.0);
    let src = attach_source(radio);
    platypus_engine_api::audio_components::with_source(src, |d| {
        d.spatial_blend = 1.0;
        d.clip = audio::cached_sound("radio.wav");
    });
    platypus_engine_api::audio_components::with_source(src, |d| d.play());
    step();
    audio::take_commands();
    audio::set_listener([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let emitter = source_views()[0].emitter.expect("it plays from an emitter");

    update(0.016);
    let behind = audio::take_commands();
    let low = fx(&behind, emitter.0, "lowpass").expect("dulled");
    assert!(low < 1500.0, "behind the wall: {low} Hz");
    assert!(behind.iter().any(|c| matches!(c,
        AudioCommand::TrackVolume { track: Some(t), volume, .. } if *t == emitter.0 && *volume < 0.7)));

    // The wall opens (a door swung wide): the next probe finds the way clear.
    crate::components::occluder_open(occ, 1.0);
    for _ in 0..8 {
        update(0.016);
    }
    let open = audio::take_commands();
    assert_eq!(fx(&open, emitter.0, "lowpass"), Some(22000.0));
}

// ACO-SYS-04
//
// A sensor in the next room hears a gunshot through the wall quieter than one in the open, and
// not at all out of range; a sensor hears each noise once.
#[test]
fn a_sensor_hears_noises_once_quieter_through_walls_and_not_beyond_their_reach() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let wall = make("wall", 5.0, 0.0, 0.0);
    BoxColliderRef::attach(wall).set_size(V3 {
        x: 0.5,
        y: 10.0,
        z: 10.0,
    });
    attach_occluder(wall);
    step();
    let guard = make("guard", 8.0, 0.0, 0.0);
    let sensor = attach_sensor(guard);
    let heard = Rc::new(Cell::new((0u32, 0.0f64)));
    crate::components::test_on_hear(sensor, {
        let heard = heard.clone();
        move |level| heard.set((heard.get().0 + 1, level))
    });
    let shot = |at: f64, loudness: f64| {
        push_event([at, 0.0, 0.0], loudness, "gunshot", None, false);
    };
    // In the open, 3 m from the guard (same side of the wall).
    shot(11.0, 30.0);
    update(0.016);
    let (n, open_level) = heard.get();
    assert_eq!(n, 1);
    // Through the wall, also 3 m away.
    shot(2.0, 30.0);
    update(0.016);
    let (n, wall_level) = heard.get();
    assert_eq!(n, 2);
    assert!(
        wall_level < open_level * 0.8,
        "{wall_level} vs {open_level}"
    );
    // Too quiet to reach.
    shot(-40.0, 10.0);
    update(0.016);
    assert_eq!(heard.get().0, 2);
    // Not delivered again on later frames.
    update(0.016);
    assert_eq!(heard.get().0, 2);
    assert!(st(|s| s.sensors.get_mut(sensor.0).unwrap().level) < open_level);
}

// ACO-SYS-05
//
// A source flying at the listener plays higher, one leaving plays lower, and standing still
// leaves the pitch alone.
#[test]
fn doppler_changes_the_rate_of_a_moving_source() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let car = make("car", 100.0, 0.0, 0.0);
    let src = attach_source(car);
    platypus_engine_api::audio_components::with_source(src, |d| {
        d.spatial_blend = 1.0;
        d.doppler_level = 1.0;
        d.clip = audio::cached_sound("horn.wav");
    });
    platypus_engine_api::audio_components::with_source(src, |d| d.play());
    audio::take_commands();
    audio::set_listener([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let rate = |cmds: &[AudioCommand]| {
        cmds.iter().rev().find_map(|c| match c {
            AudioCommand::Set {
                property: "rate",
                value,
                ..
            } => Some(*value),
            _ => None,
        })
    };
    // 30 m/s towards the listener for a second.
    for i in 0..60 {
        platypus_scene::api::with_scene(|s| {
            s.set_world(
                car,
                Vec3::new(100.0 - 0.5 * i as f32, 0.0, 0.0),
                Quat::IDENTITY,
            )
        });
        update(1.0 / 60.0);
    }
    let coming = rate(&audio::take_commands()).expect("rate set");
    assert!(coming > 1.05 && coming < 1.15, "coming: {coming}");
    // And away.
    for i in 0..60 {
        platypus_scene::api::with_scene(|s| {
            s.set_world(
                car,
                Vec3::new(70.0 + 0.5 * i as f32, 0.0, 0.0),
                Quat::IDENTITY,
            )
        });
        update(1.0 / 60.0);
    }
    let going = rate(&audio::take_commands()).expect("rate set");
    assert!(going < 0.95, "going: {going}");
    // Standing still: back to 1.
    for _ in 0..90 {
        update(1.0 / 60.0);
    }
    let still = rate(&audio::take_commands()).expect("rate set");
    assert!((still - 1.0).abs() < 0.01, "still: {still}");
}
