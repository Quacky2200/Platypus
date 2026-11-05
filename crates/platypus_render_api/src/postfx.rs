// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// SPDX-License-Identifier: Apache-2.0

//! `PostProcess`: passes over a camera's finished picture. Scripts add built-in effects
//! (`PostProcess.create("vignette")`) or their own WGSL fragment function
//! (`PostProcess.createCustom(source, ["strength", "tint:color"])`), order them, turn them on
//! and off, aim them at a camera and set their parameters by name. Nothing here draws: the app
//! reads the enabled passes ([`post_passes`]) when the registry's [`post_version`] changes and
//! hands them to the renderer (`platypus_render::PostEffect`).
//!
//! The built-ins are the table [`EFFECTS`]: the WGSL of each (`postfx/*.wgsl`) and its
//! parameters with their defaults, in slot order. A custom shader gets the renderer's helpers
//! (`scene`, `scene_depth`, `view_uv`, ..) and one private variable per declared parameter.
//! Usage: `docs/postprocessing.md`.

use super::*;
use platypus_engine_api::camera::CameraRef;
use std::sync::Arc;

numbered_handle!(
    /// A post-process pass by number (1-based: 0 is "none").
    PostProcessRef
);

/// How many parameters a pass has (a `vec4` slot each); the renderer's `POST_PARAMS`.
pub const MAX_PARAMS: usize = 8;

/// How a parameter is typed in the shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamType {
    Float,
    Vec2,
    Vec3,
    Vec4,
}

/// One parameter of a built-in effect.
pub struct ParamDef {
    pub name: &'static str,
    pub ty: ParamType,
    pub default: [f32; 4],
}

/// A built-in effect.
pub struct EffectDef {
    pub name: &'static str,
    pub params: &'static [ParamDef],
    /// How many times it runs over the picture (blur: along x, then y).
    pub passes: u32,
    /// Reads the scene's depth and normals.
    pub depth: bool,
    /// The WGSL, joined (a shared part first).
    pub sources: &'static [&'static str],
}

const fn float(name: &'static str, default: f32) -> ParamDef {
    ParamDef {
        name,
        ty: ParamType::Float,
        default: [default, 0.0, 0.0, 0.0],
    }
}

const fn vec2(name: &'static str, x: f32, y: f32) -> ParamDef {
    ParamDef {
        name,
        ty: ParamType::Vec2,
        default: [x, y, 0.0, 0.0],
    }
}

const fn vec3(name: &'static str, x: f32, y: f32, z: f32) -> ParamDef {
    ParamDef {
        name,
        ty: ParamType::Vec3,
        default: [x, y, z, 0.0],
    }
}

const fn vec4(name: &'static str, x: f32, y: f32, z: f32, w: f32) -> ParamDef {
    ParamDef {
        name,
        ty: ParamType::Vec4,
        default: [x, y, z, w],
    }
}

