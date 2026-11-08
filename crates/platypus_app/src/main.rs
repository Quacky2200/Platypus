// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// A windowed program: no console window of its own (the log goes to a file, see
// `logging`); tests keep the console harness.
#![cfg_attr(not(test), windows_subsystem = "windows")]
//! `platypus-app [game] [--screenshot out.png] [--frames N] [--renderer vulkan|gles|cpu]`: opens a
//! window, loads the game's script bundle and runs it, drawn by the renderer `host.rs` picks.
//!
//! `[game]` is either a game folder (loads `<game>/build/game.pbundle`, built with
//! `platypus-build build`) or a bundle (or a `.ts` file) directly; without one, the current folder
//! is the game. `--screenshot` saves the UI after `N` frames (default 60) and exits, which is handy
//! for checking output.

pub(crate) mod anim_cull;
pub(crate) mod asset_db;
mod asset_root;
mod bench;
pub(crate) mod bone_cache;
mod canvas_cameras;
mod cdp;
mod cloth_sync;
pub(crate) mod color_space;
pub(crate) mod cook;
mod cpu_gfx;
mod crash;
mod crash_dialog;
mod cursor;
mod dap;
mod debugger;
mod display;
mod file_drop;
mod fps;
mod frame_target;
mod game_input;
mod gles_gfx;
mod headless;
mod host;
mod identity;
pub(crate) mod info;
mod lazy_world;
mod licenses;
mod lifecycle;
mod logging;
mod main_window;
mod mem_trace;
pub(crate) mod models;
mod models_fbx;
mod native;
mod native_pump;
mod net_host;
mod native_render;
mod notice;
mod panels;
pub(crate) mod particles;
mod payload;
mod physics_sync;
pub(crate) mod postfx;
mod prefs;
mod profile;
pub(crate) mod reload;
mod render_sync;
pub(crate) mod sync_profile;
mod ui_frame;
mod ui_inspector;
mod video_prompt;
mod windows;
pub(crate) mod world_view;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use platypus_platform::{
    CursorIcon, CustomCursorRequest, ImeEvent, InputEvent, MouseButton, Platform, PlatformApp,
    WheelDelta, Window, WindowConfig,
};
use platypus_render::wgpu;
use platypus_render::{Color, Gpu, OffscreenTarget, RenderTarget, UiRenderer, WorldRenderer};
use web_ui_dom::{Clipboard, Input, Interaction, UiTree, events};

const TICK: f64 = 1.0 / 60.0;
/// Stops a slow frame from triggering a long burst of catch-up ticks.
const MAX_TICKS_PER_FRAME: u32 = 5;
/// The longest frame `onUpdate` is told about (a hitch shouldn't teleport things).
const MAX_FRAME_DT: f64 = 0.1;
/// Pixels per wheel notch ("line"), like browsers.
const WHEEL_LINE: f32 = 40.0;
const CLEAR: Color = Color::rgb(0.05, 0.06, 0.09);

struct Args {
    game: PathBuf,
    screenshot: Option<PathBuf>,
    frames: u32,
    /// `--frames` was on the command line (a headless run stops after that many iterations).
    frames_given: bool,
    /// Cook the game's assets (`--cook`) and exit.
    cook: bool,
    /// `--fps` / `--no-fps`: the FPS counter on or off, over the defaults.
    fps: Option<bool>,
    /// `--data <dir>`: where a packaged game's assets are (over `$PLATYPUS_DATA` and `data/` beside
    /// the exe).
    data: Option<PathBuf>,
    /// The folder assets are read from when it is not the game folder (a packaged game).
    asset_root: Option<PathBuf>,
    /// Everything after `--`, for the game's scripts (`Application.args`).
    script_args: Vec<String>,
    /// `--bench`: a windowless timed run (see `bench.rs`).
    bench: Option<bench::BenchArgs>,
    /// `--headless`: no window, GPU or audio device, for servers and tests (see `headless.rs`).
    headless: Option<headless::HeadlessArgs>,
    /// `--crash-dialog <folder>`: the crash dialog's own run (`crash_dialog.rs`), whatever game
    /// a packaged exe holds.
    crash_dialog: bool,
    /// `--video-settings-prompt` / `--no-video-settings-prompt`: the startup video-settings prompt
    /// forced on or off (`video_prompt.rs`).
    video_prompt: Option<bool>,
    /// `--video-settings-dialog <folder>`: the prompt's own run, UI only.
    video_dialog: bool,
    /// `--renderer vulkan|gles|cpu|auto`: the renderer, over the project's `renderer` and the
    /// bundle's tier (`host.rs`).
    renderer: platypus_core::project::Renderer,
    /// `--project <folder>`: the game is the editor (`editor/`), working on this project
    /// (`Editor.*`, `Application.isEditor`).
    project: Option<PathBuf>,
    /// `--debug-port <port>`: a debug build of the game with a controller on this local port
    /// (`debugger.rs`).
    debug_port: Option<u16>,
    /// `--dap-port <port>`: the same, speaking the Debug Adapter Protocol (`dap.rs`); the game waits
    /// for the debugger's `configurationDone`.
    dap_port: Option<u16>,
    /// `--inspect-brk`: the controller (port 9229 unless `--debug-port` / `--dap-port` says) and a stop
    /// before the program's first statement.
    inspect_brk: bool,
    /// `--inspect-ts-tool devtools|dap|console`: which protocol `--inspect-brk` serves on 9229
    /// (`devtools`, the default: Chrome DevTools / Node tooling; `dap`: editors; `console`: text).
    inspect_tool: InspectTool,
    /// `--devtools-port <port>`: the DevTools protocol server on this port (`cdp.rs`).
    cdp_port: Option<u16>,
    /// A packaged single-file game (its code in the exe): it registers its app id and keeps a
    /// Start-menu shortcut (`identity.rs`).
    packaged: bool,
}

/// The protocol `--inspect-brk` serves.
#[derive(Clone, Copy, PartialEq)]
enum InspectTool {
    Devtools,
    Dap,
    Console,
}

impl Args {
    /// No player: a screenshot or bench run.
    fn offline(&self) -> bool {
        self.screenshot.is_some() || self.bench.is_some() || self.headless.is_some()
    }
}

/// Why a JavaScript bundle (the QuickJS backend's `game.js`) is refused.
const QUICKJS_REMOVED: &str = "is a QuickJS bundle: QuickJS is no longer part of Platypus; rebuild the game with `platypus-build build` (writes build/game.pbundle)";

fn parse_args() -> Result<Args> {
    let mut args = parse_args_from(std::env::args_os().skip(1))?;
    // The crash dialog and the video-settings prompt run their own folders, not a packaged game.
    if args.crash_dialog || args.video_dialog {
        return Ok(args);
    }
    if let Some(dir) = payload::embedded_game() {
        // A packaged single-file game runs its own code (extracted from the exe), whatever the
        // arguments say, and reads its assets from the data folder beside the exe.
        let exe = std::env::current_exe().ok();
        let env = std::env::var_os("PLATYPUS_DATA");
        args.asset_root = Some(
            asset_root::locate_data(&asset_root::DataSearch {
                flag: args.data.as_deref(),
                env: env.as_deref(),
                exe_dir: exe.as_deref().and_then(Path::parent),
                code_dir: &dir,
            })
            .map_err(anyhow::Error::msg)?,
        );
        args.game = dir;
        args.packaged = true;
    }
    Ok(args)
}

