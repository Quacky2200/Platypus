// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `RenderSettings`: scene-wide lighting (ambient light, the procedural sky, bloom, tonemapping),
//! as `rendering.ts`'s `RenderSettings` object has it. The app reads [`settings`] when
//! [`version`] changed. Scene files save and load it under `renderSettings` (the active scene).

use super::*;

/// A procedural sky: gradients from the zenith to the horizon to the ground, and a sun disc.
#[derive(Clone, Debug, PartialEq)]
pub struct Sky {
    pub zenith: Rgba,
    pub horizon: Rgba,
    pub ground: Rgba,
    /// Towards the sun; none for no sun disc.
    pub sun_direction: Option<V3>,
    pub sun_color: Rgba,
    /// The disc's size in degrees.
    pub sun_size: f64,
}

impl Default for Sky {
    fn default() -> Self {
        let c = |r, g, b| Rgba { r, g, b, a: 1.0 };
        Sky {
            zenith: c(0.16, 0.32, 0.62),
            horizon: c(0.62, 0.72, 0.82),
            ground: c(0.22, 0.2, 0.18),
            sun_direction: Some(V3 {
                x: 0.4,
                y: 0.6,
                z: 0.3,
            }),
            sun_color: c(20.0, 18.0, 15.0),
            sun_size: 1.5,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bloom {
    /// Linear brightness where glowing starts (1: only what is brighter than white).
    pub threshold: f64,
    pub intensity: f64,
    /// 0..1: how far it spreads.
    pub scatter: f64,
}

/// Fog lying low (Unreal's exponential height fog).
#[derive(Clone, Debug, PartialEq)]
pub struct HeightFog {
    pub density: f64,
    pub base_height: f64,
    pub falloff: f64,
}

impl Default for HeightFog {
    fn default() -> Self {
        HeightFog {
            density: 0.05,
            base_height: 0.0,
            falloff: 0.2,
        }
    }
}

/// Light shafts in the fog.
#[derive(Clone, Debug, PartialEq)]
pub struct Volumetric {
    pub intensity: f64,
    pub anisotropy: f64,
    pub steps: f64,
    pub distance: f64,
    pub albedo: Rgba,
    pub sharp_distance: f64,
    pub sharp_samples: f64,
    /// `low`, `medium`, `high` or `ultra`.
    pub resolution: String,
    /// `none`, `reprojection`, `gaussian` or `both`.
    pub denoising: String,
    pub history_weight: f64,
    pub samples: f64,
    pub motion_samples: f64,
}

impl Default for Volumetric {
    fn default() -> Self {
        Volumetric {
            intensity: 1.0,
            anisotropy: 0.4,
            steps: 4.0,
            distance: 50.0,
            albedo: Rgba {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            sharp_distance: 0.0,
            sharp_samples: 32.0,
            resolution: "high".to_string(),
            denoising: "both".to_string(),
            history_weight: 0.9,
            samples: 1.0,
            motion_samples: 2.0,
        }
    }
}

/// Distance fog and its extras (`RenderSettings.fog...`).
#[derive(Clone, Debug, PartialEq)]
pub struct Fog {
    pub enabled: bool,
    /// `linear`, `exponential` or `exponentialSquared`.
    pub mode: String,
    pub color: Rgba,
    pub density: f64,
    pub start: f64,
    pub end: f64,
    /// Skip drawing what lies wholly past where the fog hides everything.
    pub culling: bool,
    pub height: Option<HeightFog>,
    pub volumetric: Option<Volumetric>,
}

impl Default for Fog {
    fn default() -> Self {
        Fog {
            enabled: false,
            mode: "exponentialSquared".to_string(),
            color: Rgba {
                r: 0.5,
                g: 0.5,
                b: 0.5,
                a: 1.0,
            },
            density: 0.01,
            start: 0.0,
            end: 300.0,
            culling: true,
            height: None,
            volumetric: None,
        }
    }
}

/// A deck of cloud drawn with the sky (HDRP's cloud layer), behind the fog.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudLayer {
    /// The deck's height in metres.
    pub altitude: f64,
    /// About how wide a cloud is, metres.
    pub size: f64,
    /// How much of the sky is cloud, 0 to 1.
    pub coverage: f64,
    pub opacity: f64,
    /// How soft the edges are, 0 (hard) to 1.
    pub softness: f64,
    pub tint: Rgba,
    /// m/s, and the way it blows in degrees from +x towards +z.
    pub wind_speed: f64,
    pub wind_direction: f64,
    /// Over about this distance along a ray the clouds fade into the sky, metres.
    pub fade_distance: f64,
}

impl Default for CloudLayer {
    fn default() -> Self {
        CloudLayer {
            altitude: 1500.0,
            size: 2000.0,
            coverage: 0.5,
            opacity: 1.0,
            softness: 0.3,
            tint: Rgba {
                r: 0.8,
                g: 0.8,
                b: 0.82,
                a: 1.0,
            },
            wind_speed: 0.0,
            wind_direction: 0.0,
            fade_distance: 20000.0,
        }
    }
}

/// Auto exposure (HDRP's Automatic mode).
#[derive(Clone, Debug, PartialEq)]
pub struct AutoExposure {
    pub min_ev: f64,
    pub max_ev: f64,
    pub compensation: f64,
    pub speed_dark_to_light: f64,
    pub speed_light_to_dark: f64,
    /// `average`, `centerWeighted` or `spot`.
    pub metering: String,
    pub low_percent: f64,
    pub high_percent: f64,
    pub hold: bool,
    pub dark_adaptation: f64,
    pub dark_below: f64,
    /// Up to 8 points of [metered EV100, stops].
    pub curve: Vec<(f64, f64)>,
}

impl Default for AutoExposure {
    fn default() -> Self {
        AutoExposure {
            min_ev: -10.0,
            max_ev: 20.0,
            compensation: 0.0,
            speed_dark_to_light: 3.0,
            speed_light_to_dark: 3.0,
            metering: "centerWeighted".to_string(),
            low_percent: 0.0,
            high_percent: 100.0,
            hold: false,
            dark_adaptation: 1.0,
            dark_below: 5.0,
            curve: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderSettingsData {
    /// Light everything gets regardless of lights (linear).
    pub ambient_light: Rgba,
    /// `skybox` (with a sky) or `color` (`ambient_light`).
    pub ambient_mode: String,
    pub reflection_intensity: f64,
    pub skybox: Option<Sky>,
    pub bloom: Option<Bloom>,
    /// `none`, `neutral` or `aces`.
    pub tonemapping: String,
    /// Stops, before tonemapping.
    pub post_exposure: f64,
    pub fog: Fog,
    /// The cloud layer (none: no clouds).
    pub cloud_layer: Option<CloudLayer>,
    /// Adapts the exposure to the view (none: off).
    pub auto_exposure: Option<AutoExposure>,
    /// The manual exposure in EV100 (none: light as it is).
    pub fixed_exposure: Option<f64>,
    /// Seconds auto exposure takes to blend in and out.
    pub exposure_transition: f64,
}

impl Default for RenderSettingsData {
    fn default() -> Self {
        RenderSettingsData {
            ambient_light: Rgba {
                r: 0.2,
                g: 0.2,
                b: 0.22,
                a: 1.0,
            },
            ambient_mode: "skybox".to_string(),
            reflection_intensity: 1.0,
            skybox: None,
            bloom: None,
            tonemapping: "none".to_string(),
            post_exposure: 0.0,
            fog: Fog::default(),
            cloud_layer: None,
            auto_exposure: None,
            fixed_exposure: None,
            exposure_transition: 0.0,
        }
    }
}

thread_local! {
    static SETTINGS: RefCell<(RenderSettingsData, u64)> = RefCell::new((RenderSettingsData::default(), 0));
}

/// The settings now.
pub fn settings() -> RenderSettingsData {
    SETTINGS.with(|s| s.borrow().0.clone())
}

/// A number that grows with every change, to tell the renderer to look again.
pub fn version() -> u64 {
    SETTINGS.with(|s| s.borrow().1)
}

fn change<R>(f: impl FnOnce(&mut RenderSettingsData) -> R) -> R {
    SETTINGS.with(|s| {
        let mut s = s.borrow_mut();
        s.1 += 1;
        f(&mut s.0)
    })
}

fn read<R>(f: impl FnOnce(&RenderSettingsData) -> R) -> R {
    SETTINGS.with(|s| f(&s.borrow().0))
}

pub(crate) fn reset_render_settings() {
    SETTINGS.with(|s| *s.borrow_mut() = (RenderSettingsData::default(), 0));
    set_render_stats(Stats::default());
}

fn color_json(c: Rgba) -> Value {
    json!({ "$type": "Color", "value": [c.r, c.g, c.b, c.a] })
}

/// The settings as a scene file holds them (`renderSettings`).
pub fn save_render_settings() -> Value {
    let s = settings();
    let sky = s.skybox.as_ref().map_or(Value::Null, |k| {
        json!({
            "zenith": color_json(k.zenith),
            "horizon": color_json(k.horizon),
            "ground": color_json(k.ground),
            "sunDirection": k.sun_direction.map_or(Value::Null, |d| json!({ "$type": "Vector3", "value": [d.x, d.y, d.z] })),
            "sunColor": color_json(k.sun_color),
            "sunSize": k.sun_size,
        })
    });
    let bloom = s.bloom.as_ref().map_or(
        Value::Null,
        |b| json!({ "threshold": b.threshold, "intensity": b.intensity, "scatter": b.scatter }),
    );
    let height = s.fog.height.as_ref().map_or(
        Value::Null,
        |h| json!({ "density": h.density, "baseHeight": h.base_height, "falloff": h.falloff }),
    );
    let clouds = s.cloud_layer.as_ref().map_or(Value::Null, |c| {
        json!({
            "altitude": c.altitude,
            "size": c.size,
            "coverage": c.coverage,
            "opacity": c.opacity,
            "softness": c.softness,
            "tint": color_json(c.tint),
            "windSpeed": c.wind_speed,
            "windDirection": c.wind_direction,
            "fadeDistance": c.fade_distance,
        })
    });
    let volumetric = s.fog.volumetric.as_ref().map_or(Value::Null, |v| {
        json!({
            "intensity": v.intensity,
            "anisotropy": v.anisotropy,
            "steps": v.steps,
            "distance": v.distance,
            "albedo": color_json(v.albedo),
            "sharpDistance": v.sharp_distance,
            "sharpSamples": v.sharp_samples,
            "resolution": v.resolution,
            "denoising": v.denoising,
            "historyWeight": v.history_weight,
            "samples": v.samples,
            "motionSamples": v.motion_samples,
        })
    });
    json!({
        "ambientLight": color_json(s.ambient_light),
        "ambientMode": s.ambient_mode,
        "reflectionIntensity": s.reflection_intensity,
        "skybox": sky,
        "bloom": bloom,
        "fog": s.fog.enabled,
        "fogMode": s.fog.mode,
        "fogColor": color_json(s.fog.color),
        "fogDensity": s.fog.density,
        "fogStartDistance": s.fog.start,
        "fogEndDistance": s.fog.end,
        "heightFog": height,
        "cloudLayer": clouds,
        "volumetricFog": volumetric,
        "fogCulling": s.fog.culling,
    })
}

/// Applies the `renderSettings` of a scene file (what a scene loaded with is applied when it
/// becomes the active scene). Missing keys keep the current value.
pub fn apply_render_settings(saved: &Value) {
    change(|s| {
        if let Some(c) = saved.get("ambientLight").and_then(color_of) {
            s.ambient_light = c;
        }
        if let Some(m) = saved.get("ambientMode").and_then(Value::as_str)
            && (m == "skybox" || m == "color")
        {
            s.ambient_mode = m.to_string();
        }
        if let Some(r) = saved.get("reflectionIntensity").and_then(Value::as_f64) {
            s.reflection_intensity = r;
        }
        match saved.get("skybox") {
            Some(Value::Null) => s.skybox = None,
            Some(k) if k.is_object() => {
                let mut sky = Sky::default();
                if let Some(c) = k.get("zenith").and_then(color_of) {
                    sky.zenith = c;
                }
                if let Some(c) = k.get("horizon").and_then(color_of) {
                    sky.horizon = c;
                }
                if let Some(c) = k.get("ground").and_then(color_of) {
                    sky.ground = c;
                }
                match k.get("sunDirection") {
                    Some(Value::Null) => sky.sun_direction = None,
                    Some(d) => {
                        if let Some(v) = v3_of(d) {
                            sky.sun_direction = Some(v);
                        }
                    }
                    None => {}
                }
                if let Some(c) = k.get("sunColor").and_then(color_of) {
                    sky.sun_color = c;
                }
                if let Some(z) = k.get("sunSize").and_then(Value::as_f64) {
                    sky.sun_size = z;
                }
                s.skybox = Some(sky);
            }
            _ => {}
        }
        match saved.get("bloom") {
            Some(Value::Null) => s.bloom = None,
            Some(b) if b.is_object() => {
                let n = |k: &str, d: f64| b.get(k).and_then(Value::as_f64).unwrap_or(d);
                s.bloom = Some(Bloom {
                    threshold: n("threshold", 1.0),
                    intensity: n("intensity", 1.0),
                    scatter: n("scatter", 0.5),
                });
            }
            _ => {}
        }
        // The fog's settings (the switch last, as the TypeScript applies them).
        if let Some(m) = saved.get("fogMode").and_then(Value::as_str)
            && ["linear", "exponential", "exponentialSquared"].contains(&m)
        {
            s.fog.mode = m.to_string();
        }
        if let Some(c) = saved.get("fogColor").and_then(color_of) {
            s.fog.color = c;
        }
        if let Some(d) = saved.get("fogDensity").and_then(Value::as_f64) {
            s.fog.density = d;
        }
        if let Some(d) = saved.get("fogStartDistance").and_then(Value::as_f64) {
            s.fog.start = d;
        }
        if let Some(d) = saved.get("fogEndDistance").and_then(Value::as_f64) {
            s.fog.end = d;
        }
        match saved.get("heightFog") {
            Some(Value::Null) => s.fog.height = None,
            Some(h) if h.is_object() => {
                let d = HeightFog::default();
                let n = |k: &str, d: f64| h.get(k).and_then(Value::as_f64).unwrap_or(d);
                s.fog.height = Some(HeightFog {
                    density: n("density", d.density),
                    base_height: n("baseHeight", d.base_height),
                    falloff: n("falloff", d.falloff),
                });
            }
            _ => {}
        }
        match saved.get("cloudLayer") {
            Some(Value::Null) => s.cloud_layer = None,
            Some(c) if c.is_object() => {
                let d = CloudLayer::default();
                let n = |k: &str, d: f64| c.get(k).and_then(Value::as_f64).unwrap_or(d);
                s.cloud_layer = Some(CloudLayer {
                    altitude: n("altitude", d.altitude),
                    size: n("size", d.size),
                    coverage: n("coverage", d.coverage),
                    opacity: n("opacity", d.opacity),
                    softness: n("softness", d.softness),
                    tint: c.get("tint").and_then(color_of).unwrap_or(d.tint),
                    wind_speed: n("windSpeed", d.wind_speed),
                    wind_direction: n("windDirection", d.wind_direction),
                    fade_distance: n("fadeDistance", d.fade_distance),
                });
            }
            _ => {}
        }
        match saved.get("volumetricFog") {
            Some(Value::Null) => s.fog.volumetric = None,
            Some(v) if v.is_object() => {
                let d = Volumetric::default();
                let n = |k: &str, d: f64| v.get(k).and_then(Value::as_f64).unwrap_or(d);
                let pick = |k: &str, names: &[&str], d: &str| {
                    v.get(k)
                        .and_then(Value::as_str)
                        .filter(|s| names.contains(s))
                        .unwrap_or(d)
                        .to_string()
                };
                s.fog.volumetric = Some(Volumetric {
                    intensity: n("intensity", d.intensity),
                    anisotropy: n("anisotropy", d.anisotropy),
                    steps: n("steps", d.steps),
                    distance: n("distance", d.distance),
                    albedo: v.get("albedo").and_then(color_of).unwrap_or(d.albedo),
                    sharp_distance: n("sharpDistance", d.sharp_distance),
                    sharp_samples: n("sharpSamples", d.sharp_samples),
                    resolution: pick(
                        "resolution",
                        &["low", "medium", "high", "ultra"],
                        &d.resolution,
                    ),
                    denoising: pick(
                        "denoising",
                        &["none", "reprojection", "gaussian", "both"],
                        &d.denoising,
                    ),
                    history_weight: n("historyWeight", d.history_weight),
                    samples: n("samples", d.samples),
                    motion_samples: n("motionSamples", d.motion_samples),
                });
            }
            _ => {}
        }
        if let Some(b) = saved.get("fogCulling").and_then(Value::as_bool) {
            s.fog.culling = b;
        }
        if let Some(b) = saved.get("fog").and_then(Value::as_bool) {
            s.fog.enabled = b;
        }
    });
}

/// Whether the settings differ from a fresh game's (a scene then saves them).
pub fn settings_changed() -> bool {
    read(|s| *s != RenderSettingsData::default())
}

/// How fog thickens with distance (Unity's `FogMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogKind {
    Linear,
    Exponential,
    ExponentialSquared,
}

/// The light-shaft volume's detail levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Low,
    Medium,
    High,
    Ultra,
}

use render_settings_api::{
    exposure_metering_name, exposure_metering_parse, volume_denoising_name, volume_denoising_parse,
    volume_detail_name, volume_detail_parse,
};

/// How the light-shaft volume's sampling noise is cleaned up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denoise {
    None,
    Reprojection,
    Gaussian,
    Both,
}

/// Which part of the view auto exposure measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metering {
    Average,
    CenterWeighted,
    Spot,
}

/// A procedural sky as a script gives it: every key may be left out.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyPatch {
    pub zenith: Option<Rgba>,
    pub horizon: Option<Rgba>,
    pub ground: Option<Rgba>,
    /// `Some(None)`: `null`, no sun disc.
    pub sun_direction: Option<Option<V3>>,
    pub sun_color: Option<Rgba>,
    pub sun_size: Option<f64>,
}

impl SkyPatch {
    /// The sky with the keys that are given changed.
    fn apply(self, mut s: Sky) -> Sky {
        if let Some(c) = self.zenith {
            s.zenith = c;
        }
        if let Some(c) = self.horizon {
            s.horizon = c;
        }
        if let Some(c) = self.ground {
            s.ground = c;
        }
        if let Some(d) = self.sun_direction {
            s.sun_direction = d;
        }
        if let Some(c) = self.sun_color {
            s.sun_color = c;
        }
        if let Some(n) = self.sun_size {
            s.sun_size = n;
        }
        s
    }

