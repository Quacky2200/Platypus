// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `input`: keyboard, mouse and gamepad state for gameplay code, as `js/engine/src/input.ts` has
//! it. Keys are named by physical code (`"Space"`, `"KeyV"`); mouse buttons are keys named
//! `MouseLeft`, `MouseRight`, `MouseMiddle` (then `Mouse3`...). The host reports events as they
//! arrive; at the start of each frame ([`tick`], which [`crate::time::advance`] calls) what arrived
//! since the last tick becomes "this frame", so `getKeyDown` is true for exactly one frame.
//!
//! Listeners: `input.onKeyDown(fn)` and friends return a token for `input.off(token)` (the
//! TypeScript's `input.on(type, fn)` returned an unsubscribe function).

use crate::window::{self, DomainId};
use platypus_api::callback::{Listeners, emit};
use platypus_api::{Callback, platypus_api};
use platypus_math::V2;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

/// One gamepad. Buttons and axes by name, in the order they were first reported.
#[derive(Default, Clone)]
struct Pad {
    id: i64,
    name: String,
    connected: bool,
    buttons: Vec<(String, f64)>,
    axes: Vec<(String, f64)>,
}

/// A gamepad as scripts get it (`Gamepad`): the standard buttons and axes by name, 0 when it
/// does not report one (and for a pad that is gone).
#[derive(Clone, Debug, PartialEq)]
pub struct PadState {
    pub id: f64,
    pub name: String,
    pub connected: bool,
    pub buttons: Vec<(String, f64)>,
    pub axes: Vec<(String, f64)>,
}

/// What an `input.on` listener gets: the fields of its kind (`js/engine`'s `InputEvents`), the
/// others empty (a key event's `gamepad` is no pad, id -1).
#[derive(Clone, Debug, PartialEq)]
pub struct InputEvent {
    pub gamepad: PadState,
    /// `gamepadbuttondown` / `up`: the button's name (`South`).
    pub button: String,
    /// `gamepadaxis`: the axis (`LeftStickX`) and its value (-1 to 1).
    pub axis: String,
    pub value: f64,
    /// `keydown` / `keyup`: the physical key (`KeyW`), what it means in the layout (`w`), and
    /// whether it is the OS's auto-repeat.
    pub code: String,
    pub key: String,
    pub repeat: bool,
}

impl InputEvent {
    fn of_pad(gamepad: PadState) -> Self {
        InputEvent {
            gamepad,
            button: String::new(),
            axis: String::new(),
            value: 0.0,
            code: String::new(),
            key: String::new(),
            repeat: false,
        }
    }
}

/// Which event `input.on` listens to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InputEventType {
    #[default]
    KeyDown,
    KeyUp,
    Connected,
    Disconnected,
    ButtonDown,
    ButtonUp,
    Axis,
}

const STANDARD_BUTTONS: &[&str] = &[
    "South",
    "East",
    "West",
    "North",
    "C",
    "Z",
    "LeftBumper",
    "RightBumper",
    "LeftTrigger",
    "RightTrigger",
    "Select",
    "Start",
    "Mode",
    "LeftStick",
    "RightStick",
    "DPadUp",
    "DPadDown",
    "DPadLeft",
    "DPadRight",
];

const STANDARD_AXES: &[&str] = &[
    "LeftStickX",
    "LeftStickY",
    "RightStickX",
    "RightStickY",
    "LeftZ",
    "RightZ",
    "DPadX",
    "DPadY",
];

/// The pad `id` as scripts see it now.
fn pad_state(s: &State, id: i64) -> PadState {
    let pad = s.connected(id);
    let read = |list: Option<&Vec<(String, f64)>>, names: &[&str]| {
        names
            .iter()
            .map(|n| ((*n).to_string(), list.map_or(0.0, |l| value_of(l, n))))
            .collect()
    };
    PadState {
        id: id as f64,
        name: pad.map(|p| p.name.clone()).unwrap_or_default(),
        connected: pad.is_some(),
        buttons: read(pad.map(|p| &p.buttons), STANDARD_BUTTONS),
        axes: read(pad.map(|p| &p.axes), STANDARD_AXES),
    }
}

