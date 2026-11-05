// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The tables in `platypus_ui::tables` against what this crate implements: every style property, attribute,
//! element and event the tables declare (and so the compiler writes into `types/platypus.d.ts`) is
//! handled, and what the code handles is declared.

use crate::common;

use common::Dom;
use platypus_ui::events::Modifiers;
use platypus_ui::tables::attrs::{ATTRS, Scope};
use platypus_ui::tables::elements::{ELEMENTS, SVG_KINDS, tag_of_component};
use platypus_ui::tables::events::{
    EVENT_FIELDS, EVENT_INTERFACES, EVENT_PROPS, Init, NON_BUBBLING, NOT_EXPOSED, kind_of_prop,
};
use platypus_ui::tables::style::{
    CURSORS, LENGTH_UNITS, NAMED_TYPES, SHORTHANDS, STYLE_PROPS, StyleProp,
};
use platypus_ui::{Attrs, EventKind, LayoutStyle, StyleError, UiEvent, Visual, apply_style};
use std::collections::BTreeSet;
use web_ui_core::Value;

fn apply(prop: &str, v: Value) -> Result<(), StyleError> {
    let mut s = LayoutStyle::default();
    let mut vis = Visual::default();
    apply_style(&mut s, &mut vis, prop, &v)
}

fn unknown(r: &Result<(), StyleError>) -> bool {
    matches!(r, Err(StyleError::UnknownProperty(_)))
}

/// Strings that are valid for some free-text property; one of them must apply.
const SAMPLES: &[&str] = &[
    "red",
    "10px",
    "1px solid red",
    "center",
    "auto",
    "none",
    "1fr 1fr",
    "row",
    "1 / 2",
    "span 2",
    "linear-gradient(red, blue)",
    "url(a.png)",
    "blur(2px)",
    "0 0 4px red",
    "rotate(10deg)",
    "all 1s",
    "1s",
    "ease",
    "spin",
    "0.5",
    "10px 10px",
    "\"a b\" \"c d\"",
    "bold",
    "underline",
    "disc",
    "alpha",
    "inset(10px)",
    "start",
    "ss01",
    "tabular-nums",
    "stretch",
    "normal",
    "contain",
    "repeat",
    "border-box",
    "pixelated",
    "fill",
    "0 0",
    "1",
];

