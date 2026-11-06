// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Secondary windows in the app (design: `docs/research/platypus-windows.md` section 8): the script's
//! `Window` requests become platform commands, each opened window gets its own UI tree, interaction
//! state and surface, and the platform's window events are routed to them.
//!
//! The main window is not an [`AppWindow`]: the screen UI, its `Interaction` and `Gfx.surface` stay
//! as they are, so single-window games run the same code as before.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use platypus_engine_api::window::{self as win, Style, WindowRef, WindowRequest, WindowState};
use platypus_platform::{
    InputEvent, WheelDelta, WindowCommand, WindowHost, WindowKey, WindowKind, WindowSpec,
};
use platypus_render::{OffscreenTarget, WindowSurface};
use web_ui_dom::{Input, Interaction, UiTree, events};

use super::*;

/// One secondary window.
pub struct AppWindow {
    /// Dropped before `host`: a wgpu surface must go before the OS window it draws to.
    pub surface: Option<WindowSurface>,
    /// Virtual windows (screenshot runs) draw into this instead.
    pub offscreen: Option<OffscreenTarget>,
    pub host: WindowHost,
    pub tree: UiTree,
    pub interaction: Interaction,
    /// Stylesheets added to this tree, by the app's sheet key.
    pub sheets: HashMap<u32, u32>,
    /// The pointer in window pixels, while it is over the window.
    pub pointer_px: Option<(f32, f32)>,
    pub buttons_down: u32,
    pub ime_allowed: bool,
    pub cursor: &'static str,
    /// Client size in physical pixels, and physical pixels per UI pixel.
    pub physical: (u32, u32),
    pub scale: f64,
    /// A frame was drawn (the platform shows the window after the first one).
    pub drawn: bool,
}

/// The open secondary windows, and which of the game's windows have OS focus.
#[derive(Default)]
pub struct AppWindows {
    pub map: BTreeMap<WindowKey, AppWindow>,
    /// Windows with OS focus (the main window is in it from the start).
    pub focused: BTreeSet<WindowKey>,
    /// The position each window was opened at (the requests resolved it).
    pub placed: HashMap<WindowKey, Option<(f64, f64)>>,
    /// Titles in use for screenshot file names.
    pub slugs: HashMap<WindowKey, String>,
}

impl AppWindows {
    pub fn new() -> Self {
        AppWindows {
            focused: BTreeSet::from([WindowKey::MAIN]),
            ..AppWindows::default()
        }
    }
}

/// One isolated event domain (a family of windows under a root other than the main window). The
/// main domain keeps `App::lifecycle` and handles its windows' events at once.
pub struct Domain {
    /// Whether one of its windows has OS focus, as last reported.
    pub focused: bool,
    /// One of its windows had focus at least once (a new domain is not "unfocused" before that).
    pub seen_focus: bool,
    /// Its own pause rule: `runInBackground` of the root, never in screenshot runs.
    pub lifecycle: lifecycle::Lifecycle,
    /// Pointer, key and IME events of its windows, dispatched in `pump_domains`.
    pub queue: std::collections::VecDeque<(WindowKey, InputEvent)>,
}

/// The isolated domains (not the main one), by domain id.
#[derive(Default)]
pub struct Domains {
    pub map: BTreeMap<win::DomainId, Domain>,
}

impl Domains {
    /// Makes a domain for each open root that has none and drops the ones whose root closed.
    pub fn sync(
        &mut self,
        open: &[win::DomainId],
        run_in_background: impl Fn(win::DomainId) -> bool,
        always_run: bool,
    ) {
        self.map
            .retain(|d, _| *d != win::DomainId::MAIN && open.contains(d));
        for d in open {
            if *d == win::DomainId::MAIN {
                continue;
            }
            self.map.entry(*d).or_insert_with(|| Domain {
                focused: true,
                seen_focus: false,
                lifecycle: lifecycle::Lifecycle::new(run_in_background(*d), always_run),
                queue: Default::default(),
            });
        }
    }

    /// Queues an event of one of the domain's windows.
    pub fn push(&mut self, d: win::DomainId, key: WindowKey, event: InputEvent) {
        if let Some(dom) = self.map.get_mut(&d) {
            dom.queue.push_back((key, event));
        }
    }

    /// What is queued for `d`, in arrival order; the queue is emptied.
    pub fn take_queue(&mut self, d: win::DomainId) -> Vec<(WindowKey, InputEvent)> {
        self.map
            .get_mut(&d)
            .map(|x| x.queue.drain(..).collect())
            .unwrap_or_default()
    }

    /// Whether the isolated domains are all paused (vacuously true with none).
    pub fn all_paused(&self) -> impl Iterator<Item = bool> + '_ {
        self.map.values().map(|d| d.lifecycle.paused())
    }
}

/// Events that wait for their domain's turn: what drives its input and UI. Window state events
/// (focus, size, position, scale) apply at once.
pub fn queues_for_domain(event: &InputEvent) -> bool {
    matches!(
        event,
        InputEvent::MouseMove { .. }
            | InputEvent::MouseLeave
            | InputEvent::MouseDown { .. }
            | InputEvent::MouseUp { .. }
            | InputEvent::MouseWheel(_)
            | InputEvent::KeyDown { .. }
            | InputEvent::KeyUp { .. }
            | InputEvent::Modifiers(_)
            | InputEvent::Ime(_)
    )
}