/// The built-in effects, in the order of the script's `PostEffect` names.
pub const EFFECTS: &[EffectDef] = &[
    EffectDef {
        name: "chromaticAberration",
        params: &[float("amount", 0.015)],
        passes: 1,
        depth: false,
        sources: &[include_str!("postfx/chromatic_aberration.wgsl")],
    },
    EffectDef {
        name: "blur",
        params: &[float("radius", 6.0)],
        passes: 2,
        depth: false,
        sources: &[include_str!("postfx/blur.wgsl")],
    },
    EffectDef {
        name: "depthOfField",
        params: &[
            float("focus_distance", 8.0),
            float("aperture", 2.0),
            float("focal_length", 50.0),
            float("max_blur", 16.0),
            float("blades", 0.0),
            float("highlight", 1.0),
        ],
        passes: 1,
        depth: true,
        sources: &[
            include_str!("postfx/focus_gather.wgsl"),
            include_str!("postfx/depth_of_field.wgsl"),
        ],
    },
    EffectDef {
        name: "tiltShift",
        params: &[
            float("focus", 0.5),
            float("width", 0.12),
            float("max_blur", 14.0),
            float("angle", 0.0),
            float("blades", 0.0),
            float("highlight", 1.0),
        ],
        passes: 1,
        depth: false,
        sources: &[
            include_str!("postfx/focus_gather.wgsl"),
            include_str!("postfx/tilt_shift.wgsl"),
        ],
    },
    EffectDef {
        name: "outline",
        params: &[
            float("thickness", 1.5),
            vec4("ink", 0.01, 0.008, 0.006, 1.0),
            float("depth_threshold", 0.04),
            float("normal_threshold", 0.5),
            float("fade_distance", 0.0),
            float("color_threshold", 0.0),
        ],
        passes: 1,
        depth: true,
        sources: &[include_str!("postfx/outline.wgsl")],
    },
    EffectDef {
        name: "vignette",
        params: &[
            float("intensity", 0.6),
            float("smoothness", 0.5),
            float("roundness", 1.0),
            vec3("tint", 0.0, 0.0, 0.0),
        ],
        passes: 1,
        depth: false,
        sources: &[include_str!("postfx/vignette.wgsl")],
    },
    EffectDef {
        name: "pixelate",
        params: &[float("size", 6.0), float("levels", 0.0)],
        passes: 1,
        depth: false,
        sources: &[include_str!("postfx/pixelate.wgsl")],
    },
    EffectDef {
        name: "filmGrain",
        params: &[
            float("intensity", 0.15),
            float("size", 1.5),
            float("response", 0.6),
            float("colored", 0.3),
        ],
        passes: 1,
        depth: false,
        sources: &[include_str!("postfx/film_grain.wgsl")],
    },
    EffectDef {
        name: "colorGrading",
        params: &[
            float("exposure", 0.0),
            float("contrast", 1.0),
            float("saturation", 1.0),
            float("hue_shift", 0.0),
            vec2("balance", 0.0, 0.0),
            vec3("lift", 0.0, 0.0, 0.0),
            vec3("gamma", 1.0, 1.0, 1.0),
            vec3("gain", 1.0, 1.0, 1.0),
        ],
        passes: 1,
        depth: false,
        sources: &[include_str!("postfx/color_grading.wgsl")],
    },
    EffectDef {
        name: "crt",
        params: &[
            float("curvature", 0.3),
            float("scanlines", 0.35),
            float("mask", 0.25),
            float("line_height", 3.0),
        ],
        passes: 1,
        depth: false,
        sources: &[include_str!("postfx/crt.wgsl")],
    },
    EffectDef {
        name: "posterize",
        params: &[float("levels", 4.0), float("softness", 0.12)],
        passes: 1,
        depth: false,
        sources: &[include_str!("postfx/posterize.wgsl")],
    },
];

/// The built-in effects by name, in the order of [`EFFECTS`] (the script's `PostEffect`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostEffectKind {
    ChromaticAberration,
    Blur,
    DepthOfField,
    TiltShift,
    Outline,
    Vignette,
    Pixelate,
    FilmGrain,
    ColorGrading,
    Crt,
    Posterize,
}

/// A built-in effect's index in [`EFFECTS`], or none for a name that is not one.
pub fn effect_named(name: &str) -> Option<usize> {
    EFFECTS.iter().position(|e| e.name == name)
}

/// A shader as the renderer needs it: the same source and parameters make the same shader.
#[derive(Debug, PartialEq)]
pub struct ShaderDef {
    /// For the log: the effect's name, or `custom`.
    pub name: String,
    pub source: String,
    pub params: Vec<(String, ParamType)>,
    pub passes: u32,
    pub depth: bool,
}

/// An enabled pass for the renderer.
#[derive(Clone, Debug)]
pub struct PostPass {
    pub id: u32,
    pub shader: Arc<ShaderDef>,
    /// A `vec4` per parameter slot.
    pub values: [[f32; 4]; MAX_PARAMS],
    /// The camera it applies to (`CameraRef` number), or 0 for every camera.
    pub camera: u32,
}

