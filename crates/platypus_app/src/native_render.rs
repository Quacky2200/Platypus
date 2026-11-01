// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The app's side of rendering a compiled game: what [`render_sync::sync`] cannot do on its own
//! because it lives in the app. World panels (each has a UI tree, a native `Runtime` and a
//! texture), cloth, the fonts and stylesheets scripts add (`UiResources`), the cursor and the
//! window (`Cursor`, `Screen`), the quality settings from `package.json`, and what the world
//! renderer reports back (particles).

use super::*;
use platypus_core::Value;
use platypus_ui_native::{RootId, Runtime};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// The first node id the native reconciler hands out (the same as the screen's runtime).
const FIRST_NODE: u32 = 1 << 30;

#[derive(Default)]
struct State {
    /// One reconciler per world panel (a runtime serves one tree).
    runtimes: HashMap<u32, (Runtime, RootId)>,
    /// One reconciler per secondary window (by window handle), like the panels'.
    windows: HashMap<u32, (Runtime, RootId)>,
    /// Window roots mounted before their window was opened.
    window_pending: Vec<(u32, platypus_ui_native::Element)>,
    /// Panels a `WorldPanel` component exists for (the others only had a mount so far).
    live: HashSet<u32>,
    /// The `TextMesh` panels (ids from `render_sync::TEXT_BASE`).
    live_texts: HashSet<u32>,
    /// The `Screen` size last told to the game, and the display state last pushed.
    screen: Option<(u32, u32, u64)>,
    display: Option<String>,
    /// When the display state was last read (it asks the OS for every monitor's modes).
    display_checked: Option<std::time::Instant>,
    cursor_version: Option<u64>,
    root: Option<std::path::PathBuf>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// The window moved, resized or changed scale: read the display state next frame. The slow
/// re-read covers what the platform doesn't announce (monitors added or removed).
pub(super) fn display_dirty() {
    st(|s| s.display_checked = None);
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn num(n: f64) -> Value {
    Value::Number(n)
}

/// The game's quality levels (`package.json`), the player's choice and the layer names: before
/// the program runs, because its top level reads them. Again after a reload.
pub fn configure_engine(root: &std::path::Path) {
    st(|s| s.root = Some(root.to_path_buf()));
    let manifest = info::Manifest::load(root);
    platypus_engine_api::screen::configure_quality(&manifest.quality);
    platypus_engine_api::screen::apply_default_quality();
    // `platypus.layers`: names of the layers above the built-in ones.
    if let Some(layers) = manifest.layers.as_object() {
        platypus_engine_api::layers::set_game_layers(
            layers
                .iter()
                .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string()))),
        );
    }
}

/// The panels' reconcilers hold closures of the program: they are dropped while the program's
/// heap is alive (thread-local destructors run in no fixed order).
pub fn clear_state() {
    st(|s| *s = State::default());
}

/// Configures again after a reload reset the engine's state (before the new program runs).
pub fn reconfigure() {
    if let Some(root) = st(|s| s.root.clone()) {
        configure_engine(&root);
    }
}

/// Particle counts the world renderer reports (`[id, count, alive, ...]`): `onStopped` runs and
/// `stopAction: "destroy"` applies.
pub fn report_particles(changed: &[f64], scene: &mut platypus_scene::Scene) {
    if changed.is_empty() {
        return;
    }
    let _scope = platypus_scene::api::enter(scene);
    for c in changed.as_chunks::<3>().0 {
        platypus_render_api::report_particles(
            platypus_render_api::ParticleSystemRef(c[0] as u32),
            c[1] as u32,
            c[2] != 0.0,
        );
    }
}

impl App {
    /// Called before drawing each frame of a compiled game.
    pub(super) fn sync_native_render(&mut self) {
        let mut lap = crate::sync_profile::Lap::start();
        {
            // Culling animators ask what the last frame saw of their renderers.
            let roots = platypus_engine_api::animation::culled_roots();
            let mut sight = self.world_view.animation_sight(&self.scene, &roots);
            // `CameraFollow` and `CameraOrbit` move their cameras after every `onLateUpdate`.
            let _scope = platypus_scene::api::enter(&mut self.scene.scene);
            let dt = platypus_engine_api::time::state().delta_time;
            // `Animation` and `Animator` components pose their objects (events are not
            // delivered to components yet).
            platypus_engine_api::animation::update_culled(dt, None, &mut sight);
            // `SplineAnimate` components put their objects on their paths.
            platypus_engine_api::spline::update(dt);
            platypus_render_api::late_update_camera_follows(dt);
            platypus_render_api::late_update_camera_orbits(dt);
        }
        lap.mark("animation, splines, camera follow");
        let extras = render_sync::sync(&mut self.world_view, &mut self.scene, &self.asset_root);
        let mut lap = crate::sync_profile::Lap::start();
        self.apply_ui_resources();
        lap.mark("ui resources");
        self.sync_ui_inspector();
        lap.mark("ui inspector");
        self.apply_cursor_and_screen();
        lap.mark("cursor and screen");
        for (name, args) in extras.cloths {
            if let Err(e) = self
                .cloth
                .apply(&name, &args, &self.scene, &mut self.world_view)
            {
                tracing::warn!("cloth.{name}: {e}");
            }
        }
        lap.mark("cloth ops");
        self.sync_panels(extras.panels);
        lap.mark("panels");
        self.sync_texts(extras.texts);
        lap.mark("texts");
        self.sync_window_roots();
        lap.mark("window roots");
    }

