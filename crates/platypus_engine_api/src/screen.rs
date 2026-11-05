// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Screen` and `QualitySettings` (`js/engine/src/screen.ts`, `quality.ts`): the window as
//! scripts see it, and the rendering quality levels. The host feeds the window ([`update`],
//! [`set_display`]) and reads what scripts asked for: [`state`] / [`take_requests`] for the
//! window, [`quality`] / [`quality_version`] for quality. Quality levels come from the game's
//! `package.json` ([`configure_quality`]) and the player's saved choice (`platypus.quality` in
//! PlayerPrefs, [`apply_default_quality`]).

use crate::prefs;
use serde_json::{Value, json};
use std::cell::RefCell;
use ts_api::callback::{Listeners, emit};
use ts_api::{Callback, ts_api};

/// A display mode: physical pixels and Hz (0 when unknown).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    pub refresh_rate: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenState {
    /// The window's size in UI pixels.
    pub width: u32,
    pub height: u32,
    /// Physical pixels per UI pixel (DPI scale x UI scale).
    pub scale: f64,
    /// `ExclusiveFullScreen`, `FullScreenWindow`, `MaximizedWindow` or `Windowed`.
    pub mode: String,
    pub full_screen: bool,
    /// The window's size when windowed, in physical pixels.
    pub window_width: u32,
    pub window_height: u32,
    /// The monitor's display modes, largest and fastest first.
    pub resolutions: Vec<Resolution>,
    pub current_resolution: Resolution,
    pub supported_modes: Vec<String>,
}

