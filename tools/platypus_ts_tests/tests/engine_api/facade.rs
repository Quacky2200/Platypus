// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The TypeScript `@platypus/engine` shapes over the native API (`bench/facade.ts`, prefix `fac_`),
//! in the normal, stress GC and collection-off modes. IDs are in TESTS.md (M9-TS-1700..).
use platypus_scene::Scene;
use platypus_ts_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use platypus_ts_tests::facade_t::*;

fn reset() {
    platypus_render_api::reset();
    platypus_engine_api::camera::reset();
    platypus_engine_api::assets::reset();
    platypus_scenes::reset();
    platypus_scenes::take_warnings();
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    let run = || {
        reset();
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        unsafe { ts_fac_init() };
        f();
    };
    run();
    ts_rt_gc_stress(true);
    run();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    run();
    ts_rt_gc_enable(true);
    reset();
}

fn text(p: *mut u8) -> String {
    assert!(!p.is_null());
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

// M9-TS-1700
#[test]
fn object_valued_render_settings_split_into_the_field_setters() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_skyAndBloom()),
            "true|1|2|true|false|false|true|1.2|0.55|false|false"
        );
    });
}

// M9-TS-1701
#[test]
fn game_object_options_are_local_to_the_parent_and_null_is_no_parent() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_gameObjectOptions()),
            "true|true|3|false|1|1|4|3"
        );
    });
}

// M9-TS-1702
#[test]
fn camera_rect_cull_distances_main_camera_and_screen_getters() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_cameraAndScreen()),
            "0.25|0.25|14|0|true|60|true|true"
        );
    });
}

// M9-TS-1703
#[test]
fn positional_constructors_with_texture_and_create_primitive_options() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_constructors()),
            "1|0.25|true|4|1|Ball|2|2|3|0.5|0.8|0.5"
        );
    });
}

// M9-TS-1704
#[test]
fn lod_levels_cloth_options_and_root_objects_go_through_the_element_wise_api() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_structured()), "2|1|0|true|true");
    });
}

// M9-TS-1705, M9-TS-3355
#[test]
fn the_animator_controller_builder_takes_object_options() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_animatorController()),
            "3|2|2|Locomotion|2|2|1|0.95|1"
        );
    });
}

// M9-TS-1706
#[test]
fn sprite_create_with_a_rect_and_sprite_grid() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_sprites()), "40|true|2|10|false|25|50|false");
    });
}

// M9-TS-1707
#[test]
fn style_sheets_fonts_and_gamepad_wrappers_go_through_the_ui_resources_queue() {
    in_every_gc_mode(|| unsafe {
        platypus_engine_api::ui_resources::reset();
        assert_eq!(text(ts_fac_uiResources()), "1|0");
        let r = platypus_engine_api::ui_resources::take_requests();
        assert_eq!(r.len(), 4);
        assert!(matches!(
            r[1],
            platypus_engine_api::ui_resources::UiRequest::LoadFont { weight, .. } if weight == 700.0
        ));
        // A font object held in a variable, with the weight and style left out: the font's own.
        assert!(matches!(
            &r[3],
            platypus_engine_api::ui_resources::UiRequest::LoadFont { family, weight, style, .. }
                if family == "Plain" && *weight == 0.0 && style.is_empty()
        ));
    });
}

// M9-TS-1708
#[test]
fn particle_ranges_bursts_shape_keys_and_point_lists_are_options() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_particlesAndPoints()),
            "1.2|1.8|3|2|cone|14|3|2|true|2|5|9|9|1"
        );
    });
}

// M9-TS-2000
#[test]
fn curve_from_point_pairs_and_a_pairs_literal() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_curveFromPairs()), "5|3|2");
    });
}

// M9-TS-2001
#[test]
fn subscriptions_return_an_unsubscribe_function_and_modes_index_a_record() {
    in_every_gc_mode(|| unsafe {
        let s = text(ts_fac_unsubscribers());
        assert!(s.starts_with("function|true|true|"), "{s}");
    });
}

// M9-TS-2002
#[test]
fn a_callback_returning_nothing_then_a_function_is_nullable() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_bareReturnThenCleanup()), "true|true|7");
    });
}