/// Parses the arguments after the program name. The game is the first argument that isn't
/// an option, or the current folder when there's none.
fn parse_args_from(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Args> {
    let usage = "usage: platypus-app [game folder | game.pbundle | main.ts] [--screenshot out.png] [--frames N] [--headless [--tick-rate N]] [--cook] [--fps | --no-fps] [--renderer vulkan|gles|cpu] [--video-settings-prompt | --no-video-settings-prompt] [--data folder] [--project folder] [-- script args]
       platypus-app editor --project <folder> (the editor, working on a project folder)
       platypus-app [game] --bench [--bench-input spec.json] [--bench-out out.json] [--warmup N] [--frames N] [--fixed-dt seconds] [--size WxH]
       platypus-app --licenses (prints the third-party notices built into the program and exits)
games are built with `platypus-build build`; arguments after -- reach scripts as Application.args;
--bench runs without a window as fast as it can and writes timings as JSON (docs/platypus-build.md, \"Benchmarks\")";
    let mut args = args.into_iter();
    let mut game = None;
    let mut parsed = Args {
        game: PathBuf::from("."),
        screenshot: None,
        frames: 60,
        frames_given: false,
        cook: false,
        fps: None,
        data: None,
        asset_root: None,
        script_args: Vec::new(),
        bench: None,
        headless: None,
        crash_dialog: false,
        video_prompt: None,
        video_dialog: false,
        renderer: platypus_core::project::Renderer::Auto,
        project: None,
        debug_port: None,
        dap_port: None,
        inspect_brk: false,
        inspect_tool: InspectTool::Devtools,
        cdp_port: None,
        packaged: false,
    };
    let mut frames_given = false;
    while let Some(arg) = args.next() {
        if let Some(flag) = arg.to_str().filter(|s| bench::is_flag(s)) {
            let bench = parsed.bench.get_or_insert_with(bench::BenchArgs::default);
            bench.flag(flag, &mut args).context(usage)?;
            continue;
        }
        if let Some(flag) = arg.to_str().filter(|s| headless::is_flag(s)) {
            let headless = parsed
                .headless
                .get_or_insert_with(headless::HeadlessArgs::default);
            headless.flag(flag, &mut args).context(usage)?;
            continue;
        }
        match arg.to_str() {
            Some("--data") => parsed.data = Some(args.next().context(usage)?.into()),
            Some("--project") => parsed.project = Some(args.next().context(usage)?.into()),
            Some("--screenshot") => parsed.screenshot = Some(args.next().context(usage)?.into()),
            Some("--cook") => parsed.cook = true,
            Some("--inspect-brk") => parsed.inspect_brk = true,
            Some("--inspect-ts-tool") => {
                parsed.inspect_tool = match args.next().context(usage)?.to_str() {
                    Some("devtools") => InspectTool::Devtools,
                    Some("dap") => InspectTool::Dap,
                    Some("console") => InspectTool::Console,
                    _ => anyhow::bail!("--inspect-ts-tool takes devtools, dap or console"),
                }
            }
            Some("--devtools-port") => {
                parsed.cdp_port = Some(
                    args.next()
                        .context(usage)?
                        .to_str()
                        .context(usage)?
                        .parse()?,
                )
            }
            Some("--dap-port") => {
                parsed.dap_port = Some(
                    args.next()
                        .context(usage)?
                        .to_str()
                        .context(usage)?
                        .parse()?,
                )
            }
            Some("--debug-port") => {
                parsed.debug_port = Some(
                    args.next()
                        .context(usage)?
                        .to_str()
                        .context(usage)?
                        .parse()?,
                )
            }
            Some(crash_dialog::DIALOG_FLAG) => {
                parsed.crash_dialog = true;
                game = Some(args.next().context(usage)?.into());
            }
            Some(video_prompt::DIALOG_FLAG) => {
                parsed.video_dialog = true;
                game = Some(args.next().context(usage)?.into());
            }
            Some(video_prompt::PROMPT_FLAG) => parsed.video_prompt = Some(true),
            Some(video_prompt::NO_PROMPT_FLAG) => parsed.video_prompt = Some(false),
            Some("--renderer") => {
                let v = args.next().context(usage)?;
                parsed.renderer = v
                    .to_str()
                    .and_then(platypus_core::project::Renderer::parse)
                    .with_context(|| format!("--renderer {v:?}: expected vulkan, gles or cpu"))?;
            }
            Some("--fps") => parsed.fps = Some(true),
            Some("--no-fps") => parsed.fps = Some(false),
            Some("--") => {
                parsed.script_args = args.map(|a| a.to_string_lossy().into_owned()).collect();
                break;
            }
            Some("--frames") => {
                frames_given = true;
                parsed.frames = args
                    .next()
                    .and_then(|n| n.to_str()?.parse().ok())
                    .context("--frames needs a number")?
            }
            Some(s) if game.is_none() && !s.starts_with("--") => game = Some(PathBuf::from(s)),
            None if game.is_none() => game = Some(PathBuf::from(arg)),
            _ => bail!("unexpected argument {arg:?}\n{usage}"),
        }
    }
    if let Some(game) = game {
        parsed.game = game;
    }
    parsed.frames_given = frames_given;
    // `--frames` is the measured frames of a bench run without an input file.
    if let Some(bench) = parsed.bench.as_mut()
        && frames_given
    {
        bench.frames = parsed.frames;
    }
    Ok(parsed)
}

/// The process's exit code, set by `Application.quit(code)`.
static EXIT_CODE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub(crate) fn main() -> Result<()> {
    // Started by a game that crashed: show its crash dialog, nothing else.
    if let Some(code) = crash_dialog::main_if_asked() {
        std::process::exit(code);
    }
    // `--licenses`: the notices, nothing else (no window, log or unpacked payload).
    if licenses::asked(std::env::args_os().skip(1)) {
        licenses::print();
        return Ok(());
    }
    bench::mark_start();
    mem_trace::install();
    crash::install();
    logging::attach_console();
    let result = run();
    if let Err(e) = &result {
        // Into the log as well as stderr: a windowed program's stderr may go nowhere.
        tracing::error!("{e:#}");
    }
    let code = EXIT_CODE.load(std::sync::atomic::Ordering::Relaxed);
    if result.is_ok() && code != 0 {
        std::process::exit(code);
    }
    result
}

/// Threads the animation update poses animators on: `PLATYPUS_ANIM_THREADS` (1: in turn), else
/// the machine's threads, up to 16 (more gain little: the writing stays in turn).
fn animation_threads() -> usize {
    std::env::var("PLATYPUS_ANIM_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get().min(16)))
}

/// Where the editor's world loads assets from: the project folder (`--project`), made absolute
/// like `Editor.project`.
fn editor_asset_root(project: &Path) -> PathBuf {
    std::path::absolute(project).unwrap_or_else(|_| project.to_path_buf())
}

fn run() -> Result<()> {
    let mut args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            logging::init(None);
            return Err(e);
        }
    };
    // `--bench` reports where start-up goes (`startupParts`).
    if args.bench.is_some() {
        web_ui_ts::timing::enable();
    }
    platypus_animation::set_threads(animation_threads());
    let mut lap = web_ui_ts::timing::Lap::start();
    // The log file sits with the game's settings (not for screenshot runs, which leave
    // the player's files alone; `PLATYPUS_LOG_FILE` still works there).
    let manifest_dir = match resolve_bundle(&args.game) {
        Ok((_, root)) => Some(root),
        Err(_) => None,
    };
    // The editor keeps the log for its console from the start.
    if let Some(project) = &args.project {
        let ui_root = manifest_dir.clone().unwrap_or_else(|| args.game.clone());
        platypus_engine_api::editor::enable(project, &ui_root);
    }
    let manifest = manifest_dir.as_deref().map(info::Manifest::load);
    let settings = manifest
        .as_ref()
        .and_then(prefs::settings_dir)
        .filter(|_| !args.offline());
    let log = logging::init(logging::log_path(settings.as_deref()));
    if let Some(log) = &log {
        tracing::info!("log: {}", log.display());
    }
    if args.inspect_brk {
        debugger::break_at_start();
        if args.debug_port.is_none() && args.dap_port.is_none() && args.cdp_port.is_none() {
            match args.inspect_tool {
                InspectTool::Devtools => args.cdp_port = Some(9229),
                InspectTool::Dap => args.dap_port = Some(9229),
                InspectTool::Console => args.debug_port = Some(9229),
            }
        }
    }
    if let Some(port) = args.debug_port {
        match debugger::enable(port) {
            Ok(p) => eprintln!("debugger: listening on 127.0.0.1:{p}"),
            Err(e) => tracing::error!("debugger: cannot listen on port {port}: {e}"),
        }
    }
    if let Some(root) = &manifest_dir {
        crash::note_game_dir(root);
    }
    // A source file run directly is its own bundle: no sidecar to map lines through.
    if args.game.is_file()
        && let Some(name) = args.game.file_name().and_then(|n| n.to_str())
        && (name.ends_with(".ts") || name.ends_with(".tsx"))
    {
        debugger::note_single_file(name);
    }
    if let Some(port) = args.cdp_port {
        match cdp::enable(port) {
            Ok(p) => {
                eprintln!("debugger: DevTools protocol on 127.0.0.1:{p} (ws://127.0.0.1:{p}/game)");
                // Like node --inspect-brk: the game starts when the debugger lets it run.
                debugger::wait_for_configuration();
            }
            Err(e) => tracing::error!("debugger: cannot listen on port {port}: {e}"),
        }
    }
    if let Some(port) = args.dap_port {
        match dap::enable(port) {
            Ok(p) => {
                eprintln!("debugger: DAP server listening on 127.0.0.1:{p}");
                // Breakpoints are on their way: the game starts when the editor is ready.
                debugger::wait_for_configuration();
            }
            Err(e) => tracing::error!("debugger: cannot listen on port {port}: {e}"),
        }
    }
    // Crash reports go beside the log (the temp folder without one); no dialog without a player.
    crash::set_reports(
        log.as_deref()
            .and_then(Path::parent)
            .map_or_else(std::env::temp_dir, Path::to_path_buf),
        manifest
            .as_ref()
            .map_or_else(|| "The game".to_string(), |m| m.title.clone()),
        log.clone(),
        !args.offline(),
    );
    if args.cook {
        // Cooking needs only the folder, not a build.
        let root = if args.game.is_dir() {
            args.game.clone()
        } else {
            args.game.parent().unwrap_or(Path::new(".")).to_path_buf()
        };
        let report = cook::cook_game(&root);
        tracing::info!(
            "cooked {} textures and {} models into {}",
            report.textures,
            report.models,
            root.join(".platypus/cooked").display()
        );
        for f in &report.failed {
            tracing::warn!("not cooked: {f}");
        }
        return Ok(());
    }

    // The code folder (bundle, `package.json`, icon) and the assets' folder: the same for a game
    // folder, apart for a packaged exe (`--data` also sets it for a folder).
    let (bundle_path, code_root) = resolve_bundle(&args.game)?;
    // The editor's world loads the project's assets (scenes, models, textures by their project
    // paths); its own UI (game.css) still comes from the code folder.
    let asset_root = asset_root::AssetRoot::new(
        args.asset_root
            .clone()
            .or_else(|| args.data.clone())
            .or_else(|| args.project.as_ref().map(|p| editor_asset_root(p)))
            .unwrap_or_else(|| code_root.clone()),
    )
    .path()
    .to_path_buf();
    // Every asset read goes through this: the packs of a packaged game (all mounted now, so a
    // script can load any path), else the folder.
    let vfs = asset_root::mount(&asset_root).map_err(anyhow::Error::msg)?;
    platypus_core::set_asset_fs(Some(std::sync::Arc::new(vfs.clone())));
    // Dev loop: `platypus-build build` lists what a packaged build would hold; loading a file outside
    // it logs a warning (a packaged game would not find it).
    // (Not for the editor: its plan is its own UI's, and its world is the project's.)
    if args.project.is_none()
        && let Ok(plan) = std::fs::read_to_string(code_root.join("build/assets.plan"))
    {
        platypus_core::set_pack_plan(plan.lines().filter(|l| !l.is_empty()).map(str::to_owned));
    }
    lap.mark("log, manifest, asset mount");
    // The game is JIT-compiled below.
    let bundle = native::Bundle::read(&bundle_path)?;
    lap.mark("bundle read");
    let name = bundle_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game.pbundle".into());

    // The game's manifest and the machine, for `Application` and `SystemInfo`, before
    // the bundle's top level runs.
    let manifest = info::Manifest::load(&code_root);
    // The renderer: `--renderer`, then the project's `renderer`, then what the program uses (the
    // bundle's tier).
    let (tier, uses) = (bundle.tier, bundle.uses.clone());
    if bundle.ui_inspector {
        tracing::info!("UI inspector on (built with --add-ui-inspector)");
        ui_inspector::enable();
    }
    let choice = host::resolve(args.renderer, manifest.renderer, tier);
    host::set(choice);
    tracing::info!(
        "renderer: {} (the program needs {}{})",
        choice.name(),
        tier.as_str(),
        if uses.is_empty() {
            String::new()
        } else {
            format!(": {}", uses.join(", "))
        }
    );
    if let Some(w) = host::world_warning(choice, tier, &uses) {
        tracing::warn!("{w}");
    }
    // `Window.main` starts with the manifest's title; the real size and scale come with the window.
    platypus_engine_api::window::set_main(manifest.window_title(), (1280.0, 720.0), 1.0);
    platypus_engine_api::window::set_supported(platypus_platform::Os::current().supports_windows());
    let mut info = info::Info::new(&manifest);
    info.set_app("args", serde_json::json!(args.script_args));
    // The player's saved settings (not for screenshot runs, which stay the same every
    // time) and the window mode: the manifest's default, then the player's choice.
    let offline = args.offline();
    let mut prefs = if offline {
        prefs::Prefs::in_memory()
    } else {
        prefs::Prefs::load(&manifest)
    };
    // A game that keeps a fixed size (`platypus.display.resizable: false`) always gets its own
    // settings: saved ones would tell its scripts about a window size it never has.
    let resizable = manifest.display["resizable"].as_bool().unwrap_or(true);
    let defaults = display::from_json(Some(&manifest.display), Default::default());
    // The startup video-settings prompt, before anything of the game's window exists; its choice
    // goes into the prefs read below.
    if !args.video_dialog
        && args.bench.is_none()
        && args.headless.is_none()
        && video_prompt::should_prompt(
            args.video_prompt,
            manifest.video_settings_prompt,
            video_prompt::chosen(&prefs),
            offline,
        )
    {
        let current = if resizable {
            display::from_json(prefs.get("platypus.display"), defaults)
        } else {
            defaults
        };
        let current = video_prompt::Current::new(&manifest, &prefs, current, resizable);
        // The game's own icon (a packaged game's is in its extracted payload), else Platypus's:
        // the same as its window's.
        let icon = manifest.window_icon(&code_root);
        let shot = args
            .screenshot
            .as_deref()
            .map(|path| video_prompt::Screenshot {
                path,
                frames: args.frames,
            });
        match video_prompt::run(&current, &icon, shot) {
            video_prompt::Outcome::Play(choice) => {
                choice.apply(&mut prefs, current.display, resizable);
                prefs.save();
            }
            // Quit: the game doesn't start. A screenshot run's picture is the dialog's.
            video_prompt::Outcome::Quit | video_prompt::Outcome::Screenshot => return Ok(()),
            video_prompt::Outcome::Failed => {}
        }
    }
    let display_settings = if resizable {
        display::from_json(prefs.get("platypus.display"), defaults)
    } else {
        defaults
    };
    if let Some(path) = prefs.path() {
        tracing::info!("player prefs: {}", path.display());
    }
    info.set("prefs", serde_json::Value::Object(prefs.all().clone()));
    // Assets' GUIDs and importer settings from their `.meta` files.
    info.set("assets", asset_db::scan_vfs(&vfs, &asset_root));
    info.set("assetIndex", asset_db::index(&vfs));
    native_pump::seed_asset_index(&info);
    info.set("display", display::state(None, &display_settings));
    lap.mark("prefs, asset index (.meta files)");
    // The game's CSS: embedded in the bundle (`cssText`), else the loose file `platypus-build`
    // wrote next to it (game.pbundle -> game.css). Its `url()`s resolve from that file's folder.
    let mut ui = UiTree::with_system_fonts(1.0, 1.0);
    lap.mark("UI tree and system fonts");
    let css_path = bundle_path.with_extension("css");
    let game_css_source = game_css_source(bundle.css_text.clone(), &code_root, &css_path);
    if let Some((_, css, base, source)) = &game_css_source {
        add_stylesheet(&mut ui, &asset_root, css, base, source);
        tracing::info!("loaded stylesheet {source}");
    }
    lap.mark("game CSS");
    let mut scene = platypus_scene::ScriptScene::new();
    native_render::configure_engine(&asset_root);
    native_pump::seed_prefs(&prefs);
    let (m, i) = (manifest.clone(), info.clone());
    let mut native = native::Native::load_with(
        bundle,
        &mut scene.scene,
        &mut ui,
        Box::new(move || native_pump::configure_engine(&m, &i)),
    )?;
    native.set_root(&asset_root);
    if let Some(start) = &manifest.start_scene {
        native.start_scene(&mut scene.scene, &mut ui, start);
    }
    let mut lap = web_ui_ts::timing::Lap::start();

    // Rebuilds by `platypus-build dev` reload the game (not for screenshot or bench runs).
    let watcher = if offline {
        None
    } else {
        reload::BundleWatcher::new(vec![
            bundle_path.clone(),
            css_path.clone(),
            // `platypus-build dev` writes this after the bundle (or with the error of a failed build).
            bundle_path.with_file_name(".platypus-reload"),
        ])
        .inspect_err(|e| tracing::warn!("not watching for script changes: {e}"))
        .ok()
    };
    // Asset files that change (textures, models, sounds) load again in place. Packs are
    // read-only: only loose files are watched.
    let asset_watcher = if offline || vfs.root().is_none() {
        None
    } else {
        reload::AssetWatcher::new(&asset_root)
            .inspect_err(|e| tracing::warn!("not watching for asset changes: {e}"))
            .ok()
    };

    let app = App {
        prefs,
        display: display_settings,
        target_frame_rate: -1,
        bundle_path,
        bundle_name: name.clone(),
        watcher,
        asset_watcher,
        sound_files: HashMap::new(),
        native: Some(native),
        scene,
        world_view: world_view::WorldViews::with_physical_units(manifest.physical_units),
        panels: panels::Panels::default(),
        windows: windows::AppWindows::new(),
        domains: windows::Domains::default(),
        reload_claims_pending: false,
        window_commands: Vec::new(),
        windows_to_show: Vec::new(),
        main_focus_sent: None,
        pointer_on: Surface::Screen,
        keys_on: Surface::Screen,
        buttons_down: 0,
        pointer_px: None,
        sheet_sources: game_css_source.into_iter().collect(),
        font_sources: Vec::new(),
        screen: (0.0, 0.0, 0.0),
        render_stats: platypus_render::WorldStats::default(),
        exposure_sent: HashMap::new(),
        ui,
        sheets: HashMap::new(),
        interaction: Interaction::new(),
        physics: platypus_physics::Physics::new(),
        attachments: physics_sync::Attachments::default(),
        cloth: cloth_sync::ClothSystem::default(),
        audio: None,
        nav: game_input::GamepadNav::default(),
        cursor: ("default", Vec::new(), 1.0),
        custom_cursor: None,
        script_cursor: ScriptCursor::default(),
        focused: true,
        cursors_loaded: HashSet::new(),
        cursors_failed: HashSet::new(),
        clipboard: None,
        ime_allowed: false,
        ime_area: None,
        asset_root,
        vfs,
        code_root: code_root.clone(),
        gfx: None,
        gles: None,
        cpu: None,
        notice: None,
        started: Instant::now(),
        clock: lifecycle::FixedClock::default(),
        lifecycle: lifecycle::Lifecycle::new(manifest.run_in_background, offline),
        physical_units: manifest.physical_units,
        blending: manifest.blending,
        pipeline_warned: false,
        profile: profile::Profile::default(),
        frames: 0,
        fps_on: fps::enabled(
            args.fps,
            offline,
            std::env::var("PLATYPUS_FPS").ok().as_deref(),
            manifest.fps,
        ),
        fps: fps::FpsCounter::default(),
        fps_overlay: fps::FpsOverlay::new(),
        screenshot: args.screenshot.map(|path| (path, args.frames)),
        offline,
        fake_now: None,
        script_time: Duration::ZERO,
        render_time: Duration::ZERO,
        sync_time: Duration::ZERO,
        done: false,
        info,
        file_drag: file_drop::FileDrag::default(),
        manifest: manifest.clone(),
        video_dialog: None,
    };
    // The game's own icon, else Platypus's.
    let icon = Some(manifest.window_icon(&code_root));
    // The game's own app id (taskbar group, pinned icon) before its window exists; not for
    // screenshot / bench runs or the editor.
    if !offline && args.project.is_none() && !args.video_dialog {
        identity::apply(&manifest, icon.as_ref(), args.packaged);
    }
    // A window opens at the saved window size (a fixed one at its own); fullscreen modes are
    // applied once it exists.
    let (width, height) = (
        display_settings.window_width,
        display_settings.window_height,
    );
    lap.mark("app set-up");
    // A bench run makes no OS window at all: it draws offscreen, its own loop drives the frames.
    if let Some(bench) = args.bench {
        let size = bench.size.unwrap_or((width, height));
        return bench::run(app, &bench, size, manifest.window_title());
    }
    // A headless run makes no window and no graphics device; `--frames N` stops it after N iterations.
    if let Some(headless) = &args.headless {
        let frames = (args.frames_given).then_some(args.frames);
        return headless::run(app, headless, frames, manifest.window_title());
    }
    // Screenshot runs make no OS windows for secondary windows: they draw offscreen.
    let virtual_windows = app.screenshot.is_some();
    Platform::new().run(
        WindowConfig {
            title: manifest.window_title().to_owned(),
            icon,
            width,
            height,
            virtual_windows,
            resizable,
            center: !virtual_windows,
        },
        app,
    )?;
    Ok(())
}

/// The bundle to load and the folder its assets (`url(...)` images) live in.
fn resolve_bundle(path: &Path) -> Result<(PathBuf, PathBuf)> {
    if path.is_dir() {
        // A game built for the native backend has a `.pbundle`.
        let native = path.join("build").join("game.pbundle");
        if native.is_file() {
            return Ok((native, path.to_path_buf()));
        }
        // Games built before the rename to Platypus still load.
        let legacy = path.join("build").join(native::LEGACY_BUNDLE);
        if legacy.is_file() {
            tracing::warn!(
                "{} is deprecated: rebuild with `platypus-build build` (writes game.pbundle)",
                legacy.display()
            );
            return Ok((legacy, path.to_path_buf()));
        }
        let js = path.join("build").join("game.js");
        if js.is_file() {
            bail!("{} {QUICKJS_REMOVED}", js.display());
        }
        // Games used to build into `dist/`; that folder is not read any more.
        let old = path.join("dist");
        if ["game.pbundle", native::LEGACY_BUNDLE, "game.js"]
            .iter()
            .any(|name| old.join(name).is_file())
        {
            bail!(
                "{} has only an old `dist/` build; build output now goes to `build/`: rebuild with `platypus-build build {}`",
                path.display(),
                path.display()
            );
        }
        bail!(
            "{} not found; build it first with `platypus-build build {}`",
            native.display(),
            path.display()
        );
    }
    if path.is_file() {
        if !native::is_native(path) {
            bail!("{} {QUICKJS_REMOVED}", path.display());
        }
        let root = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        return Ok((path.to_path_buf(), root));
    }
    bail!("{} is not a game folder or bundle", path.display())
}

struct Gfx {
    gpu: Gpu,
    surface: frame_target::FrameTarget,
    /// The OS window; `None` for a windowless `--bench` run.
    window: Option<Arc<Window>>,
    renderer: UiRenderer,
    world: lazy_world::LazyWorld,
}

/// Hands the world renderer what scripts changed (meshes, textures, quality, sky, ...).
fn apply_world_view(
    world: &mut WorldRenderer,
    gpu: &Gpu,
    view: &mut world_view::WorldViews,
    pipeline_warned: &mut bool,
) {
    for (id, data) in view.mesh_uploads.drain(..) {
        match data {
            Some(data) => {
                world.set_mesh(gpu, id, &data);
            }
            None => world.remove_mesh(id),
        }
    }
    world.bloom = view.bloom;
    world.grading = view.grading;
    for id in view.exposure_resets.drain(..) {
        world.reset_exposure(id);
    }
    warn_pipeline(pipeline_warned, view);
    (
        world.shadow_distance,
        world.shadow_resolution,
        world.shadow_cascades,
    ) = view.shadow_settings;
    world.set_sky(view.sky);
    let q = view.quality;
    world.msaa = q.msaa;
    world.render_scale = q.render_scale;
    world.shadow_quality = q.shadows;
    world.soft_shadow_radius = q.soft_radius;
    world.anisotropic = q.anisotropic;
    world.texture_mip_limit = q.texture_mip_limit;
    world.pipeline = q.pipeline;
    (world.environment_intensity, world.ambient_from_sky) = view.environment;
    for (id, data) in view.texture_uploads.drain(..) {
        match data {
            Some(data) => {
                world.set_texture(gpu, id, &data);
            }
            None => world.remove_texture(id),
        }
    }
    for (id, size) in view.render_texture_uploads.drain(..) {
        world.set_render_texture(gpu, id, size);
    }
    for (id, cooked) in view.cooked_uploads.drain(..) {
        // GPUs without BC get the blocks decoded.
        if !world.set_cooked_texture(gpu, id, &cooked) {
            world.set_texture(gpu, id, &cook::uncook_texture(&cooked));
        }
    }
}

/// Draws one frame into `target`: the 3D world when a camera is active, with the
/// screen UI over it; otherwise the UI over the plain background.
fn draw_frame(
    gpu: &Gpu,
    renderer: &mut UiRenderer,
    world: &mut lazy_world::LazyWorld,
    target: &RenderTarget<'_>,
    views: &[platypus_render::WorldView],
    textures: &[&wgpu::TextureView],
    cmds: &[web_ui_dom::DrawCmd],
) {
    if views.is_empty() {
        renderer.render(gpu, target, CLEAR, cmds);
        return;
    }
    let world = world.get(gpu);
    if ui_frame::over_a_copy(renderer.blending(), target.format, cmds) {
        // Backdrop blurs and blend modes read the world, and sRGB blending can't be done
        // by blending onto an sRGB target: draw the world where the UI can sample it, then
        // the UI over a copy of it into the target.
        let backdrop = renderer.backdrop_target(gpu, target.size, target.format);
        let world_target = RenderTarget {
            view: &backdrop,
            format: target.format,
            size: target.size,
            scale: target.scale,
        };
        world.render_views(gpu, &world_target, views, textures);
        renderer.render_over(gpu, target, &backdrop, cmds);
    } else {
        world.render_views(gpu, target, views, textures);
        renderer.render_overlay(gpu, target, cmds);
    }
}

struct App {
    /// The player's saved settings (`PlayerPrefs`), written on `prefs.save` and on exit.
    prefs: prefs::Prefs,
    /// The window's mode and size, as last applied.
    display: platypus_platform::DisplaySettings,
    /// `Application.targetFrameRate` (-1: no cap); ignored while vsync is on.
    target_frame_rate: i32,
    /// GameObjects scripts made (and the engine made for them).
    scene: platypus_scene::ScriptScene,
    /// The screen size in UI pixels and the UI scale, as last told to scripts.
    screen: (f32, f32, f32),
    /// The world renderer's last stats, as last told to scripts (`RenderStats`).
    render_stats: platypus_render::WorldStats,
    /// Each camera's exposure as last sent to scripts (`Camera.exposure`).
    exposure_sent: HashMap<u32, (f32, f32)>,
    /// React UI on planes in the world (`WorldPanel`s), each with its own tree.
    panels: panels::Panels,
    /// Secondary windows (`Window.create`), each with its own tree, and which windows have focus.
    windows: windows::AppWindows,
    /// Isolated event domains (families of windows under a root other than the main window).
    domains: windows::Domains,
    /// A reload happened: after the new code's first frame, keyed windows nobody claimed close.
    reload_claims_pending: bool,
    /// Platform commands scripts' window requests became, and the windows drawn for the first time.
    window_commands: Vec<platypus_platform::WindowCommand>,
    windows_to_show: Vec<platypus_platform::WindowKey>,
    /// The game's focus as last given to the lifecycle (any of its windows focused).
    main_focus_sent: Option<bool>,
    /// The surface under the pointer (or holding it while a button is down), and the one
    /// keys go to (the last one clicked).
    pointer_on: Surface,
    keys_on: Surface,
    buttons_down: u32,
    /// The pointer in window pixels, while it's over the window.
    pointer_px: Option<(f32, f32)>,
    /// Stylesheets every UI tree gets: (key, css, base, source name). Keys are script
    /// sheet ids, or `GAME_CSS`.
    sheet_sources: Vec<(u32, String, String, String)>,
    /// Fonts scripts loaded (family, src, weight, italic), replayed into new panel trees.
    font_sources: Vec<(String, String, Option<f32>, Option<bool>)>,
    /// Cameras and planes scripts put in the world.
    world_view: world_view::WorldViews,
    bundle_path: PathBuf,
    bundle_name: String,
    watcher: Option<reload::BundleWatcher>,
    asset_watcher: Option<reload::AssetWatcher>,
    /// Sounds loaded from files (by asset key): their `audio.load` arguments, to load
    /// again when the file changes.
    sound_files: HashMap<String, Vec<Vec<platypus_core::Value>>>,
    /// The compiled game (always there once running).
    native: Option<native::Native>,
    ui: UiTree,
    /// Where textures, models, sounds, fonts and scenes are read from.
    asset_root: PathBuf,
    /// The assets' file system (also installed for the loaders through `platypus_core`).
    vfs: platypus_pack::Vfs,
    /// Where the game's code, `package.json` and CSS are (the asset root, except in a packaged exe).
    code_root: PathBuf,
    gfx: Option<Gfx>,
    /// The main window drawn by the GLES executor (the light path, `host::Choice::Gles`),
    /// instead of `gfx`.
    gles: Option<gles_gfx::GlesGfx>,
    /// The main window drawn on the CPU (`host::Choice::Cpu`), instead of `gfx`.
    cpu: Option<cpu_gfx::CpuGfx>,
    /// "3D Not Supported", shown by a light path once the game asks for the world.
    notice: Option<notice::Notice>,
    started: Instant,
    clock: lifecycle::FixedClock,
    /// Focus, and pausing in the background (`Application.runInBackground`).
    lifecycle: lifecycle::Lifecycle,
    frames: u32,
    screenshot: Option<(PathBuf, u32)>,
    /// A run without a player (`--screenshot`, `--bench`): no saved prefs, file watching, frame
    /// cap or pausing, and a silent mixer.
    offline: bool,
    /// The clock of a `--bench --fixed-dt` run (each frame exactly one step); `None`: real time.
    fake_now: Option<Instant>,
    /// Time spent in game code this frame, and drawing it, for `--bench`.
    script_time: Duration,
    render_time: Duration,
    /// A compiled game's per-frame sync of its registries into the renderer
    /// (`sync_native_render`: animation, splines, cameras, `WorldViews` ops).
    sync_time: Duration,
    /// The FPS counter overlay (drawn last, at window resolution).
    fps_on: bool,
    fps: fps::FpsCounter,
    fps_overlay: fps::FpsOverlay,
    /// Stylesheets added by scripts: script id -> tree id.
    sheets: HashMap<u32, u32>,
    interaction: Interaction,
    /// The physics world scripts drive with `physics.*` ops.
    physics: platypus_physics::Physics,
    /// Bodies attached to GameObjects (`physics.attach`).
    attachments: physics_sync::Attachments,
    /// `Cloth` components' simulations.
    cloth: cloth_sync::ClothSystem,
    /// The sound mixer, opened by the first `audio.*` op.
    audio: Option<Box<dyn platypus_audio::Mixer>>,
    nav: game_input::GamepadNav,
    /// The CSS cursor currently shown: keyword, images, and the scale they were for.
    cursor: (&'static str, Vec<web_ui_dom::CursorImage>, f64),
    /// A custom cursor for the platform to show.
    custom_cursor: Option<CustomCursorRequest>,
    /// The game's manifest and the machine, as scripts see them (`__platypus_info`).
    info: info::Info,
    /// Real-world light units (`platypus.lightUnits`), and whether the quality level has warned
    /// about leaving out light shafts the game asked for yet.
    physical_units: bool,
    /// How translucent UI colours combine (`blending` in `project.json`), on every renderer.
    blending: platypus_core::project::Blending,
    pipeline_warned: bool,
    /// `PLATYPUS_PROFILE` timings.
    profile: profile::Profile,
    /// Scripts' `Cursor`: shown or not, how it's held, and their default image.
    script_cursor: ScriptCursor,
    /// Whether the window has focus (raw mouse motion only counts then).
    focused: bool,
    /// Cursor images (by `url@scale`) already sent to the platform, or unusable.
    cursors_loaded: HashSet<String>,
    cursors_failed: HashSet<String>,
    clipboard: Option<arboard::Clipboard>,
    /// Whether the window currently accepts IME input, and the last candidate area.
    ime_allowed: bool,
    ime_area: Option<web_ui_dom::Rect>,
    /// Files dragged onto the window from the system.
    file_drag: file_drop::FileDrag,
    /// The game's manifest, for the video-settings dialog `Application.showVideoSettings` opens.
    manifest: info::Manifest,
    /// That dialog's process while it is open (the game is paused).
    video_dialog: Option<video_prompt::Running>,
    done: bool,
}

impl App {
    /// Counts the frame and, in a `--screenshot` run, saves it when it's the wanted one.
    /// One frame was presented: the counter counts it whether or not it is drawn, and
    /// scripts read its number as `Application.fps`.
    fn tick_fps(&mut self) {
        if self
            .fps
            .frame(self.started.elapsed().as_secs_f64())
            .is_some()
        {
            platypus_engine_api::app::set_fps(self.fps.value());
        }
    }

    fn count_frame(&mut self) {
        self.frames += 1;
        // `PLATYPUS_RELOAD_AT_FRAME=N`: a screenshot run reloads its scripts after frame N,
        // to check hot reload without a window.
        if self.screenshot.is_some()
            && std::env::var("PLATYPUS_RELOAD_AT_FRAME")
                .ok()
                .and_then(|n| n.parse::<u32>().ok())
                == Some(self.frames)
        {
            self.reload_scripts();
        }
        if let Some((path, frames)) = self.screenshot.clone()
            && self.frames >= frames
        {
            match self.save_screenshot(&path) {
                Ok(()) => tracing::info!("saved screenshot to {}", path.display()),
                Err(e) => tracing::error!("screenshot failed: {e}"),
            }
            self.done = true;
        }
    }

    /// Sizes the UI to the window (in UI pixels: the window size divided by its DPI and UI
    /// scale) and tells scripts' `Screen` when that changes. Nothing before the window
    /// exists.
    fn sync_screen(&mut self) {
        let Some((dpi, size)) = self.main_metrics() else {
            return;
        };
        let scale = ui_factor_for(dpi, size);
        let (width, height) = size;
        self.ui
            .set_size(width as f32 / scale, height as f32 / scale);
        // Scripts' `Screen`: the size in UI pixels (what pointer events use) and the scale.
        let screen = (width as f32 / scale, height as f32 / scale, scale);
        self.screen = screen;
    }

    /// Hot reload (DESIGN.md §6.1): the rebuilt bundle is compiled again and swapped in, or the
    /// old code keeps running if it fails.
    fn reload_scripts(&mut self) {
        self.reload_native(Instant::now());
    }

    /// Hot reload of a compiled game: the bundle is compiled again; a failure keeps the old one.
    fn reload_native(&mut self, started: Instant) {
        // A failed build leaves the old bundle: say why and keep running it.
        if let Some(error) = native::failed_build(&self.bundle_path) {
            tracing::error!("build failed, keeping the old code: {error}");
            return;
        }
        let bundle = match native::Bundle::read(&self.bundle_path) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("reload: {e:#}");
                return;
            }
        };
        let Some(n) = self.native.as_mut() else {
            return;
        };
        // The new code starts unscaled (its top level may set `UiScale` again); a failed reload
        // keeps the old code's.
        let ui_scale = platypus_engine_api::ui_scale::current();
        platypus_engine_api::ui_scale::reset();
        let result = n.reload(bundle, &mut self.scene.scene, &mut self.ui);
        if result.is_err() {
            platypus_engine_api::ui_scale::set_current(ui_scale);
        }
        // The scene was replaced: the renderer's script ids pointed into the old one.
        self.scene.clear_ids();
        self.reset_native_render();
        self.close_reload_windows();
        self.native_after();
        match result {
            Ok(()) => tracing::info!(
                "reloaded {} in {:.1} ms",
                self.bundle_name,
                started.elapsed().as_secs_f64() * 1000.0
            ),
            Err(e) => tracing::error!("reload failed, keeping the old code: {e:#}"),
        }
    }

    /// Starts or ends a pause in the background and tells scripts about focus and
    /// pausing (`__platypus_focus(focused, paused)`).
    fn sync_lifecycle(&mut self) {
        let change = self.lifecycle.update(Instant::now());
        if change.paused
            && let Some(audio) = self.audio.as_mut()
        {
            audio.set_muted(true);
        }
        if let Some(gap) = change.resumed {
            // Script time (timers, caret, UI time) skips the pause, and fixed ticks
            // start over instead of catching up.
            self.started += gap;
            self.clock.restart();
            if let Some(audio) = self.audio.as_mut() {
                audio.set_muted(false);
            }
        }
        if let Some((focused, paused)) = change.report {
            // `Window.onDomainFocusChanged` of the main game.
            {
                let _scope = platypus_scene::api::enter(&mut self.scene.scene);
                platypus_engine_api::window::domain_focus_changed(
                    platypus_engine_api::window::DomainId::MAIN,
                    focused,
                    paused,
                );
            }
            if let Some(n) = self.native.as_mut() {
                n.focus_changed(&mut self.scene.scene, &mut self.ui, focused, paused);
                self.native_after();
            }
        }
    }

    fn tick(&mut self) {
        let now = self.now();
        if self.watcher.as_mut().is_some_and(|w| w.poll(now)) {
            self.reload_scripts();
        }
        let changed = self
            .asset_watcher
            .as_mut()
            .map(|w| w.poll(now))
            .unwrap_or_default();
        if !changed.is_empty() {
            self.reload_assets(&changed);
        }
        // Paused in the background: the game's time stands still.
        if self.lifecycle.paused() {
            return;
        }
        // The network: script requests (`Network.startServer` / `connect`), then packets in and out.
        net_host::pump(now.duration_since(self.started));
        let (elapsed, ticks) = self.clock.advance(now, TICK, MAX_TICKS_PER_FRAME);
        for _ in 0..ticks {
            // Script time leaves out applying what scripts asked for (`native_after`): that is
            // engine work.
            if let Some(n) = self.native.as_mut() {
                let t = Instant::now();
                n.fixed_tick(&mut self.scene.scene, &mut self.ui, TICK);
                self.script_time += t.elapsed();
                self.native_after();
            }
            self.step_physics();
            if !self.cloth.is_empty() {
                let gravity = platypus_core::glam::Vec3::from(self.physics.gravity());
                let scene = &self.scene;
                self.cloth.retain(|o| scene.object(o).is_some());
                self.cloth
                    .step(TICK as f32, gravity, &self.scene, &mut self.world_view);
            }
            // The engine's moves are read from the scene by compiled code itself.
            let _ = self.scene.take_batch();
        }

        // Per-frame component updates (`onUpdate`, `onLateUpdate`), timers and their microtasks.
        let now_ms = now.duration_since(self.started).as_secs_f64() * 1000.0;
        if let Some(n) = self.native.as_mut() {
            let t = Instant::now();
            n.frame(
                &mut self.scene.scene,
                &mut self.ui,
                elapsed.min(MAX_FRAME_DT),
                now_ms,
            );
            self.script_time += t.elapsed();
            self.native_after();
        }
        let dt = elapsed.min(MAX_FRAME_DT) as f32;
        // Auto exposure adapts by the frame's time.
        self.world_view.grading.delta_time = dt;
        self.world_view.advance_clouds(dt);
        let changed = self.world_view.update_particles(dt, &self.scene);
        native_render::report_particles(&changed, &mut self.scene.scene);
        self.update_audio(elapsed);

        // The trees may have moved under a still pointer, or the focused element gone.
        self.handle_ui(Input::Refresh);
        let ids: Vec<u32> = self.panels.map.keys().copied().collect();
        for id in ids {
            self.handle_on(Surface::Panel(id), Input::Refresh);
        }
    }

    /// One physics tick of the bodies attached to GameObjects.
    fn step_physics(&mut self) {
        let _ = self.attachments.step(&mut self.physics, &mut self.scene);
    }

    /// The screen and every world panel.
    fn surfaces(&self) -> Vec<Surface> {
        std::iter::once(Surface::Screen)
            .chain(self.panels.map.keys().map(|id| Surface::Panel(*id)))
            .collect()
    }

    /// The UI tree holding a node (ops like `focus` name nodes, not trees).
    fn surface_of(&self, node: platypus_core::NodeId) -> Surface {
        if self.ui.contains(node) {
            return Surface::Screen;
        }
        if let Some((key, _)) = self.windows.map.iter().find(|(_, w)| w.tree.contains(node)) {
            return Surface::Window(key.0);
        }
        self.panels
            .map
            .iter()
            .find(|(_, p)| p.tree.contains(node))
            .map_or(Surface::Screen, |(id, _)| Surface::Panel(*id))
    }

    /// The ray from the camera under a window pixel.
    fn camera_ray(
        &self,
        px: f32,
        py: f32,
    ) -> Option<(platypus_core::glam::Vec3, platypus_core::glam::Vec3)> {
        let size = self.gfx.as_ref()?.surface.size();
        // Cameras drawing into textures aren't on the screen.
        let mut views = self.world_view.views(&self.scene, size, &[]);
        views.retain(|v| v.target_texture.is_none());
        panels::camera_ray(&views, size, px, py)
    }

    /// What the pointer at a window pixel is over: screen UI wherever the screen has an
    /// element (layout boxes covering the screen should use `pointerEvents: none`),
    /// otherwise the nearest world panel the camera ray hits, otherwise the screen.
    fn surface_at(&mut self, px: f32, py: f32) -> (Surface, f32, f32) {
        let scale = self.scale();
        let (ux, uy) = (px / scale, py / scale);
        let on_screen = self
            .ui
            .hit_test(ux, uy)
            .is_some_and(|n| n != web_ui_dom::ROOT);
        if !on_screen
            && !self.panels.map.is_empty()
            && let Some((origin, dir)) = self.camera_ray(px, py)
            && let Some(hit) = self.panels.hit(&self.scene, origin, dir)
        {
            return (Surface::Panel(hit.id), hit.x, hit.y);
        }
        (Surface::Screen, ux, uy)
    }

    /// Sends the pointer's position to the surface it's over (or the one holding it while
    /// a button is down), leaving the previous one.
    fn route_pointer(&mut self) {
        let Some((px, py)) = self.pointer_px else {
            return;
        };
        let (surface, x, y) = if self.buttons_down > 0 {
            // A drag stays with where it started, even off the panel's edge.
            match self.pointer_on {
                Surface::Panel(id) => {
                    let point = self
                        .camera_ray(px, py)
                        .and_then(|(o, d)| self.panels.project(&self.scene, id, o, d));
                    let (x, y) = point.unwrap_or((-1e6, -1e6));
                    (Surface::Panel(id), x, y)
                }
                // (`pointer_on` is never a window: those route their own pointer.)
                Surface::Screen | Surface::Window(_) => {
                    let scale = self.scale();
                    (Surface::Screen, px / scale, py / scale)
                }
            }
        } else {
            self.surface_at(px, py)
        };
        if surface != self.pointer_on {
            self.handle_on(self.pointer_on, Input::PointerLeave);
            self.pointer_on = surface;
        }
        self.handle_on(surface, Input::PointerMove { x, y });
    }

    /// Lets audio progress and tells scripts which sounds finished (`__platypus_audio`).
    fn update_audio(&mut self, elapsed: f64) {
        self.native_audio_commands();
        let Some(mixer) = self.audio.as_mut() else {
            return;
        };
        mixer.advance(elapsed);
        let (level, running, error) = mixer.mic_status();
        platypus_engine_api::audio::set_mic_status(f64::from(level), running, &error);
        let ended = mixer.poll();
        if !ended.is_empty() {
            self.native_audio_ended(&ended);
        }
    }

    /// Runs one `audio.*` op, opening the mixer on first use (games without sound never
    /// open a device). Screenshot runs mix offline, silently.
    fn audio_op(&mut self, name: &str, args: &[platypus_core::Value]) {
        let offline = self.offline;
        let paused = self.lifecycle.paused();
        let mixer = self
            .audio
            .get_or_insert_with(|| platypus_audio::open_mixer(offline));
        // A mixer opened during a pause starts silent.
        mixer.set_muted(paused);
        if name == "load"
            && let Some(src) = args.get(1).and_then(platypus_core::Value::as_str)
            && !src.trim_start().starts_with("data:")
        {
            let list = self.sound_files.entry(reload::asset_key(src)).or_default();
            let id = args.first();
            list.retain(|a| a.first() != id);
            list.push(args.to_vec());
        }
        let result = if name == "load" {
            load_sound(mixer.as_mut(), &self.asset_root, args)
        } else {
            mixer.apply(name, args)
        };
        if let Err(e) = result {
            tracing::warn!("audio.{name}: {e}");
        }
    }

    /// Asset files changed on disk: textures, models and sounds from them load again
    /// under their ids, then scripts hear which files (`__platypus_assets_changed`).
    fn reload_assets(&mut self, keys: &[String]) {
        // Files came or went: the index `Assets.exists` / `list` answer from (packs never change).
        if self.vfs.root().is_some() {
            self.info.set("assetIndex", asset_db::index(&self.vfs));
            native_pump::seed_asset_index(&self.info);
        }
        // New or changed `.meta` files: the database scripts read.
        if keys.iter().any(|k| k.ends_with(".meta")) {
            self.info
                .set("assets", asset_db::scan_vfs(&self.vfs, &self.asset_root));
        }
        let mut sizes = Vec::new();
        if keys.iter().any(|k| k.ends_with(".meta"))
            && let Some(list) = self.info.0.get("assets").and_then(|a| a.as_array())
        {
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
        for key in keys {
            let (s, models) = self.world_view.reload_asset(&self.asset_root, key);
            if !s.is_empty() || !models.is_empty() {
                tracing::info!("reloaded {key}");
            }
            sizes.extend(s);
            for args in self.sound_files.get(key).cloned().unwrap_or_default() {
                if let Some(mixer) = self.audio.as_mut()
                    && let Err(e) = load_sound(mixer.as_mut(), &self.asset_root, &args)
                {
                    tracing::warn!("reloading sound `{key}`: {e}");
                }
            }
        }
        self.native_assets_changed(keys, &sizes);
    }

    /// Runs one UI input, sending its events to scripts, then updates the OS cursor.
    fn handle_ui(&mut self, input: Input) {
        self.handle_on(Surface::Screen, input);
    }

    /// Runs one UI input on the screen's UI or a world panel's, sending its events to
    /// scripts, then updates the IME and the OS cursor.
    fn handle_on(&mut self, surface: Surface, input: Input) {
        // A compiled game runs its handlers inside the interaction layer's dispatch, so
        // `preventDefault()` reaches the default actions.
        let (native, scene) = (&mut self.native, &mut self.scene.scene);
        let clipboard = match surface {
            Surface::Screen => {
                self.interaction.handle(&mut self.ui, input, &mut |event| {
                    native.as_mut().is_some_and(|n| n.ui_event(scene, event))
                });
                self.interaction.take_clipboard()
            }
            Surface::Panel(id) => {
                let Some(p) = self.panels.map.get_mut(&id) else {
                    return;
                };
                p.interaction.handle(&mut p.tree, input, &mut |event| {
                    native.is_some() && native_render::panel_event(scene, id, event)
                });
                p.interaction.take_clipboard()
            }
            Surface::Window(key) => {
                let Some(w) = self.windows.map.get_mut(&platypus_platform::WindowKey(key)) else {
                    return;
                };
                let _current = platypus_engine_api::window::enter_current(
                    platypus_engine_api::window::WindowRef(key),
                );
                w.interaction.handle(&mut w.tree, input, &mut |event| {
                    native.is_some() && native_render::window_event(scene, key, event)
                });
                w.interaction.take_clipboard()
            }
        };
        match clipboard {
            Some(Clipboard::Copy(text)) => {
                if let Some(Err(e)) = self.clipboard().map(|c| c.set_text(text)) {
                    tracing::warn!("copy failed: {e}");
                }
            }
            Some(Clipboard::Paste) => {
                if let Some(text) = self.clipboard().and_then(|c| c.get_text().ok()) {
                    self.handle_on(surface, Input::Paste(text));
                }
            }
            None => {}
        }
        if let Some(n) = self.native.as_mut() {
            if let Surface::Panel(id) = surface
                && let Some(p) = self.panels.map.get_mut(&id)
            {
                native_render::panel_settle(&mut self.scene.scene, id, &mut p.tree);
            }
            if let Surface::Window(key) = surface
                && let Some(w) = self.windows.map.get_mut(&platypus_platform::WindowKey(key))
            {
                let _current = platypus_engine_api::window::enter_current(
                    platypus_engine_api::window::WindowRef(key),
                );
                native_render::window_settle(&mut self.scene.scene, key, &mut w.tree);
                n.settle(&mut self.scene.scene, &mut self.ui);
                self.native_after();
            } else {
                n.settle(&mut self.scene.scene, &mut self.ui);
                self.native_after();
            }
        }
        if let Surface::Window(key) = surface {
            self.update_window_cursor_and_ime(key);
            return;
        }
        self.update_ime();
        self.update_cursor();
    }

    /// A window's own cursor and IME state (the main window's are `update_cursor` / `update_ime`).
    fn update_window_cursor_and_ime(&mut self, key: u32) {
        let Some(w) = self.windows.map.get_mut(&platypus_platform::WindowKey(key)) else {
            return;
        };
        let platypus_platform::WindowHost::Os(window) = &w.host else {
            return;
        };
        let (keyword, _) = w.interaction.cursor_with_images(&w.tree);
        if keyword != w.cursor {
            w.cursor = keyword;
            window.set_cursor_visible(keyword != "none");
            if let Ok(icon) = keyword.parse::<CursorIcon>() {
                window.set_cursor(icon);
            }
        }
        let area = w.interaction.ime_area(&w.tree);
        if area.is_some() != w.ime_allowed {
            w.ime_allowed = area.is_some();
            window.set_ime_allowed(w.ime_allowed);
        }
        if let Some(a) = area {
            let z = 1.0 / window.scale_factor() as f32 * w.scale as f32;
            window.set_ime_cursor_area(
                platypus_platform::dpi::LogicalPosition::new(a.x * z, a.y * z),
                platypus_platform::dpi::LogicalSize::new(
                    (a.width * z).max(1.0),
                    (a.height * z).max(1.0),
                ),
            );
        }
    }

    /// After a call into a compiled game: its node commands, window, `Application`,
    /// `PlayerPrefs` and quality requests.
    fn native_after(&mut self) {
        self.apply_node_commands();
        self.native_requests();
        self.pump_windows();
    }

    /// Applies what compiled code asked of nodes it holds refs to (`focus()`, `blur()`,
    /// `scrollTo()`).
    fn apply_node_commands(&mut self) {
        use web_ui_bridge::NodeCommand;
        for c in web_ui_bridge::take_node_commands() {
            if let NodeCommand::DragImage(id, x, y) = c {
                // id 0: no drag image (the screen's interaction holds the drag).
                let node = (id > 0).then_some(platypus_core::NodeId(id));
                let surface = node.map_or(Surface::Screen, |n| self.surface_of(n));
                let (x, y) = (x as f32, y as f32);
                self.handle_on(surface, Input::SetDragImage { id: node, x, y });
                continue;
            }
            let (id, input) = match c {
                NodeCommand::Focus(id) => (id, Input::Focus(platypus_core::NodeId(id))),
                NodeCommand::Blur(id) => (id, Input::Blur(platypus_core::NodeId(id))),
                NodeCommand::ScrollTo(id, x, y, behavior) => (
                    id,
                    Input::ScrollTo {
                        id: platypus_core::NodeId(id),
                        x: x as f32,
                        y: y as f32,
                        behavior,
                    },
                ),
                NodeCommand::ScrollBy(id, dx, dy, behavior) => (
                    id,
                    Input::ScrollBy {
                        id: platypus_core::NodeId(id),
                        dx: dx as f32,
                        dy: dy as f32,
                        behavior,
                    },
                ),
                NodeCommand::ScrollIntoView {
                    id,
                    block,
                    inline,
                    behavior,
                } => (
                    id,
                    Input::ScrollIntoView {
                        id: platypus_core::NodeId(id),
                        block,
                        inline,
                        behavior,
                    },
                ),
                NodeCommand::SetPointerCapture(id) => {
                    (id, Input::SetPointerCapture(platypus_core::NodeId(id)))
                }
                NodeCommand::ReleasePointerCapture(id) => {
                    (id, Input::ReleasePointerCapture(platypus_core::NodeId(id)))
                }
                NodeCommand::DragImage(..) => unreachable!("handled above"),
            };
            let surface = self.surface_of(platypus_core::NodeId(id));
            if matches!(c, NodeCommand::Focus(_)) && !matches!(surface, Surface::Window(_)) {
                self.keys_on = surface;
            }
            self.handle_on(surface, input);
        }
    }

    /// `screen.set mode width height refreshRate`: changes the window's mode and size,
    /// remembers it for next time and tells scripts what took effect. A size of 0 is the
    /// mode's default: the monitor's resolution (exclusive) or the last window size.
    fn screen_op(&mut self, args: &[platypus_core::Value]) {
        let num = |i: usize| args.get(i).and_then(platypus_core::Value::as_f64);
        let wanted = platypus_platform::DisplaySettings {
            mode: args
                .first()
                .and_then(platypus_core::Value::as_str)
                .and_then(platypus_platform::FullScreenMode::parse)
                .unwrap_or(self.display.mode),
            width: num(1).map_or(self.display.width, |w| w.max(0.0) as u32),
            height: num(2).map_or(self.display.height, |h| h.max(0.0) as u32),
            refresh_rate: num(3).map_or(self.display.refresh_rate, |r| r.max(0.0) as f32),
            ..self.display
        };
        self.apply_display(wanted);
        self.prefs
            .set("platypus.display", display::to_json(&self.display));
    }

    /// Puts the window in a mode (screenshot runs leave it alone) and updates
    /// `__platypus_info.display`.
    fn apply_display(&mut self, wanted: platypus_platform::DisplaySettings) {
        let window = self.main_window();
        self.display = match (&window, self.offline) {
            (Some(w), false) => platypus_platform::display::apply(w, &wanted),
            _ => wanted,
        };
        self.info
            .set("display", display::state(window.as_deref(), &self.display));
    }

    fn cursor_op(&mut self, name: &str, args: &[platypus_core::Value]) {
        match (name, args.first()) {
            ("visible", Some(platypus_core::Value::Bool(v))) => self.script_cursor.visible = *v,
            ("lock", Some(platypus_core::Value::String(mode))) => {
                self.script_cursor.lock = match mode.as_str() {
                    "locked" => CursorLock::Locked,
                    "confined" => CursorLock::Confined,
                    _ => CursorLock::None,
                };
                self.apply_cursor_lock();
            }
            ("image", Some(platypus_core::Value::String(src))) => {
                let hotspot = match args.get(1) {
                    Some(platypus_core::Value::Array(h)) => {
                        let n = |i: usize| {
                            h.get(i)
                                .and_then(platypus_core::Value::as_f64)
                                .unwrap_or(0.0)
                        };
                        Some((n(0) as f32, n(1) as f32))
                    }
                    _ => None,
                };
                self.script_cursor.image = Some(web_ui_dom::CursorImage {
                    url: src.clone(),
                    hotspot,
                });
            }
            ("image", _) => self.script_cursor.image = None,
            _ => {
                tracing::warn!("cursor.{name}: bad arguments {args:?}");
                return;
            }
        }
        // Show the change even if what's hovered didn't change.
        self.cursor.0 = "";
        self.update_cursor();
    }

    /// Holds the cursor as scripts asked. Locking isn't available everywhere (Windows
    /// can only confine): it falls back to confining, with the cursor hidden.
    fn apply_cursor_lock(&mut self) {
        let Some(window) = self.main_window() else {
            return;
        };
        use platypus_platform::CursorGrabMode;
        let result = match self.script_cursor.lock {
            CursorLock::None => window.set_cursor_grab(CursorGrabMode::None),
            CursorLock::Confined => window.set_cursor_grab(CursorGrabMode::Confined),
            CursorLock::Locked => window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined)),
        };
        if let Err(e) = result {
            tracing::debug!("cursor lock: {e}");
        }
    }

    /// Shows the hovered element's cursor: its first image that loads, else its keyword.
    /// Where the UI asks for nothing special, scripts' `Cursor` image is the default.
    fn update_cursor(&mut self) {
        // No window, no cursor.
        let Some(window) = self.main_window() else {
            return;
        };
        let (keyword, ui_images) = match self.pointer_on {
            Surface::Panel(id) if self.panels.map.contains_key(&id) => {
                let p = &self.panels.map[&id];
                p.interaction.cursor_with_images(&p.tree)
            }
            _ => self.interaction.cursor_with_images(&self.ui),
        };
        let script_image;
        let images = match &self.script_cursor.image {
            Some(image) if ui_images.is_empty() && keyword == "default" => {
                script_image = [image.clone()];
                &script_image[..]
            }
            _ => ui_images,
        };
        let scale = window.scale_factor();
        if (keyword, images, scale) == (self.cursor.0, &self.cursor.1[..], self.cursor.2) {
            return;
        }
        self.cursor = (keyword, images.to_vec(), scale);
        // Hidden by scripts, or locked (which hides it, as in Unity).
        let shown = self.script_cursor.visible && self.script_cursor.lock != CursorLock::Locked;
        let custom = self.cursor.1.iter().find_map(|image| {
            let key = format!("{}@{scale}", image.url);
            if self.cursors_loaded.contains(&key) {
                return Some(CustomCursorRequest { key, image: None });
            }
            if self.cursors_failed.contains(&key) {
                return None;
            }
            let decoded = cursor::read(&self.asset_root, &image.url)
                .and_then(|bytes| cursor::decode(&bytes, image.hotspot, scale));
            match decoded {
                Some(rgba) => {
                    self.cursors_loaded.insert(key.clone());
                    Some(CustomCursorRequest {
                        key,
                        image: Some(rgba),
                    })
                }
                None => {
                    tracing::warn!(
                        "cursor image `{}` can't be used (missing, undecodable or over {}px)",
                        image.url.chars().take(80).collect::<String>(),
                        cursor::MAX_CURSOR_SIZE
                    );
                    self.cursors_failed.insert(key);
                    None
                }
            }
        });
        match custom {
            Some(request) => {
                window.set_cursor_visible(shown);
                self.custom_cursor = Some(request);
            }
            None => {
                window.set_cursor_visible(shown && keyword != "none");
                if let Ok(icon) = keyword.parse::<CursorIcon>() {
                    window.set_cursor(icon);
                }
            }
        }
    }

    /// Allows IME input while an editable field has focus, and keeps the candidate
    /// window next to its caret.
    fn update_ime(&mut self) {
        let Some(window) = self.main_window() else {
            return;
        };
        // A field on a world panel takes IME text too; its candidate window stays where
        // the platform puts it.
        let (allowed, area) = match self.keys_on {
            Surface::Panel(id) => (
                self.panels
                    .map
                    .get(&id)
                    .is_some_and(|p| p.interaction.ime_area(&p.tree).is_some()),
                None,
            ),
            Surface::Screen | Surface::Window(_) => {
                let area = self.interaction.ime_area(&self.ui);
                (area.is_some(), area)
            }
        };
        if allowed != self.ime_allowed {
            self.ime_allowed = allowed;
            window.set_ime_allowed(self.ime_allowed);
            self.ime_area = None;
        }
        if let Some(a) = area
            && Some(a) != self.ime_area
        {
            self.ime_area = Some(a);
            // UI pixels → the window's logical pixels.
            let z = self.scale() / self.main_metrics().map_or(1.0, |(dpi, _)| dpi as f32);
            window.set_ime_cursor_area(
                platypus_platform::dpi::LogicalPosition::new(a.x * z, a.y * z),
                platypus_platform::dpi::LogicalSize::new(
                    (a.width * z).max(1.0),
                    (a.height * z).max(1.0),
                ),
            );
        }
    }

    /// Sends an input event to the game's `input` API (@platypus/engine).
    fn send_game_input(&mut self, args: &[platypus_core::Value]) {
        if let Some(n) = self.native.as_mut() {
            n.game_input(&mut self.scene.scene, &mut self.ui, args);
            self.native_after();
        }
    }

    /// The system clipboard, opened on first use (it can be unavailable, e.g. headless).
    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(c) => self.clipboard = Some(c),
                Err(e) => tracing::warn!("clipboard unavailable: {e}"),
            }
        }
        self.clipboard.as_mut()
    }

    /// Physical pixels per UI pixel: the window's DPI scale times the UI scale.
    fn scale(&self) -> f32 {
        self.main_metrics()
            .map_or(1.0, |(dpi, size)| ui_factor_for(dpi, size))
    }

    fn now_ms(&self) -> f64 {
        self.now().duration_since(self.started).as_secs_f64() * 1000.0
    }

    /// The game's clock: real time, or a `--fixed-dt` bench run's steps.
    fn now(&self) -> Instant {
        self.fake_now.unwrap_or_else(Instant::now)
    }

    /// `loadFont(family, src, weight, style)` from `@platypus/engine`.
    fn load_font_op(&mut self, args: &[platypus_core::Value]) {
        use platypus_core::Value;
        let (Some(family), Some(src)) = (
            args.first().and_then(Value::as_str),
            args.get(1).and_then(Value::as_str),
        ) else {
            tracing::warn!("loadFont needs a family and a src: {args:?}");
            return;
        };
        let weight = match args.get(2) {
            Some(Value::Number(n)) => Some(*n as f32),
            Some(Value::String(s)) if s == "bold" => Some(700.0),
            Some(Value::String(s)) if s == "normal" => Some(400.0),
            _ => None,
        };
        let italic = args.get(3).and_then(Value::as_str).map(|s| s == "italic");
        load_font(&mut self.ui, &self.asset_root, family, src, weight, italic);
        for p in self.panels.map.values_mut() {
            load_font(&mut p.tree, &self.asset_root, family, src, weight, italic);
        }
        for w in self.windows.map.values_mut() {
            load_font(&mut w.tree, &self.asset_root, family, src, weight, italic);
        }
        self.font_sources
            .push((family.to_owned(), src.to_owned(), weight, italic));
    }

    /// `addStyleSheet(id, css)` / `removeStyleSheet(id)` from `@platypus/engine`.
    fn stylesheet_op(&mut self, name: &str, args: &[platypus_core::Value]) {
        let Some(js_id) = args
            .first()
            .and_then(platypus_core::Value::as_f64)
            .map(|n| n as u32)
        else {
            tracing::warn!("{name} needs an id: {args:?}");
            return;
        };
        // Every UI tree (the screen's and each panel's) has the same stylesheets.
        self.sheet_sources.retain(|(key, ..)| *key != js_id);
        for p in self.panels.map.values_mut() {
            if let Some(id) = p.sheets.remove(&js_id) {
                p.tree.remove_stylesheet(id);
            }
        }
        for w in self.windows.map.values_mut() {
            if let Some(id) = w.sheets.remove(&js_id) {
                w.tree.remove_stylesheet(id);
            }
        }
        if name == "removeStyleSheet" {
            if let Some(id) = self.sheets.remove(&js_id) {
                self.ui.remove_stylesheet(id);
            }
            return;
        }
        let css = args
            .get(1)
            .and_then(platypus_core::Value::as_str)
            .unwrap_or("");
        let id = add_stylesheet(&mut self.ui, &self.asset_root, css, "", "addStyleSheet");
        if let Some(old) = self.sheets.insert(js_id, id) {
            self.ui.remove_stylesheet(old);
        }
        for p in self.panels.map.values_mut() {
            let id = add_stylesheet(&mut p.tree, &self.asset_root, css, "", "addStyleSheet");
            p.sheets.insert(js_id, id);
        }
        for w in self.windows.map.values_mut() {
            let id = add_stylesheet(&mut w.tree, &self.asset_root, css, "", "addStyleSheet");
            w.sheets.insert(js_id, id);
        }
        self.sheet_sources
            .push((js_id, css.to_owned(), String::new(), "addStyleSheet".into()));
    }

    /// The renderer is ready: tells `SystemInfo` and `Window.main` about the GPU and the screen
    /// and makes the UI and world renderers. `window` is `None` for a windowless `--bench` run.
    fn install_gfx(
        &mut self,
        gpu: Gpu,
        surface: frame_target::FrameTarget,
        window: Option<Arc<Window>>,
    ) {
        tracing::info!(adapter = %gpu.adapter_name(), "renderer ready");
        let size = surface.size();
        let adapter = gpu.adapter.get_info();
        let monitor = window.as_ref().and_then(|w| w.current_monitor());
        let (sw, sh) = monitor.as_ref().map_or(size, |m| {
            let s = m.size();
            (s.width, s.height)
        });
        let hz = monitor
            .as_ref()
            .and_then(|m| m.refresh_rate_millihertz())
            .map_or(0.0, |mhz| f64::from(mhz) / 1000.0);
        let dpi = window.as_ref().map_or(1.0, |w| w.scale_factor());
        self.info.set_system(serde_json::json!({
            "graphicsDeviceName": adapter.name,
            "graphicsDeviceVendorID": adapter.vendor,
            "graphicsDeviceType": format!("{:?}", adapter.backend),
            "graphicsDeviceKind": format!("{:?}", adapter.device_type),
            "graphicsDriver": adapter.driver_info,
            "screenWidth": sw,
            "screenHeight": sh,
            "screenRefreshRate": hz,
            "dpi": dpi * 96.0,
        }));
        self.info
            .set("display", display::state(window.as_deref(), &self.display));
        let mut renderer = UiRenderer::new(&gpu);
        renderer.set_asset_root(&self.asset_root);
        renderer.set_blending(self.blending);
        let world = lazy_world::LazyWorld::new(&gpu, self.physical_units);
        // `Window.main` learns the real size and scale (its title may be the game's own).
        let title = platypus_engine_api::window::state(platypus_engine_api::window::MAIN)
            .map_or_else(String::new, |s| s.title);
        platypus_engine_api::window::set_main(&title, windows::to_ui(size, dpi), dpi);
        if let Some(w) = &window
            && title != w.title()
        {
            w.set_title(&title);
        }
        self.gfx = Some(Gfx {
            gpu,
            surface,
            window,
            renderer,
            world,
        });
    }

    fn save_screenshot(&mut self, path: &Path) -> Result<()> {
        if self.gles.is_some() {
            return self.save_screenshot_gles(path);
        }
        if self.cpu.is_some() {
            return self.save_screenshot_cpu(path);
        }
        let gfx = self.gfx.as_mut().context("no renderer")?;
        let (w, h) = gfx.surface.size();
        let target =
            OffscreenTarget::with_format(&gfx.gpu, w, h, wgpu::TextureFormat::Rgba8UnormSrgb);
        let view = target.view().context("window has no size")?;
        load_images(&mut self.ui, &mut gfx.renderer, &gfx.gpu);
        let cmds = self.ui.draw_list();
        self.scene.scene.update_world();
        // Screenshot runs never wait for the display (as before vsync was on by default).
        gfx.surface
            .set_vsync(&gfx.gpu, self.world_view.quality.vsync && !self.offline);
        let scale = ui_factor(gfx);
        let canvases = canvas_cameras::sync(&mut self.world_view, &self.ui, scale);
        if let Some(world) = gfx.world.ensure(&gfx.gpu, self.world_view.needs_world()) {
            apply_world_view(
                world,
                &gfx.gpu,
                &mut self.world_view,
                &mut self.pipeline_warned,
            );
        }
        canvas_cameras::show(
            &canvases,
            &mut self.world_view,
            &mut gfx.world,
            &mut gfx.renderer,
            &gfx.gpu,
        );
        let now = self.started.elapsed().as_secs_f64() * 1000.0;
        let (quads, textures) = self.panels.render(
            &gfx.gpu,
            &mut gfx.renderer,
            &self.scene,
            now,
            |tree, renderer| load_images(tree, renderer, &gfx.gpu),
        );
        let views = self.world_view.views(&self.scene, (w, h), &quads);
        let textures: Vec<&wgpu::TextureView> = textures.iter().collect();
        let mut cmds = cmds;
        if self.fps_on {
            let now = self.started.elapsed().as_secs_f64();
            let fps_scale = gfx.dpi() as f32;
            let text = self.fps.text(now);
            let fps_cmds = self.fps_overlay.draw_list(&text, w, h, fps_scale);
            ui_frame::append_scaled(&mut cmds, fps_cmds, fps_scale / scale);
        }
        draw_frame(
            &gfx.gpu,
            &mut gfx.renderer,
            &mut gfx.world,
            &RenderTarget {
                view: &view,
                format: target.format(),
                size: (w, h),
                scale,
            },
            &views,
            &textures,
            &cmds,
        );
        let pixels = target.read_pixels(&gfx.gpu)?;
        platypus_render::testing::write_png(path, w, h, &pixels)?;
        self.save_window_screenshots(path)
    }
}