impl Default for ScreenState {
    fn default() -> Self {
        ScreenState {
            width: 1280,
            height: 720,
            scale: 1.0,
            mode: "Windowed".into(),
            full_screen: false,
            window_width: 1280,
            window_height: 720,
            resolutions: Vec::new(),
            current_resolution: Resolution::default(),
            supported_modes: [
                "ExclusiveFullScreen",
                "FullScreenWindow",
                "MaximizedWindow",
                "Windowed",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

/// A change of the window that scripts asked for (a size of 0 and a refresh rate of 0 mean
/// "unchanged").
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenRequest {
    pub mode: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: f64,
}

fn is_full_screen(mode: &str) -> bool {
    mode == "FullScreenWindow" || mode == "ExclusiveFullScreen"
}

/// One quality level, or the current settings.
#[derive(Clone, Debug, PartialEq)]
pub struct QualityState {
    pub name: String,
    /// Samples per pixel (MSAA): 1 (off), 2, 4 or 8.
    pub anti_aliasing: u32,
    /// The world's resolution relative to the window (0.25 to 2).
    pub render_scale: f64,
    /// `disable`, `hardOnly` or `all`.
    pub shadows: String,
    /// `low` (3 x 3 samples), `medium` (5 x 5) or `high` (7 x 7).
    pub soft_shadow_quality: String,
    pub shadow_resolution: u32,
    pub shadow_distance: f64,
    pub shadow_cascades: u32,
    pub lod_bias: f64,
    /// 1: frames wait for the display's refresh; 0: they do not.
    pub v_sync_count: i32,
    /// `disable`, `enable` or `forceEnable`.
    pub anisotropic_filtering: String,
    /// Top mip levels skipped, 0 to 3.
    pub global_texture_mipmap_limit: u32,
    /// Lets the game's auto exposure run (a player's option).
    pub auto_exposure: bool,
    /// Light shafts and god rays (`RenderSettings.volumetricFog`).
    pub volumetric_fog: bool,
    /// Rendering in HDR.
    pub hdr: bool,
    /// How many lights besides the main one a camera draws (-1: no limit).
    pub additional_lights: i32,
    /// Whether spot and point lights cast shadows.
    pub additional_light_shadows: bool,
}

#[allow(clippy::too_many_arguments)]
fn level(
    name: &str,
    aa: u32,
    scale: f64,
    shadows: &str,
    soft: &str,
    res: u32,
    dist: f64,
    cascades: u32,
    lod: f64,
    aniso: &str,
    mips: u32,
) -> QualityState {
    QualityState {
        name: name.into(),
        anti_aliasing: aa,
        render_scale: scale,
        shadows: shadows.into(),
        soft_shadow_quality: soft.into(),
        shadow_resolution: res,
        shadow_distance: dist,
        shadow_cascades: cascades,
        lod_bias: lod,
        // On: players usually want it (`Application.targetFrameRate` caps frames when it is off).
        v_sync_count: 1,
        anisotropic_filtering: aniso.into(),
        global_texture_mipmap_limit: mips,
        auto_exposure: true,
        volumetric_fog: true,
        hdr: true,
        additional_lights: -1,
        additional_light_shadows: true,
    }
}

fn default_levels() -> Vec<QualityState> {
    let mut low = level(
        "Low", 1, 0.75, "hardOnly", "low", 1024, 20.0, 1, 0.4, "disable", 1,
    );
    // The phones' tier: no light shafts, 8 additional lights without shadows.
    low.volumetric_fog = false;
    low.additional_lights = 8;
    low.additional_light_shadows = false;
    let mut medium = level(
        "Medium", 2, 1.0, "all", "low", 2048, 25.0, 2, 0.7, "enable", 0,
    );
    medium.additional_lights = 32;
    medium.additional_light_shadows = false;
    vec![
        low,
        medium,
        level(
            "High", 4, 1.0, "all", "low", 2048, 30.0, 4, 1.0, "enable", 0,
        ),
        level(
            "Ultra",
            8,
            1.0,
            "all",
            "medium",
            4096,
            40.0,
            4,
            2.0,
            "forceEnable",
            0,
        ),
    ]
}

/// Applies the keys a JSON object has to a level (the game's overrides, or a saved choice).
fn apply_fields(q: &mut QualityState, o: &Value) {
    let n = |k: &str| o.get(k).and_then(Value::as_f64);
    let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
    if let Some(x) = n("antiAliasing") {
        q.anti_aliasing = x as u32;
    }
    if let Some(x) = n("renderScale") {
        q.render_scale = x;
    }
    if let Some(x) = s("shadows") {
        q.shadows = x;
    }
    if let Some(x) = s("softShadowQuality") {
        q.soft_shadow_quality = x;
    }
    if let Some(x) = n("shadowResolution") {
        q.shadow_resolution = x as u32;
    }
    if let Some(x) = n("shadowDistance") {
        q.shadow_distance = x;
    }
    if let Some(x) = n("shadowCascades") {
        q.shadow_cascades = x as u32;
    }
    if let Some(x) = n("lodBias") {
        q.lod_bias = x;
    }
    if let Some(x) = n("vSyncCount") {
        q.v_sync_count = x as i32;
    }
    if let Some(x) = s("anisotropicFiltering") {
        q.anisotropic_filtering = x;
    }
    if let Some(x) = n("globalTextureMipmapLimit") {
        q.global_texture_mipmap_limit = x as u32;
    }
    let b = |k: &str| o.get(k).and_then(Value::as_bool);
    if let Some(x) = b("autoExposure") {
        q.auto_exposure = x;
    }
    if let Some(x) = b("volumetricFog") {
        q.volumetric_fog = x;
    }
    if let Some(x) = b("hdr") {
        q.hdr = x;
    }
    if let Some(x) = n("additionalLights") {
        q.additional_lights = if x < 0.0 { -1 } else { x.min(256.0) as i32 };
    }
    if let Some(x) = b("additionalLightShadows") {
        q.additional_light_shadows = x;
    }
}

fn level_json(q: &QualityState) -> Value {
    json!({
        "name": q.name,
        "antiAliasing": q.anti_aliasing,
        "renderScale": q.render_scale,
        "shadows": q.shadows,
        "softShadowQuality": q.soft_shadow_quality,
        "shadowResolution": q.shadow_resolution,
        "shadowDistance": q.shadow_distance,
        "shadowCascades": q.shadow_cascades,
        "lodBias": q.lod_bias,
        "vSyncCount": q.v_sync_count,
        "anisotropicFiltering": q.anisotropic_filtering,
        "globalTextureMipmapLimit": q.global_texture_mipmap_limit,
        "autoExposure": q.auto_exposure,
        "volumetricFog": q.volumetric_fog,
        "hdr": q.hdr,
        "additionalLights": q.additional_lights,
        "additionalLightShadows": q.additional_light_shadows,
    })
}

struct Quality {
    levels: Vec<QualityState>,
    index: usize,
    current: QualityState,
    version: u64,
    restoring: bool,
    on_level: Listeners<(f64,)>,
}

impl Quality {
    fn new() -> Quality {
        let levels = default_levels();
        let index = 2;
        Quality {
            current: levels[index].clone(),
            levels,
            index,
            version: 0,
            restoring: false,
            on_level: Listeners::default(),
        }
    }
}

struct State {
    screen: ScreenState,
    requests: Vec<ScreenRequest>,
    on_change: Listeners<()>,
    quality: Quality,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        screen: ScreenState::default(),
        requests: Vec::new(),
        on_change: Listeners::default(),
        quality: Quality::new(),
    });
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// The window as scripts see it.
pub fn state() -> ScreenState {
    st(|s| s.screen.clone())
}

/// The current quality settings.
pub fn quality() -> QualityState {
    st(|s| s.quality.current.clone())
}

/// A number that grows with every change of the quality settings (the renderer looks again).
pub fn quality_version() -> u64 {
    st(|s| s.quality.version)
}

/// The window's size and scale changed (UI pixels): `onChange` listeners run.
pub fn update(width: u32, height: u32, scale: f64) {
    let cbs = st(|s| {
        s.screen.width = width;
        s.screen.height = height;
        s.screen.scale = scale;
        s.on_change.snapshot()
    });
    emit(&cbs, ());
}

/// The window's size changed.
pub fn set_size(width: u32, height: u32) {
    let scale = st(|s| s.screen.scale);
    update(width, height, scale);
}

/// What the window and monitor are now (after each change).
pub fn set_display(
    mode: &str,
    window: (u32, u32),
    resolutions: Vec<Resolution>,
    current: Resolution,
    supported: Vec<String>,
) {
    st(|s| {
        s.screen.mode = mode.to_string();
        s.screen.full_screen = is_full_screen(mode);
        s.screen.window_width = window.0;
        s.screen.window_height = window.1;
        s.screen.resolutions = resolutions;
        s.screen.current_resolution = current;
        if !supported.is_empty() {
            s.screen.supported_modes = supported;
        }
    });
}

/// The window changes scripts asked for since the last call, in order.
pub fn take_requests() -> Vec<ScreenRequest> {
    st(|s| std::mem::take(&mut s.requests))
}

/// Back to a 1280x720 window and the default quality levels (a test, or a reload).
pub fn reset() {
    st(|s| {
        s.screen = ScreenState::default();
        s.requests.clear();
        s.on_change.clear();
        s.quality = Quality::new();
    });
}

fn request_mode(mode: &str, width: u32, height: u32, refresh_rate: f64) {
    st(|s| {
        s.requests.push(ScreenRequest {
            mode: mode.to_string(),
            width,
            height,
            refresh_rate,
        });
        // The window answers with `set_display`; until then scripts see their request.
        s.screen.mode = mode.to_string();
        s.screen.full_screen = is_full_screen(mode);
    });
}

/// How the window covers the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullScreenKind {
    ExclusiveFullScreen,
    FullScreenWindow,
    MaximizedWindow,
    Windowed,
}

/// A size in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenSize {
    pub width: f64,
    pub height: f64,
}

/// One display mode of the monitor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayMode {
    pub width: f64,
    pub height: f64,
    /// Hertz.
    pub refresh_rate: f64,
}