    fn of(s: &Sky) -> SkyPatch {
        SkyPatch {
            zenith: Some(s.zenith),
            horizon: Some(s.horizon),
            ground: Some(s.ground),
            sun_direction: Some(s.sun_direction),
            sun_color: Some(s.sun_color),
            sun_size: Some(s.sun_size),
        }
    }
}

/// Light shafts as a script gives them: every key may be left out.
#[derive(Clone, Debug, PartialEq)]
pub struct VolumetricPatch {
    pub intensity: Option<f64>,
    pub anisotropy: Option<f64>,
    pub steps: Option<f64>,
    pub distance: Option<f64>,
    pub albedo: Option<Rgba>,
    pub sharp_distance: Option<f64>,
    pub sharp_samples: Option<f64>,
    pub resolution: Option<Detail>,
    pub denoising: Option<Denoise>,
    pub history_weight: Option<f64>,
    pub samples: Option<f64>,
    pub motion_samples: Option<f64>,
}

impl VolumetricPatch {
    fn apply(self, mut v: Volumetric) -> Volumetric {
        macro_rules! set {
            ($($f:ident),*) => { $( if let Some(x) = self.$f { v.$f = x; } )* };
        }
        set!(
            intensity,
            anisotropy,
            steps,
            distance,
            albedo,
            sharp_distance,
            sharp_samples,
            history_weight,
            samples,
            motion_samples
        );
        if let Some(d) = self.resolution {
            v.resolution = volume_detail_name(d).to_string();
        }
        if let Some(d) = self.denoising {
            v.denoising = volume_denoising_name(d).to_string();
        }
        v
    }

