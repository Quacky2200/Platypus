// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The draw list: GPU-free paint commands produced by [`crate::UiTree::draw_list`]
//! and consumed by the renderer (DESIGN.md §5.2 "Draw list shape").

use std::sync::Arc;

use crate::Rgba;
use crate::paint::{
    BackgroundImage, BackgroundPosition, BackgroundRepeat, BackgroundSize, BlendMode, BorderImage,
    BorderStyle, ColorMatrix, MaskMode,
};
use crate::text_style::DecorationStyle;
use crate::transform::Homography;

/// An axis-aligned rectangle in logical pixels (window space).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn expand(self, by: f32) -> Rect {
        Rect::new(
            self.x - by,
            self.y - by,
            self.width + 2.0 * by,
            self.height + 2.0 * by,
        )
    }

    pub fn offset(self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Whether (x, y) is inside the rectangle with rounded (possibly elliptical)
    /// corners `radii`, as used for hit testing.
    pub fn contains_rounded(self, radii: Radii, x: f32, y: f32) -> bool {
        ClipShape::rounded(self, radii).contains(x, y)
    }

    /// The overlap of two rectangles (zero-sized when they don't overlap).
    pub fn intersect(self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
    }

    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    pub fn union(self, other: Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Rect::new(x, y, right - x, bottom - y)
    }
}

/// Corner radii, horizontal (`x`) and vertical (`y`), each top-left, top-right,
/// bottom-right, bottom-left. Equal for circular corners.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Radii {
    pub x: [f32; 4],
    pub y: [f32; 4],
}

impl Radii {
    pub const ZERO: Self = Self {
        x: [0.0; 4],
        y: [0.0; 4],
    };

    pub fn circular(r: [f32; 4]) -> Self {
        Self { x: r, y: r }
    }

    /// The radii of an inner edge `inset` (top, right, bottom, left) inside this
    /// shape, as CSS does: horizontal radii lose the left/right inset, vertical
    /// ones the top/bottom.
    pub fn inner(self, inset: [f32; 4]) -> Self {
        let [t, r, b, l] = inset;
        let x = [l, r, r, l];
        let y = [t, t, b, b];
        Self {
            x: [0, 1, 2, 3].map(|i| (self.x[i] - x[i]).max(0.0)),
            y: [0, 1, 2, 3].map(|i| (self.y[i] - y[i]).max(0.0)),
        }
    }

    /// Rounded corners grown by `by` (shadow spread); square corners stay square.
    pub fn grow(self, by: f32) -> Self {
        let g = |r: f32| if r > 0.0 { (r + by).max(0.0) } else { 0.0 };
        Self {
            x: self.x.map(g),
            y: self.y.map(g),
        }
    }
}

/// A clip region: a rectangle with elliptical corners (`radii_x` / `radii_y` per corner:
/// top-left, top-right, bottom-right, bottom-left), or a polygon inside `rect` (its
/// bounding box). Rectangles, rounded boxes, circles and ellipses are all the first kind.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipShape {
    pub rect: Rect,
    pub radii_x: [f32; 4],
    pub radii_y: [f32; 4],
    pub polygon: Option<Arc<Polygon>>,
}

/// `clip-path: polygon()` / `path()`: closed contours in window px.
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon {
    pub contours: Vec<Vec<(f32, f32)>>,
    /// `evenodd` fill; otherwise `nonzero`.
    pub even_odd: bool,
}

impl Polygon {
    /// Whether (x, y) is inside, by the fill rule.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let (mut winding, mut crossings) = (0, 0);
        for contour in &self.contours {
            for (i, &(ax, ay)) in contour.iter().enumerate() {
                let (bx, by) = contour[(i + 1) % contour.len()];
                let left = (bx - ax) * (y - ay) - (x - ax) * (by - ay);
                if ay <= y && by > y && left > 0.0 {
                    winding += 1;
                } else if ay > y && by <= y && left < 0.0 {
                    winding -= 1;
                }
                if (ay > y) != (by > y) && x < ax + (y - ay) * (bx - ax) / (by - ay) {
                    crossings += 1;
                }
            }
        }
        if self.even_odd {
            crossings % 2 == 1
        } else {
            winding != 0
        }
    }
}