fn value_of(list: &[(String, f64)], name: &str) -> f64 {
    list.iter().find(|e| e.0 == name).map_or(0.0, |e| e.1)
}

fn set_value(list: &mut Vec<(String, f64)>, name: &str, v: f64) {
    match list.iter_mut().find(|e| e.0 == name) {
        Some(e) => e.1 = v,
        None => list.push((name.to_string(), v)),
    }
}

/// Unity's sensitivity and gravity for the default axes' keys, per second.
const AXIS_SPEED: f64 = 3.0;

#[derive(Default)]
struct State {
    /// The eased key values behind `getAxis("Horizontal")` and `("Vertical")`.
    eased: [f64; 2],
    keys: HashSet<String>,
    pressed_pending: HashSet<String>,
    released_pending: HashSet<String>,
    pressed: HashSet<String>,
    released: HashSet<String>,
    mouse: V2,
    delta_pending: (f64, f64),
    wheel_pending: (f64, f64),
    delta: (f64, f64),
    wheel: (f64, f64),
    pads: Vec<Pad>,
    /// The layout's text for the key event being delivered (`input.on`'s `key`).
    key_text: String,
    on_key_down: Listeners<(String, bool)>,
    on_key_up: Listeners<(String,)>,
    on_pad_connected: Listeners<(f64,)>,
    on_pad_disconnected: Listeners<(f64,)>,
    on_pad_button_down: Listeners<(f64, String)>,
    on_pad_button_up: Listeners<(f64, String)>,
    on_pad_axis: Listeners<(f64, String, f64)>,
}

impl State {
    fn pad(&mut self, id: i64, name: &str) -> &mut Pad {
        if let Some(i) = self.pads.iter().position(|p| p.id == id) {
            return &mut self.pads[i];
        }
        self.pads.push(Pad {
            id,
            name: name.to_string(),
            connected: true,
            ..Pad::default()
        });
        self.pads.last_mut().expect("just pushed")
    }

    fn connected(&self, id: i64) -> Option<&Pad> {
        self.pads.iter().find(|p| p.id == id && p.connected)
    }

    /// The keys of a virtual axis as -1, 0 or 1 (no easing).
    fn key_axis(&self, neg: &[&str], pos: &[&str]) -> f64 {
        let held = |ks: &[&str]| ks.iter().any(|k| self.keys.contains(*k));
        f64::from(u8::from(held(pos))) - f64::from(u8::from(held(neg)))
    }

    /// The left stick of the connected pad pushed furthest on `axis`.
    fn stick(&self, axis: &str) -> f64 {
        let mut stick = 0.0f64;
        for p in self.pads.iter().filter(|p| p.connected) {
            let v = value_of(&p.axes, axis);
            if v.abs() > stick.abs() {
                stick = v;
            }
        }
        stick
    }

    /// Eases the key values of Horizontal and Vertical towards their keys over `dt` seconds.
    fn ease_axes(&mut self, dt: f64) {
        for (i, name) in ["Horizontal", "Vertical"].into_iter().enumerate() {
            let Some((neg, pos, _)) = axis_def(name) else {
                continue;
            };
            let target = self.key_axis(neg, pos);
            let mut v = self.eased[i];
            let step = AXIS_SPEED * dt;
            if target != 0.0 {
                // Reversing starts again from 0.
                if v != 0.0 && v.signum() != target {
                    v = 0.0;
                }
                v = if (target - v).abs() <= step {
                    target
                } else {
                    v + (target - v).signum() * step
                };
            } else {
                v = if v.abs() <= step {
                    0.0
                } else {
                    v - v.signum() * step
                };
            }
            self.eased[i] = v;
        }
    }