    fn of(v: &Volumetric) -> VolumetricPatch {
        VolumetricPatch {
            intensity: Some(v.intensity),
            anisotropy: Some(v.anisotropy),
            steps: Some(v.steps),
            distance: Some(v.distance),
            albedo: Some(v.albedo),
            sharp_distance: Some(v.sharp_distance),
            sharp_samples: Some(v.sharp_samples),
            resolution: Some(volume_detail_parse(&v.resolution).unwrap_or(Detail::High)),
            denoising: Some(volume_denoising_parse(&v.denoising).unwrap_or(Denoise::Both)),
            history_weight: Some(v.history_weight),
            samples: Some(v.samples),
            motion_samples: Some(v.motion_samples),
        }
    }
}

/// A cloud layer as a script gives it: every key may be left out.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudLayerPatch {
    pub altitude: Option<f64>,
    pub size: Option<f64>,
    pub coverage: Option<f64>,
    pub opacity: Option<f64>,
    pub softness: Option<f64>,
    pub tint: Option<Rgba>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub fade_distance: Option<f64>,
}

impl CloudLayerPatch {
    fn apply(self, mut c: CloudLayer) -> CloudLayer {
        macro_rules! set {
            ($($f:ident),*) => { $( if let Some(x) = self.$f { c.$f = x; } )* };
        }
        set!(
            altitude,
            size,
            coverage,
            opacity,
            softness,
            tint,
            wind_speed,
            wind_direction,
            fade_distance
        );
        c
    }