// M9-TS-3301
#[test]
fn every_style_property_in_the_table_is_applied() {
    let mut problems = Vec::new();
    for p in STYLE_PROPS {
        let mut ok = false;
        for kw in p.keywords() {
            let r = apply(p.name, kw.into());
            if r.is_err() {
                problems.push(format!("{} = {kw:?}: {r:?}", p.name));
            }
            ok = true;
        }
        if p.accepts_number() {
            // A number is pixels for a length.
            let r = apply(p.name, 1.0.into());
            if r.is_err() {
                problems.push(format!("{} = 1: {r:?}", p.name));
            }
            ok = true;
        }
        if p.accepts_string() && !ok {
            if !SAMPLES.iter().any(|s| apply(p.name, (*s).into()).is_ok()) {
                problems.push(format!("no sample string applies to {}", p.name));
            }
            ok = true;
        }
        if !ok {
            problems.push(format!("{} takes no value", p.name));
        }
        // A property that is in the table is known: resetting it is accepted.
        if unknown(&apply(p.name, Value::Null)) {
            problems.push(format!("{} is unknown to apply_style", p.name));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The names `apply_style` matches on, read from the source: the `"camelCase" =>` arms.
fn implemented_names() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for src in [
        include_str!("../src/style.rs"),
        include_str!("../src/text_style.rs"),
        include_str!("../src/paint.rs"),
    ] {
        for line in src.lines() {
            let t = line.trim_start();
            if !t.starts_with('"') {
                continue;
            }
            // `"a" | "b" =>` or `"a" if ..`.
            let head = t.split("=>").next().unwrap_or("");
            if !t.contains("=>") {
                continue;
            }
            for part in head.split('|') {
                let part = part.trim().trim_end_matches(',');
                let name = part.trim_matches('"');
                let camel = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                    && name.chars().all(|c| c.is_ascii_alphanumeric())
                    && (name.starts_with("Webkit")
                        || name.chars().skip(1).any(|c| c.is_ascii_uppercase()));
                if part.starts_with('"') && part.ends_with('"') && camel {
                    out.insert(name.to_owned());
                }
            }
        }
    }
    out
}

/// Words of the form `"fooBar" =>` that are values, not properties (colour and font names, ...).
const NOT_PROPERTIES: &[&str] = &[
    "Color",
    "Style",
    "currentColor",
    "currentcolor",
    "lineThrough",
    "fromFont",
    "pixelatedZ",
];

// M9-TS-3302
#[test]
fn everything_apply_style_matches_on_is_in_the_table() {
    let table: BTreeSet<&str> = STYLE_PROPS.iter().map(|p| p.name).collect();
    let missing: Vec<_> = implemented_names()
        .into_iter()
        .filter(|n| !table.contains(n.as_str()) && !NOT_PROPERTIES.contains(&n.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "apply_style handles these but the table in platypus_ui::tables::style does not declare them (add them with a type and a doc, or list them in NOT_PROPERTIES if they are values): {missing:?}"
    );
}

// M9-TS-3303
#[test]
fn the_names_in_the_table_are_unique_and_the_named_unions_apply() {
    let mut seen = BTreeSet::new();
    for p in STYLE_PROPS {
        assert!(seen.insert(p.name), "{} is listed twice", p.name);
        assert!(!p.name.is_empty());
        for part in p.ty {
            if let platypus_ui::tables::style::Part::Named(n) = part {
                assert!(
                    NAMED_TYPES.iter().any(|(k, _)| k == n),
                    "{} names the unknown type {n}",
                    p.name
                );
            }
        }
    }
    for kw in CURSORS {
        assert!(apply("cursor", (*kw).into()).is_ok(), "cursor: {kw}");
    }
    assert_eq!(platypus_ui::CURSORS, CURSORS);
}

// M9-TS-3304
#[test]
fn lengths_take_numbers_and_the_listed_units_and_shorthands_set_longhands() {
    let table: BTreeSet<&str> = STYLE_PROPS.iter().map(|p| p.name).collect();
    // Relative and absolute units are resolved by the tree (`units.rs`) before `apply_style`.
    let mut d = Dom::new();
    for unit in LENGTH_UNITS {
        d.add(0, "box", &[("width", format!("10{unit}").into())]);
    }
    let _tree = d.build("");
    assert!(apply("width", 10.0.into()).is_ok());
    for (short, longs) in SHORTHANDS {
        assert!(table.contains(short), "{short}");
        let _: &StyleProp = STYLE_PROPS.iter().find(|p| p.name == *short).unwrap();
        // The shorthand, then each longhand on the same style, all apply.
        let mut s = LayoutStyle::default();
        let mut vis = Visual::default();
        apply_style(&mut s, &mut vis, short, &a_value(short))
            .unwrap_or_else(|e| panic!("{short}: {e}"));
        for l in *longs {
            assert!(table.contains(l), "{l} is not in the table");
            apply_style(&mut s, &mut vis, l, &a_value(l)).unwrap_or_else(|e| panic!("{l}: {e}"));
        }
    }
}

/// A value `prop` accepts: a keyword of its type, else a number, else the first sample string.
fn a_value(prop: &str) -> Value {
    let p = STYLE_PROPS.iter().find(|p| p.name == prop).unwrap();
    if let Some(k) = p.keywords().first() {
        return (*k).into();
    }
    if p.accepts_number() && apply(prop, 2.0.into()).is_ok() {
        return 2.0.into();
    }
    SAMPLES
        .iter()
        .find(|s| apply(prop, (**s).into()).is_ok())
        .map_or(Value::Null, |s| (*s).into())
}

// M9-TS-3305
#[test]
fn attributes_in_the_table_are_handled_as_their_scope_says() {
    for a in ATTRS {
        let mut attrs = Attrs::default();
        let sample: Value = match a.ts {
            "boolean" => true.into(),
            "number" => 1.0.into(),
            t if t.starts_with('"') => "any".into(),
            _ if a.name == "dir" => "ltr".into(),
            _ => "x".into(),
        };
        let r = attrs.set(a.name, &sample);
        match a.scope {
            Scope::Html => assert!(r.is_ok(), "{}: {r:?}", a.name),
            Scope::Core | Scope::Svg | Scope::Compat => {
                assert!(r.is_err(), "{} is handled by Attrs::set after all", a.name)
            }
        }
        // SVG elements store any attribute (`set_svg`).
        if a.scope == Scope::Svg {
            let mut d = Dom::new();
            let svg = d.add(0, "svg", &[]);
            let shape = d.add(svg, "rect", &[]);
            let mut tree = d.build("");
            let errors = tree.apply_commit(&web_ui_core::Commit {
                ops: vec![common::op(
                    platypus_ui::opcodes::SET_ATTRS,
                    &[common::n(shape), a.name.into(), sample.clone()],
                )],
            });
            assert!(errors.is_empty(), "{}: {errors:?}", a.name);
        }
    }
    // The other direction: every name `Attrs::set` matches is declared as Html.
    let src = include_str!("../src/attrs.rs");
    let body = &src[src.find("pub fn set(").unwrap()..src.find("other =>").unwrap()];
    let html: BTreeSet<&str> = ATTRS
        .iter()
        .filter(|a| a.scope == Scope::Html)
        .map(|a| a.name)
        .collect();
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with('"') && t.contains("=>") {
            let name = t.split('"').nth(1).unwrap();
            assert!(
                html.contains(name),
                "Attrs::set handles `{name}`: declare it"
            );
        }
    }
    assert!(Attrs::default().set("data-kind", &"x".into()).is_ok());
}

// M9-TS-3306
#[test]
fn elements_in_the_table_can_be_created() {
    let mut d = Dom::new();
    for e in ELEMENTS {
        let id = d.add(0, e.tag, &[]);
        let _ = id;
    }
    let svg = d.add(0, "svg", &[]);
    for k in SVG_KINDS {
        d.add(svg, k, &[]);
    }
    let _tree = d.build("");
    assert_eq!(tag_of_component("TextArea"), Some("textarea"));
    assert_eq!(tag_of_component("Nope"), None);
    assert_eq!(platypus_ui::svg::SVG_KINDS, SVG_KINDS);
}

// M9-TS-3307
#[test]
fn event_fields_read_the_slots_script_args_fills() {
    let mut e = UiEvent::new(
        EventKind::KeyDown,
        vec![web_ui_core::NodeId(7)],
        Modifiers {
            shift: true,
            ctrl: false,
            alt: true,
            meta: false,
        },
    );
    e.x = 11.0;
    e.y = 12.0;
    e.button = 2;
    e.buttons = 3;
    e.detail = 4;
    e.delta_x = 5.0;
    e.delta_y = 6.0;
    e.repeat = true;
    e.key = "a".into();
    e.code = "KeyA".into();
    e.scroll = Some(platypus_ui::ScrollInfo {
        left: 20.0,
        top: 21.0,
        width: 22.0,
        height: 23.0,
        client_width: 24.0,
        client_height: 25.0,
    });
    let args = e.script_args();
    let Value::Array(data) = &args[2] else {
        panic!()
    };
    let num = |i: usize| data[i].as_f64().unwrap();
    let want: &[(&str, f64)] = &[
        ("clientX", 11.0),
        ("clientY", 12.0),
        ("x", 11.0),
        ("y", 12.0),
        ("button", 2.0),
        ("buttons", 3.0),
        ("detail", 4.0),
        ("deltaX", 5.0),
        ("deltaY", 6.0),
        ("scrollLeft", 20.0),
        ("scrollTop", 21.0),
        ("scrollWidth", 22.0),
        ("scrollHeight", 23.0),
        ("clientWidth", 24.0),
        ("clientHeight", 25.0),
    ];
    for (name, v) in want {
        let f = EVENT_FIELDS.iter().find(|f| f.name == *name).unwrap();
        let Init::Num(i) = f.init else {
            panic!("{name} is not a number slot")
        };
        assert_eq!(num(i), *v, "{name}");
    }
    let flag = |name: &str| {
        let f = EVENT_FIELDS.iter().find(|f| f.name == name).unwrap();
        let Init::Flag(i, mask) = f.init else {
            panic!("{name}")
        };
        (num(i) as u32 & mask) != 0
    };
    assert!(flag("shiftKey") && flag("altKey") && !flag("ctrlKey") && !flag("metaKey"));
    let Init::One(r) = EVENT_FIELDS
        .iter()
        .find(|f| f.name == "repeat")
        .unwrap()
        .init
    else {
        panic!()
    };
    assert_eq!(num(r), 1.0);
    let Value::Array(strs) = &args[3] else {
        panic!()
    };
    for (name, want) in [("key", "a"), ("code", "KeyA")] {
        let Init::Str(i) = EVENT_FIELDS.iter().find(|f| f.name == name).unwrap().init else {
            panic!()
        };
        assert_eq!(strs[i].as_str(), Some(want), "{name}");
    }
    // An input event carries its text in string slot 0.
    let mut input = UiEvent::new(
        EventKind::Input,
        vec![web_ui_core::NodeId(1)],
        Modifiers::default(),
    );
    input.value = Some("hello".into());
    let Value::Array(strs) = &input.script_args()[3] else {
        panic!()
    };
    let Init::Str(i) = EVENT_FIELDS
        .iter()
        .find(|f| f.name == "value")
        .unwrap()
        .init
    else {
        panic!()
    };
    assert_eq!(strs[i].as_str(), Some("hello"));
}

// M9-TS-3308
#[test]
fn event_props_name_real_event_types_and_interfaces_use_declared_fields() {
    let kinds: BTreeSet<&str> = EventKind::ALL.iter().map(|k| k.name()).collect();
    assert_eq!(
        kinds.len(),
        EventKind::ALL.len(),
        "duplicate in EventKind::ALL"
    );
    let ifaces: BTreeSet<&str> = EVENT_INTERFACES.iter().map(|i| i.name).collect();
    for p in EVENT_PROPS {
        assert!(
            kinds.contains(p.kind),
            "{} listens to unknown `{}`",
            p.prop,
            p.kind
        );
        assert!(ifaces.contains(p.interface), "{}: {}", p.prop, p.interface);
        assert!(p.prop.starts_with("on"));
        assert_eq!(kind_of_prop(p.prop), Some((p.kind, false)));
        assert_eq!(
            kind_of_prop(&format!("{}Capture", p.prop)),
            Some((p.kind, true))
        );
    }
    for i in EVENT_INTERFACES {
        for f in i.fields {
            assert!(EVENT_FIELDS.iter().any(|x| x.name == *f), "{}.{f}", i.name);
        }
        if let Some(b) = i.extends {
            assert!(ifaces.contains(b), "{} extends {b}", i.name);
        }
    }
    // Every event type has an `on*` prop or is listed as not exposed, with the reason.
    let typed: BTreeSet<&str> = EVENT_PROPS.iter().map(|p| p.kind).collect();
    let hidden: BTreeSet<&str> = NOT_EXPOSED
        .iter()
        .map(|(k, why)| {
            assert!(!why.is_empty(), "NOT_EXPOSED `{k}` needs a reason");
            *k
        })
        .collect();
    for k in &hidden {
        assert!(kinds.contains(k), "NOT_EXPOSED lists unknown `{k}`");
        assert!(
            !typed.contains(k),
            "`{k}` has a prop and is listed as not exposed"
        );
    }
    let untyped: Vec<_> = kinds
        .iter()
        .filter(|k| !typed.contains(**k) && !hidden.contains(**k))
        .collect();
    assert!(
        untyped.is_empty(),
        "event types without an on* prop (add one, or list them in NOT_EXPOSED with a reason): {untyped:?}"
    );
    for k in NON_BUBBLING {
        assert!(kinds.contains(k), "NON_BUBBLING lists unknown `{k}`");
    }
}

// M9-TS-3329
//
// `EventKind::ALL` lists every event type: the exhaustive `match` below stops compiling when a
// type is added, and then `ALL` must have grown with it.
#[test]
fn every_event_kind_is_in_the_all_list() {
    fn index(k: EventKind) -> usize {
        match k {
            EventKind::PointerMove => 0,
            EventKind::PointerDown => 1,
            EventKind::PointerUp => 2,
            EventKind::PointerEnter => 3,
            EventKind::PointerLeave => 4,
            EventKind::Click => 5,
            EventKind::DblClick => 6,
            EventKind::ContextMenu => 7,
            EventKind::Wheel => 8,
            EventKind::KeyDown => 9,
            EventKind::KeyUp => 10,
            EventKind::FocusIn => 11,
            EventKind::FocusOut => 12,
            EventKind::Scroll => 13,
            EventKind::Input => 14,
            EventKind::Cancel => 15,
            EventKind::Close => 16,
            EventKind::TransitionRun => 17,
            EventKind::TransitionStart => 18,
            EventKind::TransitionEnd => 19,
            EventKind::TransitionCancel => 20,
            EventKind::AnimationStart => 21,
            EventKind::AnimationIteration => 22,
            EventKind::AnimationEnd => 23,
            EventKind::AnimationCancel => 24,
            EventKind::GotPointerCapture => 25,
            EventKind::LostPointerCapture => 26,
            EventKind::DragStart => 27,
            EventKind::Drag => 28,
            EventKind::DragEnter => 29,
            EventKind::DragOver => 30,
            EventKind::DragLeave => 31,
            EventKind::Drop => 32,
            EventKind::DragEnd => 33,
        }
    }
    const COUNT: usize = 34;
    let mut seen = [false; COUNT];
    for k in EventKind::ALL {
        seen[index(*k)] = true;
    }
    assert!(
        seen.iter().all(|s| *s) && EventKind::ALL.len() == COUNT,
        "EventKind::ALL is missing a type (or the list in this test is out of date)"
    );
}

// M9-TS-3331
//
// The slots of the transition, animation, focus and drag fields read what the events carry: the
// property or animation name in string 0, the elapsed seconds in number 5, the related element
// in number 9, the dragged files as the strings and `dragend`'s `detail` for `dropped`.
#[test]
fn motion_focus_and_drag_fields_read_their_slots() {
    let slot = |name: &str| EVENT_FIELDS.iter().find(|f| f.name == name).unwrap().init;
    let mut t = UiEvent::new(
        EventKind::TransitionEnd,
        vec![web_ui_core::NodeId(1)],
        Modifiers::default(),
    );
    t.value = Some("background-color".into());
    t.delta_x = 1.5;
    let args = t.script_args();
    let (Value::Array(data), Value::Array(strs)) = (&args[2], &args[3]) else {
        panic!()
    };
    for name in ["propertyName", "animationName"] {
        let Init::Str(i) = slot(name) else { panic!() };
        assert_eq!(strs[i].as_str(), Some("background-color"));
    }
    let Init::Num(i) = slot("elapsedTime") else {
        panic!()
    };
    assert_eq!(data[i].as_f64(), Some(1.5));
    let mut f = UiEvent::new(
        EventKind::FocusIn,
        vec![web_ui_core::NodeId(1)],
        Modifiers::default(),
    );
    f.related = Some(web_ui_core::NodeId(9));
    assert!(matches!(slot("relatedTarget"), Init::Related));
    let Value::Array(data) = &f.script_args()[2] else {
        panic!()
    };
    assert_eq!(data[9].as_f64(), Some(9.0), "relatedTarget reads data[9]");
    let mut d = UiEvent::new(
        EventKind::DragEnd,
        vec![web_ui_core::NodeId(1)],
        Modifiers::default(),
    );
    d.detail = 1;
    d.files = vec!["a.png".into()];
    let args = d.script_args();
    assert!(matches!(slot("dropped"), Init::Dropped));
    assert!(matches!(slot("dataTransfer"), Init::Transfer));
    let (Value::Array(data), Value::Array(strs)) = (&args[2], &args[3]) else {
        panic!()
    };
    assert_eq!(
        data[4].as_f64(),
        Some(1.0),
        "dropped reads detail (data[4])"
    );
    assert_eq!(strs[0].as_str(), Some("a.png"), "the files are the strings");
}
