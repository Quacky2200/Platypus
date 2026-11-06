// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The native rendering data API from compiled programs (`bench/render_native.ts`) and its
//! scene-file kinds, in the normal, stress GC and collection-off modes. IDs are in TESTS.md
//! (M9-TS-1220..).
use platypus_render_api as render;
use platypus_scene::Scene;
use platypus_ts_tests::render_native_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn reset() {
    render::reset();
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
        unsafe { ts_rdn_init() };
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

// M9-TS-1220
#[test]
fn a_light_keeps_its_settings_and_rejects_unknown_names() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_rdn_light()), "spot|2.5|0.5|40|soft|5|20|true");
        let lights = render::lights();
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].1.r#type, "spot");
    });
}

// M9-TS-1221
#[test]
fn mesh_renderers_take_primitives_by_name_script_meshes_and_materials() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_rdn_meshes()), "sphere|3|1|cutout|off|cube|0.5|0.25");
        let renderers = render::mesh_renderers();
        assert_eq!(renderers.len(), 3);
        // The material of the first renderer is the one made by the script.
        let m = render::with_material(render::MaterialRef(renderers[0].1.material), |d| d.color.r);
        assert_eq!(m, Some(1.0));
        let cmds = render::take_commands();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, render::RenderCommand::Mesh(_)))
        );
    });
}

// M9-TS-1222
#[test]
fn textures_come_from_files_pixels_and_render_targets_and_cameras_use_them() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_rdn_textures()),
            "point|4|false|2x1|true|64|64|skybox|3|0.5|30|img/a.png"
        );
        let a = render::texture_info(render::TextureRef(1)).unwrap();
        assert_eq!(a.src.as_deref(), Some("img/a.png"));
        assert!(!a.loaded);
        render::texture_loaded(render::TextureRef(1), 8, 4);
        let a = render::texture_info(render::TextureRef(1)).unwrap();
        assert!(a.loaded && a.width == 8);
        // Pixels: sRGB bytes, the top row first; 0.5 alpha is 128.
        let b = render::texture_info(render::TextureRef(2)).unwrap();
        assert_eq!(b.pixels.unwrap(), [255, 0, 0, 255, 0, 0, 255, 128]);
        let cams = platypus_engine_api::camera::cameras();
        assert_eq!(cams[0].1.target_texture.0, 3);
        assert_eq!(cams[0].1.rect, [0.5, 0.0, 0.5, 1.0]);
        assert_eq!(cams[0].1.layer_cull_distances[4], 30.0);
    });
}

// M9-TS-1223
#[test]
fn sprites_have_a_size_from_their_texture_and_a_renderer() {
    in_every_gc_mode(|| unsafe {
        // A 4x4 texture at 50 pixels per unit is 0.08 units wide; a quarter cell at 100 is 0.02.
        assert_eq!(text(ts_rdn_sprites()), "0.08|0.02|true|3|true");
    });
}

// M9-TS-1224
#[test]
fn lod_levels_must_go_down_and_hold_renderers() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_rdn_lods()), "2|0.2|1|true");
        assert_eq!(render::lod_groups()[0].1.forced, 1);
    });
}

// M9-TS-1225
#[test]
fn mirrors_probes_planes_panels_and_strokes_hold_their_data() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_rdn_others()), "6|256|everyFrame|4|0.8|2|1");
        assert_eq!(render::reflection_probes()[0].1.render_requests, 2.0);
    });
}

// M9-TS-1228
#[test]
fn render_settings_and_the_cursor_hold_their_state_and_ignore_unknown_names() {
    in_every_gc_mode(|| unsafe {
        platypus_engine_api::cursor::reset();
        assert_eq!(
            text(ts_rdn_settings()),
            "0.5|true|3|2|true|aces|true|skybox"
        );
        let s = render::settings();
        assert_eq!(s.skybox.unwrap().sun_size, 3.0);
        assert_eq!(s.bloom.unwrap().intensity, 2.0);
        assert!(render::version() > 0);
        assert_eq!(text(ts_rdn_cursor()), "false|locked");
        let c = platypus_engine_api::cursor::state();
        assert_eq!(c.image, Some(("ui/hand.png".to_string(), Some([4.0, 5.0]))));
    });
}