    fn of(c: &CloudLayer) -> CloudLayerPatch {
        CloudLayerPatch {
            altitude: Some(c.altitude),
            size: Some(c.size),
            coverage: Some(c.coverage),
            opacity: Some(c.opacity),
            softness: Some(c.softness),
            tint: Some(c.tint),
            wind_speed: Some(c.wind_speed),
            wind_direction: Some(c.wind_direction),
            fade_distance: Some(c.fade_distance),
        }
    }
}

/// Auto exposure as a script gives it: every key may be left out.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoExposurePatch {
    pub min_ev: Option<f64>,
    pub max_ev: Option<f64>,
    pub compensation: Option<f64>,
    pub speed_dark_to_light: Option<f64>,
    pub speed_light_to_dark: Option<f64>,
    pub metering: Option<Metering>,
    pub low_percent: Option<f64>,
    pub high_percent: Option<f64>,
    pub dark_adaptation: Option<f64>,
    pub dark_below: Option<f64>,
    pub curve: Option<Vec<(f64, f64)>>,
    pub hold: Option<bool>,
}

impl AutoExposurePatch {
    fn apply(self, mut a: AutoExposure) -> AutoExposure {
        macro_rules! set {
            ($($f:ident),*) => { $( if let Some(x) = self.$f { a.$f = x; } )* };
        }
        set!(
            min_ev,
            max_ev,
            compensation,
            speed_dark_to_light,
            speed_light_to_dark,
            low_percent,
            high_percent,
            dark_adaptation,
            dark_below,
            hold
        );
        if let Some(m) = self.metering {
            a.metering = exposure_metering_name(m).to_string();
        }
        if let Some(c) = self.curve {
            a.curve = c.into_iter().take(8).collect();
        }
        a
    }

    fn of(a: &AutoExposure) -> AutoExposurePatch {
        AutoExposurePatch {
            min_ev: Some(a.min_ev),
            max_ev: Some(a.max_ev),
            compensation: Some(a.compensation),
            speed_dark_to_light: Some(a.speed_dark_to_light),
            speed_light_to_dark: Some(a.speed_light_to_dark),
            metering: Some(
                exposure_metering_parse(&a.metering).unwrap_or(Metering::CenterWeighted),
            ),
            low_percent: Some(a.low_percent),
            high_percent: Some(a.high_percent),
            dark_adaptation: Some(a.dark_adaptation),
            dark_below: Some(a.dark_below),
            curve: Some(a.curve.clone()),
            hold: Some(a.hold),
        }
    }
}

#[ts_api(
    namespace = "RenderSettings", values(
        Color = Rgba [r: f64, g: f64, b: f64, a: f64],
        Vector3 = V3 [x: f64, y: f64, z: f64]
    ),
    enums(
        /// How fog thickens with distance (Unity's `FogMode`).
        FogMode = FogKind [Linear, Exponential, ExponentialSquared],
        /// The light-shaft volume's detail levels.
        VolumeDetail = Detail [Low, Medium, High, Ultra],
        /// How the light-shaft volume's sampling noise is cleaned up.
        VolumeDenoising = Denoise [None, Reprojection, Gaussian, Both],
        /// Which part of the view auto exposure measures.
        ExposureMetering = Metering [Average, CenterWeighted, Spot]
    ),
    records(
        /// A procedural sky. Every key is optional: the sky starts from the default one.
        ProceduralSky = SkyPatch [
            zenith: Option<Color>,
            horizon: Option<Color>,
            ground: Option<Color>,
            /// Towards the sun; `null` for no sun disc.
            sun_direction: Option<Option<Vector3Like>>,
            sun_color: Option<Color>,
            /// The disc's size in degrees.
            sun_size: Option<f64>
        ],
        BloomSettings = Bloom [
            /// Linear brightness where glowing starts (1: only what is brighter than white).
            threshold: f64,
            intensity: f64,
            /// 0..1: how far it spreads.
            scatter: f64
        ],
        /// Fog lying low (Unreal's exponential height fog), in `fogColor`.
        HeightFogSettings = HeightFog [
            /// Per metre at `baseHeight`.
            density: f64,
            base_height: f64,
            /// Per metre (larger: a thinner layer).
            falloff: f64
        ],
        /// Light shafts (volumetric fog): lights shining through the fog light it up, cut by their shadows. Every field is optional.
        VolumetricFogSettings = VolumetricPatch [
            /// Scales the scattered light (1).
            intensity: Option<f64>,
            /// -1 to 1: positive scatters forward (0.4).
            anisotropy: Option<f64>,
            /// Samples per depth slice, 1 to 16 (4).
            steps: Option<f64>,
            /// How far the volume reaches in metres (50).
            distance: Option<f64>,
            /// How much of each colour of light the fog scatters (white).
            albedo: Option<Color>,
            /// God rays: within this many metres of the camera the sun's shafts are worked out per pixel (0: off).
            sharp_distance: Option<f64>,
            /// Samples per pixel for the sharp part, 4 to 128 (32).
            sharp_samples: Option<f64>,
            /// How fine the light-shaft volume is (`high`).
            resolution: Option<VolumeDetail>,
            /// How the volume's sampling noise is cleaned up (`both`).
            denoising: Option<VolumeDenoising>,
            /// With reprojection, how much of each cell comes from earlier frames (0.9).
            history_weight: Option<f64>,
            /// Shadow samples per cell each frame, 1 to 8 (1).
            samples: Option<f64>,
            /// Samples for cells whose view moved this frame (2).
            motion_samples: Option<f64>
        ],
        /// A deck of cloud drawn with the sky (HDRP's cloud layer): behind everything, so the distance fog doesn't hide it. Each pixel of sky looks up to where its ray meets the deck, so the clouds shift overhead as the camera moves under them; the wind carries them; they thin out towards the horizon. Every field is optional.
        CloudLayerSettings = CloudLayerPatch [
            /// The deck's height in metres (1500).
            altitude: Option<f64>,
            /// About how wide a cloud is, metres (2000).
            size: Option<f64>,
            /// How much of the sky is cloud, 0 to 1 (0.5).
            coverage: Option<f64>,
            /// 0 to 1 (1).
            opacity: Option<f64>,
            /// How soft the clouds' edges are, 0 (hard) to 1 (0.3).
            softness: Option<f64>,
            /// Their colour (linear; thick parts are a little darker underneath). Light grey.
            tint: Option<Color>,
            /// The wind, m/s (0).
            wind_speed: Option<f64>,
            /// The way the wind blows, degrees from +x towards +z (0).
            wind_direction: Option<f64>,
            /// Over about this distance along a ray they fade into the sky, metres (20000).
            fade_distance: Option<f64>
        ],
        /// Auto exposure (HDRP's Automatic mode). Every field is optional.
        AutoExposureSettings = AutoExposurePatch [
            /// The darkest average brightness it adapts to, in EV100 (-10).
            min_ev: Option<f64> as "minEV",
            /// The brightest, in EV100 (20).
            max_ev: Option<f64> as "maxEV",
            /// Stops added after metering (0; positive is brighter).
            compensation: Option<f64>,
            /// How fast it adapts when the view gets brighter (3, per second).
            speed_dark_to_light: Option<f64>,
            /// How fast it adapts when the view gets darker (3, per second).
            speed_light_to_dark: Option<f64>,
            /// Which part of the view counts (`centerWeighted`).
            metering: Option<ExposureMetering>,
            /// The part of the brightness histogram averaged, in percent from the darkest (0 and 100: everything).
            low_percent: Option<f64>,
            high_percent: Option<f64>,
            /// Below `darkBelow` (EV100, 5) the eyes adapt only this much (0..1, 1) of the way.
            dark_adaptation: Option<f64>,
            dark_below: Option<f64>,
            /// More stops by how bright the view is: up to 8 points of [metered EV100, stops].
            curve: Option<pairs>,
            /// Stops adapting: the exposure holds where it is.
            hold: Option<bool>
        ]
    )
)]
pub mod render_settings_api {
    use super::*;

