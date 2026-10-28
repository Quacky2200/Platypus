// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The game: compiled by `platypus_ts` (TypeScript to machine code) and run in this process.
//!
//! A native game is `build/game.pbundle` (JSON written by `platypus-build`: the bundled TypeScript
//! source plus the stylesheet and asset list) or a bare `.ts` / `.tsx` file. [`Native::load`]
//! JIT-compiles the source with the manifests of every engine namespace, runs the program's top
//! level (`register(..)`, `UiRoot.mount(..)`) and spawns the starting components. A module with a
//! top-level `await` keeps the program from starting: until every module has finished evaluating
//! (`platypus_ts_rt::startup_state`) the registered classes are not spawned, the start scene is not
//! loaded and no component hook runs, while the timers, microtasks and the UI the top level
//! mounted keep going. After that the app calls the `Native` methods:
//!
//! | App event | Native call |
//! |---|---|
//! | fixed tick | [`Native::fixed_tick`]: `onFixedUpdate`, physics step, contact hooks |
//! | frame | [`Native::frame`]: `Time`, timers and microtasks, `onUpdate` / `onLateUpdate`, UI state |
//! | before drawing | [`Native::pre_render`] |
//! | UI events | [`Native::ui_events`] (pointer, keys, focus) to the `Runtime` |
//! | hot reload | [`Native::reload`] |
//!
//! Every call runs with the app's scene made current (`platypus_scene::api::enter`), because compiled
//! code reaches the scene through a per-thread pointer, not through an argument.
//!
//! UI events run through the handlers while the interaction layer handles the input, so
//! `preventDefault()` stops default actions.

use anyhow::{Context, Result, anyhow, bail};
use platypus_scene::Scene;
use platypus_ts::jit::{Game, JitOptions, compile_and_load};
use platypus_ts::renderer::Tier;
use platypus_ts_rt::{
    NativeHost, StartupState, startup_state, ts_rt_microtasks_run, ts_rt_timers_advance,
};
use platypus_ui::{UiEvent, UiTree};
use platypus_ui_native::{Event, RootId, Runtime};
use std::path::Path;

/// The first node id the native reconciler hands out; below it are the ids of React's side.
const FIRST_NODE: u32 = 1 << 30;

/// What `platypus-build` wrote (`build/game.pbundle`), or a bare source file.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // css, assets and title are for the asset and window setup that follows
pub struct Bundle {
    /// The program: one TypeScript / TSX source text.
    pub source: String,
    /// The bundled stylesheet, relative to `build/`, if any.
    pub css: Option<String>,
    /// Assets the game names, relative to the game folder.
    pub assets: Vec<String>,
    pub title: Option<String>,
    /// What the program needs from the renderer (`renderer`, found at build time); a bundle
    /// without it (older, or a bare source file) counts as using the 3D world.
    pub tier: Tier,
    /// The engine names that set the tier (`uses`, for the log).
    pub uses: Vec<String>,
    /// Built with `platypus-build --add-ui-inspector` (`uiInspector`): the host answers the
    /// `UiInspector` calls. Off, the fast path.
    pub ui_inspector: bool,
}

impl Bundle {
    pub fn read(path: &Path) -> Result<Bundle> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        if path
            .extension()
            .is_some_and(|e| e == "pbundle" || e == "irbundle")
        {
            Bundle::parse(&text).with_context(|| path.display().to_string())
        } else {
            Ok(Bundle {
                source: text,
                tier: Tier::World3d,
                ..Default::default()
            })
        }
    }

    pub fn parse(json: &str) -> Result<Bundle> {
        let v: serde_json::Value = serde_json::from_str(json).context("not JSON")?;
        let format = v["format"].as_u64().unwrap_or(0);
        if format != 1 {
            bail!("unknown bundle format {format}");
        }
        let text = |k: &str| v[k].as_str().map(str::to_string);
        Ok(Bundle {
            source: text("source").ok_or_else(|| anyhow!("the bundle has no `source`"))?,
            css: text("css"),
            assets: v["assets"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            title: text("title"),
            tier: v["renderer"]
                .as_str()
                .and_then(Tier::parse)
                .unwrap_or(Tier::World3d),
            uses: v["uses"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            ui_inspector: v["uiInspector"].as_bool().unwrap_or(false),
        })
    }
}

/// The bundle name `platypus-build` wrote before the rename to Platypus (still read).
pub const LEGACY_BUNDLE: &str = "game.irbundle";

/// Is this path a game the host runs (a bundle, or TypeScript to compile)? `.irbundle` is the name
/// from before the rename to Platypus.
pub fn is_native(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e == "pbundle" || e == "irbundle" || e == "ts" || e == "tsx")
}

/// What `platypus-build dev` wrote to `build/.platypus-reload` for a failed build (`{"seq":N,"ok":false,
/// "error":".."}`); `None` when the last build worked or there is no such file.
pub fn failed_build(bundle: &Path) -> Option<String> {
    let text = std::fs::read_to_string(bundle.with_file_name(".platypus-reload")).ok()?;
    parse_reload_note(&text)
}

fn parse_reload_note(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    (v["ok"] == serde_json::Value::Bool(false))
        .then(|| v["error"].as_str().unwrap_or("unknown error").to_string())
}

/// The manifests a game is compiled against: every engine namespace and the UI bridge.
fn manifests() -> Vec<String> {
    let mut all = platypus_engine_api::manifests();
    all.push(platypus_scene::api::scene_api::MANIFEST.to_json());
    all.extend(platypus_scenes::manifests());
    all.extend(platypus_physics_api::manifests());
    all.extend(platypus_render_api::manifests());
    all.push(platypus_ts_ui::manifest_json());
    all.push(platypus_ts_ui::root_manifest_json());
    all
}

extern "C" fn console_hook(level: i32, text: *const u8, len: usize) {
    // SAFETY: the runtime passes a live UTF-8 buffer of `len` bytes.
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(text, len)) };
    match level {
        3 => tracing::warn!(target: "script", "{s}"),
        4 => tracing::error!(target: "script", "{s}"),
        2 => tracing::debug!(target: "script", "{s}"),
        _ => tracing::info!(target: "script", "{s}"),
    }
}

/// `PLATYPUS_TS_GC=stress` collects at every allocation of the game's code and poisons freed memory,
/// so a missing root crashes at once instead of now and then; `off` never collects on its own.
/// The same switch as the platypus_ts tests (`platypus_ts_tests::gc_mode_from_env`), for chasing memory
/// bugs in a running game.
fn gc_mode_from_env() {
    match std::env::var("PLATYPUS_TS_GC").as_deref() {
        Ok("stress") => platypus_ts_rt::ts_rt_gc_stress(true),
        Ok("off") => platypus_ts_rt::ts_rt_gc_enable(false),
        _ => {}
    }
}

/// Registers the scene-file kinds of every engine crate, once per thread.
fn register_kinds() {
    platypus_scenes::register_engine_kinds();
    platypus_render_api::register_kinds();
    platypus_engine_api::audio_components::register_kinds();
    platypus_physics_api::register_kinds();
}

/// A compiled game in the running app.
pub struct Native {
    game: Game,
    host: NativeHost,
    rt: Runtime,
    screen_root: RootId,
    /// The UI inspector's overlay: a second root on the screen, made when it first mounts.
    inspector_root: Option<RootId>,
    /// How many games this app compiled (struct headers of each get their own range).
    generation: u32,
    /// Components whose hook list changed in a reload: they start over, the others keep state.
    remount: Option<Vec<u32>>,
    /// `createRoot(panel).render(..)` calls: the panel's handle and its root element, for the
    /// app (each panel has its own tree and runtime).
    panel_mounts: Vec<(u64, platypus_ui_native::Element)>,
    /// `createRoot(win).render(..)` calls for windows other than the main one: the window's handle
    /// and its root element, for the app (each window has its own tree and runtime).
    window_mounts: Vec<(u64, platypus_ui_native::Element)>,
    /// Windows without a key that the last reload asks the app to close.
    reload_closes: Vec<u32>,
    /// Sets the engine state the program reads at start (`Application`, quality levels, layers):
    /// runs before the program's top level, at load and again after a reload's resets.
    env: Box<dyn Fn()>,
    /// Every module of the program has finished evaluating: the registered classes have started.
    started: bool,
    /// The message of a startup that failed (a rejected top-level `await`): the game never starts.
    startup_failed: Option<String>,
    /// When the startup began, for the warning about a module that stays pending.
    startup_since: std::time::Instant,
    startup_warned: bool,
    /// How long a module may stay pending before the log says so (ten seconds).
    warn_after: std::time::Duration,
    /// A reload whose program is still starting: what is put back once it has started.
    deferred_reload: Option<DeferredReload>,
    /// The scene to load once the program has started (`platypus.startScene`).
    pending_start_scene: Option<String>,
    /// The game folder: scene files the program loads come from here.
    root: Option<std::path::PathBuf>,
    #[allow(dead_code)]
    pub bundle: Bundle,
}