impl PlatformApp for App {
    fn take_custom_cursor(&mut self) -> Option<CustomCursorRequest> {
        self.custom_cursor.take()
    }

    fn take_window_commands(&mut self) -> Vec<platypus_platform::WindowCommand> {
        std::mem::take(&mut self.window_commands)
    }

    fn window_opened(
        &mut self,
        key: platypus_platform::WindowKey,
        host: platypus_platform::WindowHost,
    ) {
        self.window_open(key, host);
    }

    fn window_open_failed(&mut self, key: platypus_platform::WindowKey, error: String) {
        self.in_game(|| {
            platypus_engine_api::window::open_failed(
                platypus_engine_api::window::WindowRef(key.0),
                &error,
            )
        });
    }

    fn window_input(&mut self, key: platypus_platform::WindowKey, event: InputEvent) {
        if key == platypus_platform::WindowKey::MAIN {
            self.input(event);
        } else {
            self.window_input_for(key, event);
        }
    }

    fn close_requested(&mut self, key: platypus_platform::WindowKey) -> bool {
        self.window_close_requested(key)
    }

    fn window_closed(&mut self, key: platypus_platform::WindowKey) {
        self.window_gone(key);
    }

    fn events_done(&mut self) {
        self.window_events_done();
    }

    fn take_windows_to_show(&mut self) -> Vec<platypus_platform::WindowKey> {
        std::mem::take(&mut self.windows_to_show)
    }