/// The domains with a focused window: focus moving between a game's windows (`Focused(false)` then
/// `Focused(true)` in one batch) leaves a family focused when it lands in the same one.
pub fn domain_focused(
    focused: &BTreeSet<WindowKey>,
    domain_of: impl Fn(WindowKey) -> win::DomainId,
) -> BTreeSet<win::DomainId> {
    focused.iter().map(|k| domain_of(*k)).collect()
}

/// Draws one window's UI into its surface (vsync stays off) or its offscreen target. Returns
/// whether anything was drawn (a minimised or 0 x 0 window is skipped).
pub fn draw_window(
    gpu: &Gpu,
    renderer: &mut UiRenderer,
    w: &mut AppWindow,
    now_ms: f64,
    dark: bool,
) -> bool {
    if w.physical.0 == 0 || w.physical.1 == 0 {
        return false;
    }
    let ui = to_ui(w.physical, w.scale);
    w.tree.set_size(ui.0 as f32, ui.1 as f32);
    w.tree.set_time(now_ms);
    w.tree.set_media_env(web_ui_dom::media::MediaEnv {
        resolution: w.scale as f32,
        dark,
        ..w.tree.media_env()
    });
    load_images(&mut w.tree, renderer, gpu);
    let cmds = w.tree.draw_list();
    let scale = w.scale as f32;
    if let Some(surface) = w.surface.as_mut() {
        surface.present_with(gpu, |view, format, size| {
            let target = RenderTarget {
                view,
                format,
                size,
                scale,
            };
            renderer.render(gpu, &target, CLEAR, &cmds);
        });
    } else if let Some(off) = &w.offscreen {
        let Some(view) = off.view() else {
            return false;
        };
        let target = RenderTarget {
            view: &view,
            format: off.format(),
            size: off.size(),
            scale,
        };
        renderer.render(gpu, &target, CLEAR, &cmds);
    } else {
        return false;
    }
    true
}

/// Draws a window offscreen and reads its pixels back (RGBA, the window's physical size). Virtual
/// windows use their own target; an OS window gets a temporary one.
pub fn window_pixels(
    gpu: &Gpu,
    renderer: &mut UiRenderer,
    w: &mut AppWindow,
    now_ms: f64,
    dark: bool,
) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    if w.offscreen.is_some() {
        draw_window(gpu, renderer, w, now_ms, dark);
        let off = w.offscreen.as_ref().context("no target")?;
        let (width, height) = off.size();
        return Ok((width, height, off.read_pixels(gpu)?));
    }
    let (width, height) = (w.physical.0.max(1), w.physical.1.max(1));
    let tmp = OffscreenTarget::with_format(gpu, width, height, wgpu::TextureFormat::Rgba8UnormSrgb);
    let tmp_window = w.offscreen.replace(tmp);
    let surface = w.surface.take();
    draw_window(gpu, renderer, w, now_ms, dark);
    w.surface = surface;
    let tmp = std::mem::replace(&mut w.offscreen, tmp_window).context("no target")?;
    Ok((width, height, tmp.read_pixels(gpu)?))
}

/// UI pixels of a physical size at `scale` physical pixels per UI pixel.
pub fn to_ui(physical: (u32, u32), scale: f64) -> (f64, f64) {
    let s = if scale > 0.0 { scale } else { 1.0 };
    (f64::from(physical.0) / s, f64::from(physical.1) / s)
}

/// A file-name part for a window's screenshot: the title lowercased, runs of characters other
/// than `a-z0-9` as `-`, trimmed. Empty becomes `window<handle>`; one already in `used` gets
/// `-<handle>`.
pub fn slug(title: &str, handle: u32, used: &mut HashSet<String>) -> String {
    let mut out = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut s = out.trim_matches('-').to_string();
    if s.is_empty() {
        s = format!("window{handle}");
    }
    if used.contains(&s) {
        s = format!("{s}-{handle}");
    }
    used.insert(s.clone());
    s
}

fn key(w: WindowRef) -> WindowKey {
    WindowKey(w.0)
}

fn key_ref(k: WindowKey) -> WindowRef {
    WindowRef(k.0)
}

/// An event as the arguments of `__platypus_input` / `input::apply_native` (`keydown code key repeat`,
/// `mousemove x y`, `wheel dx dy`, `blur`); the mouse in UI pixels.
pub fn game_args(event: &InputEvent, ui: f64) -> Option<Vec<platypus_core::Value>> {
    let name = |b: &platypus_platform::MouseButton| match b {
        platypus_platform::MouseButton::Left => "MouseLeft".to_owned(),
        platypus_platform::MouseButton::Right => "MouseRight".to_owned(),
        platypus_platform::MouseButton::Middle => "MouseMiddle".to_owned(),
        platypus_platform::MouseButton::Other(n) => format!("Mouse{n}"),
    };
    Some(match event {
        InputEvent::MouseMove { x, y } => {
            vec!["mousemove".into(), (x / ui).into(), (y / ui).into()]
        }
        InputEvent::MouseDown { button } => {
            vec![
                "keydown".into(),
                name(button).into(),
                "".into(),
                false.into(),
            ]
        }
        InputEvent::MouseUp { button } => vec!["keyup".into(), name(button).into(), "".into()],
        InputEvent::MouseWheel(delta) => {
            let (dx, dy) = match delta {
                WheelDelta::Lines { x, y } => (f64::from(*x), f64::from(*y)),
                WheelDelta::Pixels { x, y } => (x / 40.0, y / 40.0),
            };
            vec!["wheel".into(), dx.into(), dy.into()]
        }
        InputEvent::KeyDown {
            code, key, repeat, ..
        } => vec![
            "keydown".into(),
            code.as_str().into(),
            key.as_str().into(),
            (*repeat).into(),
        ],
        InputEvent::KeyUp { code, key } => {
            vec!["keyup".into(), code.as_str().into(), key.as_str().into()]
        }
        InputEvent::Focused(false) => vec!["blur".into()],
        _ => return None,
    })
}