impl From<&Resolution> for DisplayMode {
    fn from(r: &Resolution) -> Self {
        DisplayMode {
            width: f64::from(r.width),
            height: f64::from(r.height),
            refresh_rate: r.refresh_rate,
        }
    }
}

#[ts_api(
    namespace = "Screen", records(
        /// A size in pixels.
        WindowSize = ScreenSize [width: f64, height: f64],
        /// One display mode of the monitor (its size and refresh rate).
        Resolution = DisplayMode [width: f64, height: f64, refresh_rate: f64]
    ),
    enums(
        /// How the window covers the screen (Unity's `FullScreenMode`).
        FullScreenMode = FullScreenKind [
            ExclusiveFullScreen = "ExclusiveFullScreen",
            FullScreenWindow = "FullScreenWindow",
            MaximizedWindow = "MaximizedWindow",
            Windowed = "Windowed"
        ]
    )
)]
pub mod screen_api {
    use super::*;

    /// The window's width in UI pixels.
    #[property]
    pub fn width() -> u32 {
        st(|s| s.screen.width)
    }

    #[property]
    pub fn height() -> u32 {
        st(|s| s.screen.height)
    }

    /// Physical pixels per UI pixel (DPI scale x UI scale).
    #[property]
    pub fn scale() -> f64 {
        st(|s| s.screen.scale)
    }

