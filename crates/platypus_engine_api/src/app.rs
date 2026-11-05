// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Application` and `SystemInfo` (Unity's): the game's manifest (its `package.json`) and what the
//! OS and GPU report. The host fills them ([`set_app`], [`set_system`], [`set_focus`]) before the
//! game's code runs; scripts read them, and their few writes (`targetFrameRate`,
//! `runInBackground`, `quit()`) come back to the host through [`take_requests`].

use std::cell::RefCell;
use std::sync::OnceLock;
use ts_api::ts_api;

/// What `Application.crash(message, plain)` runs: the host's crash report and dialog. It ends the
/// process.
static CRASH_HOOK: OnceLock<fn(&str, bool)> = OnceLock::new();

/// Sets the handler of `Application.crash` (the host does, at start).
pub fn set_crash_hook(hook: fn(&str, bool)) {
    let _ = CRASH_HOOK.set(hook);
}

/// The game's manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct AppInfo {
    pub name: String,
    pub product_name: String,
    /// The manifest's `subtitle` (empty when unset).
    pub subtitle: String,
    pub version: String,
    pub company_name: String,
    pub identifier: String,
    pub engine_version: String,
    /// True in `platypus_editor`.
    pub is_editor: bool,
    /// The manifest's `platypus.runInBackground`.
    pub run_in_background: bool,
    /// The manifest's frame rate, or -1.
    pub target_frame_rate: i32,
    /// The command line's arguments after `--` (`platypus-app game -- --level 2`).
    pub args: Vec<String>,
}

impl Default for AppInfo {
    fn default() -> Self {
        AppInfo {
            name: String::new(),
            product_name: String::new(),
            subtitle: String::new(),
            version: "0.0.0".into(),
            company_name: String::new(),
            identifier: String::new(),
            engine_version: String::new(),
            is_editor: false,
            run_in_background: false,
            target_frame_rate: -1,
            args: Vec::new(),
        }
    }
}

/// The machine.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemInfo {
    pub operating_system: String,
    /// `Windows`, `MacOSX`, `Linux`, `Android`, `iOS` or `Other`.
    pub operating_system_family: String,
    pub operating_system_version: String,
    /// `Desktop`, `Handheld`, `Console` or `Unknown`.
    pub device_type: String,
    pub device_name: String,
    pub processor_type: String,
    pub processor_count: u32,
    /// Memory in MB.
    pub system_memory_size: u32,
    pub architecture: String,
    /// A BCP 47 tag (`en-GB`).
    pub language: String,
    pub touch_supported: bool,
    pub graphics_device_name: String,
    pub graphics_device_vendor_id: u32,
    pub graphics_device_type: String,
    pub graphics_device_kind: String,
    pub graphics_driver: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub screen_refresh_rate: u32,
    pub dpi: u32,
}

impl Default for SystemInfo {
    fn default() -> Self {
        SystemInfo {
            operating_system: String::new(),
            operating_system_family: "Other".into(),
            operating_system_version: String::new(),
            device_type: "Unknown".into(),
            device_name: String::new(),
            processor_type: String::new(),
            processor_count: 1,
            system_memory_size: 0,
            architecture: String::new(),
            language: "en-US".into(),
            touch_supported: false,
            graphics_device_name: String::new(),
            graphics_device_vendor_id: 0,
            graphics_device_type: String::new(),
            graphics_device_kind: String::new(),
            graphics_driver: String::new(),
            screen_width: 0,
            screen_height: 0,
            screen_refresh_rate: 0,
            dpi: 96,
        }
    }
}

/// What a script asked of the host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    /// `Application.targetFrameRate = n` (-1: as fast as presenting allows).
    TargetFrameRate(i32),
    RunInBackground(bool),
    /// `Application.quit(exitCode)`: the process's exit code (0 by default).
    Quit(i32),
    /// `Application.showVideoSettings()`: open the video-settings dialog.
    ShowVideoSettings,
}

/// What changed in [`set_focus`], so the host can run `onApplicationPause` / `onApplicationFocus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusChange {
    pub pause_changed: bool,
    pub focus_changed: bool,
}

