// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The component kinds of the engine components that are native: `Camera`, `Animation` and
//! `Animator`. Other engine components register their own as they are ported.

use platypus_animation::{ClipId, ControllerId};
use platypus_engine_api::animation::{AnimationRef, ClipRef, animation_component_api};
use platypus_engine_api::animator::{AnimatorRef, ControllerRef, animator_api};
use platypus_engine_api::camera::{self, CameraRef};
use platypus_engine_api::color::Rgba;
use platypus_kinds::{AssetKind, Kind};
use platypus_scene::ObjectId;
use serde_json::{Map, Value, json};
use ts_api::Handle;

fn number(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32)
}

fn color(v: &Value) -> Option<Rgba> {
    let a = v.get("value")?.as_array()?;
    let n = |i: usize| a.get(i).and_then(Value::as_f64);
    Some(Rgba {
        r: n(0)?,
        g: n(1)?,
        b: n(2)?,
        a: n(3)?,
    })
}

fn rect(v: &Value) -> Option<[f64; 4]> {
    let a = v.get("value")?.as_array()?;
    let n = |i: usize| a.get(i).and_then(Value::as_f64);
    Some([n(0)?, n(1)?, n(2)?, n(3)?])
}

fn camera_kind() -> Kind {
    Kind {
        name: "Camera".into(),
        create: Box::new(|object| Ok(u64::from(CameraRef::attach(ObjectId::from_raw(object)).0))),
        on: Box::new(|object| {
            let o = ObjectId::from_raw(object);
            camera::cameras()
                .into_iter()
                .filter(|(_, d)| d.object == o)
                .map(|(c, _)| u64::from(c.0))
                .collect()
        }),
        save: Box::new(|h| {
            let mut m = Map::new();
            camera::with_camera(CameraRef(h as u32), |d| {
                m.insert("fieldOfView".into(), json!(f64::from(d.fov)));
                m.insert("nearClipPlane".into(), json!(f64::from(d.near)));
                m.insert("farClipPlane".into(), json!(f64::from(d.far)));
                let b = d.background;
                m.insert(
                    "backgroundColor".into(),
                    json!({ "$type": "Color", "value": [b.r, b.g, b.b, b.a] }),
                );
                m.insert("depth".into(), json!(f64::from(d.depth)));
                m.insert("orthographic".into(), json!(d.orthographic));
                m.insert(
                    "orthographicSize".into(),
                    json!(f64::from(d.orthographic_size)),
                );
                m.insert("clearFlags".into(), json!(d.clear_flags));
                m.insert("rect".into(), json!({ "$type": "Rect", "value": d.rect }));
                m.insert("cullingMask".into(), json!(d.culling_mask));
                m.insert("layerCullDistances".into(), json!(d.layer_cull_distances));
                m.insert("layerCullSpherical".into(), json!(d.layer_cull_spherical));
                m.insert("antialiasing".into(), json!(d.antialiasing));
                m.insert(
                    "targetTexture".into(),
                    platypus_kinds::asset_ref("Texture", u64::from(d.target_texture.0)),
                );
            });
            m
        }),
        load: Box::new(|h, props| {
            let mut warnings = Vec::new();
            camera::with_camera(CameraRef(h as u32), |d| {
                for (k, v) in props {
                    let ok = match k.as_str() {
                        "fieldOfView" => number(v).map(|x| d.fov = x.clamp(1.0, 179.0)),
                        "nearClipPlane" => number(v).map(|x| d.near = x),
                        "farClipPlane" => number(v).map(|x| d.far = x),
                        "backgroundColor" => color(v).map(|c| d.background = c),
                        "depth" => number(v).map(|x| d.depth = x),
                        "orthographic" => v.as_bool().map(|x| d.orthographic = x),
                        "orthographicSize" => number(v).map(|x| d.orthographic_size = x),
                        "clearFlags" => v
                            .as_str()
                            .filter(|s| ["solidColor", "skybox"].contains(s))
                            .map(|s| d.clear_flags = s.to_string()),
                        "rect" => rect(v).map(|r| d.rect = r),
                        "cullingMask" => v.as_f64().map(|x| d.culling_mask = x as i64 as u32),
                        "layerCullDistances" => v.as_array().map(|a| {
                            for (i, x) in d.layer_cull_distances.iter_mut().enumerate() {
                                *x = a.get(i).and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
                            }
                        }),
                        "layerCullSpherical" => v.as_bool().map(|x| d.layer_cull_spherical = x),
                        "antialiasing" => v
                            .as_str()
                            .filter(|s| ["none", "fxaa"].contains(s))
                            .map(|s| d.antialiasing = s.to_string()),
                        "targetTexture" => platypus_kinds::asset_handle(v, "Texture")
                            .map(|h| d.target_texture = camera::TextureRef(h as u32)),
                        other => {
                            warnings.push(format!("no property `{other}` yet; dropped"));
                            continue;
                        }
                    };
                    if ok.is_none() {
                        warnings.push(format!("property `{k}` has a value of the wrong shape"));
                    }
                }
            });
            warnings
        }),
        awake: Box::new(|_| {}),
        object: Box::new(|h| camera::with_camera(CameraRef(h as u32), |d| d.object.to_raw())),
        destroy: Box::new(|h| camera::remove(CameraRef(h as u32))),
    }
}