    fn window_ready(&mut self, window: Arc<Window>) {
        // The saved (or default) window mode, before the first frame shows. Screenshot
        // runs stay windowed.
        if !self.offline && self.display.mode != platypus_platform::FullScreenMode::Windowed {
            self.display = platypus_platform::display::apply(&window, &self.display);
        }
        match host::choice() {
            host::Choice::World => {}
            choice => {
                // The light path: GLES through EGL (ANGLE) or the CPU renderer; wgpu with small
                // GPU blocks (the world renderer built only if something asks for it) when that
                // isn't available.
                let light = if choice == host::Choice::Cpu {
                    self.install_cpu(window.clone())
                } else {
                    std::env::var_os("PLATYPUS_GLES").is_none_or(|v| v != "0")
                        && self.install_gles(window.clone())
                };
                if light {
                    return;
                }
                tracing::warn!("falling back to wgpu with small GPU allocations");
                platypus_render::set_low_memory(true);
            }
        }
        let size = window.inner_size();
        match Gpu::with_surface(window.clone(), size.width, size.height) {
            Ok((gpu, surface)) => {
                self.install_gfx(
                    gpu,
                    frame_target::FrameTarget::Window(surface),
                    Some(window),
                );
            }
            Err(e) => tracing::error!("renderer unavailable: {e}"),
        }
    }

