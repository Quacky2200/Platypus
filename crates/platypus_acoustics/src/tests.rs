// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the whole path: scene objects, physics rays and the audio commands that come out
//! (`ACO-SYS-*`). The maths alone is tested in `model.rs`.

use crate::components::{attach_occluder, attach_portal, attach_sensor, attach_zone, zone};
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
    assert!(
        fx(&open, emitter.0, "lowpass").is_some_and(|hz| hz > 20000.0),
        "open again (the air over 10 m takes a sliver)"
    );
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

/// A source with an emitter that is playing (3D, with the full chain if asked).
fn playing_source(object: ObjectId, effects: bool) -> platypus_engine_api::audio::EmitterRef {
    let src = attach_source(object);
    platypus_engine_api::audio_components::with_source(src, |d| {
        d.spatial_blend = 1.0;
        d.effects = effects;
        d.clip = audio::cached_sound("s.wav");
        d.play();
    });
    audio::take_commands();
    source_views()
        .iter()
        .find(|v| v.object == object)
        .and_then(|v| v.emitter)
        .expect("an emitter")
}

// ACO-SYS-06
//
// A wall between listener and radio, with an opening round its end marked by a portal: the radio
// is heard from the portal (its emitter moves there) and duller by the edge, not as muffled as
// through the wall.
#[test]
fn a_portal_sends_sound_round_a_wall_and_it_is_heard_from_the_opening() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let wall = make("wall", 5.0, 0.0, 0.0);
    BoxColliderRef::attach(wall).set_size(V3 {
        x: 0.5,
        y: 10.0,
        z: 6.0,
    });
    attach_occluder(wall);
    let doorway = make("doorway", 5.0, 0.0, 5.0);
    attach_portal(doorway);
    let radio = make("radio", 10.0, 0.0, 0.0);
    let emitter = playing_source(radio, false);
    step();
    audio::set_listener([0.0; 3], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let cmds = audio::take_commands();
    let heard_from = cmds.iter().rev().find_map(|c| match c {
        AudioCommand::EmitterPosition {
            emitter: e,
            position,
            ..
        } if e.0 == emitter.0 => Some(*position),
        _ => None,
    });
    let at = heard_from.expect("the emitter moved to the portal's direction");
    assert!(
        at[2] > 8.0 && at[0] > 5.0,
        "round by the doorway at +z: {at:?}"
    );
    let low = fx(&cmds, emitter.0, "lowpass").expect("dulled");
    assert!(
        low > 4000.0 && low < 12000.0,
        "dulled by the edge, not by the wall: {low}"
    );
    // The portal is shut (a script closes a window): back to the wall.
    crate::components::portal(crate::AudioPortalRef(1), |p| p.gain = 0.0);
    for _ in 0..8 {
        update(0.016);
    }
    let cmds = audio::take_commands();
    let low = fx(&cmds, emitter.0, "lowpass").expect("muffled again");
    assert!(low < 1500.0, "through the wall: {low}");
    let back = cmds.iter().rev().find_map(|c| match c {
        AudioCommand::EmitterPosition {
            emitter: e,
            position,
            ..
        } if e.0 == emitter.0 => Some(*position),
        _ => None,
    });
    assert!(
        back.is_some_and(|p| (p[0] - 10.0).abs() < 1e-6 && p[2].abs() < 1e-6),
        "heard from the radio again: {back:?}"
    );
}