struct Pass {
    shader: Arc<ShaderDef>,
    values: [[f32; 4]; MAX_PARAMS],
    enabled: bool,
    order: f64,
    camera: u32,
}

#[derive(Default)]
struct State {
    passes: Vec<Option<Pass>>,
    version: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn with_pass<R>(p: PostProcessRef, f: impl FnOnce(&mut Pass) -> R) -> Option<R> {
    st(|s| {
        let r = s
            .passes
            .get_mut((p.0 as usize).checked_sub(1)?)?
            .as_mut()
            .map(f);
        s.version += 1;
        r
    })
}

fn read_pass<R>(p: PostProcessRef, f: impl FnOnce(&Pass) -> R) -> Option<R> {
    st(|s| {
        s.passes
            .get((p.0 as usize).checked_sub(1)?)?
            .as_ref()
            .map(f)
    })
}

/// Forgets every pass (a test, or a reload).
pub(crate) fn reset_postfx() {
    st(|s| {
        s.passes.clear();
        s.version += 1;
    });
}

/// Counts changes to the passes: the app sends them on while it stays the same.
pub fn post_version() -> u64 {
    st(|s| s.version)
}

/// The enabled passes in drawing order (by `order`, then as they were made).
pub fn post_passes() -> Vec<PostPass> {
    st(|s| {
        let mut all: Vec<(f64, u32, PostPass)> = s
            .passes
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let p = p.as_ref().filter(|p| p.enabled)?;
                let id = i as u32 + 1;
                Some((
                    p.order,
                    id,
                    PostPass {
                        id,
                        shader: p.shader.clone(),
                        values: p.values,
                        camera: p.camera,
                    },
                ))
            })
            .collect();
        all.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        all.into_iter().map(|(_, _, p)| p).collect()
    })
}

fn warn(message: &str) {
    platypus_engine_api::log::log_api::warning(message);
}

fn add(shader: Arc<ShaderDef>, values: [[f32; 4]; MAX_PARAMS]) -> PostProcessRef {
    st(|s| {
        s.passes.push(Some(Pass {
            shader,
            values,
            enabled: true,
            order: 0.0,
            camera: 0,
        }));
        s.version += 1;
        PostProcessRef(s.passes.len() as u32)
    })
}

thread_local! {
    /// The built-ins' shaders, made once so their passes share them.
    static BUILTINS: RefCell<Vec<Option<Arc<ShaderDef>>>> = const { RefCell::new(Vec::new()) };
}

fn builtin_shader(index: usize) -> Arc<ShaderDef> {
    BUILTINS.with(|b| {
        let mut b = b.borrow_mut();
        b.resize(EFFECTS.len(), None);
        b[index]
            .get_or_insert_with(|| {
                let e = &EFFECTS[index];
                Arc::new(ShaderDef {
                    name: e.name.to_string(),
                    source: e.sources.join("\n"),
                    params: e
                        .params
                        .iter()
                        .map(|p| (p.name.to_string(), p.ty))
                        .collect(),
                    passes: e.passes,
                    depth: e.depth,
                })
            })
            .clone()
    })
}

fn create_builtin(index: usize) -> PostProcessRef {
    let mut values = [[0.0; 4]; MAX_PARAMS];
    for (slot, p) in EFFECTS[index].params.iter().enumerate() {
        values[slot] = p.default;
    }
    add(builtin_shader(index), values)
}

/// `name` or `name:type` (`float`, `vec2`, `vec3`, `vec4`, `color`): a WGSL identifier.
fn parse_param(spec: &str) -> Option<(String, ParamType)> {
    let (name, ty) = spec.split_once(':').unwrap_or((spec, "float"));
    let name = name.trim();
    let valid = !name.is_empty()
        && name.len() <= 40
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with("__");
    let ty = match ty.trim() {
        "float" | "f32" => ParamType::Float,
        "vec2" => ParamType::Vec2,
        "vec3" => ParamType::Vec3,
        "vec4" | "color" => ParamType::Vec4,
        _ => return None,
    };
    valid.then(|| (name.to_string(), ty))
}

