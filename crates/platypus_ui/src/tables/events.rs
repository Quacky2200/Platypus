// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! UI events as a script sees them: the fields (and where they come from in the event's number and
//! string lists), the event interfaces and the `on*` props.
//!
//! The slots index `UiEvent::script_args()` (`platypus_ui::events`): `data` is `[x, y, button, buttons,
//! detail, deltaX, deltaY, modifiers, repeat, related, scrollLeft, scrollTop, scrollWidth,
//! scrollHeight, clientWidth, clientHeight, selectionStart, selectionEnd]`, `strings` is `[key, code]` for keyboard events and
//! `[value]` for input events. `crates/platypus_ui/tests/ui_tables.rs` checks the slots against it.

/// Where a field's value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Init {
    /// The event type (`"click"`).
    Kind,
    /// The target (`{ id, type, value }`).
    Target,
    /// `data[i]`.
    Num(usize),
    /// `data[i] & mask != 0`.
    Flag(usize, u32),
    /// `data[i] == 1`.
    One(usize),
    /// `strings[i]`.
    Str(usize),
    /// A constant (TypeScript expression text).
    Const(&'static str),
    /// The element that lost or gained focus (`data[9]`, 0 for none) as `{ id }`, or null.
    Related,
    /// The drag's `PlatypusDataTransfer` (one object from `dragstart` to `dragend`; the dragged files
    /// are the event's strings).
    Transfer,
    /// `dragend` of a drag that was dropped somewhere that accepted it (`detail` is 1).
    Dropped,
    /// The element whose handler runs (`currentTarget`), as a `PlatypusElement`.
    CurrentTarget,
    /// Whether a handler called `preventDefault()` so far: read when asked (a getter), since a
    /// handler can prevent it after the event object was made.
    Prevented,
}

#[derive(Clone, Copy, Debug)]
pub struct EventField {
    pub name: &'static str,
    /// The TypeScript type.
    pub ts: &'static str,
    pub init: Init,
}

const fn f(name: &'static str, ts: &'static str, init: Init) -> EventField {
    EventField { name, ts, init }
}

use Init::*;

/// Every field any event interface has, in the order the compiled `UiEvent` class declares them.
pub const EVENT_FIELDS: &[EventField] = &[
    f("type", "string", Kind),
    f(
        "target",
        "{ readonly id: number; readonly value: string; readonly type: string }",
        Target,
    ),
    f("currentTarget", "PlatypusElement", CurrentTarget),
    f("defaultPrevented", "boolean", Prevented),
    f("clientX", "number", Num(0)),
    f("clientY", "number", Num(1)),
    f("x", "number", Num(0)),
    f("y", "number", Num(1)),
    f("button", "number", Num(2)),
    f("buttons", "number", Num(3)),
    f("detail", "number", Num(4)),
    f("deltaX", "number", Num(5)),
    f("deltaY", "number", Num(6)),
    f("shiftKey", "boolean", Flag(7, 1)),
    f("ctrlKey", "boolean", Flag(7, 2)),
    f("altKey", "boolean", Flag(7, 4)),
    f("metaKey", "boolean", Flag(7, 8)),
    f("repeat", "boolean", One(8)),
    f("scrollLeft", "number", Num(10)),
    f("scrollTop", "number", Num(11)),
    f("scrollWidth", "number", Num(12)),
    f("scrollHeight", "number", Num(13)),
    f("clientWidth", "number", Num(14)),
    f("clientHeight", "number", Num(15)),
    f("selectionStart", "number", Num(16)),
    f("selectionEnd", "number", Num(17)),
    f("pointerId", "number", Const("1")),
    f("key", "string", Str(0)),
    f("code", "string", Str(1)),
    f("value", "string", Str(0)),
    f("relatedTarget", "{ readonly id: number } | null", Related),
    f("propertyName", "string", Str(0)),
    f("animationName", "string", Str(0)),
    f("elapsedTime", "number", Num(5)),
    f("pseudoElement", "string", Const("\"\"")),
    f("dataTransfer", "PlatypusDataTransfer", Transfer),
    f("dropped", "boolean", Dropped),
];

/// The methods every event has, with the `Ui` call the compiled class makes.
pub const EVENT_METHODS: &[(&str, &str)] = &[
    ("stopPropagation", "Ui.evStop()"),
    ("preventDefault", "Ui.evPrevent()"),
];