// M9-TS-1229
#[test]
fn camera_follow_and_orbit_move_their_objects_when_the_host_runs_them() {
    in_every_gc_mode(|| unsafe {
        ts_rdn_behaviours();
        render::late_update_camera_follows(0.016);
        render::late_update_camera_orbits(1.0);
        let pos = |name: &str| {
            platypus_scene::api::with_scene(|sc| {
                let o = sc.find(name).unwrap();
                sc.world(o).position
            })
            .unwrap()
        };
        // Smooth time 0: it sits at the target plus the offset.
        let f = pos("Follower");
        assert_eq!((f.x, f.y, f.z), (10.0, 2.0, 5.0));
        // yaw 90 + 90 degrees a second for one second = 180: behind the centre at -z (`Vector3.back` is +z: yaw 0 sits at +z).
        let o = pos("Orbiter");
        assert!(
            (o.x).abs() < 1e-4 && (o.z + 4.0).abs() < 1e-4 && o.y.abs() < 1e-4,
            "{o:?}"
        );
    });
}

// M9-TS-1230
//
// `renderSettings` of a scene file applies when the scene becomes active, and the active scene
// saves the current settings.
#[test]
fn scene_files_apply_and_save_render_settings() {
    let _serial = platypus_ts_tests::serial();
    reset();
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let text = r#"{"format":"platypus.scene","version":1,"name":"Sky","objects":[],
      "renderSettings":{"ambientLight":{"$type":"Color","value":[0.1,0.2,0.3,1]},"ambientMode":"color",
        "reflectionIntensity":0.5,
        "skybox":{"zenith":{"$type":"Color","value":[0,0,1,1]},"sunDirection":null},
        "bloom":{"threshold":0.8,"intensity":1.5,"scatter":0.3}}}"#;
    let handle = platypus_scenes::load_scene(text, false).expect("loads");
    let s = render::settings();
    assert_eq!(s.ambient_mode, "color");
    assert_eq!(s.ambient_light.b, 0.3);
    assert_eq!(s.reflection_intensity, 0.5);
    let sky = s.skybox.unwrap();
    assert_eq!((sky.zenith.b, sky.sun_direction), (1.0, None));
    assert_eq!(s.bloom.unwrap().threshold, 0.8);
    let saved: serde_json::Value =
        serde_json::from_str(&platypus_scenes::serialize(handle).unwrap()).unwrap();
    assert_eq!(saved["renderSettings"]["ambientMode"], "color");
    assert_eq!(saved["renderSettings"]["bloom"]["intensity"], 1.5);
    assert!(saved["renderSettings"]["skybox"]["sunDirection"].is_null());
    reset();
}

// M9-TS-1231
#[test]
fn particle_systems_hold_their_settings_and_report_back_to_scripts() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_rdn_particles()), "1|2|1|false|true|false|true|1");
        let (r, d) = render::particle_systems().remove(0);
        assert_eq!(d.shape_type, "sphere");
        assert_eq!(d.blend_mode, "additive");
        assert_eq!(d.texture_sheet, Some([4.0, 4.0, 1.0]));
        assert_eq!(d.size_over_lifetime.as_ref().unwrap().len(), 2);
        let controls: Vec<(String, f64)> = render::take_particle_controls()
            .into_iter()
            .map(|(_, a, n)| (a, n))
            .collect();
        assert_eq!(
            controls,
            [
                ("play".to_string(), 0.0),
                ("emit".to_string(), 5.0),
                ("pause".to_string(), 0.0)
            ]
        );
        // The host reports: alive, then gone. `onStopped` runs and `destroy` destroys the object.
        render::report_particles(r, 12, true);
        assert_eq!(ts_rdn_stoppedCount(), 0.0);
        render::report_particles(r, 0, false);
        assert_eq!(ts_rdn_stoppedCount(), 1.0);
        assert!(platypus_scene::api::take_destroyed().contains(&d.object));
    });
}