/// What a hot reload puts back after the new program's startup finishes.
struct DeferredReload {
    saved: Vec<String>,
    active: usize,
    old_classes: Vec<&'static str>,
}

fn active(o: u64) -> bool {
    use platypus_api::Handle;
    platypus_scene::api::scene_api::active(platypus_scene::ObjectId::from_raw(o))
}

fn log_faults(faults: Vec<platypus_ts_rt::Fault>) {
    for f in faults {
        tracing::error!(target: "script", "{}.{} threw; the component is disabled", f.class, f.hook);
    }
}

impl Native {
    /// Compiles and starts the game. The starting components are one GameObject per registered
    /// class, named after it.
    #[cfg(test)]
    pub fn load(bundle: Bundle, scene: &mut Scene, ui: &mut UiTree) -> Result<Native> {
        Self::load_with(bundle, scene, ui, Box::new(|| {}))
    }

    /// [`Native::load`] with `env` run before the program's top level (and before it again after
    /// every reload).
    pub fn load_with(
        bundle: Bundle,
        scene: &mut Scene,
        ui: &mut UiTree,
        env: Box<dyn Fn()>,
    ) -> Result<Native> {
        let generation = 0;
        platypus_ts_rt::ts_rt_set_console(Some(console_hook));
        // Timers and promise jobs keep the event domain (window family) that made them.
        platypus_ts_rt::ts_rt_set_domain_hooks(
            Some(platypus_engine_api::window::runtime_current_domain),
            Some(platypus_engine_api::window::runtime_enter_domain),
            Some(platypus_engine_api::window::runtime_leave_domain),
        );
        register_kinds();
        gc_mode_from_env();
        let mut bundle = bundle;
        let game = Self::compile(&mut bundle, generation)?;
        let mut lap = platypus_ts::timing::Lap::start();
        let mut rt = Runtime::new(FIRST_NODE);
        let screen_root = rt.create_root(platypus_ui::ROOT);
        env();
        let host = {
            let _scope = platypus_scene::api::enter(scene);
            if crate::debugger::take_first_statement_break() {
                platypus_ts::debug::pause();
                tracing::info!(
                    "debugger: stopping at the program's first statement (--inspect-brk)"
                );
            }
            game.init().map_err(|e| anyhow!("{e}"))?;
            NativeHost::new(game.classes().map_err(|e| anyhow!("{e}"))?)
        };
        lap.mark("program top level");
        let mut n = Native {
            game,
            host,
            rt,
            screen_root,
            inspector_root: None,
            generation,
            remount: None,
            panel_mounts: Vec::new(),
            window_mounts: Vec::new(),
            reload_closes: Vec::new(),
            env,
            root: None,
            started: false,
            startup_failed: None,
            startup_since: std::time::Instant::now(),
            startup_warned: false,
            warn_after: std::time::Duration::from_secs(10),
            deferred_reload: None,
            pending_start_scene: None,
            bundle,
        };
        // Spawns the starting components now when the program has nothing to wait for.
        n.after_script(scene, ui);
        lap.mark("starting components (onAwake, onStart)");
        tracing::info!(classes = ?n.host.class_names(), "compiled and started the game in {:.0} ms ({:.0} lower, {:.0} codegen)",
            (n.game.lower_time + n.game.codegen_time).as_secs_f64() * 1000.0,
            n.game.lower_time.as_secs_f64() * 1000.0,
            n.game.codegen_time.as_secs_f64() * 1000.0);
        Ok(n)
    }

    /// Compiles the bundle's program, then lets its source text go: nothing reads it again, and
    /// a game that bundles documents holds megabytes of it (md-reader: 2.4 MB).
    fn compile(bundle: &mut Bundle, generation: u32) -> Result<Game> {
        let mut lap = platypus_ts::timing::Lap::start();
        let manifests = manifests();
        lap.mark("engine manifests (JSON)");
        let game = compile_and_load(
            &bundle.source,
            &JitOptions {
                prefix: "g_",
                // Programs of different generations share one heap: their headers must differ.
                struct_base: generation * 100_000,
                manifests: &manifests,
                symbols: platypus_ts_ui::symbols(),
                debug: super::debugger::enabled(),
                ..Default::default()
            },
        )
        .map_err(|e| anyhow!("compiling the game: {e}"))?;
        super::crash::note_source_hash(platypus_ts::modules::source_hash(&bundle.source));
        bundle.source = String::new();
        // So a crash in the compiled code can be named (`crash.rs`): every function, local ones
        // too, or a fault in one before the first exported function goes unnamed and unreported.
        super::crash::note_jit_functions(game.code_symbols().into_iter().map(|(sym, a)| {
            let name = sym
                .strip_prefix("ts_")
                .or_else(|| sym.strip_prefix("platypus_ts_local_"))
                .map_or_else(|| sym.clone(), str::to_string);
            (name, a)
        }));
        super::crash::note_jit_lines(game.line_table());
        // A reload moves lines: the debugger's breakpoints are placed again.
        super::debugger::resolve_breakpoints();
        Ok(game)
    }

    /// Has every module finished evaluating, so the game runs?
    #[allow(dead_code)]
    pub fn started(&self) -> bool {
        self.started
    }

    /// Why the game did not start: a module's top-level `await` was rejected.
    #[allow(dead_code)]
    pub fn startup_error(&self) -> Option<&str> {
        self.startup_failed.as_deref()
    }

    /// Changes how long a module may stay pending before the log warns about it.
    #[allow(dead_code)]
    pub fn set_startup_warning(&mut self, after: std::time::Duration) {
        self.warn_after = after;
    }

    /// Looks at how the program's startup stands and, when every module has finished, starts the
    /// game: the registered classes get their objects (in the order of the `register(..)` calls)
    /// and a reload puts the saved world back. A failed startup is logged once.
    fn advance_startup(&mut self, scene: &mut Scene) {
        if self.started || self.startup_failed.is_some() {
            return;
        }
        match startup_state() {
            StartupState::Done => {
                self.started = true;
                match self.deferred_reload.take() {
                    Some(d) => self.restore_world(scene, d),
                    None => {
                        let classes: Vec<&'static str> = self
                            .host
                            .registered_class_names()
                            .into_iter()
                            .filter(|(_, spawn)| *spawn)
                            .map(|(c, _)| c)
                            .collect();
                        self.spawn_classes(scene, &classes);
                    }
                }
                if let Some(path) = self.pending_start_scene.take() {
                    let _scope = platypus_scene::api::enter(scene);
                    platypus_scenes::request_load(&path, false);
                }
            }
            StartupState::Failed { module, message } => {
                let text = format!(
                    "the game did not start: module `{module}` failed while evaluating: {message}"
                );
                tracing::error!(target: "script", "{text}");
                self.startup_failed = Some(text);
            }
            StartupState::Pending(modules) => {
                if !self.startup_warned && self.startup_since.elapsed() >= self.warn_after {
                    self.startup_warned = true;
                    tracing::warn!(
                        target: "script",
                        "module `{}` is still evaluating after {:.0} s (a top-level await has not finished); the game has not started",
                        modules[0],
                        self.warn_after.as_secs_f64()
                    );
                }
            }
        }
    }

