// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The backend-neutral half of the UI renderer: a draw list ([`platypus_ui::DrawCmd`]s) turned into a
//! [`FramePlan`], which any renderer can execute (wgpu in `mod.rs`; a GLES one is to follow).
//!
//! Plain boxes, shadows, glyphs and images become instances (one 144-byte record each, drawn
//! by `quad.wgsl`); clips and transforms go into a table every instance indexes; gradients and
//! border styles into a gradient table; layers, blurs, backdrop filters and masks into
//! [`Step`]s over offscreen textures of the frame's size (index 0 is the canvas). Clips
//! (`overflow`, `clipPath`) don't break batches: every instance carries the index of the clip
//! active when it was drawn, and a layer is clipped when it is composited.
//!
//! The plan holds no GPU object. What it needs from a renderer (glyph placement in an atlas,
//! images' sizes, SVGs rasterized at a size) comes through [`PlanResources`].

use std::ops::Range;

use platypus_ui::text_style::DecorationStyle;
use platypus_ui::{
    ClipShape, DrawCmd, Rect, Rgba, border_image_axis, is_svg, repeat_tiles, tile_rect,
};

use super::glyph_clocks::{GlyphClocks, hue_color};
use super::glyphs::{GlyphEntry, SUBPIXEL_BINS};
use super::gradient;
pub use super::gradient::GpuGradient;
use crate::Color;
pub use platypus_core::project::Blending;

pub const KIND_QUAD: f32 = 0.0;
pub const KIND_OUTER_SHADOW: f32 = 1.0;
pub const KIND_INSET_SHADOW: f32 = 2.0;
pub const KIND_IMAGE: f32 = 3.0;
pub const KIND_GLYPH: f32 = 4.0;
pub const KIND_BORDER: f32 = 5.0;
pub const KIND_GRADIENT_PIECE: f32 = 6.0;
pub const KIND_WAVY: f32 = 7.0;

/// Matches `struct Instance` in `quad.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub rect: [f32; 4],
    /// Horizontal corner radii; `radii_y` (last, so earlier locations stay put) the
    /// vertical ones.
    pub radii: [f32; 4],
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    pub color0: [f32; 4],
    pub color1: [f32; 4],
    pub params: [f32; 4],
    pub radii_y: [f32; 4],
    /// x: transform index in the clip table (1-based, 0 = none): `rect` and the
    /// rest are in that transform's plane.
    pub xf: [f32; 4],
}

/// Circular corners approximating elliptical ones (the mean radius per corner),
/// for shapes the shader only draws circular (blurred shadows).
fn circular(r: platypus_ui::Radii) -> [f32; 4] {
    [0, 1, 2, 3].map(|i| (r.x[i] + r.y[i]) / 2.0)
}

/// Matches `struct Clip` in the shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuClip {
    pub rect: [f32; 4],
    pub rx: [f32; 4],
    pub ry: [f32; 4],
    pub bounds: [f32; 4],
    /// x: parent index (1-based, 0 = none); y: 0 for a rounded box, 1 for a nonzero
    /// polygon, 2 for an evenodd one; z: the polygon's first point (an index into the
    /// table read as an array of vec4s); w: its point count.
    pub info: [f32; 4],
}

/// The clip table for one frame. Index 0 means "no clip"; entry `i` is `table[i - 1]`.
#[derive(Default)]
struct Clips {
    table: Vec<GpuClip>,
    /// Bounds of each entry's whole chain, for culling and scissors.
    bounds: Vec<Rect>,
}

impl Clips {
    /// Adds `shape` inside `parent`; returns the new index.
    /// `xf`: the transform the shape is in (its clip-table index and homography).
    fn push(
        &mut self,
        shape: &ClipShape,
        parent: u32,
        xf: Option<&(u32, platypus_ui::transform::Homography)>,
    ) -> u32 {
        let mut bounds = xf.map_or(shape.rect, |(_, h)| h.map_rect(shape.rect));
        if parent > 0 {
            bounds = bounds.intersect(self.bounds[parent as usize - 1]);
        }
        let index = self.table.len();
        self.table.push(GpuClip {
            rect: rect4(shape.rect),
            rx: shape.radii_x,
            ry: shape.radii_y,
            bounds: rect4(bounds),
            info: [parent as f32, 0.0, 0.0, 0.0],
        });
        self.bounds.push(bounds);
        // The transform index rides above the shape kind: kind + 4 × index.
        let xf_index = xf.map_or(0, |(i, _)| *i) as f32;
        self.table[index].info[1] = 4.0 * xf_index;
        if let Some(polygon) = &shape.polygon {
            // The points follow the entry in the table, one vec4 each: x, y, and the
            // point range of its contour (start, end), so the shader can close it.
            let mut words = Vec::new();
            for contour in &polygon.contours {
                let (start, end) = (words.len(), words.len() + contour.len());
                for &(x, y) in contour {
                    words.push([x, y, start as f32, end as f32]);
                }
            }
            let first = (index + 1) * 5;
            self.table[index].info = [
                parent as f32,
                4.0 * xf_index + if polygon.even_odd { 2.0 } else { 1.0 },
                first as f32,
                words.len() as f32,
            ];
            for chunk in words.chunks(5) {
                let mut slot = [[0.0; 4]; 5];
                slot[..chunk.len()].copy_from_slice(chunk);
                self.table.push(bytemuck::cast(slot));
                // Not a clip: keeps `bounds` indexed like the table.
                self.bounds.push(bounds);
            }
        }
        index as u32 + 1
    }