// M9-TS-1232
#[test]
fn a_line_renderer_draws_a_tube_through_a_mesh_renderer_of_its_own() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_rdn_lines()), "3|1");
        let (_, line) = render::line_renderers().remove(0);
        let renderers = render::mesh_renderers_on(line.object);
        assert_eq!(renderers.len(), 1);
        let mesh = render::with_mesh_renderer(renderers[0], |d| d.mesh).unwrap();
        let info = render::mesh_info(render::MeshRef(mesh)).unwrap();
        // 2 segments x (1 + 1) points + the end = 5 rings of 7 vertices.
        assert_eq!(info.vertices.len(), 5 * 7 * 3);
        assert_eq!(info.triangles.len(), 4 * 6 * 6);
        // Moving a point rebuilds it on the next pre-render.
        let before = info.vertices.clone();
        let l = render::line_renderers().remove(0).0;
        render::with_line_renderer(l, |d| d.positions[2].y = 3.0);
        render::pre_render_lines();
        let after = render::mesh_info(render::MeshRef(mesh)).unwrap();
        assert_ne!(after.vertices, before);
    });
}

// M9-TS-1233
#[test]
fn cloth_swaps_the_renderers_mesh_for_a_simulated_copy_and_back() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_rdn_cloth()), "0.9|false");
        let (c, cloth) = render::cloths().remove(0);
        let r = render::mesh_renderers_on(cloth.object)[0];
        let now = render::with_mesh_renderer(r, |d| d.mesh).unwrap();
        assert!(
            render::mesh_info(render::MeshRef(now)).unwrap().engine,
            "the engine fills it"
        );
        assert_eq!(cloth.pin_box.unwrap().1.y, 0.1);
        assert_eq!(cloth.pins.len(), 1);
        render::destroyed(&[cloth.object]);
        let _ = c;
    });
}

// M9-TS-1234
#[test]
fn primitives_have_a_mesh_a_name_and_a_collider_hook() {
    in_every_gc_mode(|| unsafe {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let s2 = seen.clone();
        render::set_primitive_collider_hook(Some(std::rc::Rc::new(move |_, kind: &str| {
            s2.borrow_mut().push(kind.to_string())
        })));
        assert_eq!(text(ts_rdn_primitives()), "Crate|Sphere|cube");
        assert_eq!(*seen.borrow(), ["cube", "sphere"]);
        render::set_primitive_collider_hook(None);
    });
}