/// The functions of a kind with one component per object (`find` gives it, 0 for none).
struct One {
    create: fn(ObjectId) -> u64,
    find: fn(ObjectId) -> u64,
    object: fn(u64) -> Option<ObjectId>,
}

/// A kind of one component per object, saved and loaded by `save` / `load` (`load` applies one
/// property, `None` for a value of the wrong shape, `Some(false)` for an unknown name).
fn one(
    name: &str,
    f: One,
    save: fn(u64) -> Map<String, Value>,
    load: fn(u64, &str, &Value) -> Option<bool>,
) -> Kind {
    Kind {
        name: name.into(),
        create: Box::new(move |o| Ok((f.create)(ObjectId::from_raw(o)))),
        on: Box::new(move |o| match (f.find)(ObjectId::from_raw(o)) {
            0 => Vec::new(),
            h => vec![h],
        }),
        save: Box::new(save),
        load: Box::new(move |h, props| {
            let mut warnings = Vec::new();
            for (k, v) in props {
                match load(h, k, v) {
                    Some(true) => {}
                    Some(false) => warnings.push(format!("no property `{k}` yet; dropped")),
                    None => warnings.push(format!("property `{k}` has a value of the wrong shape")),
                }
            }
            warnings
        }),
        awake: Box::new(|_| {}),
        object: Box::new(move |h| (f.object)(h).map(Handle::to_raw)),
        destroy: Box::new(|_| {}),
    }
}

/// An animation asset (`AnimationController`, `AnimationClip`): shared, so components using one
/// keep using one; its definition is the JSON `toJson` writes.
fn animation_asset(
    name: &str,
    describe: fn(&platypus_animation::World, u64) -> Value,
    create: fn(&mut platypus_animation::World, &Value) -> u64,
) -> AssetKind {
    let type_name = name.to_owned();
    AssetKind {
        name: name.into(),
        also: Vec::new(),
        shared: true,
        create: Box::new(move |def| Ok(platypus_animation::with_world(|w| create(w, def)))),
        describe: Box::new(move |h| {
            let mut def = platypus_animation::with_world(|w| describe(w, h));
            // The table entry names its kind.
            def.as_object_mut()?.insert("type".into(), json!(type_name));
            Some(def)
        }),
    }
}