    fn input(&mut self, event: InputEvent) {
        if self.file_drag.event(&event) {
            return;
        }
        if let InputEvent::Gamepad { id, event } = &event {
            self.send_game_input(&game_input::script_args(*id, event));
            // The d-pad moves UI focus only once something has it.
            let nav = self.nav.ui_input(*id, event);
            if let Some(nav) = nav.filter(|_| self.interaction.focused().is_some()) {
                self.handle_ui(nav);
            }
            return;
        }
        // The mouse for gameplay code (`input.mousePosition`, buttons, look, wheel), in UI
        // pixels, whatever the UI does with it.
        let ui = self.scale() as f64;
        let game_mouse: Option<Vec<platypus_core::Value>> = match &event {
            InputEvent::MouseMove { x, y } => {
                Some(vec!["mousemove".into(), (x / ui).into(), (y / ui).into()])
            }
            InputEvent::MouseDown { button } | InputEvent::MouseUp { button } => {
                let code = match button {
                    MouseButton::Left => "MouseLeft".to_owned(),
                    MouseButton::Right => "MouseRight".to_owned(),
                    MouseButton::Middle => "MouseMiddle".to_owned(),
                    MouseButton::Other(n) => format!("Mouse{n}"),
                };
                let down = matches!(event, InputEvent::MouseDown { .. });
                Some(if down {
                    vec!["keydown".into(), code.into(), "".into(), false.into()]
                } else {
                    vec!["keyup".into(), code.into(), "".into()]
                })
            }
            InputEvent::MouseWheel(delta) => {
                let (dx, dy) = match delta {
                    WheelDelta::Lines { x, y } => (f64::from(*x), f64::from(*y)),
                    // About a line per 40 pixels, like browsers.
                    WheelDelta::Pixels { x, y } => (x / 40.0, y / 40.0),
                };
                Some(vec!["wheel".into(), dx.into(), dy.into()])
            }
            InputEvent::MouseMotion { dx, dy } if self.focused => {
                Some(vec!["mousedelta".into(), (*dx).into(), (*dy).into()])
            }
            _ => None,
        };
        if let Some(args) = game_mouse {
            self.send_game_input(&args);
        }
        match event {
            // The game's focus is any of its windows' (`window_events_done`).
            InputEvent::Focused(f) => {
                if f {
                    self.windows
                        .focused
                        .insert(platypus_platform::WindowKey::MAIN);
                } else {
                    self.windows
                        .focused
                        .remove(&platypus_platform::WindowKey::MAIN);
                }
                self.in_game(|| {
                    platypus_engine_api::window::focus_changed(platypus_engine_api::window::MAIN, f)
                });
            }
            InputEvent::Occluded(o) => self.lifecycle.set_occluded(o),
            InputEvent::Resized { width, height } => {
                native_render::display_dirty();
                self.lifecycle.set_size(width, height);
                if let Some((dpi, _)) = self.main_metrics() {
                    let size = windows::to_ui((width, height), dpi);
                    self.in_game(|| {
                        platypus_engine_api::window::resized(
                            platypus_engine_api::window::MAIN,
                            size.0,
                            size.1,
                        )
                    });
                }
            }
            InputEvent::Moved { x, y } => {
                native_render::display_dirty();
                if let Some((dpi, _)) = self.main_metrics() {
                    let (lx, ly) = (f64::from(x) / dpi, f64::from(y) / dpi);
                    self.in_game(|| {
                        platypus_engine_api::window::moved(
                            platypus_engine_api::window::MAIN,
                            lx,
                            ly,
                        )
                    });
                }
            }
            InputEvent::ScaleChanged(s) => {
                native_render::display_dirty();
                self.in_game(|| {
                    platypus_engine_api::window::scale_changed(platypus_engine_api::window::MAIN, s)
                });
            }
            _ => {}
        }
        if matches!(event, InputEvent::Occluded(_) | InputEvent::Resized { .. }) {
            self.sync_lifecycle();
        }
        if let InputEvent::Focused(focused) = event {
            self.focused = focused;
            // Grabs are let go while the window is in the background.
            if focused {
                self.apply_cursor_lock();
            }
        }
        if let InputEvent::MouseMotion { .. } = event {
            return;
        }
        let game_key: Option<Vec<platypus_core::Value>> = match &event {
            InputEvent::KeyDown {
                code, key, repeat, ..
            } => Some(vec![
                "keydown".into(),
                code.as_str().into(),
                key.as_str().into(),
                (*repeat).into(),
            ]),
            InputEvent::KeyUp { code, key } => Some(vec![
                "keyup".into(),
                code.as_str().into(),
                key.as_str().into(),
            ]),
            InputEvent::Focused(false) => Some(vec!["blur".into()]),
            _ => None,
        };
        let scale = self.scale() as f64;
        let input = match event {
            InputEvent::Resized { width, height } => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.surface.resize(&gfx.gpu, width, height);
                }
                return;
            }
            InputEvent::MouseMove { x, y } => {
                self.pointer_px = Some((x as f32, y as f32));
                self.route_pointer();
                return;
            }
            InputEvent::MouseLeave => {
                self.pointer_px = None;
                self.handle_on(self.pointer_on, Input::PointerLeave);
                return;
            }
            InputEvent::MouseDown { button } => {
                self.buttons_down += 1;
                // The UI inspector's pick mode keeps the press from the game.
                if self.inspector_takes_press() {
                    return;
                }
                // Keys go to whatever was clicked last.
                self.keys_on = self.pointer_on;
                let time_ms = self.now_ms();
                self.handle_on(
                    self.pointer_on,
                    Input::PointerDown {
                        button: dom_button(button),
                        time_ms,
                    },
                );
                return;
            }
            InputEvent::MouseUp { button } => {
                if platypus_engine_api::ui_inspector::release() {
                    self.buttons_down = self.buttons_down.saturating_sub(1);
                    return;
                }
                self.handle_on(
                    self.pointer_on,
                    Input::PointerUp {
                        button: dom_button(button),
                    },
                );
                self.buttons_down = self.buttons_down.saturating_sub(1);
                // The pointer may be over something else now.
                if self.buttons_down == 0 {
                    self.route_pointer();
                }
                return;
            }
            InputEvent::MouseWheel(delta) => {
                let (dx, dy) = match delta {
                    WheelDelta::Lines { x, y } => (x * WHEEL_LINE, y * WHEEL_LINE),
                    WheelDelta::Pixels { x, y } => ((x / scale) as f32, (y / scale) as f32),
                };
                self.handle_on(self.pointer_on, Input::Wheel { dx, dy });
                return;
            }
            InputEvent::KeyDown {
                code,
                key,
                repeat,
                text,
            } => Input::KeyDown {
                key,
                code,
                repeat,
                text,
            },
            InputEvent::KeyUp { code, key } => Input::KeyUp { key, code },
            InputEvent::Modifiers(m) => {
                let modifiers = events::Modifiers {
                    shift: m.shift,
                    ctrl: m.ctrl,
                    alt: m.alt,
                    meta: m.meta,
                };
                for surface in self.surfaces() {
                    self.handle_on(surface, Input::Modifiers(modifiers));
                }
                return;
            }
            // Releases outside an unfocused window may never arrive.
            InputEvent::Focused(false) => {
                self.buttons_down = 0;
                for surface in self.surfaces() {
                    self.handle_on(surface, Input::WindowBlur);
                }
                if let Some(args) = game_key {
                    self.send_game_input(&args);
                }
                return;
            }
            InputEvent::Ime(ImeEvent::Preedit(text, cursor)) => Input::ImePreedit { text, cursor },
            InputEvent::Ime(ImeEvent::Commit(text)) => Input::ImeCommit(text),
            InputEvent::Ime(ImeEvent::Disabled) => Input::ImePreedit {
                text: String::new(),
                cursor: None,
            },
            _ => return,
        };
        // Keys and IME text go to the surface clicked last.
        self.handle_on(self.keys_on, input);
        // Keys typed into a text field are the field's, not the game's.
        let typing = match self.keys_on {
            Surface::Screen | Surface::Window(_) => self
                .interaction
                .focused()
                .is_some_and(|id| self.ui.is_editor(id)),
            Surface::Panel(id) => self
                .panels
                .map
                .get(&id)
                .is_some_and(|p| p.interaction.focused().is_some_and(|f| p.tree.is_editor(f))),
        };
        if let Some(args) = game_key.filter(|_| !typing) {
            self.send_game_input(&args);
        }
    }

    fn frame(&mut self) {
        // Size the UI before scripts run and their output applies: on the first frame the
        // tree is still 1x1, and their layout and scrolling (`scrollIntoView` on mount)
        // would happen at that size.
        self.sync_screen();
        self.poll_video_settings();
        // Domain turns: the main game's messages first, then its tick, then the isolated domains
        // (messages, timers, input), then the setup window of new windows ends.
        if !self.lifecycle.paused() {
            let now_ms = self.now_ms();
            self.in_game(|| {
                platypus_engine_api::window::deliver(
                    platypus_engine_api::window::DomainId::MAIN,
                    now_ms,
                )
            });
        }
        let t_tick = Instant::now();
        self.tick();
        let d_tick = t_tick.elapsed();
        let now_ms = self.now_ms();
        self.pump_domains(now_ms);
        platypus_engine_api::window::end_frame();
        self.close_unclaimed_windows();
        self.pump_file_drag();
        // Last-moment changes before drawing (`onPreRender`).
        if !self.lifecycle.paused()
            && let Some(n) = self.native.as_mut()
        {
            let t = Instant::now();
            n.pre_render(&mut self.scene.scene, &mut self.ui);
            self.script_time += t.elapsed();
            self.native_after();
            self.native_audio_commands();
            let t = Instant::now();
            self.sync_native_render();
            self.sync_time += t.elapsed();
        }
        // Caret blinking.
        self.ui.set_time(self.now_ms());
        // The caret may have moved with layout.
        self.update_ime();
        // Again, for a UI scale the scripts changed this frame.
        self.sync_screen();
        let render_started = Instant::now();
        if self.gles.is_some() {
            self.render_gles(render_started);
            self.count_frame();
            return;
        }
        if self.cpu.is_some() {
            self.render_cpu(render_started);
            self.count_frame();
            return;
        }
        let Some(gfx) = &mut self.gfx else {
            return;
        };
        let scale = ui_factor(gfx);
        // Scripts' `RenderStats`: what the last frame drew and culled, when it changes.
        let stats = gfx.world.built().map(|w| w.stats()).unwrap_or_default();
        if self.render_stats != stats {
            self.render_stats = stats;
            // A compiled game reads `RenderStats` from the registry.
            platypus_render_api::set_render_stats(platypus_render_api::Stats {
                meshes: stats.meshes as f64,
                culled_meshes: stats.culled_meshes as f64,
                quads: stats.quads as f64,
                culled_quads: stats.culled_quads as f64,
                fog_culled: stats.fog_culled as f64,
                lights: stats.lights as f64,
                culled_lights: stats.culled_lights as f64,
                shadow_maps: stats.shadow_maps as f64,
                shadow_casters: stats.shadow_casters as f64,
            });
        }
        // Each camera's exposure (EV100 in use, and how bright it metered), when it changes.
        for id in self.world_view.camera_ids() {
            let Some(r) = gfx.world.built().and_then(|w| w.exposure(id)) else {
                continue;
            };
            let now = (r.ev, r.scene_ev);
            let moved = self.exposure_sent.get(&id).is_none_or(|&(ev, scene)| {
                (ev - now.0).abs() > 0.01
                    || (scene - now.1).abs() > 0.01
                    || scene.is_nan() != now.1.is_nan()
            });
            if moved {
                self.exposure_sent.insert(id, now);
                // A compiled game's cameras (the id is the camera component's).
                platypus_engine_api::camera::set_exposure(
                    platypus_engine_api::camera::CameraRef(id),
                    f64::from(now.0),
                    f64::from(now.1),
                );
            }
        }
        // `@media (resolution)` follows the window, `(prefers-color-scheme)` the system (or the
        // game's `theme`).
        let dark = self.manifest.dark(gfx.window.as_deref());
        self.ui.set_media_env(web_ui_dom::media::MediaEnv {
            resolution: scale,
            dark,
            ..self.ui.media_env()
        });
        load_images(&mut self.ui, &mut gfx.renderer, &gfx.gpu);
        let t_draw = Instant::now();
        let cmds = self.ui.draw_list();
        let d_draw = t_draw.elapsed();
        self.scene.scene.update_world();
        // Screenshot runs never wait for the display (as before vsync was on by default).
        gfx.surface
            .set_vsync(&gfx.gpu, self.world_view.quality.vsync && !self.offline);
        let canvases = canvas_cameras::sync(&mut self.world_view, &self.ui, scale);
        if let Some(world) = gfx.world.ensure(&gfx.gpu, self.world_view.needs_world()) {
            apply_world_view(
                world,
                &gfx.gpu,
                &mut self.world_view,
                &mut self.pipeline_warned,
            );
        }
        canvas_cameras::show(
            &canvases,
            &mut self.world_view,
            &mut gfx.world,
            &mut gfx.renderer,
            &gfx.gpu,
        );
        let now = self
            .fake_now
            .unwrap_or_else(Instant::now)
            .duration_since(self.started)
            .as_secs_f64()
            * 1000.0;
        let gfx_window_scale = gfx.dpi() as f32;
        let (scene, world_view) = (&self.scene, &self.world_view);
        let Gfx {
            gpu,
            surface,
            renderer,
            world,
            ..
        } = gfx;
        // World panels draw into their textures first.
        let (quads, textures) = self
            .panels
            .render(gpu, renderer, scene, now, |tree, renderer| {
                load_images(tree, renderer, gpu)
            });
        let textures: Vec<&wgpu::TextureView> = textures.iter().collect();
        // The FPS counter: at window resolution and scale, over the world and the UI.
        let window_scale = gfx_window_scale;
        let fps_text = self
            .fps_on
            .then(|| self.fps.text(self.started.elapsed().as_secs_f64()));
        let fps_overlay = &mut self.fps_overlay;
        let mut cmds = cmds;
        let t_render = Instant::now();
        surface.present_with(gpu, |view, format, size| {
            let target = RenderTarget {
                view,
                format,
                size,
                scale,
            };
            if let Some(text) = &fps_text {
                let fps_cmds = fps_overlay.draw_list(text, size.0, size.1, window_scale);
                ui_frame::append_scaled(&mut cmds, fps_cmds, window_scale / scale);
            }
            draw_frame(
                gpu,
                renderer,
                world,
                &target,
                &world_view.views(scene, size, &quads),
                &textures,
                &cmds,
            );
        });
        if profile::enabled() {
            self.profile
                .record(d_tick, d_draw, t_render.elapsed(), cmds.len());
        }
        self.world_view.probes_rendered();
        // What it saw of the renderers animation culling watches (nothing if no view drew).
        if let Some(gfx) = &mut self.gfx {
            let seen = gfx.world.built().map(|w| w.take_seen()).unwrap_or_default();
            self.world_view.culling.note_seen(&seen);
        }
        self.draw_windows();
        self.render_time += render_started.elapsed();
        self.tick_fps();
        self.count_frame();
    }

    fn should_exit(&self) -> bool {
        self.done
    }

    fn frame_interval(&self) -> Option<Duration> {
        let cap = lifecycle::frame_cap_with(
            self.target_frame_rate,
            self.world_view.quality.vsync,
            self.offline,
            self.lifecycle.paused() && !self.paused(),
        );
        // A frame that changed nothing isn't swapped, so nothing waits for the display: the
        // monitor's refresh rate unless the game sets a rate.
        if (self.gles.is_some() || self.cpu.is_some()) && !self.offline {
            let window = self
                .gles
                .as_ref()
                .map(|g| &g.window)
                .or_else(|| self.cpu.as_ref().map(|c| &c.window));
            let refresh = window.map_or(gles_gfx::FRAME_CAP, |w| gles_gfx::frame_cap(w));
            return Some(cap.unwrap_or(refresh));
        }
        cap
    }

    /// Paused only when every event domain is (a live inspector window keeps the app running).
    fn paused(&self) -> bool {
        lifecycle::app_paused(self.lifecycle.paused(), self.domains.all_paused())
    }

    fn idle(&mut self) {
        // The video-settings dialog pauses the game: its answer arrives here.
        self.poll_video_settings();
        // Hot reload still picks up rebuilds; they show once the game resumes.
        if self
            .watcher
            .as_mut()
            .is_some_and(|w| w.poll(Instant::now()))
        {
            self.reload_scripts();
        }
    }
}

