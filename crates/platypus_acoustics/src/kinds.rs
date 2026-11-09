// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Scene-file kinds for `AudioZone`, `AudioOccluder` and `SoundSensor` (so the editor and scene
//! files can make, save and load them), and the field docs the Inspector shows: the zone's
//! gradient controls are its `radius` / `size`, `innerFraction` and `curve`.

use crate::components::{
    attach_occluder, attach_portal, attach_sensor, attach_zone, occluders_on, portals_on,
    remove_occluder, remove_portal, remove_sensor, remove_zone, sensors_on, zones_on,
};
use crate::model::{Curve, Shape};
use crate::state::{AudioOccluderRef, AudioPortalRef, AudioZoneRef, SoundSensorRef, st};
use platypus_engine_api::audio::{self, SoundRef};
use platypus_kinds::{FieldDoc, Kind, Props, register, register_fields};
use platypus_scene::ObjectId;
use serde_json::{Map, Value, json};
use ts_api::Handle;

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn warn_unknown(k: &str, out: &mut Vec<String>) {
    out.push(format!("no property `{k}` yet; dropped"));
}

fn f(name: &str, ty: &str, default: Value, doc: &str) -> FieldDoc {
    FieldDoc {
        name: name.into(),
        ty: ty.into(),
        default,
        doc: doc.into(),
    }
}

fn shape_name(s: Shape) -> &'static str {
    match s {
        Shape::Sphere => "sphere",
        Shape::Box => "box",
    }
}

fn curve_name(c: Curve) -> &'static str {
    match c {
        Curve::Linear => "linear",
        Curve::Smooth => "smooth",
        Curve::EaseIn => "easeIn",
        Curve::EaseOut => "easeOut",
    }
}

fn sound_value(clip: SoundRef) -> Value {
    if !audio::sound_exists(clip) {
        return Value::Null;
    }
    json!(audio::sound_path(clip))
}

