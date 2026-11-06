// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What scripts learn about the game and the machine: the game's manifest (its
//! `project.json`) for `Application`, and the OS, hardware, GPU and screen for
//! `SystemInfo`. It reaches scripts as `globalThis.__platypus_info` before the bundle runs.

use std::path::Path;

use platypus_core::project::{LoadOptions, Platform, Project, SystemUser};
use serde_json::{Map, Value, json};

/// The game's details, from `project.json` in its folder (else the `platypus` section of
/// `package.json`): `name`, `title`, `version`, `company`, `identifier` and `icon` (a PNG path).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Manifest {
    pub name: String,
    /// Shown in the window's title bar; the name if not given.
    pub title: String,
    /// `subtitle`: a second line under the title (empty when unset).
    pub subtitle: String,
    pub version: String,
    pub company: String,
    /// Reverse-DNS id (`com.example.game`).
    pub identifier: String,
    /// The window and taskbar icon, a path in the game folder.
    pub icon: Option<String>,
    /// `platypus.quality`: the default quality level and per-level overrides, passed to
    /// scripts' `QualitySettings` as is.
    pub quality: Value,
    /// `platypus.layers`: layer names by number, passed to scripts' `LayerMask` as is.
    pub layers: Value,
    /// `platypus.startScene`: a scene file scripts load at startup.
    pub start_scene: Option<String>,
    /// `platypus.display`: the window's default mode and size (`{ mode, width, height }`);
    /// `resizable: false` keeps the window at that size.
    pub display: Value,
    /// `platypus.runInBackground`: keep running while the window is in the background
    /// (Unity's `Application.runInBackground`, off by default).
    pub run_in_background: bool,
    /// `platypus.fps`: `false` turns the FPS counter off.
    pub fps: Option<bool>,
    /// `platypus.videoSettingsPrompt`: ask for the video settings once at startup
    /// (`video_prompt.rs`).
    pub video_settings_prompt: Option<bool>,
    /// `platypus.lightUnits`: `physical` for real-world units (the sun in lux, lamps in lumens,
    /// glowing surfaces in nits, as Unity 7 and HDRP), or `legacy` (the default: Platypus's own).
    pub physical_units: bool,
    /// `colorSpace`: `srgb` when the game writes world colours as sRGB (as Unity's inspector
    /// does); they are converted to linear for the renderer. `linear` (the default): colours are
    /// linear values already.
    pub srgb_colors: bool,
    /// `renderer`: `vulkan`, `gles`, `cpu` or (left out) chosen from the bundle's tier.
    pub renderer: platypus_core::project::Renderer,
    /// `blending`: how translucent UI colours combine (`srgb`, the default, or `linear`).
    pub blending: platypus_core::project::Blending,
    /// `theme`: the system's light/dark preference (the default), or `light` / `dark` forced.
    pub theme: platypus_core::project::Theme,
}

impl Manifest {
    /// Whether the UI matches `prefers-color-scheme: dark`: the forced theme, else the system's
    /// (`window` is asked where winit knows it: Windows, macOS).
    pub fn dark(&self, window: Option<&platypus_platform::Window>) -> bool {
        self.theme
            .is_dark(|| platypus_platform::theme::prefers_dark(window))
    }

    /// Reads the project of `root` (`project.json`, else the `platypus` section of `package.json`,
    /// see `platypus_core::project`) for this platform and user. A broken file is logged and the
    /// defaults named after the folder are used.
    pub fn load(root: &Path) -> Self {
        let user = SystemUser;
        let opts = LoadOptions::new(Platform::current(), &user);
        match Project::load(root, &opts) {
            Ok(p) => Self::from_project(p),
            Err(e) => {
                tracing::error!("{e}");
                // `.` (the current folder) has no name of its own until it's made absolute.
                let absolute = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
                let folder = absolute
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "game".into());
                Self::from_json(&Value::Null, &folder)
            }
        }
    }

    /// From a `package.json` value (its `platypus` section), without templates.
    pub fn from_json(json: &Value, folder: &str) -> Self {
        Self::from_project(Project::from_package_json(json, folder))
    }

    /// The window's title (the game's window and its video-settings prompt): the title, else
    /// the name, else "Platypus".
    pub fn window_title(&self) -> &str {
        [self.title.as_str(), self.name.as_str()]
            .into_iter()
            .find(|s| !s.trim().is_empty())
            .unwrap_or("Platypus")
    }

    /// The window and taskbar icon as RGBA: the project's `icon`, else Platypus's.
    pub fn window_icon(&self, root: &Path) -> (Vec<u8>, u32, u32) {
        self.icon
            .as_deref()
            .and_then(|path| load_icon(root, path))
            .unwrap_or_else(default_icon)
    }

    pub fn from_project(p: Project) -> Self {
        let physical_units = light_units(&p.extra);
        let srgb_colors = color_space(&p.extra);
        for w in &p.warnings {
            tracing::warn!("project.json: {w}");
        }
        Self {
            name: p.name,
            title: p.title,
            subtitle: p.subtitle.unwrap_or_default(),
            version: p.version,
            company: p.company,
            identifier: p.identifier,
            icon: p.icon,
            quality: p.quality,
            layers: p.layers,
            start_scene: p.start_scene,
            display: p.display,
            run_in_background: p.run_in_background,
            fps: p.fps,
            video_settings_prompt: p.video_settings_prompt,
            physical_units,
            srgb_colors,
            renderer: p.renderer,
            blending: p.blending,
            theme: p.theme,
        }
    }
}