    #[property]
    pub fn aspect() -> f64 {
        st(|s| f64::from(s.screen.width) / f64::from(s.screen.height.max(1)))
    }

    /// How the window covers the screen: `ExclusiveFullScreen`, `FullScreenWindow`,
    /// `MaximizedWindow` or `Windowed`. Setting it asks the host (a mode the platform lacks falls
    /// back to the nearest).
    #[property]
    pub fn full_screen_mode() -> FullScreenKind {
        st(|s| full_screen_mode_parse(&s.screen.mode).unwrap_or(FullScreenKind::Windowed))
    }

    /// Another name is ignored.
    pub fn set_full_screen_mode(mode: Option<FullScreenKind>) {
        if let Some(mode) = mode {
            request_mode(full_screen_mode_name(mode), 0, 0, 0.0);
        }
    }

    /// Fullscreen (borderless, or exclusive when that is the mode) or not; the app applies it at
    /// the end of the frame.
    #[property]
    pub fn full_screen() -> bool {
        st(|s| s.screen.full_screen)
    }

    pub fn set_full_screen(on: bool) {
        if on != st(|s| s.screen.full_screen) {
            request_mode(if on { "FullScreenWindow" } else { "Windowed" }, 0, 0, 0.0);
        }
    }

    /// Sets the window's size in physical pixels (windowed) or the display's mode (exclusive
    /// fullscreen: the closest the monitor has), optionally with a mode and a refresh rate.
    /// Borderless and maximized windows ignore the size.
    #[optional(mode, refresh_rate = 0)]
    pub fn set_resolution(
        width: f64,
        height: f64,
        mode: Option<FullScreenKind>,
        refresh_rate: f64,
    ) {
        let mode = match mode {
            Some(m) => full_screen_mode_name(m).to_string(),
            None => st(|s| s.screen.mode.clone()),
        };
        request_mode(
            &mode,
            width.round().max(0.0) as u32,
            height.round().max(0.0) as u32,
            refresh_rate,
        );
    }

    /// The window's size when windowed (kept through fullscreen), in physical pixels.
    #[property]
    pub fn window_width() -> u32 {
        st(|s| s.screen.window_width)
    }

    #[property]
    pub fn window_height() -> u32 {
        st(|s| s.screen.window_height)
    }

    /// The window's size when windowed, in physical pixels (`windowWidth` and `windowHeight`).
    #[property]
    pub fn window_size() -> ScreenSize {
        st(|s| ScreenSize {
            width: f64::from(s.screen.window_width),
            height: f64::from(s.screen.window_height),
        })
    }

    /// The monitor's display modes, largest and fastest first.
    #[property]
    pub fn resolutions() -> Vec<DisplayMode> {
        st(|s| s.screen.resolutions.iter().map(DisplayMode::from).collect())
    }

    /// What the monitor runs at now.
    #[property]
    pub fn current_resolution() -> DisplayMode {
        st(|s| DisplayMode::from(&s.screen.current_resolution))
    }