// M9-TS-2003
#[test]
fn array_literals_of_mixed_types_are_tuples() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_tuples()), "0:-2.5:4:F,1:2.5:8:T,8|three");
    });
}

// M9-TS-2004
#[test]
fn closure_parameters_with_defaults_may_be_left_out() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_defaultedClosureParams()), "1a0.75t|2b2t|3c3z");
    });
}

// M9-TS-2005
#[test]
fn void_operator_evaluates_and_yields_nothing() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_voidAndExtraArguments()), "2|2");
    });
}

// M9-TS-2006
#[test]
fn a_record_is_indexed_by_a_string_from_the_engine() {
    in_every_gc_mode(|| unsafe {
        assert!(!text(ts_fac_recordByEngineString()).is_empty());
    });
}

// M9-TS-2007
#[test]
fn plain_xy_objects_go_where_value_vectors_are_expected() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_plainPointsToValues()), "4|2");
    });
}

// M9-TS-2008
#[test]
fn for_of_over_an_array_of_unrelated_types_runs_the_body_for_each() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_unrolledMixedLoop()), "3|a2|2");
    });
}

// M9-TS-2010
#[test]
fn a_generic_arrow_over_the_keys_of_a_type_is_written_per_key() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_keyedSetter()), "b|5");
    });
}

// M9-TS-2880
#[test]
fn fog_height_fog_light_shafts_and_exposure_objects_become_field_setters() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_fogAndExposureObjects()),
            "true|3|0.5|true|2|0.4|0|ultra|true|-3|20|9|null|false|false|false|false"
        );
    });
}

// M9-TS-2881
#[test]
fn flat_meshes_take_typed_arrays_and_random_pick_chooses_an_item() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_flatMeshesAndPicks()), "3|1|4|2|true|true");
    });
}

// M9-TS-3359
#[test]
fn random_pick_is_a_built_in_over_lists_of_any_type() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_picksOfEveryKind()), "true|true|7|true|true");
    });
}

// M9-TS-2882
#[test]
fn the_light_meter_sums_what_each_light_sheds_on_a_point() {
    in_every_gc_mode(|| unsafe {
        // The sun 3 lux (no normal: head-on) plus the lamp: 16 pi lumens over 4 pi sr = 4 cd, 1 lux at 2 m.
        assert_eq!(text(ts_fac_lightMeter()), "2|1|cone|4|1");
    });
}

// M9-TS-2883
#[test]
fn an_optional_rotation_passed_on_is_left_alone_and_an_optional_bool_compares_with_false() {
    in_every_gc_mode(|| unsafe {
        // `a` keeps the identity rotation (w = 1), `b` is turned 90 degrees (w = cos 45 degrees).
        assert_eq!(
            text(ts_fac_optionalValuesPassedOn()),
            "2|100|71|lit|dim|lit"
        );
    });
}

// M9-TS-2884
#[test]
fn bloom_settings_from_a_variable_go_through_the_facade_write() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_bloomFromAVariable()), "true|1.5|0.6|false");
    });
}

// M9-TS-2885
#[test]
fn shapes_make_gdi_style_outlines() {
    in_every_gc_mode(|| unsafe {
        // rectangle: 4 corners, the third at (4, 6); circle of 8: the third point on top;
        // arc of 2 segments: 3 points; pie adds the centre; rounded corners: 4 x (4 + 1);
        // an open curve: 2 spans x 4 + the end; a closed one: 3 spans x 4.
        assert_eq!(text(ts_fac_shapesAndSplines()), "4|4|6|8|2|3|1|4|3|20|9|12");
    });
}

// M9-TS-2886
#[test]
fn a_spline_goes_by_distance_finds_the_nearest_point_and_edits_its_knots() {
    in_every_gc_mode(|| unsafe {
        // The edited spline is longer than 8; the middle of a straight 6 by distance is 3; the
        // nearest point to (1.5, 2) is a quarter of the way (t 0.25), 2 away; two segments
        // after inserting a knot and removing the first; a closed square has 4 segments and
        // (smoothed at its corners) length 17; the knot that is first now is the linear one
        // that was inserted; the heading leaves +x a little; 4 points per segment closed; a
        // straight 2-knot spline samples 2 plus the end.
        assert_eq!(
            text(ts_fac_splineMath()),
            "true|3|0.25|2|2|4|17|true|linear|0.89|16|3"
        );
    });
}