    /// A compiled game reloaded: its registries start over, and so does what hangs on them.
    pub(super) fn reset_native_render(&mut self) {
        render_sync::reset();
        st(|s| *s = State::default());
        self.panels = panels::Panels::default();
        self.cloth = cloth_sync::ClothSystem::default();
        self.world_view = world_view::WorldViews::default();
        (self.pointer_on, self.keys_on) = (Surface::Screen, Surface::Screen);
        // Windows a reload kept start with empty trees: the new code mounts its roots again.
        for w in self.windows.map.values_mut() {
            w.tree.clear_script_nodes();
        }
    }

    /// `addStyleSheet`, `removeStyleSheet` and `loadFont` from the game, on the screen and every
    /// panel.
    fn apply_ui_resources(&mut self) {
        use platypus_engine_api::ui_resources::UiRequest;
        for request in platypus_engine_api::ui_resources::take_requests() {
            match request {
                UiRequest::AddStyleSheet { id, css } => {
                    self.stylesheet_op("addStyleSheet", &[num(f64::from(id)), Value::String(css)]);
                }
                UiRequest::RemoveStyleSheet { id } => {
                    self.stylesheet_op("removeStyleSheet", &[num(f64::from(id))]);
                }
                UiRequest::LoadFont {
                    family,
                    src,
                    weight,
                    style,
                } => {
                    self.load_font_op(&[
                        Value::String(family),
                        Value::String(src),
                        if weight > 0.0 {
                            num(weight)
                        } else {
                            Value::Null
                        },
                        if style.is_empty() {
                            Value::Null
                        } else {
                            Value::String(style)
                        },
                    ]);
                }
            }
        }
    }

    /// `Cursor` and `Screen`: what the game asked for, and what the window is now.
    fn apply_cursor_and_screen(&mut self) {
        let version = platypus_engine_api::cursor::version();
        if st(|s| s.cursor_version.replace(version)) != Some(version) {
            let c = platypus_engine_api::cursor::state();
            self.cursor_op("visible", &[Value::Bool(c.visible)]);
            self.cursor_op("lock", &[Value::String(c.lock_state.clone())]);
            match c.image {
                Some((src, hotspot)) => {
                    let mut args = vec![Value::String(src)];
                    if let Some(h) = hotspot {
                        args.push(Value::Array(vec![num(h[0]), num(h[1])]));
                    }
                    self.cursor_op("image", &args);
                }
                None => self.cursor_op("image", &[Value::Null]),
            }
        }
        for r in platypus_engine_api::screen::take_requests() {
            self.screen_op(&[
                Value::String(r.mode),
                num(f64::from(r.width)),
                num(f64::from(r.height)),
                num(r.refresh_rate),
            ]);
        }
        // What the window is now, for `Screen.resolutions`, `windowSize`, ...
        // Reading it asks the OS for every monitor's video modes (milliseconds with a few
        // monitors), so it is read when the window's size or scale changed (or it moved), else every 2 s.
        let (w, h, scale) = self.screen;
        let key = (
            w.round() as u32,
            h.round() as u32,
            f64::from(scale).to_bits(),
        );
        let due = st(|s| {
            let changed = s.screen != Some(key);
            let stale = s
                .display_checked
                .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(2));
            if changed || stale {
                s.display_checked = Some(std::time::Instant::now());
            }
            changed || stale
        });
        let window = self.main_window();
        let state = if due {
            display::state(window.as_deref(), &self.display)
        } else {
            serde_json::Value::Null
        };
        let text = state.to_string();
        if due && st(|s| s.display.replace(text.clone())).as_deref() != Some(&text) {
            let res = |v: &serde_json::Value| platypus_engine_api::screen::Resolution {
                width: v["width"].as_u64().unwrap_or(0) as u32,
                height: v["height"].as_u64().unwrap_or(0) as u32,
                refresh_rate: v["refreshRate"].as_f64().unwrap_or(0.0),
            };
            let resolutions = state["resolutions"]
                .as_array()
                .map(|a| a.iter().map(res).collect())
                .unwrap_or_default();
            let modes = state["supportedModes"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            platypus_engine_api::screen::set_display(
                state["mode"].as_str().unwrap_or("Windowed"),
                (
                    state["windowWidth"].as_u64().unwrap_or(1280) as u32,
                    state["windowHeight"].as_u64().unwrap_or(720) as u32,
                ),
                resolutions,
                res(&state["currentResolution"]),
                modes,
            );
        }
        // The size and scale `Screen` reports follow the window: `onChange` listeners run.
        if w > 0.0 && st(|s| s.screen.replace(key)) != Some(key) {
            let _scope = platypus_scene::api::enter(&mut self.scene.scene);
            platypus_engine_api::screen::update(key.0, key.1, f64::from(scale));
        }
    }