// M9-TS-1235
//
// Particle systems, line renderers and cloth load from a scene file and save the same data
// again (ranges as `[min, max]`, shapes, bursts, curves, pins).
#[test]
fn effect_components_round_trip_through_scene_files() {
    let _serial = platypus_ts_tests::serial();
    reset();
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let text = r#"{"format":"platypus.scene","version":1,"name":"Fx","objects":[
     {"id":1,"name":"P","components":[
       {"id":2,"type":"ParticleSystem","properties":{"duration":2,"loop":false,"startLifetime":[1,2],"startSpeed":3,
         "startColor":[{"$type":"Color","value":[1,0,0,1]},{"$type":"Color","value":[1,1,0,1]}],
         "bursts":[{"time":0.5,"count":20,"cycles":2,"interval":0.1}],
         "shape":{"type":"box","size":{"x":1,"y":2,"z":3}},
         "sizeOverLifetime":[{"time":0,"value":1},{"time":1,"value":0}],
         "colorOverLifetime":[{"time":1,"color":{"$type":"Color","value":[1,1,1,0]}}],
         "textureSheet":{"tilesX":4,"tilesY":2,"cycles":1},"blendMode":"additive","stopAction":"destroy"}}]},
     {"id":3,"name":"L","components":[
       {"id":4,"type":"LineRenderer","properties":{"width":0.1,"sides":5,
         "positions":[{"$type":"Vector3","value":[0,0,0]},{"$type":"Vector3","value":[0,1,0]}]}}]},
     {"id":5,"name":"C","components":[
       {"id":6,"type":"MeshRenderer","properties":{"mesh":"plane"}},
       {"id":7,"type":"Cloth","properties":{"bendingStiffness":0.7,"pins":[{"$type":"Vector3","value":[1,2,3]}],
         "pinBox":{"min":{"$type":"Vector3","value":[0,0,0]},"max":{"$type":"Vector3","value":[1,1,1]}},
         "sphereColliders":[{"$ref":"GameObject","id":1}]}}]}]}"#;
    let handle = platypus_scenes::load_scene(text, false).expect("loads");
    assert!(platypus_scenes::take_warnings().is_empty());
    let (_, p) = render::particle_systems().remove(0);
    assert_eq!((p.start_lifetime, p.start_speed), ([1.0, 2.0], [3.0, 3.0]));
    assert_eq!(p.shape_type, "box");
    assert_eq!(p.bursts, [[0.5, 20.0, 2.0, 0.1]]);
    assert!(!p.r#loop);
    let first = platypus_scenes::serialize(handle).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&first).unwrap();
    let props = |object: usize, ty: &str| {
        saved["objects"][object]["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == ty)
            .map(|c| c["properties"].clone())
            .unwrap()
    };
    let ps = props(0, "ParticleSystem");
    assert_eq!(ps["startLifetime"], serde_json::json!([1.0, 2.0]));
    assert_eq!(ps["startSpeed"], 3.0);
    assert_eq!(ps["shape"]["type"], "box");
    assert_eq!(ps["textureSheet"]["tilesX"], 4.0);
    assert_eq!(props(1, "LineRenderer")["sides"], 5);
    assert_eq!(
        props(2, "Cloth")["pinBox"]["max"]["value"],
        serde_json::json!([1.0, 1.0, 1.0])
    );
    // What was saved loads and saves the same.
    let again = platypus_scenes::load_scene(&first, false).expect("loads again");
    assert!(platypus_scenes::take_warnings().is_empty());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&platypus_scenes::serialize(again).unwrap())
            .unwrap(),
        saved
    );
    reset();
}

// M9-TS-1236
#[test]
fn gizmo_shapes_go_through_the_matrix_and_count_their_lines() {
    in_every_gc_mode(|| unsafe {
        // 12 box edges + 1 ray + 3 circles of 48 + 12 frustum edges.
        assert_eq!(ts_rdn_gizmos(), 169.0);
        let before = platypus_engine_api::gizmos::take_frame();
        assert!(before.is_none(), "nothing was presented");
    });
}

// M9-TS-1237
#[test]
fn screen_requests_go_to_the_host_and_change_listeners_run() {
    in_every_gc_mode(|| unsafe {
        use platypus_engine_api::screen;
        screen::reset();
        assert_eq!(
            text(ts_rdn_screenApi()),
            "1280|true|FullScreenWindow|1.78|0"
        );
        assert_eq!(
            screen::take_requests(),
            [
                screen::ScreenRequest {
                    mode: "FullScreenWindow".into(),
                    width: 0,
                    height: 0,
                    refresh_rate: 0.0
                },
                screen::ScreenRequest {
                    mode: "FullScreenWindow".into(),
                    width: 800,
                    height: 600,
                    refresh_rate: 60.0
                },
            ]
        );
        screen::update(1024, 768, 2.0);
        screen::set_size(640, 480);
        assert_eq!(ts_rdn_screenChangeCount(), 2.0);
        assert_eq!(screen::state().scale, 2.0);
        screen::set_display(
            "Windowed",
            (900, 700),
            Vec::new(),
            screen::Resolution::default(),
            Vec::new(),
        );
        assert!(!screen::state().full_screen);
        screen::reset();
    });
}