fn zone_kind() -> Kind {
    Kind {
        name: "AudioZone".into(),
        create: Box::new(|o| Ok(u64::from(attach_zone(ObjectId::from_raw(o)).0))),
        on: Box::new(|o| {
            zones_on(ObjectId::from_raw(o))
                .into_iter()
                .map(|z| u64::from(z.0))
                .collect()
        }),
        save: Box::new(|h| {
            let mut m = Map::new();
            crate::components::zone(AudioZoneRef(h as u32), |z| {
                m.insert("enabled".into(), json!(z.enabled));
                m.insert("shape".into(), json!(shape_name(z.shape)));
                m.insert("radius".into(), json!(z.radius));
                m.insert("size".into(), json!(z.size));
                m.insert("innerFraction".into(), json!(z.inner));
                m.insert("curve".into(), json!(curve_name(z.curve)));
                m.insert("invert".into(), json!(z.invert));
                m.insert("affectsSources".into(), json!(z.affects_sources));
                m.insert("priority".into(), json!(z.priority));
                m.insert("lowPass".into(), json!(z.target.low_pass));
                m.insert("highPass".into(), json!(z.target.high_pass));
                m.insert("reverb".into(), json!(z.target.reverb));
                m.insert("reverbSize".into(), json!(z.target.reverb_size));
                m.insert("reverbDamping".into(), json!(z.target.reverb_damping));
                m.insert("echo".into(), json!(z.target.echo));
                m.insert("volume".into(), json!(z.target.volume));
                m.insert("ambience".into(), sound_value(z.ambience));
                m.insert("ambienceVolume".into(), json!(z.ambience_volume));
            });
            m
        }),
        load: Box::new(|h, props: &Props| {
            let mut warnings = Vec::new();
            crate::components::zone(AudioZoneRef(h as u32), |z| {
                for (k, v) in props {
                    let ok = match k.as_str() {
                        "enabled" => v.as_bool().map(|x| z.enabled = x),
                        "shape" => v.as_str().map(|x| {
                            z.shape = if x == "box" {
                                Shape::Box
                            } else {
                                Shape::Sphere
                            }
                        }),
                        "radius" => num(v).map(|x| z.radius = x),
                        "size" => v.as_array().filter(|a| a.len() == 3).map(|a| {
                            for (i, x) in a.iter().enumerate() {
                                z.size[i] = x.as_f64().unwrap_or(z.size[i]);
                            }
                        }),
                        "innerFraction" => num(v).map(|x| z.inner = x),
                        "curve" => v.as_str().map(|x| {
                            z.curve = match x {
                                "linear" => Curve::Linear,
                                "easeIn" => Curve::EaseIn,
                                "easeOut" => Curve::EaseOut,
                                _ => Curve::Smooth,
                            }
                        }),
                        "invert" => v.as_bool().map(|x| z.invert = x),
                        "affectsSources" => v.as_bool().map(|x| z.affects_sources = x),
                        "priority" => num(v).map(|x| z.priority = x),
                        "lowPass" => num(v).map(|x| z.target.low_pass = x),
                        "highPass" => num(v).map(|x| z.target.high_pass = x),
                        "reverb" => num(v).map(|x| z.target.reverb = x),
                        "reverbSize" => num(v).map(|x| z.target.reverb_size = x),
                        "reverbDamping" => num(v).map(|x| z.target.reverb_damping = x),
                        "echo" => num(v).map(|x| z.target.echo = x),
                        "volume" => num(v).map(|x| z.target.volume = x),
                        "ambience" => match v {
                            Value::Null => Some(SoundRef(0)),
                            Value::String(p) => Some(audio::cached_sound(p)),
                            _ => None,
                        }
                        .map(|c| z.ambience = c),
                        "ambienceVolume" => num(v).map(|x| z.ambience_volume = x),
                        other => {
                            warn_unknown(other, &mut warnings);
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
        object: Box::new(|h| {
            crate::components::zone(AudioZoneRef(h as u32), |z| z.object.to_raw())
        }),
        destroy: Box::new(|h| remove_zone(AudioZoneRef(h as u32))),
    }
}

fn occluder_kind() -> Kind {
    let get = |h: u64| AudioOccluderRef(h as u32);
    Kind {
        name: "AudioOccluder".into(),
        create: Box::new(|o| Ok(u64::from(attach_occluder(ObjectId::from_raw(o)).0))),
        on: Box::new(|o| {
            occluders_on(ObjectId::from_raw(o))
                .into_iter()
                .map(|z| u64::from(z.0))
                .collect()
        }),
        save: Box::new(move |h| {
            let mut m = Map::new();
            st(|s| {
                if let Some(o) = s.occluders.get_mut(get(h).0) {
                    m.insert("openness".into(), json!(o.openness));
                    m.insert("absorption".into(), json!(o.absorption));
                    m.insert("lowPass".into(), json!(o.low_pass));
                }
            });
            m
        }),
        load: Box::new(move |h, props: &Props| {
            let mut warnings = Vec::new();
            st(|s| {
                let Some(o) = s.occluders.get_mut(get(h).0) else {
                    return;
                };
                for (k, v) in props {
                    let ok = match k.as_str() {
                        "openness" => num(v).map(|x| o.openness = x),
                        "absorption" => num(v).map(|x| o.absorption = x),
                        "lowPass" => num(v).map(|x| o.low_pass = x),
                        other => {
                            warn_unknown(other, &mut warnings);
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
        object: Box::new(move |h| st(|s| s.occluders.get_mut(get(h).0).map(|o| o.object.to_raw()))),
        destroy: Box::new(move |h| remove_occluder(get(h))),
    }
}

fn surface_kind() -> Kind {
    let get = |h: u64| crate::state::AudioSurfaceRef(h as u32);
    Kind {
        name: "AudioSurface".into(),
        create: Box::new(|o| {
            Ok(u64::from(
                crate::components::attach_surface(ObjectId::from_raw(o)).0,
            ))
        }),
        on: Box::new(|o| {
            crate::components::surfaces_on(ObjectId::from_raw(o))
                .into_iter()
                .map(|s| u64::from(s.0))
                .collect()
        }),
        save: Box::new(move |h| {
            let mut m = Map::new();
            crate::components::surface(get(h), |s| {
                m.insert("surface".into(), json!(s.name));
            });
            m
        }),
        load: Box::new(move |h, props: &Props| {
            let mut warnings = Vec::new();
            crate::components::surface(get(h), |s| {
                for (k, v) in props {
                    match (k.as_str(), v.as_str()) {
                        ("surface", Some(name)) => s.name = name.to_string(),
                        ("surface", None) => {
                            warnings.push(format!("property `{k}` has a value of the wrong shape"))
                        }
                        (other, _) => warn_unknown(other, &mut warnings),
                    }
                }
            });
            warnings
        }),
        awake: Box::new(|_| {}),
        object: Box::new(move |h| crate::components::surface(get(h), |s| s.object.to_raw())),
        destroy: Box::new(move |h| crate::components::remove_surface(get(h))),
    }
}

fn portal_kind() -> Kind {
    let get = |h: u64| AudioPortalRef(h as u32);
    Kind {
        name: "AudioPortal".into(),
        create: Box::new(|o| Ok(u64::from(attach_portal(ObjectId::from_raw(o)).0))),
        on: Box::new(|o| {
            portals_on(ObjectId::from_raw(o))
                .into_iter()
                .map(|p| u64::from(p.0))
                .collect()
        }),
        save: Box::new(move |h| {
            let mut m = Map::new();
            crate::components::portal(get(h), |p| {
                m.insert("enabled".into(), json!(p.enabled));
                m.insert("gain".into(), json!(p.gain));
                m.insert("radius".into(), json!(p.radius));
                m.insert("lowPass".into(), json!(p.low_pass));
            });
            m
        }),
        load: Box::new(move |h, props: &Props| {
            let mut warnings = Vec::new();
            crate::components::portal(get(h), |p| {
                for (k, v) in props {
                    let ok = match k.as_str() {
                        "enabled" => v.as_bool().map(|b| p.enabled = b),
                        "gain" => num(v).map(|n| p.gain = n),
                        "radius" => num(v).map(|n| p.radius = n),
                        "lowPass" => num(v).map(|n| p.low_pass = n),
                        other => {
                            warn_unknown(other, &mut warnings);
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
        object: Box::new(move |h| crate::components::portal(get(h), |p| p.object.to_raw())),
        destroy: Box::new(move |h| remove_portal(get(h))),
    }
}

fn sensor_kind() -> Kind {
    let get = |h: u64| SoundSensorRef(h as u32);
    Kind {
        name: "SoundSensor".into(),
        create: Box::new(|o| Ok(u64::from(attach_sensor(ObjectId::from_raw(o)).0))),
        on: Box::new(|o| {
            sensors_on(ObjectId::from_raw(o))
                .into_iter()
                .map(|z| u64::from(z.0))
                .collect()
        }),
        save: Box::new(move |h| {
            let mut m = Map::new();
            st(|s| {
                if let Some(x) = s.sensors.get_mut(get(h).0) {
                    m.insert("enabled".into(), json!(x.enabled));
                    m.insert("sensitivity".into(), json!(x.sensitivity));
                    m.insert("threshold".into(), json!(x.threshold));
                    m.insert("occlusion".into(), json!(x.occlusion));
                }
            });
            m
        }),
        load: Box::new(move |h, props: &Props| {
            let mut warnings = Vec::new();
            st(|s| {
                let Some(x) = s.sensors.get_mut(get(h).0) else {
                    return;
                };
                for (k, v) in props {
                    let ok = match k.as_str() {
                        "enabled" => v.as_bool().map(|b| x.enabled = b),
                        "sensitivity" => num(v).map(|n| x.sensitivity = n),
                        "threshold" => num(v).map(|n| x.threshold = n),
                        "occlusion" => v.as_bool().map(|b| x.occlusion = b),
                        other => {
                            warn_unknown(other, &mut warnings);
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
        object: Box::new(move |h| st(|s| s.sensors.get_mut(get(h).0).map(|x| x.object.to_raw()))),
        destroy: Box::new(move |h| remove_sensor(get(h))),
    }
}

/// Registers the three kinds and their Inspector fields on this thread.
pub fn register_kinds() {
    register(zone_kind());
    register(occluder_kind());
    register(portal_kind());
    register(surface_kind());
    register_fields(
        "AudioSurface",
        vec![f(
            "surface",
            "string",
            json!("default"),
            "What it is made of, as a name the game looks up (metal, wood, stone, grass...).",
        )],
    );
    register_fields(
        "AudioPortal",
        vec![
            f(
                "enabled",
                "bool",
                json!(true),
                "A portal that is not enabled passes nothing.",
            ),
            f("gain", "f64", json!(1.0), "How much gets through (0 to 1)."),
            f(
                "radius",
                "f64",
                json!(0.25),
                "Metres short of the portal the far leg stops, so a door in it counts once.",
            ),
            f(
                "lowPass",
                "f64",
                json!(9000.0),
                "The low-pass cutoff (Hz) of sound that bends through.",
            ),
        ],
    );
    register(sensor_kind());
    register_fields(
        "AudioZone",
        vec![
            f(
                "enabled",
                "bool",
                json!(true),
                "A zone that is not enabled does nothing.",
            ),
            f(
                "shape",
                "enum:sphere|box",
                json!("sphere"),
                "The volume it covers.",
            ),
            f(
                "radius",
                "f64",
                json!(10.0),
                "Sphere: the outer radius (metres, times the object's scale). The effect is gone here.",
            ),
            f(
                "size",
                "v3",
                json!([10.0, 10.0, 10.0]),
                "Box: the outer size (metres, times the object's scale).",
            ),
            f(
                "innerFraction",
                "f64",
                json!(0.5),
                "How much of the volume has the full effect (0 to 1): the core. Between core and edge the curve applies.",
            ),
            f(
                "curve",
                "enum:linear|smooth|easeIn|easeOut",
                json!("smooth"),
                "How the effect grows from the edge to the core.",
            ),
            f(
                "invert",
                "bool",
                json!(false),
                "Strongest outside the volume instead of inside (nature outside a cave).",
            ),
            f(
                "affectsSources",
                "bool",
                json!(false),
                "Also colours the AudioSources (with effects) that stand in it, by where each stands.",
            ),
            f(
                "priority",
                "f64",
                json!(0.0),
                "Where zones overlap on a bus the higher one wins.",
            ),
            f(
                "lowPass",
                "f64",
                json!(22000.0),
                "Low-pass cutoff (Hz) at full strength; 22000 leaves it open.",
            ),
            f(
                "highPass",
                "f64",
                json!(0.0),
                "High-pass cutoff (Hz) at full strength; 0 leaves it open.",
            ),
            f(
                "reverb",
                "f64",
                json!(0.0),
                "Reverb wet level at full strength (0 to 1).",
            ),
            f(
                "reverbSize",
                "f64",
                json!(0.5),
                "How big the reverberating room sounds (0 to 1).",
            ),
            f(
                "reverbDamping",
                "f64",
                json!(0.5),
                "How fast the reverb loses its highs (0 to 1).",
            ),
            f(
                "echo",
                "f64",
                json!(0.0),
                "Echo wet level at full strength (0 to 1).",
            ),
            f(
                "volume",
                "f64",
                json!(1.0),
                "Gain of the buses it names at full strength.",
            ),
            f(
                "ambience",
                "sound",
                Value::Null,
                "A looping sound that fades in with the zone's strength.",
            ),
            f(
                "ambienceVolume",
                "f64",
                json!(1.0),
                "The ambience's volume at full strength.",
            ),
        ],
    );
    register_fields(
        "AudioOccluder",
        vec![
            f(
                "openness",
                "f64",
                json!(0.0),
                "0 shut (a wall, a closed door) to 1 open: how much sound gets through.",
            ),
            f(
                "absorption",
                "f64",
                json!(0.55),
                "Volume lost when shut (0 to 1).",
            ),
            f(
                "lowPass",
                "f64",
                json!(900.0),
                "The low-pass cutoff (Hz) a shut occluder leaves.",
            ),
        ],
    );
    register_fields(
        "SoundSensor",
        vec![
            f(
                "enabled",
                "bool",
                json!(true),
                "A sensor that is not enabled hears nothing.",
            ),
            f("sensitivity", "f64", json!(1.0), "Scales how far it hears."),
            f(
                "threshold",
                "f64",
                json!(0.03),
                "The quietest level (0 to 1) it reacts to.",
            ),
            f(
                "occlusion",
                "bool",
                json!(true),
                "Walls between it and a noise quieten the noise.",
            ),
        ],
    );
}