    /// `TextMesh` components: panels of one text, dropped with their component (`None`: none
    /// changed).
    fn sync_texts(&mut self, specs: Option<Vec<Vec<Value>>>) {
        let Some(specs) = specs else {
            return;
        };
        let (root, fonts, sheets) = (&self.asset_root, &self.font_sources, &self.sheet_sources);
        let mut ids = HashSet::new();
        for args in specs {
            if let Some(id) = args.first().and_then(Value::as_f64) {
                ids.insert(id as u32);
            }
            if let Err(e) = self
                .panels
                .apply_text(&args, || new_panel_tree(root, fonts, sheets))
            {
                tracing::warn!("text mesh: {e}");
            }
        }
        let gone: Vec<u32> = st(|s| {
            let gone = s
                .live_texts
                .iter()
                .filter(|id| !ids.contains(id))
                .copied()
                .collect();
            s.live_texts = ids;
            gone
        });
        for id in gone {
            self.panels.remove(id);
        }
    }

    /// World panels: the `WorldPanel` components' settings, new roots of `createRoot(panel)`,
    /// and every panel's pending state updates (`specs` is `None` when no `WorldPanel` changed).
    fn sync_panels(&mut self, specs: Option<Vec<Vec<Value>>>) {
        if let Some(specs) = specs {
            let (root, fonts, sheets) = (&self.asset_root, &self.font_sources, &self.sheet_sources);
            let mut ids = HashSet::new();
            for args in specs {
                if let Some(id) = args.first().and_then(Value::as_f64) {
                    ids.insert(id as u32);
                }
                if let Err(e) = self
                    .panels
                    .apply(&args, || new_panel_tree(root, fonts, sheets))
                {
                    tracing::warn!("world panel: {e}");
                }
            }
            // A panel whose component is gone goes with its tree.
            let gone: Vec<u32> = st(|s| {
                let gone = s
                    .live
                    .iter()
                    .filter(|id| !ids.contains(id))
                    .copied()
                    .collect();
                s.live = ids;
                gone
            });
            for id in gone {
                self.panels.remove(id);
                st(|s| s.runtimes.remove(&id));
            }
        }
        let Some(native) = self.native.as_mut() else {
            return;
        };
        let mounts = native.take_panel_mounts();
        let scene = &mut self.scene.scene;
        let _scope = platypus_scene::api::enter(scene);
        for (target, element) in mounts {
            let id = target as u32;
            let (root, fonts, sheets) = (&self.asset_root, &self.font_sources, &self.sheet_sources);
            let panel = self
                .panels
                .get_or_create(id, || new_panel_tree(root, fonts, sheets));
            let errors = st(|s| {
                let (rt, rid) = s.runtimes.entry(id).or_insert_with(|| {
                    let mut rt = Runtime::new(FIRST_NODE);
                    let rid = rt.create_root(platypus_ui::ROOT);
                    (rt, rid)
                });
                let rid = *rid;
                rt.render(rid, element, &mut panel.tree)
            });
            for e in errors {
                tracing::warn!(target: "ui", "{e}");
            }
        }
        // State set by timers and effects since the last frame.
        let ids: Vec<u32> = st(|s| s.runtimes.keys().copied().collect());
        for id in ids {
            let Some(panel) = self.panels.map.get_mut(&id) else {
                continue;
            };
            let errors = st(|s| {
                s.runtimes
                    .get_mut(&id)
                    .map(|(rt, _)| rt.flush(&mut panel.tree))
                    .unwrap_or_default()
            });
            for e in errors {
                tracing::warn!(target: "ui", "{e}");
            }
        }
    }
}

