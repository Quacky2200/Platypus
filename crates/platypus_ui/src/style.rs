// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Applying style properties from React to a node.
//!
//! Names are React-style camelCase. Numbers are logical pixels; strings are CSS
//! values parsed with Taffy's own CSS parser (layout) or [`Rgba::parse`] (colours).
//! The same functions will back CSS files in M3.

use std::str::FromStr;

use taffy::{
    AlignContent, AlignItems, Dimension, Display, FlexDirection, FlexWrap, GridAutoFlow,
    GridPlacement, GridTemplateComponent, GridTemplateTracks, LengthPercentage,
    LengthPercentageAuto, Line, Position, Rect, Size, Style,
};
use web_ui_core::Value;

use std::sync::Arc;

use crate::Rgba;
use crate::paint::{
    BackgroundClip, BackgroundImage, BackgroundPosition, BackgroundRepeat, BackgroundSize,
    BlendMode, BorderImage, BorderStyle, ClipPath, ColorMatrix, CornerRadius, Corners, DropShadow,
    Filter, Len, MaskMode, ObjectFit, Overflow, Shadow, parse_background_image,
    parse_background_position, parse_background_repeat, parse_background_size, parse_clip_rect,
    parse_layers, parse_radii, parse_shadows, split_top_level,
};
use crate::text_style::{self, TextProps};
use crate::transform::Transforms;

/// Paint-only properties (layout properties live in Taffy's `Style`).
#[derive(Clone, Debug, PartialEq)]
pub struct Visual {
    pub background: Rgba,
    /// `backgroundColor: currentColor`: use the text colour.
    pub background_current: bool,
    /// Background layers, top first (`None` for a `none` layer). Shared so building
    /// the draw list each frame doesn't copy gradient stops.
    pub background_images: Vec<Option<Arc<BackgroundImage>>>,
    /// Per-layer lists; layer `i` uses entry `i % len`, like CSS.
    pub background_sizes: Vec<BackgroundSize>,
    pub background_positions: Vec<BackgroundPosition>,
    pub background_repeats: Vec<BackgroundRepeat>,
    pub background_clips: Vec<BackgroundClip>,
    /// `maskImage` layers (top first) and their lists, like the background ones.
    /// Any mask puts the element in a layer.
    pub mask_images: Vec<Option<Arc<BackgroundImage>>>,
    pub mask_sizes: Vec<BackgroundSize>,
    pub mask_positions: Vec<BackgroundPosition>,
    pub mask_repeats: Vec<BackgroundRepeat>,
    pub mask_modes: Vec<MaskMode>,
    /// `backgroundBlendMode`, per layer like the other background lists.
    pub background_blends: Vec<BlendMode>,
    /// `mixBlendMode`: blends the element (as a group) with what's beneath it.
    pub mix_blend: BlendMode,
    /// Per side: top, right, bottom, left.
    pub border_colors: [Rgba; 4],
    /// `currentColor` per side (also the CSS default when `border` omits a colour).
    pub border_currents: [bool; 4],
    pub border_styles: [BorderStyle; 4],
    /// Transforms and their related properties; `None` when all are at their defaults.
    pub transform: Option<Arc<Transforms>>,
    /// `borderImage` and its longhands; `None` is the default (no image).
    pub border_image: Option<Arc<BorderImage>>,
    /// s&box's `background-image-tint` / `border-image-tint`: a colour multiplied into
    /// the image pixels (background images and an `image` element's picture; the
    /// border image). Gradients aren't tinted.
    pub background_image_tint: Option<Rgba>,
    pub border_image_tint: Option<Rgba>,
    /// `outline`: drawn outside the border box (plus `outline_offset`), over the
    /// element's content; it takes no space. Style `none` (the default) hides it.
    pub outline_width: f32,
    pub outline_style: BorderStyle,
    pub outline_color: Rgba,
    /// `outlineColor: currentColor` (the default).
    pub outline_current: bool,
    pub outline_offset: f32,
    pub border_radius: Corners<CornerRadius>,
    pub shadows: Vec<Shadow>,
    /// `filter: blur(..)` radius; 0 means none.
    pub filter_blur: f32,
    /// `filter` colour functions, combined (`None`: no colour change).
    pub filter_matrix: Option<ColorMatrix>,
    pub drop_shadows: Vec<DropShadow>,
    /// `backdropFilter: blur(..)` radius; 0 means none.
    pub backdrop_blur: f32,
    pub backdrop_matrix: Option<ColorMatrix>,
    pub opacity: f32,
    /// `pointerEvents`: `Some(false)` for `none`; `None` inherits from the parent.
    pub pointer_events: Option<bool>,
    /// `order`: reorders flex and grid items (layout and painting; not Tab order).
    pub order: i32,
    /// `zIndex`: paint order among siblings (`None` is `auto`).
    pub z_index: Option<i32>,
    /// `isolation: isolate` (accepted; every z-indexed element already stacks on its
    /// own in Platypus's simplified model).
    pub isolate: bool,
    /// `willChange` names a property that makes a containing block (`transform`, `filter`,
    /// ...); see [`makes_containing_block`].
    pub will_change: bool,
    /// `overflowX`, `overflowY` as specified (see [`Overflow::computed`]).
    pub overflow: [Overflow; 2],
    /// `clipPath`: clips the element and its descendants.
    pub clip_path: Option<ClipPath>,
    /// Legacy `clip: rect(...)` (offsets of top, right, bottom, left; `None` = `auto`).
    /// Only applies to absolutely positioned elements.
    pub clip_rect: Option<[Option<f32>; 4]>,
    /// `scrollbarWidth`: `auto`, `thin` or `none` (hidden, still scrollable).
    pub scrollbar_width: ScrollbarWidth,
    /// `scrollbarGutter: stable`: reserve the scrollbar's space even before the content
    /// overflows (Platypus's scrollbars otherwise overlay the content).
    pub scrollbar_gutter: bool,
    /// `overscrollBehavior` (x, y): `true` for `contain` / `none`, which stop scrolling
    /// from passing to the scroll containers outside once this one hits its end.
    pub overscroll_contain: [bool; 2],
    /// `scrollBehavior: smooth`: script scrolls that don't name a behaviour animate.
    pub scroll_smooth: bool,
    /// `scrollPadding` (top, right, bottom, left) in px: insets of a scroll container's view
    /// that scrolling an element into view keeps it clear of (a sticky header, a fade).
    pub scroll_padding: [f32; 4],
    /// `scrollbarColor` as (thumb, track); `None` inherits, `Some(None)` is `auto`.
    pub scrollbar_color: Option<Option<(Rgba, Rgba)>>,
    /// `display: contents`: no box of its own; children join the parent's layout.
    /// Taffy has no such display, so `Style::display` stays `Block`.
    pub contents: bool,
    /// `display: list-item`: a block with a marker, like the children of `list`.
    pub list_item: bool,
    /// A stylesheet rule set the start padding (lists keep it over their default indent).
    pub padding_set: bool,
    /// `position: fixed`: laid out (as absolute) against the window and painted above
    /// the normal tree. Taffy has no fixed, so `Style::position` is `Absolute`.
    pub fixed: bool,
    /// `objectFit` and `objectPosition` (default `50% 50%`) for `image` elements.
    pub object_fit: ObjectFit,
    pub object_position: BackgroundPosition,
    /// `cursor` (a CSS keyword from [`CURSORS`]); `None` inherits from the parent.
    pub cursor: Option<&'static str>,
    /// `cursor` images to try before the keyword (`url(x.png) 4 4, pointer`).
    pub cursor_images: Vec<CursorImage>,
    /// SVG presentation properties (`fill`, `stroke`, ...) and the SVG meaning of some
    /// ordinary ones (`opacity`, `transform`), as written, for inline SVG output.
    pub svg_props: Vec<(String, String)>,
    /// `transition` and its longhands.
    pub transitions: crate::motion::TransitionLists,
    /// `animation` and its longhands.
    pub animations: crate::motion::AnimationLists,
    /// Inherited text properties set on this node (see [`text_style`]).
    pub text: TextProps,
}

impl Default for Visual {
    fn default() -> Self {
        Self {
            background: Rgba::TRANSPARENT,
            background_current: false,
            background_images: Vec::new(),
            background_sizes: vec![BackgroundSize::default()],
            background_positions: vec![BackgroundPosition::default()],
            background_repeats: vec![BackgroundRepeat::default()],
            background_clips: vec![BackgroundClip::default()],
            background_blends: vec![BlendMode::Normal],
            mask_images: Vec::new(),
            mask_sizes: vec![BackgroundSize::default()],
            mask_positions: vec![BackgroundPosition::default()],
            mask_repeats: vec![BackgroundRepeat::default()],
            mask_modes: vec![MaskMode::Alpha],
            mix_blend: BlendMode::Normal,
            border_colors: [Rgba::TRANSPARENT; 4],
            border_currents: [false; 4],
            border_styles: [BorderStyle::Solid; 4],
            border_image: None,
            background_image_tint: None,
            border_image_tint: None,
            transform: None,
            outline_width: 3.0,
            outline_style: BorderStyle::None,
            outline_color: Rgba::TRANSPARENT,
            outline_current: true,
            outline_offset: 0.0,
            border_radius: Corners::all(CornerRadius::ZERO),
            shadows: Vec::new(),
            filter_blur: 0.0,
            filter_matrix: None,
            drop_shadows: Vec::new(),
            backdrop_blur: 0.0,
            backdrop_matrix: None,
            opacity: 1.0,
            pointer_events: None,
            order: 0,
            z_index: None,
            isolate: false,
            will_change: false,
            overflow: [Overflow::Visible; 2],
            clip_path: None,
            clip_rect: None,
            scrollbar_width: ScrollbarWidth::Auto,
            scrollbar_gutter: false,
            overscroll_contain: [false; 2],
            scroll_smooth: false,
            scroll_padding: [0.0; 4],
            scrollbar_color: None,
            contents: false,
            list_item: false,
            padding_set: false,
            fixed: false,
            object_fit: ObjectFit::Fill,
            object_position: BackgroundPosition {
                x: Len::Percent(0.5),
                y: Len::Percent(0.5),
            },
            cursor: None,
            cursor_images: Vec::new(),
            svg_props: Vec::new(),
            transitions: crate::motion::TransitionLists::default(),
            animations: crate::motion::AnimationLists::default(),
            text: TextProps::default(),
        }
    }
}

/// A custom cursor image from `cursor: url(...) [x y], ...`.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorImage {
    /// The image path (inside the game folder) or `data:` URL.
    pub url: String,
    /// The hotspot in image pixels; `None` uses the file's own (`.cur`) or the
    /// top-left corner.
    pub hotspot: Option<(f32, f32)>,
}