fn kind_of(style: Style) -> WindowKind {
    WindowKind::parse(style.name()).unwrap_or_default()
}

fn spec_flags(spec: &mut WindowSpec, st: &WindowState) {
    spec.resizable = st.flags.resizable;
    spec.minimizable = st.flags.minimizable;
    spec.maximizable = st.flags.maximizable;
    spec.closable = st.flags.closable;
    spec.always_on_top = st.flags.always_on_top;
    spec.visible = st.flags.visible;
}

/// The spec a `Create` asks for. A window without a position is centred on its parent, each
/// further child 24 px lower and to the right.
fn open_spec(
    st: &WindowState,
    state: &impl Fn(WindowRef) -> Option<WindowState>,
    open: &[WindowRef],
    me: WindowRef,
) -> WindowSpec {
    let position = st.position.or_else(|| {
        let parent = st.parent?;
        let p = state(parent)?;
        let n = open
            .iter()
            .take_while(|w| **w != me)
            .filter(|w| state(**w).is_some_and(|s| s.parent == Some(parent)))
            .count() as u32;
        Some(platypus_platform::place(
            p.position.unwrap_or((0.0, 0.0)),
            p.size,
            st.size,
            n,
        ))
    });
    let mut spec = WindowSpec {
        title: st.title.clone(),
        size: st.size,
        position,
        min_size: st.min_size,
        kind: kind_of(st.style),
        parent: st.parent.map(key),
        active: st.flags.focus_on_show,
        ..WindowSpec::default()
    };
    spec_flags(&mut spec, st);
    spec
}

/// What scripts asked for, as platform commands, and the main window's new title if it changed.
/// `open` is the open windows in creation order.
pub fn requests_to_commands(
    reqs: Vec<WindowRequest>,
    state: impl Fn(WindowRef) -> Option<WindowState>,
    open: &[WindowRef],
) -> (Vec<WindowCommand>, Option<String>) {
    let mut cmds = Vec::new();
    let mut main_title = None;
    for r in reqs {
        match r {
            WindowRequest::Create(w) => {
                if let Some(st) = state(w) {
                    cmds.push(WindowCommand::Open(key(w), open_spec(&st, &state, open, w)));
                }
            }
            WindowRequest::Title(w, t) => {
                if w == win::MAIN {
                    main_title = Some(t);
                } else {
                    cmds.push(WindowCommand::SetTitle(key(w), t));
                }
            }
            WindowRequest::Style(w, s) => cmds.push(WindowCommand::SetKind(key(w), kind_of(s))),
            WindowRequest::Position(w, x, y) => cmds.push(WindowCommand::SetPosition(key(w), x, y)),
            WindowRequest::Size(w, x, y) => cmds.push(WindowCommand::SetSize(key(w), x, y)),
            WindowRequest::Center(w) => cmds.push(WindowCommand::Center(key(w))),
            WindowRequest::Flags(w, f) => {
                if let Some(mut st) = state(w) {
                    st.flags = f;
                    let mut spec = WindowSpec::default();
                    spec_flags(&mut spec, &st);
                    cmds.push(WindowCommand::SetFlags(key(w), spec));
                }
            }
            WindowRequest::Minimized(w, v) => cmds.push(WindowCommand::SetMinimized(key(w), v)),
            WindowRequest::Maximized(w, v) => cmds.push(WindowCommand::SetMaximized(key(w), v)),
            WindowRequest::Focus(w) => cmds.push(WindowCommand::Focus(key(w))),
            // The main window ends with the game (`Application.quit`), not with a command.
            WindowRequest::Close(w) if w == win::MAIN => {}
            WindowRequest::Close(w) => cmds.push(WindowCommand::Close(key(w))),
        }
    }
    // The main window's other setters (size, position, flags) are the player's and the OS's.
    cmds.retain(|c| !matches!(c, WindowCommand::SetTitle(k, _) if *k == WindowKey::MAIN));
    cmds.retain(|c| match c {
        WindowCommand::SetKind(k, _)
        | WindowCommand::SetPosition(k, ..)
        | WindowCommand::SetSize(k, ..)
        | WindowCommand::Center(k)
        | WindowCommand::SetFlags(k, _)
        | WindowCommand::SetMinimized(k, _)
        | WindowCommand::SetMaximized(k, _) => *k != WindowKey::MAIN,
        _ => true,
    });
    (cmds, main_title)
}

impl App {
    /// Turns the window requests scripts made into platform commands (and sets the main title).
    /// Called after every point that runs scripts.
    pub(super) fn pump_windows(&mut self) {
        let reqs = win::take_requests();
        if reqs.is_empty() {
            return;
        }
        let open = win::open_windows();
        let (cmds, title) = requests_to_commands(reqs, win::state, &open);
        if let (Some(t), Some(window)) = (title, self.main_window().as_ref()) {
            window.set_title(&t);
        }
        for c in &cmds {
            if let WindowCommand::Open(k, spec) = c {
                self.windows.placed.insert(*k, spec.position);
            }
        }
        self.window_commands.extend(cmds);
    }