    fn spawn_classes(&mut self, scene: &mut Scene, classes: &[&'static str]) {
        use platypus_api::Handle;
        let _scope = platypus_scene::api::enter(scene);
        for &class in classes {
            let go = platypus_scene::ObjectId::create();
            go.set_name(class);
            if let Err(e) = self.host.spawn(class, Some(go.to_raw())) {
                tracing::error!(target: "script", "{e}");
            }
        }
    }

    /// A text asset (scene, prefab) through the asset file system.
    fn read_text(&self, path: &str) -> Result<String, String> {
        match self.root.as_deref() {
            Some(root) => platypus_core::read_asset_string(root, path).map_err(|e| e.to_string()),
            None => Err("no asset folder".to_string()),
        }
    }

    /// Scene files the program asked for (`SceneManager.load`): read from the game folder.
    /// Runs with the scene current.
    fn load_requested_scenes(&mut self) {
        for (id, path) in platypus_scenes::take_load_requests() {
            let text = self.read_text(&path);
            let result = match text {
                Ok(text) => platypus_scenes::finish_load(id, &text),
                Err(e) => {
                    // Drops the request; `loadSceneAsync` rejects.
                    platypus_scenes::fail_load(id, &e);
                    Err(e)
                }
            };
            if let Err(e) = result {
                tracing::error!(target: "script", "loading scene `{path}`: {e}");
            }
            for w in platypus_scenes::take_warnings() {
                tracing::warn!(target: "script", "scene `{path}`: {w}");
            }
        }
        for (id, path) in platypus_scenes::take_prefab_requests() {
            let text = self.read_text(&path);
            let result = match &text {
                Ok(text) => platypus_scenes::finish_prefab(id, Ok(text)),
                Err(e) => platypus_scenes::finish_prefab(id, Err(e)),
            };
            if let Err(e) = result {
                tracing::error!(target: "script", "loading prefab `{path}`: {e}");
            }
        }
    }

    /// The game folder, for scene files.
    pub fn set_root(&mut self, root: &Path) {
        self.root = Some(root.to_path_buf());
    }

    /// Loads the manifest's `platypus.startScene` (a single-mode load, like the scripts' own).
    pub fn start_scene(&mut self, scene: &mut Scene, ui: &mut UiTree, path: &str) {
        if !self.started {
            // After the last module has finished (`advance_startup`).
            self.pending_start_scene = Some(path.to_string());
            return;
        }
        self.with_scene(scene, ui, || {
            platypus_scenes::request_load(path, false);
        });
    }

    /// Objects the program destroyed: components on them and the engine's data for them go too.
    /// Runs with the scene current.
    fn pump_destroyed(&mut self) {
        let gone = platypus_engine_api::objects::process_destroyed();
        self.objects_destroyed(&gone);
    }

    fn objects_destroyed(&mut self, gone: &[platypus_scene::ObjectId]) {
        use platypus_api::Handle;
        if gone.is_empty() {
            return;
        }
        let raws: Vec<u64> = gone.iter().map(|o| o.to_raw()).collect();
        self.host.destroy_on_objects(&raws);
        platypus_render_api::destroyed(gone);
        for o in gone {
            platypus_physics_api::object_destroyed(*o);
        }
        log_faults(self.host.settle());
    }

    /// The world panel roots the program mounted since the last call.
    pub fn take_panel_mounts(&mut self) -> Vec<(u64, platypus_ui_native::Element)> {
        std::mem::take(&mut self.panel_mounts)
    }

    /// The windows a reload left without an owner (no key): the app closes them.
    pub fn take_reload_closes(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.reload_closes)
    }

    /// The window roots the program mounted since the last call (window handle, root element).
    pub fn take_window_mounts(&mut self) -> Vec<(u64, platypus_ui_native::Element)> {
        std::mem::take(&mut self.window_mounts)
    }

    /// What follows any call into the program: mounts, queued component work, microtasks.
    pub fn after_script(&mut self, scene: &mut Scene, ui: &mut UiTree) {
        // A program with nothing to wait for starts here; one that waits starts when the microtasks
        // (the continuation of an `await`) have finished its last module.
        self.advance_startup(scene);
        {
            let _scope = platypus_scene::api::enter(scene);
            ts_rt_microtasks_run();
        }
        self.advance_startup(scene);
        let _scope = platypus_scene::api::enter(scene);
        if self.started {
            log_faults(self.host.settle());
        }
        self.pump_destroyed();
        self.load_requested_scenes();
        // Scene events reach the program's listeners once the world is consistent.
        platypus_scenes::dispatch();
        // A `loadSceneAsync` promise settled by that dispatch continues in this tick.
        ts_rt_microtasks_run();
        if self.started {
            log_faults(self.host.settle());
        }
        // `createRoot(win)`: the main window is the screen; other windows get their root from the app.
        let mut mounts: Vec<(u64, u64)> = platypus_ts_ui::take_mounts();
        for (window, handle) in platypus_ts_ui::take_window_mounts() {
            let w = platypus_engine_api::window::WindowRef(window as u32);
            if !platypus_engine_api::window::can_set_up(w) {
                tracing::warn!(target: "script", "createRoot(window {window}): that window belongs to another family; ignored");
                continue;
            }
            if window == 1 {
                mounts.push((0, handle));
            } else {
                self.window_mounts
                    .push((window, platypus_ts_ui::element_from(handle)));
            }
        }
        for (target, handle) in mounts {
            tracing::debug!(target, handle, "mount");
            let element = platypus_ts_ui::element_from(handle);
            if target == 0 {
                let errors = match self.remount.take() {
                    // After a reload every component renders again with its new code.
                    Some(ids) => self.rt.refresh(self.screen_root, element, &ids, ui),
                    None => self.rt.render(self.screen_root, element, ui),
                };
                for e in errors {
                    tracing::warn!(target: "ui", "{e}");
                }
            } else if target == platypus_ts_ui::INSPECTOR_TARGET {
                let root = *self
                    .inspector_root
                    .get_or_insert_with(|| self.rt.create_root(platypus_ui::ROOT));
                for e in self.rt.render(root, element, ui) {
                    tracing::warn!(target: "ui", "{e}");
                }
            } else {
                self.panel_mounts.push((target, element));
            }
        }
        for e in self.rt.flush(ui) {
            tracing::warn!(target: "ui", "{e}");
        }
        for e in platypus_ts_ui::take_errors() {
            tracing::error!(target: "script", "{e}");
        }
    }

    /// One fixed tick: `onFixedUpdate` for every component, then the physics world steps and
    /// contact hooks run.
    pub fn fixed_tick(&mut self, scene: &mut Scene, ui: &mut UiTree, dt: f64) {
        if self.started {
            let _scope = platypus_scene::api::enter(scene);
            log_faults(self.host.fixed_update(dt, &active));
            platypus_physics_api::step();
            log_faults(self.host.dispatch_physics(&active));
        }
        self.after_script(scene, ui);
    }

    /// One frame: the game clock, timers, then `onUpdate` and `onLateUpdate`.
    pub fn frame(&mut self, scene: &mut Scene, ui: &mut UiTree, dt: f64, now_ms: f64) {
        {
            let _scope = platypus_scene::api::enter(scene);
            platypus_engine_api::time::advance(dt);
            ts_rt_timers_advance(now_ms);
            ts_rt_microtasks_run();
            if self.started {
                log_faults(self.host.update(dt, &active));
            }
        }
        self.after_script(scene, ui);
    }

    /// Last-moment changes before drawing (`onPreRender`).
    pub fn pre_render(&mut self, scene: &mut Scene, ui: &mut UiTree) {
        {
            let _scope = platypus_scene::api::enter(scene);
            if self.started {
                log_faults(self.host.pre_render(&active));
            }
            // Emitters and the listener follow their objects.
            platypus_engine_api::audio_components::pre_render();
        }
        self.after_script(scene, ui);
    }

    /// Runs one UI event through the handlers and says whether one prevented its default action
    /// (the interaction layer asks while it handles the input). Follow with [`Native::settle`].
    pub fn ui_event(&mut self, scene: &mut Scene, e: &UiEvent) -> bool {
        let Some(mut ev) = to_event(e) else {
            return false;
        };
        set_target_kind(self.rt.host_kind(ev.target));
        let _scope = platypus_scene::api::enter(scene);
        self.rt.dispatch_handlers(&mut ev)
    }

    /// Commits what the event handlers changed and runs what they queued. Components render and
    /// effects run here, so the scene is current (`new GameObject`, `GameObject.roots`).
    pub fn settle(&mut self, scene: &mut Scene, ui: &mut UiTree) {
        let errors = {
            let _scope = platypus_scene::api::enter(scene);
            self.rt.settle(ui)
        };
        for e in errors {
            tracing::warn!(target: "ui", "{e}");
        }
        self.after_script(scene, ui);
    }