// M9-TS-2888
#[test]
fn line_renderer_fill_color_reads_and_writes_null_and_text_mesh_takes_options() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_fillColorAndTextMesh()),
            "true|1|true|false|3|x|20|left"
        );
    });
}

// M9-TS-2889
#[test]
fn screen_to_world_point_matches_the_camera_frustum() {
    in_every_gc_mode(|| unsafe {
        platypus_engine_api::screen::update(800, 600, 1.0);
        // Orthographic (5 tall, 800 x 600): the middle, and the top left (-6.67, 5);
        // perspective 60 degrees: the bottom right 10 in front is (7.7, -5.77).
        assert_eq!(text(ts_fac_screenToWorld()), "0|0|0|-6.67|5|0|7.7|-5.77|0");
    });
}

// M9-TS-3127
#[test]
fn partial_engine_classes_are_option_bags_that_spread_into_options() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_optionBags()),
            "0.05|false|0.5|true|2|true|0.25|7|0.25|true|7|40|t|24"
        );
    });
}

// M9-TS-3135
#[test]
fn static_engine_functions_are_values() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_engineStaticsAsValues()), "1|0|2|1|22|4,4");
    });
}

// M9-TS-3138
#[test]
fn options_chosen_by_a_conditional_expression() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_conditionalOptions()), "true|true|4|2");
    });
}

// M9-TS-3340
#[test]
fn enums_take_names_and_unknown_ones_change_nothing_nullables_read_null_and_aliases_name_the_class()
{
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_enumsAndNullables()),
            "linear|exponential|exponential|ultra|point|mirror|trilinear|bilinear|repeat|4|1|Windowed|true|Cube|Capsule|4.5|null|true|1|true"
        );
    });
}

// M9-TS-3341
#[test]
fn result_records_and_lists_of_records_come_back_as_plain_objects_with_strings() {
    in_every_gc_mode(|| unsafe {
        use platypus_engine_api::screen::{Resolution, set_display};
        let r = |width, height, refresh_rate| Resolution {
            width,
            height,
            refresh_rate,
        };
        set_display(
            "Windowed",
            (1024, 768),
            vec![r(1920, 1080, 144.0), r(1280, 720, 60.0)],
            r(1920, 1080, 144.0),
            vec![],
        );
        assert_eq!(text(ts_fac_resultRecords()), "1024x768|144|2|1280|true|0");
    });
}

// M9-TS-3343
#[test]
fn record_properties_take_object_literals_keep_the_keys_they_write_and_read_back() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_recordProperties()),
            "0.2|3|0.7|true|true|1|true|false|-4|2|spot|20|true"
        );
    });
}

// M9-TS-3350
//
// Lists of handles, numbers and values as properties of the engine API: `bones`, the cloth's
// colliders and pins, `layerCullDistances`, `getRootGameObjects()`, `QualitySettings.names` and a
// `Vector2Like[]` setter. A setter replaces the list.
#[test]
fn handle_and_number_lists_are_native_properties() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_nativeLists()),
            "2|true|1|2|true|3|true|3|true|32|2|0|true|true|3"
        );
    });
}

// M9-TS-3351
//
// A listener function returns the function that stops it (`Screen.onChange`,
// `QualitySettings.onLevelChanged`, `SceneManager.onSceneLoaded`, ...): written by the compiler over
// the token-returning native and its stopping function; stopping twice is harmless.
#[test]
fn listeners_return_an_unsubscribe_function() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_unsubscribes()), "0|2|0");
    });
}

// M9-TS-3352
//
// Lists of records (some holding handles) as parameters and properties: `LODGroup.setLODs`,
// `ParticleSystem.bursts` / `colorOverLifetime` / `sizeOverLifetime` / `shape`. They travel as JSON
// text (an engine handle is its id); keys left out stay out (`cycles` defaults to 1).
#[test]
fn record_lists_with_handles_and_optional_keys_cross_as_json() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_recordLists()), "2|2|1|2|2|1|0.5|1|0.5|box|2|25");
    });
}