    /// Light everything gets regardless of lights (linear).
    #[property]
    pub fn ambient_light() -> Rgba {
        read(|s| s.ambient_light)
    }

    pub fn set_ambient_light(c: Rgba) {
        change(|s| s.ambient_light = c);
    }

    /// Where ambient light comes from: the sky (`skybox`, with one) or `ambientLight` (`color`).
    #[property]
    pub fn ambient_mode() -> String {
        read(|s| s.ambient_mode.clone())
    }

    pub fn set_ambient_mode(mode: &str) {
        if mode == "skybox" || mode == "color" {
            change(|s| s.ambient_mode = mode.to_string());
        }
    }

    /// How bright the sky's reflections and light are.
    #[property]
    pub fn reflection_intensity() -> f64 {
        read(|s| s.reflection_intensity)
    }

    pub fn set_reflection_intensity(i: f64) {
        change(|s| s.reflection_intensity = i);
    }

    /// The procedural sky as one object (`null`: none): what reflections show, where sky light comes
    /// from, and what cameras with `clearFlags: "skybox"` show. Setting it replaces the sky: the
    /// default one with the given keys changed.
    #[property]
    pub fn skybox() -> Option<SkyPatch> {
        read(|s| s.skybox.as_ref().map(SkyPatch::of))
    }

    pub fn set_skybox(v: Option<SkyPatch>) {
        change(|s| s.skybox = v.map(|p| p.apply(Sky::default())));
    }

    /// Bloom over every camera as one object (`null`: off).
    #[property]
    pub fn bloom() -> Option<Bloom> {
        read(|s| s.bloom.clone())
    }

    pub fn set_bloom(v: Option<Bloom>) {
        change(|s| s.bloom = v);
    }

    /// Height fog on top of the distance fog, while `fog` is on, as one object (`null`: none).
    #[property]
    pub fn height_fog() -> Option<HeightFog> {
        read(|s| s.fog.height.clone())
    }

    pub fn set_height_fog(v: Option<HeightFog>) {
        change(|s| s.fog.height = v);
    }

    /// The cloud layer as one object (`null`: none, the default). Setting it replaces the
    /// layer: the defaults with the given keys changed. See `CloudLayerSettings`.
    #[property]
    pub fn cloud_layer() -> Option<CloudLayerPatch> {
        read(|s| s.cloud_layer.as_ref().map(CloudLayerPatch::of))
    }

    pub fn set_cloud_layer(v: Option<CloudLayerPatch>) {
        change(|s| s.cloud_layer = v.map(|p| p.apply(CloudLayer::default())));
    }

    /// Light shafts in the fog, while `fog` is on, as one object (`null`: none). Setting it
    /// replaces the shafts: the defaults with the given keys changed.
    #[property]
    pub fn volumetric_fog() -> Option<VolumetricPatch> {
        read(|s| s.fog.volumetric.as_ref().map(VolumetricPatch::of))
    }

    pub fn set_volumetric_fog(v: Option<VolumetricPatch>) {
        change(|s| s.fog.volumetric = v.map(|p| p.apply(Volumetric::default())));
    }

    /// Adapts the exposure to the view, as one object (`null`: off, the default). Setting it
    /// replaces the settings: the defaults with the given keys changed.
    #[property]
    pub fn auto_exposure() -> Option<AutoExposurePatch> {
        read(|s| s.auto_exposure.as_ref().map(AutoExposurePatch::of))
    }

    pub fn set_auto_exposure(v: Option<AutoExposurePatch>) {
        change(|s| s.auto_exposure = v.map(|p| p.apply(AutoExposure::default())));
    }

    /// Whether there is a procedural sky: what reflections show, where sky light comes from, and
    /// what cameras with `clearFlags: "skybox"` show. Turning it on gives the default sky.
    #[property]
    pub fn skybox_enabled() -> bool {
        read(|s| s.skybox.is_some())
    }

    pub fn set_skybox_enabled(on: bool) {
        change(|s| {
            if on {
                s.skybox.get_or_insert_with(Sky::default);
            } else {
                s.skybox = None;
            }
        });
    }

    #[property]
    pub fn sky_zenith() -> Rgba {
        read(|s| s.skybox.clone().unwrap_or_default().zenith)
    }

    /// Setting a sky property turns the sky on.
    pub fn set_sky_zenith(c: Rgba) {
        change(|s| s.skybox.get_or_insert_with(Sky::default).zenith = c);
    }

    #[property]
    pub fn sky_horizon() -> Rgba {
        read(|s| s.skybox.clone().unwrap_or_default().horizon)
    }

    pub fn set_sky_horizon(c: Rgba) {
        change(|s| s.skybox.get_or_insert_with(Sky::default).horizon = c);
    }

    #[property]
    pub fn sky_ground() -> Rgba {
        read(|s| s.skybox.clone().unwrap_or_default().ground)
    }

    pub fn set_sky_ground(c: Rgba) {
        change(|s| s.skybox.get_or_insert_with(Sky::default).ground = c);
    }

    /// Towards the sun.
    #[property]
    pub fn sky_sun_direction() -> V3 {
        read(|s| {
            s.skybox
                .clone()
                .unwrap_or_default()
                .sun_direction
                .unwrap_or(V3::ZERO)
        })
    }

    pub fn set_sky_sun_direction(d: V3) {
        change(|s| s.skybox.get_or_insert_with(Sky::default).sun_direction = Some(d));
    }

    /// Whether the sky has a sun disc.
    #[property]
    pub fn sky_has_sun() -> bool {
        read(|s| s.skybox.as_ref().is_none_or(|k| k.sun_direction.is_some()))
    }

    pub fn set_sky_has_sun(on: bool) {
        change(|s| {
            let sky = s.skybox.get_or_insert_with(Sky::default);
            sky.sun_direction = on.then(|| {
                sky.sun_direction
                    .unwrap_or(Sky::default().sun_direction.unwrap_or(V3::UP))
            });
        });
    }