    /// Stores a transform (homography and inverse) as a table entry; returns its
    /// 1-based index. It isn't a clip: nothing chains to it.
    fn push_transform(&mut self, h: &platypus_ui::transform::Homography) -> u32 {
        let m = h.0;
        let i = h
            .inverse()
            .unwrap_or(platypus_ui::transform::Homography::IDENTITY)
            .0;
        self.table.push(GpuClip {
            rect: [m[0][0], m[0][1], m[0][2], m[1][0]],
            rx: [m[1][1], m[1][2], m[2][0], m[2][1]],
            ry: [m[2][2], i[0][0], i[0][1], i[0][2]],
            bounds: [i[1][0], i[1][1], i[1][2], i[2][0]],
            info: [i[2][1], i[2][2], 0.0, 0.0],
        });
        self.bounds.push(Rect::new(-1e6, -1e6, 2e6, 2e6));
        self.table.len() as u32
    }

    fn bounds(&self, index: u32) -> Option<Rect> {
        (index > 0).then(|| self.bounds[index as usize - 1])
    }
}

/// A layer that's open in the draw list, until its `PopLayer`.
struct OpenLayer {
    /// The layer's own texture (drawing may have moved on to its mask).
    texture: usize,
    /// The mask texture and mode, once `BeginMask` started it.
    mask: Option<(usize, platypus_ui::MaskMode)>,
    parent: usize,
    bounds: Rect,
    blur: f32,
    opacity: f32,
    blend: u32,
    matrix: Option<platypus_ui::ColorMatrix>,
    shadows: Vec<platypus_ui::LayerShadow>,
    outer_clips: Vec<u32>,
}

/// A scissor rectangle in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PxRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// For a `Step::Blur` of `area` with `radius` taps either side on a canvas of `size`: the
/// rows the horizontal pass must write (`area` grown by the radius vertically, so the
/// vertical pass has valid rows to read, unless `contained`), and the pixels reads clamp
/// to (first x, first y, last x, last y; y down).
pub fn blur_reach(
    area: &PxRect,
    radius: f32,
    contained: bool,
    size: (u32, u32),
) -> (PxRect, [f32; 4]) {
    if contained {
        let last = [area.x + area.w - 1, area.y + area.h - 1];
        return (
            *area,
            [area.x as f32, area.y as f32, last[0] as f32, last[1] as f32],
        );
    }
    let r = radius as u32;
    let top = area.y.saturating_sub(r);
    let rows = PxRect {
        x: area.x,
        y: top,
        w: area.w,
        h: (area.y + area.h + r).min(size.1) - top,
    };
    let last = [
        size.0.saturating_sub(1) as f32,
        size.1.saturating_sub(1) as f32,
    ];
    (rows, [0.0, 0.0, last[0], last[1]])
}

/// Whether UI drawn over the world needs to read it: backdrop filters, and layers with
/// a blend mode (both read what's behind them).
pub fn needs_backdrop(cmds: &[DrawCmd]) -> bool {
    cmds.iter().any(|c| match c {
        DrawCmd::BackdropBlur { .. } => true,
        DrawCmd::PushLayer { blend, .. } => *blend != platypus_ui::BlendMode::Normal,
        _ => false,
    })
}

/// One step of a frame, recorded after all instances are known.
pub enum Step {
    Clear {
        target: usize,
        color: Color,
    },
    /// Instanced quads; with `image`, one background image draw using that texture
    /// (and nearest-neighbour sampling when the flag is set).
    Draw {
        target: usize,
        range: Range<u32>,
        image: Option<(String, bool)>,
    },
    Copy {
        src: usize,
        dst: usize,
    },
    /// Blurs `area` of `tex` (through `temp`). With `contained`, reads stay inside `area`,
    /// repeating its edge pixels (`backdrop-filter`: only the element's own box is
    /// blurred, so neighbouring glass doesn't bleed in); otherwise they reach outside it.
    Blur {
        tex: usize,
        temp: usize,
        area: PxRect,
        sigma: f32,
        contained: bool,
    },
    Composite {
        src: usize,
        dst: usize,
        area: PxRect,
        opacity: f32,
        clip: u32,
        /// A blend mode other than normal, with a copy of `dst` to read as the backdrop.
        blend: Option<(u32, usize)>,
        /// `filter` colour functions.
        matrix: Option<platypus_ui::ColorMatrix>,
        /// Draw the source's alpha in this colour (a drop shadow), shifted by `shift`
        /// physical pixels.
        tint: Option<[f32; 4]>,
        shift: [f32; 2],
        /// Multiply by this texture's alpha or luminance.
        mask: Option<(usize, platypus_ui::MaskMode)>,
    },
}

/// An image the plan draws: its natural size and the key of its texture (what `Step::Draw`
/// names it by).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    /// Natural size in pixels.
    pub size: (u32, u32),
    /// The renderer's texture key for the image.
    pub key: String,
}

/// What planning a frame needs from the renderer that will draw it.
pub trait PlanResources {
    /// Places one glyph of `run` in the glyph atlas (`None` when it has no image or the atlas
    /// is full); see [`super::glyphs::GlyphCache::glyph`].
    fn glyph(
        &mut self,
        run: &platypus_ui::GlyphRun,
        glyph: u32,
        size_px: f32,
        subpixel: u8,
        stroke_px: f32,
    ) -> Option<GlyphEntry>;

    /// When the glyphs of animated text were first shown (`text-effect`, `text-reveal`).
    fn glyph_clocks(&mut self) -> &mut GlyphClocks;

    /// The image at `url`, loading it if needed (`None` if it can't be loaded).
    fn image(&mut self, url: &str) -> Option<ImageInfo>;

