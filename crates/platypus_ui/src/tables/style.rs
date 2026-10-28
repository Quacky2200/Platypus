// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The style properties a script can set (`PlatypusStyle`): name, TypeScript value type, documentation.
//! `platypus_ui::apply_style` implements them; `crates/platypus_ui/tests/ui_tables.rs` checks that every entry
//! here is applied and that nothing it applies is missing, and `tools/platypus_ts` prints the table as
//! `interface PlatypusStyle` in the generated `types/platypus.d.ts`.
//!
//! Units: a `Length` is a number (logical pixels) or a string with a unit (`"10px"`, `"50%"`,
//! `"2em"`, `calc(..)`); see [`LENGTH_UNITS`]. Shorthands expand to longhands, see [`SHORTHANDS`].

/// One piece of a property's TypeScript type; a type is the union of its parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// `number`.
    Num,
    /// `string`.
    Str,
    /// `Length` (`number | string`, a number being pixels).
    Length,
    /// A named union from [`NAMED_TYPES`] (`BorderStyle`, `Cursor`, ...).
    Named(&'static str),
    /// A string literal type (`"flex"`).
    Lit(&'static str),
    /// TypeScript text printed as written (`` `url(${string}` ``, `Exclude<..>`).
    Raw(&'static str),
}
use Part::*;

impl Part {
    pub fn ts(self) -> String {
        match self {
            Num => "number".into(),
            Str => "string".into(),
            Length => "Length".into(),
            Named(n) | Raw(n) => n.into(),
            Lit(l) => format!("\"{l}\""),
        }
    }
}

/// The TypeScript text of a union of parts.
pub fn ts_type(parts: &[Part]) -> String {
    parts.iter().map(|p| p.ts()).collect::<Vec<_>>().join(" | ")
}

/// A style property.
#[derive(Clone, Copy, Debug)]
pub struct StyleProp {
    /// The comment line this property starts a group under (`"Layout"`); empty continues the group
    /// before it.
    pub group: &'static str,
    /// The script (camelCase) name.
    pub name: &'static str,
    pub ty: &'static [Part],
    pub doc: &'static str,
}

impl StyleProp {
    /// Does the type take a number (pixels for a length, a plain number otherwise)?
    pub fn accepts_number(&self) -> bool {
        self.ty.iter().any(|p| matches!(p, Num | Length))
    }

    /// Does the type take a free string (`Str`, a `Length` with a unit, or a `Raw` text)?
    pub fn accepts_string(&self) -> bool {
        self.ty.iter().any(|p| matches!(p, Str | Length | Raw(_)))
    }

    /// The keywords the type lists (literals and the members of named unions).
    pub fn keywords(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for p in self.ty {
            match p {
                Lit(l) => out.push(*l),
                Named(n) => out.extend(named_type(n).unwrap_or(&[]).iter().copied()),
                _ => {}
            }
        }
        out
    }
}

const fn sp(
    group: &'static str,
    name: &'static str,
    ty: &'static [Part],
    doc: &'static str,
) -> StyleProp {
    StyleProp {
        group,
        name,
        ty,
        doc,
    }
}

const STR: &[Part] = &[Str];
const NUM: &[Part] = &[Num];
const LENGTH: &[Part] = &[Length];

/// `Length`'s units, besides a bare number (pixels).
pub const LENGTH_UNITS: &[&str] = &[
    "px", "%", "em", "rem", "vw", "vh", "vmin", "vmax", "pt", "pc", "in", "cm", "mm", "q",
];

/// Shorthands and the longhands they set (`margin` sets four sides).
pub const SHORTHANDS: &[(&str, &[&str])] = &[
    (
        "margin",
        &["marginTop", "marginRight", "marginBottom", "marginLeft"],
    ),
    (
        "padding",
        &["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"],
    ),
    (
        "borderWidth",
        &[
            "borderTopWidth",
            "borderRightWidth",
            "borderBottomWidth",
            "borderLeftWidth",
        ],
    ),
    ("inset", &["top", "right", "bottom", "left"]),
    ("gap", &["rowGap", "columnGap"]),
    (
        "borderRadius",
        &[
            "borderTopLeftRadius",
            "borderTopRightRadius",
            "borderBottomRightRadius",
            "borderBottomLeftRadius",
        ],
    ),
    ("flexFlow", &["flexDirection", "flexWrap"]),
    ("overflow", &["overflowX", "overflowY"]),
    (
        "scrollPadding",
        &[
            "scrollPaddingTop",
            "scrollPaddingRight",
            "scrollPaddingBottom",
            "scrollPaddingLeft",
        ],
    ),
];

/// `cursor` keywords. `auto` is the text cursor over text and the arrow elsewhere.
pub const CURSORS: &[&str] = &[
    "auto",
    "default",
    "none",
    "context-menu",
    "help",
    "pointer",
    "progress",
    "wait",
    "cell",
    "crosshair",
    "text",
    "vertical-text",
    "alias",
    "copy",
    "move",
    "no-drop",
    "not-allowed",
    "grab",
    "grabbing",
    "all-scroll",
    "col-resize",
    "row-resize",
    "n-resize",
    "e-resize",
    "s-resize",
    "w-resize",
    "ne-resize",
    "nw-resize",
    "se-resize",
    "sw-resize",
    "ew-resize",
    "ns-resize",
    "nesw-resize",
    "nwse-resize",
    "zoom-in",
    "zoom-out",
];

pub const BORDER_STYLES: &[&str] = &[
    "none", "hidden", "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset",
];

pub const BLEND_MODES: &[&str] = &[
    "normal",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "color-dodge",
    "color-burn",
    "hard-light",
    "soft-light",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
];

pub const OVERFLOWS: &[&str] = &["visible", "hidden", "clip", "scroll", "auto"];

/// The named unions (`type BorderStyle = "none" | ..;`) the properties refer to.
pub const NAMED_TYPES: &[(&str, &[&str])] = &[
    ("BorderStyle", BORDER_STYLES),
    ("BlendMode", BLEND_MODES),
    ("Overflow", OVERFLOWS),
    ("Cursor", CURSORS),
];

pub fn named_type(name: &str) -> Option<&'static [&'static str]> {
    NAMED_TYPES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| *v)
}