    /// UI events delivered after the fact (tests; the app asks [`Native::ui_event`] while its
    /// interaction layer handles the input).
    #[allow(dead_code)]
    pub fn ui_events(&mut self, scene: &mut Scene, ui: &mut UiTree, events: &[UiEvent]) {
        for e in events {
            self.ui_event(scene, e);
            self.settle(scene, ui);
        }
    }

    /// An input event for the game's `input` API, in the argument form `__platypus_input` takes
    /// (`keydown code key repeat`, `mousemove x y`, `gamepadbutton id name value`, ...).
    pub fn game_input(
        &mut self,
        scene: &mut Scene,
        ui: &mut UiTree,
        args: &[platypus_core::Value],
    ) {
        {
            let _scope = platypus_scene::api::enter(scene);
            super::game_input::apply_native(args);
        }
        self.after_script(scene, ui);
    }

    /// Sounds the mixer finished: `onEnded` callbacks and `AudioSource`s hear about it.
    pub fn audio_ended(
        &mut self,
        scene: &mut Scene,
        ui: &mut UiTree,
        ids: &[platypus_engine_api::audio::PlaybackRef],
    ) {
        {
            let _scope = platypus_scene::api::enter(scene);
            platypus_engine_api::audio::ended(ids);
        }
        self.after_script(scene, ui);
    }

    /// Focus or pause changed: `Application.onFocusChanged` closures and the components'
    /// `onApplicationFocus` / `onApplicationPause`.
    pub fn focus_changed(
        &mut self,
        scene: &mut Scene,
        ui: &mut UiTree,
        focused: bool,
        paused: bool,
    ) {
        {
            let _scope = platypus_scene::api::enter(scene);
            let change = platypus_engine_api::app::set_focus(focused, paused);
            if change.focus_changed && self.started {
                log_faults(self.host.application_focus(focused, &active));
            }
            if change.pause_changed && self.started {
                log_faults(self.host.application_pause(paused, &active));
            }
        }
        self.after_script(scene, ui);
    }

    /// Runs `f` with the scene current, then what follows a call into the program.
    pub fn with_scene<R>(
        &mut self,
        scene: &mut Scene,
        ui: &mut UiTree,
        f: impl FnOnce() -> R,
    ) -> R {
        let r = {
            let _scope = platypus_scene::api::enter(scene);
            f()
        };
        self.after_script(scene, ui);
        r
    }

    /// The game closes: everything that holds closures of the program is dropped here, in an
    /// order that does not depend on the order thread-local destructors run in.
    pub fn shutdown(mut self) {
        drop(std::mem::replace(
            &mut self.host,
            NativeHost::new(Vec::new()),
        ));
        platypus_engine_api::input::reset();
        platypus_engine_api::assets::reset();
        platypus_engine_api::app::reset();
        platypus_engine_api::screen::reset();
        platypus_engine_api::audio::reset();
        platypus_engine_api::audio_components::reset();
        platypus_engine_api::spline::reset();
        platypus_engine_api::event_list::reset();
        platypus_engine_api::animation::reset();
        platypus_engine_api::camera::reset();
        platypus_render_api::reset();
        platypus_physics_api::reset();
        platypus_scenes::reset();
        platypus_engine_api::window::reset();
    }

    /// The class names of the running game.
    #[allow(dead_code)]
    pub fn classes(&self) -> Vec<&'static str> {
        self.host.class_names()
    }

    /// Puts the saved scenes back after a reload and gives the classes the new program registered
    /// that the old one did not their starting objects.
    fn restore_world(&mut self, scene: &mut Scene, d: DeferredReload) {
        let DeferredReload {
            saved,
            active,
            old_classes,
        } = d;
        if !saved.is_empty() {
            let mut gone = Vec::new();
            for r in scene.roots().to_vec() {
                gone.extend(scene.destroy(r));
            }
            let _scope = platypus_scene::api::enter(scene);
            {
                self.objects_destroyed(&gone);
                let mut handles = Vec::new();
                for (i, text) in saved.iter().enumerate() {
                    match platypus_scenes::load_scene(text, i > 0) {
                        Ok(h) => handles.push(h),
                        Err(e) => {
                            tracing::error!(target: "script", "reload: restoring a scene: {e}")
                        }
                    }
                }
                if let Some(h) = handles.get(active) {
                    platypus_scenes::set_active(*h);
                }
                for w in platypus_scenes::take_warnings() {
                    tracing::warn!(target: "script", "reload: {w}");
                }
            }
        }
        // Classes that are new in this program get their starting object.
        let fresh: Vec<&'static str> = self
            .host
            .registered_class_names()
            .into_iter()
            .filter(|(c, spawn)| *spawn && !old_classes.contains(c))
            .map(|(c, _)| c)
            .collect();
        self.spawn_classes(scene, &fresh);
    }

    /// Replaces the game with a newly compiled one, keeping the world. Fails (and keeps running
    /// the old game) when the new source does not compile.
    ///
    /// The scenes are saved as scene files (compiled components included, through their
    /// `@property` tables), the engine's per-thread state is reset (listeners hold closures of the
    /// old code), the new program runs its top level, and the scenes load again: components come
    /// back on their objects with their saved fields, and `onAwake` runs for them. Objects the
    /// new program's top level made are removed first, as the saved ones stand for them. The UI
    /// keeps its hook state (`Runtime::refresh`), except for components whose hook list changed.
    pub fn reload(&mut self, bundle: Bundle, scene: &mut Scene, ui: &mut UiTree) -> Result<()> {
        let generation = self.generation + 1;
        // Nothing is touched until this works.
        let mut bundle = bundle;
        let game = Self::compile(&mut bundle, generation)?;
        let old_sigs = parse_signatures(&signatures_of(&self.game));
        let old_classes: Vec<&'static str> = self
            .host
            .registered_class_names()
            .into_iter()
            .map(|(c, _)| c)
            .collect();
        let (saved, active) = {
            let _scope = platypus_scene::api::enter(scene);
            let n = platypus_scenes::scene_count();
            let saved: Vec<String> = (0..n)
                .filter_map(|i| platypus_scenes::serialize(platypus_scenes::scene_at(i)))
                .collect();
            let active =
                (0..n).position(|i| platypus_scenes::scene_at(i) == platypus_scenes::active());
            (saved, active.unwrap_or(0))
        };
        // The old code's window listeners go; windows with a key stay for the new code to claim.
        self.reload_closes = platypus_engine_api::window::reload_begin()
            .into_iter()
            .map(|w| w.0)
            .collect();
        // Tear down: the old components (no `onDestroy`: the program is being replaced), then
        // everything that holds closures or objects of the old code.
        drop(std::mem::replace(
            &mut self.host,
            NativeHost::new(Vec::new()),
        ));
        platypus_engine_api::input::reset();
        platypus_engine_api::assets::reset();
        platypus_engine_api::app::reset();
        platypus_engine_api::screen::reset();
        platypus_engine_api::audio::reset();
        platypus_engine_api::audio_components::reset();
        platypus_engine_api::spline::reset();
        platypus_engine_api::event_list::reset();
        platypus_engine_api::animation::reset();
        platypus_engine_api::camera::reset();
        platypus_render_api::reset();
        super::native_render::reconfigure();
        platypus_physics_api::reset();
        platypus_scenes::reset();
        // Timers and awaits of the old code would run it against the new world.
        platypus_ts_rt::ts_rt_events_clear();
        *scene = Scene::new();
        self.game = game;
        self.generation = generation;
        self.bundle = bundle;
        self.started = false;
        self.startup_failed = None;
        self.startup_since = std::time::Instant::now();
        self.startup_warned = false;
        self.deferred_reload = None;
        {
            let _scope = platypus_scene::api::enter(scene);
            platypus_engine_api::time::reset();
            (self.env)();
            self.game.init().map_err(|e| anyhow!("{e}"))?;
            let host = NativeHost::new(self.game.classes().map_err(|e| anyhow!("{e}"))?);
            // The stand-in's `Drop` would wipe the registry the new host just filled.
            std::mem::forget(std::mem::replace(&mut self.host, host));
        }
        // What the top level made is replaced by the saved world, once every module of the new
        // program has finished evaluating (its `register(..)` calls may come after an `await`).
        let d = DeferredReload {
            saved,
            active,
            old_classes,
        };
        if matches!(startup_state(), StartupState::Done) {
            self.started = true;
            self.restore_world(scene, d);
        } else {
            self.deferred_reload = Some(d);
        }
        // Components of the UI whose hooks changed start over.
        let new_sigs = parse_signatures(&signatures_of(&self.game));
        self.remount = Some(
            old_sigs
                .iter()
                .filter(|(id, hooks)| new_sigs.get(id).is_some_and(|n| n != *hooks))
                .map(|(id, _)| *id)
                .collect(),
        );
        self.after_script(scene, ui);
        self.remount = None;
        Ok(())
    }
}