    /// An SVG at `url` rasterized for `px` physical pixels: the key [`Step::Draw`] names it by,
    /// and its natural size (`None`: draw it by its url).
    fn svg_at(&mut self, url: &str, px: (f32, f32)) -> Option<(String, (u32, u32))>;
}

/// How one frame is drawn.
#[derive(Clone, Copy, Debug)]
pub struct PlanParams {
    /// The target's size in physical pixels.
    pub size: (u32, u32),
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// What the canvas is cleared to (unless `seeded`).
    pub clear: Color,
    /// The canvas starts as a copy of what is behind the UI (the 3D world) instead of a clear.
    pub seeded: bool,
    /// The space translucent colours blend in (docs/rendering.md "Blending").
    pub blending: Blending,
}

/// A frame, ready for a renderer to execute.
pub struct FramePlan {
    pub instances: Vec<Instance>,
    pub gradients: Vec<GpuGradient>,
    /// The clip and transform table (see [`GpuClip`]); entry `i` is clip index `i + 1`.
    pub clips: Vec<GpuClip>,
    pub steps: Vec<Step>,
    /// Offscreen textures of the frame's size the steps use; 0 is the canvas the frame ends in.
    pub textures: usize,
    /// Layers the draw list opened.
    pub layers: u32,
    /// The space the canvases hold and blend: with `Srgb` they are plain 8-bit textures of
    /// premultiplied sRGB values, with `Linear` sRGB-format ones of linear values. Every colour
    /// above is premultiplied in this space.
    pub blending: Blending,
}