// M9-TS-3353
//
// `Camera.screenToWorldPoint`, `horizontalFieldOfView` (read and write) and `rect` are native
// maths: an 800x600 window, a 90 degree camera at (1, 2, 3) looking down -z (setting the horizontal 90 gives a vertical 74).
#[test]
fn camera_screen_maths_is_native() {
    in_every_gc_mode(|| unsafe {
        platypus_engine_api::screen::set_size(800, 600);
        // mid: (1, 2, -2); corner: x = 1 - 5 * 4/3, y = 2 + 5, z = -2; horizontal fov of a 4:3 window.
        assert_eq!(
            text(ts_fac_cameraMaths()),
            "1000|2000|-2000|-5667|7000|106|74|100"
        );
    });
}

// M9-TS-3354
//
// `Mesh.create` takes one object (a variable here) whose keys are the parameters of the native
// function: bone influences become four indices and weights per vertex (a short list is padded
// with zeros), bind poses their 16 numbers column by column.
#[test]
fn a_mesh_is_made_from_an_object_with_bones_and_bind_poses() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_skinnedMesh()), "3|1");
        let skinned = (1..16)
            .filter_map(|i| platypus_render_api::mesh_info(platypus_render_api::MeshRef(i)))
            .find(|m| !m.joints.is_empty())
            .expect("a skinned mesh");
        assert_eq!(
            skinned.joints,
            vec![0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            skinned.weights,
            vec![0.75, 0.25, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(skinned.bindposes.len(), 32);
        assert_eq!(skinned.bindposes[28..31], [0.0, -1.0, 0.0]);
    });
}

// M9-TS-3357
//
// `Scene.serialize()` is a record (a name and its root objects, a record with a list of records
// that comes back as JSON text); `toJson()` is the whole scene as text.
#[test]
fn a_scene_serializes_to_a_record_and_to_text() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_fac_sceneSummary()), "2|alpha,beta,|true|true");
    });
}

// M9-TS-3388
//
// `UiScale` from compiled code sets the mode the host scales screen UI by; a zero factor is ignored.
#[test]
fn ui_scale_modes_reach_the_host() {
    use platypus_engine_api::ui_scale::{UiScale, current, reset};
    in_every_gc_mode(|| unsafe {
        reset();
        assert_eq!(text(ts_fac_uiScale()), "set");
        assert_eq!(current(), UiScale::Fixed(1.5));
        assert_eq!(current().factor(1280.0, 720.0), 1.5);
        reset();
    });
}

// M3-INS-07
//
// `UiInspector` from compiled code reads the snapshot the host published and queues the edit and
// the selection; with the inspector off its calls answer nothing and drop the edit.
#[test]
fn ui_inspector_reads_the_snapshot_and_queues_edits() {
    use platypus_engine_api::ui_inspector as ins;
    in_every_gc_mode(|| unsafe {
        ins::reset();
        assert_eq!(
            text(ts_fac_uiInspector()),
            "false|0||0||false|",
            "off: nothing"
        );
        assert!(ins::take_requests().is_empty());

        ins::enable();
        ins::set_hovered(7);
        ins::publish(vec![ins::NodeSnapshot {
            id: 7,
            kind: "box".into(),
            children: vec![8, 9],
            inline: vec![("width".into(), "100".into(), true)],
            rules: vec![ins::RuleSnapshot {
                sheet: 1,
                selector: ".card".into(),
                declarations: vec![ins::DeclSnapshot {
                    prop: "height".into(),
                    value: "40px".into(),
                    important: false,
                    applied: true,
                }],
            }],
            ..Default::default()
        }]);
        assert_eq!(text(ts_fac_uiInspector()), "true|7|box|9|.card|true|100");
        assert_eq!(ins::watched(), [7]);
        let edits = ins::take_requests();
        assert_eq!((edits[0].id, edits[0].prop.as_str()), (7, "width"));
        ins::reset();
    });
}

// M9-TS-3384, M9-TS-3385
//
// What `examples/fog-white-room` needed: a plain `{ x, y }` for a Vector2 key of an options
// object, a `bool` parameter's default given as `true`, `find` and `reduce` (no initial value)
// over engine handles, `Animation.destroy()`, and a model's `materials` / `animations` lists
// (empty before it loads).
#[test]
fn white_room_shapes_options_finds_over_handles_and_model_lists() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_fac_whiteRoomShapes()),
            "4|2|1|B|true|C|true|true|true|0"
        );
    });
}