// M9-TS-1238
#[test]
fn quality_levels_validate_remember_and_tell_listeners() {
    in_every_gc_mode(|| unsafe {
        use platypus_engine_api::{prefs, screen};
        screen::reset();
        prefs::reset();
        screen::configure_quality(&serde_json::json!({
            "default": "Medium",
            "levels": { "Low": { "renderScale": 0.6 } }
        }));
        assert_eq!(screen::quality().name, "Medium");
        assert_eq!(text(ts_rdn_quality()), "1|8|3|Medium|4|all");
        // Switching to Low (0 + 1) and to Ultra (3 + 1) tells the listener; "bogus" shadows are ignored.
        assert_eq!(ts_rdn_levelChangeCount(), 1.0 + 4.0);
        let q = screen::quality();
        assert_eq!(
            (q.name.as_str(), q.anti_aliasing, q.shadow_resolution),
            ("Ultra", 8, 4096)
        );
        assert!(prefs::json("platypus.quality").unwrap().contains("Ultra"));
        // The saved choice comes back; the default applies without a save.
        screen::configure_quality(&serde_json::json!({ "default": "Low" }));
        assert_eq!(screen::quality().name, "Low");
        screen::apply_default_quality();
        assert_eq!(screen::quality().name, "Ultra");
        let low = screen::quality_version();
        assert!(low > 0);
        screen::reset();
        prefs::reset();
    });
}

// M9-TS-3387
#[test]
fn renderers_say_whether_the_last_frame_saw_them_once_asked() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_rdn_visibility()), "false|false|0");
        let mesh = render::RendererId::Mesh(render::mesh_renderers()[0].0.0);
        let body = render::RendererId::Skinned(render::skinned_mesh_renderers()[0].0.0);
        let mut watched = render::watched_renderers();
        watched.sort_by_key(|r| matches!(r, render::RendererId::Skinned(_)));
        assert_eq!(watched, [mesh, body], "asking starts the watch");
        render::set_seen_renderers([body]);
        render::set_render_stats(render::Stats {
            shadow_casters: 5.0,
            ..Default::default()
        });
        assert_eq!(text(ts_rdn_visibility()), "false|true|5");
    });
}