/// `colorSpace`: `srgb` or `linear` (the default).
fn color_space(platypus: &serde_json::Map<String, Value>) -> bool {
    match platypus.get("colorSpace").and_then(Value::as_str) {
        None | Some("linear") => false,
        Some("srgb") => true,
        Some(other) => {
            tracing::warn!("colorSpace `{other}`: not srgb or linear");
            false
        }
    }
}

/// `platypus.lightUnits`, warning about the old pipeline settings: Platypus has one render pipeline,
/// as Unity 7 (its URP), whose settings come with the quality levels.
fn light_units(platypus: &serde_json::Map<String, Value>) -> bool {
    if platypus.contains_key("renderPipeline") || platypus.contains_key("renderPipelineSettings") {
        tracing::warn!(
            "platypus.renderPipeline: Platypus has one render pipeline (as Unity 7): set its features \
             per quality level (platypus.quality.levels: volumetricFog, hdr, additionalLights, \
             additionalLightShadows), and real-world light units with platypus.lightUnits"
        );
    }
    match platypus.get("lightUnits").and_then(Value::as_str) {
        None | Some("legacy") => false,
        Some("physical") => true,
        Some(other) => {
            tracing::warn!("platypus.lightUnits `{other}`: not physical or legacy");
            false
        }
    }
}

/// The Platypus icon as RGBA, for games without their own (made from `resources/icon/icon.png`
/// by the build script).
pub fn default_icon() -> (Vec<u8>, u32, u32) {
    const RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/window_icon.rgba"));
    (RGBA.to_vec(), 256, 256)
}

/// Reads a PNG icon from the game folder as RGBA (`None` if it can't).
pub fn load_icon(root: &Path, path: &str) -> Option<(Vec<u8>, u32, u32)> {
    let file = platypus_core::resolve_asset(root, path)?;
    match image::open(&file) {
        Ok(image) => {
            let image = image.to_rgba8();
            let (w, h) = image.dimensions();
            Some((image.into_raw(), w, h))
        }
        Err(e) => {
            tracing::warn!("icon `{path}`: {e}");
            None
        }
    }
}

/// Everything scripts get, as one JSON object. Starts with the manifest and the
/// machine; the GPU and screen join once the window and renderer exist.
#[derive(Clone, Debug)]
pub struct Info(pub Map<String, Value>);

impl Info {
    pub fn new(manifest: &Manifest) -> Self {
        let mut map = Map::new();
        map.insert(
            "app".into(),
            json!({
                "name": manifest.name,
                "productName": manifest.title,
                "subtitle": manifest.subtitle,
                "version": manifest.version,
                "companyName": manifest.company,
                "identifier": manifest.identifier,
                "engineVersion": env!("CARGO_PKG_VERSION"),
                "quality": manifest.quality,
                "layers": manifest.layers,
                "startScene": manifest.start_scene,
                "runInBackground": manifest.run_in_background,
                "lightUnits": if manifest.physical_units { "physical" } else { "legacy" },
                "colorSpace": if manifest.srgb_colors { "srgb" } else { "linear" },
                "targetFrameRate": -1,
            }),
        );
        map.insert("system".into(), system());
        Self(map)
    }

    /// Sets a top-level section (`prefs`, `display`).
    pub fn set(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_owned(), value);
    }

    /// Sets a field of the `app` section, so scripts reloaded later see what the
    /// old ones chose (`runInBackground`, `targetFrameRate`).
    pub fn set_app(&mut self, key: &str, value: Value) {
        if let Some(Value::Object(app)) = self.0.get_mut("app") {
            app.insert(key.to_owned(), value);
        }
    }

    /// Adds (or replaces) fields of the `system` section.
    pub fn set_system(&mut self, fields: Value) {
        if let (Some(Value::Object(system)), Value::Object(fields)) =
            (self.0.get_mut("system"), fields)
        {
            system.extend(fields);
        }
    }
}