impl Drop for App {
    /// The player's settings are written when the game closes, like Unity's.
    fn drop(&mut self) {
        // The program's closures go while the engine's per-thread state is still alive.
        native_render::clear_state();
        if let Some(n) = self.native.take() {
            n.shutdown();
        }
        self.prefs.save();
    }
}

/// Physical pixels per UI pixel for `window`: its DPI scale times the scripts' `UiScale` factor
/// for its logical size.
fn ui_factor(gfx: &Gfx) -> f32 {
    ui_factor_for(gfx.dpi(), gfx.inner_size())
}

/// Physical pixels per UI pixel for a window with this DPI scale and physical size.
fn ui_factor_for(dpi: f64, (width, height): (u32, u32)) -> f32 {
    let dpi = dpi as f32;
    let (w, h) = (width as f32 / dpi, height as f32 / dpi);
    dpi * platypus_engine_api::ui_scale::current().factor(w, h)
}

/// The DOM's button number: 0 primary, 1 middle, 2 secondary, 3 back, 4 forward.
fn dom_button(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Other(n) => n.min(u8::MAX as u16) as u8,
    }
}

/// How scripts hold the cursor (Unity's `CursorLockMode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CursorLock {
    #[default]
    None,
    /// Hidden and held in place; raw motion still arrives (mouse look).
    Locked,
    /// Kept inside the window.
    Confined,
}