/// Every property of `PlatypusStyle`, in declaration order.
pub const STYLE_PROPS: &[StyleProp] = &[
    sp(
        "Layout",
        "display",
        &[
            Lit("flex"),
            Lit("grid"),
            Lit("block"),
            Lit("none"),
            Lit("contents"),
        ],
        r#"`contents` drops the box: children join the parent's layout."#,
    ),
    sp(
        "",
        "position",
        &[
            Lit("static"),
            Lit("relative"),
            Lit("absolute"),
            Lit("fixed"),
        ],
        r#"`static` (the default) is no containing block for absolute children (unless a transform, filter, backdrop filter, `perspective`, `contain` or `willChange` makes one). `fixed` places the element against the window and paints it above the tree."#,
    ),
    sp(
        "",
        "float",
        &[
            Lit("none"),
            Lit("left"),
            Lit("right"),
            Lit("inline-start"),
            Lit("inline-end"),
        ],
        r#"Takes the box out of the line flow to the left or right of a block container; the text that follows wraps around it. Only blocks (`display: block` / `flow-root`) lay floats out; flex and grid items ignore it."#,
    ),
    sp(
        "",
        "clear",
        &[
            Lit("none"),
            Lit("left"),
            Lit("right"),
            Lit("both"),
            Lit("inline-start"),
            Lit("inline-end"),
        ],
        r#"Moves the box below the floats on that side."#,
    ),
    sp(
        "",
        "boxSizing",
        &[Lit("border-box"), Lit("content-box")],
        r#"Platypus defaults to `border-box` (sizes include padding and border), unlike CSS."#,
    ),
    sp("", "width", LENGTH, ""),
    sp("", "height", LENGTH, ""),
    sp("", "minWidth", LENGTH, ""),
    sp("", "minHeight", LENGTH, ""),
    sp("", "maxWidth", LENGTH, ""),
    sp("", "maxHeight", LENGTH, ""),
    sp("", "aspectRatio", NUM, ""),
    sp("", "top", LENGTH, ""),
    sp("", "right", LENGTH, ""),
    sp("", "bottom", LENGTH, ""),
    sp("", "left", LENGTH, ""),
    sp("", "inset", LENGTH, ""),
    sp("", "margin", LENGTH, ""),
    sp("", "marginTop", LENGTH, ""),
    sp("", "marginRight", LENGTH, ""),
    sp("", "marginBottom", LENGTH, ""),
    sp("", "marginLeft", LENGTH, ""),
    sp("", "padding", LENGTH, ""),
    sp("", "paddingTop", LENGTH, ""),
    sp("", "paddingRight", LENGTH, ""),
    sp("", "paddingBottom", LENGTH, ""),
    sp("", "paddingLeft", LENGTH, ""),
    sp(
        "Logical properties (horizontal, left-to-right): block = top/bottom, inline = left/right.",
        "marginBlock",
        &[Length, Str],
        "",
    ),
    sp("", "marginInline", &[Length, Str], ""),
    sp("", "marginBlockStart", LENGTH, ""),
    sp("", "marginBlockEnd", LENGTH, ""),
    sp("", "marginInlineStart", LENGTH, ""),
    sp("", "marginInlineEnd", LENGTH, ""),
    sp("", "paddingBlock", &[Length, Str], ""),
    sp("", "paddingInline", &[Length, Str], ""),
    sp("", "paddingBlockStart", LENGTH, ""),
    sp("", "paddingBlockEnd", LENGTH, ""),
    sp("", "paddingInlineStart", LENGTH, ""),
    sp("", "paddingInlineEnd", LENGTH, ""),
    sp("", "insetBlock", &[Length, Str], ""),
    sp("", "insetInline", &[Length, Str], ""),
    sp("", "insetBlockStart", LENGTH, ""),
    sp("", "insetBlockEnd", LENGTH, ""),
    sp("", "insetInlineStart", LENGTH, ""),
    sp("", "insetInlineEnd", LENGTH, ""),
    sp("", "blockSize", LENGTH, ""),
    sp("", "inlineSize", LENGTH, ""),
    sp("", "minBlockSize", LENGTH, ""),
    sp("", "minInlineSize", LENGTH, ""),
    sp("", "maxBlockSize", LENGTH, ""),
    sp("", "maxInlineSize", LENGTH, ""),
    sp("", "borderWidth", LENGTH, ""),
    sp("", "borderTopWidth", LENGTH, ""),
    sp("", "borderRightWidth", LENGTH, ""),
    sp("", "borderBottomWidth", LENGTH, ""),
    sp("", "borderLeftWidth", LENGTH, ""),
    sp("", "gap", LENGTH, ""),
    sp("", "rowGap", LENGTH, ""),
    sp("", "columnGap", LENGTH, ""),
    sp(
        "Interaction",
        "pointerEvents",
        &[Lit("auto"), Lit("none")],
        "",
    ),
    sp(
        "",
        "cursor",
        &[Named("Cursor"), Raw("`url(${string}`")],
        r#"CSS cursor keyword (`pointer`, `text`, `grab`, `not-allowed`, ...), optionally after
images to try first: `url(sword.png) 4 4, pointer` (PNG, JPEG or `.cur`, up to
128px; the numbers are the hotspot). Inherited."#,
    ),
    sp(
        "Flexbox",
        "flexDirection",
        &[
            Lit("row"),
            Lit("column"),
            Lit("row-reverse"),
            Lit("column-reverse"),
        ],
        "",
    ),
    sp(
        "",
        "flexWrap",
        &[Lit("nowrap"), Lit("wrap"), Lit("wrap-reverse")],
        "",
    ),
    sp(
        "",
        "flexFlow",
        STR,
        r#"Shorthand for `flexDirection` and `flexWrap`, in either order: `"column wrap"`."#,
    ),
    sp("", "flexGrow", NUM, ""),
    sp("", "flexShrink", NUM, ""),
    sp("", "flexBasis", LENGTH, ""),
    sp("", "flex", &[Num, Lit("none"), Lit("auto")], ""),
    sp("Alignment", "alignItems", STR, ""),
    sp("", "alignSelf", STR, ""),
    sp("", "alignContent", STR, ""),
    sp("", "justifyItems", STR, ""),
    sp("", "justifySelf", STR, ""),
    sp(
        "",
        "placeItems",
        STR,
        r#"`<align> [<justify>]`; one value sets both."#,
    ),
    sp("", "placeSelf", STR, ""),
    sp("", "placeContent", STR, ""),
    sp("", "justifyContent", STR, ""),
    sp("Grid", "gridTemplateColumns", STR, ""),
    sp("", "gridTemplateRows", STR, ""),
    sp("", "gridAutoFlow", STR, ""),
    sp("", "gridColumn", &[Num, Str], ""),
    sp("", "gridRow", &[Num, Str], ""),
    sp("", "gridRowStart", &[Num, Str], ""),
    sp("", "gridRowEnd", &[Num, Str], ""),
    sp("", "gridColumnStart", &[Num, Str], ""),
    sp("", "gridColumnEnd", &[Num, Str], ""),
    sp(
        "",
        "gridArea",
        STR,
        r#"An area name, or `row-start / column-start / row-end / column-end`."#,
    ),
    sp(
        "",
        "gridTemplateAreas",
        STR,
        r#"Quoted rows of area names: `'"head head" "side main"'`."#,
    ),
    sp(
        "",
        "gridTemplate",
        STR,
        r#"`rows / columns`, or area strings with row sizes: `'"a b" 40px "c c" 1fr / 1fr 2fr'`."#,
    ),
    sp("", "gridAutoRows", STR, ""),
    sp("", "gridAutoColumns", STR, ""),
    sp("Paint", "background", STR, ""),
    sp("", "backgroundColor", STR, ""),
    sp(
        "",
        "maskImage",
        STR,
        r#"Mask layers (images or gradients), top first; the element and its children show where the mask does."#,
    ),
    sp("", "mask", STR, r#"Shorthand for `maskImage` only."#),
    sp("", "maskSize", STR, ""),
    sp("", "maskPosition", STR, ""),
    sp("", "maskRepeat", STR, ""),
    sp(
        "",
        "maskMode",
        STR,
        r#"`alpha` (and `match-source`) or `luminance`; the top layer's mode applies to all."#,
    ),
    sp("", "WebkitMaskImage", STR, ""),
    sp("", "WebkitMaskSize", STR, ""),
    sp("", "WebkitMaskPosition", STR, ""),
    sp("", "WebkitMaskRepeat", STR, ""),
    sp(
        "",
        "transform",
        STR,
        r#"Transform functions, applied right to left about `transformOrigin`: `matrix`, `matrix3d`,
`perspective`, `rotate`/`rotateX`/`rotateY`/`rotateZ`/`rotate3d`, `translate`/`translateX`/`Y`/`Z`/
`translate3d`, `scale`/`scaleX`/`Y`/`Z`/`scale3d`, `skew`/`skewX`/`skewY`, or `none`."#,
    ),
    sp(
        "",
        "translate",
        STR,
        r#"`x [y [z]]`, applied before `transform`."#,
    ),
    sp(
        "",
        "rotate",
        STR,
        r#"`angle`, `x|y|z angle` or `vx vy vz angle`, applied after `translate`."#,
    ),
    sp(
        "",
        "scale",
        &[Str, Num],
        r#"`x [y [z]]` (numbers or percentages), applied after `rotate`."#,
    ),
    sp(
        "",
        "transformOrigin",
        STR,
        r#"`x y [z]`; default `50% 50%`."#,
    ),
    sp(
        "",
        "perspective",
        &[Length, Lit("none")],
        r#"Depth for this element's transformed children (`none` or a length)."#,
    ),
    sp("", "perspectiveOrigin", STR, ""),
    sp(
        "",
        "transformStyle",
        &[Lit("flat"), Lit("preserve-3d")],
        r#"`preserve-3d` lets children share this element's 3D space (no depth sorting)."#,
    ),
    sp(
        "",
        "backfaceVisibility",
        &[Lit("visible"), Lit("hidden")],
        "",
    ),
    sp(
        "",
        "backgroundImage",
        STR,
        r#"Comma-separated layers, top first; the other background properties take matching lists."#,
    ),
    sp("", "backgroundSize", STR, ""),
    sp("", "backgroundPosition", STR, ""),
    sp("", "backgroundPositionX", STR, ""),
    sp("", "backgroundPositionY", STR, ""),
    sp(
        "",
        "backgroundRepeat",
        STR,
        r#"`repeat`, `no-repeat`, `repeat-x`, `repeat-y`, `round`, `space`, or one per axis."#,
    ),
    sp(
        "",
        "backgroundClip",
        STR,
        r#"`border-box` (default), `padding-box`, `content-box` or `text` (gradient text)."#,
    ),
    sp(
        "",
        "WebkitBackgroundClip",
        STR,
        r#"The prefixed spelling of `backgroundClip` (`text` clips the background to the glyphs)."#,
    ),
    sp(
        "",
        "backgroundBlendMode",
        STR,
        r#"How each background layer blends with the layers and colour beneath it."#,
    ),
    sp(
        "",
        "mixBlendMode",
        &[Named("BlendMode")],
        r#"How the element blends with what's beneath it."#,
    ),
    sp(
        "",
        "border",
        STR,
        r#"Width, style and colour, e.g. `"2px dashed red"`. No style means no border, like CSS."#,
    ),
    sp("", "borderTop", STR, ""),
    sp("", "borderRight", STR, ""),
    sp("", "borderBottom", STR, ""),
    sp("", "borderLeft", STR, ""),
    sp(
        "",
        "borderColor",
        STR,
        r#"1-4 colours (top, right, bottom, left)."#,
    ),
    sp("", "borderTopColor", STR, ""),
    sp("", "borderRightColor", STR, ""),
    sp("", "borderBottomColor", STR, ""),
    sp("", "borderLeftColor", STR, ""),
    sp(
        "",
        "borderStyle",
        STR,
        r#"1-4 styles. Platypus's default is `solid`, so a width alone draws a border."#,
    ),
    sp("", "borderTopStyle", &[Named("BorderStyle")], ""),
    sp("", "borderRightStyle", &[Named("BorderStyle")], ""),
    sp("", "borderBottomStyle", &[Named("BorderStyle")], ""),
    sp("", "borderLeftStyle", &[Named("BorderStyle")], ""),
    sp(
        "",
        "borderImage",
        STR,
        r#"A 9-slice image or gradient over the border, e.g. `"url(frame.png) 12 fill / 8px round"`."#,
    ),
    sp("", "borderImageSource", STR, ""),
    sp(
        "",
        "borderImageSlice",
        STR,
        r#"1-4 numbers (image px) or percentages, plus `fill` to draw the middle."#,
    ),
    sp(
        "",
        "borderImageWidth",
        STR,
        r#"1-4 multiples of the border width, lengths, percentages or `auto`."#,
    ),
    sp("", "borderImageOutset", STR, ""),
    sp(
        "",
        "borderImageRepeat",
        STR,
        r#"`stretch`, `repeat`, `round` or `space`; one or two (horizontal, vertical)."#,
    ),
    sp(
        "",
        "outline",
        STR,
        r#"Drawn outside the border box, over the content; takes no space. E.g. `"2px solid white"`."#,
    ),
    sp("", "outlineWidth", LENGTH, ""),
    sp(
        "",
        "outlineStyle",
        &[Raw("Exclude<BorderStyle, \"hidden\">"), Lit("auto")],
        r#"Default `none`; `auto` draws solid."#,
    ),
    sp("", "outlineColor", STR, ""),
    sp(
        "",
        "outlineOffset",
        LENGTH,
        r#"Gap between the border box and the outline; negative draws it inside."#,
    ),
    sp(
        "",
        "borderRadius",
        LENGTH,
        r#"1-4 radii, optionally `/` and vertical radii for elliptical corners (`"40px / 12px"`)."#,
    ),
    sp("", "borderTopLeftRadius", LENGTH, ""),
    sp("", "borderTopRightRadius", LENGTH, ""),
    sp("", "borderBottomRightRadius", LENGTH, ""),
    sp("", "borderBottomLeftRadius", LENGTH, ""),
    sp("", "boxShadow", STR, ""),
    sp(
        "",
        "filter",
        STR,
        r#"Chain of `blur()`, `brightness()`, `contrast()`, `saturate()`, `grayscale()`, `sepia()`, `hue-rotate()`, `invert()`, `opacity()`, `drop-shadow()`."#,
    ),
    sp(
        "",
        "backdropFilter",
        STR,
        r#"Like `filter`, without `drop-shadow()`, applied to what is behind the box."#,
    ),
    sp(
        "",
        "opacity",
        &[Num, Str],
        r#"0-1, or a percentage string (`"40%"`)."#,
    ),
    sp(
        "Images",
        "objectFit",
        &[
            Lit("fill"),
            Lit("contain"),
            Lit("cover"),
            Lit("none"),
            Lit("scale-down"),
        ],
        r#"How an `Image` fills its content box. Default `fill`."#,
    ),
    sp(
        "",
        "objectPosition",
        STR,
        r#"Where the image sits in its content box, like `backgroundPosition`. Default `50% 50%`."#,
    ),
    sp(
        "Ordering",
        "order",
        NUM,
        r#"Flex and grid item order (layout and painting; Tab order stays in document order)."#,
    ),
    sp(
        "",
        "zIndex",
        &[Num, Lit("auto")],
        r#"Paint and hit-test order among siblings."#,
    ),
    sp(
        "",
        "isolation",
        &[Lit("auto"), Lit("isolate")],
        r#"Accepted; every z-indexed element already stacks on its own."#,
    ),
    sp(
        "",
        "willChange",
        STR,
        r#"`auto` or a list of property names. Naming `transform`, `filter`, `perspective`, `backdrop-filter` (or `translate`, `rotate`, `scale`, `transform-style`, `contain`) makes the element a containing block for absolute children, as those properties do."#,
    ),
    sp(
        "",
        "contain",
        STR,
        r#"`none`, `strict`, `content` or any of `size`, `inline-size`, `layout`, `style`, `paint`. Layout and paint containment make an independent formatting context and a containing block for absolute children; paint containment also clips the content to the padding box. Size containment is accepted and ignored."#,
    ),
    sp(
        "Clipping",
        "overflow",
        STR,
        r#"`visible`, `hidden`, `clip`, `scroll` or `auto`; one value or `x y`. Clips children to the padding box."#,
    ),
    sp("", "overflowX", &[Named("Overflow")], ""),
    sp("", "overflowY", &[Named("Overflow")], ""),
    sp(
        "",
        "clipPath",
        STR,
        r#"`inset()`, `circle()`, `ellipse()`, `polygon()` or `path()`, and/or a reference box."#,
    ),
    sp(
        "",
        "clip",
        STR,
        r#"Legacy `rect(top, right, bottom, left)`; absolutely positioned elements only. Prefer `clipPath`."#,
    ),
    sp(
        "",
        "scrollbarWidth",
        &[Lit("auto"), Lit("thin"), Lit("none")],
        r#"`auto`, `thin` or `none` (hidden but still scrollable)."#,
    ),
    sp(
        "",
        "fill",
        STR,
        r#"SVG presentation properties, for shapes inside an `<svg>` (inherit as in SVG)."#,
    ),
    sp("", "fillOpacity", &[Num, Str], ""),
    sp("", "fillRule", &[Lit("nonzero"), Lit("evenodd")], ""),
    sp("", "stroke", STR, ""),
    sp("", "strokeWidth", &[Num, Str], ""),
    sp("", "strokeOpacity", &[Num, Str], ""),
    sp(
        "",
        "strokeLinecap",
        &[Lit("butt"), Lit("round"), Lit("square")],
        "",
    ),
    sp(
        "",
        "strokeLinejoin",
        &[Lit("miter"), Lit("round"), Lit("bevel")],
        "",
    ),
    sp("", "strokeDasharray", STR, ""),
    sp("", "strokeDashoffset", &[Num, Str], ""),
    sp("", "strokeMiterlimit", &[Num, Str], ""),
    sp("", "stopColor", STR, ""),
    sp("", "stopOpacity", &[Num, Str], ""),
    sp(
        "",
        "textAnchor",
        &[Lit("start"), Lit("middle"), Lit("end")],
        "",
    ),
    sp("", "dominantBaseline", STR, ""),
    sp("", "paintOrder", STR, ""),
    sp("", "vectorEffect", STR, ""),
    sp(
        "",
        "cx",
        &[Num, Str],
        r#"SVG geometry as CSS (`circle { r: 10px }`)."#,
    ),
    sp("", "cy", &[Num, Str], ""),
    sp("", "r", &[Num, Str], ""),
    sp("", "rx", &[Num, Str], ""),
    sp("", "ry", &[Num, Str], ""),
    sp("", "x", &[Num, Str], ""),
    sp("", "y", &[Num, Str], ""),
    sp("", "d", STR, ""),
    sp(
        "",
        "backgroundImageTint",
        STR,
        r#"s&box's image tint: a colour multiplied into background images and an `Image`'s picture."#,
    ),
    sp(
        "",
        "borderImageTint",
        STR,
        r#"A colour multiplied into the `borderImage` source image."#,
    ),
    sp(
        "",
        "scrollbarGutter",
        &[Lit("auto"), Lit("stable"), Lit("stable both-edges")],
        r#"`stable` keeps room for the (otherwise overlaid) scrollbar even before content overflows."#,
    ),
    sp(
        "",
        "overscrollBehavior",
        STR,
        r#"`contain` / `none`: scrolling past this container's end doesn't scroll the ones outside."#,
    ),
    sp(
        "",
        "overscrollBehaviorX",
        &[Lit("auto"), Lit("contain"), Lit("none")],
        "",
    ),
    sp(
        "",
        "overscrollBehaviorY",
        &[Lit("auto"), Lit("contain"), Lit("none")],
        "",
    ),
    sp(
        "",
        "overscrollBehaviorInline",
        &[Lit("auto"), Lit("contain"), Lit("none")],
        "",
    ),
    sp(
        "",
        "overscrollBehaviorBlock",
        &[Lit("auto"), Lit("contain"), Lit("none")],
        "",
    ),
    sp(
        "",
        "scrollBehavior",
        &[Lit("auto"), Lit("smooth")],
        r#"`smooth`: script scrolls (`scrollTo`, `scrollIntoView`) ease instead of jumping."#,
    ),
    sp(
        "",
        "scrollPadding",
        LENGTH,
        r#"Insets that `scrollIntoView` keeps elements clear of (a sticky header, a fade); px or `auto` (0)."#,
    ),
    sp("", "scrollPaddingTop", LENGTH, ""),
    sp("", "scrollPaddingRight", LENGTH, ""),
    sp("", "scrollPaddingBottom", LENGTH, ""),
    sp("", "scrollPaddingLeft", LENGTH, ""),
    sp(
        "",
        "scrollPaddingBlock",
        LENGTH,
        "Top and bottom (one or two values).",
    ),
    sp(
        "",
        "scrollPaddingInline",
        LENGTH,
        "Left and right (one or two values).",
    ),
    sp(
        "",
        "scrollbarColor",
        STR,
        r#"`auto` or `<thumb colour> <track colour>`; inherited."#,
    ),
    sp(
        "Text (inherited, as in CSS). `currentColor` in colours means `color`.",
        "color",
        STR,
        "",
    ),
    sp(
        "",
        "font",
        STR,
        r#"`[style] [weight] size[/line-height] family`, e.g. `"italic bold 16px/1.4 Inter"`."#,
    ),
    sp("", "fontFamily", STR, ""),
    sp("", "fontSize", &[Num, Str], ""),
    sp(
        "",
        "fontWeight",
        &[Num, Str],
        r#"1-1000, `normal`, `bold`, `bolder`, `lighter`, or a name (`thin` ... `black`)."#,
    ),
    sp(
        "",
        "fontStyle",
        &[Lit("normal"), Lit("italic"), Lit("oblique")],
        "",
    ),
    sp("", "lineHeight", &[Num, Str], ""),
    sp("", "letterSpacing", &[Num, Str], ""),
    sp("", "wordSpacing", &[Num, Str], ""),
    sp(
        "",
        "textAlign",
        &[
            Lit("start"),
            Lit("end"),
            Lit("left"),
            Lit("right"),
            Lit("center"),
            Lit("justify"),
        ],
        "",
    ),
    sp("", "direction", &[Lit("ltr"), Lit("rtl")], ""),
    sp(
        "",
        "textDecoration",
        STR,
        r#"Lines, style, colour and thickness, e.g. `"underline wavy red 2px"`."#,
    ),
    sp("", "textDecorationLine", STR, ""),
    sp(
        "",
        "textDecorationStyle",
        &[
            Lit("solid"),
            Lit("double"),
            Lit("dotted"),
            Lit("dashed"),
            Lit("wavy"),
        ],
        "",
    ),
    sp("", "textDecorationColor", STR, ""),
    sp(
        "",
        "textDecorationThickness",
        &[Length, Lit("auto"), Lit("from-font")],
        "",
    ),
    sp(
        "",
        "textUnderlineOffset",
        &[Length, Lit("auto")],
        r#"Distance of the underline below the baseline."#,
    ),
    sp(
        "",
        "WebkitTextStroke",
        STR,
        r#"Outlined text: width and colour, drawn over the fill (`color: "transparent"` for hollow text)."#,
    ),
    sp("", "WebkitTextStrokeWidth", LENGTH, ""),
    sp("", "WebkitTextStrokeColor", STR, ""),
    sp(
        "",
        "imageRendering",
        &[
            Lit("auto"),
            Lit("smooth"),
            Lit("high-quality"),
            Lit("pixelated"),
            Lit("crisp-edges"),
        ],
        r#"`pixelated` / `crisp-edges` scale images (backgrounds, `Image`, border images) without smoothing. Inherited."#,
    ),
    sp(
        "",
        "visibility",
        &[Lit("visible"), Lit("hidden"), Lit("collapse")],
        r#"`hidden` (or `collapse`) keeps the layout space but draws nothing and takes no hits; children can be `visible` again. Inherited."#,
    ),
    sp(
        "",
        "transition",
        STR,
        r#"`transition: opacity 0.3s ease-out, transform 200ms 50ms`. Lengths, numbers, colours and
same-shaped transforms, filters and shadows animate; other properties change at once."#,
    ),
    sp("", "transitionProperty", STR, ""),
    sp(
        "",
        "transitionDuration",
        STR,
        r#"Times with units: `"0.3s"`, `"150ms"`."#,
    ),
    sp("", "transitionDelay", STR, ""),
    sp(
        "",
        "transitionTimingFunction",
        STR,
        r#"`ease`, `linear`, `ease-in`, `ease-out`, `ease-in-out`, `cubic-bezier()`, `steps()`, ..."#,
    ),
    sp(
        "",
        "transitionBehavior",
        &[Lit("normal"), Lit("allow-discrete")],
        "",
    ),
    sp(
        "",
        "animation",
        STR,
        r#"`animation: spin 1s linear infinite`, naming an `@keyframes` rule from a stylesheet."#,
    ),
    sp("", "animationName", STR, ""),
    sp("", "animationDuration", STR, ""),
    sp("", "animationDelay", STR, ""),
    sp(
        "",
        "animationIterationCount",
        &[Num, Lit("infinite"), Str],
        "",
    ),
    sp(
        "",
        "animationDirection",
        &[
            Lit("normal"),
            Lit("reverse"),
            Lit("alternate"),
            Lit("alternate-reverse"),
            Str,
        ],
        "",
    ),
    sp(
        "",
        "animationFillMode",
        &[
            Lit("none"),
            Lit("forwards"),
            Lit("backwards"),
            Lit("both"),
            Str,
        ],
        "",
    ),
    sp(
        "",
        "animationPlayState",
        &[Lit("running"), Lit("paused"), Str],
        "",
    ),
    sp("", "animationTimingFunction", STR, ""),
    sp(
        "",
        "userSelect",
        &[
            Lit("auto"),
            Lit("text"),
            Lit("none"),
            Lit("all"),
            Lit("contain"),
        ],
        "",
    ),
    sp(
        "",
        "WebkitUserSelect",
        &[
            Lit("auto"),
            Lit("text"),
            Lit("none"),
            Lit("all"),
            Lit("contain"),
        ],
        "",
    ),
    sp(
        "",
        "listStyleType",
        STR,
        r#"List markers: `disc`, `circle`, `square`, `decimal`, `decimal-leading-zero`, `lower-alpha`, `upper-alpha`, `lower-roman`, `upper-roman`, `lower-greek`, `none` or a quoted string. Inherited."#,
    ),
    sp(
        "",
        "listStylePosition",
        &[Lit("inside"), Lit("outside")],
        r#"`outside` (default: hangs left of the item) or `inside`."#,
    ),
    sp("", "listStyleImage", STR, r#"`url(...)` or `none`."#),
    sp(
        "",
        "listStyle",
        STR,
        r#"Type, position and image in any order."#,
    ),
    sp(
        "",
        "caretColor",
        STR,
        r#"Text field caret; `auto` is the text colour."#,
    ),
    sp(
        "",
        "textEffect",
        STR,
        r#"Per-glyph effects drawn by the renderer (the typewriter look): names separated by spaces, or `none`; they combine. Entrances: `fade default drop zoom-in zoom-out slide-left slide-right slide-up slide-down blink`. One-shots after a glyph appears: `bounce pixar-bounce hop rope pulse haunted`. Always on: `wavy rainbow insanity faulty shake-x shake-y swing`, and `cursor` (a block at the first unrevealed glyph). They run from the moment `textReveal` first shows a glyph. Inherited."#,
    ),
    sp(
        "",
        "textReveal",
        &[Num, Lit("all")],
        r#"How many glyphs are shown (a fraction eases the last in); `all` (default) shows everything. Inherited."#,
    ),
    sp(
        "",
        "fontSmooth",
        &[Lit("auto"), Lit("never"), Lit("always")],
        r#"`never` draws glyphs without anti-aliasing (pixel fonts). s&box keywords: `auto`, `never`, `always`."#,
    ),
    sp(
        "",
        "WebkitFontSmoothing",
        &[
            Lit("auto"),
            Lit("none"),
            Lit("antialiased"),
            Lit("subpixel-antialiased"),
        ],
        "",
    ),
    sp(
        "",
        "textStroke",
        STR,
        r#"s&box's name for `WebkitTextStroke`."#,
    ),
    sp("", "textStrokeWidth", LENGTH, ""),
    sp("", "textStrokeColor", STR, ""),
    sp(
        "",
        "textDecorationSkipInk",
        &[Lit("auto"), Lit("none"), Lit("all")],
        r#"Parsed; underlines don't skip descenders yet."#,
    ),
    sp(
        "",
        "whiteSpace",
        &[
            Lit("normal"),
            Lit("nowrap"),
            Lit("pre"),
            Lit("pre-wrap"),
            Lit("pre-line"),
            Lit("break-spaces"),
        ],
        "",
    ),
    sp(
        "",
        "wordBreak",
        &[
            Lit("normal"),
            Lit("break-all"),
            Lit("keep-all"),
            Lit("break-word"),
        ],
        "",
    ),
    sp(
        "",
        "overflowWrap",
        &[Lit("normal"), Lit("anywhere"), Lit("break-word")],
        r#"`anywhere` / `break-word` break long words that don't fit. `wordWrap` is an alias."#,
    ),
    sp(
        "",
        "wordWrap",
        &[Lit("normal"), Lit("anywhere"), Lit("break-word")],
        "",
    ),
    sp("", "textShadow", STR, ""),
    sp(
        "",
        "fontVariantNumeric",
        STR,
        r#"e.g. `"tabular-nums"`, `"slashed-zero"`."#,
    ),
    sp(
        "",
        "fontFeatureSettings",
        STR,
        r#"OpenType features, e.g. `"'ss01', 'liga' off"`."#,
    ),
    sp(
        "",
        "textTransform",
        &[
            Lit("none"),
            Lit("uppercase"),
            Lit("lowercase"),
            Lit("capitalize"),
        ],
        "",
    ),
    sp(
        "",
        "textOverflow",
        &[Lit("clip"), Lit("ellipsis")],
        r#"Not inherited. `ellipsis` cuts overflowing lines when `overflow` clips (use with `whiteSpace: "nowrap"`)."#,
    ),
];