/// Parses `cursor`: any number of `url(...) [x y]` images, then a keyword.
fn parse_cursor(value: &str) -> Option<(&'static str, Vec<CursorImage>)> {
    let parts = crate::paint::split_top_level(value, ',');
    let (keyword, images) = parts.split_last()?;
    let keyword = keyword.trim();
    let keyword = *CURSORS.iter().find(|c| **c == keyword)?;
    let images = images
        .iter()
        .map(|part| {
            let part = part.trim();
            let close = part.find(')')?;
            let url = part.strip_prefix("url(")?[..close - 4].trim();
            let url = url.trim_matches(['"', '\'']);
            let rest: Vec<f32> = part[close + 1..]
                .split_whitespace()
                .map(|n| n.parse::<f32>().ok().filter(|n| n.is_finite()))
                .collect::<Option<_>>()?;
            let hotspot = match rest[..] {
                [] => None,
                [x, y] => Some((x, y)),
                _ => return None,
            };
            (!url.is_empty()).then(|| CursorImage {
                url: url.to_owned(),
                hotspot,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((keyword, images))
}

/// CSS `cursor` keywords (the table in `tables::style`). `auto` is the text cursor over text and the
/// arrow elsewhere.
pub const CURSORS: &[&str] = crate::tables::style::CURSORS;

/// `scrollbarWidth`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollbarWidth {
    #[default]
    Auto,
    Thin,
    None,
}

impl ScrollbarWidth {
    /// Thickness in logical pixels.
    pub fn thickness(self) -> f32 {
        match self {
            ScrollbarWidth::Auto => 8.0,
            ScrollbarWidth::Thin => 5.0,
            ScrollbarWidth::None => 0.0,
        }
    }
}

/// A CSS-wide keyword: `initial`, `inherit`, `unset`, or `revert` / `revert-layer`
/// (which act like `unset`, as Platypus has no user-agent stylesheet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WideKeyword {
    Initial,
    Inherit,
    Unset,
}

impl WideKeyword {
    pub fn parse(value: &Value) -> Option<Self> {
        let s = value.as_str()?.trim();
        Some(match s.to_ascii_lowercase().as_str() {
            "initial" => WideKeyword::Initial,
            "inherit" => WideKeyword::Inherit,
            "unset" | "revert" | "revert-layer" => WideKeyword::Unset,
            _ => return None,
        })
    }
}

/// Properties that inherit by default: text properties, `cursor`, `pointerEvents`
/// and `scrollbarColor`.
pub fn is_inherited(prop: &str) -> bool {
    text_style::is_text_prop(prop) || matches!(prop, "cursor" | "pointerEvents" | "scrollbarColor")
}

/// The initial value of an inherited property, as CSS text (setting it explicitly
/// stops inheritance, unlike resetting, which inherits).
fn inherited_initial(prop: &str) -> &'static str {
    match prop {
        "color" => "black",
        "fontFamily" => "sans-serif",
        "fontSize" => "16px",
        "fontWeight" => "400",
        "fontStyle" => "normal",
        "lineHeight" => "normal",
        "letterSpacing" | "wordSpacing" => "0px",
        "textAlign" => "start",
        "direction" => "ltr",
        "textDecoration" | "textDecorationLine" => "none",
        "whiteSpace" => "normal",
        "textShadow" => "none",
        "scrollbarColor" => "auto",
        // cursor, pointerEvents
        _ => "auto",
    }
}

/// Applies a CSS-wide keyword to `prop`. `parent` is the parent's style, for
/// `inherit` on a property that doesn't inherit by itself (none at the root).
pub fn apply_wide_keyword(
    style: &mut Style,
    visual: &mut Visual,
    prop: &str,
    keyword: WideKeyword,
    parent: Option<(&Style, &Visual)>,
) -> Result<(), StyleError> {
    let inherited = is_inherited(prop);
    match (keyword, inherited) {
        // Inherited properties inherit when they aren't set.
        (WideKeyword::Inherit | WideKeyword::Unset, true) => reset(style, visual, prop),
        (WideKeyword::Initial, true) => {
            apply_style(style, visual, prop, &Value::from(inherited_initial(prop)))
        }
        (WideKeyword::Initial | WideKeyword::Unset, false) => reset(style, visual, prop),
        (WideKeyword::Inherit, false) => match parent {
            Some((ps, pv)) => copy_prop(style, visual, ps.clone(), pv.clone(), prop),
            None => reset(style, visual, prop),
        },
    }
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum StyleError {
    #[error("unknown style property `{0}`")]
    UnknownProperty(String),
    #[error("invalid value {value:?} for `{prop}`")]
    InvalidValue { prop: String, value: Value },
}

/// Sets one property. `Value::Null` resets it to its default. On error nothing changes.
pub fn apply_style(
    style: &mut Style,
    visual: &mut Visual,
    prop: &str,
    value: &Value,
) -> Result<(), StyleError> {
    if !crate::svg::keeps_raw(prop) {
        return apply_property(style, visual, prop, value);
    }
    let text = match value {
        Value::String(s) => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    // `clip-path: url(#id)` and `mask: url(#id)` refer to SVG elements.
    let svg_only = crate::svg::is_svg_prop(prop)
        || (matches!(prop, "clipPath" | "mask")
            && text.as_deref().is_some_and(|t| t.starts_with("url(#")));
    let result = if svg_only {
        Ok(())
    } else {
        apply_property(style, visual, prop, value)
    };
    if result.is_ok() {
        visual.svg_props.retain(|(p, _)| p != prop);
        if let Some(text) = text {
            visual.svg_props.push((prop.to_owned(), text));
        }
    }
    result
}

/// [`apply_style`] for one property, without the SVG bookkeeping.
fn apply_property(
    style: &mut Style,
    visual: &mut Visual,
    prop: &str,
    value: &Value,
) -> Result<(), StyleError> {
    let invalid = || StyleError::InvalidValue {
        prop: prop.to_owned(),
        value: value.clone(),
    };
    if *value == Value::Null {
        return reset(style, visual, prop);
    }
    let d = |v: &Value| dimension(v).ok_or_else(invalid);
    let lpa = |v: &Value| length_percentage_auto(v).ok_or_else(invalid);
    let lp = |v: &Value| length_percentage(v).ok_or_else(invalid);
    let num = |v: &Value| v.as_f64().map(|n| n as f32).ok_or_else(invalid);
    let kw = || value.as_str().ok_or_else(invalid);

    match prop {
        "display" => {
            let k = kw()?.trim();
            visual.contents = k == "contents";
            visual.list_item = k == "list-item";
            style.display = if visual.contents || visual.list_item {
                Display::Block
            } else {
                parse_kw::<Display>(k).ok_or_else(invalid)?
            };
        }
        "position" => {
            let k = kw()?.trim();
            visual.fixed = k == "fixed";
            style.position = if visual.fixed {
                Position::Absolute
            } else {
                parse_kw::<Position>(k).ok_or_else(invalid)?
            };
        }
        "float" => {
            style.float = match kw()?.trim() {
                "none" => taffy::Float::None,
                "left" | "inline-start" => taffy::Float::Left,
                "right" | "inline-end" => taffy::Float::Right,
                _ => return Err(invalid()),
            }
        }
        "clear" => {
            style.clear = match kw()?.trim() {
                "none" => taffy::Clear::None,
                "left" | "inline-start" => taffy::Clear::Left,
                "right" | "inline-end" => taffy::Clear::Right,
                "both" => taffy::Clear::Both,
                _ => return Err(invalid()),
            }
        }
        // Platypus (like Taffy and s&box) defaults to border-box; CSS's default is content-box.
        "boxSizing" => {
            style.box_sizing = match kw()?.trim() {
                "border-box" => taffy::BoxSizing::BorderBox,
                "content-box" => taffy::BoxSizing::ContentBox,
                _ => return Err(invalid()),
            }
        }

        "width" => style.size.width = d(value)?,
        "height" => style.size.height = d(value)?,
        "minWidth" => style.min_size.width = lpa(value)?,
        "minHeight" => style.min_size.height = lpa(value)?,
        "maxWidth" => style.max_size.width = lpa(value)?,
        "maxHeight" => style.max_size.height = lpa(value)?,
        "aspectRatio" => style.aspect_ratio = Some(num(value)?),

        "top" => style.inset.top = lpa(value)?,
        "right" => style.inset.right = lpa(value)?,
        "bottom" => style.inset.bottom = lpa(value)?,
        "left" => style.inset.left = lpa(value)?,
        "inset" => style.inset = sides(value, length_percentage_auto).ok_or_else(invalid)?,

        "margin" => style.margin = sides(value, length_percentage_auto).ok_or_else(invalid)?,
        "marginTop" => style.margin.top = lpa(value)?,
        "marginRight" => style.margin.right = lpa(value)?,
        "marginBottom" => style.margin.bottom = lpa(value)?,
        "marginLeft" => style.margin.left = lpa(value)?,

        "padding" => style.padding = sides(value, length_percentage).ok_or_else(invalid)?,
        "paddingTop" => style.padding.top = lp(value)?,
        "paddingRight" => style.padding.right = lp(value)?,
        "paddingBottom" => style.padding.bottom = lp(value)?,
        "paddingLeft" => style.padding.left = lp(value)?,

        "borderWidth" => style.border = sides(value, length_percentage).ok_or_else(invalid)?,
        "borderTopWidth" => style.border.top = lp(value)?,
        "borderRightWidth" => style.border.right = lp(value)?,
        "borderBottomWidth" => style.border.bottom = lp(value)?,
        "borderLeftWidth" => style.border.left = lp(value)?,

        "gap" => {
            // CSS order: row gap, then column gap.
            let [row, column] = pair(value, length_percentage).ok_or_else(invalid)?;
            style.gap = Size {
                width: column,
                height: row,
            };
        }
        "rowGap" => style.gap.height = lp(value)?,
        "columnGap" => style.gap.width = lp(value)?,

        "flexDirection" => {
            style.flex_direction = parse_kw::<FlexDirection>(kw()?).ok_or_else(invalid)?
        }
        "flexWrap" => style.flex_wrap = parse_kw::<FlexWrap>(kw()?).ok_or_else(invalid)?,
        // `flex-flow: <direction> || <wrap>`, either order, each optional.
        "flexFlow" => {
            let (mut direction, mut wrap) = (FlexDirection::Row, FlexWrap::NoWrap);
            let tokens: Vec<&str> = kw()?.split_whitespace().collect();
            if tokens.is_empty() || tokens.len() > 2 {
                return Err(invalid());
            }
            let (mut seen_direction, mut seen_wrap) = (false, false);
            for t in tokens {
                if let (Some(d), false) = (parse_kw::<FlexDirection>(t), seen_direction) {
                    (direction, seen_direction) = (d, true);
                } else if let (Some(w), false) = (parse_kw::<FlexWrap>(t), seen_wrap) {
                    (wrap, seen_wrap) = (w, true);
                } else {
                    return Err(invalid());
                }
            }
            style.flex_direction = direction;
            style.flex_wrap = wrap;
        }
        "flexGrow" => style.flex_grow = num(value)?,
        "flexShrink" => style.flex_shrink = num(value)?,
        "flexBasis" => style.flex_basis = d(value)?,
        "flex" => {
            let (grow, shrink, basis) = flex_shorthand(value).ok_or_else(invalid)?;
            style.flex_grow = grow;
            style.flex_shrink = shrink;
            style.flex_basis = basis;
        }

        "alignItems" => style.align_items = parse_kw::<AlignItems>(kw()?).ok_or_else(invalid)?,
        "justifyItems" => {
            style.justify_items = parse_kw::<AlignItems>(kw()?).ok_or_else(invalid)?
        }
        "alignSelf" => style.align_self = align_self(kw()?).ok_or_else(invalid)?,
        "justifySelf" => style.justify_self = align_self(kw()?).ok_or_else(invalid)?,
        "alignContent" => {
            style.align_content = parse_kw::<AlignContent>(kw()?).ok_or_else(invalid)?
        }
        "justifyContent" => {
            style.justify_content = parse_kw::<AlignContent>(kw()?).ok_or_else(invalid)?
        }

        "gridTemplateColumns" => {
            let tracks = grid_template(kw()?).ok_or_else(invalid)?;
            style.grid_template_columns = tracks.tracks;
            style.grid_template_column_names = tracks.line_names;
        }
        "gridTemplateRows" => {
            let tracks = grid_template(kw()?).ok_or_else(invalid)?;
            style.grid_template_rows = tracks.tracks;
            style.grid_template_row_names = tracks.line_names;
        }
        "gridAutoFlow" => {
            style.grid_auto_flow = parse_kw::<GridAutoFlow>(kw()?).ok_or_else(invalid)?
        }
        "gridColumn" => style.grid_column = grid_line(value).ok_or_else(invalid)?,
        "gridRow" => style.grid_row = grid_line(value).ok_or_else(invalid)?,
        "gridRowStart" => style.grid_row.start = placement(value).ok_or_else(invalid)?,
        "gridRowEnd" => style.grid_row.end = placement(value).ok_or_else(invalid)?,
        "gridColumnStart" => style.grid_column.start = placement(value).ok_or_else(invalid)?,
        "gridColumnEnd" => style.grid_column.end = placement(value).ok_or_else(invalid)?,
        "gridAutoRows" => style.grid_auto_rows = auto_tracks(kw()?).ok_or_else(invalid)?,
        "gridAutoColumns" => style.grid_auto_columns = auto_tracks(kw()?).ok_or_else(invalid)?,
        "gridTemplateAreas" => {
            style.grid_template_areas = match kw()?.trim() {
                "none" => None,
                s => Some(
                    template_areas(&quoted_strings(s).ok_or_else(invalid)?).ok_or_else(invalid)?,
                ),
            }
        }
        // `grid-area: name` or `row-start / column-start / row-end / column-end`.
        "gridArea" => {
            let parts: Vec<&str> = kw()?.split('/').map(str::trim).collect();
            let p = |i: usize| -> Result<GridPlacement<String>, StyleError> {
                match parts.get(i) {
                    Some(s) => s.parse().map_err(|_| invalid()),
                    // Omitted ends copy a named start, else auto.
                    None => Ok(
                        match parts
                            .first()
                            .and_then(|s| s.parse::<GridPlacement<String>>().ok())
                        {
                            Some(named @ GridPlacement::NamedLine(..)) if i != 0 => named,
                            _ => GridPlacement::Auto,
                        },
                    ),
                }
            };
            if parts.is_empty() || parts.len() > 4 {
                return Err(invalid());
            }
            let (rs, cs) = (p(0)?, p(1)?);
            let re = match parts.get(2) {
                Some(_) => p(2)?,
                None => same_name_or_auto(&rs),
            };
            let ce = match parts.get(3) {
                Some(_) => p(3)?,
                None => same_name_or_auto(&cs),
            };
            style.grid_row = Line { start: rs, end: re };
            style.grid_column = Line { start: cs, end: ce };
        }
        // `grid-template: rows / columns`, or area strings with optional row sizes:
        // `"a a" 40px "b c" 1fr / 1fr 2fr`.
        "gridTemplate" => {
            let text = kw()?.trim();
            if text == "none" {
                style.grid_template_rows = Vec::new();
                style.grid_template_columns = Vec::new();
                style.grid_template_row_names = Vec::new();
                style.grid_template_column_names = Vec::new();
                style.grid_template_areas = None;
                return Ok(());
            }
            let (rows, columns) = match split_top_level(text, '/')[..] {
                [r, c] => (r, Some(c)),
                [r] => (r, None),
                _ => return Err(invalid()),
            };
            if rows.contains('"') {
                let (areas, sizes) = areas_and_sizes(rows).ok_or_else(invalid)?;
                let tracks = grid_template(&sizes.join(" ")).ok_or_else(invalid)?;
                style.grid_template_areas = Some(template_areas(&areas).ok_or_else(invalid)?);
                style.grid_template_rows = tracks.tracks;
                style.grid_template_row_names = tracks.line_names;
            } else {
                let tracks = grid_template(rows).ok_or_else(invalid)?;
                style.grid_template_rows = tracks.tracks;
                style.grid_template_row_names = tracks.line_names;
                style.grid_template_areas = None;
            }
            match columns {
                Some(c) => {
                    let tracks = grid_template(c).ok_or_else(invalid)?;
                    style.grid_template_columns = tracks.tracks;
                    style.grid_template_column_names = tracks.line_names;
                }
                None => style.grid_template_columns = Vec::new(),
            }
        }
        // `place-*: <align> [<justify>]` (one value sets both).
        "placeItems" => {
            let [a, j] = two_keywords(kw()?).ok_or_else(invalid)?;
            style.align_items = parse_kw::<AlignItems>(a).ok_or_else(invalid)?;
            style.justify_items = parse_kw::<AlignItems>(j).ok_or_else(invalid)?;
        }
        "placeSelf" => {
            let [a, j] = two_keywords(kw()?).ok_or_else(invalid)?;
            style.align_self = align_self(a).ok_or_else(invalid)?;
            style.justify_self = align_self(j).ok_or_else(invalid)?;
        }
        "placeContent" => {
            let [a, j] = two_keywords(kw()?).ok_or_else(invalid)?;
            style.align_content = parse_kw::<AlignContent>(a).ok_or_else(invalid)?;
            style.justify_content = parse_kw::<AlignContent>(j).ok_or_else(invalid)?;
        }
        // Logical properties, mapped for horizontal left-to-right writing.
        p if logical_sides(p).is_some() => {
            let sides = logical_sides(p).expect("checked");
            match sides {
                Logical::One(physical) => apply_style(style, visual, physical, value)?,
                Logical::Two(start, end) => {
                    let parts: Vec<Value> = match value {
                        Value::String(s) => split_top_level(s, ' ')
                            .into_iter()
                            .map(Value::from)
                            .collect(),
                        other => vec![other.clone()],
                    };
                    let (a, b) = match &parts[..] {
                        [one] => (one.clone(), one.clone()),
                        [a, b] => (a.clone(), b.clone()),
                        _ => return Err(invalid()),
                    };
                    // Check both before changing either, so an error changes nothing.
                    let (mut s2, mut v2) = (style.clone(), visual.clone());
                    apply_style(&mut s2, &mut v2, start, &a)?;
                    apply_style(&mut s2, &mut v2, end, &b)?;
                    *style = s2;
                    *visual = v2;
                }
            }
        }

        "backgroundColor" if is_current_color(value) => visual.background_current = true,
        "backgroundColor" => {
            visual.background = color(value).ok_or_else(invalid)?;
            visual.background_current = false;
        }
        "background" => {
            // Shorthand subset: a colour, or a list of images/gradients.
            let s = kw()?;
            if let Some(c) = Rgba::parse(s) {
                visual.background = c;
                visual.background_current = false;
                visual.background_images.clear();
            } else {
                visual.background_images = image_layers(s).ok_or_else(invalid)?;
                visual.background = Rgba::TRANSPARENT;
            }
        }
        "backgroundImage" => visual.background_images = image_layers(kw()?).ok_or_else(invalid)?,
        "backgroundSize" => {
            visual.background_sizes =
                parse_layers(kw()?, parse_background_size).ok_or_else(invalid)?
        }
        "backgroundPosition" => {
            visual.background_positions =
                parse_layers(kw()?, parse_background_position).ok_or_else(invalid)?
        }
        "backgroundPositionX" | "backgroundPositionY" => {
            let horizontal = prop == "backgroundPositionX";
            let axis = |t: &str| {
                // `top center` is a valid position, but `top` isn't an x value.
                let other_axis: &[&str] = if horizontal {
                    &["top", "bottom"]
                } else {
                    &["left", "right"]
                };
                if t.split_whitespace().any(|w| other_axis.contains(&w)) {
                    return None;
                }
                let p = if horizontal {
                    parse_background_position(&format!("{t} center"))
                } else {
                    parse_background_position(&format!("center {t}"))
                }?;
                Some(if horizontal { p.x } else { p.y })
            };
            let values = parse_layers(kw()?, axis).ok_or_else(invalid)?;
            let old = &visual.background_positions;
            let n = values.len().max(old.len());
            visual.background_positions = (0..n)
                .map(|i| {
                    let mut p = old[i % old.len()];
                    let v = values[i % values.len()];
                    if horizontal {
                        p.x = v
                    } else {
                        p.y = v
                    }
                    p
                })
                .collect();
        }
        "backgroundClip" | "WebkitBackgroundClip" => {
            visual.background_clips =
                parse_layers(kw()?, BackgroundClip::parse).ok_or_else(invalid)?
        }
        "backgroundImageTint" | "borderImageTint" => {
            let tint = match kw()?.trim() {
                "none" => None,
                c => Some(Rgba::parse(c).ok_or_else(invalid)?),
            };
            if prop == "backgroundImageTint" {
                visual.background_image_tint = tint;
            } else {
                visual.border_image_tint = tint;
            }
        }
        "backgroundBlendMode" => {
            visual.background_blends = parse_layers(kw()?, BlendMode::parse).ok_or_else(invalid)?
        }
        // The `mask` shorthand here takes just the images.
        "maskImage" | "WebkitMaskImage" | "mask" => {
            visual.mask_images = image_layers(kw()?).ok_or_else(invalid)?
        }
        "maskSize" | "WebkitMaskSize" => {
            visual.mask_sizes = parse_layers(kw()?, parse_background_size).ok_or_else(invalid)?
        }
        "maskPosition" | "WebkitMaskPosition" => {
            visual.mask_positions =
                parse_layers(kw()?, parse_background_position).ok_or_else(invalid)?
        }
        "maskRepeat" | "WebkitMaskRepeat" => {
            visual.mask_repeats =
                parse_layers(kw()?, parse_background_repeat).ok_or_else(invalid)?
        }
        "maskMode" => {
            visual.mask_modes = parse_layers(kw()?, MaskMode::parse).ok_or_else(invalid)?
        }
        "mixBlendMode" => visual.mix_blend = BlendMode::parse(kw()?).ok_or_else(invalid)?,
        "objectFit" => visual.object_fit = ObjectFit::parse(kw()?).ok_or_else(invalid)?,
        "objectPosition" => {
            visual.object_position = parse_background_position(kw()?).ok_or_else(invalid)?
        }
        "backgroundRepeat" => {
            visual.background_repeats =
                parse_layers(kw()?, parse_background_repeat).ok_or_else(invalid)?
        }
        "borderColor" => {
            let colors = four_sides(value, |t| {
                if t.eq_ignore_ascii_case("currentcolor") {
                    Some(None)
                } else {
                    Rgba::parse(t).map(Some)
                }
            })
            .ok_or_else(invalid)?;
            for (side, c) in colors.into_iter().enumerate() {
                visual.border_currents[side] = c.is_none();
                visual.border_colors[side] = c.unwrap_or(Rgba::TRANSPARENT);
            }
        }
        "transform"
        | "translate"
        | "rotate"
        | "scale"
        | "transformOrigin"
        | "perspective"
        | "perspectiveOrigin"
        | "transformStyle"
        | "backfaceVisibility"
        | "WebkitBackfaceVisibility" => {
            // `scale: 1.5` and `perspective: 800` (pixels) take numbers, as `PlatypusStyle` declares.
            let text;
            let s = match (prop, value) {
                ("scale", Value::Number(n)) => {
                    text = n.to_string();
                    text.as_str()
                }
                ("perspective", Value::Number(n)) => {
                    text = format!("{n}px");
                    text.as_str()
                }
                _ => kw()?,
            };
            let t = Arc::make_mut(visual.transform.get_or_insert_with(Default::default));
            match prop {
                "transform" => {
                    t.spec.functions = crate::transform::parse_transform(s).ok_or_else(invalid)?
                }
                "translate" => {
                    t.spec.translate = crate::transform::parse_translate(s).ok_or_else(invalid)?
                }
                "rotate" => {
                    t.spec.rotate = crate::transform::parse_rotate(s).ok_or_else(invalid)?
                }
                "scale" => t.spec.scale = crate::transform::parse_scale(s).ok_or_else(invalid)?,
                "transformOrigin" => {
                    t.origin = crate::transform::parse_origin(s).ok_or_else(invalid)?
                }
                "perspective" => {
                    t.perspective = match s.trim() {
                        "none" => None,
                        d => match Len::parse(d) {
                            Some(Len::Px(px)) if px >= 0.0 => Some(px),
                            _ => return Err(invalid()),
                        },
                    }
                }
                "perspectiveOrigin" => {
                    let p = parse_background_position(s).ok_or_else(invalid)?;
                    t.perspective_origin = (p.x, p.y);
                }
                "transformStyle" => {
                    t.preserve_3d = match s.trim() {
                        "flat" => false,
                        "preserve-3d" => true,
                        _ => return Err(invalid()),
                    }
                }
                _ => {
                    t.backface_hidden = match s.trim() {
                        "visible" => false,
                        "hidden" => true,
                        _ => return Err(invalid()),
                    }
                }
            }
        }
        "borderImage" => {
            visual.border_image = Some(Arc::new(BorderImage::parse(kw()?).ok_or_else(invalid)?))
        }
        "borderImageSource" | "borderImageSlice" | "borderImageWidth" | "borderImageOutset"
        | "borderImageRepeat" => {
            let s = kw()?;
            let image = Arc::make_mut(visual.border_image.get_or_insert_with(Default::default));
            match prop {
                "borderImageSource" => {
                    image.source = parse_background_image(s).ok_or_else(invalid)?.map(Arc::new)
                }
                "borderImageSlice" => {
                    (image.slice, image.fill) = BorderImage::parse_slice(s).ok_or_else(invalid)?
                }
                "borderImageWidth" => {
                    image.width = BorderImage::parse_lengths(s, false).ok_or_else(invalid)?
                }
                "borderImageOutset" => {
                    image.outset = BorderImage::parse_lengths(s, true).ok_or_else(invalid)?
                }
                _ => image.repeat = BorderImage::parse_repeat(s).ok_or_else(invalid)?,
            }
        }
        "outline" => {
            let (width, s, color) = outline_shorthand(value).ok_or_else(invalid)?;
            visual.outline_width = width;
            visual.outline_style = s;
            visual.outline_current = color.is_none();
            visual.outline_color = color.unwrap_or(Rgba::TRANSPARENT);
        }
        "outlineWidth" => visual.outline_width = line_width(value).ok_or_else(invalid)?,
        "outlineStyle" => visual.outline_style = outline_style(kw()?).ok_or_else(invalid)?,
        "outlineColor" if is_current_color(value) => visual.outline_current = true,
        "outlineColor" => {
            visual.outline_color = color(value).ok_or_else(invalid)?;
            visual.outline_current = false;
        }
        "outlineOffset" => {
            visual.outline_offset = match value {
                Value::Number(n) => *n as f32,
                Value::String(s) => match Len::parse(s) {
                    Some(Len::Px(px)) => px,
                    _ => return Err(invalid()),
                },
                _ => return Err(invalid()),
            }
        }
        "borderStyle" => {
            let styles = four_sides(value, BorderStyle::parse).ok_or_else(invalid)?;
            for (side, s) in styles.into_iter().enumerate() {
                set_border_style(style, visual, side, s);
            }
        }
        "border" => {
            let (width, s, color) = border_shorthand(value).ok_or_else(invalid)?;
            for side in 0..4 {
                set_border_side(style, visual, side, width, s, color);
            }
        }
        _ if border_side(prop).is_some() => {
            let (side, part) = border_side(prop).expect("checked");
            match part {
                "" => {
                    let (width, s, color) = border_shorthand(value).ok_or_else(invalid)?;
                    set_border_side(style, visual, side, width, s, color);
                }
                "Color" if is_current_color(value) => visual.border_currents[side] = true,
                "Color" => {
                    visual.border_colors[side] = color(value).ok_or_else(invalid)?;
                    visual.border_currents[side] = false;
                }
                "Style" => {
                    let s = BorderStyle::parse(kw()?).ok_or_else(invalid)?;
                    set_border_style(style, visual, side, s);
                }
                _ => return Err(StyleError::UnknownProperty(prop.to_owned())),
            }
        }
        "borderRadius" => {
            visual.border_radius = match value {
                Value::Number(n) if *n >= 0.0 => {
                    Corners::all(CornerRadius::circular(Len::Px(*n as f32)))
                }
                Value::String(s) => parse_radii(s).ok_or_else(invalid)?,
                _ => return Err(invalid()),
            }
        }
        "borderTopLeftRadius" => {
            visual.border_radius.top_left = radius(value).ok_or_else(invalid)?
        }
        "borderTopRightRadius" => {
            visual.border_radius.top_right = radius(value).ok_or_else(invalid)?
        }
        "borderBottomRightRadius" => {
            visual.border_radius.bottom_right = radius(value).ok_or_else(invalid)?
        }
        "borderBottomLeftRadius" => {
            visual.border_radius.bottom_left = radius(value).ok_or_else(invalid)?
        }
        "boxShadow" => visual.shadows = parse_shadows(kw()?).ok_or_else(invalid)?,
        "filter" => {
            let f = Filter::parse(kw()?).ok_or_else(invalid)?;
            visual.filter_blur = f.blur;
            visual.filter_matrix = f.matrix;
            visual.drop_shadows = f.drop_shadows;
        }
        "backdropFilter" => {
            let f = Filter::parse(kw()?).ok_or_else(invalid)?;
            // Behind the element there's no shape to shadow.
            if !f.drop_shadows.is_empty() {
                return Err(invalid());
            }
            visual.backdrop_blur = f.blur;
            visual.backdrop_matrix = f.matrix;
        }
        "opacity" => {
            visual.opacity = number_or_percent(value)
                .ok_or_else(invalid)?
                .clamp(0.0, 1.0)
        }
        "order" => visual.order = integer(value).ok_or_else(invalid)?,
        "zIndex" => {
            visual.z_index = match value {
                Value::String(s) if s.trim() == "auto" => None,
                v => Some(integer(v).ok_or_else(invalid)?),
            }
        }
        "transition"
        | "transitionProperty"
        | "transitionDuration"
        | "transitionDelay"
        | "transitionTimingFunction"
        | "transitionBehavior" => {
            use crate::motion as m;
            // Stylesheet values like `0` arrive as numbers.
            let text = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Err(invalid()),
            };
            let t = &mut visual.transitions;
            match prop {
                "transition" => *t = m::parse_transition(&text).ok_or_else(invalid)?,
                "transitionProperty" => {
                    t.property = m::parse_list(&text, m::parse_property).ok_or_else(invalid)?
                }
                "transitionDuration" => {
                    t.duration = m::parse_list(&text, m::parse_time).ok_or_else(invalid)?
                }
                "transitionDelay" => {
                    t.delay = m::parse_list(&text, m::parse_time).ok_or_else(invalid)?
                }
                "transitionTimingFunction" => {
                    t.timing = m::parse_list(&text, m::Timing::parse).ok_or_else(invalid)?
                }
                // `allow-discrete` is accepted; discrete properties never transition.
                _ => {
                    m::parse_list(&text, |b| {
                        matches!(b, "normal" | "allow-discrete").then_some(())
                    })
                    .ok_or_else(invalid)?;
                }
            }
        }
        "animation"
        | "animationName"
        | "animationDuration"
        | "animationDelay"
        | "animationIterationCount"
        | "animationDirection"
        | "animationFillMode"
        | "animationPlayState"
        | "animationTimingFunction" => {
            use crate::motion as m;
            let text = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Err(invalid()),
            };
            let a = &mut visual.animations;
            let bad = invalid;
            match prop {
                "animation" => *a = m::parse_animation(&text).ok_or_else(bad)?,
                "animationName" => a.name = m::parse_list(&text, m::parse_name).ok_or_else(bad)?,
                "animationDuration" => {
                    a.duration = m::parse_list(&text, m::parse_time).ok_or_else(bad)?
                }
                "animationDelay" => {
                    a.delay = m::parse_list(&text, m::parse_time).ok_or_else(bad)?
                }
                "animationIterationCount" => {
                    a.iterations = m::parse_list(&text, m::parse_iterations).ok_or_else(bad)?
                }
                "animationDirection" => {
                    a.direction = m::parse_list(&text, m::parse_direction).ok_or_else(bad)?
                }
                "animationFillMode" => {
                    a.fill = m::parse_list(&text, m::parse_fill).ok_or_else(bad)?
                }
                "animationPlayState" => {
                    a.paused = m::parse_list(&text, m::parse_play_state).ok_or_else(bad)?
                }
                _ => a.timing = m::parse_list(&text, m::Timing::parse).ok_or_else(bad)?,
            }
        }
        "isolation" => {
            visual.isolate = match kw()?.trim() {
                "auto" => false,
                "isolate" => true,
                _ => return Err(invalid()),
            }
        }
        "willChange" => visual.will_change = parse_will_change(kw()?).ok_or_else(invalid)?,
        "contain" => style.contain = parse_contain(kw()?).ok_or_else(invalid)?,
        "overflow" => set_overflow(
            style,
            visual,
            Overflow::parse_pair(kw()?).ok_or_else(invalid)?,
        ),
        "overflowX" => {
            let o = Overflow::parse(kw()?).ok_or_else(invalid)?;
            set_overflow(style, visual, [o, visual.overflow[1]]);
        }
        "overflowY" => {
            let o = Overflow::parse(kw()?).ok_or_else(invalid)?;
            set_overflow(style, visual, [visual.overflow[0], o]);
        }
        "clipPath" => {
            visual.clip_path = match kw()?.trim() {
                "none" => None,
                s => Some(ClipPath::parse(s).ok_or_else(invalid)?),
            }
        }
        "scrollbarWidth" => {
            visual.scrollbar_width = match kw()?.trim() {
                "auto" => ScrollbarWidth::Auto,
                "thin" => ScrollbarWidth::Thin,
                "none" => ScrollbarWidth::None,
                _ => return Err(invalid()),
            };
            set_overflow(style, visual, visual.overflow);
        }
        "scrollbarGutter" => {
            visual.scrollbar_gutter = match kw()?.split_whitespace().collect::<Vec<_>>()[..] {
                ["auto"] => false,
                ["stable"] | ["stable", "both-edges"] | ["both-edges", "stable"] => true,
                _ => return Err(invalid()),
            };
            set_overflow(style, visual, visual.overflow);
        }
        "overscrollBehavior"
        | "overscrollBehaviorX"
        | "overscrollBehaviorY"
        | "overscrollBehaviorInline"
        | "overscrollBehaviorBlock" => {
            let one = |k: &str| match k {
                "auto" => Some(false),
                "contain" | "none" => Some(true),
                _ => None,
            };
            let words: Vec<&str> = kw()?.split_whitespace().collect();
            match (prop, &words[..]) {
                ("overscrollBehavior", [v]) => {
                    let v = one(v).ok_or_else(invalid)?;
                    visual.overscroll_contain = [v, v];
                }
                ("overscrollBehavior", [x, y]) => {
                    visual.overscroll_contain =
                        [one(x).ok_or_else(invalid)?, one(y).ok_or_else(invalid)?];
                }
                ("overscrollBehaviorX" | "overscrollBehaviorInline", [v]) => {
                    visual.overscroll_contain[0] = one(v).ok_or_else(invalid)?;
                }
                ("overscrollBehaviorY" | "overscrollBehaviorBlock", [v]) => {
                    visual.overscroll_contain[1] = one(v).ok_or_else(invalid)?;
                }
                _ => return Err(invalid()),
            }
        }
        "scrollPadding"
        | "scrollPaddingTop"
        | "scrollPaddingRight"
        | "scrollPaddingBottom"
        | "scrollPaddingLeft"
        | "scrollPaddingBlock"
        | "scrollPaddingInline" => {
            // `auto` is 0 here: Platypus has no UA-chosen padding.
            let one = |t: &str| match t {
                "auto" => Some(0.0),
                t => match Len::parse(t) {
                    Some(Len::Px(px)) if px >= 0.0 => Some(px),
                    _ => None,
                },
            };
            let words: Vec<f32> = match value {
                Value::Number(n) if *n >= 0.0 => vec![*n as f32],
                Value::String(s) => s
                    .split_whitespace()
                    .map(one)
                    .collect::<Option<_>>()
                    .ok_or_else(invalid)?,
                _ => return Err(invalid()),
            };
            let p = &mut visual.scroll_padding;
            match (prop, &words[..]) {
                ("scrollPadding", [a]) => *p = [*a; 4],
                ("scrollPadding", [a, b]) => *p = [*a, *b, *a, *b],
                ("scrollPadding", [a, b, c]) => *p = [*a, *b, *c, *b],
                ("scrollPadding", [a, b, c, d]) => *p = [*a, *b, *c, *d],
                ("scrollPaddingTop", [a]) => p[0] = *a,
                ("scrollPaddingRight", [a]) => p[1] = *a,
                ("scrollPaddingBottom", [a]) => p[2] = *a,
                ("scrollPaddingLeft", [a]) => p[3] = *a,
                ("scrollPaddingBlock", [a]) => (p[0], p[2]) = (*a, *a),
                ("scrollPaddingBlock", [a, b]) => (p[0], p[2]) = (*a, *b),
                ("scrollPaddingInline", [a]) => (p[3], p[1]) = (*a, *a),
                ("scrollPaddingInline", [a, b]) => (p[3], p[1]) = (*a, *b),
                _ => return Err(invalid()),
            }
        }
        "scrollBehavior" => {
            visual.scroll_smooth = match kw()?.trim() {
                "auto" => false,
                "smooth" => true,
                _ => return Err(invalid()),
            };
        }
        "scrollbarColor" => {
            visual.scrollbar_color = match kw()?.trim() {
                "auto" => Some(None),
                "inherit" => None,
                s => {
                    let colors = split_top_level(s, ' ');
                    let [thumb, track] = colors[..] else {
                        return Err(invalid());
                    };
                    let parse = |c: &str| Rgba::parse(c).ok_or_else(invalid);
                    Some(Some((parse(thumb)?, parse(track)?)))
                }
            }
        }
        "clip" => {
            visual.clip_rect = match kw()?.trim() {
                "auto" => None,
                s => Some(parse_clip_rect(s).ok_or_else(invalid)?),
            }
        }
        "pointerEvents" => {
            visual.pointer_events = match kw()?.trim() {
                "auto" | "all" => Some(true),
                "none" => Some(false),
                "inherit" => None,
                _ => return Err(invalid()),
            }
        }
        "cursor" => {
            let kw = kw()?.trim();
            if kw == "inherit" {
                visual.cursor = None;
                visual.cursor_images.clear();
            } else {
                let (keyword, images) = parse_cursor(kw).ok_or_else(invalid)?;
                visual.cursor = Some(keyword);
                visual.cursor_images = images;
            }
        }

        p if text_style::is_text_prop(p) => text_style::apply(&mut visual.text, prop, value)
            .expect("is a text prop")
            .map_err(|()| invalid())?,

        _ => return Err(StyleError::UnknownProperty(prop.to_owned())),
    }
    Ok(())
}

/// `willChange`: whether the list names a property whose non-initial value would make the
/// element a containing block for absolute descendants (CSS says the hint does that too).
/// Other names are accepted and mean nothing.
fn parse_will_change(text: &str) -> Option<bool> {
    let text = text.trim();
    if text == "auto" {
        return Some(false);
    }
    let names: Vec<&str> = text.split(',').map(str::trim).collect();
    if names
        .iter()
        .any(|n| n.is_empty() || n.contains(char::is_whitespace))
    {
        return None;
    }
    Some(names.iter().any(|n| {
        matches!(
            *n,
            "transform"
                | "translate"
                | "rotate"
                | "scale"
                | "perspective"
                | "filter"
                | "backdrop-filter"
                | "transform-style"
                | "contain"
        )
    }))
}

/// `contain`: `none`, `strict`, `content` or any of `size`, `inline-size`, `layout`, `style`
/// and `paint`. Only layout and paint containment are kept (size containment isn't
/// implemented; style containment changes nothing Platypus has).
fn parse_contain(text: &str) -> Option<taffy::Contain> {
    use taffy::Contain;
    match text.trim() {
        "none" => return Some(Contain::NONE),
        "strict" | "content" => return Some(Contain::CONTENT),
        _ => {}
    }
    let mut out = Contain::NONE;
    let mut seen = Vec::new();
    for word in text.split_whitespace() {
        if seen.contains(&word) {
            return None;
        }
        seen.push(word);
        match word {
            "layout" => out = out.union(Contain::LAYOUT),
            "paint" => out = out.union(Contain::PAINT),
            "size" | "inline-size" | "style" => {}
            _ => return None,
        }
    }
    (!seen.is_empty()).then_some(out)
}

/// Whether an element with this style is the containing block of its absolute descendants
/// even when its `position` is `static`: a transform (or `perspective`, `preserve-3d`), a
/// filter, a backdrop filter, layout or paint containment, or `willChange` naming one of them.
pub fn makes_containing_block(style: &Style, visual: &Visual) -> bool {
    visual
        .transform
        .as_ref()
        .is_some_and(|t| !t.spec.is_none() || t.perspective.is_some() || t.preserve_3d)
        || visual.filter_blur > 0.0
        || visual.filter_matrix.is_some()
        || !visual.drop_shadows.is_empty()
        || visual.backdrop_blur > 0.0
        || visual.backdrop_matrix.is_some()
        || visual.will_change
        || style.contain.establishes_independent_formatting_context()
}

/// Resets `prop` to its initial value.
fn reset(style: &mut Style, visual: &mut Visual, prop: &str) -> Result<(), StyleError> {
    copy_prop(style, visual, Style::default(), Visual::default(), prop)
}

/// Sets `prop` from another style: the defaults (a reset) or the parent's (for
/// `inherit` on a property that doesn't inherit by itself). Text properties are set
/// back to inheriting.
pub(crate) fn copy_prop(
    style: &mut Style,
    visual: &mut Visual,
    d: Style,
    v: Visual,
    prop: &str,
) -> Result<(), StyleError> {
    if crate::svg::keeps_raw(prop) {
        visual.svg_props.retain(|(p, _)| p != prop);
        if let Some(raw) = v.svg_props.iter().find(|(p, _)| p == prop) {
            visual.svg_props.push(raw.clone());
        }
    }
    match prop {
        "display" => {
            style.display = d.display;
            visual.contents = v.contents;
            visual.list_item = v.list_item;
        }
        "position" => {
            style.position = d.position;
            visual.fixed = v.fixed;
        }
        "float" => style.float = d.float,
        "clear" => style.clear = d.clear,
        "boxSizing" => style.box_sizing = d.box_sizing,
        "width" => style.size.width = d.size.width,
        "height" => style.size.height = d.size.height,
        "minWidth" => style.min_size.width = d.min_size.width,
        "minHeight" => style.min_size.height = d.min_size.height,
        "maxWidth" => style.max_size.width = d.max_size.width,
        "maxHeight" => style.max_size.height = d.max_size.height,
        "aspectRatio" => style.aspect_ratio = d.aspect_ratio,
        "top" => style.inset.top = d.inset.top,
        "right" => style.inset.right = d.inset.right,
        "bottom" => style.inset.bottom = d.inset.bottom,
        "left" => style.inset.left = d.inset.left,
        "inset" => style.inset = d.inset,
        "margin" => style.margin = d.margin,
        "marginTop" => style.margin.top = d.margin.top,
        "marginRight" => style.margin.right = d.margin.right,
        "marginBottom" => style.margin.bottom = d.margin.bottom,
        "marginLeft" => style.margin.left = d.margin.left,
        "padding" => style.padding = d.padding,
        "paddingTop" => style.padding.top = d.padding.top,
        "paddingRight" => style.padding.right = d.padding.right,
        "paddingBottom" => style.padding.bottom = d.padding.bottom,
        "paddingLeft" => style.padding.left = d.padding.left,
        "borderWidth" => style.border = d.border,
        "borderTopWidth" => style.border.top = d.border.top,
        "borderRightWidth" => style.border.right = d.border.right,
        "borderBottomWidth" => style.border.bottom = d.border.bottom,
        "borderLeftWidth" => style.border.left = d.border.left,
        "gap" => style.gap = d.gap,
        "rowGap" => style.gap.height = d.gap.height,
        "columnGap" => style.gap.width = d.gap.width,
        "flexDirection" => style.flex_direction = d.flex_direction,
        "flexWrap" => style.flex_wrap = d.flex_wrap,
        "flexFlow" => {
            style.flex_direction = d.flex_direction;
            style.flex_wrap = d.flex_wrap;
        }
        "flexGrow" => style.flex_grow = d.flex_grow,
        "flexShrink" => style.flex_shrink = d.flex_shrink,
        "flexBasis" => style.flex_basis = d.flex_basis,
        "flex" => {
            style.flex_grow = d.flex_grow;
            style.flex_shrink = d.flex_shrink;
            style.flex_basis = d.flex_basis;
        }
        "alignItems" => style.align_items = d.align_items,
        "justifyItems" => style.justify_items = d.justify_items,
        "alignSelf" => style.align_self = d.align_self,
        "justifySelf" => style.justify_self = d.justify_self,
        "alignContent" => style.align_content = d.align_content,
        "justifyContent" => style.justify_content = d.justify_content,
        "gridTemplateColumns" => {
            style.grid_template_columns = d.grid_template_columns;
            style.grid_template_column_names = d.grid_template_column_names;
        }
        "gridTemplateRows" => {
            style.grid_template_rows = d.grid_template_rows;
            style.grid_template_row_names = d.grid_template_row_names;
        }
        "gridAutoFlow" => style.grid_auto_flow = d.grid_auto_flow,
        "gridColumn" => style.grid_column = d.grid_column,
        "gridRow" => style.grid_row = d.grid_row,
        "gridRowStart" => style.grid_row.start = d.grid_row.start,
        "gridRowEnd" => style.grid_row.end = d.grid_row.end,
        "gridColumnStart" => style.grid_column.start = d.grid_column.start,
        "gridColumnEnd" => style.grid_column.end = d.grid_column.end,
        "gridAutoRows" => style.grid_auto_rows = d.grid_auto_rows,
        "gridAutoColumns" => style.grid_auto_columns = d.grid_auto_columns,
        "gridTemplateAreas" => style.grid_template_areas = d.grid_template_areas,
        "gridArea" => {
            style.grid_row = d.grid_row;
            style.grid_column = d.grid_column;
        }
        "gridTemplate" => {
            style.grid_template_rows = d.grid_template_rows;
            style.grid_template_columns = d.grid_template_columns;
            style.grid_template_row_names = d.grid_template_row_names;
            style.grid_template_column_names = d.grid_template_column_names;
            style.grid_template_areas = d.grid_template_areas;
        }
        "placeItems" => {
            style.align_items = d.align_items;
            style.justify_items = d.justify_items;
        }
        "placeSelf" => {
            style.align_self = d.align_self;
            style.justify_self = d.justify_self;
        }
        "placeContent" => {
            style.align_content = d.align_content;
            style.justify_content = d.justify_content;
        }
        p if logical_sides(p).is_some() => match logical_sides(p).expect("checked") {
            Logical::One(physical) => return copy_prop(style, visual, d, v, physical),
            Logical::Two(start, end) => {
                copy_prop(style, visual, d.clone(), v.clone(), start)?;
                return copy_prop(style, visual, d, v, end);
            }
        },
        "backgroundColor" => {
            visual.background = v.background;
            visual.background_current = false;
        }
        "borderColor" => {
            visual.border_colors = v.border_colors;
            visual.border_currents = v.border_currents;
        }
        "borderStyle" => visual.border_styles = v.border_styles,
        "borderImage" => visual.border_image = v.border_image,
        "transform"
        | "translate"
        | "rotate"
        | "scale"
        | "transformOrigin"
        | "perspective"
        | "perspectiveOrigin"
        | "transformStyle"
        | "backfaceVisibility"
        | "WebkitBackfaceVisibility" => {
            let from = v.transform.unwrap_or_default();
            let t = Arc::make_mut(visual.transform.get_or_insert_with(Default::default));
            match prop {
                "transform" => t.spec.functions = from.spec.functions.clone(),
                "translate" => t.spec.translate = from.spec.translate,
                "rotate" => t.spec.rotate = from.spec.rotate,
                "scale" => t.spec.scale = from.spec.scale,
                "transformOrigin" => t.origin = from.origin,
                "perspective" => t.perspective = from.perspective,
                "perspectiveOrigin" => t.perspective_origin = from.perspective_origin,
                "transformStyle" => t.preserve_3d = from.preserve_3d,
                _ => t.backface_hidden = from.backface_hidden,
            }
        }
        "borderImageSource" | "borderImageSlice" | "borderImageWidth" | "borderImageOutset"
        | "borderImageRepeat" => {
            let from = v.border_image.unwrap_or_default();
            let image = Arc::make_mut(visual.border_image.get_or_insert_with(Default::default));
            match prop {
                "borderImageSource" => image.source = from.source.clone(),
                "borderImageSlice" => (image.slice, image.fill) = (from.slice, from.fill),
                "borderImageWidth" => image.width = from.width,
                "borderImageOutset" => image.outset = from.outset,
                _ => image.repeat = from.repeat,
            }
        }
        "outline" => {
            visual.outline_width = v.outline_width;
            visual.outline_style = v.outline_style;
            visual.outline_color = v.outline_color;
            visual.outline_current = v.outline_current;
        }
        "outlineWidth" => visual.outline_width = v.outline_width,
        "outlineStyle" => visual.outline_style = v.outline_style,
        "outlineColor" => {
            visual.outline_color = v.outline_color;
            visual.outline_current = v.outline_current;
        }
        "outlineOffset" => visual.outline_offset = v.outline_offset,
        _ if border_side(prop).is_some_and(|(_, part)| part != "Width") => {
            let (side, part) = border_side(prop).expect("checked");
            if matches!(part, "" | "Color") {
                visual.border_colors[side] = v.border_colors[side];
                visual.border_currents[side] = v.border_currents[side];
            }
            if matches!(part, "" | "Style") {
                visual.border_styles[side] = v.border_styles[side];
            }
            if part.is_empty() {
                let width = [d.border.top, d.border.right, d.border.bottom, d.border.left][side];
                set_side(&mut style.border, side, width);
            }
        }
        "background" => {
            visual.background = v.background;
            visual.background_images = v.background_images;
        }
        "backgroundImage" => visual.background_images = v.background_images,
        "backgroundSize" => visual.background_sizes = v.background_sizes,
        "backgroundPosition" | "backgroundPositionX" | "backgroundPositionY" => {
            visual.background_positions = v.background_positions
        }
        "backgroundClip" | "WebkitBackgroundClip" => visual.background_clips = v.background_clips,
        "backgroundBlendMode" => visual.background_blends = v.background_blends,
        "backgroundImageTint" => visual.background_image_tint = v.background_image_tint,
        "borderImageTint" => visual.border_image_tint = v.border_image_tint,
        "maskImage" | "WebkitMaskImage" | "mask" => visual.mask_images = v.mask_images,
        "maskSize" | "WebkitMaskSize" => visual.mask_sizes = v.mask_sizes,
        "maskPosition" | "WebkitMaskPosition" => visual.mask_positions = v.mask_positions,
        "maskRepeat" | "WebkitMaskRepeat" => visual.mask_repeats = v.mask_repeats,
        "maskMode" => visual.mask_modes = v.mask_modes,
        "mixBlendMode" => visual.mix_blend = v.mix_blend,
        "objectFit" => visual.object_fit = v.object_fit,
        "objectPosition" => visual.object_position = v.object_position,
        "backgroundRepeat" => visual.background_repeats = v.background_repeats,
        "border" => {
            style.border = d.border;
            visual.border_colors = v.border_colors;
            visual.border_currents = v.border_currents;
            visual.border_styles = v.border_styles;
        }
        "borderRadius" => visual.border_radius = v.border_radius,
        "borderTopLeftRadius" => visual.border_radius.top_left = v.border_radius.top_left,
        "borderTopRightRadius" => visual.border_radius.top_right = v.border_radius.top_right,
        "borderBottomRightRadius" => {
            visual.border_radius.bottom_right = v.border_radius.bottom_right
        }
        "borderBottomLeftRadius" => visual.border_radius.bottom_left = v.border_radius.bottom_left,
        "boxShadow" => visual.shadows = v.shadows,
        "filter" => {
            visual.filter_blur = v.filter_blur;
            visual.filter_matrix = v.filter_matrix;
            visual.drop_shadows = v.drop_shadows;
        }
        "backdropFilter" => {
            visual.backdrop_blur = v.backdrop_blur;
            visual.backdrop_matrix = v.backdrop_matrix;
        }
        "opacity" => visual.opacity = v.opacity,
        "order" => visual.order = v.order,
        "zIndex" => visual.z_index = v.z_index,
        "isolation" => visual.isolate = v.isolate,
        "willChange" => visual.will_change = v.will_change,
        "contain" => style.contain = d.contain,
        "transition" => visual.transitions = v.transitions,
        "transitionProperty" => visual.transitions.property = v.transitions.property,
        "transitionDuration" => visual.transitions.duration = v.transitions.duration,
        "transitionDelay" => visual.transitions.delay = v.transitions.delay,
        "transitionTimingFunction" => visual.transitions.timing = v.transitions.timing,
        "transitionBehavior" => {}
        "animation" => visual.animations = v.animations,
        "animationName" => visual.animations.name = v.animations.name,
        "animationDuration" => visual.animations.duration = v.animations.duration,
        "animationDelay" => visual.animations.delay = v.animations.delay,
        "animationIterationCount" => visual.animations.iterations = v.animations.iterations,
        "animationDirection" => visual.animations.direction = v.animations.direction,
        "animationFillMode" => visual.animations.fill = v.animations.fill,
        "animationPlayState" => visual.animations.paused = v.animations.paused,
        "animationTimingFunction" => visual.animations.timing = v.animations.timing,
        "overflow" => set_overflow(style, visual, v.overflow),
        "overflowX" => set_overflow(style, visual, [v.overflow[0], visual.overflow[1]]),
        "overflowY" => set_overflow(style, visual, [visual.overflow[0], v.overflow[1]]),
        "clipPath" => visual.clip_path = v.clip_path,
        "clip" => visual.clip_rect = v.clip_rect,
        "scrollbarWidth" => {
            visual.scrollbar_width = v.scrollbar_width;
            set_overflow(style, visual, visual.overflow);
        }
        "scrollbarGutter" => {
            visual.scrollbar_gutter = v.scrollbar_gutter;
            set_overflow(style, visual, visual.overflow);
        }
        "overscrollBehavior" => visual.overscroll_contain = v.overscroll_contain,
        "overscrollBehaviorX" | "overscrollBehaviorInline" => {
            visual.overscroll_contain[0] = v.overscroll_contain[0]
        }
        "overscrollBehaviorY" | "overscrollBehaviorBlock" => {
            visual.overscroll_contain[1] = v.overscroll_contain[1]
        }
        "scrollBehavior" => visual.scroll_smooth = v.scroll_smooth,
        "scrollPadding" => visual.scroll_padding = v.scroll_padding,
        "scrollPaddingTop" => visual.scroll_padding[0] = v.scroll_padding[0],
        "scrollPaddingRight" => visual.scroll_padding[1] = v.scroll_padding[1],
        "scrollPaddingBottom" => visual.scroll_padding[2] = v.scroll_padding[2],
        "scrollPaddingLeft" => visual.scroll_padding[3] = v.scroll_padding[3],
        "scrollPaddingBlock" => {
            visual.scroll_padding[0] = v.scroll_padding[0];
            visual.scroll_padding[2] = v.scroll_padding[2];
        }
        "scrollPaddingInline" => {
            visual.scroll_padding[1] = v.scroll_padding[1];
            visual.scroll_padding[3] = v.scroll_padding[3];
        }
        "scrollbarColor" => visual.scrollbar_color = v.scrollbar_color,
        "pointerEvents" => visual.pointer_events = v.pointer_events,
        "cursor" => {
            visual.cursor = v.cursor;
            visual.cursor_images = v.cursor_images.clone();
        }
        p if text_style::is_text_prop(p) => {
            let _ = text_style::apply(&mut visual.text, prop, &Value::Null);
        }
        // SVG-only: the raw value is all there is (copied below).
        p if crate::svg::is_svg_prop(p) => {}
        _ => return Err(StyleError::UnknownProperty(prop.to_owned())),
    }
    Ok(())
}

/// Sets `overflow` on both the visual (clipping, scrolling) and Taffy's style, where it
/// changes automatic minimum sizes and makes the content size available.
fn set_overflow(style: &mut Style, visual: &mut Visual, overflow: [Overflow; 2]) {
    visual.overflow = overflow;
    let taffy = |o: Overflow| match o {
        Overflow::Visible => taffy::Overflow::Visible,
        Overflow::Clip => taffy::Overflow::Clip,
        Overflow::Hidden => taffy::Overflow::Hidden,
        Overflow::Scroll | Overflow::Auto => taffy::Overflow::Scroll,
    };
    let [x, y] = Overflow::computed(overflow);
    style.overflow = taffy::Point {
        x: taffy(x),
        y: taffy(y),
    };
    // `scrollbar-gutter: stable` on a box that clips or scrolls vertically: Taffy keeps
    // room for the scrollbar (and its margins) at the end, which it only does for
    // `Scroll` (equivalent to `Hidden` for layout otherwise).
    let gutter = visual.scrollbar_gutter
        && matches!(y, Overflow::Hidden | Overflow::Scroll | Overflow::Auto);
    style.scrollbar_width = if gutter {
        style.overflow.y = taffy::Overflow::Scroll;
        visual.scrollbar_width.thickness() + 2.0 * SCROLLBAR_MARGIN
    } else {
        0.0
    };
}

/// Space between an overlay scrollbar and the padding box's edge.
pub const SCROLLBAR_MARGIN: f32 = 2.0;

/// A whole number, from a number or a string (`3`, `"-1"`).
fn integer(v: &Value) -> Option<i32> {
    let n = match v {
        Value::Number(n) => *n,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    (n.fract() == 0.0).then_some(n as i32)
}

/// A number, or a percentage as a fraction (`50%` → 0.5), from a number or string.
fn number_or_percent(v: &Value) -> Option<f32> {
    match v {
        Value::Number(n) => Some(*n as f32),
        Value::String(s) => {
            let s = s.trim();
            match s.strip_suffix('%') {
                Some(p) => p.trim().parse::<f32>().ok().map(|p| p / 100.0),
                None => s.parse().ok(),
            }
        }
        _ => None,
    }
}

/// `background-image` layers: images, gradients or `none`; a lone `none` is no layers.
fn image_layers(s: &str) -> Option<Vec<Option<Arc<BackgroundImage>>>> {
    let layers = parse_layers(s, |l| parse_background_image(l).map(|i| i.map(Arc::new)))?;
    Some(if layers.iter().all(Option::is_none) {
        Vec::new()
    } else {
        layers
    })
}

fn parse_kw<T: FromStr>(s: &str) -> Option<T> {
    s.trim().parse().ok()
}

/// A unitless number inside a CSS string (`"0"` in `"0 20px"`). CSS only allows
/// unitless zero, but numbers mean pixels everywhere in Platypus, so any number is
/// accepted. Taffy's parser rejects these, so they're handled first.
fn plain_number(s: &str) -> Option<f32> {
    s.trim().parse().ok()
}

fn dimension(v: &Value) -> Option<Dimension> {
    match v {
        Value::Number(n) => Some(Dimension::length(*n as f32)),
        // Math with percentages, evaluated by layout (see `crate::calc`).
        Value::String(s) if crate::calc::is_math(s) => {
            Some(Dimension::calc(crate::calc::handle(s)?))
        }
        Value::String(s) => match plain_number(s) {
            Some(n) => Some(Dimension::length(n)),
            None => s.trim().parse().ok(),
        },
        _ => None,
    }
}

fn length_percentage_auto(v: &Value) -> Option<LengthPercentageAuto> {
    match v {
        Value::Number(n) => Some(LengthPercentageAuto::length(*n as f32)),
        Value::String(s) if crate::calc::is_math(s) => {
            Some(LengthPercentageAuto::calc(crate::calc::handle(s)?))
        }
        Value::String(s) => match plain_number(s) {
            Some(n) => Some(LengthPercentageAuto::length(n)),
            None => s.trim().parse().ok(),
        },
        _ => None,
    }
}

fn length_percentage(v: &Value) -> Option<LengthPercentage> {
    match v {
        Value::Number(n) => Some(LengthPercentage::length(*n as f32)),
        Value::String(s) if crate::calc::is_math(s) => {
            Some(LengthPercentage::calc(crate::calc::handle(s)?))
        }
        Value::String(s) => match plain_number(s) {
            Some(n) => Some(LengthPercentage::length(n)),
            None => s.trim().parse().ok(),
        },
        _ => None,
    }
}

fn color(v: &Value) -> Option<Rgba> {
    Rgba::parse(v.as_str()?)
}

fn is_current_color(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("currentcolor"))
}

/// One corner radius: a number (px), `"8px"`, `"50%"`, or `"10px 20px"` (elliptical).
fn radius(v: &Value) -> Option<CornerRadius> {
    match v {
        Value::Number(n) if *n >= 0.0 => Some(CornerRadius::circular(Len::Px(*n as f32))),
        Value::String(s) => CornerRadius::parse(s),
        _ => None,
    }
}

/// CSS `border` shorthand: width, style and colour in any order, e.g. `"2px solid #fff"`.
/// Like CSS, a missing style means `none` (no border) and a missing colour comes back
/// as `None` (`currentColor`, resolved from the text colour). Only `solid`,
/// `none` and `hidden` styles are supported; other styles are rejected.
/// `border` / `borderTop`...: width, style and colour in any order. Like CSS, no
/// style (or `none`/`hidden`) means no border; a style without a width is `medium`.
/// The colour is `None` for `currentColor` (also when omitted).
fn border_shorthand(v: &Value) -> Option<(LengthPercentage, BorderStyle, Option<Rgba>)> {
    let s = v.as_str()?;
    let (mut width, mut style, mut color) = (None, None, None);
    for token in split_top_level(s, ' ') {
        let w = match token {
            "thin" => Some(1.0),
            "medium" => Some(3.0),
            "thick" => Some(5.0),
            t => match Len::parse(t) {
                Some(Len::Px(px)) if px >= 0.0 => Some(px),
                _ => None,
            },
        };
        if let Some(w) = w
            && width.is_none()
        {
            width = Some(w);
        } else if let Some(st) = BorderStyle::parse(token).filter(|_| style.is_none()) {
            style = Some(st);
        } else if color.is_none() && token.eq_ignore_ascii_case("currentcolor") {
            color = Some(None);
        } else if color.is_none() {
            color = Some(Some(Rgba::parse(token)?));
        } else {
            return None;
        }
    }
    let style = style.unwrap_or(BorderStyle::None);
    let width = if style.is_visible() {
        width.unwrap_or(3.0)
    } else {
        0.0
    };
    Some((LengthPercentage::length(width), style, color.flatten()))
}

/// `outline`: width, style and colour in any order; unlike `border`, a missing
/// width or style keeps its default (`medium`, `none`). `auto` is drawn solid.
fn outline_shorthand(v: &Value) -> Option<(f32, BorderStyle, Option<Rgba>)> {
    let (mut width, mut style, mut color) = (None, None, None);
    for token in split_top_level(v.as_str()?, ' ') {
        if let Some(w) = line_width(&Value::String(token.to_owned())).filter(|_| width.is_none()) {
            width = Some(w);
        } else if let Some(s) = outline_style(token).filter(|_| style.is_none()) {
            style = Some(s);
        } else if color.is_none() && token.eq_ignore_ascii_case("currentcolor") {
            color = Some(None);
        } else if color.is_none() {
            color = Some(Some(Rgba::parse(token)?));
        } else {
            return None;
        }
    }
    Some((
        width.unwrap_or(3.0),
        style.unwrap_or(BorderStyle::None),
        color.flatten(),
    ))
}

/// A border or outline style; outlines also accept `auto` (drawn solid) but not
/// `hidden`.
fn outline_style(s: &str) -> Option<BorderStyle> {
    match s.trim() {
        "auto" => Some(BorderStyle::Solid),
        "hidden" => None,
        other => BorderStyle::parse(other),
    }
}

/// `thin`, `medium`, `thick` or a non-negative length in px.
fn line_width(v: &Value) -> Option<f32> {
    match v {
        Value::Number(n) if *n >= 0.0 => Some(*n as f32),
        Value::String(s) => match s.trim() {
            "thin" => Some(1.0),
            "medium" => Some(3.0),
            "thick" => Some(5.0),
            t => match Len::parse(t)? {
                Len::Px(px) if px >= 0.0 => Some(px),
                _ => None,
            },
        },
        _ => None,
    }
}

/// `borderTop`, `borderRightColor`, `borderLeftStyle`, ...: the side (0 top, 1 right,
/// 2 bottom, 3 left) and the rest of the name (`""`, `Color`, `Style`, `Width`).
fn border_side(prop: &str) -> Option<(usize, &str)> {
    let rest = prop.strip_prefix("border")?;
    ["Top", "Right", "Bottom", "Left"]
        .iter()
        .enumerate()
        .find_map(|(i, side)| {
            let part = rest.strip_prefix(side)?;
            matches!(part, "" | "Color" | "Style" | "Width").then_some((i, part))
        })
}

fn set_side<T>(sides: &mut Rect<T>, side: usize, value: T) {
    match side {
        0 => sides.top = value,
        1 => sides.right = value,
        2 => sides.bottom = value,
        _ => sides.left = value,
    }
}

/// Sets one side's style; like CSS, `none` and `hidden` take no space.
fn set_border_style(style: &mut Style, visual: &mut Visual, side: usize, s: BorderStyle) {
    visual.border_styles[side] = s;
    if !s.is_visible() {
        set_side(&mut style.border, side, LengthPercentage::length(0.0));
    }
}

fn set_border_side(
    style: &mut Style,
    visual: &mut Visual,
    side: usize,
    width: LengthPercentage,
    s: BorderStyle,
    color: Option<Rgba>,
) {
    set_side(&mut style.border, side, width);
    visual.border_styles[side] = s;
    visual.border_currents[side] = color.is_none();
    visual.border_colors[side] = color.unwrap_or(Rgba::TRANSPARENT);
}

/// 1-4 space-separated values for top, right, bottom, left (CSS box order).
fn four_sides<T: Copy>(v: &Value, parse: impl Fn(&str) -> Option<T>) -> Option<[T; 4]> {
    let values: Vec<T> = split_top_level(v.as_str()?, ' ')
        .into_iter()
        .map(parse)
        .collect::<Option<_>>()?;
    Some(match values[..] {
        [all] => [all; 4],
        [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
        [top, horizontal, bottom] => [top, horizontal, bottom, horizontal],
        [top, right, bottom, left] => [top, right, bottom, left],
        _ => return None,
    })
}

/// CSS box shorthand: one number, or 1–4 space-separated values (top right bottom left).
fn sides<T: Copy>(v: &Value, parse: fn(&Value) -> Option<T>) -> Option<Rect<T>> {
    let values: Vec<T> = match v {
        Value::Number(_) => vec![parse(v)?],
        // Top-level split, so `calc(10% - 2px)` stays one value.
        Value::String(s) => split_top_level(s, ' ')
            .into_iter()
            .map(|part| parse(&Value::String(part.to_owned())))
            .collect::<Option<_>>()?,
        _ => return None,
    };
    let (top, right, bottom, left) = match values[..] {
        [all] => (all, all, all, all),
        [vertical, horizontal] => (vertical, horizontal, vertical, horizontal),
        [top, horizontal, bottom] => (top, horizontal, bottom, horizontal),
        [top, right, bottom, left] => (top, right, bottom, left),
        _ => return None,
    };
    Some(Rect {
        top,
        right,
        bottom,
        left,
    })
}

/// One or two values (`gap: 10` or `gap: "10px 20px"`).
fn pair<T: Copy>(v: &Value, parse: fn(&Value) -> Option<T>) -> Option<[T; 2]> {
    let values: Vec<T> = match v {
        Value::Number(_) => vec![parse(v)?],
        // Top-level split, so `calc(10% - 2px)` stays one value.
        Value::String(s) => split_top_level(s, ' ')
            .into_iter()
            .map(|part| parse(&Value::String(part.to_owned())))
            .collect::<Option<_>>()?,
        _ => return None,
    };
    match values[..] {
        [both] => Some([both, both]),
        [first, second] => Some([first, second]),
        _ => None,
    }
}

/// `flex: <number>` (grow, shrink 1, basis 0), `"none"` or `"auto"`.
fn flex_shorthand(v: &Value) -> Option<(f32, f32, Dimension)> {
    match v {
        Value::Number(n) => Some((*n as f32, 1.0, Dimension::length(0.0))),
        Value::String(s) => match s.trim() {
            "none" => Some((0.0, 0.0, Dimension::auto())),
            "auto" => Some((1.0, 1.0, Dimension::auto())),
            other => other
                .parse::<f32>()
                .ok()
                .map(|n| (n, 1.0, Dimension::length(0.0))),
        },
        _ => None,
    }
}

/// `alignSelf`/`justifySelf`: `"auto"` means "inherit from the parent" (None).
fn align_self(s: &str) -> Option<Option<AlignItems>> {
    match s.trim() {
        "auto" => Some(None),
        other => other.parse().ok().map(Some),
    }
}

fn grid_template(s: &str) -> Option<GridTemplateTracks<String, GridTemplateComponent<String>>> {
    s.trim().parse().ok()
}

/// A single grid line: `2`, `"span 2"`, `"header"` (named), `"auto"`.
fn placement(v: &Value) -> Option<GridPlacement<String>> {
    match v {
        Value::Number(n) => n.to_string().parse().ok(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `grid-area: a` gives `a / a / a / a`; an omitted end copies a named start.
fn same_name_or_auto(start: &GridPlacement<String>) -> GridPlacement<String> {
    match start {
        GridPlacement::NamedLine(..) => start.clone(),
        _ => GridPlacement::Auto,
    }
}

/// `grid-auto-rows` / `grid-auto-columns`: one or more track sizes.
fn auto_tracks(s: &str) -> Option<Vec<taffy::TrackSizingFunction>> {
    let tracks = grid_template(s)?;
    tracks
        .tracks
        .into_iter()
        .map(|t| match t {
            GridTemplateComponent::Single(f) => Some(f),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .filter(|v| !v.is_empty())
}

/// The quoted strings in `"a a" "b c"` (other tokens are an error).
fn quoted_strings(s: &str) -> Option<Vec<String>> {
    let (areas, sizes) = areas_and_sizes(s)?;
    sizes.is_empty().then_some(areas)
}

/// `"a a" 40px "b c" 1fr`: the area strings, and the row sizes after them (`auto`
/// for rows without one; empty when no row has a size).
fn areas_and_sizes(s: &str) -> Option<(Vec<String>, Vec<String>)> {
    let mut areas = Vec::new();
    let mut sizes: Vec<Option<String>> = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('"').or_else(|| rest.strip_prefix('\'')) {
            let quote = rest.chars().next()?;
            let end = r.find(quote)?;
            areas.push(r[..end].to_owned());
            sizes.push(None);
            rest = r[end + 1..].trim_start();
        } else {
            let token = split_top_level(rest, ' ').first()?.to_string();
            *sizes.last_mut()? = Some(token.clone());
            rest = rest[rest.find(&token)? + token.len()..].trim_start();
        }
    }
    let any_size = sizes.iter().any(Option::is_some);
    let sizes = if any_size {
        sizes
            .into_iter()
            .map(|s| s.unwrap_or_else(|| "auto".into()))
            .collect()
    } else {
        Vec::new()
    };
    (!areas.is_empty()).then_some((areas, sizes))
}

/// Grid areas from `grid-template-areas` rows: each name must cover a rectangle,
/// `.` cells are empty, and rows must have the same number of cells.
fn template_areas(rows: &[String]) -> Option<taffy::GridTemplateAreas<String>> {
    let cells: Vec<Vec<&str>> = rows
        .iter()
        .map(|r| r.split_whitespace().collect())
        .collect();
    let columns = cells.first()?.len();
    if columns == 0 || cells.iter().any(|r| r.len() != columns) {
        return None;
    }
    let mut names: Vec<&str> = Vec::new();
    for row in &cells {
        for c in row {
            if !c.chars().all(|ch| ch == '.') && !names.contains(c) {
                names.push(c);
            }
        }
    }
    let mut areas = Vec::new();
    for name in names {
        let at: Vec<(usize, usize)> = cells
            .iter()
            .enumerate()
            .flat_map(|(r, row)| {
                row.iter()
                    .enumerate()
                    .filter(|(_, c)| **c == name)
                    .map(move |(c, _)| (r, c))
            })
            .collect();
        let (r0, r1) = (at.iter().map(|p| p.0).min()?, at.iter().map(|p| p.0).max()?);
        let (c0, c1) = (at.iter().map(|p| p.1).min()?, at.iter().map(|p| p.1).max()?);
        // Must be a filled rectangle.
        if at.len() != (r1 - r0 + 1) * (c1 - c0 + 1) {
            return None;
        }
        areas.push(taffy::GridTemplateArea {
            name: name.to_owned(),
            row_start: r0 as u16 + 1,
            row_end: r1 as u16 + 2,
            column_start: c0 as u16 + 1,
            column_end: c1 as u16 + 2,
        });
    }
    Some(taffy::GridTemplateAreas {
        areas,
        row_count: cells.len() as u16,
        column_count: columns as u16,
    })
}

/// One or two keywords; one sets both.
fn two_keywords(s: &str) -> Option<[&str; 2]> {
    match s.split_whitespace().collect::<Vec<_>>()[..] {
        [a] => Some([a, a]),
        [a, b] => Some([a, b]),
        _ => None,
    }
}

/// A logical property's physical counterparts (horizontal, left-to-right writing).
#[derive(Clone, Copy)]
enum Logical {
    One(&'static str),
    Two(&'static str, &'static str),
}

fn logical_sides(prop: &str) -> Option<Logical> {
    use Logical::*;
    Some(match prop {
        "marginBlock" => Two("marginTop", "marginBottom"),
        "marginInline" => Two("marginLeft", "marginRight"),
        "marginBlockStart" => One("marginTop"),
        "marginBlockEnd" => One("marginBottom"),
        "marginInlineStart" => One("marginLeft"),
        "marginInlineEnd" => One("marginRight"),
        "paddingBlock" => Two("paddingTop", "paddingBottom"),
        "paddingInline" => Two("paddingLeft", "paddingRight"),
        "paddingBlockStart" => One("paddingTop"),
        "paddingBlockEnd" => One("paddingBottom"),
        "paddingInlineStart" => One("paddingLeft"),
        "paddingInlineEnd" => One("paddingRight"),
        "insetBlock" => Two("top", "bottom"),
        "insetInline" => Two("left", "right"),
        "insetBlockStart" => One("top"),
        "insetBlockEnd" => One("bottom"),
        "insetInlineStart" => One("left"),
        "insetInlineEnd" => One("right"),
        "blockSize" => One("height"),
        "inlineSize" => One("width"),
        "minBlockSize" => One("minHeight"),
        "minInlineSize" => One("minWidth"),
        "maxBlockSize" => One("maxHeight"),
        "maxInlineSize" => One("maxWidth"),
        _ => return None,
    })
}

/// `gridColumn: 2`, `"2"`, `"1 / 3"`, `"span 2"`, `"2 / span 2"`.
fn grid_line(v: &Value) -> Option<Line<GridPlacement<String>>> {
    let placement = |s: &str| s.trim().parse::<GridPlacement<String>>().ok();
    match v {
        Value::Number(n) => Some(Line {
            start: placement(&n.to_string())?,
            end: GridPlacement::Auto,
        }),
        Value::String(s) => match s.split_once('/') {
            Some((start, end)) => Some(Line {
                start: placement(start)?,
                end: placement(end)?,
            }),
            None => Some(Line {
                start: placement(s)?,
                end: GridPlacement::Auto,
            }),
        },
        _ => None,
    }
}

// M2-STY-01
#[cfg(test)]
mod tests {
    use super::*;

    fn apply(prop: &str, value: Value) -> Result<(Style, Visual), StyleError> {
        let mut style = Style::default();
        let mut visual = Visual::default();
        apply_style(&mut style, &mut visual, prop, &value)?;
        Ok((style, visual))
    }

    // M3-BRD-01
    #[test]
    fn border_styles_and_side_colours_parse() {
        let mut style = Style::default();
        let mut v = Visual::default();
        let set = |style: &mut Style, v: &mut Visual, prop: &str, value: &str| {
            apply_style(style, v, prop, &value.into()).map(|_| ())
        };
        set(&mut style, &mut v, "border", "2px dashed red").unwrap();
        set(&mut style, &mut v, "borderColor", "red currentColor").unwrap();
        set(&mut style, &mut v, "borderLeftStyle", "double").unwrap();
        set(&mut style, &mut v, "borderBottom", "1px dotted blue").unwrap();
        set(&mut style, &mut v, "borderRightColor", "lime").unwrap();
        assert!(set(&mut style, &mut v, "borderStyle", "wavy").is_err());
        assert!(set(&mut style, &mut v, "borderTopFoo", "1px").is_err());
        assert_eq!(
            v.border_styles,
            [
                BorderStyle::Dashed,
                BorderStyle::Dashed,
                BorderStyle::Dotted,
                BorderStyle::Double
            ]
        );
        assert_eq!(v.border_currents, [false, false, false, true]);
        assert_eq!(v.border_colors[1], Rgba::parse("lime").unwrap());
        assert_eq!(v.border_colors[2], Rgba::parse("blue").unwrap());
        assert_eq!(style.border.bottom, LengthPercentage::length(1.0));

        // `none` and `hidden` sides take no space.
        set(&mut style, &mut v, "borderStyle", "solid none").unwrap();
        assert_eq!(style.border.top, LengthPercentage::length(2.0));
        assert_eq!(style.border.right, LengthPercentage::length(0.0));
        assert_eq!(style.border.left, LengthPercentage::length(0.0));
        // Platypus draws a width alone (default style solid).
        let (_, v) = apply("borderWidth", 2.0.into()).unwrap();
        assert_eq!(v.border_styles, [BorderStyle::Solid; 4]);
    }

    // M3-BRD-03
    #[test]
    fn outline_properties() {
        let (_, v) = apply("outline", "2px dashed red".into()).unwrap();
        assert_eq!(
            (v.outline_width, v.outline_style, v.outline_current),
            (2.0, BorderStyle::Dashed, false)
        );
        let (_, v) = apply("outline", "auto".into()).unwrap();
        assert_eq!(
            (v.outline_width, v.outline_style, v.outline_current),
            (3.0, BorderStyle::Solid, true),
            "medium, currentColor"
        );
        let (_, v) = apply("outlineOffset", "-2px".into()).unwrap();
        assert_eq!(v.outline_offset, -2.0);
        let (_, v) = apply("outlineWidth", "thick".into()).unwrap();
        assert_eq!(v.outline_width, 5.0);
        assert_eq!(
            Visual::default().outline_style,
            BorderStyle::None,
            "hidden by default"
        );
        for (prop, bad) in [
            ("outlineStyle", "hidden"),
            ("outline", "2px solid red blue"),
            ("outlineWidth", "-1px"),
            ("outlineOffset", "10%"),
        ] {
            assert!(apply(prop, bad.into()).is_err(), "{prop}: {bad}");
        }
    }

    // M3-BG-02
    #[test]
    fn background_layer_lists() {
        let mut style = Style::default();
        let mut v = Visual::default();
        let mut set = |prop: &str, value: &str| {
            apply_style(&mut style, &mut v, prop, &value.into()).map(|_| ())
        };
        set(
            "backgroundImage",
            "url(a.png), none, linear-gradient(red, blue)",
        )
        .unwrap();
        set("backgroundClip", "padding-box, text").unwrap();
        set("backgroundRepeat", "repeat-x, round space").unwrap();
        set("backgroundPosition", "left top, right bottom").unwrap();
        set("backgroundPositionY", "25%").unwrap();
        assert!(set("backgroundClip", "margin-box").is_err());
        assert!(
            set("backgroundImage", "url(a.png),").is_ok(),
            "empty parts are dropped"
        );
        assert!(set("backgroundPositionX", "top").is_err());
        assert_eq!(v.background_images.len(), 1);
        assert_eq!(
            v.background_clips,
            vec![BackgroundClip::PaddingBox, BackgroundClip::Text]
        );
        assert_eq!(v.background_repeats.len(), 2);
        assert_eq!(
            v.background_positions
                .iter()
                .map(|p| (p.x, p.y))
                .collect::<Vec<_>>(),
            vec![
                (Len::Percent(0.0), Len::Percent(0.25)),
                (Len::Percent(1.0), Len::Percent(0.25))
            ],
            "position-y replaces y in every layer"
        );
        let (_, v) = apply("backgroundImage", "none".into()).unwrap();
        assert!(v.background_images.is_empty());
    }

    #[test]
    fn numbers_are_pixels_and_strings_are_css() {
        assert_eq!(
            apply("width", 10.0.into()).unwrap().0.size.width,
            Dimension::length(10.0)
        );
        assert_eq!(
            apply("width", "50%".into()).unwrap().0.size.width,
            Dimension::percent(0.5)
        );
        assert_eq!(
            apply("width", "auto".into()).unwrap().0.size.width,
            Dimension::auto()
        );
        assert_eq!(
            apply("height", "12px".into()).unwrap().0.size.height,
            Dimension::length(12.0)
        );
    }

    #[test]
    fn keywords_parse_with_taffy() {
        let (s, _) = apply("justifyContent", "space-between".into()).unwrap();
        assert_eq!(s.justify_content, AlignContent::SPACE_BETWEEN);
        let (s, _) = apply("display", "grid".into()).unwrap();
        assert_eq!(s.display, Display::Grid);
        let (s, _) = apply("alignSelf", "auto".into()).unwrap();
        assert_eq!(s.align_self, None);
        let (s, _) = apply("flexDirection", "column".into()).unwrap();
        assert_eq!(s.flex_direction, FlexDirection::Column);
    }

    #[test]
    fn flex_flow_sets_direction_and_wrap_in_either_order() {
        let (s, _) = apply("flexFlow", "row wrap".into()).unwrap();
        assert_eq!(
            (s.flex_direction, s.flex_wrap),
            (FlexDirection::Row, FlexWrap::Wrap)
        );
        let (s, _) = apply("flexFlow", "wrap-reverse column".into()).unwrap();
        assert_eq!(
            (s.flex_direction, s.flex_wrap),
            (FlexDirection::Column, FlexWrap::WrapReverse)
        );
        let (s, _) = apply("flexFlow", "column".into()).unwrap();
        assert_eq!(
            (s.flex_direction, s.flex_wrap),
            (FlexDirection::Column, FlexWrap::NoWrap)
        );
        assert!(apply("flexFlow", "row column".into()).is_err());
        assert!(apply("flexFlow", "wrap nowrap".into()).is_err());
    }

    #[test]
    fn overflow_sets_both_axes_and_taffy() {
        let (s, v) = apply("overflow", "hidden auto".into()).unwrap();
        assert_eq!(v.overflow, [Overflow::Hidden, Overflow::Auto]);
        assert_eq!(s.overflow.x, taffy::Overflow::Hidden);
        assert_eq!(s.overflow.y, taffy::Overflow::Scroll);
        let (s, v) = apply("overflowX", "hidden".into()).unwrap();
        assert_eq!(v.overflow, [Overflow::Hidden, Overflow::Visible]);
        assert_eq!(
            s.overflow.y,
            taffy::Overflow::Scroll,
            "visible next to hidden computes to auto"
        );
        assert!(apply("overflow", "sideways".into()).is_err());
    }

    #[test]
    fn grid_templates_and_lines() {
        let (s, _) = apply("gridTemplateColumns", "1fr 2fr".into()).unwrap();
        assert_eq!(s.grid_template_columns.len(), 2);
        let (s, _) = apply("gridColumn", "1 / 3".into()).unwrap();
        assert_eq!(s.grid_column.start, "1".parse().unwrap());
        assert_eq!(s.grid_column.end, "3".parse().unwrap());
        let (s, _) = apply("gridRow", "span 2".into()).unwrap();
        assert_eq!(s.grid_row.start, GridPlacement::Span(2));
    }

    #[test]
    fn shorthands_follow_css_order() {
        let (s, _) = apply("padding", "1px 2px 3px 4px".into()).unwrap();
        assert_eq!(
            (
                s.padding.top,
                s.padding.right,
                s.padding.bottom,
                s.padding.left
            ),
            (
                LengthPercentage::length(1.0),
                LengthPercentage::length(2.0),
                LengthPercentage::length(3.0),
                LengthPercentage::length(4.0)
            )
        );
        // Unitless numbers inside strings are pixels (CSS allows unitless 0).
        let (s, _) = apply("padding", "0 20px".into()).unwrap();
        assert_eq!(s.padding.top, LengthPercentage::length(0.0));
        assert_eq!(s.padding.left, LengthPercentage::length(20.0));
        let (s, _) = apply("margin", "0 auto".into()).unwrap();
        assert_eq!(s.margin.left, LengthPercentageAuto::auto());
        let (s, _) = apply("width", "0".into()).unwrap();
        assert_eq!(s.size.width, Dimension::length(0.0));
        let (s, _) = apply("margin", 5.0.into()).unwrap();
        assert_eq!(s.margin.left, LengthPercentageAuto::length(5.0));
        let (s, _) = apply("gap", "10px 20px".into()).unwrap();
        assert_eq!(s.gap.height, LengthPercentage::length(10.0));
        assert_eq!(s.gap.width, LengthPercentage::length(20.0));
        let (s, _) = apply("flex", 2.0.into()).unwrap();
        assert_eq!(
            (s.flex_grow, s.flex_shrink, s.flex_basis),
            (2.0, 1.0, Dimension::length(0.0))
        );
    }

    #[test]
    fn visual_properties() {
        // M3-FX-04
        let (_, v) = apply(
            "maskImage",
            "linear-gradient(black, transparent), none".into(),
        )
        .unwrap();
        assert_eq!(v.mask_images.len(), 2);
        let (_, v) = apply("maskMode", "luminance, match-source".into()).unwrap();
        assert_eq!(v.mask_modes, vec![MaskMode::Luminance, MaskMode::Alpha]);
        assert!(apply("maskMode", "brightness".into()).is_err());
        assert!(apply("WebkitMaskSize", "10px".into()).is_ok());
        // M3-FX-02
        assert!(apply("backdropFilter", "drop-shadow(1px 1px red)".into()).is_err());
        let (_, v) = apply("backdropFilter", "blur(4px) saturate(2)".into()).unwrap();
        assert!(v.backdrop_blur == 4.0 && v.backdrop_matrix.is_some());
        // M3-FX-01
        let (_, v) = apply("opacity", "40%".into()).unwrap();
        assert!((v.opacity - 0.4).abs() < 1e-6);
        let (_, v) = apply("opacity", "0.25".into()).unwrap();
        assert_eq!(v.opacity, 0.25);
        assert!(apply("opacity", "half".into()).is_err());
        let (_, v) = apply("backgroundColor", "#ff0000".into()).unwrap();
        assert_eq!(v.background, Rgba::new(1.0, 0.0, 0.0, 1.0));
        let (_, v) = apply("borderRadius", "8px".into()).unwrap();
        let circle = |l| Corners::all(CornerRadius::circular(l));
        assert_eq!(v.border_radius, circle(Len::Px(8.0)));
        let (_, v) = apply("borderRadius", 6.0.into()).unwrap();
        assert_eq!(v.border_radius, circle(Len::Px(6.0)));
        let (_, v) = apply("borderTopRightRadius", "50%".into()).unwrap();
        assert_eq!(
            v.border_radius.top_right,
            CornerRadius::circular(Len::Percent(0.5))
        );
        assert_eq!(v.border_radius.top_left, CornerRadius::ZERO);
        let (_, v) = apply("borderBottomLeftRadius", "10px 20px".into()).unwrap();
        assert_eq!(v.border_radius.bottom_left.y, Len::Px(20.0));
        let (_, v) = apply("boxShadow", "0 2px 4px black".into()).unwrap();
        assert_eq!(v.shadows.len(), 1);
        let (_, v) = apply("filter", "blur(3px)".into()).unwrap();
        assert_eq!(v.filter_blur, 3.0);
        let (_, v) = apply("backdropFilter", "blur(12px)".into()).unwrap();
        assert_eq!(v.backdrop_blur, 12.0);
        let (_, v) = apply("background", "linear-gradient(red, blue)".into()).unwrap();
        assert_eq!(v.background_images.len(), 1);
        let (_, v) = apply("background", "teal".into()).unwrap();
        assert!(v.background_images.is_empty());
        assert_eq!(v.background, Rgba::parse("teal").unwrap());
        let (_, v) = apply("opacity", 1.5.into()).unwrap();
        assert_eq!(v.opacity, 1.0, "opacity is clamped");
    }

    #[test]
    fn unknown_properties_and_bad_values_change_nothing() {
        let mut style = Style::default();
        let mut visual = Visual::default();
        apply_style(&mut style, &mut visual, "width", &10.0.into()).unwrap();
        apply_style(&mut style, &mut visual, "backgroundColor", &"red".into()).unwrap();

        assert_eq!(
            apply_style(&mut style, &mut visual, "colour", &"red".into()),
            Err(StyleError::UnknownProperty("colour".into()))
        );
        assert!(apply_style(&mut style, &mut visual, "width", &"ten".into()).is_err());
        assert!(apply_style(&mut style, &mut visual, "backgroundColor", &"#zz".into()).is_err());
        assert!(apply_style(&mut style, &mut visual, "flexGrow", &"lots".into()).is_err());

        assert_eq!(style.size.width, Dimension::length(10.0));
        assert_eq!(visual.background, Rgba::parse("red").unwrap());
    }

    #[test]
    fn null_resets_to_default() {
        let mut style = Style::default();
        let mut visual = Visual::default();
        apply_style(&mut style, &mut visual, "width", &10.0.into()).unwrap();
        apply_style(&mut style, &mut visual, "opacity", &0.5.into()).unwrap();
        apply_style(&mut style, &mut visual, "width", &Value::Null).unwrap();
        apply_style(&mut style, &mut visual, "opacity", &Value::Null).unwrap();
        assert_eq!(style.size.width, Style::<String>::default().size.width);
        assert_eq!(visual.opacity, 1.0);
    }

    // M2-VIS-02
    #[test]
    fn border_shorthand_sets_widths_and_colour() {
        let (s, v) = apply("border", "2px solid red".into()).unwrap();
        assert_eq!(s.border.left, LengthPercentage::length(2.0));
        assert_eq!(s.border.top, LengthPercentage::length(2.0));
        assert_eq!(v.border_colors[0], Rgba::new(1.0, 0.0, 0.0, 1.0));

        // Any order; keyword widths; style without width is `medium` (3px).
        let (s, v) = apply("border", "#00f thick solid".into()).unwrap();
        assert_eq!(s.border.right, LengthPercentage::length(5.0));
        assert_eq!(v.border_colors[3], Rgba::new(0.0, 0.0, 1.0, 1.0));
        let (s, _) = apply("border", "solid".into()).unwrap();
        assert_eq!(s.border.bottom, LengthPercentage::length(3.0));

        // Like CSS: `none`, or no style at all, means no border.
        let (s, _) = apply("border", "none".into()).unwrap();
        assert_eq!(s.border.top, LengthPercentage::length(0.0));
        let (s, _) = apply("border", "2px red".into()).unwrap();
        assert_eq!(s.border.top, LengthPercentage::length(0.0));

        // Unsupported styles and junk are rejected (and reported as warnings).
        for bad in ["2px wavy red", "2px solid red blue", "wide solid"] {
            assert!(apply("border", bad.into()).is_err(), "{bad:?}");
        }
    }
}