impl ClipShape {
    pub fn rect(rect: Rect) -> Self {
        Self {
            rect,
            radii_x: [0.0; 4],
            radii_y: [0.0; 4],
            polygon: None,
        }
    }

    /// A box with rounded corners.
    pub fn rounded(rect: Rect, radii: Radii) -> Self {
        Self {
            rect,
            radii_x: radii.x,
            radii_y: radii.y,
            polygon: None,
        }
    }

    /// A polygon; `rect` becomes its bounding box.
    pub fn polygon(polygon: Polygon) -> Self {
        let points = polygon.contours.iter().flatten();
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for &(x, y) in points {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
        let rect = if lo.0 <= hi.0 {
            Rect::new(lo.0, lo.1, hi.0 - lo.0, hi.1 - lo.1)
        } else {
            Rect::default()
        };
        Self {
            rect,
            radii_x: [0.0; 4],
            radii_y: [0.0; 4],
            polygon: Some(Arc::new(polygon)),
        }
    }

    /// Whether (x, y) is inside the shape (right and bottom edges excluded).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let r = self.rect;
        let (right, bottom) = (r.x + r.width, r.y + r.height);
        if x < r.x || y < r.y || x >= right || y >= bottom {
            return false;
        }
        if let Some(polygon) = &self.polygon {
            return polygon.contains(x, y);
        }
        let left = x < r.x + r.width / 2.0;
        let top = y < r.y + r.height / 2.0;
        let corner = match (top, left) {
            (true, true) => 0,
            (true, false) => 1,
            (false, false) => 2,
            (false, true) => 3,
        };
        let (rx, ry) = (self.radii_x[corner], self.radii_y[corner]);
        if rx <= 0.0 || ry <= 0.0 {
            return true;
        }
        let cx = if left { r.x + rx } else { right - rx };
        let cy = if top { r.y + ry } else { bottom - ry };
        let outside_x = if left { x < cx } else { x > cx };
        let outside_y = if top { y < cy } else { y > cy };
        if !outside_x || !outside_y {
            return true;
        }
        let (dx, dy) = ((x - cx) / rx, (y - cy) / ry);
        dx * dx + dy * dy <= 1.0
    }
}

/// A background image or gradient layer. Images are sized and positioned against
/// the quad's rect and painted only inside `area` (the `background-clip` box,
/// rounded by `radii`). Gradients fill `area`.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageFill {
    pub image: Arc<BackgroundImage>,
    pub size: BackgroundSize,
    pub position: BackgroundPosition,
    pub repeat: BackgroundRepeat,
    pub area: Rect,
    pub radii: Radii,
    /// `image-rendering: pixelated`: nearest-neighbour sampling.
    pub pixelated: bool,
    /// Multiplied into image pixels (`background-image-tint`).
    pub tint: Option<Rgba>,
}

/// One box: background colour, image/gradient layers, border and rounded corners.
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    pub rect: Rect,
    pub background: Rgba,
    /// Background layers in paint order (bottom first), over the colour, under the border.
    pub images: Vec<ImageFill>,
    pub border_color: Rgba,
    /// Border widths: top, right, bottom, left.
    pub border: [f32; 4],
    /// Resolved corner radii: top-left, top-right, bottom-right, bottom-left.
    pub radii: Radii,
}

impl Quad {
    /// Whether painting this quad would change any pixels.
    pub fn is_visible(&self) -> bool {
        let has_border = self.border_color.a > 0.0 && self.border.iter().any(|w| *w > 0.0);
        self.rect.width > 0.0
            && self.rect.height > 0.0
            && (self.background.a > 0.0 || !self.images.is_empty() || has_border)
    }
}