    /// The modes this platform offers, most fullscreen first.
    #[property]
    pub fn supported_full_screen_modes() -> Vec<FullScreenKind> {
        st(|s| {
            s.screen
                .supported_modes
                .iter()
                .filter_map(|m| full_screen_mode_parse(m))
                .collect()
        })
    }

    /// Calls `f()` after the size or scale changes; returns a token for `offChange`.
    #[subscription(off = "off_change")]
    pub fn on_change(f: Callback<()>) -> u32 {
        st(|s| s.on_change.add(f))
    }

    pub fn off_change(token: u32) -> bool {
        st(|s| s.on_change.remove(token))
    }
}

// ---- QualitySettings ----

/// The game's quality levels and default (`platypus.quality` in its `package.json`):
/// `{ "default": "Medium", "levels": { "Low": { "renderScale": 0.6 } } }`. Starts at the default
/// level; the player's saved choice is applied by [`apply_default_quality`].
pub fn configure_quality(manifest: &Value) {
    st(|s| {
        let mut q = Quality::new();
        if let Some(over) = manifest.get("levels").and_then(Value::as_object) {
            for l in q.levels.iter_mut() {
                if let Some(o) = over.get(&l.name) {
                    let name = l.name.clone();
                    apply_fields(l, o);
                    l.name = name;
                }
            }
        }
        if let Some(d) = manifest.get("default").and_then(Value::as_str)
            && let Some(i) = q.levels.iter().position(|l| l.name == d)
        {
            q.index = i;
        }
        q.current = q.levels[q.index].clone();
        s.quality = q;
    });
}

/// Applies the player's saved choice (PlayerPrefs `platypus.quality`, or `iron.quality` saved
/// before the rename to Platypus), else the default level. The starting state is not a choice to
/// save.
pub fn apply_default_quality() {
    let saved: Option<Value> = prefs::json("platypus.quality")
        .or_else(|| prefs::json("iron.quality"))
        .and_then(|t| serde_json::from_str(&t).ok());
    st(|s| {
        let q = &mut s.quality;
        if let Some(saved) = saved
            && let Some(cur) = saved.get("current")
        {
            if let Some(i) = saved
                .get("level")
                .and_then(Value::as_str)
                .and_then(|n| q.levels.iter().position(|l| l.name == n))
            {
                q.index = i;
            }
            let name = q.levels[q.index].name.clone();
            q.current = q.levels[q.index].clone();
            apply_fields(&mut q.current, cur);
            // Saves from before vsync was the player's (no `playerVSync`, vsync was off by
            // default then) take the level's.
            if saved.get(PLAYER_VSYNC).and_then(Value::as_bool) != Some(true) {
                q.current.v_sync_count = q.levels[q.index].v_sync_count;
            }
            q.current.name = name;
        }
        q.version += 1;
    });
}

/// Marks a saved `platypus.quality` whose `vSyncCount` is the player's choice (the startup
/// video-settings prompt and saves since vsync became on by default write it).
pub const PLAYER_VSYNC: &str = "playerVSync";

/// The player's choice is saved and comes back next time.
fn remember(s: &State) {
    if s.quality.restoring {
        return;
    }
    let text = json!({
        "level": s.quality.levels.get(s.quality.index).map(|l| l.name.clone()),
        "current": level_json(&s.quality.current),
        (PLAYER_VSYNC): true,
    })
    .to_string();
    prefs::set_json("platypus.quality", &text);
}

fn change(f: impl FnOnce(&mut QualityState)) {
    st(|s| {
        f(&mut s.quality.current);
        s.quality.version += 1;
        remember(s);
    });
}

/// Which shadows are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowKind {
    Disable,
    HardOnly,
    All,
}

#[ts_api(
    namespace = "QualitySettings", enums(
        /// Which shadows are drawn: none, only hard ones, or soft ones too.
        ShadowQualityMode = ShadowKind [Disable, HardOnly, All]
    )
)]
pub mod quality_api {
    use super::*;

