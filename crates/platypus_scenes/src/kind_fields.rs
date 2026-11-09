// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What the fields of the hand-written engine kinds are, for the editor's Inspector
//! (`platypus_kinds::register_fields`): Camera, Animation, Animator, AudioSource, AudioListener and
//! SplineAnimate, under the names their kinds save, with types, enum choices and docs. (The
//! generated render components register theirs from `platypus_render_api`'s build script, the
//! physics ones in `platypus_physics_api`.)

use platypus_kinds::{FieldDoc, register_fields};
use serde_json::{Value, json};

fn f(name: &str, ty: &str, default: Value, doc: &str) -> FieldDoc {
    FieldDoc {
        name: name.into(),
        ty: ty.into(),
        default,
        doc: doc.into(),
    }
}

/// Registers the schemas (after the kinds, in `register_engine_kinds`).
pub fn register() {
    register_fields(
        "Camera",
        vec![
            f(
                "fieldOfView",
                "f64",
                json!(60.0),
                "The vertical view angle, in degrees.",
            ),
            f(
                "nearClipPlane",
                "f64",
                json!(0.3),
                "Nothing nearer is drawn.",
            ),
            f(
                "farClipPlane",
                "f64",
                json!(1000.0),
                "Nothing farther is drawn.",
            ),
            f(
                "backgroundColor",
                "color",
                Value::Null,
                "The colour it clears to (solid colour).",
            ),
            f(
                "depth",
                "f64",
                json!(0.0),
                "Cameras draw in order of depth, lowest first.",
            ),
            f("orthographic", "bool", json!(false), "No perspective."),
            f(
                "orthographicSize",
                "f64",
                json!(5.0),
                "Half the view's height when orthographic.",
            ),
            f(
                "clearFlags",
                "enum:skybox|solidColor",
                json!("skybox"),
                "What it draws behind everything.",
            ),
            f(
                "cullingMask",
                "mask",
                json!(-1),
                "The layers it draws (a bit per layer).",
            ),
            f(
                "layerCullSpherical",
                "bool",
                json!(false),
                "Per-layer cull distances are spheres.",
            ),
            f(
                "antialiasing",
                "enum:none|fxaa",
                json!("none"),
                "Smoothing of edges.",
            ),
            f(
                "targetTexture",
                "texture",
                Value::Null,
                "Draws into this texture instead of the screen.",
            ),
        ],
    );
    register_fields(
        "Animation",
        vec![
            f("clip", "clip", Value::Null, "The clip it plays by default."),
            f(
                "playAutomatically",
                "bool",
                json!(true),
                "Plays the clip when it starts.",
            ),
            f(
                "cullingType",
                "enum:alwaysAnimate|basedOnRenderers",
                json!("alwaysAnimate"),
                "Whether it stops while its renderers are off screen.",
            ),
        ],
    );
    register_fields(
        "Animator",
        vec![
            f(
                "controller",
                "controller",
                Value::Null,
                "The state machine it runs.",
            ),
            f("speed", "f64", json!(1.0), "How fast it plays."),
            f(
                "cullingMode",
                "enum:alwaysAnimate|cullUpdateTransforms|cullCompletely",
                json!("alwaysAnimate"),
                "What it does while off screen.",
            ),
            f("enabled", "bool", json!(true), "Runs each frame."),
        ],
    );
    register_fields(
        "AudioSource",
        vec![
            f(
                "clip",
                "sound",
                Value::Null,
                "The sound it plays (a project file).",
            ),
            f("playOnAwake", "bool", json!(true), "Plays when it starts."),
            f("loop", "bool", json!(false), "Starts again at the end."),
            f("mute", "bool", json!(false), "Silent, but still playing."),
            f("volume", "f64", json!(1.0), "0 to 1."),
            f(
                "pitch",
                "f64",
                json!(1.0),
                "1 is as recorded; 2 an octave up.",
            ),
            f(
                "panStereo",
                "f64",
                json!(0.0),
                "-1 left to 1 right (2D sound).",
            ),
            f("spatialBlend", "f64", json!(0.0), "0 is 2D, 1 is fully 3D."),
            f(
                "minDistance",
                "f64",
                json!(1.0),
                "Full volume within this distance (3D).",
            ),
            f(
                "maxDistance",
                "f64",
                json!(500.0),
                "Where it stops getting quieter (3D).",
            ),
            f(
                "effects",
                "bool",
                json!(false),
                "Gives its emitter the full effect chain (high-pass, reverb, echo); set before it plays.",
            ),
            f(
                "dopplerLevel",
                "f64",
                json!(0.0),
                "How much pitch follows its speed towards or away from the listener (0 off, 1 physical).",
            ),
            f(
                "loudness",
                "f64",
                json!(0.0),
                "Metres a SoundSensor hears it from while it plays; 0 means sensors do not hear it.",
            ),
            f(
                "occlusion",
                "f64",
                json!(1.0),
                "How much walls between it and the listener muffle it (0 to 1).",
            ),
            f(
                "directivity",
                "enum:omni|cardioid|supercardioid|hypercardioid|figureEight|ultraDirectional",
                json!("omni"),
                "Its polar pattern: how it sounds from different directions round it (front is the object's -z).",
            ),
            f(
                "directivityStrength",
                "f64",
                json!(1.0),
                "How much of the pattern applies (0 all round, 1 all of it).",
            ),
            f(
                "coneInnerAngle",
                "f64",
                json!(360.0),
                "With directivity omni: the full angle (degrees) it is at full strength in front; 360 is all round.",
            ),
            f(
                "coneOuterAngle",
                "f64",
                json!(360.0),
                "Beyond this full angle it is coneOuterGain as loud and low-passed.",
            ),
            f(
                "coneOuterGain",
                "f64",
                json!(0.3),
                "Linear gain outside the outer cone.",
            ),
            f(
                "coneOuterLowPass",
                "f64",
                json!(5000.0),
                "Low-pass cutoff (Hz) outside the outer cone and in a pattern's quiet directions.",
            ),
            f(
                "airAbsorption",
                "f64",
                json!(1.0),
                "How much of Acoustics.airAbsorption applies to it (distance takes the highs off).",
            ),
        ],
    );
    register_fields("AudioListener", Vec::new());
    register_fields(
        "SplineAnimate",
        vec![
            f("spline", "spline", Value::Null, "The path it moves along."),
            f(
                "container",
                "object",
                Value::Null,
                "The object whose transform the spline is in.",
            ),
            f("duration", "f64", json!(1.0), "Seconds for the whole path."),
            f(
                "loopMode",
                "enum:loop|pingPong|once",
                json!("loop"),
                "What happens at the end.",
            ),
            f(
                "alignment",
                "enum:none|path",
                json!("path"),
                "Whether it turns to face along the path.",
            ),
            f("playing", "bool", json!(true), "Moving now."),
        ],
    );
}