/// A shadow cast by a box, already resolved to pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowShape {
    /// The box casting the shadow (before offset and spread).
    pub rect: Rect,
    pub radii: Radii,
    pub offset_x: f32,
    pub offset_y: f32,
    /// CSS blur radius (the Gaussian's standard deviation is half of this).
    pub blur: f32,
    pub spread: f32,
    pub color: Rgba,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd {
    /// Blur what has already been drawn behind `rect`, clipped to its rounded shape.
    BackdropBlur {
        rect: Rect,
        radii: Radii,
        /// Gaussian deviation (px); may be 0 with only a colour change.
        blur: f32,
        /// `backdrop-filter` colour functions.
        matrix: Option<ColorMatrix>,
    },
    /// An outer `boxShadow`, drawn before its box.
    BoxShadow(ShadowShape),
    Quad(Quad),
    /// An `inset` shadow, drawn over the background inside the border.
    InsetShadow {
        shadow: ShadowShape,
        border: [f32; 4],
    },
    /// Following commands draw into an offscreen layer covering `bounds`, which is
    /// blurred by `blur` and composited with `opacity` and `blend` at the matching
    /// `PopLayer`.
    PushLayer {
        bounds: Rect,
        blur: f32,
        opacity: f32,
        blend: BlendMode,
        /// `filter` colour functions, applied when compositing.
        matrix: Option<ColorMatrix>,
        /// `drop-shadow()`s of the layer's content, drawn beneath it.
        shadows: Vec<LayerShadow>,
    },
    PopLayer,
    /// What follows (up to the matching `PopTransform`) is drawn in a transformed
    /// plane: the homography maps its coordinates to the window (it replaces, not
    /// composes with, the transform outside).
    PushTransform(Homography),
    PopTransform,
    /// Inside a layer, after its content: what follows (up to `PopLayer`) draws the
    /// layer's mask, which the layer is multiplied by when composited.
    BeginMask(MaskMode),
    /// A border that isn't one solid colour: per-side colours and styles (top, right,
    /// bottom, left) drawn in the ring between `rect` and its inner edge.
    Border {
        rect: Rect,
        radii: Radii,
        widths: [f32; 4],
        colors: [Rgba; 4],
        styles: [BorderStyle; 4],
    },
    /// A `border-image` over `area` (the border box grown by the outset); `border`
    /// is the element's border widths (top, right, bottom, left).
    BorderImage {
        area: Rect,
        border: [f32; 4],
        image: Arc<BorderImage>,
        pixelated: bool,
        /// Multiplied into image pixels (`border-image-tint`).
        tint: Option<Rgba>,
    },
    /// Following commands are clipped to `shape` (intersected with any clip already
    /// active) until the matching `PopClip`.
    PushClip(ClipShape),
    PopClip,
    /// Shaped, positioned text and its decorations.
    Text(TextCmd),
}

/// One run of glyphs in a single font, size and colour.
#[derive(Clone, Debug)]
pub struct GlyphRun {
    pub font: parley::FontData,
    /// Font size in logical pixels.
    pub font_size: f32,
    /// Variable-font coordinates (e.g. weight), normalized as in the font.
    pub normalized_coords: Vec<i16>,
    /// Synthetic bold when the family has no bold face.
    pub embolden: bool,
    /// Synthetic italic angle in degrees when the family has no italic face.
    pub skew: Option<f32>,
    pub color: Rgba,
    /// `text-stroke`: outline width (px) and colour, drawn over the fill.
    pub stroke: Option<(f32, Rgba)>,
    /// Anti-aliased edges; `false` for crisp, thresholded glyphs (`font-smooth: never`).
    pub smooth: bool,
    pub glyphs: Vec<PositionedGlyph>,
}

impl PartialEq for GlyphRun {
    fn eq(&self, other: &Self) -> bool {
        self.font.data.id() == other.font.data.id()
            && self.font.index == other.font.index
            && self.font_size == other.font_size
            && self.normalized_coords == other.normalized_coords
            && self.embolden == other.embolden
            && self.skew == other.skew
            && self.color == other.color
            && self.stroke == other.stroke
            && self.smooth == other.smooth
            && self.glyphs == other.glyphs
    }
}

/// A glyph at its baseline origin, in logical pixels (window space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedGlyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    /// Horizontal advance (px), for cutting lines short (`text-overflow`).
    pub advance: f32,
}

/// A resolved `drop-shadow()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    /// Gaussian standard deviation (px).
    pub sigma: f32,
    pub color: Rgba,
}