#[derive(Clone, Copy, Debug)]
pub struct EventInterface {
    pub name: &'static str,
    pub extends: Option<&'static str>,
    /// Names from [`EVENT_FIELDS`].
    pub fields: &'static [&'static str],
}

pub const EVENT_INTERFACES: &[EventInterface] = &[
    EventInterface {
        name: "PlatypusEvent",
        extends: None,
        fields: &["type", "target", "currentTarget", "defaultPrevented"],
    },
    EventInterface {
        name: "PlatypusPointerEvent",
        extends: Some("PlatypusEvent"),
        fields: &[
            "clientX",
            "clientY",
            "x",
            "y",
            "button",
            "buttons",
            "detail",
            "shiftKey",
            "ctrlKey",
            "altKey",
            "metaKey",
            "pointerId",
        ],
    },
    EventInterface {
        name: "PlatypusWheelEvent",
        extends: Some("PlatypusPointerEvent"),
        fields: &["deltaX", "deltaY"],
    },
    EventInterface {
        name: "PlatypusKeyboardEvent",
        extends: Some("PlatypusEvent"),
        fields: &[
            "key",
            "code",
            "repeat",
            "shiftKey",
            "ctrlKey",
            "altKey",
            "metaKey",
            "selectionStart",
            "selectionEnd",
        ],
    },
    EventInterface {
        name: "PlatypusInputEvent",
        extends: Some("PlatypusEvent"),
        fields: &["value"],
    },
    EventInterface {
        name: "PlatypusFocusEvent",
        extends: Some("PlatypusEvent"),
        fields: &["relatedTarget"],
    },
    EventInterface {
        name: "PlatypusTransitionEvent",
        extends: Some("PlatypusEvent"),
        fields: &["propertyName", "elapsedTime", "pseudoElement"],
    },
    EventInterface {
        name: "PlatypusAnimationEvent",
        extends: Some("PlatypusEvent"),
        fields: &["animationName", "elapsedTime", "pseudoElement"],
    },
    EventInterface {
        name: "PlatypusDragEvent",
        extends: Some("PlatypusPointerEvent"),
        fields: &["dataTransfer", "dropped"],
    },
    EventInterface {
        name: "PlatypusScrollEvent",
        extends: Some("PlatypusEvent"),
        fields: &[
            "scrollLeft",
            "scrollTop",
            "scrollWidth",
            "scrollHeight",
            "clientWidth",
            "clientHeight",
        ],
    },
];

/// An `on*` prop: the event type (`EventKind::name`) it listens to and the interface of its argument.
#[derive(Clone, Copy, Debug)]
pub struct EventProp {
    pub prop: &'static str,
    pub kind: &'static str,
    pub interface: &'static str,
}

const fn ep(prop: &'static str, kind: &'static str, interface: &'static str) -> EventProp {
    EventProp {
        prop,
        kind,
        interface,
    }
}