    #[property]
    pub fn sky_sun_color() -> Rgba {
        read(|s| s.skybox.clone().unwrap_or_default().sun_color)
    }

    pub fn set_sky_sun_color(c: Rgba) {
        change(|s| s.skybox.get_or_insert_with(Sky::default).sun_color = c);
    }

    /// The disc's size in degrees.
    #[property]
    pub fn sky_sun_size() -> f64 {
        read(|s| s.skybox.clone().unwrap_or_default().sun_size)
    }

    pub fn set_sky_sun_size(z: f64) {
        change(|s| s.skybox.get_or_insert_with(Sky::default).sun_size = z);
    }

    /// Bloom over every camera: bright things glow onto their surroundings. Turning it on gives
    /// threshold 1, intensity 1, scatter 0.5.
    #[property]
    pub fn bloom_enabled() -> bool {
        read(|s| s.bloom.is_some())
    }

    pub fn set_bloom_enabled(on: bool) {
        change(|s| {
            s.bloom = on.then(|| {
                s.bloom.clone().unwrap_or(Bloom {
                    threshold: 1.0,
                    intensity: 1.0,
                    scatter: 0.5,
                })
            })
        });
    }

    #[property]
    pub fn bloom_threshold() -> f64 {
        read(|s| s.bloom.as_ref().map_or(1.0, |b| b.threshold))
    }

    pub fn set_bloom_threshold(v: f64) {
        change(|s| bloom_of(s).threshold = v);
    }

    #[property]
    pub fn bloom_intensity() -> f64 {
        read(|s| s.bloom.as_ref().map_or(1.0, |b| b.intensity))
    }

    pub fn set_bloom_intensity(v: f64) {
        change(|s| bloom_of(s).intensity = v);
    }

    #[property]
    pub fn bloom_scatter() -> f64 {
        read(|s| s.bloom.as_ref().map_or(0.5, |b| b.scatter))
    }

    pub fn set_bloom_scatter(v: f64) {
        change(|s| bloom_of(s).scatter = v);
    }

    /// How HDR light (brighter than white) becomes screen colours: `none` clips it, `neutral`
    /// keeps colours and rolls highlights off softly, `aces` is filmic.
    #[property]
    pub fn tonemapping() -> String {
        read(|s| s.tonemapping.clone())
    }

    pub fn set_tonemapping(mode: &str) {
        if ["none", "neutral", "aces"].contains(&mode) {
            change(|s| s.tonemapping = mode.to_string());
        }
    }

    /// Brightens (positive) or darkens (negative) everything, in stops, before tonemapping.
    #[property]
    pub fn post_exposure() -> f64 {
        read(|s| s.post_exposure)
    }

    pub fn set_post_exposure(ev: f64) {
        change(|s| s.post_exposure = ev);
    }

    // ---- fog ----

    /// Fog (off by default): things fade towards `fogColor` with distance from the camera;
    /// give the camera a background in the same colour for a seamless horizon. Cameras can opt
    /// out (`Camera.fog`).
    #[property]
    pub fn fog() -> bool {
        read(|s| s.fog.enabled)
    }

    pub fn set_fog(on: bool) {
        change(|s| s.fog.enabled = on);
    }

    /// `linear` (from `fogStartDistance` to `fogEndDistance`), `exponential` (by `fogDensity` per
    /// metre) or `exponentialSquared` (clear nearby, then closing in).
    #[property]
    pub fn fog_mode() -> FogKind {
        read(|s| fog_mode_parse(&s.fog.mode).unwrap_or(FogKind::ExponentialSquared))
    }

    /// An unknown name is ignored.
    pub fn set_fog_mode(mode: Option<FogKind>) {
        if let Some(mode) = mode {
            change(|s| s.fog.mode = fog_mode_name(mode).to_string());
        }
    }

    /// The fog's colour (linear).
    #[property]
    pub fn fog_color() -> Rgba {
        read(|s| s.fog.color)
    }

    pub fn set_fog_color(c: Rgba) {
        change(|s| s.fog.color = c);
    }

    #[property]
    pub fn fog_density() -> f64 {
        read(|s| s.fog.density)
    }

    pub fn set_fog_density(d: f64) {
        change(|s| s.fog.density = d.max(0.0));
    }

    /// Where linear fog begins, in metres from the camera.
    #[property]
    pub fn fog_start_distance() -> f64 {
        read(|s| s.fog.start)
    }

    pub fn set_fog_start_distance(d: f64) {
        change(|s| s.fog.start = d);
    }

    /// Where linear fog hides everything.
    #[property]
    pub fn fog_end_distance() -> f64 {
        read(|s| s.fog.end)
    }

    pub fn set_fog_end_distance(d: f64) {
        change(|s| s.fog.end = d);
    }

    /// Skip drawing what lies wholly past where the distance fog hides everything (on by
    /// default). `RenderStats.fogCulled` counts what it saved.
    #[property]
    pub fn fog_culling() -> bool {
        read(|s| s.fog.culling)
    }

    pub fn set_fog_culling(on: bool) {
        change(|s| s.fog.culling = on);
    }

    /// Height fog on top of the distance fog, while `fog` is on. Turning it on gives the default
    /// layer; setting one of its properties turns it on.
    #[property]
    pub fn height_fog_enabled() -> bool {
        read(|s| s.fog.height.is_some())
    }

    pub fn set_height_fog_enabled(on: bool) {
        change(|s| {
            if on {
                s.fog.height.get_or_insert_with(HeightFog::default);
            } else {
                s.fog.height = None;
            }
        });
    }

    /// Per metre at `heightFogBaseHeight`.
    #[property]
    pub fn height_fog_density() -> f64 {
        read(|s| s.fog.height.clone().unwrap_or_default().density)
    }

    pub fn set_height_fog_density(v: f64) {
        change(|s| s.fog.height.get_or_insert_with(HeightFog::default).density = v);
    }

    #[property]
    pub fn height_fog_base_height() -> f64 {
        read(|s| s.fog.height.clone().unwrap_or_default().base_height)
    }

    pub fn set_height_fog_base_height(v: f64) {
        change(|s| {
            s.fog
                .height
                .get_or_insert_with(HeightFog::default)
                .base_height = v
        });
    }

    /// Per metre (larger: a thinner layer).
    #[property]
    pub fn height_fog_falloff() -> f64 {
        read(|s| s.fog.height.clone().unwrap_or_default().falloff)
    }

    pub fn set_height_fog_falloff(v: f64) {
        change(|s| s.fog.height.get_or_insert_with(HeightFog::default).falloff = v);
    }

    /// Light shafts in the fog, while `fog` is on. Turning it on gives the defaults; setting one
    /// of its properties turns it on.
    #[property]
    pub fn volumetric_fog_enabled() -> bool {
        read(|s| s.fog.volumetric.is_some())
    }

    pub fn set_volumetric_fog_enabled(on: bool) {
        change(|s| {
            if on {
                s.fog.volumetric.get_or_insert_with(Volumetric::default);
            } else {
                s.fog.volumetric = None;
            }
        });
    }

    /// Scales the scattered light (1).
    #[property]
    pub fn volumetric_fog_intensity() -> f64 {
        read(|s| volumetric_of(s).intensity)
    }

