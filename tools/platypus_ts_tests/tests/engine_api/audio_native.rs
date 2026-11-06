// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Native audio from compiled programs (`bench/audio_native.ts`): the commands the host would run
//! on the mixer, in the normal, stress GC and collection-off modes. IDs are in TESTS.md
//! (M9-TS-1210..).
use platypus_engine_api::audio::{self, AudioCommand, PlaybackRef};
use platypus_engine_api::audio_components;
use platypus_scene::Scene;
use platypus_ts_tests::audio_native_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    let run = || {
        audio::reset();
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        unsafe { ts_aud_init() };
        f();
    };
    run();
    ts_rt_gc_stress(true);
    run();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    run();
    ts_rt_gc_enable(true);
    audio::reset();
}

fn text(p: *mut u8) -> String {
    assert!(!p.is_null());
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

fn ops() -> Vec<String> {
    audio::take_commands()
        .iter()
        .map(|c| match c.to_op() {
            Some((name, args)) => format!(
                "{name} {}",
                args.iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            None => match c {
                AudioCommand::Load { path, stream, .. } => format!("load {path} {stream}"),
                _ => unreachable!(),
            },
        })
        .collect()
}

// M9-TS-1210
#[test]
fn sounds_play_with_options_and_playbacks_change() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_aud_playbacks()),
            "sfx/hit.wav|music/theme.ogg|true|true|music/theme.ogg|0.7"
        );
        assert_eq!(
            ops(),
            [
                "load sfx/hit.wav false",
                "load music/theme.ogg true",
                "bus 1 null 0.5",
                "bus 2 1 0.25",
                // volume, rate, pan, loop, fadeIn, start; on bus 2
                "play 1 1 2 [0.8,1.5,0.0,true,0.0,0.0]",
                "set 1 \"volume\" 0.4 0.2",
                "set 1 \"pan\" -1.0 0.0",
                "pause 1 0.0",
                "resume 1 0.1",
                "seek 1 1.5",
                "stop 1 0.3",
                "play 2 2 null [1.0,1.0,0.0,false,0.0,0.0]",
                "trackVolume 1 0.9 1.0",
                "trackPause 1 0.0",
                "removeTrack 2",
                "trackVolume null 0.7 0.0",
            ]
        );
    });
}

// M9-TS-1211
#[test]
fn on_ended_runs_when_the_host_reports_the_end() {
    in_every_gc_mode(|| unsafe {
        let _ = ts_aud_playbacks();
        assert_eq!(text(ts_aud_endedLog()), "");
        audio::ended(&[PlaybackRef(1)]);
        assert_eq!(text(ts_aud_endedLog()), "p;");
        audio::ended(&[PlaybackRef(1)]);
        assert_eq!(text(ts_aud_endedLog()), "p;", "only once");
    });
}

// M9-TS-1212
#[test]
fn audio_play_loads_a_path_once_and_options_name_the_parameters() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_aud_playByPath()), "1|ui/click.wav|ui/click.wav");
        assert_eq!(
            ops(),
            [
                "load ui/click.wav false",
                "play 1 1 null [1.0,1.0,0.0,false,0.0,0.0]",
                "play 2 1 null [0.5,1.0,0.0,false,0.0,0.0]",
            ]
        );
    });
}

// M9-TS-1213
#[test]
fn emitters_move_and_the_listener_is_set() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_aud_emitterAndListener()), "ok");
        assert_eq!(
            ops(),
            [
                "emitter 1 [1.0,2.0,3.0] [2.0,20.0] null",
                "emitterPosition 1 [4.0,5.0,6.0] 0.5",
                "trackVolume 1 0.5 0.0",
                "listener [0.0,1.0,0.0] [0.0,0.0,0.0,1.0]",
                "removeTrack 1",
            ]
        );
    });
}

// M9-TS-1214
#[test]
fn a_3d_audio_source_plays_from_an_emitter_that_follows_its_object() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_aud_source(), 1.0);
        let o = ops();
        assert_eq!(o[0], "load sfx/hit.wav false");
        // playOnAwake is not run by addComponent (the scene loader wakes components): play()
        // makes the emitter at the object's world position, with min distance 3.
        assert_eq!(o[1], "emitter 1 [2.0,0.0,0.0] [3.0,50.0] null");
        assert_eq!(o[2], "play 1 1 1 [0.5,1.0,0.0,true,0.0,0.0]");
        // pitch = 2, mute = true change the playback; the one shot plays at gain 0 (muted).
        assert_eq!(o[3], "set 1 \"rate\" 2.0 0.0");
        assert_eq!(o[4], "set 1 \"volume\" 0.0 0.0");
        assert!(o[5].starts_with("play 2 1 1 [0.0,2.0,"), "{}", o[5]);
        // Moving the object sends the emitter's new position, once.
        let go = platypus_scene::api::with_scene(|s| s.find("Speaker"))
            .unwrap()
            .unwrap();
        platypus_scene::api::with_scene(|s| {
            s.set_world(go, glam::Vec3::new(5.0, 0.0, 0.0), glam::Quat::IDENTITY);
            s.update_world();
        });
        audio_components::pre_render();
        audio_components::pre_render();
        assert_eq!(ops(), ["emitterPosition 1 [5.0,0.0,0.0] 0.0"]);
    });
}