fn create_custom(source: &str, parameters: &[String]) -> PostProcessRef {
    let mut params: Vec<(String, ParamType)> = Vec::new();
    for spec in parameters {
        match parse_param(spec) {
            Some(p) if params.len() < MAX_PARAMS && !params.iter().any(|q| q.0 == p.0) => {
                params.push(p)
            }
            Some(p) => warn(&format!(
                "PostProcess: parameter '{}' is repeated or past the {MAX_PARAMS} allowed, left out",
                p.0
            )),
            None => warn(&format!(
                "PostProcess: '{spec}' is not a parameter (use name or name:float|vec2|vec3|vec4|color)"
            )),
        }
    }
    let depth = source.contains("scene_depth") || source.contains("scene_normal");
    let shader = Arc::new(ShaderDef {
        name: "custom".to_string(),
        source: source.to_string(),
        params,
        passes: 1,
        depth,
    });
    add(shader, [[0.0; 4]; MAX_PARAMS])
}

fn slot_of(p: &Pass, name: &str) -> Option<usize> {
    p.shader.params.iter().position(|(n, _)| n == name)
}

/// Sets a parameter's slot from `v`; a name the pass does not have is reported.
fn set_param(p: PostProcessRef, name: &str, v: [f32; 4]) {
    let found = with_pass(p, |pass| {
        slot_of(pass, name).map(|slot| pass.values[slot] = v)
    });
    if found == Some(None) {
        warn(&format!(
            "PostProcess: no parameter '{name}' (it has: {})",
            parameter_names(p).join(", ")
        ));
    }
}

fn parameter_names(p: PostProcessRef) -> Vec<String> {
    read_pass(p, |pass| {
        pass.shader.params.iter().map(|(n, _)| n.clone()).collect()
    })
    .unwrap_or_default()
}

#[ts_api(
    namespace = "PostProcess", handles(PostProcess = PostProcessRef, Camera = CameraRef),
    values(Color = Rgba [r: f64, g: f64, b: f64, a: f64]),
    enums(
        /// The built-in post-process effects.
        PostEffect = PostEffectKind [ChromaticAberration, Blur, DepthOfField, TiltShift, Outline,
            Vignette, Pixelate, FilmGrain, ColorGrading, Crt, Posterize]
    )
)]
pub mod post_process_api {
    use super::*;

    /// A pass over a camera's finished picture, after bloom, exposure, tonemapping and
    /// anti-aliasing: a built-in effect or a WGSL function of your own, with parameters you set by
    /// name. Passes run in `order` (then as they were made) over the cameras they are aimed at.
    impl PostProcessRef {
        /// Adds a built-in effect with its defaults, on (every camera, order 0). Its
        /// `parameters` lists what `setFloat` / `setColor` / `setVector` take.
        pub fn create(effect: PostEffectKind) -> PostProcessRef {
            super::create_builtin(effect as usize)
        }

        /// Adds your own effect. `source` is WGSL that defines
        /// `fn effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32>`: `color` is the picture at
        /// `uv` (0..1 across the screen) and the result replaces it. Helpers: `scene(uv)` (the
        /// picture, bilinear), `scene_px(pixel)`, `scene_depth(uv)` (distance along the view in
        /// metres) and `scene_normal(uv)` (world space; zero on the background), `view_uv(uv)`
        /// (0..1 across the camera's part of the screen), `pixel()` (one pixel in uv), `aspect()`,
        /// `post_time()` (seconds), `frame_index()`, `pass_index()`, `luma(c)`, `hash21(p)`,
        /// `to_gamma(c)` / `to_linear(c)`. `parameters` declares up to eight values you set
        /// by name, each a private variable of the shader: `"strength"` (a float) or
        /// `"name:float|vec2|vec3|vec4|color"`. A shader that does not compile is logged and
        /// skipped.
        #[optional(parameters)]
        pub fn create_custom(source: &str, parameters: &[String]) -> PostProcessRef {
            super::create_custom(source, parameters)
        }