    fn button_down_on_a_pad(&self, button: &str) -> bool {
        self.pads
            .iter()
            .filter(|p| p.connected)
            .any(|p| value_of(&p.buttons, button) > 0.5)
    }
}

thread_local! {
    /// One input state per event domain (one per window family); the main domain's is the one
    /// single-window games use. Script reads and the host's functions below act on the *current*
    /// domain (`window::current_domain()`, the main one outside any window code); the `_in` twins
    /// enter a domain first.
    static DOMAINS: RefCell<BTreeMap<u32, Rc<RefCell<State>>>> = const { RefCell::new(BTreeMap::new()) };
}

/// The current domain's state, as `STATE.with(|s| ..)` (`s` is its `RefCell`).
struct Store;
static STATE: Store = Store;

impl Store {
    fn with<R>(&self, f: impl FnOnce(&RefCell<State>) -> R) -> R {
        let d = window::current_domain().0;
        let state = DOMAINS.with(|m| m.borrow_mut().entry(d).or_default().clone());
        f(&state)
    }
}

/// Runs `f` with domain `d` current: input reads and the host's input functions then act on `d`'s
/// state (and its listeners run in `d`).
pub fn in_domain<R>(d: DomainId, f: impl FnOnce() -> R) -> R {
    let _g = window::enter_domain(d);
    f()
}

/// [`key_down`] for domain `d`.
pub fn key_down_in(d: DomainId, code: &str, repeat: bool) {
    in_domain(d, || key_down(code, repeat));
}

/// [`key_up_event`] for domain `d`.
pub fn key_up_in(d: DomainId, code: &str) {
    in_domain(d, || key_up_event(code));
}

/// [`mouse_button_event`] for domain `d`.
pub fn mouse_button_in(d: DomainId, button: usize, down: bool) {
    in_domain(d, || mouse_button_event(button, down));
}

/// [`mouse_move`] for domain `d`.
pub fn mouse_move_in(d: DomainId, x: f32, y: f32) {
    in_domain(d, || mouse_move(x, y));
}

/// [`mouse_delta`] for domain `d`.
pub fn mouse_delta_in(d: DomainId, dx: f64, dy: f64) {
    in_domain(d, || mouse_delta(dx, dy));
}

/// [`wheel`] for domain `d`.
pub fn wheel_in(d: DomainId, dx: f64, dy: f64) {
    in_domain(d, || wheel(dx, dy));
}

/// [`blur`] for domain `d`: its keys are let go.
pub fn blur_domain(d: DomainId) {
    in_domain(d, blur);
}

/// [`tick`] for domain `d`: what arrived since its last tick becomes "this frame" for it only.
pub fn tick_domain(d: DomainId) {
    in_domain(d, tick);
}

/// Gamepads have no window: the host sends their events to the focused domain (the main one when
/// none is focused) with these.
pub fn gamepad_connected_in(d: DomainId, id: i64, name: &str) {
    in_domain(d, || gamepad_connected(id, name));
}

pub fn gamepad_disconnected_in(d: DomainId, id: i64) {
    in_domain(d, || gamepad_disconnected(id));
}

pub fn gamepad_button_in(d: DomainId, id: i64, button: &str, value: f64) {
    in_domain(d, || gamepad_button(id, button, value));
}

pub fn gamepad_axis_in(d: DomainId, id: i64, axis: &str, value: f64) {
    in_domain(d, || gamepad_axis(id, axis, value));
}

/// A key went down (a repeat of a held key is not a new press).
pub fn key_down_event(code: &str) {
    key_down(code, false);
}

/// A key went down; `repeat` is the OS auto-repeat.
pub fn key_down(code: &str, repeat: bool) {
    key_down_with_text(code, "", repeat);
}