// M9-TS-1239
//
// A model asked for before the host read its file gets its nodes when the host reports the
// description: one GameObject per node, a renderer per primitive (child objects when a node has
// several), materials from the glTF, and colliders through the physics hook. A scene file saves
// the copy as `"model"` without its nodes and builds them again on load.
#[test]
fn a_model_builds_its_nodes_when_loaded_and_scene_files_keep_only_the_root() {
    in_every_gc_mode(|| unsafe {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let s2 = seen.clone();
        render::set_model_collider_hook(Some(std::rc::Rc::new(move |_, _, convex| {
            s2.borrow_mut().push(convex)
        })));
        assert_eq!(text(ts_rdn_model()), "false|ship|ship|true|models/ship.glb");
        let cmds = render::take_commands();
        assert_eq!(
            cmds.iter()
                .filter(|c| matches!(c, render::RenderCommand::Model(_)))
                .count(),
            1,
            "one file, read once"
        );
        let find_root = |i: usize| platypus_scene::api::with_scene(|sc| sc.roots()[i]).unwrap();
        let first_root = find_root(0);
        assert_eq!(
            platypus_scene::api::with_scene(|sc| sc.children(first_root).len()),
            Some(0)
        );
        let (m1, m2, m3) = (
            render::engine_mesh(),
            render::engine_mesh(),
            render::engine_mesh(),
        );
        let tex = render::engine_texture(4, 4);
        let description = format!(
            r#"{{"nodes":[
              {{"name":"Body","position":[0,1,0],"rotation":[0,0,0,1],"scale":[1,1,1],"children":[1],
               "primitives":[{{"mesh":{},"material":0,"bounds":[[0,0,0],[1,1,1]]}}]}},
              {{"name":"Wheel","position":[1,0,0],"rotation":[0,0,0,1],"scale":[1,1,1],"children":[],
               "primitives":[{{"mesh":{},"material":null,"bounds":[[0,0,0],[1,1,1]]}},{{"mesh":{},"material":0,"bounds":[[0,0,0],[1,1,1]]}}]}}],
             "roots":[0],
             "materials":[{{"name":"Paint","color":[1,0,0,1],"emission":[0,0,0],"smoothness":0.3,"metallic":0.5,
               "unlit":false,"texture":[{},4,4],"alpha":"mask","alphaCutoff":0.4,"doubleSided":true,
               "normal":[{},0.8],"metallicRoughness":null,"occlusion":null,"emissive":null}}]}}"#,
            m1.0, m2.0, m3.0, tex.0, tex.0
        );
        let model = render::ModelRef(1);
        render::model_loaded(model, Some(&description));
        // Each of the two copies: root > Body > Wheel > two child objects with a renderer each.
        let root = find_root(0);
        let body = platypus_scene::api::with_scene(|sc| sc.children(root)[0]).unwrap();
        let wheel = platypus_scene::api::with_scene(|sc| sc.children(body)[0]).unwrap();
        let names = platypus_scene::api::with_scene(|sc| {
            (
                sc.name(body).unwrap().to_owned(),
                sc.children(wheel)
                    .iter()
                    .map(|c| sc.name(*c).unwrap().to_owned())
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap();
        assert_eq!(names.0, "Body");
        assert_eq!(names.1, ["Wheel 0", "Wheel 1"]);
        let r = render::mesh_renderers_on(body)[0];
        let (mesh, mat) = render::with_mesh_renderer(r, |d| (d.mesh, d.material)).unwrap();
        assert_eq!(mesh, m1.0);
        let paint = render::with_material(render::MaterialRef(mat), |d| {
            (
                d.rendering_mode.clone(),
                d.alpha_cutoff,
                d.double_sided,
                d.metallic,
                d.normal_scale,
                d.gltf_packing,
            )
        })
        .unwrap();
        assert_eq!(paint, ("cutout".to_string(), 0.4, true, 0.5, 0.8, true));
        // Colliders were asked for the first copy only, one per primitive: 3 (convex).
        assert_eq!(*seen.borrow(), [true, true, true]);
        // The scene saves each copy as its root with the model, not the nodes.
        let handle = platypus_scenes::active();
        let first = platypus_scenes::serialize(handle).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&first).unwrap();
        let objects = saved["objects"].as_array().unwrap();
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0]["model"]["src"], "models/ship.glb");
        assert_eq!(objects[0]["model"]["colliders"], "convex");
        assert!(objects[0].get("children").is_none(), "nodes are not saved");
        // Loading what was saved builds the nodes again (the model is loaded already).
        platypus_scenes::load_scene(&first, true).expect("loads");
        let roots = platypus_scene::api::with_scene(|sc| sc.roots().len()).unwrap();
        assert_eq!(roots, 4);
        let last = find_root(roots - 2);
        assert_eq!(
            platypus_scene::api::with_scene(|sc| sc.children(last).len()),
            Some(1)
        );
        render::set_model_collider_hook(None);
    });
}

// M9-TS-1226
//
// The example game's scene file loads its Light, MeshRenderer (primitive mesh, inline Material)
// and Camera extras, and saves them back.
#[test]
fn a_real_scene_file_loads_lights_renderers_materials_and_camera_settings() {
    let _serial = platypus_ts_tests::serial();
    reset();
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let text = include_str!("../../../../examples/scenes/scenes/level1.scene.json");
    let handle = platypus_scenes::load_scene(text, false).expect("loads");
    let warnings = platypus_scenes::take_warnings();
    assert!(
        warnings.iter().all(|w| w.contains("Spinner")),
        "only the game's own component is unknown: {warnings:?}"
    );
    let cams = platypus_engine_api::camera::cameras();
    assert_eq!(cams[0].1.clear_flags, "skybox");
    let lights = render::lights();
    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].1.r#type, "directional");
    assert_eq!(lights[0].1.intensity, 1.3);
    assert_eq!(lights[0].1.shadows, "soft");
    let renderers = render::mesh_renderers();
    assert!(renderers.len() > 3);
    let ground = &renderers[0].1;
    assert_eq!(
        render::primitive_name(render::MeshRef(ground.mesh)).unwrap(),
        "plane"
    );
    let (smooth, g) = render::with_material(render::MaterialRef(ground.material), |m| {
        (m.smoothness, m.color.g)
    })
    .unwrap();
    assert_eq!((smooth, g), (0.2, 0.36));
    // Saving writes the primitive as its name and the material inline, without default extras.
    let saved: serde_json::Value =
        serde_json::from_str(&platypus_scenes::serialize(handle).unwrap()).unwrap();
    let ground_json = &saved["objects"][2]["components"][0]["properties"];
    assert_eq!(ground_json["mesh"], "plane");
    assert_eq!(ground_json["material"]["$type"], "Material");
    assert_eq!(ground_json["material"]["smoothness"], 0.2);
    assert!(ground_json["material"].get("normalMap").is_none());
    assert_eq!(
        saved["objects"][1]["components"][0]["properties"]["type"],
        "directional"
    );
    reset();
}