/// An underline, overline or line-through: `rect` is one line's area (a `double`
/// line draws a second one below it, a `wavy` one waves around it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextDecoration {
    pub rect: Rect,
    pub color: Rgba,
    pub style: DecorationStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextCmd {
    /// Area covered by the glyphs (approximate: line boxes).
    pub bounds: Rect,
    pub runs: Vec<GlyphRun>,
    /// Underlines and line-throughs as filled rectangles.
    pub decorations: Vec<TextDecoration>,
    /// `background-clip: text`: a gradient painted inside the glyphs, laid out over
    /// the element's box (the rect), under the text colour.
    pub fill: Option<(Arc<BackgroundImage>, Rect)>,
    /// `text-effect` and the glyphs shown (`text-reveal`).
    pub effect: crate::text_effect::TextEffect,
    pub reveal: f32,
    /// The UI node (the renderer remembers when each glyph of it was first shown).
    pub node: u32,
}

impl TextCmd {
    /// A copy moved by (dx, dy) with every colour replaced by `color`
    /// (used for text shadows).
    pub fn shadow(&self, dx: f32, dy: f32, color: Rgba) -> TextCmd {
        TextCmd {
            bounds: self.bounds.offset(dx, dy),
            runs: self
                .runs
                .iter()
                .map(|r| GlyphRun {
                    color,
                    stroke: r.stroke.map(|(w, _)| (w, color)),
                    glyphs: r
                        .glyphs
                        .iter()
                        .map(|g| PositionedGlyph {
                            x: g.x + dx,
                            y: g.y + dy,
                            ..*g
                        })
                        .collect(),
                    ..r.clone()
                })
                .collect(),
            decorations: self
                .decorations
                .iter()
                .map(|d| TextDecoration {
                    rect: d.rect.offset(dx, dy),
                    color,
                    style: d.style,
                })
                .collect(),
            fill: None,
            effect: self.effect,
            reveal: self.reveal,
            node: self.node,
        }
    }
}

impl DrawCmd {
    /// The area this command can change, if it paints anything itself.
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            DrawCmd::BackdropBlur { rect, .. } => Some(*rect),
            DrawCmd::BoxShadow(s) => Some(
                s.rect
                    .offset(s.offset_x, s.offset_y)
                    .expand(s.spread.max(0.0) + s.blur),
            ),
            DrawCmd::Quad(q) => Some(q.rect),
            DrawCmd::Border { rect, .. } => Some(*rect),
            DrawCmd::BorderImage { area, .. } => Some(*area),
            DrawCmd::InsetShadow { shadow, .. } => Some(shadow.rect),
            DrawCmd::PushLayer { bounds, .. } => Some(*bounds),
            DrawCmd::Text(t) => Some(t.bounds),
            DrawCmd::PopLayer
            | DrawCmd::BeginMask(_)
            | DrawCmd::PushTransform(_)
            | DrawCmd::PopTransform
            | DrawCmd::PushClip(_)
            | DrawCmd::PopClip => None,
        }
    }

    /// Scales the alpha of every colour this command paints.
    pub(crate) fn scale_alpha(&mut self, factor: f32) {
        match self {
            DrawCmd::BoxShadow(s) | DrawCmd::InsetShadow { shadow: s, .. } => {
                s.color = s.color.with_alpha_scaled(factor)
            }
            DrawCmd::Quad(q) => {
                q.background = q.background.with_alpha_scaled(factor);
                q.border_color = q.border_color.with_alpha_scaled(factor);
            }
            DrawCmd::Border { colors, .. } => {
                for c in colors {
                    *c = c.with_alpha_scaled(factor);
                }
            }
            DrawCmd::Text(t) => {
                for run in &mut t.runs {
                    run.color = run.color.with_alpha_scaled(factor);
                    if let Some((_, c)) = &mut run.stroke {
                        *c = c.with_alpha_scaled(factor);
                    }
                }
                for d in &mut t.decorations {
                    d.color = d.color.with_alpha_scaled(factor);
                }
            }
            DrawCmd::BackdropBlur { .. }
            | DrawCmd::BorderImage { .. }
            | DrawCmd::PushLayer { .. }
            | DrawCmd::PopLayer
            | DrawCmd::BeginMask(_)
            | DrawCmd::PushTransform(_)
            | DrawCmd::PopTransform
            | DrawCmd::PushClip(_)
            | DrawCmd::PopClip => {}
        }
    }
}