/// [`key_down`] with what the key means in the layout (`w` for `KeyW`), for `input.on`'s `key`.
pub fn key_down_with_text(code: &str, key: &str, repeat: bool) {
    let cbs = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.key_text = key.to_string();
        if !repeat && !s.keys.contains(code) {
            s.pressed_pending.insert(code.to_string());
        }
        s.keys.insert(code.to_string());
        s.on_key_down.snapshot()
    });
    emit(&cbs, (code.to_string(), repeat));
}

pub fn key_up_event(code: &str) {
    key_up_with_text(code, "");
}

/// [`key_up_event`] with the layout's text for the key.
pub fn key_up_with_text(code: &str, key: &str) {
    let cbs = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.key_text = key.to_string();
        if s.keys.remove(code) {
            s.released_pending.insert(code.to_string());
        }
        s.on_key_up.snapshot()
    });
    emit(&cbs, (code.to_string(),));
}

/// The name a mouse button has as a key: Unity's numbers 0 left, 1 right, 2 middle.
fn mouse_code(button: usize) -> String {
    match button {
        0 => "MouseLeft".into(),
        1 => "MouseRight".into(),
        2 => "MouseMiddle".into(),
        n => format!("Mouse{n}"),
    }
}

pub fn mouse_button_event(button: usize, down: bool) {
    if down {
        key_down(&mouse_code(button), false);
    } else {
        key_up_event(&mouse_code(button));
    }
}

pub fn mouse_move(x: f32, y: f32) {
    STATE.with(|s| {
        s.borrow_mut().mouse = V2 {
            x: f64::from(x),
            y: f64::from(y),
        }
    });
}

/// Raw mouse movement (it keeps coming while the cursor is locked).
pub fn mouse_delta(dx: f64, dy: f64) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.delta_pending.0 += dx;
        s.delta_pending.1 += dy;
    });
}

/// Wheel movement in lines.
pub fn wheel(dx: f64, dy: f64) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.wheel_pending.0 += dx;
        s.wheel_pending.1 += dy;
    });
}

/// The window lost focus: releases may never arrive.
pub fn blur() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.keys.clear();
        for p in &mut s.pads {
            for b in &mut p.buttons {
                b.1 = 0.0;
            }
        }
    });
}

pub fn gamepad_connected(id: i64, name: &str) {
    let cbs = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.pad(id, name).connected = true;
        s.on_pad_connected.snapshot()
    });
    emit(&cbs, (id as f64,));
}

pub fn gamepad_disconnected(id: i64) {
    let cbs = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.pad(id, "").connected = false;
        s.on_pad_disconnected.snapshot()
    });
    emit(&cbs, (id as f64,));
}

pub fn gamepad_button(id: i64, button: &str, value: f64) {
    let (was, down, up) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let pad = s.pad(id, "");
        let was = value_of(&pad.buttons, button) > 0.5;
        set_value(&mut pad.buttons, button, value);
        (
            was,
            s.on_pad_button_down.snapshot(),
            s.on_pad_button_up.snapshot(),
        )
    });
    let now = value > 0.5;
    if now && !was {
        emit(&down, (id as f64, button.to_string()));
    } else if !now && was {
        emit(&up, (id as f64, button.to_string()));
    }
}

pub fn gamepad_axis(id: i64, axis: &str, value: f64) {
    let cbs = STATE.with(|s| {
        let mut s = s.borrow_mut();
        set_value(&mut s.pad(id, "").axes, axis, value);
        s.on_pad_axis.snapshot()
    });
    emit(&cbs, (id as f64, axis.to_string(), value));
}

/// Subscribes a Rust function to key presses (the host, or a test); returns the token for [`off`].
pub fn on_key_down(f: impl Fn(&str, bool) + 'static) -> u32 {
    STATE.with(|s| {
        s.borrow_mut()
            .on_key_down
            .add(Callback::from_fn(move |(c, r): (String, bool)| f(&c, r)))
    })
}