thread_local! {
    static TARGET_KIND: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// The element kind of the target of the event about to be dispatched (what `e.target.type`
/// answers: the compiled side asks for the current event's target only).
pub fn set_target_kind(kind: Option<String>) {
    platypus_ts_ui::set_target_kind_resolver(|_| TARGET_KIND.with(|k| k.borrow().clone()));
    TARGET_KIND.with(|k| *k.borrow_mut() = kind.unwrap_or_default());
}

/// A UI event of the interaction layer as the native reconciler's event.
pub fn to_event(e: &UiEvent) -> Option<Event> {
    platypus_ui_native::event_of(e)
}

/// The program's `__uiSignatures()` text (`Name:ID:hook,hook;...`), empty when it has no UI.
fn signatures_of(game: &Game) -> String {
    let Some(f) = game.function("__uiSignatures") else {
        return String::new();
    };
    // SAFETY: the compiled function takes nothing and returns a live string.
    let p = unsafe {
        std::mem::transmute::<usize, unsafe extern "C" fn() -> *const platypus_ts_rt::Str>(f)()
    };
    if p.is_null() {
        return String::new();
    }
    String::from_utf16_lossy(&unsafe { platypus_ts_rt::Str::from_raw(p) }.to_utf16())
}

/// Component id to its hook list.
fn parse_signatures(text: &str) -> std::collections::HashMap<u32, String> {
    text.split(';')
        .filter_map(|e| {
            let mut parts = e.splitn(3, ':');
            let _name = parts.next()?;
            let id = parts.next()?.parse().ok()?;
            Some((id, parts.next().unwrap_or("").to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_ui::events::Modifiers;
    use platypus_ui::{EventKind, ROOT};

    fn bundle(src: &str) -> Bundle {
        Bundle {
            source: src.to_string(),
            ..Default::default()
        }
    }

    fn start(src: &str) -> (Native, Scene, UiTree) {
        let mut scene = Scene::new();
        let mut ui = UiTree::new(400.0, 300.0);
        let n = Native::load(bundle(src), &mut scene, &mut ui).expect("the game compiles");
        (n, scene, ui)
    }

    fn dump(tree: &UiTree, id: platypus_core::NodeId) -> String {
        let kind = tree.kind(id).unwrap_or("?");
        if kind == "#text" {
            return format!("text:{}", tree.text(id).unwrap_or(""));
        }
        let kids: Vec<String> = tree.children(id).iter().map(|c| dump(tree, *c)).collect();
        format!("{kind}({})", kids.join(","))
    }

    // M9-API-610
    #[test]
    fn a_bundle_is_json_with_a_format_number_and_a_source() {
        let b = Bundle::parse(
            r#"{"format":1,"title":"t","entry":"src/main.ts","source":"let a = 1;","css":"game.css","assets":["assets/x.png"]}"#,
        )
        .unwrap();
        assert_eq!(b.source, "let a = 1;");
        assert_eq!(b.css.as_deref(), Some("game.css"));
        assert_eq!(b.assets, ["assets/x.png"]);
        assert_eq!(b.tier, Tier::World3d, "no `renderer`: the 3D world");
        // M3-INS-04
        assert!(!b.ui_inspector, "no `uiInspector`: the fast path");
        let b = Bundle::parse(r#"{"format":1,"source":"","uiInspector":true}"#).unwrap();
        assert!(b.ui_inspector);
        let b =
            Bundle::parse(r#"{"format":1,"source":"","renderer":"world2d","uses":["TextMesh"]}"#)
                .unwrap();
        assert_eq!(
            (b.tier, b.uses.as_slice()),
            (Tier::World2d, &["TextMesh".to_string()][..])
        );
        assert!(Bundle::parse(r#"{"format":2,"source":""}"#).is_err());
        assert!(Bundle::parse(r#"{"format":1}"#).is_err());
        assert!(Bundle::parse("not json").is_err());
        assert!(is_native(Path::new("build/game.pbundle")));
        assert!(!is_native(Path::new("build/game.js")));
    }

    // M9-REN-01
    #[test]
    fn a_bundle_from_before_the_rename_is_still_native() {
        assert!(is_native(Path::new("build/game.irbundle")));
        let d = std::env::temp_dir().join(format!("platypus_legacy_bundle_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(LEGACY_BUNDLE);
        std::fs::write(&p, r#"{"format":1,"source":"let a = 1;"}"#).unwrap();
        assert_eq!(Bundle::read(&p).unwrap().source, "let a = 1;");
        std::fs::remove_dir_all(&d).ok();
    }

    // M9-API-611
    #[test]
    fn a_compiled_component_gets_an_object_and_a_frame_every_update() {
        let (mut n, mut scene, mut ui) = start(
            "class Mover extends Component {\n\
               onUpdate(dt: number): void {\n\
                 const t = this.gameObject!.transform;\n\
                 const p = t.localPosition;\n\
                 t.localPosition = new Vector3(p.x + dt, p.y, p.z);\n\
               }\n\
             }\n\
             register(Mover);",
        );
        assert_eq!(n.classes(), ["Mover"]);
        let go = scene.find("Mover").expect("the starting object");
        for i in 0..3 {
            n.frame(&mut scene, &mut ui, 0.5, 16.0 * (i + 1) as f64);
        }
        assert_eq!(scene.local(go).unwrap().position.x, 1.5);
    }

    // M9-API-612
    #[test]
    fn timers_run_from_the_frames_clock() {
        let (mut n, mut scene, mut ui) = start(
            "class Late extends Component {\n\
               onStart(): void {\n\
                 setTimeout(() => {\n\
                   this.gameObject!.transform.localPosition = new Vector3(7, 0, 0);\n\
                 }, 100);\n\
               }\n\
             }\n\
             register(Late);",
        );
        let go = scene.find("Late").unwrap();
        n.frame(&mut scene, &mut ui, 0.016, 50.0);
        assert_eq!(scene.local(go).unwrap().position.x, 0.0);
        n.frame(&mut scene, &mut ui, 0.016, 150.0);
        assert_eq!(scene.local(go).unwrap().position.x, 7.0);
    }

    // M9-API-613
    #[test]
    fn a_mounted_root_is_drawn_and_a_click_reaches_its_handler() {
        let (mut n, mut scene, mut ui) = start(
            "function Counter(props: { start: number }): UiElement {\n\
               const [n, setN] = useState(props.start);\n\
               return (\n\
                 <box style={{ width: 100 }} onClick={() => setN(n + 1)}>\n\
                   <text>{\"clicks \" + n}</text>\n\
                 </box>\n\
               );\n\
             }\n\
             function boot(): number {\n\
               UiRoot.mount(0, <Counter start={2} />);\n\
               return 0;\n\
             }\n\
             const booted = boot();",
        );
        assert_eq!(dump(&ui, ROOT), "root(box(text(text:clicks 2)))");
        let target = ui.children(ROOT)[0];
        let click = UiEvent::new(EventKind::Click, vec![target], Modifiers::default());
        n.ui_events(&mut scene, &mut ui, &[click.clone(), click]);
        assert_eq!(dump(&ui, ROOT), "root(box(text(text:clicks 4)))");
    }

    // M9-API-614
    #[test]
    fn a_program_that_does_not_compile_is_an_error_not_a_panic() {
        let mut scene = Scene::new();
        let mut ui = UiTree::new(100.0, 100.0);
        let e = Native::load(bundle("let x: number = ;"), &mut scene, &mut ui).err();
        assert!(e.is_some_and(|e| e.to_string().contains("compiling the game")));
    }

    const RELOAD_GAME: &str = "class Counter extends Component {\n\
           @property count = 0;\n\
           onUpdate(dt: number): void {\n\
             this.count += STEP;\n\
             this.gameObject!.transform.localPosition = new Vector3(this.count, 0, 0);\n\
           }\n\
         }\n\
         register(Counter);\n\
         function Label(): UiElement {\n\
           const [n, setN] = useState(0);\n\
           return (\n\
             <box style={{ width: 100 }} onClick={() => setN(n + 1)}>\n\
               <text>{\"WORD\" + n}</text>\n\
             </box>\n\
           );\n\
         }\n\
         function boot(): number {\n\
           UiRoot.mount(0, <Label />);\n\
           return 0;\n\
         }\n\
         const booted = boot();";

    // M9-API-615
    #[test]
    fn a_reload_keeps_component_fields_ui_state_and_the_objects() {
        let v1 = RELOAD_GAME.replace("STEP", "1").replace("WORD", "n");
        let v2 = RELOAD_GAME.replace("STEP", "10").replace("WORD", "m");
        let (mut n, mut scene, mut ui) = start(&v1);
        for i in 0..3 {
            n.frame(&mut scene, &mut ui, 0.016, 16.0 * (i + 1) as f64);
        }
        let go = scene.find("Counter").unwrap();
        assert_eq!(scene.local(go).unwrap().position.x, 3.0);
        let target = ui.children(ROOT)[0];
        let click = UiEvent::new(EventKind::Click, vec![target], Modifiers::default());
        n.ui_events(&mut scene, &mut ui, &[click.clone(), click]);
        assert_eq!(dump(&ui, ROOT), "root(box(text(text:n2)))");
        let objects = scene.len();

        n.reload(bundle(&v2), &mut scene, &mut ui).expect("reloads");
        assert_eq!(scene.len(), objects, "no object was added or lost");
        // The UI is the new code's, with the old hook state.
        assert_eq!(dump(&ui, ROOT), "root(box(text(text:m2)))");
        n.frame(&mut scene, &mut ui, 0.016, 100.0);
        let go = scene.find("Counter").unwrap();
        // count 3 was saved; the new code adds 10.
        assert_eq!(scene.local(go).unwrap().position.x, 13.0);
    }

    // M9-API-617
    #[test]
    fn a_failed_build_note_names_its_error() {
        assert_eq!(
            parse_reload_note(r#"{"seq":3,"ok":false,"error":"line 4: nope"}"#).as_deref(),
            Some("line 4: nope")
        );
        assert_eq!(parse_reload_note(r#"{"seq":4,"ok":true}"#), None);
        assert_eq!(parse_reload_note("garbage"), None);
    }

    // M9-API-616
    #[test]
    fn a_reload_that_does_not_compile_keeps_the_old_game_running() {
        let v1 = RELOAD_GAME.replace("STEP", "1").replace("WORD", "n");
        let (mut n, mut scene, mut ui) = start(&v1);
        let e = n.reload(bundle("let x: number = ;"), &mut scene, &mut ui);
        assert!(e.is_err());
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        let go = scene.find("Counter").unwrap();
        assert_eq!(scene.local(go).unwrap().position.x, 1.0);
        assert_eq!(dump(&ui, ROOT), "root(box(text(text:n0)))");
    }

    // M9-API-800
    #[test]
    fn keys_reach_the_input_state_and_the_listeners_of_compiled_code() {
        let (mut n, mut scene, mut ui) = start(
            "class Pad extends Component {
               hits = 0;
               onStart(): void {
                 input.onKeyDown((code: string, repeat: boolean) => { this.hits += 1; });
               }
               onUpdate(dt: number): void {
                 let x = this.hits;
                 if (input.getKeyDown(\"Space\")) x += 10;
                 this.gameObject!.transform.localPosition = new Vector3(x, 0, 0);
               }
             }
             register(Pad);",
        );
        let go = scene.find("Pad").unwrap();
        n.frame(&mut scene, &mut ui, 0.016, 8.0); // onStart registers the listener
        let key = |k: &str| vec!["keydown".into(), k.into(), "".into(), false.into()];
        n.game_input(&mut scene, &mut ui, &key("Space"));
        n.game_input(&mut scene, &mut ui, &key("KeyA"));
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        assert_eq!(scene.local(go).unwrap().position.x, 12.0);
        n.frame(&mut scene, &mut ui, 0.016, 32.0);
        assert_eq!(
            scene.local(go).unwrap().position.x,
            2.0,
            "pressed for one frame"
        );
    }

    // M9-API-801
    #[test]
    fn prevent_default_in_a_handler_reaches_the_interaction_layer() {
        let (mut n, mut scene, mut ui) = start(
            "function App(): UiElement {
               return <box style={{ width: 100 }} onClick={(e: UiEvent) => { e.preventDefault(); }}><text>x</text></box>;
             }
             function boot(): number {
               UiRoot.mount(0, <App />);
               return 0;
             }
             const booted = boot();",
        );
        let target = ui.children(ROOT)[0];
        let click = UiEvent::new(EventKind::Click, vec![target], Modifiers::default());
        assert!(n.ui_event(&mut scene, &click));
        n.settle(&mut scene, &mut ui);
        let other = UiEvent::new(EventKind::DblClick, vec![target], Modifiers::default());
        assert!(!n.ui_event(&mut scene, &other));
    }

    // M9-API-802
    #[test]
    fn fixed_ticks_step_physics_and_move_the_objects() {
        let (mut n, mut scene, mut ui) = start(
            "class Drop extends Component {
               onStart(): void {
                 const ball = new GameObject(\"ball\", { position: new Vector3(0, 5, 0) });
                 ball.addComponent(SphereCollider, { radius: 0.5 });
                 ball.addComponent(Rigidbody, { mass: 1 });
               }
             }
             register(Drop);",
        );
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        for _ in 0..30 {
            n.fixed_tick(&mut scene, &mut ui, 1.0 / 60.0);
        }
        let ball = scene.find("ball").expect("the ball");
        let y = scene.world(ball).position.y;
        assert!(y < 4.9 && y > 0.0, "the ball fell: {y}");
    }

    // M9-API-803
    #[test]
    fn audio_commands_come_out_and_finished_sounds_run_on_ended() {
        use platypus_engine_api::audio::{AudioCommand, PlaybackRef};
        let (mut n, mut scene, mut ui) = start(
            "class Ding extends Component {
               onStart(): void {
                 const p = audio.load(\"sfx/hit.wav\").play();
                 p.onEnded(() => {
                   this.gameObject!.transform.localPosition = new Vector3(5, 0, 0);
                 });
               }
             }
             register(Ding);",
        );
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        let cmds = platypus_engine_api::audio::take_commands();
        assert!(matches!(cmds.first(), Some(AudioCommand::Load { .. })));
        let play = cmds
            .iter()
            .find_map(|c| match c {
                AudioCommand::Play { playback, .. } => Some(*playback),
                _ => None,
            })
            .expect("a play command");
        let go = scene.find("Ding").unwrap();
        assert_eq!(scene.local(go).unwrap().position.x, 0.0);
        n.audio_ended(&mut scene, &mut ui, &[play, PlaybackRef(99)]);
        assert_eq!(scene.local(go).unwrap().position.x, 5.0);
    }

    // M9-API-804
    #[test]
    fn screen_quality_and_focus_closures_run_and_spawn_false_classes_get_no_object() {
        let (mut n, mut scene, mut ui) = start(
            "class Watch extends Component {\n\
               onStart(): void {\n\
                 Screen.onChange(() => {\n\
                   this.gameObject!.transform.localPosition = new Vector3(Screen.width, 0, 0);\n\
                 });\n\
                 QualitySettings.onLevelChanged((level: number) => {\n\
                   this.gameObject!.transform.localScale = new Vector3(level + 1, 1, 1);\n\
                 });\n\
                 Application.onFocusChanged((focused: boolean) => {\n\
                   this.gameObject!.transform.localPosition = new Vector3(focused ? 1 : 2, 3, 0);\n\
                 });\n\
               }\n\
             }\n\
             class Quiet extends Component {}\n\
             register(Quiet, { spawn: false });\n\
             register(Watch);",
        );
        assert!(scene.find("Quiet").is_none(), "spawn: false");
        assert!(scene.find("Watch").is_some());
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        let go = scene.find("Watch").unwrap();
        n.with_scene(&mut scene, &mut ui, || {
            platypus_engine_api::screen::update(800, 600, 1.0)
        });
        assert_eq!(scene.local(go).unwrap().position.x, 800.0);
        n.with_scene(&mut scene, &mut ui, || {
            platypus_engine_api::screen::quality_api::set_quality_level(2.0)
        });
        assert_eq!(scene.local(go).unwrap().scale.x, 3.0);
        n.focus_changed(&mut scene, &mut ui, false, true);
        assert_eq!(scene.local(go).unwrap().position.x, 2.0);
    }

    // M9-API-805
    #[test]
    fn spawn_false_registers_without_a_starting_object_and_the_order_is_the_call_order() {
        let (_n, scene, _ui) = start(
            "class A extends Component {}\n\
             class B extends Component {}\n\
             class C extends Component {}\n\
             register({ B, A }, { spawn: false });\n\
             register(C);",
        );
        assert!(scene.find("A").is_none() && scene.find("B").is_none());
        assert!(scene.find("C").is_some());
        assert_eq!(
            platypus_ts_rt::registered_classes(),
            [
                ("B".to_string(), false),
                ("A".to_string(), false),
                ("C".to_string(), true)
            ]
        );
    }

    // M9-API-1615
    #[test]
    fn a_pending_top_level_await_shows_the_loading_ui_and_holds_back_the_components() {
        let (mut n, mut scene, mut ui) = start(
            "class Boss extends Component {
               onUpdate(dt: number): void {
                 this.gameObject!.transform.localPosition = new Vector3(5, 0, 0);
               }
             }
             UiRoot.mount(0, <text>loading</text>);
             await new Promise<void>((r) => setTimeout(r, 50));
             register(Boss);
",
        );
        // The UI mounted before the `await` is on screen; nothing else has started.
        assert_eq!(dump(&ui, ROOT), "root(text(text:loading))");
        assert!(!n.started());
        assert!(scene.find("Boss").is_none());
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        n.frame(&mut scene, &mut ui, 0.016, 32.0);
        assert!(!n.started() && scene.find("Boss").is_none());
        // The timer fires: the module finishes, the class is registered and spawned, and
        // its first update runs on the following frame.
        n.frame(&mut scene, &mut ui, 0.016, 60.0);
        assert!(n.started());
        let go = scene.find("Boss").expect("spawned after the startup");
        n.frame(&mut scene, &mut ui, 0.016, 80.0);
        assert_eq!(scene.local(go).unwrap().position.x, 5.0);
    }

    // M9-API-1616
    #[test]
    fn a_rejected_top_level_await_stops_the_startup_with_the_module_in_the_message() {
        let (mut n, mut scene, mut ui) = start(
            "class Boss extends Component {}
             register(Boss);
             await Promise.reject(new Error(\"no network\"));
",
        );
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        assert!(!n.started());
        let e = n.startup_error().expect("the startup failed");
        assert!(e.contains("no network") && e.contains("main"), "{e}");
        assert!(scene.find("Boss").is_none(), "nothing starts");
    }

    // M9-API-1617
    #[test]
    fn a_scene_naming_a_class_that_never_registered_says_so() {
        let (mut n, mut scene, mut ui) = start(
            "class Registered extends Component {}
             class Forgotten extends Component {}
             register(Registered);
",
        );
        let text = r#"{"format":"platypus.scene","version":1,"name":"s","objects":[{"name":"o","components":[{"type":"Forgotten"}]}]}"#;
        let warnings = n.with_scene(&mut scene, &mut ui, || {
            platypus_scenes::load_scene(text, false).expect("the scene loads");
            platypus_scenes::take_warnings()
        });
        let all = warnings.join(
            "
",
        );
        assert!(
            all.contains("Forgotten") && all.contains("never registered"),
            "{all}"
        );
    }

    // M9-API-1618
    #[test]
    fn a_reload_into_a_program_that_awaits_restores_the_world_once_it_has_started() {
        let (mut n, mut scene, mut ui) = start(
            "class A extends Component {}
register(A);
",
        );
        assert!(scene.find("A").is_some());
        n.reload(
            bundle(
                "class A extends Component {}
class B extends Component {}
register(A);
                 await new Promise<void>((r) => setTimeout(r, 20));
register(B);
",
            ),
            &mut scene,
            &mut ui,
        )
        .unwrap();
        assert!(!n.started() && scene.find("A").is_none() && scene.find("B").is_none());
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        n.frame(&mut scene, &mut ui, 0.016, 40.0);
        assert!(n.started());
        assert!(scene.find("A").is_some(), "the saved world came back");
        assert!(
            scene.find("B").is_some(),
            "a class that is new gets its object"
        );
    }

    // M9-API-806
    #[test]
    fn scene_files_load_from_the_game_folder_and_assets_changed_runs_its_closure() {
        let dir =
            std::env::temp_dir().join(format!("platypus-native-scene-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("lvl.scene.json"),
            r#"{"format":"platypus.scene","version":1,"name":"Second","objects":[]}"#,
        )
        .unwrap();
        let (mut n, mut scene, mut ui) = start(
            "class Loader extends Component {\n\
               onStart(): void {\n\
                 SceneManager.onSceneLoaded((s: Scene, mode: string) => {\n\
                   this.gameObject!.transform.localPosition = new Vector3(mode === \"additive\" ? 4 : 5, 0, 0);\n\
                 });\n\
                 Assets.onChanged((paths: string) => {\n\
                   this.gameObject!.transform.localScale = new Vector3(paths.length, 1, 1);\n\
                 });\n\
                 SceneManager.loadSceneAsync(\"lvl.scene.json\", \"additive\");\n\
               }\n\
             }\n\
             register(Loader);",
        );
        n.set_root(&dir);
        let go = scene.find("Loader").unwrap();
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        assert_eq!(scene.local(go).unwrap().position.x, 4.0);
        n.with_scene(&mut scene, &mut ui, || {
            platypus_engine_api::assets::changed(["a.png".to_string()]);
        });
        assert_eq!(scene.local(go).unwrap().scale.x, 5.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // M9-API-1512
    #[test]
    fn load_scene_async_is_a_promise_that_resolves_with_the_scene_in_the_tick_it_loads() {
        let dir =
            std::env::temp_dir().join(format!("platypus-native-await-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("lvl.scene.json"),
            r#"{"format":"platypus.scene","version":1,"name":"Second","objects":[]}"#,
        )
        .unwrap();
        let (mut n, mut scene, mut ui) = start(
            "async function go(o: GameObject): Promise<void> {
               const s = await SceneManager.loadSceneAsync(\"lvl.scene.json\", \"additive\");
               o.transform.localPosition = new Vector3(s.name === \"Second\" ? 4 : 5, SceneManager.sceneCount, 0);
             }
             class Loader extends Component {
               onStart(): void {
                 go(this.gameObject!);
               }
             }
             register(Loader);",
        );
        n.set_root(&dir);
        let go = scene.find("Loader").unwrap();
        // The scene loads in the tick that asked for it, and the awaiting code runs in that tick.
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        assert_eq!(scene.local(go).unwrap().position.x, 4.0);
        assert_eq!(scene.local(go).unwrap().position.y, 2.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // M9-API-1513
    #[test]
    fn load_scene_async_rejects_with_an_error_for_a_missing_file_and_a_bad_one() {
        let dir =
            std::env::temp_dir().join(format!("platypus-native-reject-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.scene.json"), "not json").unwrap();
        let (mut n, mut scene, mut ui) = start(
            "async function go(o: GameObject): Promise<void> {
               let seen = 0;
               try { await SceneManager.loadSceneAsync(\"missing.scene.json\"); } catch (e) { seen += 1; }
               try { await SceneManager.loadSceneAsync(\"bad.scene.json\"); } catch (e) { seen += 10; }
               try { await SceneManager.loadSceneAsync(\"../outside.scene.json\"); } catch (e) { seen += 100; }
               o.transform.localPosition = new Vector3(seen, 0, 0);
             }
             class Loader extends Component {
               async onStart(): Promise<void> {
                 await go(this.gameObject!);
               }
             }
             register(Loader);",
        );
        n.set_root(&dir);
        let go = scene.find("Loader").unwrap();
        // Each failed read rejects in the tick that found it; the awaiting code continues there
        // and asks for the next file, which the following tick reads.
        for _ in 0..4 {
            n.frame(&mut scene, &mut ui, 0.016, 16.0);
        }
        assert_eq!(scene.local(go).unwrap().position.x, 111.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // M9-API-1514
    #[test]
    fn code_awaiting_in_a_destroyed_object_runs_on_without_touching_freed_memory() {
        // Garbage collection at every allocation, poisoned: a dangling pointer would show.
        platypus_ts_rt::ts_rt_gc_stress(true);
        let (mut n, mut scene, mut ui) = start(
            "class Probe extends Component {}
             class Victim extends Component {
               async onStart(): Promise<void> {
                 const me = this.gameObject!;
                 const probe = GameObject.findObjectOfType(Probe)!.gameObject!;
                 await Task.delay(30);
                 me.transform.localPosition = new Vector3(5, 0, 0);
                 probe.transform.localPosition = new Vector3(me.exists() ? 1 : 2, 0, 0);
               }
             }
             class Killer extends Component {
               onStart(): void {
                 const v = GameObject.findObjectOfType(Victim)!.gameObject!;
                 setTimeout(() => { v.destroy(); }, 10);
               }
             }
             register(Probe);
             register(Victim);
             register(Killer);",
        );
        let probe = scene.find("Probe").unwrap();
        for (i, now) in [0.0, 16.0, 32.0, 48.0, 64.0].into_iter().enumerate() {
            n.frame(&mut scene, &mut ui, 0.016, now);
            if i == 0 {
                assert!(scene.find("Victim").is_some());
            }
        }
        assert!(scene.find("Victim").is_none(), "destroyed while it awaited");
        // The code after the await ran, saw its object gone (`exists()`), and writing to the
        // dead object did nothing.
        assert_eq!(scene.local(probe).unwrap().position.x, 2.0);
        platypus_ts_rt::ts_rt_gc_stress(false);
    }

    // M9-API-1515
    #[test]
    fn an_async_event_handler_continues_after_its_await_and_the_ui_updates() {
        let (mut n, mut scene, mut ui) = start(
            "function Counter(): UiElement {
               const [n, setN] = useState(0);
               return (
                 <box style={{ width: 100 }} onClick={async () => {
                   await Promise.resolve();
                   await Task.delay(5);
                   setN(n + 10);
                 }}>
                   <text>{\"n=\" + n}</text>
                 </box>
               );
             }
             function boot(): number {
               UiRoot.mount(0, <Counter />);
               return 0;
             }
             const booted = boot();",
        );
        assert_eq!(dump(&ui, ROOT), "root(box(text(text:n=0)))");
        let target = ui.children(ROOT)[0];
        let click = UiEvent::new(EventKind::Click, vec![target], Modifiers::default());
        n.ui_events(&mut scene, &mut ui, &[click]);
        assert_eq!(
            dump(&ui, ROOT),
            "root(box(text(text:n=0)))",
            "still waiting"
        );
        n.frame(&mut scene, &mut ui, 0.016, 1.0);
        n.frame(&mut scene, &mut ui, 0.016, 20.0);
        assert_eq!(dump(&ui, ROOT), "root(box(text(text:n=10)))");
    }

    // M9-API-1516
    #[test]
    fn a_reload_drops_the_timers_and_awaits_of_the_old_code() {
        let game = "class Late extends Component {
               async onStart(): Promise<void> {
                 await Task.delay(100);
                 Application.targetFrameRate = RATE;
               }
             }
             register(Late);";
        let (mut n, mut scene, mut ui) = start(&game.replace("RATE", "77"));
        n.frame(&mut scene, &mut ui, 0.016, 0.0);
        n.frame(&mut scene, &mut ui, 0.016, 50.0);
        assert!(platypus_engine_api::app::take_requests().is_empty());
        n.reload(bundle(&game.replace("RATE", "88")), &mut scene, &mut ui)
            .expect("reloads");
        // The old await would set 77 at 100 ms; the new code starts its own wait at its first frame.
        n.frame(&mut scene, &mut ui, 0.016, 60.0);
        n.frame(&mut scene, &mut ui, 0.016, 120.0);
        n.frame(&mut scene, &mut ui, 0.016, 300.0);
        let rates: Vec<_> = platypus_engine_api::app::take_requests()
            .into_iter()
            .filter_map(|r| match r {
                platypus_engine_api::app::Request::TargetFrameRate(r) => Some(r),
                _ => None,
            })
            .collect();
        assert_eq!(rates, [88]);
    }

    // M9-API-1517
    #[test]
    fn prefab_load_is_a_promise_of_the_prefab_and_rejects_for_a_missing_file() {
        let dir =
            std::env::temp_dir().join(format!("platypus-native-prefab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("thing.prefab.json"),
            r#"{"format":"platypus.prefab","version":1,"name":"Thing","root":{"id":1,"name":"Made"}}"#,
        )
        .unwrap();
        let (mut n, mut scene, mut ui) = start(
            "async function go(): Promise<void> {
               const p = await Prefab.load(\"thing.prefab.json\");
               GameObject.instantiate(p, { position: new Vector3(3, 0, 0) });
               try {
                 await Prefab.load(\"nope.prefab.json\");
               } catch (e) {
                 new GameObject(\"Failed\");
               }
             }
             class Loader extends Component {
               onStart(): void {
                 go();
               }
             }
             register(Loader);",
        );
        n.set_root(&dir);
        for i in 0..4 {
            n.frame(&mut scene, &mut ui, 0.016, 16.0 * f64::from(i));
        }
        let made = scene.find("Made").expect("the prefab was instantiated");
        assert_eq!(scene.local(made).unwrap().position.x, 3.0);
        assert!(scene.find("Failed").is_some(), "the missing file rejected");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // M9-API-1518
    #[test]
    fn components_rendered_by_an_event_see_the_scene() {
        // The editor's Scene view makes its camera in a `useState` initializer when its tab is
        // clicked, and its App reads `GameObject.roots` while rendering a selection.
        let (mut n, mut scene, mut ui) = start(
            "function Panel(): UiElement {
               const [made] = useState(() => new GameObject(\"Made\"));
               useEffect(() => {
                 made.transform.localPosition = new Vector3(GameObject.roots.length, 0, 0);
               }, [made]);
               return <text>{\"roots=\" + GameObject.roots.length}</text>;
             }
             function App(): UiElement {
               const [open, setOpen] = useState(false);
               return <box style={{ width: 100 }} onClick={() => setOpen(true)}>{open ? <Panel /> : <text>closed</text>}</box>;
             }
             new GameObject(\"First\");
             function boot(): number {
               UiRoot.mount(0, <App />);
               return 0;
             }
             const booted = boot();",
        );
        let target = ui.children(ROOT)[0];
        let click = UiEvent::new(EventKind::Click, vec![target], Modifiers::default());
        n.ui_event(&mut scene, &click);
        n.settle(&mut scene, &mut ui);
        let made = scene
            .find("Made")
            .expect("made while the click's render ran");
        assert_eq!(dump(&ui, ROOT), "root(box(text(text:roots=2)))");
        assert_eq!(scene.local(made).unwrap().position.x, 2.0, "the effect too");
    }

    // M9-API-808
    #[test]
    fn only_registered_classes_start_and_an_event_target_knows_its_kind() {
        let (mut n, mut scene, mut ui) = start(
            "class Helper extends Component {\n\
               onStart(): void {\n\
                 this.gameObject!.transform.localPosition = new Vector3(9, 0, 0);\n\
               }\n\
             }\n\
             class Boss extends Component {\n\
               onStart(): void {\n\
                 this.gameObject!.addComponent(Helper);\n\
               }\n\
             }\n\
             register(Boss);\n\
             function App(): UiElement {\n\
               return <box style={{ width: 100 }} onClick={(e: UiEvent) => {\n\
                 if (e.target.type === \"box\") { UiRoot.mount(0, <text>box</text>); }\n\
               }}><text>x</text></box>;\n\
             }\n\
             function boot(): number {\n\
               UiRoot.mount(0, <App />);\n\
               return 0;\n\
             }\n\
             const booted = boot();",
        );
        assert!(
            scene.find("Helper").is_none(),
            "unregistered: no starting object"
        );
        n.frame(&mut scene, &mut ui, 0.016, 16.0);
        let go = scene.find("Boss").unwrap();
        assert_eq!(
            scene.local(go).unwrap().position.x,
            9.0,
            "addComponent works"
        );
        let target = ui.children(ROOT)[0];
        let click = UiEvent::new(EventKind::Click, vec![target], Modifiers::default());
        n.ui_events(&mut scene, &mut ui, &[click]);
        assert_eq!(dump(&ui, ROOT), "root(text(text:box))");
    }
}