    /// The names of the levels, lowest first.
    #[property]
    pub fn names() -> Vec<String> {
        st(|s| s.quality.levels.iter().map(|l| l.name.clone()).collect())
    }

    /// A level's settings as JSON text (`{ name, antiAliasing, renderScale, ... }`).
    pub fn level_json(index: f64) -> String {
        st(|s| {
            s.quality
                .levels
                .get(index as usize)
                .map(|l| super::level_json(l).to_string())
                .unwrap_or_default()
        })
    }

    /// The current level's index; changing a setting on its own keeps it.
    pub fn get_quality_level() -> f64 {
        st(|s| s.quality.index as f64)
    }

    /// Switches to a level by index, setting everything it covers. A bad index is ignored.
    pub fn set_quality_level(index: f64) {
        set_level(index as usize);
    }

    /// Switches to a level by name.
    pub fn set_quality_level_by_name(name: &str) {
        if let Some(i) = st(|s| s.quality.levels.iter().position(|l| l.name == name)) {
            set_level(i);
        }
    }

    fn set_level(i: usize) {
        let cbs = st(|s| {
            let l = s.quality.levels.get(i).cloned()?;
            s.quality.index = i;
            s.quality.current = l;
            s.quality.version += 1;
            remember(s);
            Some(s.quality.on_level.snapshot())
        });
        if let Some(cbs) = cbs {
            emit(&cbs, (i as f64,));
        }
    }

    /// Calls `f(level)` after `setQualityLevel`; returns a token for `offLevelChanged`.
    #[subscription(off = "off_level_changed")]
    pub fn on_level_changed(f: Callback<(f64,)>) -> u32 {
        st(|s| s.quality.on_level.add(f))
    }

    pub fn off_level_changed(token: u32) -> bool {
        st(|s| s.quality.on_level.remove(token))
    }

    /// Samples per pixel (1, 2, 4 or 8; lowered to what the GPU supports).
    #[property]
    pub fn anti_aliasing() -> f64 {
        st(|s| f64::from(s.quality.current.anti_aliasing))
    }

    pub fn set_anti_aliasing(n: f64) {
        change(|q| {
            q.anti_aliasing = if n <= 1.0 {
                1
            } else if n <= 2.0 {
                2
            } else if n <= 4.0 {
                4
            } else {
                8
            }
        });
    }

    #[property]
    pub fn render_scale() -> f64 {
        st(|s| s.quality.current.render_scale)
    }

    pub fn set_render_scale(v: f64) {
        change(|q| q.render_scale = v.clamp(0.25, 2.0));
    }

    /// Which shadows are drawn: `disable`, `hardOnly` or `all`.
    #[property]
    pub fn shadows() -> ShadowKind {
        st(|s| shadow_quality_mode_parse(&s.quality.current.shadows).unwrap_or(ShadowKind::All))
    }

    /// Another name is ignored.
    pub fn set_shadows(mode: Option<ShadowKind>) {
        if let Some(mode) = mode {
            change(|q| q.shadows = shadow_quality_mode_name(mode).to_string());
        }
    }

    /// Soft shadows' filter: `low` (3 x 3 samples), `medium` (5 x 5) or `high` (7 x 7).
    #[property]
    #[ts_type = "\"low\" | \"medium\" | \"high\""]
    pub fn soft_shadow_quality() -> String {
        st(|s| s.quality.current.soft_shadow_quality.clone())
    }

    #[ts_type = "\"low\" | \"medium\" | \"high\""]
    pub fn set_soft_shadow_quality(q: &str) {
        if ["low", "medium", "high"].contains(&q) {
            change(|c| c.soft_shadow_quality = q.to_string());
        }
    }

    /// How far from the camera shadows reach, in metres: shorter is sharper.
    #[property]
    pub fn shadow_distance() -> f64 {
        st(|s| s.quality.current.shadow_distance)
    }

    pub fn set_shadow_distance(d: f64) {
        change(|q| q.shadow_distance = d);
    }