// M9-TS-1227
//
// Shared assets in a scene file: a texture and a script mesh in the `assets` table load once
// however many components use them, and saving writes the table again; loading what was saved
// and saving again gives the same data.
#[test]
fn the_assets_table_loads_shared_textures_and_meshes_and_saves_them_again() {
    let _serial = platypus_ts_tests::serial();
    reset();
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let text = r#"{"format":"platypus.scene","version":1,"name":"Assets",
     "assets":{
       "texture1":{"type":"Texture","src":"img/a.png","filterMode":"point","wrapMode":"clamp","mipmaps":false},
       "mesh1":{"type":"Mesh","vertices":[0,0,0,1,0,0,0,1,0],"triangles":[0,1,2],"normals":[],"uv":[]},
       "texture2":{"type":"RenderTexture","width":32,"height":16}},
     "objects":[
      {"id":1,"name":"A","components":[
        {"id":2,"type":"MeshRenderer","properties":{"mesh":{"$asset":"mesh1"},
          "material":{"$type":"Material","mainTexture":{"$asset":"texture1"},"normalMap":{"$asset":"texture1"}}}}]},
      {"id":3,"name":"B","components":[
        {"id":4,"type":"MeshRenderer","properties":{"mesh":{"$asset":"mesh1"}}},
        {"id":5,"type":"SpriteRenderer","properties":{"sprite":{"$type":"Sprite","texture":{"$asset":"texture1"},"rect":{"$type":"Rect","value":[0,0,2,2]}}}},
        {"id":6,"type":"Camera","properties":{"targetTexture":{"$asset":"texture2"}}}]}]}"#;
    let handle = platypus_scenes::load_scene(text, false).expect("loads");
    assert!(platypus_scenes::take_warnings().is_empty());
    let renderers = render::mesh_renderers();
    assert_eq!(renderers[0].1.mesh, renderers[1].1.mesh, "one mesh, shared");
    let tex = render::with_material(render::MaterialRef(renderers[0].1.material), |m| {
        (m.main_texture, m.normal_map)
    })
    .unwrap();
    assert_eq!(tex.0, tex.1, "one texture, shared");
    let info = render::texture_info(render::TextureRef(tex.0)).unwrap();
    assert_eq!(info.src.as_deref(), Some("img/a.png"));
    assert_eq!(
        (
            info.filter_mode.as_str(),
            info.wrap_mode.as_str(),
            info.mipmaps
        ),
        ("point", "clamp", false)
    );
    let cams = platypus_engine_api::camera::cameras();
    let rt = render::texture_info(render::TextureRef(cams[0].1.target_texture.0)).unwrap();
    assert!(rt.render_texture && rt.width == 32);
    let sprite = render::sprite_renderers()[0].1.sprite;
    let s = render::with_sprite(render::SpriteRef(sprite), |d| {
        (d.texture, d.has_rect, d.rect)
    })
    .unwrap();
    assert_eq!(s.0, tex.0);
    assert_eq!((s.1, s.2), (true, [0.0, 0.0, 2.0, 2.0]));

    let first = platypus_scenes::serialize(handle).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&first).unwrap();
    let assets = saved["assets"].as_object().unwrap();
    assert_eq!(assets.len(), 3, "{assets:?}");
    let props = |object: usize, ty: &str| {
        saved["objects"][object]["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == ty)
            .map(|c| c["properties"].clone())
            .unwrap()
    };
    let a = &props(0, "MeshRenderer");
    let b = &props(1, "MeshRenderer");
    assert_eq!(a["mesh"], b["mesh"], "both point at the same table entry");
    let mesh_id = a["mesh"]["$asset"].as_str().unwrap();
    assert_eq!(assets[mesh_id]["type"], "Mesh");
    assert_eq!(assets[mesh_id]["triangles"], serde_json::json!([0, 1, 2]));
    let tex_id = a["material"]["mainTexture"]["$asset"].as_str().unwrap();
    assert_eq!(assets[tex_id]["src"], "img/a.png");
    assert_eq!(assets[tex_id]["filterMode"], "point");
    assert_eq!(a["material"]["normalMap"]["$asset"], tex_id);
    // Loading what was saved and saving again changes nothing.
    let again = platypus_scenes::load_scene(&first, false).expect("loads again");
    assert!(platypus_scenes::take_warnings().is_empty());
    let second = platypus_scenes::serialize(again).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&second).unwrap(),
        saved
    );
    reset();
}