fn animator_save(h: u64) -> Map<String, Value> {
    let a = AnimatorRef(h);
    let mut m = Map::new();
    m.insert(
        "controller".into(),
        platypus_kinds::asset_ref("AnimationController", a.controller().0),
    );
    m.insert("speed".into(), json!(a.speed()));
    m.insert(
        "cullingMode".into(),
        json!(animator_api::animator_culling_mode_name(a.culling_mode())),
    );
    m.insert("enabled".into(), json!(a.enabled()));
    m
}

fn animator_load(h: u64, k: &str, v: &Value) -> Option<bool> {
    let a = AnimatorRef(h);
    match k {
        "controller" => {
            let c = platypus_kinds::asset_handle(v, "AnimationController")?;
            a.set_controller(ControllerRef(c));
        }
        "speed" => a.set_speed(v.as_f64()?),
        "cullingMode" => {
            a.set_culling_mode(animator_api::animator_culling_mode_parse(v.as_str()?)?)
        }
        "enabled" => a.set_enabled(v.as_bool()?),
        _ => return Some(false),
    }
    Some(true)
}

fn animation_save(h: u64) -> Map<String, Value> {
    let a = AnimationRef(h);
    let mut m = Map::new();
    m.insert(
        "clip".into(),
        platypus_kinds::asset_ref("AnimationClip", a.clip().0),
    );
    let clips: Vec<Value> = (0..a.clip_count() as usize)
        .map(|i| platypus_kinds::asset_ref("AnimationClip", a.clip_at(i as f64).0))
        .collect();
    m.insert("clips".into(), Value::Array(clips));
    m.insert("playAutomatically".into(), json!(a.play_automatically()));
    m.insert(
        "cullingType".into(),
        json!(animation_component_api::animation_culling_type_name(
            a.culling_type()
        )),
    );
    m
}

fn animation_load(h: u64, k: &str, v: &Value) -> Option<bool> {
    let a = AnimationRef(h);
    match k {
        "clip" => a.set_clip(ClipRef(platypus_kinds::asset_handle(v, "AnimationClip")?)),
        "clips" => {
            let clips = v
                .as_array()?
                .iter()
                .map(|c| platypus_kinds::asset_handle(c, "AnimationClip"))
                .collect::<Option<Vec<u64>>>()?;
            for c in clips.into_iter().filter(|&c| c != 0) {
                a.add_clip(ClipRef(c), "");
            }
        }
        "playAutomatically" => a.set_play_automatically(v.as_bool()?),
        "cullingType" => a.set_culling_type(animation_component_api::animation_culling_type_parse(
            v.as_str()?,
        )?),
        _ => return Some(false),
    }
    Some(true)
}

/// Registers the engine's kinds on this thread (a later registration replaces an earlier one).
pub fn register_engine_kinds() {
    platypus_kinds::register(camera_kind());
    platypus_engine_api::audio_components::register_kinds();
    platypus_engine_api::spline::register_kinds();
    platypus_render_api::register_kinds();
    platypus_kinds::register_asset(animation_asset(
        "AnimationController",
        |w, h| w.controller_to_json(ControllerId(h as u32)),
        |w, def| u64::from(w.controller_from_json(def).0),
    ));
    platypus_kinds::register_asset(animation_asset(
        "AnimationClip",
        |w, h| w.clip_to_json(ClipId(h as u32)),
        |w, def| u64::from(w.clip_from_json(def).0),
    ));
    platypus_kinds::register(one(
        "Animation",
        One {
            create: |o| AnimationRef::attach(o).0,
            find: |o| AnimationRef::find_on(o).0,
            object: |h| {
                let o = AnimationRef(h).game_object();
                (o.index != u32::MAX).then_some(o)
            },
        },
        animation_save,
        animation_load,
    ));
    platypus_kinds::register(one(
        "Animator",
        One {
            create: |o| AnimatorRef::attach(o).0,
            find: |o| AnimatorRef::find_on(o).0,
            object: |h| {
                let o = AnimatorRef(h).game_object();
                (o.index != u32::MAX).then_some(o)
            },
        },
        animator_save,
        animator_load,
    ));
    crate::kind_fields::register();
}