    pub fn set_volumetric_fog_intensity(v: f64) {
        change(|s| volumetric_mut(s).intensity = v);
    }

    /// -1 to 1: positive scatters forward, so looking towards a light shows more (0.4).
    #[property]
    pub fn volumetric_fog_anisotropy() -> f64 {
        read(|s| volumetric_of(s).anisotropy)
    }

    pub fn set_volumetric_fog_anisotropy(v: f64) {
        change(|s| volumetric_mut(s).anisotropy = v);
    }

    /// Samples per depth slice, 1 to 16 (4).
    #[property]
    pub fn volumetric_fog_steps() -> f64 {
        read(|s| volumetric_of(s).steps)
    }

    pub fn set_volumetric_fog_steps(v: f64) {
        change(|s| volumetric_mut(s).steps = v);
    }

    /// How far the volume reaches in metres (50).
    #[property]
    pub fn volumetric_fog_distance() -> f64 {
        read(|s| volumetric_of(s).distance)
    }

    pub fn set_volumetric_fog_distance(v: f64) {
        change(|s| volumetric_mut(s).distance = v);
    }

    /// How much of each colour of light the fog scatters (white).
    #[property]
    pub fn volumetric_fog_albedo() -> Rgba {
        read(|s| volumetric_of(s).albedo)
    }

    pub fn set_volumetric_fog_albedo(c: Rgba) {
        change(|s| volumetric_mut(s).albedo = c);
    }

    /// God rays: within this many metres of the camera the sun's shafts are worked out per
    /// pixel from its shadow map (0: off).
    #[property]
    pub fn volumetric_fog_sharp_distance() -> f64 {
        read(|s| volumetric_of(s).sharp_distance)
    }

    pub fn set_volumetric_fog_sharp_distance(v: f64) {
        change(|s| volumetric_mut(s).sharp_distance = v);
    }

    /// Samples per pixel for the sharp part, 4 to 128 (32).
    #[property]
    pub fn volumetric_fog_sharp_samples() -> f64 {
        read(|s| volumetric_of(s).sharp_samples)
    }

    pub fn set_volumetric_fog_sharp_samples(v: f64) {
        change(|s| volumetric_mut(s).sharp_samples = v);
    }

    /// How fine the light-shaft volume is: `low`, `medium`, `high` (the default) or `ultra`.
    #[property]
    pub fn volumetric_fog_resolution() -> Detail {
        read(|s| volume_detail_parse(&volumetric_of(s).resolution).unwrap_or(Detail::High))
    }

    /// An unknown name is ignored.
    pub fn set_volumetric_fog_resolution(v: Option<Detail>) {
        if let Some(v) = v {
            change(|s| volumetric_mut(s).resolution = volume_detail_name(v).to_string());
        }
    }

    /// How the volume's sampling noise is cleaned up: `none`, `reprojection`, `gaussian` or
    /// `both` (the default).
    #[property]
    pub fn volumetric_fog_denoising() -> Denoise {
        read(|s| volume_denoising_parse(&volumetric_of(s).denoising).unwrap_or(Denoise::Both))
    }

    /// An unknown name is ignored.
    pub fn set_volumetric_fog_denoising(v: Option<Denoise>) {
        if let Some(v) = v {
            change(|s| volumetric_mut(s).denoising = volume_denoising_name(v).to_string());
        }
    }

    /// With reprojection, how much of each cell comes from earlier frames (0.9, up to 0.98).
    #[property]
    pub fn volumetric_fog_history_weight() -> f64 {
        read(|s| volumetric_of(s).history_weight)
    }

    pub fn set_volumetric_fog_history_weight(v: f64) {
        change(|s| volumetric_mut(s).history_weight = v);
    }

    /// Shadow samples per cell each frame, 1 to 8 (1).
    #[property]
    pub fn volumetric_fog_samples() -> f64 {
        read(|s| volumetric_of(s).samples)
    }

    pub fn set_volumetric_fog_samples(v: f64) {
        change(|s| volumetric_mut(s).samples = v);
    }

    /// Samples for cells whose view moved this frame (2).
    #[property]
    pub fn volumetric_fog_motion_samples() -> f64 {
        read(|s| volumetric_of(s).motion_samples)
    }

    pub fn set_volumetric_fog_motion_samples(v: f64) {
        change(|s| volumetric_mut(s).motion_samples = v);
    }

    // ---- exposure ----

    /// Adapts the exposure to the view (HDRP's Automatic mode): metered, weighted towards its
    /// middle, easing towards one that shows its average as mid grey. Turning it on gives the
    /// defaults; setting one of its properties turns it on.
    #[property]
    pub fn auto_exposure_enabled() -> bool {
        read(|s| s.auto_exposure.is_some())
    }

    pub fn set_auto_exposure_enabled(on: bool) {
        change(|s| {
            if on {
                s.auto_exposure.get_or_insert_with(AutoExposure::default);
            } else {
                s.auto_exposure = None;
            }
        });
    }

    /// The darkest average brightness it adapts to, in EV100 (-10).
    #[property]
    pub fn auto_exposure_min_ev() -> f64 {
        read(|s| auto_of(s).min_ev)
    }

    pub fn set_auto_exposure_min_ev(v: f64) {
        change(|s| auto_mut(s).min_ev = v);
    }

    /// The brightest, in EV100 (20).
    #[property]
    pub fn auto_exposure_max_ev() -> f64 {
        read(|s| auto_of(s).max_ev)
    }

    pub fn set_auto_exposure_max_ev(v: f64) {
        change(|s| auto_mut(s).max_ev = v);
    }

    /// Stops added after metering (0; positive is brighter).
    #[property]
    pub fn auto_exposure_compensation() -> f64 {
        read(|s| auto_of(s).compensation)
    }

    pub fn set_auto_exposure_compensation(v: f64) {
        change(|s| auto_mut(s).compensation = v);
    }

    /// How fast it adapts when the view gets brighter (3, per second).
    #[property]
    pub fn auto_exposure_speed_dark_to_light() -> f64 {
        read(|s| auto_of(s).speed_dark_to_light)
    }

    pub fn set_auto_exposure_speed_dark_to_light(v: f64) {
        change(|s| auto_mut(s).speed_dark_to_light = v);
    }

    /// How fast it adapts when the view gets darker (3, per second).
    #[property]
    pub fn auto_exposure_speed_light_to_dark() -> f64 {
        read(|s| auto_of(s).speed_light_to_dark)
    }

    pub fn set_auto_exposure_speed_light_to_dark(v: f64) {
        change(|s| auto_mut(s).speed_light_to_dark = v);
    }

    /// Which part of the view counts: `average`, `centerWeighted` (the default) or `spot`.
    #[property]
    pub fn auto_exposure_metering() -> Metering {
        read(|s| exposure_metering_parse(&auto_of(s).metering).unwrap_or(Metering::CenterWeighted))
    }

    /// An unknown name is ignored.
    pub fn set_auto_exposure_metering(v: Option<Metering>) {
        if let Some(v) = v {
            change(|s| auto_mut(s).metering = exposure_metering_name(v).to_string());
        }
    }

    /// The part of the brightness histogram averaged, in percent from the darkest (0 and 100:
    /// everything).
    #[property]
    pub fn auto_exposure_low_percent() -> f64 {
        read(|s| auto_of(s).low_percent)
    }