/// Scripts' `Cursor` settings.
#[derive(Clone, Debug)]
struct ScriptCursor {
    visible: bool,
    lock: CursorLock,
    /// Shown where the UI asks for no particular cursor.
    image: Option<web_ui_dom::CursorImage>,
}

impl Default for ScriptCursor {
    fn default() -> Self {
        Self {
            visible: true,
            lock: CursorLock::None,
            image: None,
        }
    }
}

/// Loads a font file (or `data:` URL) from the game folder and registers it.
/// Loads images that `image` elements started using and tells the UI their natural
/// sizes, so they lay out before they're drawn.
fn load_images(ui: &mut UiTree, renderer: &mut UiRenderer, gpu: &Gpu) {
    for src in ui.take_image_requests() {
        let size = renderer.image_size(gpu, &src);
        ui.set_image_size(&src, size);
    }
}

fn load_font(
    ui: &mut UiTree,
    root: &Path,
    family: &str,
    src: &str,
    weight: Option<f32>,
    italic: Option<bool>,
) {
    let data = platypus_core::read_asset(root, src).map_err(|e| e.to_string());
    match data.and_then(|d| ui.register_font(d, Some(family), weight, italic)) {
        Ok(_) => tracing::info!("loaded font `{family}`"),
        Err(e) => tracing::warn!("font `{family}` failed to load: {e}"),
    }
}