impl App {
    /// Secondary windows' roots: new `createRoot(win)` mounts (kept until the window exists), and
    /// every window's pending state updates.
    pub(super) fn sync_window_roots(&mut self) {
        let Some(native) = self.native.as_mut() else {
            return;
        };
        let mounts = native.take_window_mounts();
        st(|s| {
            s.window_pending
                .extend(mounts.into_iter().map(|(h, e)| (h as u32, e)))
        });
        let pending = st(|s| std::mem::take(&mut s.window_pending));
        let _scope = platypus_scene::api::enter(&mut self.scene.scene);
        for (key, element) in pending {
            let Some(w) = self.windows.map.get_mut(&platypus_platform::WindowKey(key)) else {
                // Not opened yet: wait, unless the window is gone.
                let live =
                    platypus_engine_api::window::state(platypus_engine_api::window::WindowRef(key))
                        .is_some_and(|s| s.open);
                if live {
                    st(|s| s.window_pending.push((key, element)));
                }
                continue;
            };
            let w_ref = platypus_engine_api::window::WindowRef(key);
            let _domain = platypus_engine_api::window::enter_domain(
                platypus_engine_api::window::domain_of(w_ref),
            );
            let _current = platypus_engine_api::window::enter_current(w_ref);
            let errors = st(|s| {
                let (rt, rid) = s.windows.entry(key).or_insert_with(|| {
                    let mut rt = Runtime::new(FIRST_NODE);
                    let rid = rt.create_root(platypus_ui::ROOT);
                    (rt, rid)
                });
                let rid = *rid;
                rt.render(rid, element, &mut w.tree)
            });
            for e in errors {
                tracing::warn!(target: "ui", "{e}");
            }
        }
        // State set by timers and effects since the last frame.
        let keys: Vec<u32> = st(|s| s.windows.keys().copied().collect());
        for key in keys {
            let Some(w) = self.windows.map.get_mut(&platypus_platform::WindowKey(key)) else {
                continue;
            };
            let w_ref = platypus_engine_api::window::WindowRef(key);
            let _domain = platypus_engine_api::window::enter_domain(
                platypus_engine_api::window::domain_of(w_ref),
            );
            let _current = platypus_engine_api::window::enter_current(w_ref);
            let errors = st(|s| {
                s.windows
                    .get_mut(&key)
                    .map(|(rt, _)| rt.flush(&mut w.tree))
                    .unwrap_or_default()
            });
            for e in errors {
                tracing::warn!(target: "ui", "{e}");
            }
        }
    }
}

/// Drops a closed window's reconciler (its effects' cleanups run) with the scene current.
pub(super) fn drop_window_runtime(key: u32) {
    let rt = st(|s| s.windows.remove(&key));
    drop(rt);
    st(|s| s.window_pending.retain(|(k, _)| *k != key));
}

/// A UI event of a window through the handlers of its reconciler (see [`panel_event`]).
pub(super) fn window_event(
    scene: &mut platypus_scene::Scene,
    key: u32,
    e: &platypus_ui::UiEvent,
) -> bool {
    let Some(mut ev) = native::to_event(e) else {
        return false;
    };
    let _scope = platypus_scene::api::enter(scene);
    st(|s| {
        s.windows.get_mut(&key).is_some_and(|(rt, _)| {
            native::set_target_kind(rt.host_kind(ev.target));
            rt.dispatch_handlers(&mut ev)
        })
    })
}

/// Commits what the handlers of a window's events changed (renders and effects, with the scene
/// current).
pub(super) fn window_settle(scene: &mut platypus_scene::Scene, key: u32, tree: &mut UiTree) {
    let _scope = platypus_scene::api::enter(scene);
    let errors = st(|s| {
        s.windows
            .get_mut(&key)
            .map(|(rt, _)| rt.settle(tree))
            .unwrap_or_default()
    });
    for e in errors {
        tracing::warn!(target: "ui", "{e}");
    }
}

/// A UI event of a world panel through the handlers of its reconciler, while the panel's
/// interaction layer handles the input; says whether a handler prevented the default action.
/// Follow with [`panel_settle`].
pub(super) fn panel_event(
    scene: &mut platypus_scene::Scene,
    id: u32,
    e: &platypus_ui::UiEvent,
) -> bool {
    let Some(mut ev) = native::to_event(e) else {
        return false;
    };
    let _scope = platypus_scene::api::enter(scene);
    st(|s| {
        s.runtimes.get_mut(&id).is_some_and(|(rt, _)| {
            native::set_target_kind(rt.host_kind(ev.target));
            rt.dispatch_handlers(&mut ev)
        })
    })
}

/// Commits what the handlers of a panel's events changed (renders and effects, with the scene
/// current).
pub(super) fn panel_settle(scene: &mut platypus_scene::Scene, id: u32, tree: &mut UiTree) {
    let _scope = platypus_scene::api::enter(scene);
    let errors = st(|s| {
        s.runtimes
            .get_mut(&id)
            .map(|(rt, _)| rt.settle(tree))
            .unwrap_or_default()
    });
    for e in errors {
        tracing::warn!(target: "ui", "{e}");
    }
}