    /// Runs `f` (a call into the game's window listeners) with the scene current, then what
    /// follows a call into scripts.
    pub(super) fn in_game<R>(&mut self, f: impl FnOnce() -> R) -> R {
        let r = {
            let _scope = platypus_scene::api::enter(&mut self.scene.scene);
            f()
        };
        if let Some(n) = self.native.as_mut() {
            n.settle(&mut self.scene.scene, &mut self.ui);
            self.native_after();
        }
        r
    }

    /// The platform made the window `key`: gives it a tree and a surface (or an offscreen target
    /// for a virtual one) and tells scripts.
    pub(super) fn window_open(&mut self, key: WindowKey, host: WindowHost) {
        let w = WindowRef(key.0);
        let (physical, scale, native) = match &host {
            WindowHost::Os(window) => {
                let size = window.inner_size();
                let (handle, platform) = platypus_platform::native_handle(window);
                (
                    (size.width, size.height),
                    window.scale_factor(),
                    (handle, platform),
                )
            }
            WindowHost::Virtual {
                width,
                height,
                scale,
            } => ((*width, *height), *scale, (0, "virtual")),
        };
        let Some(gfx) = &self.gfx else {
            self.in_game(|| win::open_failed(w, "the renderer is not available"));
            self.window_commands.push(WindowCommand::Close(key));
            return;
        };
        let (mut surface, mut offscreen) = (None, None);
        match &host {
            WindowHost::Os(window) => {
                match gfx
                    .gpu
                    .create_window_surface(window.clone(), physical.0, physical.1)
                {
                    Ok(s) => surface = Some(s),
                    Err(e) => {
                        let msg = format!("no surface: {e}");
                        self.in_game(|| win::open_failed(w, &msg));
                        self.window_commands.push(WindowCommand::Close(key));
                        return;
                    }
                }
            }
            WindowHost::Virtual { .. } => {
                offscreen = Some(OffscreenTarget::with_format(
                    &gfx.gpu,
                    physical.0.max(1),
                    physical.1.max(1),
                    wgpu::TextureFormat::Rgba8UnormSrgb,
                ));
            }
        }
        let (mut tree, sheets) =
            new_panel_tree(&self.asset_root, &self.font_sources, &self.sheet_sources);
        let ui = to_ui(physical, scale);
        tree.set_size(ui.0 as f32, ui.1 as f32);
        let position = self
            .windows
            .placed
            .remove(&key)
            .flatten()
            .or_else(|| win::state(w).and_then(|s| s.position))
            .unwrap_or((0.0, 0.0));
        self.windows.map.insert(
            key,
            AppWindow {
                surface,
                offscreen,
                host,
                tree,
                interaction: Interaction::new(),
                sheets,
                pointer_px: None,
                buttons_down: 0,
                ime_allowed: false,
                cursor: "default",
                physical,
                scale,
                drawn: false,
            },
        );
        let opened = win::Opened {
            position,
            size: ui,
            scale,
            native_handle: native.0,
            platform: native.1.to_string(),
        };
        self.in_game(|| win::opened(w, opened));
    }