/// The event props. `onChange` is React's name for `input`; `onFocus` / `onBlur` bubble, so they
/// listen to `focusin` / `focusout`. A `...Capture` suffix (`onClickCapture`) listens in the capture
/// phase.
pub const EVENT_PROPS: &[EventProp] = &[
    ep("onClick", "click", "PlatypusPointerEvent"),
    ep("onDoubleClick", "dblclick", "PlatypusPointerEvent"),
    ep("onContextMenu", "contextmenu", "PlatypusPointerEvent"),
    ep("onPointerDown", "pointerdown", "PlatypusPointerEvent"),
    ep("onPointerUp", "pointerup", "PlatypusPointerEvent"),
    ep("onPointerMove", "pointermove", "PlatypusPointerEvent"),
    ep("onPointerEnter", "pointerenter", "PlatypusPointerEvent"),
    ep("onPointerLeave", "pointerleave", "PlatypusPointerEvent"),
    ep(
        "onGotPointerCapture",
        "gotpointercapture",
        "PlatypusPointerEvent",
    ),
    ep(
        "onLostPointerCapture",
        "lostpointercapture",
        "PlatypusPointerEvent",
    ),
    ep("onMouseDown", "pointerdown", "PlatypusPointerEvent"),
    ep("onMouseUp", "pointerup", "PlatypusPointerEvent"),
    ep("onMouseMove", "pointermove", "PlatypusPointerEvent"),
    ep("onMouseEnter", "pointerenter", "PlatypusPointerEvent"),
    ep("onMouseLeave", "pointerleave", "PlatypusPointerEvent"),
    ep("onWheel", "wheel", "PlatypusWheelEvent"),
    ep("onKeyDown", "keydown", "PlatypusKeyboardEvent"),
    ep("onKeyUp", "keyup", "PlatypusKeyboardEvent"),
    ep("onChange", "input", "PlatypusInputEvent"),
    ep("onInput", "input", "PlatypusInputEvent"),
    ep("onFocus", "focusin", "PlatypusFocusEvent"),
    ep("onBlur", "focusout", "PlatypusFocusEvent"),
    ep("onScroll", "scroll", "PlatypusScrollEvent"),
    ep("onCancel", "cancel", "PlatypusEvent"),
    ep("onClose", "close", "PlatypusEvent"),
    ep(
        "onTransitionRun",
        "transitionrun",
        "PlatypusTransitionEvent",
    ),
    ep(
        "onTransitionStart",
        "transitionstart",
        "PlatypusTransitionEvent",
    ),
    ep(
        "onTransitionEnd",
        "transitionend",
        "PlatypusTransitionEvent",
    ),
    ep(
        "onTransitionCancel",
        "transitioncancel",
        "PlatypusTransitionEvent",
    ),
    ep(
        "onAnimationStart",
        "animationstart",
        "PlatypusAnimationEvent",
    ),
    ep(
        "onAnimationIteration",
        "animationiteration",
        "PlatypusAnimationEvent",
    ),
    ep("onAnimationEnd", "animationend", "PlatypusAnimationEvent"),
    ep(
        "onAnimationCancel",
        "animationcancel",
        "PlatypusAnimationEvent",
    ),
    ep("onDragStart", "dragstart", "PlatypusDragEvent"),
    ep("onDrag", "drag", "PlatypusDragEvent"),
    ep("onDragEnter", "dragenter", "PlatypusDragEvent"),
    ep("onDragOver", "dragover", "PlatypusDragEvent"),
    ep("onDragLeave", "dragleave", "PlatypusDragEvent"),
    ep("onDrop", "drop", "PlatypusDragEvent"),
    ep("onDragEnd", "dragend", "PlatypusDragEvent"),
];

/// Event types that bubble in the DOM (and in React): the others reach only their target (and the
/// capture phase above it).
pub const NON_BUBBLING: &[&str] = &["pointerenter", "pointerleave", "scroll", "cancel", "close"];

/// Event types an engine can send that have no `on*` prop, with the reason.
pub const NOT_EXPOSED: &[(&str, &str)] = &[];

/// What `dataTransfer` of a drag event is: declared next to the event interfaces and written as the
/// compiled `UiDataTransfer` class by `tools/platypus_ts` (`ui_dts.rs`).
pub const DATA_TRANSFER_TS: &str = "interface PlatypusDroppedFile {
  /** The full path on this machine. */
  readonly path: string;
  /** The file (or folder) name. */
  readonly name: string;
}

/** What a drag carries, like the DOM's `DataTransfer`. */
interface PlatypusDataTransfer {
  /** Set in `dragover` to say what a drop would do. */
  dropEffect: \"none\" | \"copy\" | \"move\" | \"link\";
  effectAllowed: \"none\" | \"copy\" | \"move\" | \"link\" | \"copyMove\" | \"copyLink\" | \"linkMove\" | \"all\" | \"uninitialized\";
  /** Files dragged in from the system. */
  readonly files: PlatypusDroppedFile[];
  readonly types: string[];
  setData(format: string, data: string): void;
  getData(format: string): string;
  clearData(format?: string): void;
  /** Shows `element` under the pointer while dragging, held at (x, y) inside it; null shows none. */
  setDragImage(element: { readonly id: number } | null, x?: number, y?: number): void;
}
";

/// The event type and capture flag of an `on*` prop name (`onClickCapture` listens to `click` while
/// capturing).
pub fn kind_of_prop(prop: &str) -> Option<(&'static str, bool)> {
    // Exact first: `onGotPointerCapture` ends in "Capture" without being a capture-phase prop.
    if let Some(p) = EVENT_PROPS.iter().find(|p| p.prop == prop) {
        return Some((p.kind, false));
    }
    let base = prop.strip_suffix("Capture")?;
    EVENT_PROPS
        .iter()
        .find(|p| p.prop == base)
        .map(|p| (p.kind, true))
}
