// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The engine API crate's host functions. IDs are in TESTS.md (M9-API-*).
use platypus_engine_api::{camera, screen};
use platypus_scene::ObjectId;

// M9-API-914
//
// The levels start with vsync on; an older save (vsync off then, no `playerVSync`) does not turn it
// off, and a player's choice (`playerVSync: true`) is kept.
#[test]
fn vsync_is_on_by_default_and_only_a_players_saved_choice_turns_it_off() {
    use platypus_engine_api::prefs;
    screen::reset();
    assert_eq!(screen::quality().v_sync_count, 1);
    prefs::set_json(
        "platypus.quality",
        r#"{"level":"High","current":{"renderScale":0.5,"vSyncCount":0}}"#,
    );
    screen::apply_default_quality();
    let q = screen::quality();
    assert_eq!(
        q.render_scale, 0.5,
        "the rest of the saved choice still applies"
    );
    assert_eq!(q.v_sync_count, 1);
    screen::reset();
    prefs::set_json(
        "platypus.quality",
        r#"{"level":"Low","current":{"vSyncCount":0},"playerVSync":true}"#,
    );
    screen::apply_default_quality();
    let q = screen::quality();
    assert_eq!((q.name.as_str(), q.v_sync_count), ("Low", 0));
    prefs::reset();
}

// M9-API-35
#[test]
fn a_camera_has_the_engines_defaults_and_clamps_the_field_of_view() {
    camera::reset();
    let go = ObjectId {
        index: 3,
        generation: 0,
    };
    let c = camera::camera_api::platypus_Camera_attach(ts_api::Handle::to_raw(go));
    assert_eq!(camera::camera_api::platypus_Camera_fieldOfView(c), 60.0);
    assert!(!camera::camera_api::platypus_Camera_orthographic(c));
    camera::camera_api::platypus_Camera_setFieldOfView(c, 500.0);
    assert_eq!(camera::camera_api::platypus_Camera_fieldOfView(c), 179.0);
    camera::camera_api::platypus_Camera_setOrthographicSize(c, 7.5);
    camera::camera_api::platypus_Camera_setOrthographic(c, true);
    let all = camera::cameras();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].1.orthographic_size, 7.5);
    assert_eq!(all[0].1.object, go);
    camera::remove_on(go);
    assert!(camera::cameras().is_empty());
    // A gone camera answers with defaults.
    assert_eq!(camera::camera_api::platypus_Camera_fieldOfView(c), 60.0);
}