    /// An event of a window other than the main one.
    pub(super) fn window_event(&mut self, key: WindowKey, event: InputEvent) {
        let w = WindowRef(key.0);
        let Some(scale) = self.windows.map.get(&key).map(|x| x.scale) else {
            return;
        };
        let ui = scale as f32;
        let _current = win::enter_current(w);
        let surface = Surface::Window(key.0);
        let domain = win::domain_of(w);
        // What the game's `input` of this window's domain gets (keys not while typing in a field).
        let game = game_args(&event, f64::from(ui));
        let is_key = matches!(event, InputEvent::KeyDown { .. } | InputEvent::KeyUp { .. });
        match event {
            InputEvent::Focused(f) => {
                if f {
                    self.windows.focused.insert(key);
                } else {
                    self.windows.focused.remove(&key);
                    if let Some(x) = self.windows.map.get_mut(&key) {
                        x.buttons_down = 0;
                    }
                    self.handle_on(surface, Input::WindowBlur);
                }
                self.in_game(|| win::focus_changed(w, f));
            }
            InputEvent::Resized { width, height } => {
                let Some(x) = self.windows.map.get_mut(&key) else {
                    return;
                };
                x.physical = (width, height);
                if let (Some(s), Some(gfx)) = (x.surface.as_mut(), &self.gfx) {
                    s.resize(&gfx.gpu, width, height);
                }
                if let (Some(o), Some(gfx)) = (x.offscreen.as_mut(), &self.gfx) {
                    o.resize(&gfx.gpu, width.max(1), height.max(1));
                }
                let size = to_ui((width, height), x.scale);
                x.tree.set_size(size.0 as f32, size.1 as f32);
                self.in_game(|| win::resized(w, size.0, size.1));
            }
            InputEvent::Moved { x, y } => {
                let (lx, ly) = (f64::from(x) / f64::from(ui), f64::from(y) / f64::from(ui));
                self.in_game(|| win::moved(w, lx, ly));
            }
            InputEvent::ScaleChanged(s) => {
                if let Some(x) = self.windows.map.get_mut(&key) {
                    x.scale = s;
                }
                self.in_game(|| win::scale_changed(w, s));
            }
            InputEvent::MouseMove { x, y } => {
                if let Some(p) = self.windows.map.get_mut(&key) {
                    p.pointer_px = Some((x as f32, y as f32));
                }
                self.handle_on(
                    surface,
                    Input::PointerMove {
                        x: x as f32 / ui,
                        y: y as f32 / ui,
                    },
                );
            }
            InputEvent::MouseLeave => {
                if let Some(p) = self.windows.map.get_mut(&key) {
                    p.pointer_px = None;
                }
                self.handle_on(surface, Input::PointerLeave);
            }
            InputEvent::MouseDown { button } => {
                if let Some(p) = self.windows.map.get_mut(&key) {
                    p.buttons_down += 1;
                }
                let time_ms = self.now_ms();
                self.handle_on(
                    surface,
                    Input::PointerDown {
                        button: dom_button(button),
                        time_ms,
                    },
                );
            }
            InputEvent::MouseUp { button } => {
                self.handle_on(
                    surface,
                    Input::PointerUp {
                        button: dom_button(button),
                    },
                );
                if let Some(p) = self.windows.map.get_mut(&key) {
                    p.buttons_down = p.buttons_down.saturating_sub(1);
                }
            }
            InputEvent::MouseWheel(delta) => {
                let (dx, dy) = match delta {
                    WheelDelta::Lines { x, y } => (x * WHEEL_LINE, y * WHEEL_LINE),
                    WheelDelta::Pixels { x, y } => {
                        ((x / f64::from(ui)) as f32, (y / f64::from(ui)) as f32)
                    }
                };
                self.handle_on(surface, Input::Wheel { dx, dy });
            }
            InputEvent::KeyDown {
                code,
                key: k,
                repeat,
                text,
            } => self.handle_on(
                surface,
                Input::KeyDown {
                    key: k,
                    code,
                    repeat,
                    text,
                },
            ),
            InputEvent::KeyUp { code, key: k } => {
                self.handle_on(surface, Input::KeyUp { key: k, code })
            }
            InputEvent::Modifiers(m) => self.handle_on(
                surface,
                Input::Modifiers(events::Modifiers {
                    shift: m.shift,
                    ctrl: m.ctrl,
                    alt: m.alt,
                    meta: m.meta,
                }),
            ),
            InputEvent::Ime(platypus_platform::ImeEvent::Preedit(text, cursor)) => {
                self.handle_on(surface, Input::ImePreedit { text, cursor })
            }
            InputEvent::Ime(platypus_platform::ImeEvent::Commit(text)) => {
                self.handle_on(surface, Input::ImeCommit(text))
            }
            InputEvent::Ime(platypus_platform::ImeEvent::Disabled) => self.handle_on(
                surface,
                Input::ImePreedit {
                    text: String::new(),
                    cursor: None,
                },
            ),
            // File drops, raw motion, gamepads and the rest belong to the main window.
            _ => {}
        }
        let typing = is_key
            && self.windows.map.get(&key).is_some_and(|x| {
                x.interaction
                    .focused()
                    .is_some_and(|id| x.tree.is_editor(id))
            });
        if let Some(args) = game.filter(|_| !typing) {
            self.send_game_input_in(domain, &args);
        }
    }

    /// The user asked to close `key`: listeners may cancel.
    pub(super) fn window_close_requested(&mut self, key: WindowKey) -> bool {
        self.in_game(|| win::close_requested(WindowRef(key.0)))
    }

    /// `key` is closing: its reconciler, tree and surface go (in that order), then scripts hear
    /// `onClosed`.
    pub(super) fn window_gone(&mut self, key: WindowKey) {
        if key == WindowKey::MAIN {
            return;
        }
        {
            let _scope = platypus_scene::api::enter(&mut self.scene.scene);
            native_render::drop_window_runtime(key.0);
        }
        self.windows.map.remove(&key);
        self.windows.focused.remove(&key);
        self.windows.placed.remove(&key);
        self.windows.slugs.remove(&key);
        if matches!(self.keys_on, Surface::Window(k) if k == key.0) {
            self.keys_on = Surface::Screen;
        }
        let w = WindowRef(key.0);
        self.in_game(|| win::closed(w));
    }

    /// Draws every open secondary window after the main one. A window's first frame lists it for
    /// the platform to show (windows are made hidden, so none flashes its empty background).
    pub(super) fn draw_windows(&mut self) {
        let Some(gfx) = &mut self.gfx else {
            return;
        };
        let now_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        let manifest = &self.manifest;
        for (key, w) in self.windows.map.iter_mut() {
            let st = win::state(WindowRef(key.0));
            if st.as_ref().is_some_and(|s| s.minimized) {
                continue;
            }
            // The system's theme (or the game's `theme`), as the main window; offscreen windows
            // of screenshot runs are light unless the game forces dark.
            let dark = match &w.host {
                WindowHost::Os(window) => manifest.dark(Some(window.as_ref())),
                WindowHost::Virtual { .. } => manifest.theme.is_dark(|| false),
            };
            let first = !w.drawn;
            if draw_window(&gfx.gpu, &mut gfx.renderer, w, now_ms, dark) {
                w.drawn = true;
                if first && st.is_none_or(|s| s.flags.visible) {
                    self.windows_to_show.push(*key);
                }
            }
        }
    }