/// Removes any listener by its token; false when it is unknown.
pub fn off(token: u32) -> bool {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.on_key_down.remove(token)
            | s.on_key_up.remove(token)
            | s.on_pad_connected.remove(token)
            | s.on_pad_disconnected.remove(token)
            | s.on_pad_button_down.remove(token)
            | s.on_pad_button_up.remove(token)
            | s.on_pad_axis.remove(token)
    })
}

/// Starts a frame: what arrived since the last tick is what happened "this frame".
pub fn tick() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.pressed = std::mem::take(&mut s.pressed_pending);
        s.released = std::mem::take(&mut s.released_pending);
        s.delta = std::mem::take(&mut s.delta_pending);
        s.wheel = std::mem::take(&mut s.wheel_pending);
        s.ease_axes(crate::time::state().unscaled_delta_time);
    });
}

/// Forgets everything, every domain's (a test; a reload).
pub fn reset() {
    // Dropped outside the borrow: listeners are closures that may hold anything.
    let old = DOMAINS.with(|m| std::mem::take(&mut *m.borrow_mut()));
    drop(old);
}

/// Keys and pad parts of the virtual axes (Unity's defaults): negative keys, positive keys, the
/// stick axis.
fn axis_def(
    name: &str,
) -> Option<(
    &'static [&'static str],
    &'static [&'static str],
    &'static str,
)> {
    match name {
        "Horizontal" => Some((
            &["KeyA", "ArrowLeft"],
            &["KeyD", "ArrowRight"],
            "LeftStickX",
        )),
        "Vertical" => Some((&["KeyS", "ArrowDown"], &["KeyW", "ArrowUp"], "LeftStickY")),
        _ => None,
    }
}

/// An axis' value, with the keys eased or not.
fn axis_value(name: &str, eased: bool) -> f64 {
    STATE.with(|s| {
        let s = s.borrow();
        match name {
            "Mouse X" => return s.delta.0 * MOUSE_SENSITIVITY,
            "Mouse Y" => return -s.delta.1 * MOUSE_SENSITIVITY,
            "Mouse ScrollWheel" => return s.wheel.1 * MOUSE_SENSITIVITY,
            _ => {}
        }
        let Some((neg, pos, stick_axis)) = axis_def(name) else {
            return 0.0;
        };
        let key_value = if eased {
            s.eased[usize::from(name == "Vertical")]
        } else {
            s.key_axis(neg, pos)
        };
        let stick = s.stick(stick_axis);
        if stick.abs() > key_value.abs() {
            stick
        } else {
            key_value
        }
    })
}

/// Virtual buttons: keys and gamepad buttons.
fn button_def(name: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    match name {
        "Jump" => Some((&["Space"], &["South"])),
        "Fire1" => Some((&["ControlLeft", "MouseLeft"], &["RightTrigger"])),
        "Fire2" => Some((&["AltLeft", "MouseRight"], &["LeftTrigger"])),
        "Submit" => Some((&["Enter", "Space"], &["South"])),
        "Cancel" => Some((&["Escape"], &["East"])),
        _ => None,
    }
}

/// Raw mouse movement per "Mouse X" / "Mouse Y" unit (Unity's default sensitivity).
const MOUSE_SENSITIVITY: f64 = 0.1;

/// Keyboard, mouse and gamepads. Input belongs to the window's event domain: it reads the keys of the window
/// family whose code is running; main-game components always read the main window's, an isolated window's keys
/// never reach it (send a message), and focus loss clears held keys. See docs/windows.md.
#[platypus_api(
    namespace = "input", values(Vector2 = V2 [x: f64, y: f64]),
    enums(
        /// The events `input.on` listens to (`js/engine`'s `keyof InputEvents`).
        InputEventType = InputEventType [
            KeyDown = "keydown",
            KeyUp = "keyup",
            Connected = "gamepadconnected",
            Disconnected = "gamepaddisconnected",
            ButtonDown = "gamepadbuttondown",
            ButtonUp = "gamepadbuttonup",
            Axis = "gamepadaxis"
        ]
    ),
    records(
        /// A gamepad: its buttons (0 to 1) and axes (-1 to 1) by name; a standard name it does not report is 0.
        Gamepad = PadState [
            id: f64,
            name: string,
            connected: bool,
            buttons: dict,
            axes: dict
        ],
        /// What an `input.on` listener gets: the fields of its kind (`code`, `key` and `repeat` for
        /// keys; `gamepad`, and `button` or `axis` and `value`, for pads), the others empty.
        InputEvent = InputEvent [
            gamepad: Gamepad,
            button: string,
            axis: string,
            value: f64,
            code: string,
            key: string,
            repeat: bool
        ]
    )
)]
pub mod input_api {
    use super::{
        InputEvent, InputEventType, PadState, STATE, axis_value, button_def, mouse_code, pad_state,
        value_of,
    };
    use platypus_api::Callback;
    use platypus_math::V2;

