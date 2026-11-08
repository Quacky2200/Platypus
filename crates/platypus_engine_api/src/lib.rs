// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The engine's script API beyond the scene, declared with `#[ts_api]`: `Color`, `Time`, `input`,
//! `Gizmos`, `Camera`, `Screen`, `QualitySettings` and `Window`. Each module keeps its state per thread and
//! has plain Rust functions for the host (the app) to feed or read; compiled scripts only see the
//! generated API. The names and behaviour follow Unity and s&box.
//!
//! Wiring this into `platypus_app` is the app's job: call [`time::advance`] and the `input` event
//! functions each frame, run scripts, then read [`gizmos::take_frame`] and [`camera::cameras`].
//! Design: `docs/research/platypus-ts-engine-boundary.md` and `docs/platypus-api.md`.

pub use platypus_math;

pub mod animation;
pub mod animator;
pub mod app;
pub mod assets;
pub mod audio;
pub mod audio_components;
pub mod camera;
pub mod clock;
pub mod color;
pub mod cursor;
pub mod datetime;
pub mod editor;
pub mod event_list;
pub mod gizmos;
pub mod input;
pub mod layers;
pub mod log;
pub mod network;
pub mod objects;
pub mod prefs;
pub mod random;
pub mod screen;
pub mod spline;
pub mod time;
pub mod ui_inspector;
pub mod ui_resources;
pub mod ui_scale;
pub mod window;

/// The manifests (JSON) of every namespace this crate declares, for the compiler to check calls
/// against. The scene's (`platypus_scene::api::scene_api`) is separate.
pub fn manifests() -> Vec<String> {
    let mut all = platypus_math::manifests();
    all.extend([
        animation::keyframe_api::MANIFEST.to_json(),
        animation::animation_curve_api::MANIFEST.to_json(),
        animation::animation_clip_api::MANIFEST.to_json(),
        animation::animation_component_api::MANIFEST.to_json(),
        animation::animation_state_api::MANIFEST.to_json(),
        animation::animations_api::MANIFEST.to_json(),
        animator::animation_parameter_api::MANIFEST.to_json(),
        animator::animation_condition_api::MANIFEST.to_json(),
        animator::animation_transition_api::MANIFEST.to_json(),
        animator::animation_blend_tree_api::MANIFEST.to_json(),
        animator::animation_node_api::MANIFEST.to_json(),
        animator::animation_controller_api::MANIFEST.to_json(),
        animator::animation_builder_api::MANIFEST.to_json(),
        animator::animator_api::MANIFEST.to_json(),
        color::color_api::MANIFEST.to_json(),
        cursor::cursor_api::MANIFEST.to_json(),
        cursor::cursor_lock_mode_api::MANIFEST.to_json(),
        datetime::time_span_api::MANIFEST.to_json(),
        datetime::date_time_api::MANIFEST.to_json(),
        time::time_api::MANIFEST.to_json(),
        clock::time_since_api::MANIFEST.to_json(),
        clock::time_until_api::MANIFEST.to_json(),
        input::input_api::MANIFEST.to_json(),
        gizmos::gizmos_api::MANIFEST.to_json(),
        camera::camera_api::MANIFEST.to_json(),
        screen::screen_api::MANIFEST.to_json(),
        screen::quality_api::MANIFEST.to_json(),
        audio::audio_api::MANIFEST.to_json(),
        audio::microphone_api::MANIFEST.to_json(),
        audio_components::audio_source_api::MANIFEST.to_json(),
        audio_components::audio_listener_api::MANIFEST.to_json(),
        random::rng_api::MANIFEST.to_json(),
        random::random_api::MANIFEST.to_json(),
        layers::layer_mask_api::MANIFEST.to_json(),
        objects::hide_flags_api::MANIFEST.to_json(),
        log::debug_api::MANIFEST.to_json(),
        log::log_api::MANIFEST.to_json(),
        log::console_api::MANIFEST.to_json(),
        network::network_api::MANIFEST.to_json(),
        prefs::prefs_api::MANIFEST.to_json(),
        assets::assets_api::MANIFEST.to_json(),
        app::application_api::MANIFEST.to_json(),
        app::system_info_api::MANIFEST.to_json(),
        ui_inspector::ui_inspector_api::MANIFEST.to_json(),
        ui_resources::ui_resources_api::MANIFEST.to_json(),
        ui_scale::ui_scale_api::MANIFEST.to_json(),
        window::window_api::MANIFEST.to_json(),
        editor::editor_api::MANIFEST.to_json(),
        spline::spline_api::MANIFEST.to_json(),
        event_list::event_list_api::MANIFEST.to_json(),
        spline::spline_animate_api::MANIFEST.to_json(),
    ]);
    all
}