    /// Writes every open secondary window to `<stem>-<slug>.png` next to a screenshot (drawn
    /// offscreen at its size and scale; nothing is shown on screen).
    pub(super) fn save_window_screenshots(&mut self, path: &Path) -> Result<()> {
        let Some(gfx) = &mut self.gfx else {
            return Ok(());
        };
        let now_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "screenshot".into());
        let mut used = HashSet::new();
        // The main window's own name is taken by `out.png`.
        for (key, w) in self.windows.map.iter_mut() {
            let title = win::state(WindowRef(key.0)).map_or_else(String::new, |s| s.title);
            let name = slug(&title, key.0, &mut used);
            let (width, height, pixels) =
                window_pixels(&gfx.gpu, &mut gfx.renderer, w, now_ms, false)?;
            let file = path.with_file_name(format!("{stem}-{name}.png"));
            platypus_render::testing::write_png(&file, width, height, &pixels)?;
            tracing::info!("saved window screenshot to {}", file.display());
        }
        Ok(())
    }

    /// Once per loop iteration, after the events: the game's focus is any of its windows'.
    pub(super) fn window_events_done(&mut self) {
        // Each family is focused while any of its windows is: focus moving between the main
        // window and its tool windows never unfocuses the main game, and an isolated window
        // taking focus does.
        let focused_domains = domain_focused(&self.windows.focused, |k| win::domain_of(key_ref(k)));
        let main_focused = focused_domains.contains(&win::DomainId::MAIN);
        if self.main_focus_sent != Some(main_focused) {
            self.main_focus_sent = Some(main_focused);
            self.lifecycle.set_focused(main_focused);
            self.sync_lifecycle();
        }
        self.sync_domains();
        let now = Instant::now();
        let ids: Vec<win::DomainId> = self.domains.map.keys().copied().collect();
        for d in ids {
            let f = focused_domains.contains(&d);
            let Some(dom) = self.domains.map.get_mut(&d) else {
                continue;
            };
            dom.seen_focus |= f;
            if !dom.seen_focus || f == dom.focused {
                continue;
            }
            dom.focused = f;
            dom.lifecycle.set_focused(f);
            if let Some((focused, paused)) = dom.lifecycle.update(now).report {
                let _g = win::enter_domain(d);
                self.in_game(|| win::domain_focus_changed(d, focused, paused));
            }
        }
    }

    /// After a native reload: closes the windows without a key and waits for the new code's first
    /// frame to close the keyed ones nobody claimed.
    pub(super) fn close_reload_windows(&mut self) {
        let closes = self
            .native
            .as_mut()
            .map(|n| n.take_reload_closes())
            .unwrap_or_default();
        self.reload_windows_closed(closes);
    }

    /// Queues the closing of windows a reload left without an owner.
    pub(super) fn reload_windows_closed(&mut self, keys: Vec<u32>) {
        for k in keys {
            self.window_commands
                .push(WindowCommand::Close(WindowKey(k)));
        }
        self.reload_claims_pending = true;
    }

    /// The new code had its first frame: keyed windows it did not claim close.
    pub(super) fn close_unclaimed_windows(&mut self) {
        if !std::mem::take(&mut self.reload_claims_pending) {
            return;
        }
        for w in win::reload_end() {
            self.window_commands.push(WindowCommand::Close(key(w)));
        }
    }

    /// Keeps `domains` as the open isolated roots.
    pub(super) fn sync_domains(&mut self) {
        let open = win::domains();
        self.domains
            .sync(&open, win::run_in_background, self.offline);
    }

    /// The isolated domains' turns, after the main game's (`tick`): each, with its domain current,
    /// delivers its messages, runs its due timers (unless paused), dispatches the input queued for
    /// its windows and ends its input frame. Their UI roots flush with the rest in
    /// `sync_native_render`.
    pub(super) fn pump_domains(&mut self, now_ms: f64) {
        self.sync_domains();
        let ids: Vec<win::DomainId> = self.domains.map.keys().copied().collect();
        for d in ids {
            let _g = win::enter_domain(d);
            let paused = self
                .domains
                .map
                .get(&d)
                .is_some_and(|x| x.lifecycle.paused());
            self.in_game(|| win::deliver(d, now_ms));
            if !paused {
                self.in_game(|| web_ui_rt::ts_rt_timers_advance_domain(now_ms, d.0));
            }
            for (key, event) in self.domains.take_queue(d) {
                self.window_event(key, event);
            }
            let _scope = platypus_scene::api::enter(&mut self.scene.scene);
            platypus_engine_api::input::tick_domain(d);
        }
    }

    /// An event of a window other than the main one: window state applies at once, input and UI
    /// events wait for their domain's turn unless the window shares the main game's domain.
    pub(super) fn window_input_for(&mut self, key: WindowKey, event: InputEvent) {
        let d = win::domain_of(key_ref(key));
        if d != win::DomainId::MAIN && queues_for_domain(&event) {
            self.domains.push(d, key, event);
        } else {
            self.window_event(key, event);
        }
    }

    /// An input event for the game's `input` of domain `d`, with `d` entered so it is filed under
    /// that domain and its listeners run in it.
    pub(super) fn send_game_input_in(&mut self, d: win::DomainId, args: &[platypus_core::Value]) {
        if d == win::DomainId::MAIN {
            self.send_game_input(args);
        } else if let Some(n) = self.native.as_mut() {
            let _g = win::enter_domain(d);
            n.game_input(&mut self.scene.scene, &mut self.ui, args);
            self.native_after();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_engine_api::window::Flags;

    fn state(title: &str, style: Style, parent: Option<WindowRef>) -> WindowState {
        WindowState {
            key: String::new(),
            title: title.into(),
            style,
            parent,
            position: None,
            size: (200.0, 100.0),
            min_size: None,
            scale: 1.0,
            flags: Flags::default(),
            minimized: false,
            maximized: false,
            focused: false,
            created: false,
            open: true,
            open_error: None,
            embedded: false,
            run_in_background: true,
            native_handle: 0,
            platform: String::new(),
            order: 0,
        }
    }

    fn main_state() -> WindowState {
        WindowState {
            position: Some((100.0, 50.0)),
            size: (800.0, 600.0),
            ..state("Platypus", Style::Normal, None)
        }
    }

    // M9-API-1460
    #[test]
    fn script_requests_become_platform_commands() {
        let tool = state("Palette", Style::Tool, Some(win::MAIN));
        let lookup = |w: WindowRef| match w.0 {
            1 => Some(main_state()),
            2 => Some(tool.clone()),
            _ => None,
        };
        let reqs = vec![
            WindowRequest::Create(WindowRef(2)),
            WindowRequest::Title(win::MAIN, "Game".into()),
            WindowRequest::Title(WindowRef(2), "P".into()),
            WindowRequest::Close(win::MAIN),
            WindowRequest::Close(WindowRef(2)),
        ];
        let (cmds, title) = requests_to_commands(reqs, lookup, &[win::MAIN, WindowRef(2)]);
        assert_eq!(title.as_deref(), Some("Game"));
        assert_eq!(
            cmds.len(),
            3,
            "Close(MAIN) and the main title are not commands"
        );
        let WindowCommand::Open(k, spec) = &cmds[0] else {
            panic!("{:?}", cmds[0]);
        };
        assert_eq!(*k, WindowKey(2));
        assert_eq!(spec.kind, WindowKind::Tool);
        assert_eq!(spec.parent, Some(WindowKey::MAIN));
        assert_eq!(spec.title, "Palette");
        assert!(
            matches!(&cmds[1], WindowCommand::SetTitle(k, t) if *k == WindowKey(2) && t == "P")
        );
        assert!(matches!(&cmds[2], WindowCommand::Close(k) if *k == WindowKey(2)));
    }

    // M9-API-1461
    #[test]
    fn windows_without_a_position_are_placed_on_their_parent() {
        let a = state("A", Style::Tool, Some(win::MAIN));
        let b = state("B", Style::Tool, Some(win::MAIN));
        let c = WindowState {
            position: Some((5.0, 6.0)),
            ..state("C", Style::Tool, Some(win::MAIN))
        };
        let lookup = |w: WindowRef| match w.0 {
            1 => Some(main_state()),
            2 => Some(a.clone()),
            3 => Some(b.clone()),
            4 => Some(c.clone()),
            _ => None,
        };
        let open = [win::MAIN, WindowRef(2), WindowRef(3), WindowRef(4)];
        let reqs = (2..=4)
            .map(|n| WindowRequest::Create(WindowRef(n)))
            .collect();
        let (cmds, _) = requests_to_commands(reqs, lookup, &open);
        let pos = |i: usize| match &cmds[i] {
            WindowCommand::Open(_, s) => s.position,
            _ => None,
        };
        // Centred on the main window (100, 50, 800 x 600), 24 px further for the second child.
        assert_eq!(pos(0), Some((100.0 + 300.0, 50.0 + 250.0)));
        assert_eq!(pos(1), Some((100.0 + 300.0 + 24.0, 50.0 + 250.0 + 24.0)));
        assert_eq!(pos(2), Some((5.0, 6.0)));
    }

    // M9-API-1462
    #[test]
    fn screenshot_names_are_slugs_of_the_titles_and_unique() {
        let mut used = HashSet::new();
        assert_eq!(slug("Counter 1", 2, &mut used), "counter-1");
        assert_eq!(slug("!!", 3, &mut used), "window3");
        assert_eq!(slug("Counter 1", 4, &mut used), "counter-1-4");
        assert_eq!(
            slug("  --Tool: Palette (x)--", 5, &mut used),
            "tool-palette-x"
        );
    }

    // M9-API-1463
    #[test]
    fn the_game_is_focused_while_any_of_its_windows_is() {
        // MAIN <- 2 (tool); 5 is an isolated root.
        let mut tree = platypus_platform::WindowTree::default();
        tree.insert(WindowKey::MAIN, None);
        tree.insert(WindowKey(2), Some(WindowKey::MAIN));
        tree.insert(WindowKey(5), None);
        let domain = |k: WindowKey| win::DomainId(tree.root(k).0);
        let main = win::DomainId::MAIN;
        let f = BTreeSet::from([WindowKey::MAIN]);
        assert_eq!(domain_focused(&f, domain), BTreeSet::from([main]));
        // Focus moving to a tool window: Focused(false) then Focused(true) in one batch.
        let f = BTreeSet::from([WindowKey(2)]);
        assert!(domain_focused(&f, domain).contains(&main));
        assert!(domain_focused(&BTreeSet::new(), domain).is_empty());
    }

    // M9-TS-2834
    #[test]
    fn focus_moving_to_a_tool_window_keeps_the_main_domain_focused() {
        let mut tree = platypus_platform::WindowTree::default();
        tree.insert(WindowKey::MAIN, None);
        tree.insert(WindowKey(2), Some(WindowKey::MAIN));
        tree.insert(WindowKey(3), Some(WindowKey(2)));
        let domain = |k: WindowKey| win::DomainId(tree.root(k).0);
        for k in [WindowKey::MAIN, WindowKey(2), WindowKey(3)] {
            let f = BTreeSet::from([k]);
            assert_eq!(
                domain_focused(&f, domain),
                BTreeSet::from([win::DomainId::MAIN]),
                "{k:?}"
            );
        }
    }

    // M9-TS-2835
    #[test]
    fn focusing_an_isolated_window_unfocuses_the_main_domain_and_pauses_it_without_run_in_background()
     {
        let mut tree = platypus_platform::WindowTree::default();
        tree.insert(WindowKey::MAIN, None);
        tree.insert(WindowKey(2), Some(WindowKey::MAIN));
        tree.insert(WindowKey(5), None);
        let domain = |k: WindowKey| win::DomainId(tree.root(k).0);
        let f = domain_focused(&BTreeSet::from([WindowKey(5)]), domain);
        assert!(!f.contains(&win::DomainId::MAIN));
        assert!(f.contains(&win::DomainId(5)));
        // The main game's lifecycle follows: paused without `runInBackground`, running with it.
        let now = Instant::now();
        let mut main = lifecycle::Lifecycle::new(false, false);
        main.set_focused(f.contains(&win::DomainId::MAIN));
        assert!(main.update(now).paused);
        let mut bg = lifecycle::Lifecycle::new(true, false);
        bg.set_focused(false);
        assert!(!bg.update(now).paused);
        // An isolated domain pauses by its own rule.
        let mut doms = Domains::default();
        doms.sync(&[win::DomainId::MAIN, win::DomainId(5)], |_| false, false);
        let d = doms.map.get_mut(&win::DomainId(5)).unwrap();
        d.lifecycle.set_focused(false);
        assert!(d.lifecycle.update(now).paused);
        assert!(doms.all_paused().all(|p| p));
    }

    // M9-TS-2837
    #[test]
    fn events_of_an_isolated_window_queue_for_its_domain() {
        let mut doms = Domains::default();
        let (a, b) = (win::DomainId(5), win::DomainId(7));
        doms.sync(&[win::DomainId::MAIN, a, b], |_| true, false);
        assert_eq!(doms.map.len(), 2, "the main domain has no queue");
        doms.push(a, WindowKey(5), InputEvent::MouseMove { x: 1.0, y: 2.0 });
        doms.push(b, WindowKey(7), InputEvent::MouseLeave);
        doms.push(a, WindowKey(6), InputEvent::MouseLeave);
        // State events apply at once, input waits.
        assert!(queues_for_domain(&InputEvent::MouseLeave));
        assert!(!queues_for_domain(&InputEvent::Focused(true)));
        assert!(!queues_for_domain(&InputEvent::Resized {
            width: 1,
            height: 1
        }));
        let q = doms.take_queue(a);
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].0, WindowKey(5));
        assert_eq!(q[1].0, WindowKey(6));
        assert!(doms.take_queue(a).is_empty());
        assert_eq!(doms.take_queue(b).len(), 1);
        // A domain whose root closed goes with its queue.
        doms.push(a, WindowKey(5), InputEvent::MouseLeave);
        doms.sync(&[win::DomainId::MAIN, b], |_| true, false);
        assert!(!doms.map.contains_key(&a));
    }

    // M9-API-1470
    #[test]
    fn a_virtual_window_draws_its_ui_offscreen() {
        use web_ui_dom::opcodes::{APPEND_CHILD, CREATE, SET_STYLE};
        let gpu = platypus_render::testing::shared_gpu();
        let mut renderer = UiRenderer::new(&gpu);
        let mut tree = UiTree::with_system_fonts(1.0, 1.0);
        let n = |v: u32| platypus_core::Value::Number(v.into());
        let ops = vec![
            platypus_core::Value::Array(vec![n(CREATE), n(1), "box".into()]),
            platypus_core::Value::Array(vec![
                n(SET_STYLE),
                n(1),
                "width".into(),
                n(40),
                "height".into(),
                n(40),
                "backgroundColor".into(),
                "#ff0000".into(),
            ]),
            platypus_core::Value::Array(vec![n(APPEND_CHILD), n(0), n(1)]),
        ];
        assert!(tree.apply_commit(&platypus_core::Commit { ops }).is_empty());
        let mut w = AppWindow {
            surface: None,
            offscreen: Some(OffscreenTarget::with_format(
                &gpu,
                100,
                80,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            )),
            host: WindowHost::Virtual {
                width: 100,
                height: 80,
                scale: 1.0,
            },
            tree,
            interaction: Interaction::new(),
            sheets: HashMap::new(),
            pointer_px: None,
            buttons_down: 0,
            ime_allowed: false,
            cursor: "default",
            physical: (100, 80),
            scale: 1.0,
            drawn: false,
        };
        assert!(draw_window(&gpu, &mut renderer, &mut w, 0.0, false));
        let (width, height, px) = window_pixels(&gpu, &mut renderer, &mut w, 0.0, false).unwrap();
        assert_eq!((width, height), (100, 80));
        let at = |x: usize, y: usize| &px[(y * 100 + x) * 4..(y * 100 + x) * 4 + 3];
        assert_eq!(at(10, 10), [255, 0, 0], "inside the box");
        assert_ne!(at(60, 60), [255, 0, 0], "outside it");
    }

    // M9-API-1464
    #[test]
    fn window_targets_do_not_collide_with_panels() {
        let panels = [0u32, 1, 7, 4096, 1 << 20];
        for w in [2u32, 3, 500] {
            let t = platypus_core::WINDOW_TARGET | w;
            assert!(t >= platypus_core::WINDOW_TARGET);
            assert_eq!(t - platypus_core::WINDOW_TARGET, w);
            assert!(!panels.contains(&t));
        }
        assert_eq!(to_ui((1600, 1200), 2.0), (800.0, 600.0));
    }
}