// M9-TS-3201
//
// Fog in the `renderSettings` of a scene file: the keys the TypeScript `scenes.ts` saves.
#[test]
fn scene_files_apply_and_save_fog() {
    let _serial = platypus_ts_tests::serial();
    reset();
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let text = r#"{"format":"platypus.scene","version":1,"name":"Mist","objects":[],
      "renderSettings":{"fog":true,"fogMode":"linear","fogColor":{"$type":"Color","value":[0.2,0.3,0.4,1]},
        "fogDensity":0.02,"fogStartDistance":5,"fogEndDistance":80,"fogCulling":false,
        "heightFog":{"density":0.3,"baseHeight":1.5},
        "volumetricFog":{"intensity":2,"resolution":"ultra","denoising":"none","samples":4}}}"#;
    let handle = platypus_scenes::load_scene(text, false).expect("loads");
    let f = render::settings().fog;
    assert!(f.enabled && !f.culling);
    assert_eq!(
        (f.mode.as_str(), f.color.g, f.density, f.start, f.end),
        ("linear", 0.3, 0.02, 5.0, 80.0)
    );
    let h = f.height.expect("height fog");
    assert_eq!(
        (h.density, h.base_height, h.falloff),
        (0.3, 1.5, 0.2),
        "falloff keeps its default"
    );
    let v = f.volumetric.expect("light shafts");
    assert_eq!(
        (
            v.intensity,
            v.resolution.as_str(),
            v.denoising.as_str(),
            v.samples
        ),
        (2.0, "ultra", "none", 4.0)
    );
    assert_eq!(v.steps, 4.0, "a missing key keeps its default");
    let saved: serde_json::Value =
        serde_json::from_str(&platypus_scenes::serialize(handle).unwrap()).unwrap();
    let r = &saved["renderSettings"];
    assert_eq!(r["fog"], true);
    assert_eq!(r["fogMode"], "linear");
    assert_eq!(r["fogEndDistance"], 80.0);
    assert_eq!(r["fogCulling"], false);
    assert_eq!(r["heightFog"]["baseHeight"], 1.5);
    assert_eq!(r["volumetricFog"]["resolution"], "ultra");
    assert_eq!(r["volumetricFog"]["albedo"]["$type"], "Color");
    // Turning fog off and a null shaft setting round-trip as null.
    let off = r#"{"format":"platypus.scene","version":1,"name":"Clear","objects":[],
      "renderSettings":{"fog":false,"heightFog":null,"volumetricFog":null}}"#;
    let handle = platypus_scenes::load_scene(off, false).expect("loads");
    let f = render::settings().fog;
    assert!(!f.enabled && f.height.is_none() && f.volumetric.is_none());
    let saved: serde_json::Value =
        serde_json::from_str(&platypus_scenes::serialize(handle).unwrap()).unwrap();
    assert!(saved["renderSettings"]["heightFog"].is_null());
    reset();
}