struct State {
    app: AppInfo,
    system: SystemInfo,
    target_frame_rate: Option<i32>,
    run_in_background: Option<bool>,
    focused: bool,
    paused: bool,
    /// The host's frame counter (the `--fps` overlay's number); 0 before its first window.
    fps: f64,
    requests: Vec<Request>,
    focus_listeners: ts_api::callback::Listeners<(bool,)>,
    video_listeners: ts_api::callback::Listeners<()>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        app: AppInfo::default(),
        system: SystemInfo::default(),
        target_frame_rate: None,
        run_in_background: None,
        focused: true,
        paused: false,
        fps: 0.0,
        requests: Vec::new(),
        focus_listeners: Default::default(),
        video_listeners: Default::default(),
    });
}

/// The measured frame rate, from the host's frame counter (`Application.fps`).
pub fn set_fps(fps: f64) {
    STATE.with(|s| s.borrow_mut().fps = fps);
}

pub fn set_app(info: AppInfo) {
    STATE.with(|s| s.borrow_mut().app = info);
}

pub fn set_system(info: SystemInfo) {
    STATE.with(|s| s.borrow_mut().system = info);
}

/// The window gained or lost focus (or was minimised), and whether that pauses the game.
pub fn set_focus(focused: bool, paused: bool) -> FocusChange {
    let (change, cbs) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let change = FocusChange {
            pause_changed: paused != s.paused,
            focus_changed: focused != s.focused,
        };
        s.focused = focused;
        s.paused = paused;
        (change, s.focus_listeners.snapshot())
    });
    if change.focus_changed {
        ts_api::callback::emit(&cbs, (focused,));
    }
    change
}

/// What scripts asked since the last call.
pub fn take_requests() -> Vec<Request> {
    STATE.with(|s| std::mem::take(&mut s.borrow_mut().requests))
}

/// Back to a focused, running game with nothing set by scripts (a test, or a reload).
pub fn reset() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.target_frame_rate = None;
        s.run_in_background = None;
        s.focused = true;
        s.paused = false;
        s.requests.clear();
        s.focus_listeners.clear();
        s.video_listeners.clear();
    });
}

/// The player applied new video settings in the dialog `showVideoSettings` opened: runs the
/// `onVideoSettingsChanged` listeners.
pub fn video_settings_changed() {
    let cbs = STATE.with(|s| s.borrow().video_listeners.snapshot());
    ts_api::callback::emit(&cbs, ());
}

fn app<R>(f: impl FnOnce(&AppInfo) -> R) -> R {
    STATE.with(|s| f(&s.borrow().app))
}

fn sys<R>(f: impl FnOnce(&SystemInfo) -> R) -> R {
    STATE.with(|s| f(&s.borrow().system))
}

#[ts_api(namespace = "Application")]
pub mod application_api {
    use super::{Request, STATE, app, sys};

    /// The game's `package.json` name.
    #[property]
    pub fn name() -> String {
        app(|a| a.name.clone())
    }

    /// The title it shows (`platypus.title`, else the name). Unity's `productName`.
    #[property]
    pub fn product_name() -> String {
        app(|a| a.product_name.clone())
    }

    /// A second line under the title (`subtitle` in `project.json`, say a series' episode);
    /// empty when the game has none.
    #[property]
    pub fn subtitle() -> String {
        app(|a| a.subtitle.clone())
    }

    #[property]
    pub fn version() -> String {
        app(|a| a.version.clone())
    }

    #[property]
    pub fn company_name() -> String {
        app(|a| a.company_name.clone())
    }

    /// Reverse-DNS id (`platypus.identifier`).
    #[property]
    pub fn identifier() -> String {
        app(|a| a.identifier.clone())
    }

    /// Platypus's version (Unity's `unityVersion`).
    #[property]
    pub fn engine_version() -> String {
        app(|a| a.engine_version.clone())
    }

    /// The OS family it runs on.
    #[property]
    pub fn platform() -> String {
        sys(|s| s.operating_system_family.clone())
    }

    /// The user's language as a BCP 47 tag (`en-GB`). Unity's `systemLanguage`.
    #[property]
    pub fn system_language() -> String {
        sys(|s| s.language.clone())
    }

    /// Whether this runs in the editor (`platypus_editor`) rather than the player (`platypus-app`).
    #[property]
    pub fn is_editor() -> bool {
        app(|a| a.is_editor)
    }

