// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Fog, light shafts, exposure, `RenderStats` and `GraphicsSettings` in `platypus_render_api`
//! (`RenderSettings` is per thread: every test runs on its own).
use platypus_render_api::render_settings_api as rs;
use ts_api::Handle;

// M9-API-1502
#[test]
fn fog_starts_off_and_its_extras_turn_on_with_defaults() {
    platypus_render_api::reset();
    let s = platypus_render_api::settings();
    assert!(!s.fog.enabled);
    assert!(s.fog.height.is_none() && s.fog.volumetric.is_none());
    assert_eq!(s.fog.mode, "exponentialSquared");
    rs::set_fog(true);
    rs::set_fog_mode(Some(platypus_render_api::FogKind::Linear));
    // A name that is not one (a compiled call passes -1) changes nothing.
    rs::set_fog_mode(rs::fog_mode_parse("nonsense"));
    rs::set_fog_end_distance(70.0);
    // A property turns its feature on with the defaults for the rest.
    rs::set_volumetric_fog_steps(8.0);
    rs::set_height_fog_base_height(2.0);
    let s = platypus_render_api::settings();
    assert_eq!(s.fog.mode, "linear", "an unknown mode is ignored");
    assert_eq!(s.fog.end, 70.0);
    let v = s.fog.volumetric.expect("light shafts");
    assert_eq!((v.steps, v.intensity, v.anisotropy), (8.0, 1.0, 0.4));
    assert_eq!(s.fog.height.expect("height fog").base_height, 2.0);
    rs::set_volumetric_fog_enabled(false);
    rs::set_height_fog_enabled(false);
    let s = platypus_render_api::settings();
    assert!(s.fog.volumetric.is_none() && s.fog.height.is_none());
}

// M9-API-1503
#[test]
fn exposure_settings_freeze_to_the_main_cameras_exposure() {
    use platypus_engine_api::camera::{CameraRef, set_exposure};
    use platypus_scene::ObjectId;
    platypus_render_api::reset();
    platypus_engine_api::camera::reset();
    rs::set_auto_exposure_compensation(1.5);
    rs::add_auto_exposure_curve_point(-2.0, -1.0);
    rs::set_exposure_transition(3.0);
    let s = platypus_render_api::settings();
    let a = s.auto_exposure.expect("auto exposure");
    assert_eq!((a.compensation, a.min_ev, a.curve.len()), (1.5, -10.0, 1));
    assert!(s.fixed_exposure.is_none());
    let cam = CameraRef::attach(ObjectId::from_raw(1));
    assert!(cam.exposure().is_nan(), "not read back yet");
    // Frozen before the first read-back: nothing to freeze to, auto exposure still goes off.
    rs::freeze_exposure();
    assert!(platypus_render_api::settings().fixed_exposure.is_none());
    rs::set_auto_exposure_enabled(true);
    set_exposure(cam, 9.5, 8.0);
    assert_eq!((cam.exposure(), cam.scene_brightness()), (9.5, 8.0));
    rs::freeze_exposure();
    let s = platypus_render_api::settings();
    assert_eq!(s.fixed_exposure, Some(9.5));
    assert!(s.auto_exposure.is_none());
    assert_eq!(s.exposure_transition, 3.0);
}

// M9-API-1504
#[test]
fn a_physical_cameras_exposure_follows_hdrps_formula() {
    use platypus_engine_api::camera::CameraRef;
    use platypus_scene::ObjectId;
    platypus_engine_api::camera::reset();
    let cam = CameraRef::attach(ObjectId::from_raw(1));
    // f/16, 1/200 s, ISO 200: log2(256 * 200) - log2(2) = 14.64.
    assert!((cam.physical_exposure() - 14.643_856).abs() < 1e-4);
    cam.set_aperture(1.0);
    cam.set_shutter_speed(1.0);
    cam.set_iso(100.0);
    assert_eq!(cam.physical_exposure(), 0.0);
    assert!(cam.fog(), "cameras show the fog by default");
    cam.set_fog(false);
    assert!(!cam.fog());
    cam.reset_exposure();
    platypus_engine_api::camera::with_camera(cam, |d| assert_eq!(d.exposure_resets, 1.0));
}

// M9-API-1505
#[test]
fn render_stats_and_light_units_are_set_by_the_host() {
    platypus_render_api::reset();
    platypus_render_api::set_render_stats(platypus_render_api::Stats {
        meshes: 12.0,
        fog_culled: 3.0,
        shadow_casters: 7.0,
        ..Default::default()
    });
    assert_eq!(platypus_render_api::render_stats_api::meshes(), 12.0);
    assert_eq!(platypus_render_api::render_stats_api::fog_culled(), 3.0);
    assert_eq!(platypus_render_api::render_stats_api::shadow_casters(), 7.0);
    assert_eq!(
        platypus_render_api::graphics_settings_api::light_units(),
        "legacy"
    );
    platypus_render_api::set_physical_light_units(true);
    assert_eq!(
        platypus_render_api::graphics_settings_api::light_units(),
        "physical"
    );
    platypus_render_api::set_physical_light_units(false);
}

// M9-API-1506
#[test]
fn lights_and_fog_volumes_have_the_light_shaft_properties() {
    use platypus_render_api::{FogVolumeRef, LightRef};
    use platypus_scene::ObjectId;
    platypus_render_api::reset();
    let o = ObjectId::from_raw(7);
    let light = LightRef::attach(o);
    assert_eq!(
        (
            light.volumetric_intensity(),
            light.spot_shape().as_str(),
            light.aspect_ratio()
        ),
        (1.0, "cone", 1.0)
    );
    light.set_spot_shape("pyramid");
    light.set_spot_shape("square");
    assert_eq!(light.spot_shape(), "pyramid", "an unknown shape is ignored");
    assert_eq!(LightRef::count(), 1.0);
    assert_eq!(LightRef::at(0.0), light);
    assert_eq!(LightRef::at(5.0), LightRef(0));
    let fog = FogVolumeRef::attach(o);
    assert_eq!(
        (fog.density(), fog.shape().as_str(), fog.blend_distance()),
        (0.5, "box", 1.0)
    );
    fog.set_shape("ellipsoid");
    assert_eq!(platypus_render_api::fog_volumes()[0].1.shape, "ellipsoid");
    // Scene files keep them.
    let saved = platypus_render_api::save_light(light);
    assert_eq!(saved["spotShape"], "pyramid");
    assert_eq!(
        platypus_render_api::save_fog_volume(fog)["blendDistance"],
        1.0
    );
}
