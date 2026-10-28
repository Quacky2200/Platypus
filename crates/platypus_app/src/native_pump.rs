// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What compiled games hand to the app each frame, apart from drawing: audio commands for the
//! mixer and the finished sounds going back, `Application` requests,
//! and `PlayerPrefs` (`platypus_engine_api::{audio, app, prefs}`). The window, quality and cursor are
//! `native_render.rs`.

use super::App;
use super::{info, prefs};
use platypus_core::Value;
use platypus_engine_api::app::{AppInfo, Request, SystemInfo};
use platypus_engine_api::audio::{AudioCommand, PlaybackRef};
use platypus_engine_api::prefs::{Change, Pref};

impl App {
    /// Applies the audio commands compiled code queued to the mixer. The mixer opens on the first
    /// command.
    pub(super) fn native_audio_commands(&mut self) {
        if self.native.is_none() {
            return;
        }
        for c in platypus_engine_api::audio::take_commands() {
            match &c {
                AudioCommand::Load {
                    sound,
                    path,
                    stream,
                } => self.audio_op(
                    "load",
                    &[
                        Value::Number(f64::from(sound.0)),
                        Value::String(path.clone()),
                        Value::Bool(*stream),
                    ],
                ),
                other => {
                    if let Some((name, args)) = other.to_op() {
                        let args: Vec<Value> = args.iter().map(core_value).collect();
                        self.audio_op(name, &args);
                    }
                }
            }
        }
    }

    /// Files changed on disk: `Assets.onChanged` closures run, textures that reloaded report
    /// their size.
    pub(super) fn native_assets_changed(&mut self, keys: &[String], textures: &[(u32, u32, u32)]) {
        let Some(n) = self.native.as_mut() else {
            return;
        };
        let (keys, textures) = (keys.to_vec(), textures.to_vec());
        n.with_scene(&mut self.scene.scene, &mut self.ui, move || {
            for (id, w, h) in textures {
                platypus_render_api::texture_loaded(platypus_render_api::TextureRef(id), w, h);
            }
            platypus_engine_api::assets::changed(keys);
        });
    }

    /// Sounds the mixer finished go back to the program (`onEnded`, `AudioSource` state).
    pub(super) fn native_audio_ended(&mut self, ended: &[u32]) {
        let ids: Vec<PlaybackRef> = ended.iter().map(|i| PlaybackRef(*i)).collect();
        if let Some(n) = self.native.as_mut() {
            n.audio_ended(&mut self.scene.scene, &mut self.ui, &ids);
        }
    }

    /// What the program asked of `Application` and `PlayerPrefs`, applied. Runs after each call into the program.
    pub(super) fn native_requests(&mut self) {
        if self.native.is_none() {
            return;
        }
        for r in platypus_engine_api::app::take_requests() {
            match r {
                Request::Quit(code) => {
                    super::EXIT_CODE.store(code, std::sync::atomic::Ordering::Relaxed);
                    self.done = true;
                }
                Request::TargetFrameRate(fps) => self.target_frame_rate = fps,
                Request::RunInBackground(on) => {
                    self.lifecycle.run_in_background = on;
                    self.sync_lifecycle();
                }
                Request::ShowVideoSettings => self.open_video_settings(),
            }
        }
        for c in platypus_engine_api::prefs::take_changes() {
            match c {
                Change::Set(k, Pref::Num(n)) => self.prefs.set(&k, serde_json::json!(n)),
                Change::Set(k, Pref::Str(s)) => self.prefs.set(&k, serde_json::json!(s)),
                Change::Delete(k) => self.prefs.delete(&k),
                Change::DeleteAll => self.prefs.delete_all(),
                Change::Save => self.prefs.save(),
            }
        }
    }
}

/// The saved settings as the engine's `PlayerPrefs` start out. Once, before the first load.
pub fn seed_prefs(saved: &prefs::Prefs) {
    platypus_engine_api::prefs::load(saved.all().iter().filter_map(|(k, v)| {
        let pref = match v {
            serde_json::Value::Number(n) => Pref::Num(n.as_f64()?),
            serde_json::Value::String(s) => Pref::Str(s.clone()),
            serde_json::Value::Bool(b) => Pref::Num(f64::from(u8::from(*b))),
            _ => return None,
        };
        Some((k.clone(), pref))
    }));
}

/// Gives `Assets.exists` / `list` / `packOf` the index the host put in `info` (`assetIndex`).
pub fn seed_asset_index(info: &info::Info) {
    let rows = info
        .0
        .get("assetIndex")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r[0].as_str()?.to_owned(), r[1].as_str()?.to_owned())));
    platypus_engine_api::assets::set_asset_index(rows.collect());
}

/// What the program reads at start and after each reload: `Application`, `SystemInfo`, the
/// layer names, and the quality levels (the manifest's, then the player's saved choice).
pub fn configure_engine(manifest: &info::Manifest, info: &info::Info) {
    platypus_engine_api::app::set_app(AppInfo {
        name: manifest.name.clone(),
        product_name: manifest.title.clone(),
        subtitle: manifest.subtitle.clone(),
        version: manifest.version.clone(),
        company_name: manifest.company.clone(),
        identifier: manifest.identifier.clone(),
        engine_version: env!("CARGO_PKG_VERSION").into(),
        is_editor: platypus_engine_api::editor::enabled(),
        run_in_background: manifest.run_in_background,
        target_frame_rate: -1,
        args: info.0["app"]["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| Some(a.as_str()?.to_owned()))
            .collect(),
    });
    platypus_render_api::set_physical_light_units(manifest.physical_units);
    crate::color_space::set_srgb(manifest.srgb_colors);
    seed_asset_index(info);
    let sys = info.0.get("system").cloned().unwrap_or_default();
    let text = |k: &str| sys[k].as_str().unwrap_or_default().to_owned();
    let num = |k: &str| sys[k].as_f64().unwrap_or(0.0) as u32;
    platypus_engine_api::app::set_system(SystemInfo {
        operating_system: text("operatingSystem"),
        operating_system_family: text("operatingSystemFamily"),
        operating_system_version: text("operatingSystemVersion"),
        device_type: text("deviceType"),
        device_name: text("deviceName"),
        processor_type: text("processorType"),
        processor_count: num("processorCount").max(1),
        system_memory_size: num("systemMemorySize"),
        architecture: text("architecture"),
        language: text("language"),
        touch_supported: sys["touchSupported"].as_bool().unwrap_or(false),
        ..SystemInfo::default()
    });
    if let Some(list) = info.0.get("assets").and_then(serde_json::Value::as_array) {
        platypus_engine_api::assets::set_assets(
            list.iter()
                .filter_map(|a| {
                    Some(platypus_engine_api::assets::AssetEntry {
                        path: a["path"].as_str()?.to_owned(),
                        guid: a["guid"].as_str()?.to_owned(),
                        importer_json: a["importer"].to_string(),
                    })
                })
                .collect(),
        );
    }
}

/// The engine API's JSON values as the values ops carry.
fn core_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(a) => Value::Array(a.iter().map(core_value).collect()),
        serde_json::Value::Object(_) => Value::Null,
    }
}