    #[property]
    pub fn is_mobile_platform() -> bool {
        sys(|s| s.device_type == "Handheld")
    }

    /// Frames per second to aim for (-1: as fast as presenting allows). Like Unity's, it only
    /// counts with vsync off (`QualitySettings.vSyncCount = 0`).
    #[property]
    pub fn target_frame_rate() -> f64 {
        STATE.with(|s| {
            let s = s.borrow();
            f64::from(s.target_frame_rate.unwrap_or(s.app.target_frame_rate))
        })
    }

    pub fn set_target_frame_rate(fps: f64) {
        // `fps > 0 ? Math.round(fps) : -1`
        let fps = if fps > 0.0 { fps.round() as i32 } else { -1 };
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.target_frame_rate = Some(fps);
            s.requests.push(Request::TargetFrameRate(fps));
        });
    }

    /// Frames presented per second, measured by the host over about a quarter second of real
    /// time: the number the `--fps` counter shows, whether or not that is drawn. 0 until the
    /// first window has closed.
    #[property]
    pub fn fps() -> f64 {
        STATE.with(|s| s.borrow().fps)
    }

    /// Keep running while the window is in the background (unfocused or minimised). Off by
    /// default (`platypus.runInBackground` in `package.json` changes that): then the game pauses.
    #[property]
    pub fn run_in_background() -> bool {
        STATE.with(|s| {
            let s = s.borrow();
            s.run_in_background.unwrap_or(s.app.run_in_background)
        })
    }

    pub fn set_run_in_background(on: bool) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.run_in_background = Some(on);
            s.requests.push(Request::RunInBackground(on));
        });
    }

    /// Whether the window has focus (and is not minimised).
    #[property]
    pub fn is_focused() -> bool {
        STATE.with(|s| s.borrow().focused)
    }

    /// Whether the game is paused in the background.
    #[property]
    pub fn is_paused() -> bool {
        STATE.with(|s| s.borrow().paused)
    }

    /// Calls `f(focused)` when the window gains or loses focus; returns a token for
    /// `offFocusChanged` (the TypeScript returned an unsubscribe function).
    #[subscription(off = "off_focus_changed")]
    pub fn on_focus_changed(f: ts_api::Callback<(bool,)>) -> u32 {
        STATE.with(|s| s.borrow_mut().focus_listeners.add(f))
    }

    /// Stops a listener from `onFocusChanged`; false when the token is unknown.
    pub fn off_focus_changed(token: u32) -> bool {
        STATE.with(|s| s.borrow_mut().focus_listeners.remove(token))
    }

    /// Opens the video-settings dialog (window mode, resolution, vsync, quality level: the one
    /// `videoSettingsPrompt` shows at startup) over the game, for a game's own "Video settings..."
    /// button. Returns at once; the game pauses while the dialog is open (`isPaused`, the pause
    /// hooks run as for the background pause). Apply puts the choice into effect and saves it
    /// (`onVideoSettingsChanged` then runs); Cancel, Esc or closing the dialog changes nothing.
    /// Nothing happens in screenshot or bench runs, or while the dialog is already open.
    pub fn show_video_settings() {
        STATE.with(|s| s.borrow_mut().requests.push(Request::ShowVideoSettings));
    }

    /// Calls `f()` after the player applied new settings in `showVideoSettings`'s dialog;
    /// returns a token for `offVideoSettingsChanged`.
    #[subscription(off = "off_video_settings_changed")]
    pub fn on_video_settings_changed(f: ts_api::Callback<()>) -> u32 {
        STATE.with(|s| s.borrow_mut().video_listeners.add(f))
    }

    /// Stops a listener from `onVideoSettingsChanged`; false when the token is unknown.
    pub fn off_video_settings_changed(token: u32) -> bool {
        STATE.with(|s| s.borrow_mut().video_listeners.remove(token))
    }

    /// The command line's arguments for the game: everything after `--`
    /// (`platypus-app examples/md-reader -- --goto docs/css.md`). Unity's `GetCommandLineArgs`. A copy
    /// each read.
    #[property]
    pub fn args() -> Vec<String> {
        app(|a| a.args.clone())
    }

    /// Closes the game after this frame; `platypus-app` exits with `exit_code` (0, success, by default;
    /// a fraction is cut off, as `Math.trunc`).
    #[optional(exit_code = 0.0)]
    pub fn quit(exit_code: f64) {
        let code = if exit_code.is_finite() {
            exit_code.trunc() as i32
        } else {
            0
        };
        STATE.with(|s| s.borrow_mut().requests.push(Request::Quit(code)));
    }

    /// Ends the game at once as a crash: a crash report is saved beside the log and the crash
    /// dialog shows, as for an unexpected error. `message` says why. `plain` shows the system's
    /// plain dialog instead of Platypus's own (the one used when Platypus's can't run). A game should end
    /// with `quit`; this is for errors it can't recover from, and for trying the dialog.
    #[optional(message, plain)]
    pub fn crash(message: &str, plain: bool) {
        if let Some(hook) = super::CRASH_HOOK.get() {
            hook(message, plain);
        }
        // No host handler (a test, a tool): end the process all the same.
        std::process::exit(3);
    }
}