    /// Vertical syncs per frame: 0 off, 1 on (`Application.targetFrameRate` caps frames when off).
    #[property]
    pub fn v_sync_count() -> i32 {
        st(|s| s.quality.current.v_sync_count)
    }

    pub fn set_v_sync_count(count: i32) {
        change(|q| q.v_sync_count = i32::from(count > 0));
    }

    /// LOD groups switch as if things were this many times taller on screen.
    #[property]
    pub fn lod_bias() -> f64 {
        st(|s| s.quality.current.lod_bias)
    }

    pub fn set_lod_bias(b: f64) {
        change(|q| q.lod_bias = b.max(0.01));
    }

    /// Sharper textures at grazing angles: `disable`, `enable` or `forceEnable`.
    #[property]
    #[ts_type = "\"disable\" | \"enable\" | \"forceEnable\""]
    pub fn anisotropic_filtering() -> String {
        st(|s| s.quality.current.anisotropic_filtering.clone())
    }

    #[ts_type = "\"disable\" | \"enable\" | \"forceEnable\""]
    pub fn set_anisotropic_filtering(a: &str) {
        if ["disable", "enable", "forceEnable"].contains(&a) {
            change(|q| q.anisotropic_filtering = a.to_string());
        }
    }

    /// Top mip levels skipped, 0 to 3.
    #[property]
    pub fn global_texture_mipmap_limit() -> f64 {
        st(|s| f64::from(s.quality.current.global_texture_mipmap_limit))
    }

    pub fn set_global_texture_mipmap_limit(n: f64) {
        change(|q| q.global_texture_mipmap_limit = n.round().clamp(0.0, 3.0) as u32);
    }

    /// The player's switch for auto exposure (on by default): off, the game's auto exposure
    /// blends out to its manual exposure (over `RenderSettings.exposureTransition`).
    #[property]
    pub fn auto_exposure() -> bool {
        st(|s| s.quality.current.auto_exposure)
    }

    pub fn set_auto_exposure(on: bool) {
        change(|q| q.auto_exposure = on);
    }

    /// Light shafts and god rays on this level (off on Low, the phones' tier).
    #[property]
    pub fn volumetric_fog() -> bool {
        st(|s| s.quality.current.volumetric_fog)
    }

    pub fn set_volumetric_fog(on: bool) {
        change(|q| q.volumetric_fog = on);
    }

    /// Rendering in HDR (off: colours clip at white; bloom, exposure and tonemapping skipped).
    #[property]
    pub fn hdr() -> bool {
        st(|s| s.quality.current.hdr)
    }

    pub fn set_hdr(on: bool) {
        change(|q| q.hdr = on);
    }

    /// Additional lights a camera draws besides the main one, the nearest (-1: no limit).
    #[property]
    pub fn additional_lights() -> f64 {
        st(|s| f64::from(s.quality.current.additional_lights))
    }

    pub fn set_additional_lights(n: f64) {
        change(|q| {
            q.additional_lights = if n < 0.0 {
                -1
            } else {
                n.round().min(256.0) as i32
            }
        });
    }

    /// Whether spot and point lights cast shadows on this level.
    #[property]
    pub fn additional_light_shadows() -> bool {
        st(|s| s.quality.current.additional_light_shadows)
    }

    pub fn set_additional_light_shadows(on: bool) {
        change(|q| q.additional_light_shadows = on);
    }

    /// The sun's shadow cascades: 1, 2 or 4.
    #[property]
    pub fn shadow_cascades() -> f64 {
        st(|s| f64::from(s.quality.current.shadow_cascades))
    }

    pub fn set_shadow_cascades(n: f64) {
        change(|q| {
            q.shadow_cascades = if n >= 4.0 {
                4
            } else if n >= 2.0 {
                2
            } else {
                1
            }
        });
    }

    /// The shadow map's size in texels (256 to 8192).
    #[property]
    pub fn shadow_resolution() -> f64 {
        st(|s| f64::from(s.quality.current.shadow_resolution))
    }

    pub fn set_shadow_resolution(r: f64) {
        change(|q| q.shadow_resolution = r.round().max(1.0) as u32);
    }
}