/// Turns `cmds` into a [`FramePlan`] for a target of `params.size`.
pub fn plan(cmds: &[DrawCmd], params: &PlanParams, res: &mut dyn PlanResources) -> FramePlan {
    let (size, scale) = (params.size, params.scale);
    let (seeded, blending) = (params.seeded, params.blending);
    let clear = canvas_clear(params.clear, blending);
    let pm = |c: Rgba| canvas_premultiplied(c, blending);
    let mut layers = 0u32;
    // Textures of the frame's size the steps draw into; 0 is the canvas. A layer's texture (and its mask, shadow copies
    // and backdrop copy) is given back once the layer is composited, so a list with many layers side by side (each item
    // of a list at `opacity: 0.5`) needs as many as are open at once, not one each.
    let mut textures = TexturePool::default();
    let mut instances: Vec<Instance> = Vec::new();
    let mut gradients: Vec<GpuGradient> = Vec::new();
    // Over the world the canvas is seeded with it (below) rather than cleared.
    let mut steps = if seeded {
        Vec::new()
    } else {
        vec![Step::Clear {
            target: 0,
            color: clear,
        }]
    };
    let mut current = 0usize;
    let mut batch_start = 0u32;
    // (parent target, bounds, blur, opacity, the clip stack outside the layer)
    let mut layer_stack: Vec<OpenLayer> = Vec::new();
    // Clips: a layer's content starts unclipped and the layer is clipped when it
    // is composited, so each layer level has its own stack.
    let mut clips = Clips::default();
    let mut clip_stack: Vec<u32> = Vec::new();
    // Instances from `clip_from` on haven't been given their clip index yet.
    let mut clip_from = 0usize;
    // Instances since the last clip or transform change get the current ones. An
    // instance already placed in window space (`xf[0]` = -1: text re-rasterized at its
    // transformed size) gets no transform.
    let tag = |instances: &mut Vec<Instance>, clip_from: &mut usize, clip: u32, xf: u32| {
        for instance in &mut instances[*clip_from..] {
            instance.params[3] = clip as f32;
            instance.xf[0] = if instance.xf[0] < 0.0 { 0.0 } else { xf as f32 };
        }
        *clip_from = instances.len();
    };
    // Open `PushTransform`s: (clip-table index, homography to the window).
    let mut xf_stack: Vec<(u32, platypus_ui::transform::Homography)> = Vec::new();

    let flush =
        |steps: &mut Vec<Step>, instances: &Vec<Instance>, batch_start: &mut u32, target: usize| {
            let end = instances.len() as u32;
            if end > *batch_start {
                steps.push(Step::Draw {
                    target,
                    range: *batch_start..end,
                    image: None,
                });
                *batch_start = end;
            }
        };
    let to_px = |r: Rect| -> Option<PxRect> {
        let x0 = (r.x * scale).floor().clamp(0.0, size.0 as f32) as u32;
        let y0 = (r.y * scale).floor().clamp(0.0, size.1 as f32) as u32;
        let x1 = ((r.x + r.width) * scale).ceil().clamp(0.0, size.0 as f32) as u32;
        let y1 = ((r.y + r.height) * scale).ceil().clamp(0.0, size.1 as f32) as u32;
        (x1 > x0 && y1 > y0).then_some(PxRect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    };

    for cmd in cmds {
        let clip = clip_stack.last().copied().unwrap_or(0);
        let xf = xf_stack.last().map_or(0, |x| x.0);
        let xf_h = xf_stack.last().map(|x| x.1);
        // Window-space bounds of something drawn in the current plane.
        let world = |r: Rect| xf_h.map_or(r, |h| h.map_rect(r));
        // Skip anything the active clip removes entirely.
        if let (Some(bounds), Some(clip_bounds)) = (cmd.bounds().map(world), clips.bounds(clip))
            && !matches!(cmd, DrawCmd::PushLayer { .. })
            && bounds.intersect(clip_bounds).is_empty()
        {
            continue;
        }
        match cmd {
            DrawCmd::PushClip(shape) => {
                tag(&mut instances, &mut clip_from, clip, xf);
                clip_stack.push(clips.push(shape, clip, xf_stack.last()));
            }
            DrawCmd::PushTransform(h) => {
                tag(&mut instances, &mut clip_from, clip, xf);
                xf_stack.push((clips.push_transform(h), *h));
            }
            DrawCmd::PopTransform => {
                tag(&mut instances, &mut clip_from, clip, xf);
                if xf_stack.pop().is_none() {
                    tracing::warn!("PopTransform without PushTransform; ignored");
                }
            }
            DrawCmd::PopClip => {
                tag(&mut instances, &mut clip_from, clip, xf);
                if clip_stack.pop().is_none() {
                    tracing::warn!("PopClip without PushClip; ignored");
                }
            }
            DrawCmd::Quad(q) => {
                let quad = |rect: Rect,
                            radii: platypus_ui::Radii,
                            background: [f32; 4],
                            border_color: [f32; 4],
                            gradient: f32| Instance {
                    rect: rect4(rect),
                    radii: radii.x,
                    p0: q.border,
                    p1: [0.0; 4],
                    color0: background,
                    color1: border_color,
                    params: [KIND_QUAD, 0.0, gradient, 0.0],
                    radii_y: radii.y,
                    xf: [0.0; 4],
                };
                let background = pm(q.background);
                let border_color = pm(q.border_color);
                if q.images.is_empty() {
                    instances.push(quad(q.rect, q.radii, background, border_color, -1.0));
                    continue;
                }
                // Colour, then each layer bottom first (images are their own draws,
                // since each needs its texture), then the border on top.
                if q.background.a > 0.0 {
                    instances.push(quad(q.rect, q.radii, background, [0.0; 4], -1.0));
                }
                for fill in &q.images {
                    if !matches!(*fill.image, platypus_ui::BackgroundImage::Url(_)) {
                        // A gradient has no size of its own: `auto` is the box. One
                        // tile is laid out like an image's and repeats per the
                        // repeat axes (round rescales it; space gaps aren't kept).
                        let natural = (q.rect.width, q.rect.height);
                        let tile = tile_rect(q.rect, natural, fill.size, fill.position);
                        let (tile, _, repeat) = repeat_tiles(tile, q.rect, fill.repeat);
                        let Some(g) = gradient::build(&fill.image, tile) else {
                            continue;
                        };
                        gradients.push(g);
                        let index = (gradients.len() - 1) as f32;
                        let mut layer = quad(fill.area, fill.radii, [0.0; 4], [0.0; 4], index);
                        layer.p0 = [0.0; 4];
                        layer.p1 = rect4(tile);
                        layer.params[1] =
                            f32::from(u8::from(repeat[0])) + 2.0 * f32::from(u8::from(repeat[1]));
                        instances.push(layer);
                    } else if let platypus_ui::BackgroundImage::Url(url) = &*fill.image
                        && let Some(loaded) = res.image(url)
                    {
                        let natural = (loaded.size.0 as f32, loaded.size.1 as f32);
                        let tile = tile_rect(q.rect, natural, fill.size, fill.position);
                        let (tile, gap, repeat) = repeat_tiles(tile, q.rect, fill.repeat);
                        // SVGs are rasterized at the tile's size on screen.
                        let key = if is_svg(url) {
                            let px = (tile.width * scale, tile.height * scale);
                            res.svg_at(url, px)
                                .map_or_else(|| loaded.key.clone(), |(key, _)| key)
                        } else {
                            loaded.key.clone()
                        };
                        flush(&mut steps, &instances, &mut batch_start, current);
                        instances.push(Instance {
                            rect: rect4(fill.area),
                            radii: fill.radii.x,
                            radii_y: fill.radii.y,
                            xf: [0.0; 4],
                            p0: rect4(tile),
                            p1: [gap[0], gap[1], 0.0, 0.0],
                            color0: [0.0; 4],
                            // All zeros: untinted.
                            color1: fill.tint.map_or([0.0; 4], pm),
                            params: [
                                KIND_IMAGE,
                                f32::from(u8::from(repeat[0])),
                                f32::from(u8::from(repeat[1])),
                                0.0,
                            ],
                        });
                        let end = instances.len() as u32;
                        steps.push(Step::Draw {
                            target: current,
                            range: batch_start..end,
                            image: Some((key, fill.pixelated)),
                        });
                        batch_start = end;
                    }
                }
                if q.border_color.a > 0.0 && q.border.iter().any(|w| *w > 0.0) {
                    instances.push(quad(q.rect, q.radii, [0.0; 4], border_color, -1.0));
                }
            }
            DrawCmd::Text(text) => {
                for d in &text.decorations {
                    let line = |rect: Rect| Instance {
                        rect: rect4(rect),
                        radii: [0.0; 4],
                        radii_y: [0.0; 4],
                        xf: [0.0; 4],
                        p0: [0.0; 4],
                        p1: [0.0; 4],
                        color0: pm(d.color),
                        color1: [0.0; 4],
                        params: [KIND_QUAD, 0.0, -1.0, 0.0],
                    };
                    let t = d.rect.height;
                    match d.style {
                        DecorationStyle::Solid => instances.push(line(d.rect)),
                        DecorationStyle::Double => {
                            instances.push(line(d.rect));
                            instances.push(line(d.rect.offset(0.0, 2.0 * t)));
                        }
                        DecorationStyle::Dotted | DecorationStyle::Dashed => {
                            // A one-sided styled border: dots and dashes fitted to the line.
                            let style = if d.style == DecorationStyle::Dotted {
                                platypus_ui::BorderStyle::Dotted
                            } else {
                                platypus_ui::BorderStyle::Dashed
                            };
                            let mut entry = GpuGradient {
                                info: [style.index() as f32, 0.0, 0.0, 0.0],
                                ..Default::default()
                            };
                            entry.colors[0] = pm(d.color);
                            gradients.push(entry);
                            instances.push(Instance {
                                p0: [t, 0.0, 0.0, 0.0],
                                params: [KIND_BORDER, 0.0, (gradients.len() - 1) as f32, 0.0],
                                ..line(d.rect)
                            });
                        }
                        DecorationStyle::Wavy => {
                            // A band around the line that the wave swings within.
                            let band = Rect::new(d.rect.x, d.rect.y - t, d.rect.width, 3.0 * t);
                            instances.push(Instance {
                                params: [KIND_WAVY, t, -1.0, 0.0],
                                ..line(band)
                            });
                        }
                    }
                }
                // Text that is only scaled and moved (a pop-in, a lifted tile, a slide)
                // is drawn as a browser does: glyphs rasterized at their on-screen size
                // and placed in window space. Sampling the unscaled masks in the plane
                // (still the way for rotated, skewed or perspective text) blurs and
                // breaks it up while it moves.
                let (gs, gx, gy, window_space) = match xf_h.and_then(|h| h.as_scale_translate()) {
                    Some((s, tx, ty)) => (s, tx, ty, true),
                    None => (1.0, 0.0, 0.0, false),
                };
                let xf_mark = if window_space { -1.0 } else { 0.0 };
                // `background-clip: text`: glyphs take their colour from the gradient,
                // laid out over the element's box (in p1); the text colour goes on top.
                let fill = text.fill.as_ref().and_then(|(image, rect)| {
                    let rect = if window_space { world(*rect) } else { *rect };
                    let g = gradient::build(image, rect)?;
                    gradients.push(g);
                    Some(((gradients.len() - 1) as f32, rect4(rect)))
                });
                let mut glyph_index = 0usize;
                // Where the next glyph will appear: the `cursor` effect's block goes there.
                let mut cursor: Option<(f32, f32, f32, Rgba)> = None;
                // Glyphs of an animated element age from when this renderer first drew them
                // shown, so a pause in the typing doesn't hold their animation.
                let now = res.glyph_clocks().time();
                let animated = !text.effect.is_none() || !text.reveal.is_infinite();
                let mut shown_at = if animated {
                    res.glyph_clocks().take(text.node, now)
                } else {
                    Vec::new()
                };
                for run in &text.runs {
                    let run_color = pm(run.color);
                    for g in &run.glyphs {
                        // `text-effect` / `text-reveal`: this glyph's offset, scale, fade
                        // and hue (in ems of the run's font size).
                        let fx = if animated {
                            if shown_at.len() <= glyph_index {
                                shown_at.resize(glyph_index + 1, f32::NAN);
                            }
                            if text.reveal > glyph_index as f32 {
                                if shown_at[glyph_index].is_nan() {
                                    shown_at[glyph_index] = if text.reveal.is_infinite() {
                                        f32::NEG_INFINITY
                                    } else {
                                        now
                                    };
                                }
                                text.effect
                                    .eval(glyph_index, now - shown_at[glyph_index], now)
                            } else {
                                shown_at[glyph_index] = f32::NAN;
                                platypus_ui::GlyphFx {
                                    alpha: 0.0,
                                    ..Default::default()
                                }
                            }
                        } else {
                            platypus_ui::GlyphFx::default()
                        };
                        glyph_index += 1;
                        if animated
                            && text.effect.has_cursor()
                            && cursor.is_none()
                            && text.reveal <= (glyph_index - 1) as f32
                        {
                            cursor = Some((g.x, g.y, run.font_size, run.color));
                        }
                        if fx.alpha <= 0.0 || fx.scale < 0.05 {
                            continue;
                        }
                        let mut color = match fx.hue {
                            Some(h) => pm(hue_color(h, run.color.a)),
                            None => run_color,
                        };
                        if fx.alpha < 1.0 {
                            color = color.map(|c| c * fx.alpha);
                        }
                        let size_px = run.font_size * gs * scale * fx.scale;
                        // Snap to whole pixels vertically and to quarter pixels
                        // horizontally (the subpixel offset is baked into the glyph).
                        let px = (g.x * gs
                            + gx
                            + (fx.dx * run.font_size - (fx.scale - 1.0) * g.advance / 2.0) * gs)
                            * scale;
                        let mut x = px.floor();
                        let mut bin = ((px - x) * f32::from(SUBPIXEL_BINS)).round() as u8;
                        if bin == SUBPIXEL_BINS {
                            x += 1.0;
                            bin = 0;
                        }
                        let y = ((g.y * gs
                            + gy
                            + (fx.dy - (fx.scale - 1.0) * 0.3) * run.font_size * gs)
                            * scale)
                            .round();
                        let Some(e) = res.glyph(run, g.id, size_px, bin, 0.0) else {
                            continue;
                        };
                        let dest_x = x + e.left as f32;
                        let dest_y = y - e.top as f32;
                        let glyph = Instance {
                            rect: [
                                dest_x / scale,
                                dest_y / scale,
                                e.width as f32 / scale,
                                e.height as f32 / scale,
                            ],
                            radii: [0.0; 4],
                            radii_y: [0.0; 4],
                            xf: [xf_mark, 0.0, 0.0, 0.0],
                            p0: [e.x as f32, e.y as f32, e.width as f32, e.height as f32],
                            p1: [0.0; 4],
                            color0: color,
                            color1: [0.0; 4],
                            // y: 1 for colour glyphs, 2 for crisp (unsmoothed) masks.
                            params: [
                                KIND_GLYPH,
                                match (e.color, run.smooth) {
                                    (true, _) => 1.0,
                                    (false, true) => 0.0,
                                    (false, false) => 2.0,
                                },
                                -1.0,
                                0.0,
                            ],
                        };
                        if let Some((index, rect)) = fill
                            && !e.color
                        {
                            instances.push(Instance {
                                p1: rect,
                                params: [KIND_GLYPH, 0.0, index, 0.0],
                                ..glyph
                            });
                        }
                        if run.color.a > 0.0 || fill.is_none() {
                            instances.push(glyph);
                        }
                        // The stroke goes over the fill, centred on the outline.
                        if let Some((width, stroke_color)) = run.stroke
                            && let Some(s) = res.glyph(run, g.id, size_px, bin, width * gs * scale)
                        {
                            let (sx, sy) = (x + s.left as f32, y - s.top as f32);
                            instances.push(Instance {
                                rect: [
                                    sx / scale,
                                    sy / scale,
                                    s.width as f32 / scale,
                                    s.height as f32 / scale,
                                ],
                                p0: [s.x as f32, s.y as f32, s.width as f32, s.height as f32],
                                color0: pm(stroke_color),
                                params: [KIND_GLYPH, if run.smooth { 0.0 } else { 2.0 }, -1.0, 0.0],
                                ..glyph
                            });
                        }
                    }
                }
                if animated {
                    res.glyph_clocks().put(text.node, shown_at);
                }
                if let Some((x, baseline, size, color)) = cursor {
                    // A block `.66em` wide and an em tall, as the original's cursor bar.
                    let rect = Rect::new(
                        x * gs + gx,
                        (baseline - 0.82 * size) * gs + gy,
                        0.66 * size * gs,
                        size * gs,
                    );
                    instances.push(Instance {
                        rect: rect4(rect),
                        radii: [0.0; 4],
                        radii_y: [0.0; 4],
                        xf: [xf_mark, 0.0, 0.0, 0.0],
                        p0: [0.0; 4],
                        p1: [0.0; 4],
                        color0: pm(Rgba { a: 0.85, ..color }),
                        color1: [0.0; 4],
                        params: [KIND_QUAD, 0.0, -1.0, 0.0],
                    });
                }
            }
            DrawCmd::BoxShadow(s) => {
                let shape = s.rect.offset(s.offset_x, s.offset_y).expand(s.spread);
                // Blurred shapes use circular corners (the mean of the two radii).
                let grown = circular(s.radii.grow(s.spread));
                instances.push(Instance {
                    rect: rect4(clamp_size(shape)),
                    radii: grown,
                    radii_y: grown,
                    xf: [0.0; 4],
                    p0: rect4(s.rect),
                    p1: circular(s.radii),
                    color0: pm(s.color),
                    color1: [0.0; 4],
                    params: [KIND_OUTER_SHADOW, s.blur / 2.0, -1.0, 0.0],
                });
            }
            DrawCmd::InsetShadow {
                shadow: s,
                border: b,
            } => {
                let [top, right, bottom, left] = *b;
                let padding = clamp_size(Rect::new(
                    s.rect.x + left,
                    s.rect.y + top,
                    s.rect.width - left - right,
                    s.rect.height - top - bottom,
                ));
                let inner = s.radii.inner([top, right, bottom, left]);
                let hole = clamp_size(padding.offset(s.offset_x, s.offset_y).expand(-s.spread));
                instances.push(Instance {
                    rect: rect4(padding),
                    radii: inner.x,
                    radii_y: inner.y,
                    xf: [0.0; 4],
                    p0: rect4(hole),
                    p1: circular(inner).map(|r| (r - s.spread).max(0.0)),
                    color0: pm(s.color),
                    color1: [0.0; 4],
                    params: [KIND_INSET_SHADOW, s.blur / 2.0, -1.0, 0.0],
                });
            }
            DrawCmd::Border {
                rect,
                radii,
                widths,
                colors,
                styles,
            } => {
                // Side colours and styles ride in the gradient table: `colors[0..4]`
                // (linear, premultiplied) and `info` (style numbers).
                let mut entry = GpuGradient {
                    info: styles.map(|s| s.index() as f32),
                    ..Default::default()
                };
                for (slot, c) in entry.colors.iter_mut().zip(colors) {
                    *slot = pm(*c);
                }
                gradients.push(entry);
                instances.push(Instance {
                    rect: rect4(*rect),
                    radii: radii.x,
                    radii_y: radii.y,
                    xf: [0.0; 4],
                    p0: *widths,
                    p1: [0.0; 4],
                    color0: [0.0; 4],
                    color1: [0.0; 4],
                    params: [KIND_BORDER, 0.0, (gradients.len() - 1) as f32, 0.0],
                });
            }
            DrawCmd::BorderImage {
                area,
                border,
                image,
                pixelated,
                tint,
            } => {
                // Piece edges on whole physical pixels, so neighbours don't leave
                // anti-aliased seams between them.
                let snap = |r: Rect| {
                    let (x0, y0) = ((r.x * scale).round(), (r.y * scale).round());
                    let x1 = ((r.x + r.width) * scale).round();
                    let y1 = ((r.y + r.height) * scale).round();
                    Rect::new(x0 / scale, y0 / scale, (x1 - x0) / scale, (y1 - y0) / scale)
                };
                match image.source.as_deref() {
                    Some(platypus_ui::BackgroundImage::Url(url)) => {
                        let Some(loaded) = res.image(url) else {
                            continue;
                        };
                        let (iw, ih) = (loaded.size.0 as f32, loaded.size.1 as f32);
                        flush(&mut steps, &instances, &mut batch_start, current);
                        for piece in image.pieces(*area, *border, (iw, ih)) {
                            let dst = snap(piece.dst);
                            let x =
                                border_image_axis(dst.x, dst.width, piece.tile.0, piece.repeat[0]);
                            let y =
                                border_image_axis(dst.y, dst.height, piece.tile.1, piece.repeat[1]);
                            let (Some((tx, tw, gx, rx)), Some((ty, th, gy, ry))) = (x, y) else {
                                continue;
                            };
                            let src = piece.src;
                            instances.push(Instance {
                                rect: rect4(dst),
                                radii: [0.0; 4],
                                radii_y: [0.0; 4],
                                xf: [0.0; 4],
                                p0: [tx, ty, tw, th],
                                p1: [gx, gy, 0.0, 0.0],
                                // The piece's part of the texture (uv).
                                color0: [src.x / iw, src.y / ih, src.width / iw, src.height / ih],
                                color1: tint.map_or([0.0; 4], pm),
                                params: [
                                    KIND_IMAGE,
                                    f32::from(u8::from(rx)),
                                    f32::from(u8::from(ry)),
                                    0.0,
                                ],
                            });
                        }
                        let end = instances.len() as u32;
                        if end > batch_start {
                            steps.push(Step::Draw {
                                target: current,
                                range: batch_start..end,
                                image: Some((loaded.key.clone(), *pixelated)),
                            });
                            batch_start = end;
                        }
                    }
                    Some(source) => {
                        // A gradient: its "image" is the whole area. Pieces stretch.
                        let Some(g) = gradient::build(source, *area) else {
                            continue;
                        };
                        gradients.push(g);
                        let index = (gradients.len() - 1) as f32;
                        for piece in image.pieces(*area, *border, (area.width, area.height)) {
                            let src = piece.src;
                            instances.push(Instance {
                                rect: rect4(snap(piece.dst)),
                                radii: [0.0; 4],
                                radii_y: [0.0; 4],
                                xf: [0.0; 4],
                                p0: [area.x + src.x, area.y + src.y, src.width, src.height],
                                p1: rect4(*area),
                                color0: [0.0; 4],
                                color1: [0.0; 4],
                                params: [KIND_GRADIENT_PIECE, 0.0, index, 0.0],
                            });
                        }
                    }
                    None => {}
                }
            }
            DrawCmd::PushLayer {
                bounds,
                blur,
                opacity,
                blend,
                matrix,
                shadows,
            } => {
                tag(&mut instances, &mut clip_from, clip, xf);
                flush(&mut steps, &instances, &mut batch_start, current);
                let layer = textures.take();
                steps.push(Step::Clear {
                    target: layer,
                    color: Color::TRANSPARENT,
                });
                layer_stack.push(OpenLayer {
                    texture: layer,
                    mask: None,
                    parent: current,
                    bounds: world(*bounds),
                    blur: *blur,
                    opacity: *opacity,
                    blend: blend.index(),
                    matrix: *matrix,
                    shadows: shadows.clone(),
                    outer_clips: std::mem::take(&mut clip_stack),
                });
                current = layer;
                layers += 1;
            }
            DrawCmd::BeginMask(mode) => {
                let Some(open) = layer_stack.last_mut() else {
                    tracing::warn!("BeginMask outside a layer; ignored");
                    continue;
                };
                tag(&mut instances, &mut clip_from, clip, xf);
                flush(&mut steps, &instances, &mut batch_start, current);
                let mask = textures.take();
                steps.push(Step::Clear {
                    target: mask,
                    color: Color::TRANSPARENT,
                });
                open.mask = Some((mask, *mode));
                current = mask;
            }
            DrawCmd::PopLayer => {
                let Some(OpenLayer {
                    texture: layer_texture,
                    mask,
                    parent,
                    bounds,
                    blur,
                    opacity,
                    blend,
                    matrix,
                    shadows,
                    outer_clips,
                }) = layer_stack.pop()
                else {
                    tracing::warn!("PopLayer without PushLayer; ignored");
                    continue;
                };
                tag(&mut instances, &mut clip_from, clip, xf);
                flush(&mut steps, &instances, &mut batch_start, current);
                // Drawing may have moved on to the mask; the layer is its own texture.
                let current_layer = layer_texture;
                // What only this layer uses, free again once it is composited.
                let mut spent: Vec<usize> = vec![layer_texture];
                spent.extend(mask.map(|(m, _)| m));
                clip_stack = outer_clips;
                let layer_clip = clip_stack.last().copied().unwrap_or(0);
                let visible = match clips.bounds(layer_clip) {
                    Some(b) => bounds.intersect(b),
                    None => bounds,
                };
                // The blur reads the whole layer, so it covers `bounds`; only the
                // composite is limited to what the clip shows.
                if let (Some(area), Some(visible_area)) = (to_px(bounds), to_px(visible)) {
                    if blur > 0.0 {
                        let temp = textures.fresh();
                        steps.push(Step::Blur {
                            tex: current_layer,
                            temp,
                            area,
                            sigma: blur * scale,
                            contained: false,
                        });
                    }
                    // Drop shadows: blurred, tinted, shifted copies of the layer,
                    // beneath it (the first listed ends up on top).
                    for s in shadows.iter().rev() {
                        let shadow = textures.take();
                        spent.push(shadow);
                        steps.push(Step::Copy {
                            src: current_layer,
                            dst: shadow,
                        });
                        if s.sigma > 0.0 {
                            let temp = textures.fresh();
                            steps.push(Step::Blur {
                                tex: shadow,
                                temp,
                                area,
                                sigma: s.sigma * scale,
                                contained: false,
                            });
                        }
                        if let Some(shadow_area) = to_px(visible) {
                            steps.push(Step::Composite {
                                src: shadow,
                                dst: parent,
                                area: shadow_area,
                                opacity,
                                clip: layer_clip,
                                blend: None,
                                matrix: None,
                                tint: Some(pm(s.color)),
                                shift: [s.offset_x * scale, s.offset_y * scale],
                                mask,
                            });
                        }
                    }
                    let blend = (blend != 0).then(|| {
                        // The shader reads what's beneath from a copy of the parent.
                        let backdrop = textures.take();
                        spent.push(backdrop);
                        steps.push(Step::Copy {
                            src: parent,
                            dst: backdrop,
                        });
                        (blend, backdrop)
                    });
                    steps.push(Step::Composite {
                        src: current_layer,
                        dst: parent,
                        area: visible_area,
                        opacity,
                        clip: layer_clip,
                        blend,
                        matrix,
                        tint: None,
                        shift: [0.0; 2],
                        mask,
                    });
                }
                for t in spent {
                    textures.give(t);
                }
                current = parent;
            }
            DrawCmd::BackdropBlur {
                rect,
                radii,
                blur,
                matrix,
            } => {
                tag(&mut instances, &mut clip_from, clip, xf);
                flush(&mut steps, &instances, &mut batch_start, current);
                let own_clip =
                    clips.push(&ClipShape::rounded(*rect, *radii), clip, xf_stack.last());
                let visible = clips.bounds(own_clip).unwrap_or(world(*rect));
                // Like browsers, only the backdrop inside the element's box is blurred.
                let (Some(area), Some(blur_area)) = (to_px(visible), to_px(world(*rect))) else {
                    continue;
                };
                let copy = textures.take();
                let temp = textures.fresh();
                steps.push(Step::Copy {
                    src: current,
                    dst: copy,
                });
                if *blur > 0.0 {
                    steps.push(Step::Blur {
                        tex: copy,
                        temp,
                        area: blur_area,
                        sigma: blur * scale,
                        contained: true,
                    });
                }
                steps.push(Step::Composite {
                    src: copy,
                    dst: current,
                    area,
                    opacity: 1.0,
                    clip: own_clip,
                    blend: None,
                    matrix: *matrix,
                    tint: None,
                    shift: [0.0; 2],
                    mask: None,
                });
                textures.give(copy);
            }
        }
    }
    let clip = clip_stack.last().copied().unwrap_or(0);
    let xf = xf_stack.last().map_or(0, |x| x.0);
    tag(&mut instances, &mut clip_from, clip, xf);
    flush(&mut steps, &instances, &mut batch_start, current);
    // Close layers left open by an unbalanced list.
    while let Some(OpenLayer {
        parent,
        bounds,
        opacity,
        outer_clips,
        ..
    }) = layer_stack.pop()
    {
        tracing::warn!("PushLayer without PopLayer; closing it");
        if let Some(area) = to_px(bounds) {
            steps.push(Step::Composite {
                src: current,
                dst: parent,
                area,
                opacity,
                clip: outer_clips.last().copied().unwrap_or(0),
                blend: None,
                matrix: None,
                tint: None,
                shift: [0.0; 2],
                mask: None,
            });
        }
        current = parent;
    }

    FramePlan {
        instances,
        gradients,
        clips: clips.table,
        steps,
        textures: textures.count(),
        layers,
        blending,
    }
}

/// The frame-sized textures a plan's steps use: 0 is the canvas; others are handed out and given back.
struct TexturePool {
    /// How many there are (the next new one's index).
    count: usize,
    /// Given back, to be handed out again (each is cleared or copied over by the step that takes it).
    free: Vec<usize>,
}

impl Default for TexturePool {
    fn default() -> Self {
        Self {
            count: 1,
            free: Vec::new(),
        }
    }
}

impl TexturePool {
    /// A texture for a layer, mask or copy: one given back if there is one.
    fn take(&mut self) -> usize {
        self.free.pop().unwrap_or_else(|| self.fresh())
    }

    /// A texture no step has used yet in this frame (a blur's scratch).
    fn fresh(&mut self) -> usize {
        self.count += 1;
        self.count - 1
    }

    fn give(&mut self, texture: usize) {
        self.free.push(texture);
    }

    fn count(&self) -> usize {
        self.count
    }
}

pub(super) fn rect4(r: Rect) -> [f32; 4] {
    [r.x, r.y, r.width, r.height]
}

pub(super) fn clamp_size(r: Rect) -> Rect {
    Rect::new(r.x, r.y, r.width.max(0.0), r.height.max(0.0))
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB straight alpha → linear premultiplied.
fn linear_premultiplied(c: Rgba) -> [f32; 4] {
    [
        srgb_to_linear(c.r) * c.a,
        srgb_to_linear(c.g) * c.a,
        srgb_to_linear(c.b) * c.a,
        c.a,
    ]
}

/// sRGB straight alpha → premultiplied in `blending`'s space, as the shaders expect.
pub fn canvas_premultiplied(c: Rgba, blending: Blending) -> [f32; 4] {
    match blending {
        Blending::Linear => linear_premultiplied(c),
        Blending::Srgb => [c.r * c.a, c.g * c.a, c.b * c.a, c.a],
    }
}

/// A clear colour (linear, as [`Color`] is) in the canvases' space.
fn canvas_clear(c: Color, blending: Blending) -> Color {
    match blending {
        Blending::Linear => c,
        Blending::Srgb if c.a <= 0.0 => Color::TRANSPARENT,
        Blending::Srgb => {
            let encode = |v: f64| linear_to_srgb(v / c.a) * c.a;
            Color {
                r: encode(c.r),
                g: encode(c.g),
                b: encode(c.b),
                a: c.a,
            }
        }
    }
}