// M9-TS-1215
#[test]
fn a_flat_audio_source_pauses_resumes_and_stops_its_clip() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_aud_sourceFlat()), "false|true|false|true");
        let o = ops();
        assert!(o.iter().any(|c| c.starts_with("pause 1")), "{o:?}");
        assert!(o.iter().any(|c| c.starts_with("resume 1")), "{o:?}");
        assert!(o.iter().any(|c| c.starts_with("stop 1")), "{o:?}");
        assert!(
            o.iter()
                .any(|c| c == "play 1 1 null [1.0,1.0,-0.5,false,0.0,0.0]"),
            "{o:?}"
        );
    });
}

// M9-TS-1216
#[test]
fn the_listener_component_sends_the_objects_pose_and_the_volume_is_global() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_aud_listenerFollows(), 1.0);
        assert_eq!(ops(), ["listener [0.0,2.0,0.0] [0.0,0.0,0.0,1.0]"]);
        audio::reset();
    });
}

// M9-TS-1217
#[test]
fn play_clip_at_point_makes_an_emitter_and_removes_it_when_the_sound_ends() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_aud_clipAtPoint(), 1.0);
        let o = ops();
        assert_eq!(o[1], "emitter 1 [1.0,1.0,1.0] [1.0,50.0] null");
        assert_eq!(o[2], "play 1 1 1 [0.5,1.0,0.0,false,0.0,0.0]");
        audio::ended(&[PlaybackRef(1)]);
        assert_eq!(ops(), ["removeTrack 1"]);
    });
}

// M9-TS-1218
#[test]
fn destroying_an_object_stops_its_sources() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_aud_source(), 1.0);
        ops();
        let go = platypus_scene::api::with_scene(|s| s.find("Speaker"))
            .unwrap()
            .unwrap();
        platypus_scene::api::with_scene(|s| s.destroy(go));
        audio_components::remove_on(go);
        let o = ops();
        assert!(o.iter().any(|c| c.starts_with("stop ")), "{o:?}");
        assert!(o.iter().any(|c| c == "removeTrack 1"), "{o:?}");
    });
}

// M9-TS-1219
#[test]
fn scene_files_load_and_save_audio_components_and_play_on_awake() {
    let _serial = platypus_ts_tests::serial();
    audio::reset();
    platypus_scenes::reset();
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let data = r#"{"format":"platypus.scene","version":1,"name":"Hall","objects":[
     {"id":1,"name":"Speaker","position":[3,0,0],"components":[
       {"id":2,"type":"AudioSource","properties":{"clip":{"$type":"Sound","src":"sfx/hit.wav"},
         "spatialBlend":1,"volume":0.4,"loop":true,"minDistance":2}},
       {"id":3,"type":"AudioListener","properties":{}}]}]}"#;
    let hall = platypus_scenes::load_scene(data, false).expect("loads");
    assert!(platypus_scenes::take_warnings().is_empty());
    let o = ops();
    assert_eq!(o[0], "load sfx/hit.wav false");
    assert_eq!(o[1], "emitter 1 [3.0,0.0,0.0] [2.0,50.0] null");
    assert_eq!(o[2], "play 1 1 1 [0.4,1.0,0.0,true,0.0,0.0]");
    assert_eq!(o[3], "listener [3.0,0.0,0.0] [0.0,0.0,0.0,1.0]");
    let saved: serde_json::Value =
        serde_json::from_str(&platypus_scenes::serialize(hall).unwrap()).unwrap();
    let comps = &saved["objects"][0]["components"];
    let src = comps
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "AudioSource")
        .unwrap();
    assert_eq!(
        src["properties"]["clip"],
        serde_json::json!({"$type": "Sound", "src": "sfx/hit.wav"})
    );
    assert_eq!(src["properties"]["volume"], 0.4);
    assert_eq!(src["properties"]["loop"], true);
    assert!(
        comps
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["type"] == "AudioListener")
    );
    platypus_scenes::reset();
    audio::reset();
}