        /// Every pass that has not been destroyed, in the order they were made.
        pub fn all() -> Vec<PostProcessRef> {
            super::st(|s| {
                s.passes
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_some())
                    .map(|(i, _)| PostProcessRef(i as u32 + 1))
                    .collect()
            })
        }

        /// Removes the pass.
        pub fn destroy(self) {
            super::st(|s| {
                if let Some(slot) = s.passes.get_mut((self.0 as usize).wrapping_sub(1)) {
                    *slot = None;
                    s.version += 1;
                }
            });
        }

        /// False once destroyed.
        #[property]
        pub fn is_valid(self) -> bool {
            super::read_pass(self, |_| ()).is_some()
        }

        /// The effect's name (`custom` for your own).
        #[property]
        pub fn effect(self) -> String {
            super::read_pass(self, |p| p.shader.name.clone()).unwrap_or_default()
        }

        /// Whether it runs (on by default).
        #[property]
        pub fn enabled(self) -> bool {
            super::read_pass(self, |p| p.enabled).unwrap_or(false)
        }

        pub fn set_enabled(self, on: bool) {
            super::with_pass(self, |p| p.enabled = on);
        }

        /// Where it runs among the passes: lower first, ties in the order they were made.
        #[property]
        pub fn order(self) -> f64 {
            super::read_pass(self, |p| p.order).unwrap_or(0.0)
        }

        pub fn set_order(self, order: f64) {
            super::with_pass(self, |p| p.order = order);
        }

        /// The only camera it applies to, or none for every camera.
        #[property]
        #[nullable]
        pub fn camera(self) -> CameraRef {
            CameraRef(super::read_pass(self, |p| p.camera).unwrap_or(0))
        }

        pub fn set_camera(self, camera: CameraRef) {
            super::with_pass(self, |p| p.camera = camera.0);
        }

        /// How many times a custom shader runs over the picture (1 to 4; `pass_index()` says
        /// which). Built-in effects have their own.
        #[property]
        pub fn passes(self) -> f64 {
            super::read_pass(self, |p| f64::from(p.shader.passes)).unwrap_or(1.0)
        }

        pub fn set_passes(self, passes: f64) {
            super::with_pass(self, |p| {
                if p.shader.name == "custom" {
                    p.shader = Arc::new(ShaderDef {
                        name: p.shader.name.clone(),
                        source: p.shader.source.clone(),
                        params: p.shader.params.clone(),
                        passes: (passes as u32).clamp(1, 4),
                        depth: p.shader.depth,
                    });
                }
            });
        }

        /// Whether the shader reads `scene_depth` / `scene_normal` (a custom shader is checked
        /// for them when made); turn it on if the shader gets depth through a helper of its own.
        #[property]
        pub fn uses_depth(self) -> bool {
            super::read_pass(self, |p| p.shader.depth).unwrap_or(false)
        }

        pub fn set_uses_depth(self, on: bool) {
            super::with_pass(self, |p| {
                if p.shader.depth != on {
                    p.shader = Arc::new(ShaderDef {
                        name: p.shader.name.clone(),
                        source: p.shader.source.clone(),
                        params: p.shader.params.clone(),
                        passes: p.shader.passes,
                        depth: on,
                    });
                }
            });
        }

        /// The names of its parameters, in order.
        #[property]
        pub fn parameters(self) -> Vec<String> {
            super::parameter_names(self)
        }

        /// Sets a parameter (a vector's first component). A name it does not have is logged.
        pub fn set_float(self, name: &str, value: f64) {
            super::set_param(self, name, [value as f32, 0.0, 0.0, 0.0]);
        }

        /// A parameter's first component.
        pub fn get_float(self, name: &str) -> f64 {
            super::read_pass(self, |p| {
                super::slot_of(p, name).map_or(0.0, |s| f64::from(p.values[s][0]))
            })
            .unwrap_or(0.0)
        }