/// The OS and hardware, as far as the platform tells.
fn system() -> Value {
    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    let family = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "MacOSX",
        "linux" | "freebsd" | "openbsd" | "netbsd" => "Linux",
        "android" => "Android",
        "ios" => "iOS",
        _ => "Other",
    };
    let device_type = match std::env::consts::OS {
        "android" | "ios" => "Handheld",
        _ => "Desktop",
    };
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    json!({
        "operatingSystem": System::long_os_version().unwrap_or_else(|| family.to_owned()),
        "operatingSystemFamily": family,
        "operatingSystemVersion": System::os_version().unwrap_or_default(),
        "deviceType": device_type,
        "deviceName": System::host_name().unwrap_or_default(),
        "processorType": sys.cpus().first().map(|c| c.brand().trim().to_owned()).unwrap_or_default(),
        "processorCount": cpus,
        "systemMemorySize": sys.total_memory() / (1024 * 1024),
        "architecture": std::env::consts::ARCH,
        "language": sys_locale::get_locale().unwrap_or_else(|| "en-US".into()),
        "touchSupported": device_type == "Handheld",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What scripts read, as JSON text.
    fn text(info: &Info) -> String {
        Value::Object(info.0.clone()).to_string()
    }

    // M9-ICO-03
    #[test]
    fn the_default_window_icon_is_the_platypus_on_a_transparent_tile() {
        let (rgba, w, h) = default_icon();
        assert_eq!((w, h, rgba.len()), (256, 256, 256 * 256 * 4));
        let alpha = |x: u32, y: u32| rgba[((y * 256 + x) * 4 + 3) as usize];
        // Transparent corners, an opaque middle.
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(128, 128), 255);
    }

    // M5-APP-01
    #[test]
    fn manifests_read_package_json_with_fallbacks() {
        let m = Manifest::from_json(
            &json!({
                "name": "fox-run",
                "version": "1.2.0",
                "author": "Someone",
                "platypus": { "title": "Fox Run", "icon": "assets/icon.png" }
            }),
            "platformer",
        );
        assert_eq!(m.title, "Fox Run");
        assert_eq!(m.version, "1.2.0");
        assert_eq!(m.company, "Someone");
        assert_eq!(m.icon.as_deref(), Some("assets/icon.png"));
        let bare = Manifest::from_json(&Value::Null, "platformer");
        assert_eq!(
            (bare.name.as_str(), bare.title.as_str()),
            ("platformer", "platformer")
        );

        // The info carries the app and system sections, and later additions.
        let mut info = Info::new(&m);
        info.set_system(json!({ "graphicsDeviceName": "Test GPU" }));
        let script = text(&info);
        assert!(script.contains("\"productName\":\"Fox Run\""));
        assert!(script.contains("\"graphicsDeviceName\":\"Test GPU\""));
        assert!(script.contains("\"processorCount\":"));
    }

    // M9-API-1320
    #[test]
    fn a_folder_with_project_json_uses_it_for_title_identifier_and_start_scene() {
        let d = std::env::temp_dir().join(format!("platypus_app_manifest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("package.json"),
            r#"{ "name": "npm-name", "platypus": { "title": "FROM PACKAGE" } }"#,
        )
        .unwrap();
        std::fs::write(
            d.join("project.json"),
            r#"{ "name": "fox", "title": "Fox ${env:PLATYPUS_NO_SUCH_VAR_1320:-Run}", "identifier": "dev.platypus.fox",
                 "startScene": "scenes/a.scene.json", "layers": { "8": "Detail" },
                 "windows": { "fps": false }, "macos": { "fps": false }, "linux": { "fps": false } }"#,
        )
        .unwrap();
        let m = Manifest::load(&d);
        assert_eq!(m.title, "Fox Run");
        // package.json is not read when a project.json exists.
        assert_eq!(m.name, "fox");
        assert_eq!(m.identifier, "dev.platypus.fox");
        assert_eq!(m.start_scene.as_deref(), Some("scenes/a.scene.json"));
        assert_eq!(m.layers["8"], "Detail");
        assert_eq!(m.fps, Some(false));
        // A broken template leaves the defaults and does not stop the game.
        std::fs::write(
            d.join("project.json"),
            r#"{ "name": "fox", "title": "${nope}" }"#,
        )
        .unwrap();
        assert_eq!(
            Manifest::load(&d).title,
            d.file_name().unwrap().to_string_lossy()
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-API-910
    #[test]
    fn fps_comes_from_the_manifest() {
        assert_eq!(
            Manifest::from_json(&json!({ "platypus": { "fps": false } }), "g").fps,
            Some(false)
        );
        assert_eq!(Manifest::from_json(&Value::Null, "g").fps, None);
    }

    // M9-VSP-12
    #[test]
    fn the_window_shows_the_games_title_and_icon_else_platypus_ones() {
        let mut m = Manifest::from_json(&json!({ "platypus": { "title": "Fox Run" } }), "fox");
        assert_eq!(m.window_title(), "Fox Run");
        // M9-VSP-13: the subtitle, empty when unset.
        assert_eq!(m.subtitle, "");
        let sub = Manifest::from_json(
            &json!({ "platypus": { "title": "Game Title", "subtitle": "Shadow of the Realm" } }),
            "g",
        );
        assert_eq!(sub.subtitle, "Shadow of the Realm");
        assert!(text(&Info::new(&sub)).contains("\"subtitle\":\"Shadow of the Realm\""));
        m.title = String::new();
        m.name = "fox".into();
        assert_eq!(m.window_title(), "fox");
        m.name = " ".into();
        assert_eq!(m.window_title(), "Platypus");
        // No icon, or one that can't be read: Platypus's.
        let root = std::env::temp_dir();
        assert_eq!(m.window_icon(&root), default_icon());
        m.icon = Some("no-such-icon-9131.png".into());
        assert_eq!(m.window_icon(&root), default_icon());
        let d = root.join(format!("platypus_app_icon_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        image::save_buffer(
            d.join("i.png"),
            &[255, 0, 0, 255],
            1,
            1,
            image::ColorType::Rgba8,
        )
        .unwrap();
        m.icon = Some("i.png".into());
        assert_eq!(m.window_icon(&d), (vec![255, 0, 0, 255], 1, 1));
        let _ = std::fs::remove_dir_all(&d);
    }

    // M9-VSP-06
    #[test]
    fn the_video_settings_prompt_comes_from_the_manifest_and_is_off_by_default() {
        let prompt = |v: Value| Manifest::from_json(&v, "g").video_settings_prompt;
        assert_eq!(
            prompt(json!({ "platypus": { "videoSettingsPrompt": true } })),
            Some(true)
        );
        assert_eq!(
            prompt(json!({ "platypus": { "videoSettingsPrompt": false } })),
            Some(false)
        );
        assert_eq!(prompt(Value::Null), None);
    }

    // M7-RP-01
    #[test]
    fn light_units_come_from_the_manifest_and_old_pipelines_are_gone() {
        let units = |v: Value| Manifest::from_json(&json!({ "platypus": v }), "g").physical_units;
        assert!(units(json!({ "lightUnits": "physical" })));
        assert!(!units(json!({ "lightUnits": "legacy" })));
        assert!(!units(json!({})));
        assert!(!units(json!({ "lightUnits": "candles" })));
        // RDY-RND-01: the colour space.
        let srgb = |v: Value| Manifest::from_json(&json!({ "platypus": v }), "g").srgb_colors;
        assert!(srgb(json!({ "colorSpace": "srgb" })));
        assert!(!srgb(json!({})));
        assert!(!srgb(json!({ "colorSpace": "linear" })));
        assert!(!srgb(json!({ "colorSpace": "hsv" })));
        // The old pipeline choice no longer picks anything (it only warns).
        assert!(!units(json!({ "renderPipeline": "hd" })));
        let script = text(&Info::new(&Manifest::from_json(
            &json!({ "platypus": { "lightUnits": "physical" } }),
            "g",
        )));
        assert!(script.contains("\"lightUnits\":\"physical\""));
        assert!(!script.contains("renderPipeline"));
    }

    // M5-APP-09
    #[test]
    fn run_in_background_comes_from_the_manifest_and_follows_scripts() {
        let m = Manifest::from_json(&json!({ "platypus": { "runInBackground": true } }), "g");
        assert!(m.run_in_background);
        assert!(!Manifest::from_json(&Value::Null, "g").run_in_background);
        let mut info = Info::new(&m);
        assert!(text(&info).contains("\"runInBackground\":true"));
        info.set_app("runInBackground", json!(false));
        info.set_app("targetFrameRate", json!(30));
        let script = text(&info);
        assert!(script.contains("\"runInBackground\":false"));
        assert!(script.contains("\"targetFrameRate\":30"));
    }
}