// ACO-SYS-07
//
// A zone that affects sources colours an emitter by where it stands: reverb on one inside, none
// on one outside; a zone that does not leaves sources alone.
#[test]
fn a_zone_can_colour_the_sources_standing_in_it() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let cave = make("cave", 0.0, 0.0, 0.0);
    let z = attach_zone(cave);
    zone(z, |z| {
        z.radius = 10.0;
        z.inner = 1.0;
        z.target.reverb = 0.6;
        z.target.low_pass = 3000.0;
    });
    let inside = make("inside", 2.0, 0.0, 0.0);
    let outside = make("outside", 30.0, 0.0, 0.0);
    let e_in = playing_source(inside, true);
    let e_out = playing_source(outside, true);
    audio::set_listener([50.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let off = audio::take_commands();
    assert_eq!(
        fx(&off, e_in.0, "reverb"),
        None,
        "zones colour the listener only by default"
    );
    zone(z, |z| z.affects_sources = true);
    update(0.016);
    let cmds = audio::take_commands();
    assert!((fx(&cmds, e_in.0, "reverb").unwrap() - 0.6).abs() < 1e-6);
    assert!(fx(&cmds, e_in.0, "lowpass").unwrap() < 3500.0);
    assert_eq!(
        fx(&cmds, e_out.0, "reverb"),
        None,
        "outside the zone nothing changes"
    );
}

// ACO-SYS-08
//
// Behind the listener a source loses its highs; ahead of it, it does not.
#[test]
fn sounds_behind_the_listener_are_dulled_and_those_ahead_are_not() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    // Something to switch acoustics on (an occluder far away).
    attach_occluder(make("anything", 100.0, 0.0, 100.0));
    let behind = make("behind", 0.0, 0.0, 10.0);
    let ahead = make("ahead", 0.0, 0.0, -10.0);
    let e_behind = playing_source(behind, false);
    let e_ahead = playing_source(ahead, false);
    // The listener looks down -z.
    audio::set_listener([0.0; 3], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let cmds = audio::take_commands();
    let dull = fx(&cmds, e_behind.0, "lowpass").expect("dulled");
    assert!(dull > 8000.0 && dull < 12000.0, "{dull}");
    // Ahead stays essentially open: the air over 10 m takes only a sliver of the top off.
    assert!(
        fx(&cmds, e_ahead.0, "lowpass").is_none_or(|hz| hz > 20000.0),
        "ahead stays open"
    );
}

/// The gain last sent to an emitter's track.
fn track_gain(commands: &[AudioCommand], track: u32) -> Option<f64> {
    commands.iter().rev().find_map(|c| match c {
        AudioCommand::TrackVolume {
            track: Some(t),
            volume,
            ..
        } if *t == track => Some(*volume),
        _ => None,
    })
}

// ACO-SYS-09
//
// A cardioid speaker is loud for a listener it faces and quiet and dull for one behind it; the
// same goes for a cone; an omni source does not change.
#[test]
fn a_directional_source_is_loud_in_front_and_quiet_and_dull_behind() {
    use platypus_engine_api::audio_components::{Directivity, with_source};
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    // A busy world, so acoustics is on.
    attach_occluder(make("anything", 100.0, 0.0, 100.0));
    // Both sources at 10 m ahead of a listener at the origin (looking down -z). The first faces
    // the listener (its -z turned to +z), the second faces away.
    let facing = make("facing", 0.0, 0.0, -10.0);
    let away = make("away", 5.0, 0.0, -10.0);
    platypus_scene::api::with_scene(|s| {
        s.set_world(
            facing,
            Vec3::new(0.0, 0.0, -10.0),
            Quat::from_rotation_y(std::f32::consts::PI),
        );
    });
    let e_facing = playing_source(facing, false);
    let e_away = playing_source(away, false);
    for object in [facing, away] {
        let id = source_views()
            .iter()
            .find(|v| v.object == object)
            .unwrap()
            .source;
        with_source(id, |d| d.directivity = Directivity::Cardioid);
    }
    audio::set_listener([0.0; 3], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let cmds = audio::take_commands();
    assert!(
        track_gain(&cmds, e_facing.0).is_none_or(|g| g > 0.95),
        "facing the listener: full"
    );
    let behind = track_gain(&cmds, e_away.0).expect("quieter");
    assert!(behind < 0.2, "facing away is almost silent: {behind}");
    let dull = fx(&cmds, e_away.0, "lowpass").expect("and dull");
    assert!(dull < 7000.0, "{dull}");
    // Strength 0 is all round again.
    let id = source_views()
        .iter()
        .find(|v| v.object == away)
        .unwrap()
        .source;
    with_source(id, |d| d.directivity_strength = 0.0);
    update(0.016);
    let cmds = audio::take_commands();
    assert!(track_gain(&cmds, e_away.0).is_some_and(|g| g > 0.95));
    // A cone instead of a pattern: 60 degrees in front, 120 outer, a quarter as loud outside it.
    with_source(id, |d| {
        d.directivity = Directivity::Omni;
        d.cone_inner = 60.0;
        d.cone_outer = 120.0;
        d.cone_outer_gain = 0.25;
    });
    update(0.016);
    let cmds = audio::take_commands();
    assert!(track_gain(&cmds, e_away.0).is_some_and(|g| (g - 0.25).abs() < 0.02));
}

// ACO-SYS-12
//
// A source with an inverse rolloff gets quieter with distance by the law (-6 dB per doubling), the
// mixer's own distance falloff is moved out of its way, and one with the default rolloff is left to
// the mixer.
#[test]
fn a_rolloff_makes_a_source_fall_with_distance_by_its_law() {
    use platypus_engine_api::audio_components::{Rolloff, with_source};
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    // Rolloff alone is enough to bring acoustics in.
    let near = make("near", 6.0, 0.0, 0.0);
    let far = make("far", 300.0, 0.0, 0.0);
    let plain = make("plain", 0.0, 300.0, 0.0);
    let mut ids = Vec::new();
    for (object, rolloff) in [
        (near, Rolloff::Inverse),
        (far, Rolloff::Inverse),
        (plain, Rolloff::Default),
    ] {
        let src = attach_source(object);
        with_source(src, |d| {
            d.spatial_blend = 1.0;
            d.min_distance = 3.0;
            d.max_distance = 400.0;
            d.rolloff = rolloff;
            d.clip = audio::cached_sound("s.wav");
            d.play();
        });
        ids.push(src);
    }
    // The emitters of rolloff sources were made with the mixer's falloff moved far away.
    let made = audio::take_commands();
    let distances: Vec<[f64; 2]> = made
        .iter()
        .filter_map(|c| match c {
            AudioCommand::Emitter { distances, .. } => Some(*distances),
            _ => None,
        })
        .collect();
    assert_eq!(distances.len(), 3);
    assert!(
        distances[0][0] > 1.0e5 && distances[1][0] > 1.0e5,
        "{distances:?}"
    );
    assert_eq!(
        distances[2],
        [3.0, 400.0],
        "the default keeps the mixer's curve"
    );
    let emitter = |object| {
        source_views()
            .iter()
            .find(|v| v.object == object)
            .and_then(|v| v.emitter)
            .unwrap()
    };
    audio::set_listener([0.0; 3], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let cmds = audio::take_commands();
    let at_6 = track_gain(&cmds, emitter(near).0).expect("6 m is twice the minimum distance");
    assert!((at_6 - 0.5).abs() < 0.02, "-6 dB: {at_6}");
    let at_300 = track_gain(&cmds, emitter(far).0).expect("300 m");
    assert!((at_300 - 0.01).abs() < 0.002, "-40 dB: {at_300}");
    assert_eq!(
        track_gain(&cmds, emitter(plain).0),
        None,
        "the mixer decays the default"
    );
    // Twice the factor falls faster.
    with_source(ids[1], |d| d.rolloff_factor = 2.0);
    update(0.016);
    let cmds = audio::take_commands();
    assert!(track_gain(&cmds, emitter(far).0).is_some_and(|g| g < at_300));
}

// ACO-SYS-11
//
// A surface tag is a name on an object that scripts look up; it survives a scene-file round trip
// through its kind and goes away with the object.
#[test]
fn a_surface_tag_names_what_an_object_is_made_of() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    crate::register_kinds();
    let crate_ = make("crate", 0.0, 0.0, 0.0);
    assert!(crate::components::surfaces_on(crate_).is_empty());
    let s = crate::components::attach_surface(crate_);
    assert_eq!(
        crate::components::surface(s, |s| s.name.clone()).as_deref(),
        Some("default")
    );
    crate::components::surface(s, |s| s.name = "wood".into());
    assert_eq!(crate::components::surfaces_on(crate_), vec![s]);
    // Through the kind: save, load into a fresh tag.
    let kind = platypus_kinds::get("AudioSurface").expect("registered");
    let saved = (kind.save)(u64::from(s.0));
    assert_eq!(saved.get("surface").and_then(|v| v.as_str()), Some("wood"));
    let other = make("barrel", 1.0, 0.0, 0.0);
    let t = crate::components::attach_surface(other);
    assert!((kind.load)(u64::from(t.0), &saved).is_empty());
    assert_eq!(
        crate::components::surface(t, |s| s.name.clone()).as_deref(),
        Some("wood")
    );
    crate::object_destroyed(crate_);
    assert!(crate::components::surfaces_on(crate_).is_empty());
}

// ACO-SYS-10
//
// Air takes the highs off with distance: a source 300 m away is duller than one 10 m away, and a
// source that asks for no air keeps its highs.
#[test]
fn air_dulls_far_sources_and_a_source_can_opt_out() {
    use platypus_engine_api::audio_components::with_source;
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    attach_occluder(make("anything", 1000.0, 0.0, 1000.0));
    let near = make("near", 10.0, 0.0, 0.0);
    let far = make("far", 300.0, 0.0, 0.0);
    let far_dry = make("far dry", 0.0, 300.0, 0.0);
    let e_near = playing_source(near, false);
    let e_far = playing_source(far, false);
    let e_dry = playing_source(far_dry, false);
    let id = source_views()
        .iter()
        .find(|v| v.object == far_dry)
        .unwrap()
        .source;
    with_source(id, |d| d.air_absorption = 0.0);
    // Facing the listener's side, so the head cue does not confuse it.
    crate::state::st(|s| s.settings.head_shadow = 0.0);
    audio::set_listener([0.0; 3], [0.0, 0.0, 0.0, 1.0]);
    update(0.016);
    let cmds = audio::take_commands();
    let near_hz = fx(&cmds, e_near.0, "lowpass").unwrap_or(22000.0);
    let far_hz = fx(&cmds, e_far.0, "lowpass").expect("dulled by air");
    assert!(near_hz > 20000.0, "{near_hz}");
    assert!(
        far_hz < 9000.0 && far_hz > 2500.0,
        "300 m of air at the default strength: {far_hz}"
    );
    assert_eq!(fx(&cmds, e_dry.0, "lowpass"), None, "no air, no dullness");
    // More air: duller.
    crate::state::st(|s| s.settings.air_absorption = 2.0);
    update(0.016);
    let cmds = audio::take_commands();
    assert!(fx(&cmds, e_far.0, "lowpass").is_some_and(|hz| hz < far_hz));
}