        /// Sets a parameter from a colour (`vec4`, or the first components of a smaller one).
        pub fn set_color(self, name: &str, color: Rgba) {
            super::set_param(
                self,
                name,
                [
                    color.r as f32,
                    color.g as f32,
                    color.b as f32,
                    color.a as f32,
                ],
            );
        }

        /// A parameter as a colour (a `vec2` or `vec3` fills the first components).
        pub fn get_color(self, name: &str) -> Rgba {
            let v = super::read_pass(self, |p| {
                super::slot_of(p, name).map_or([0.0; 4], |s| p.values[s])
            })
            .unwrap_or([0.0; 4]);
            Rgba {
                r: f64::from(v[0]),
                g: f64::from(v[1]),
                b: f64::from(v[2]),
                a: f64::from(v[3]),
            }
        }

        /// Sets a `vec2`, `vec3` or `vec4` parameter; what is left out is 0.
        #[optional(y, z, w)]
        pub fn set_vector(self, name: &str, x: f64, y: f64, z: f64, w: f64) {
            super::set_param(self, name, [x as f32, y as f32, z as f32, w as f32]);
        }
    }
}

pub(crate) fn postfx_manifests() -> Vec<String> {
    vec![post_process_api::MANIFEST.to_json()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_names_match_the_table() {
        // M7-FX-01
        let names: Vec<&str> = post_process_api::POST_EFFECT_NAMES.to_vec();
        let table: Vec<&str> = EFFECTS.iter().map(|e| e.name).collect();
        assert_eq!(names, table);
    }

    #[test]
    fn builtin_parameters_fit_their_slots() {
        // M7-FX-01
        for e in EFFECTS {
            assert!(e.params.len() <= MAX_PARAMS, "{}", e.name);
            assert!(
                e.sources.iter().any(|s| s.contains("fn effect(")),
                "{}",
                e.name
            );
            for p in e.params {
                assert!(parse_param(p.name).is_some(), "{} {}", e.name, p.name);
            }
        }
    }

    #[test]
    fn passes_are_ordered_enabled_and_aimed() {
        // M7-FX-02
        reset_postfx();
        let a = create_builtin(effect_named("vignette").unwrap());
        let b = create_builtin(effect_named("blur").unwrap());
        let c = create_builtin(effect_named("pixelate").unwrap());
        with_pass(a, |p| p.order = 5.0);
        with_pass(c, |p| p.enabled = false);
        with_pass(b, |p| p.camera = 3);
        let passes = post_passes();
        let ids: Vec<u32> = passes.iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![b.0, a.0]);
        assert_eq!(passes[0].camera, 3);
        let v = post_version();
        a.set_float("intensity", 0.25);
        assert!(post_version() > v);
        assert_eq!(post_passes()[1].values[0][0], 0.25);
        reset_postfx();
    }

    #[test]
    fn custom_shaders_declare_typed_parameters() {
        // M7-FX-03
        reset_postfx();
        let p = create_custom(
            "fn effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> { return color * strength * tint; }",
            &[
                "strength".into(),
                "tint:color".into(),
                "1bad".into(),
                "strength".into(),
            ],
        );
        assert_eq!(parameter_names(p), vec!["strength", "tint"]);
        set_param(p, "tint", [1.0, 0.5, 0.25, 1.0]);
        set_param(p, "strength", [2.0, 0.0, 0.0, 0.0]);
        let pass = &post_passes()[0];
        assert_eq!(pass.values[0][0], 2.0);
        assert_eq!(pass.values[1], [1.0, 0.5, 0.25, 1.0]);
        assert!(!pass.shader.depth);
        let d = create_custom(
            "fn effect(uv: vec2<f32>, c: vec4<f32>) -> vec4<f32> { return vec4(scene_depth(uv)); }",
            &[],
        );
        assert!(read_pass(d, |p| p.shader.depth).unwrap());
        reset_postfx();
    }
}
