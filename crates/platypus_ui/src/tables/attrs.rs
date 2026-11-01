// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The attributes (`PlatypusProps`) of an element. `platypus_ui::Attrs::set` implements the [`Scope::Html`]
//! ones, SVG elements take any name (`Attrs::set_svg`); the JSX lowering handles [`Scope::Core`].

/// Who handles an attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// `key`, `style`, `children`, `ref`: the JSX lowering handles them, they never reach the tree.
    Core,
    /// `Attrs::set` (any element).
    Html,
    /// Inline SVG: `Attrs::set_svg` stores it on SVG elements (and `text` inside an `svg`).
    Svg,
    /// Declared for familiarity but not implemented: `Attrs::set` rejects it on non-SVG elements
    /// (a logged problem), SVG elements store it. No attribute uses it at the moment.
    Compat,
}

#[derive(Clone, Copy, Debug)]
pub struct Attr {
    pub name: &'static str,
    /// The TypeScript type.
    pub ts: &'static str,
    pub scope: Scope,
    /// Elements it means something on; empty for any.
    pub on: &'static [&'static str],
    pub doc: &'static str,
}

const fn a(
    name: &'static str,
    ts: &'static str,
    scope: Scope,
    on: &'static [&'static str],
    doc: &'static str,
) -> Attr {
    Attr {
        name,
        ts,
        scope,
        on,
        doc,
    }
}

use Scope::*;

pub const ATTRS: &[Attr] = &[
    a(
        "key",
        "string | number",
        Core,
        &[],
        "Identity among siblings (keyed diff).",
    ),
    a(
        "className",
        "string",
        Html,
        &[],
        "Space-separated class names.",
    ),
    a("id", "string", Html, &[], ""),
    a(
        "style",
        "PlatypusStyle",
        Core,
        &[],
        "Inline style; wins over stylesheets (except `!important`).",
    ),
    a("children", "ReactNode", Core, &[], ""),
    a(
        "ref",
        "Ref<PlatypusElement | null> | ((el: PlatypusElement | null) => void)",
        Core,
        &[],
        "Receives the element (`focus()`, `blur()`, `scrollTo()`, `scrollIntoView()`): a ref object, or a function called with the element and with null when it goes.",
    ),
    a(
        "tabIndex",
        "number",
        Html,
        &[],
        "Focusable when set; negative values are skipped by Tab.",
    ),
    a("disabled", "boolean", Html, &[], ""),
    a("checked", "boolean", Html, &["input"], ""),
    a(
        "draggable",
        "boolean",
        Html,
        &[],
        "A press and move starts a drag (`dragstart`).",
    ),
    a("placeholder", "string", Html, &["input", "textarea"], ""),
    a(
        "maxLength",
        "number",
        Html,
        &["input", "textarea"],
        "Most characters the field takes in (typed or pasted text is cut to fit).",
    ),
    a(
        "min",
        "number | string",
        Html,
        &["input"],
        "`type=\"number\"`: the lowest value ArrowUp/ArrowDown reach, and where steps count from.",
    ),
    a(
        "max",
        "number | string",
        Html,
        &["input"],
        "`type=\"number\"`: the highest value ArrowUp/ArrowDown reach.",
    ),
    a(
        "step",
        "number | string",
        Html,
        &["input"],
        "`type=\"number\"`: how far ArrowUp/ArrowDown move the value (1 by default).",
    ),
    a(
        "value",
        "string",
        Html,
        &["input", "textarea"],
        "Controlled text.",
    ),
    a(
        "defaultValue",
        "string",
        Html,
        &["input", "textarea"],
        "The initial text of an uncontrolled field.",
    ),
    a("readOnly", "boolean", Html, &["input", "textarea"], ""),
    a(
        "dir",
        "string",
        Html,
        &[],
        "`ltr` or `rtl`: the element's `direction` unless CSS sets one.",
    ),
    a(
        "src",
        "string",
        Html,
        &["image"],
        "A path inside the game folder, or a `data:` URL.",
    ),
    a("rows", "number", Html, &["textarea"], "Visible lines."),
    a("open", "boolean", Html, &["dialog"], ""),
    a(
        "modal",
        "boolean",
        Html,
        &["dialog"],
        "Shows the dialog in the top layer with a `::backdrop`.",
    ),
    a(
        "closedBy",
        "\"any\" | \"closerequest\" | \"none\"",
        Html,
        &["dialog"],
        "What may close a dialog besides scripts.",
    ),
    a(
        "title",
        "string",
        Html,
        &[],
        "Advisory text, kept on the element (no tooltip is drawn yet).",
    ),
    a(
        "cols",
        "number",
        Html,
        &["textarea"],
        "Width in characters (8 logical pixels each) when CSS sets none.",
    ),
    a(
        "type",
        "string",
        Html,
        &["input"],
        "Kept lower-cased for `input[type=\"password\"]` selectors. `password` shows each character as a bullet and gives nothing to the clipboard; `number` takes only digits, signs, `.` and `e` and steps with ArrowUp/ArrowDown (`min`, `max`, `step`); `search` clears on Escape; any other value (`text`, `email`, `tel`, `url`, ...) is a plain one-line text field.",
    ),
    // Inline SVG (`<svg>` and the shapes inside it): attributes as React names them.
    a("viewBox", "string", Svg, &["svg"], ""),
    a("points", "string", Svg, &["polyline", "polygon"], ""),
    a("d", "string", Svg, &["path"], ""),
    a("fill", "string", Svg, &[], ""),
    a("stroke", "string", Svg, &[], ""),
    a("strokeWidth", "number | string", Svg, &[], ""),
    a("strokeLinecap", "string", Svg, &[], ""),
    a("strokeLinejoin", "string", Svg, &[], ""),
    a("clipPath", "string", Svg, &[], ""),
    a("opacity", "number | string", Svg, &[], ""),
    a("transform", "string", Svg, &[], ""),
    a("textAnchor", "string", Svg, &[], ""),
    a("fontSize", "number | string", Svg, &[], ""),
    a("fontWeight", "number | string", Svg, &[], ""),
    a("cx", "number | string", Svg, &["circle", "ellipse"], ""),
    a("cy", "number | string", Svg, &["circle", "ellipse"], ""),
    a("r", "number | string", Svg, &["circle"], ""),
    a("rx", "number | string", Svg, &["rect", "ellipse"], ""),
    a("ry", "number | string", Svg, &["rect", "ellipse"], ""),
    a("x", "number | string", Svg, &[], ""),
    a("y", "number | string", Svg, &[], ""),
    a("x1", "number | string", Svg, &["line"], ""),
    a("y1", "number | string", Svg, &["line"], ""),
    a("x2", "number | string", Svg, &["line"], ""),
    a("y2", "number | string", Svg, &["line"], ""),
    a("width", "number | string", Svg, &[], ""),
    a("height", "number | string", Svg, &[], ""),
];