/// Adds a stylesheet, logging its warnings and loading its `@font-face` fonts.
/// `base` is the stylesheet's folder inside the game folder (for its `url()`s).
/// `audio.load id src stream`: reads the file (or `data:` URL) from the game folder.
fn load_sound(
    mixer: &mut dyn platypus_audio::Mixer,
    root: &Path,
    args: &[platypus_core::Value],
) -> Result<(), String> {
    let (Some(id), Some(src)) = (
        args.first().and_then(platypus_core::Value::as_f64),
        args.get(1).and_then(platypus_core::Value::as_str),
    ) else {
        return Err("needs an id and a file".into());
    };
    let stream = matches!(args.get(2), Some(platypus_core::Value::Bool(true)));
    // A streamed sound is read in pieces (a pack's chunks, a file's windows), never whole.
    if stream
        && !src.trim_start().starts_with("data:")
        && let Some(fs) = platypus_core::asset_fs()
    {
        let reader = fs.open_range(src).map_err(|e| e.to_string())?;
        return mixer
            .load_stream(id as u32, reader)
            .map_err(|e| format!("`{src}`: {e}"));
    }
    let bytes = platypus_core::read_asset(root, src).map_err(|e| e.to_string())?;
    mixer
        .load(id as u32, bytes, stream)
        .map_err(|e| format!("`{src}`: {e}"))
}

/// A UI that takes input: the screen's, or a world panel's (by its id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
    Screen,
    Panel(u32),
    /// A secondary window's UI (by window handle).
    Window(u32),
}

/// The key `game.css` has among the stylesheets every UI tree gets.
const GAME_CSS: u32 = u32::MAX;

/// The game's stylesheet as a source for every UI tree: `embedded` (the bundle's `cssText`) or
/// the loose `css_path`, with the folder its `url()`s resolve from.
fn game_css_source(
    embedded: Option<String>,
    code_root: &Path,
    css_path: &Path,
) -> Option<(u32, String, String, String)> {
    let css = embedded.or_else(|| std::fs::read_to_string(css_path).ok())?;
    let base = asset_root::css_base(code_root, css_path);
    Some((GAME_CSS, css, base, css_path.display().to_string()))
}

/// A UI tree for a new world panel, with the fonts and stylesheets the screen has.
fn new_panel_tree(
    root: &Path,
    fonts: &[(String, String, Option<f32>, Option<bool>)],
    sheets: &[(u32, String, String, String)],
) -> panels::NewTree {
    let mut tree = UiTree::with_system_fonts(1.0, 1.0);
    for (family, src, weight, italic) in fonts {
        load_font(&mut tree, root, family, src, *weight, *italic);
    }
    let ids = sheets
        .iter()
        .map(|(key, css, base, source)| (*key, add_stylesheet(&mut tree, root, css, base, source)))
        .collect();
    (tree, ids)
}

fn add_stylesheet(ui: &mut UiTree, root: &Path, css: &str, base: &str, source: &str) -> u32 {
    // `@import`s load from the game folder (Bun inlines them when bundling; this is
    // for stylesheets added at runtime).
    let added = ui.add_stylesheet_with_imports(css, base, &mut |path| {
        platypus_core::read_asset_string(root, path).ok()
    });
    for warning in &added.warnings {
        tracing::warn!(target: "css", "{source}: {warning}");
    }
    for face in &added.font_faces {
        load_font(ui, root, &face.family, &face.src, face.weight, face.italic);
    }
    added.id
}

/// Says once in the log when the game asks for light shafts its quality level leaves out
/// (`volumetricFog: false`, a phone's level, say), as Unity does.
fn warn_pipeline(warned: &mut bool, world: &world_view::WorldViews) {
    if !*warned && !world.quality.pipeline.volumetric_fog && world.uses_volumetric_fog() {
        *warned = true;
        tracing::warn!("RenderSettings.volumetricFog: this quality level has light shafts off");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args> {
        parse_args_from(args.iter().map(std::ffi::OsString::from))
    }

    // M5-APP-03
    #[test]
    fn game_folder_defaults_to_the_current_folder() {
        let args = parse(&[]).unwrap();
        assert_eq!(args.game, PathBuf::from("."));
        let args = parse(&["--screenshot", "out.png", "--frames", "5"]).unwrap();
        assert_eq!(args.game, PathBuf::from("."));
        assert_eq!(args.screenshot, Some(PathBuf::from("out.png")));
        assert_eq!(args.frames, 5);
    }

    // M9-APP-01
    #[test]
    fn force_is_no_longer_an_option() {
        assert!(parse(&["g", "--force"]).is_err());
    }

    // M9-APP-02
    #[test]
    fn a_quickjs_bundle_is_refused_with_a_pointer_to_build() {
        let dir = std::env::temp_dir().join(format!("platypus-qjs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("build")).unwrap();
        std::fs::write(dir.join("build/game.js"), "").unwrap();
        for path in [dir.clone(), dir.join("build/game.js")] {
            let err = resolve_bundle(&path).unwrap_err().to_string();
            assert!(err.contains("QuickJS"), "{err}");
            assert!(err.contains("platypus-build build"), "{err}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // M9-APP-06
    #[test]
    fn a_game_with_only_an_old_dist_build_is_refused_with_a_pointer_to_build() {
        let dir = std::env::temp_dir().join(format!("platypus-old-dist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        // A game built before the rename and the move to `build/`.
        std::fs::write(dir.join("dist").join(native::LEGACY_BUNDLE), "{}").unwrap();
        let err = resolve_bundle(&dir).unwrap_err().to_string();
        assert!(err.contains("`dist/`") && err.contains("`build/`"), "{err}");
        assert!(err.contains("platypus-build build"), "{err}");
        std::fs::create_dir_all(dir.join("build")).unwrap();
        std::fs::write(dir.join("build/game.pbundle"), "{}").unwrap();
        let (bundle, root) = resolve_bundle(&dir).unwrap();
        assert_eq!(bundle, dir.join("build").join("game.pbundle"));
        assert_eq!(root, dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // M9-API-909
    #[test]
    fn fps_flags_parse() {
        assert_eq!(parse(&[]).unwrap().fps, None);
        assert_eq!(parse(&["--fps"]).unwrap().fps, Some(true));
        assert_eq!(parse(&["g", "--no-fps"]).unwrap().fps, Some(false));
    }

    // M9-API-917
    #[test]
    fn crash_dialog_flag_names_the_dialog_folder() {
        let a = parse(&["--crash-dialog", "C:/tmp/dialog"]).unwrap();
        assert!(a.crash_dialog);
        assert_eq!(a.game, PathBuf::from("C:/tmp/dialog"));
        assert!(!parse(&["g"]).unwrap().crash_dialog);
        assert!(parse(&["--crash-dialog"]).is_err());
    }

    // M9-VSP-07
    #[test]
    fn video_settings_flags_parse() {
        assert_eq!(parse(&["g"]).unwrap().video_prompt, None);
        let on = parse(&["g", "--video-settings-prompt"]).unwrap();
        assert_eq!((on.video_prompt, on.game), (Some(true), PathBuf::from("g")));
        assert_eq!(
            parse(&["--no-video-settings-prompt", "g"])
                .unwrap()
                .video_prompt,
            Some(false)
        );
        let dialog = parse(&["--video-settings-dialog", "C:/tmp/vs"]).unwrap();
        assert!(dialog.video_dialog);
        assert_eq!(dialog.game, PathBuf::from("C:/tmp/vs"));
        assert!(parse(&["--video-settings-dialog"]).is_err());
    }

    // M5-APP-03
    #[test]
    fn game_folder_can_come_before_or_after_options() {
        assert_eq!(
            parse(&["examples/css"]).unwrap().game,
            PathBuf::from("examples/css")
        );
        let args = parse(&["--frames", "3", "examples/css"]).unwrap();
        assert_eq!(args.game, PathBuf::from("examples/css"));
        assert!(parse(&["a", "b"]).is_err());
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["--cook", "examples/world"]).unwrap().cook);
        // The editor: the game is its UI, `--project` the folder it works on.
        let args = parse(&["editor", "--project", "examples/arcade"]).unwrap();
        assert_eq!(args.game, PathBuf::from("editor"));
        assert_eq!(args.project, Some(PathBuf::from("examples/arcade")));
        assert!(parse(&["editor", "--project"]).is_err());
    }

    /// A mono 16-bit WAV of a sine, `n` samples at 8 kHz.
    fn wav(n: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + n * 2).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&[1, 0, 1, 0]);
        out.extend_from_slice(&8000u32.to_le_bytes());
        out.extend_from_slice(&16000u32.to_le_bytes());
        out.extend_from_slice(&[2, 0, 16, 0]);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(n * 2).to_le_bytes());
        for i in 0..n {
            let v = ((i as f32 * 0.3).sin() * 8000.0) as i16;
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    // M9-API-1148
    #[test]
    fn sounds_load_from_packs_whole_and_streamed_and_a_miss_says_where_it_looked() {
        use platypus_core::Value;
        use platypus_pack::{EntrySpec, Pack, PackSource, PackWriter, Vfs, WriteOptions};
        let sound = wav(40_000);
        let mut w =
            PackWriter::new(std::io::Cursor::new(Vec::new()), WriteOptions::default()).unwrap();
        w.add(&EntrySpec::new("assets/sfx/beep.wav", &sound))
            .unwrap();
        let pack = Pack::from_bytes(w.finish().unwrap().0.into_inner()).unwrap();
        let mut vfs = Vfs::new();
        vfs.mount(std::sync::Arc::new(PackSource::new("assets", pack)));
        let root = Path::new("no-such-folder");
        let load = |mixer: &mut dyn platypus_audio::Mixer, id: f64, src: &str, stream: bool| {
            let args = [
                Value::Number(id),
                Value::String(src.into()),
                Value::Bool(stream),
            ];
            platypus_core::with_asset_fs(std::sync::Arc::new(vfs.clone()), || {
                load_sound(mixer, root, &args)
            })
        };
        let mut mixer = platypus_audio::open_mixer(true);
        load(mixer.as_mut(), 1.0, "assets/sfx/beep.wav", false).unwrap();
        // Streamed: read through the pack's chunks (the file is compressed, 3 chunks of 256 KiB
        // would not be needed for 80 kB, but the reader works in windows all the same).
        load(mixer.as_mut(), 2.0, "assets/sfx/beep.wav", true).unwrap();
        let e = load(mixer.as_mut(), 3.0, "assets/sfx/none.wav", false).unwrap_err();
        assert_eq!(e, "asset \"assets/sfx/none.wav\" not found in assets.dat");
        let e = load(mixer.as_mut(), 4.0, "assets/sfx/none.wav", true).unwrap_err();
        assert!(e.contains("not found in assets.dat"), "{e}");
        mixer.advance(0.1);
    }

    // M5-APP-11
    #[test]
    fn arguments_after_a_double_dash_are_for_scripts() {
        let args = parse(&["examples/md-reader", "--", "--goto", "a.md#b", "x"]).unwrap();
        assert_eq!(args.game, PathBuf::from("examples/md-reader"));
        assert_eq!(args.script_args, ["--goto", "a.md#b", "x"]);
        // Platypus's own options before `--` still count; nothing after it does.
        let args = parse(&["--frames", "3", "--", "--frames", "9"]).unwrap();
        assert_eq!((args.frames, args.script_args.len()), (3, 2));
        assert!(parse(&["examples/css"]).unwrap().script_args.is_empty());
    }
}