    /// Every connected gamepad.
    pub fn gamepads() -> Vec<PadState> {
        STATE.with(|s| {
            let s = s.borrow();
            s.pads
                .iter()
                .filter(|p| p.connected)
                .map(|p| pad_state(&s, p.id))
                .collect()
        })
    }

    /// Listens for an input event, as `js/engine`'s `input.on`: `keydown` / `keyup` (`{ code, key,
    /// repeat }`, mouse buttons too as `MouseLeft` ...), `gamepadconnected` / `gamepaddisconnected`
    /// (`{ gamepad }`), `gamepadbuttondown` / `up` (`{ gamepad, button }`) and `gamepadaxis`
    /// (`{ gamepad, axis, value }`); returns a function that stops listening. The callback gets the
    /// event as JSON text, which the compiler parses.
    #[subscription(off = "off", json = "InputEvent")]
    pub fn on(kind: InputEventType, f: Callback<(String,)>) -> u32 {
        let send = move |e: InputEvent| f.call((__to_json_InputEvent(&e).to_string(),));
        let pad = |id: f64| STATE.with(|s| pad_state(&s.borrow(), id as i64));
        let key_text = || STATE.with(|s| s.borrow().key_text.clone());
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            match kind {
                InputEventType::KeyDown => {
                    s.on_key_down
                        .add(Callback::from_fn(move |(code, repeat): (String, bool)| {
                            send(InputEvent {
                                code,
                                key: key_text(),
                                repeat,
                                ..InputEvent::of_pad(pad(-1.0))
                            })
                        }))
                }
                InputEventType::KeyUp => {
                    s.on_key_up
                        .add(Callback::from_fn(move |(code,): (String,)| {
                            send(InputEvent {
                                code,
                                key: key_text(),
                                ..InputEvent::of_pad(pad(-1.0))
                            })
                        }))
                }
                InputEventType::Connected | InputEventType::Disconnected => {
                    let cb =
                        Callback::from_fn(move |(id,): (f64,)| send(InputEvent::of_pad(pad(id))));
                    if kind == InputEventType::Connected {
                        s.on_pad_connected.add(cb)
                    } else {
                        s.on_pad_disconnected.add(cb)
                    }
                }
                InputEventType::ButtonDown | InputEventType::ButtonUp => {
                    let cb = Callback::from_fn(move |(id, button): (f64, String)| {
                        send(InputEvent {
                            button,
                            ..InputEvent::of_pad(pad(id))
                        })
                    });
                    if kind == InputEventType::ButtonDown {
                        s.on_pad_button_down.add(cb)
                    } else {
                        s.on_pad_button_up.add(cb)
                    }
                }
                InputEventType::Axis => s.on_pad_axis.add(Callback::from_fn(
                    move |(id, axis, value): (f64, String, f64)| {
                        send(InputEvent {
                            axis,
                            value,
                            ..InputEvent::of_pad(pad(id))
                        })
                    },
                )),
            }
        })
    }

    /// Calls `f(code, repeat)` when a key goes down; returns a token for `off`.
    pub fn on_key_down(f: Callback<(String, bool)>) -> u32 {
        STATE.with(|s| s.borrow_mut().on_key_down.add(f))
    }

    /// Calls `f(code)` when a key goes up; returns a token for `off`.
    pub fn on_key_up(f: Callback<(String,)>) -> u32 {
        STATE.with(|s| s.borrow_mut().on_key_up.add(f))
    }

    /// Calls `f(id)` when a gamepad connects.
    pub fn on_gamepad_connected(f: Callback<(f64,)>) -> u32 {
        STATE.with(|s| s.borrow_mut().on_pad_connected.add(f))
    }

    /// Calls `f(id)` when a gamepad disconnects.
    pub fn on_gamepad_disconnected(f: Callback<(f64,)>) -> u32 {
        STATE.with(|s| s.borrow_mut().on_pad_disconnected.add(f))
    }

    /// Calls `f(id, button)` when a gamepad button goes down.
    pub fn on_gamepad_button_down(f: Callback<(f64, String)>) -> u32 {
        STATE.with(|s| s.borrow_mut().on_pad_button_down.add(f))
    }

    /// Calls `f(id, button)` when a gamepad button goes up.
    pub fn on_gamepad_button_up(f: Callback<(f64, String)>) -> u32 {
        STATE.with(|s| s.borrow_mut().on_pad_button_up.add(f))
    }

    /// Calls `f(id, axis, value)` when a gamepad axis moves.
    pub fn on_gamepad_axis(f: Callback<(f64, String, f64)>) -> u32 {
        STATE.with(|s| s.borrow_mut().on_pad_axis.add(f))
    }

    /// Stops a listener given by one of the `on...` functions (the TypeScript's `on` returned an
    /// unsubscribe function).
    pub fn off(token: u32) -> bool {
        super::off(token)
    }

    /// Whether a key (by physical `code`, e.g. `"KeyW"`) is held.
    pub fn is_key_down(code: &str) -> bool {
        STATE.with(|s| s.borrow().keys.contains(code))
    }

    /// Unity's `GetKey`: held.
    pub fn get_key(code: &str) -> bool {
        STATE.with(|s| s.borrow().keys.contains(code))
    }

    /// Unity's `GetKeyDown`: pressed since the last tick (true for one frame).
    pub fn get_key_down(code: &str) -> bool {
        STATE.with(|s| s.borrow().pressed.contains(code))
    }

    /// Unity's `GetKeyUp`: released since the last tick.
    pub fn get_key_up(code: &str) -> bool {
        STATE.with(|s| s.borrow().released.contains(code))
    }

    /// Where the mouse is, in UI pixels from the top-left.
    #[property]
    pub fn mouse_position() -> V2 {
        STATE.with(|s| s.borrow().mouse)
    }

    /// How far the mouse moved this frame in raw device units, right and up positive. It keeps
    /// coming while the cursor is locked: use it for mouse look.
    #[property]
    pub fn mouse_delta() -> V2 {
        STATE.with(|s| {
            let d = s.borrow().delta;
            V2 {
                x: d.0,
                y: 0.0 - d.1,
            }
        })
    }

    /// Wheel movement this frame, in lines: y up (away from you) positive.
    #[property]
    pub fn mouse_scroll_delta() -> V2 {
        STATE.with(|s| {
            let w = s.borrow().wheel;
            V2 { x: w.0, y: w.1 }
        })
    }

    /// Whether mouse `button` (0 left, 1 right, 2 middle) is held.
    pub fn get_mouse_button(button: u32) -> bool {
        STATE.with(|s| s.borrow().keys.contains(&mouse_code(button as usize)))
    }

    /// Whether mouse `button` went down this frame.
    pub fn get_mouse_button_down(button: u32) -> bool {
        STATE.with(|s| s.borrow().pressed.contains(&mouse_code(button as usize)))
    }

    /// Whether mouse `button` came up this frame.
    pub fn get_mouse_button_up(button: u32) -> bool {
        STATE.with(|s| s.borrow().released.contains(&mouse_code(button as usize)))
    }

    /// Unity's `GetAxis` for `"Horizontal"` (A/D, arrows, left stick X), `"Vertical"` (S/W, arrows,
    /// left stick Y), `"Mouse X"`, `"Mouse Y"` and `"Mouse ScrollWheel"`: -1 to 1. The keys ease in
    /// and out as Unity's default axes do (sensitivity 3 and gravity 3 per second, and pressing the
    /// opposite key restarts from 0), so the value takes a third of a second to reach 1; the stick
    /// is read as it is and wins when pushed further. `getAxisRaw` has no easing.
    pub fn get_axis(name: &str) -> f64 {
        axis_value(name, true)
    }

    /// `getAxis` without easing: the keys are -1, 0 or 1 at once (Unity's `GetAxisRaw`).
    pub fn get_axis_raw(name: &str) -> f64 {
        axis_value(name, false)
    }

    /// Unity's `GetButton` for `"Jump"`, `"Fire1"`, `"Fire2"`, `"Submit"`, `"Cancel"`: keys or
    /// gamepad buttons.
    pub fn get_button(name: &str) -> bool {
        STATE.with(|s| {
            let s = s.borrow();
            button_def(name).is_some_and(|(keys, pad)| {
                keys.iter().any(|k| s.keys.contains(*k))
                    || pad.iter().any(|b| s.button_down_on_a_pad(b))
            })
        })
    }

    /// Pressed since the last tick (keys only for now).
    pub fn get_button_down(name: &str) -> bool {
        STATE.with(|s| {
            let s = s.borrow();
            button_def(name).is_some_and(|(keys, _)| keys.iter().any(|k| s.pressed.contains(*k)))
        })
    }

    /// How many gamepads are connected.
    pub fn gamepad_count() -> f64 {
        STATE.with(|s| s.borrow().pads.iter().filter(|p| p.connected).count() as f64)
    }

    /// The id of the i-th connected gamepad (-1 past the end).
    pub fn gamepad_id(index: f64) -> f64 {
        STATE.with(|s| {
            s.borrow()
                .pads
                .iter()
                .filter(|p| p.connected)
                .nth(index as usize)
                .map_or(-1.0, |p| p.id as f64)
        })
    }

    /// Whether a gamepad is connected.
    pub fn gamepad_connected(id: f64) -> bool {
        STATE.with(|s| s.borrow().connected(id as i64).is_some())
    }

    /// A connected gamepad's name ("" if none).
    pub fn gamepad_name(id: f64) -> String {
        STATE.with(|s| {
            s.borrow()
                .connected(id as i64)
                .map(|p| p.name.clone())
                .unwrap_or_default()
        })
    }

    /// A button's value on a connected gamepad, 0 to 1 (`South`, `East`, `West`, `North`,
    /// `LeftBumper`, `RightBumper`, `LeftTrigger`, `RightTrigger`, `Select`, `Start`, `Mode`,
    /// `LeftStick`, `RightStick`, `DPadUp/Down/Left/Right`); 0 when there is none.
    pub fn gamepad_button(id: f64, button: &str) -> f64 {
        STATE.with(|s| {
            s.borrow()
                .connected(id as i64)
                .map_or(0.0, |p| value_of(&p.buttons, button))
        })
    }

    /// An axis' value on a connected gamepad, -1 to 1, up and right positive (`LeftStickX/Y`,
    /// `RightStickX/Y`, ...); 0 when there is none.
    pub fn gamepad_axis(id: f64, axis: &str) -> f64 {
        STATE.with(|s| {
            s.borrow()
                .connected(id as i64)
                .map_or(0.0, |p| value_of(&p.axes, axis))
        })
    }

    /// Whether `button` is held on any connected gamepad.
    pub fn is_button_down(button: &str) -> bool {
        STATE.with(|s| s.borrow().button_down_on_a_pad(button))
    }
}