    pub fn set_auto_exposure_low_percent(v: f64) {
        change(|s| auto_mut(s).low_percent = v);
    }

    #[property]
    pub fn auto_exposure_high_percent() -> f64 {
        read(|s| auto_of(s).high_percent)
    }

    pub fn set_auto_exposure_high_percent(v: f64) {
        change(|s| auto_mut(s).high_percent = v);
    }

    /// A soft floor: below `autoExposureDarkBelow` (EV100, 5) the eyes adapt only this much
    /// (0..1, default 1: fully) of the way.
    #[property]
    pub fn auto_exposure_dark_adaptation() -> f64 {
        read(|s| auto_of(s).dark_adaptation)
    }

    pub fn set_auto_exposure_dark_adaptation(v: f64) {
        change(|s| auto_mut(s).dark_adaptation = v);
    }

    #[property]
    pub fn auto_exposure_dark_below() -> f64 {
        read(|s| auto_of(s).dark_below)
    }

    pub fn set_auto_exposure_dark_below(v: f64) {
        change(|s| auto_mut(s).dark_below = v);
    }

    /// Stops adapting: the exposure holds where it is.
    #[property]
    pub fn auto_exposure_hold() -> bool {
        read(|s| auto_of(s).hold)
    }

    pub fn set_auto_exposure_hold(v: bool) {
        change(|s| auto_mut(s).hold = v);
    }

    /// Clears the auto exposure's curve (more stops by how bright the view is).
    pub fn clear_auto_exposure_curve() {
        change(|s| auto_mut(s).curve.clear());
    }

    /// Adds a point [metered EV100, stops] to the curve (up to 8; joined by straight lines).
    pub fn add_auto_exposure_curve_point(ev: f64, stops: f64) {
        change(|s| {
            let c = &mut auto_mut(s).curve;
            if c.len() < 8 {
                c.push((ev, stops));
            }
        });
    }

    /// The manual exposure, in EV100 (higher is darker), used without auto exposure and blended
    /// with it while it fades in or out (`null`: light as it is, the default).
    #[property]
    pub fn fixed_exposure() -> Option<f64> {
        read(|s| s.fixed_exposure)
    }

    pub fn set_fixed_exposure(ev: Option<f64>) {
        change(|s| s.fixed_exposure = ev);
    }

    /// Seconds auto exposure takes to blend in when it is turned on and out (to the manual
    /// exposure) when it is turned off, and a change of `fixedExposure` takes to ease.
    #[property]
    pub fn exposure_transition() -> f64 {
        read(|s| s.exposure_transition)
    }

    pub fn set_exposure_transition(seconds: f64) {
        change(|s| s.exposure_transition = seconds.max(0.0));
    }

    /// Turns auto exposure off without anything changing on screen: the manual exposure is set
    /// to the main camera's current one. Pair it with `exposureTransition` for a slow blend.
    pub fn freeze_exposure() {
        let main = platypus_engine_api::camera::CameraRef::main();
        let ev = platypus_engine_api::camera::with_camera(main, |d| d.exposure).unwrap_or(f64::NAN);
        change(|s| {
            if ev.is_finite() {
                s.fixed_exposure = Some(ev);
            }
            s.auto_exposure = None;
        });
    }

    fn volumetric_of(s: &RenderSettingsData) -> Volumetric {
        s.fog.volumetric.clone().unwrap_or_default()
    }

    fn volumetric_mut(s: &mut RenderSettingsData) -> &mut Volumetric {
        s.fog.volumetric.get_or_insert_with(Volumetric::default)
    }

    fn auto_of(s: &RenderSettingsData) -> AutoExposure {
        s.auto_exposure.clone().unwrap_or_default()
    }

    fn auto_mut(s: &mut RenderSettingsData) -> &mut AutoExposure {
        s.auto_exposure.get_or_insert_with(AutoExposure::default)
    }

    fn bloom_of(s: &mut RenderSettingsData) -> &mut Bloom {
        s.bloom.get_or_insert(Bloom {
            threshold: 1.0,
            intensity: 1.0,
            scatter: 0.5,
        })
    }
}

/// What the world renderer drew and skipped in the last frame (`RenderStats`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub meshes: f64,
    pub culled_meshes: f64,
    pub quads: f64,
    pub culled_quads: f64,
    pub fog_culled: f64,
    pub lights: f64,
    pub culled_lights: f64,
    pub shadow_maps: f64,
    pub shadow_casters: f64,
}

thread_local! {
    static STATS: RefCell<Stats> = RefCell::new(Stats::default());
}

/// The host reports the last frame's statistics.
pub fn set_render_stats(stats: Stats) {
    STATS.with(|s| *s.borrow_mut() = stats);
}

fn stat<R>(f: impl FnOnce(&Stats) -> R) -> R {
    STATS.with(|s| f(&s.borrow()))
}

/// What the world renderer drew and skipped in the last frame, over every camera, mirror and
/// reflection probe (like Unity's rendering statistics). Read-only; the engine updates it.
#[ts_api(namespace = "RenderStats")]
pub mod render_stats_api {
    use super::*;

    #[property]
    pub fn meshes() -> f64 {
        stat(|s| s.meshes)
    }

    /// Meshes left out: outside a camera's view or hidden by fog.
    #[property]
    pub fn culled_meshes() -> f64 {
        stat(|s| s.culled_meshes)
    }

    #[property]
    pub fn quads() -> f64 {
        stat(|s| s.quads)
    }

    #[property]
    pub fn culled_quads() -> f64 {
        stat(|s| s.culled_quads)
    }

    /// Of the culled meshes and quads, those `RenderSettings.fogCulling` skipped.
    #[property]
    pub fn fog_culled() -> f64 {
        stat(|s| s.fog_culled)
    }

    #[property]
    pub fn lights() -> f64 {
        stat(|s| s.lights)
    }

    #[property]
    pub fn culled_lights() -> f64 {
        stat(|s| s.culled_lights)
    }

    /// Shadow map layers drawn.
    #[property]
    pub fn shadow_maps() -> f64 {
        stat(|s| s.shadow_maps)
    }

    /// Meshes drawn into the shadow maps: casters outside every map are left out.
    #[property]
    pub fn shadow_casters() -> f64 {
        stat(|s| s.shadow_casters)
    }
}

thread_local! {
    static PHYSICAL_UNITS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The host tells the game its project chose real-world light units (`lightUnits: "physical"`).
pub fn set_physical_light_units(physical: bool) {
    PHYSICAL_UNITS.with(|p| p.set(physical));
}

/// Project-wide graphics settings (Unity's `GraphicsSettings`). Platypus has one render pipeline;
/// what a project chooses is its light units, in `project.json`.
#[ts_api(namespace = "GraphicsSettings")]
pub mod graphics_settings_api {
    use super::*;

    /// `physical`: real-world units (the sun in lux, lamps in lumens, glowing surfaces and the
    /// sky in nits); `legacy` (the default): Platypus's own.
    #[property]
    pub fn light_units() -> String {
        if PHYSICAL_UNITS.with(std::cell::Cell::get) {
            "physical".to_string()
        } else {
            "legacy".to_string()
        }
    }
}
