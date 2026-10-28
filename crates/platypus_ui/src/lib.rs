// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Platypus's UI core: the node tree built from React commits, CSS-like styles,
//! layout (Taffy behind the [`LayoutEngine`] trait) and a GPU-free draw list.
//!
//! Pipeline (DESIGN.md §5.1): `Commit` ops → [`UiTree`] → layout → [`DrawCmd`]s → renderer.

mod attrs;
pub mod calc;
mod cascade;
mod color;
mod color_fn;
pub mod css;
mod draw;
pub mod editor;
pub mod events;
pub mod interaction;
mod layout;
pub mod media;
pub mod motion;
pub mod opcodes;
mod paint;
mod style;
pub mod svg;
mod svg_path;
pub mod tables;
pub mod text;
pub mod text_effect;
pub mod text_style;
pub mod transform;
mod tree;
pub mod units;
pub mod validity;

mod background;
pub use attrs::{Attrs, ClosedBy, ElementState};
pub use background::{border_image_axis, is_svg, repeat_tiles, resolve_stops, tile_rect};
pub use color::Rgba;
pub use draw::{
    ClipShape, DrawCmd, GlyphRun, ImageFill, LayerShadow, Polygon, PositionedGlyph, Quad, Radii,
    Rect, ShadowShape, TextCmd, TextDecoration,
};
pub use events::{EventKind, Pointer, ScrollInfo, UiEvent};
pub use interaction::{Clipboard, Direction, Input, Interaction, ScrollAlign, ScrollBehavior};
pub use layout::{LayoutEngine, NodeLayout, TaffyLayout};
pub use paint::{
    BackgroundClip, BackgroundImage, BackgroundPosition, BackgroundRepeat, BackgroundSize,
    BlendMode, BorderImage, BorderImageLength, BorderImagePiece, BorderImageRepeat, BorderStyle,
    ClipPath, ClipShapeSpec, ColorMatrix, ColorStop, CornerRadius, Corners, DropShadow, Filter,
    GeometryBox, GradientDirection, Len, MaskMode, ObjectFit, Overflow, RadialShape, RepeatAxis,
    Shadow, ShapeRadius,
};
pub use parley::FontData;
pub use style::{
    CURSORS, CursorImage, ScrollbarWidth, StyleError, Visual, WideKeyword, apply_style,
    apply_wide_keyword, is_inherited,
};
pub use text_effect::{GlyphFx, REVEAL_ALL, TextEffect};
pub use tree::{
    AddedStylesheet, Inspected, InspectedDeclaration, InspectedInline, InspectedRule, ROOT,
    Scrollbar, TEXT_NODE, UiError, UiTree,
};

/// Re-exported because [`LayoutEngine`] uses Taffy's `Style` as its
/// backend-neutral description of CSS layout properties.
pub use taffy::Style as LayoutStyle;