/// The machine. GPU and screen details arrive once the window opens.
#[ts_api(namespace = "SystemInfo")]
pub mod system_info_api {
    use super::sys;

    /// OS name and version, e.g. `Windows 11 Pro`.
    #[property]
    pub fn operating_system() -> String {
        sys(|s| s.operating_system.clone())
    }

    #[property]
    pub fn operating_system_family() -> String {
        sys(|s| s.operating_system_family.clone())
    }

    #[property]
    pub fn operating_system_version() -> String {
        sys(|s| s.operating_system_version.clone())
    }

    #[property]
    pub fn device_type() -> String {
        sys(|s| s.device_type.clone())
    }

    /// The computer's network name.
    #[property]
    pub fn device_name() -> String {
        sys(|s| s.device_name.clone())
    }

    /// CPU model.
    #[property]
    pub fn processor_type() -> String {
        sys(|s| s.processor_type.clone())
    }

    /// Logical CPU cores.
    #[property]
    pub fn processor_count() -> f64 {
        sys(|s| f64::from(s.processor_count))
    }

    /// Memory in MB.
    #[property]
    pub fn system_memory_size() -> f64 {
        sys(|s| f64::from(s.system_memory_size))
    }

    /// CPU architecture: `x86_64`, `aarch64`.
    #[property]
    pub fn architecture() -> String {
        sys(|s| s.architecture.clone())
    }

    #[property]
    pub fn graphics_device_name() -> String {
        sys(|s| s.graphics_device_name.clone())
    }

    /// PCI vendor id (0x10DE NVIDIA, 0x1002 AMD, 0x8086 Intel).
    #[property]
    #[js_name = "graphicsDeviceVendorID"]
    pub fn graphics_device_vendor_id() -> f64 {
        sys(|s| f64::from(s.graphics_device_vendor_id))
    }

    /// The graphics API in use: `Vulkan`, `Dx12`, `Metal`, `Gl`.
    #[property]
    pub fn graphics_device_type() -> String {
        sys(|s| s.graphics_device_type.clone())
    }

    /// `DiscreteGpu`, `IntegratedGpu`, `VirtualGpu`, `Cpu`, `Other`.
    #[property]
    pub fn graphics_device_kind() -> String {
        sys(|s| s.graphics_device_kind.clone())
    }

    #[property]
    pub fn graphics_driver() -> String {
        sys(|s| s.graphics_driver.clone())
    }

    #[property]
    pub fn touch_supported() -> bool {
        sys(|s| s.touch_supported)
    }

    /// The monitor the window is on, in physical pixels (`SystemInfo.screen.width` in the old
    /// TypeScript).
    #[property]
    pub fn screen_width() -> f64 {
        sys(|s| f64::from(s.screen_width))
    }

    #[property]
    pub fn screen_height() -> f64 {
        sys(|s| f64::from(s.screen_height))
    }

    /// Its refresh rate in Hz (0 if unknown).
    #[property]
    pub fn screen_refresh_rate() -> f64 {
        sys(|s| f64::from(s.screen_refresh_rate))
    }

    #[property]
    pub fn screen_dpi() -> f64 {
        sys(|s| f64::from(s.dpi))
    }
}
