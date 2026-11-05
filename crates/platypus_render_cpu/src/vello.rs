// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A CPU renderer for the UI draw list (`vello_cpu`), shown through `softbuffer`: no GPU API, no
//! GPU driver. `platypus-app` draws with it when a project asks for `renderer: "cpu"`
//! (`cpu_gfx.rs`; never chosen automatically).
//!
//! It draws what the UI draw list holds, except `border-image`, `mask`s (their layer is drawn
//! unmasked), drop shadows and colour matrices on layers, perspective (the affine part is used) and
//! border styles other than solid (drawn solid). Each skipped command is logged once.

mod blur;
mod images;
mod paint;

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use platypus_ui::transform::Homography;
use platypus_ui::{BackgroundImage, BlendMode, DrawCmd, ImageFill, Quad, Rgba};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use vello_common::filter_effects::{Filter, FilterFunction};
/// The pixels a frame was drawn into (premultiplied RGBA8).
pub use vello_cpu::Pixmap;
use vello_cpu::kurbo::{Affine, BezPath, Rect, RoundedRect, RoundedRectRadii, Shape};
use vello_cpu::peniko::{self, Color, Extend, Fill, ImageQuality, ImageSampler};
use vello_cpu::{Glyph, Image, ImageSource, RenderContext, RenderSettings, Resources};

fn color(c: Rgba) -> Color {
    Color::new([c.r, c.g, c.b, c.a])
}

fn rect(r: platypus_ui::Rect) -> Rect {
    Rect::new(
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.x + r.width),
        f64::from(r.y + r.height),
    )
}

/// A rounded box (elliptical corners are drawn circular, at the horizontal radius).
fn rounded(r: platypus_ui::Rect, radii: [f32; 4]) -> RoundedRect {
    RoundedRect::from_rect(
        rect(r),
        RoundedRectRadii::new(
            f64::from(radii[0]),
            f64::from(radii[1]),
            f64::from(radii[2]),
            f64::from(radii[3]),
        ),
    )
}

fn mix(mode: BlendMode) -> peniko::Mix {
    use peniko::Mix;
    match mode {
        BlendMode::Normal => Mix::Normal,
        BlendMode::Multiply => Mix::Multiply,
        BlendMode::Screen => Mix::Screen,
        BlendMode::Overlay => Mix::Overlay,
        BlendMode::Darken => Mix::Darken,
        BlendMode::Lighten => Mix::Lighten,
        BlendMode::ColorDodge => Mix::ColorDodge,
        BlendMode::ColorBurn => Mix::ColorBurn,
        BlendMode::HardLight => Mix::HardLight,
        BlendMode::SoftLight => Mix::SoftLight,
        BlendMode::Difference => Mix::Difference,
        BlendMode::Exclusion => Mix::Exclusion,
        BlendMode::Hue => Mix::Hue,
        BlendMode::Saturation => Mix::Saturation,
        BlendMode::Color => Mix::Color,
        BlendMode::Luminosity => Mix::Luminosity,
    }
}

fn image_paint(pixmap: Arc<Pixmap>, quality: ImageQuality) -> Image {
    Image {
        image: ImageSource::Pixmap(pixmap),
        sampler: ImageSampler {
            x_extend: Extend::Pad,
            y_extend: Extend::Pad,
            quality,
            alpha: 1.0,
        },
    }
}

/// Where the tiles of a background layer start along one axis so they cover `lo..hi`.
fn tile_starts(start: f32, step: f32, lo: f32, hi: f32, repeats: bool) -> Vec<f32> {
    if !repeats || step <= 0.0 {
        return vec![start];
    }
    let first = start - ((start - lo) / step).ceil() * step;
    (0..)
        .map(|i| first + i as f32 * step)
        .take_while(|p| *p < hi)
        .take(512)
        .collect()
}

fn is_perspective(h: &Homography) -> bool {
    h.0[2][0].abs() > 1e-6 || h.0[2][1].abs() > 1e-6 || (h.0[2][2] - 1.0).abs() > 1e-6
}

/// The area the commands can paint (a little generous), or `None` when they paint nothing.
fn cmds_bounds(cmds: &[DrawCmd]) -> Option<platypus_ui::Rect> {
    let mut bounds: Option<platypus_ui::Rect> = None;
    let mut add = |r: platypus_ui::Rect| {
        if r.width > 0.0 && r.height > 0.0 {
            bounds = Some(bounds.map_or(r, |b| b.union(r)));
        }
    };
    for cmd in cmds {
        match cmd {
            DrawCmd::Quad(q) => add(q.rect),
            DrawCmd::BoxShadow(s) => add(s
                .rect
                .expand(s.spread + s.blur * 1.5)
                .offset(s.offset_x, s.offset_y)),
            DrawCmd::InsetShadow { shadow, .. } => add(shadow.rect),
            DrawCmd::Text(t) => add(t.bounds),
            DrawCmd::Border { rect, .. } | DrawCmd::BackdropBlur { rect, .. } => add(*rect),
            DrawCmd::BorderImage { area, .. } => add(*area),
            DrawCmd::PushLayer { bounds, .. } => add(*bounds),
            _ => {}
        }
    }
    bounds.map(|b| b.expand(1.0))
}

/// A text run's glyphs, filled with the context's current paint.
fn fill_run(ctx: &mut RenderContext, resources: &mut Resources, run: &platypus_ui::GlyphRun) {
    let skew = run.skew.map_or(Affine::IDENTITY, |deg| {
        Affine::skew(-f64::from(deg).to_radians().tan(), 0.0)
    });
    let _ = ctx
        .glyph_run(resources, &run.font)
        .font_size(run.font_size)
        .glyph_transform(skew)
        .normalized_coords(&run.normalized_coords)
        .hint(false)
        .fill_glyphs(run.glyphs.iter().map(|g| Glyph {
            id: g.id,
            x: g.x,
            y: g.y,
        }));
}

/// Most flat renders of perspective subtrees kept.
const MAX_FLATS: usize = 12;

/// A perspective subtree's commands drawn flat, to draw again while they stay the same.
struct Flat {
    cmds: Vec<DrawCmd>,
    /// The scale it was drawn at (`f32` bits).
    scale: u32,
    pixmap: Arc<Pixmap>,
}

/// Draws draw lists into a pixel buffer.
pub struct CpuRenderer {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
    size: (u16, u16),
    /// Threads the context was made for (0: one, which layer filters need).
    threads: u16,
    default_threads: u16,
    /// Physical pixels per logical pixel of the list being drawn.
    scale: f64,
    /// Logical pixels to device pixels for the list being drawn (the scale, and a shift when
    /// drawing a subtree offscreen).
    base: Affine,
    /// Draws the subtrees of perspective transforms offscreen (made when first needed).
    sub: Option<Box<CpuRenderer>>,
    /// Recent flat renders of those subtrees, the most recently used last.
    flats: Vec<Flat>,
    /// The transforms outside the open `PushTransform`s.
    transforms: Vec<Affine>,
    /// Open clips and layers (a backdrop blur needs the scene settled, so none open).
    clips: usize,
    layers: usize,
    /// Inside a layer's `BeginMask` content (not drawn): the nested layers seen.
    masking: Option<usize>,
    images: images::Images,
    /// Commands already reported as unsupported.
    skipped: Vec<&'static str>,
}

impl Default for CpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuRenderer {
    pub fn new() -> Self {
        Self::with_threads(RenderSettings::default().num_threads)
    }

    /// A renderer drawing with `default_threads` worker threads (0: the calling thread only).
    fn with_threads(default_threads: u16) -> Self {
        Self {
            ctx: RenderContext::new_with(
                1,
                1,
                RenderSettings {
                    num_threads: default_threads,
                    ..RenderSettings::default()
                },
            ),
            resources: Resources::new(),
            pixmap: Pixmap::new(1, 1),
            size: (1, 1),
            threads: default_threads,
            default_threads,
            scale: 1.0,
            base: Affine::IDENTITY,
            sub: None,
            flats: Vec::new(),
            transforms: Vec::new(),
            clips: 0,
            layers: 0,
            masking: None,
            images: images::Images::default(),
            skipped: Vec::new(),
        }
    }

    /// The folder `url(...)` images are read from (the game folder).
    pub fn set_asset_root(&mut self, root: impl Into<PathBuf>) {
        self.images.set_root(root.into());
    }

    /// Asks for linear-light blending: `vello_cpu` blends 8-bit sRGB values (as browsers), so
    /// this renderer always does; the request is logged once.
    pub fn ask_linear_blending(&mut self) {
        self.skip("linear-light blending (`blending: \"linear\"`; blended in sRGB)");
    }

    /// An image's natural size, for layout (`None` if it can't be loaded).
    pub fn image_size(&mut self, url: &str) -> Option<(u32, u32)> {
        self.images.size(url)
    }

    fn skip(&mut self, what: &'static str) {
        if !self.skipped.contains(&what) {
            self.skipped.push(what);
            tracing::warn!("CPU renderer: {what} is not drawn");
        }
    }

    /// Draws each `(scale, commands)` layer in turn (commands in logical pixels, `scale` physical
    /// pixels per logical one) over `clear` into a `width` x `height` buffer and returns the pixels.
    pub fn render(
        &mut self,
        size: (u32, u32),
        clear: Rgba,
        layers: &[(f32, &[DrawCmd])],
    ) -> &Pixmap {
        self.images.begin_draw();
        let layers: Vec<(f32, (f32, f32), &[DrawCmd])> =
            layers.iter().map(|&(s, c)| (s, (0.0, 0.0), c)).collect();
        self.render_layers(size, clear, &layers)
    }

    /// [`Self::render`] with each layer's origin: the logical point drawn at the top left.
    fn render_layers(
        &mut self,
        (width, height): (u32, u32),
        clear: Rgba,
        layers: &[(f32, (f32, f32), &[DrawCmd])],
    ) -> &Pixmap {
        let size = (width.clamp(1, 65535) as u16, height.clamp(1, 65535) as u16);
        // Layer filters need a single-threaded context.
        let filters = layers.iter().any(|(_, _, cmds)| {
            cmds.iter()
                .any(|c| matches!(c, DrawCmd::PushLayer { blur, .. } if *blur > 0.0))
        });
        let threads = if filters { 0 } else { self.default_threads };
        if size != self.size || threads != self.threads {
            self.ctx = RenderContext::new_with(
                size.0,
                size.1,
                RenderSettings {
                    num_threads: threads,
                    ..RenderSettings::default()
                },
            );
            self.pixmap = Pixmap::new(size.0, size.1);
            self.size = size;
            self.threads = threads;
        } else {
            self.ctx.reset();
        }
        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx.set_paint(color(clear));
        self.ctx
            .fill_rect(&Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
        for &(scale, (ox, oy), cmds) in layers {
            self.scale = f64::from(scale);
            self.base =
                Affine::scale(self.scale) * Affine::translate((-f64::from(ox), -f64::from(oy)));
            self.transforms.clear();
            (self.clips, self.layers, self.masking) = (0, 0, None);
            self.ctx.set_transform(self.base);
            self.run(cmds);
        }
        self.ctx.flush();
        self.ctx.render(&mut self.pixmap, &mut self.resources);
        &self.pixmap
    }

    fn fill_shape(&mut self, shape: &RoundedRect, c: Rgba) {
        self.ctx.set_paint(color(c));
        self.ctx.fill_path(&shape.to_path(0.1));
    }

    fn push_clip(&mut self, shape: &RoundedRect) {
        self.ctx.push_clip_path(&shape.to_path(0.1));
        self.clips += 1;
    }

    fn pop_clip(&mut self) {
        if self.clips > 0 {
            self.clips -= 1;
            self.ctx.pop_clip();
        }
    }

    /// One background layer (gradient or image) of a quad.
    fn image_fill(&mut self, q: &Quad, fill: &ImageFill) {
        let scale = self.scale as f32;
        let url = match &*fill.image {
            BackgroundImage::Url(u) => Some(u.as_str()),
            _ => None,
        };
        // A gradient has no size of its own: `auto` is the box.
        let natural = match url {
            None => (q.rect.width, q.rect.height),
            Some(url) => {
                // The natural size is in the file's header: nothing is decoded for it.
                let Some(natural) = self.images.size(url) else {
                    return;
                };
                (natural.0 as f32, natural.1 as f32)
            }
        };
        let tile = platypus_ui::tile_rect(q.rect, natural, fill.size, fill.position);
        let (tile, gap, repeat) = platypus_ui::repeat_tiles(tile, q.rect, fill.repeat);
        // An SVG is rasterized at the tile's size on screen.
        let image = match url {
            None => None,
            Some(url) => {
                let px = (tile.width * scale, tile.height * scale);
                let Some((pixmap, _)) = self.images.get(url, px) else {
                    return;
                };
                Some(pixmap)
            }
        };
        if fill.tint.is_some() {
            self.skip("background-image-tint");
        }
        self.push_clip(&rounded(fill.area, fill.radii.x));
        let xs = tile_starts(
            tile.x,
            tile.width + gap[0],
            fill.area.x,
            fill.area.x + fill.area.width,
            repeat[0],
        );
        let ys = tile_starts(
            tile.y,
            tile.height + gap[1],
            fill.area.y,
            fill.area.y + fill.area.height,
            repeat[1],
        );
        let quality = if fill.pixelated {
            ImageQuality::Low
        } else {
            ImageQuality::Medium
        };
        for &y in &ys {
            for &x in &xs {
                let t = platypus_ui::Rect::new(x, y, tile.width, tile.height);
                let visible = t.intersect(fill.area);
                if visible.is_empty() {
                    continue;
                }
                match &image {
                    None => {
                        let Some((g, stretch)) = paint::gradient(&fill.image, t) else {
                            continue;
                        };
                        self.ctx.set_paint(g);
                        self.ctx.set_paint_transform(stretch);
                    }
                    Some(pixmap) => {
                        let (iw, ih) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
                        self.ctx.set_paint(image_paint(pixmap.clone(), quality));
                        self.ctx.set_paint_transform(
                            Affine::translate((f64::from(t.x), f64::from(t.y)))
                                * Affine::scale_non_uniform(
                                    f64::from(t.width) / iw,
                                    f64::from(t.height) / ih,
                                ),
                        );
                    }
                }
                self.ctx.fill_rect(&rect(visible));
            }
        }
        self.ctx.reset_paint_transform();
        self.pop_clip();
    }

    /// A border whose sides differ (colours, widths or visibility): each side is its trapezoid
    /// of the ring between the outer and the inner edge.
    fn border(
        &mut self,
        r: platypus_ui::Rect,
        radii: platypus_ui::Radii,
        widths: [f32; 4],
        colors: [Rgba; 4],
        styles: [platypus_ui::BorderStyle; 4],
    ) {
        use platypus_ui::BorderStyle;
        let [t, rt, b, l] = widths;
        let inner_rect = platypus_ui::Rect::new(
            r.x + l,
            r.y + t,
            (r.width - l - rt).max(0.0),
            (r.height - t - b).max(0.0),
        );
        let outer = rounded(r, radii.x);
        let inner = rounded(inner_rect, radii.inner(widths).x);
        let mut ring: BezPath = outer.to_path(0.1);
        ring.extend(inner.to_path(0.1));
        if styles.iter().any(|s| {
            !matches!(
                s,
                BorderStyle::Solid | BorderStyle::None | BorderStyle::Hidden
            )
        }) {
            self.skip("border styles other than solid (drawn solid)");
        }
        let (ox0, oy0, ox1, oy1) = (r.x, r.y, r.x + r.width, r.y + r.height);
        let (ix0, iy0, ix1, iy1) = (
            inner_rect.x,
            inner_rect.y,
            inner_rect.x + inner_rect.width,
            inner_rect.y + inner_rect.height,
        );
        let sides = [
            [(ox0, oy0), (ox1, oy0), (ix1, iy0), (ix0, iy0)],
            [(ox1, oy0), (ox1, oy1), (ix1, iy1), (ix1, iy0)],
            [(ox1, oy1), (ox0, oy1), (ix0, iy1), (ix1, iy1)],
            [(ox0, oy1), (ox0, oy0), (ix0, iy0), (ix0, iy1)],
        ];
        self.ctx.set_fill_rule(Fill::EvenOdd);
        for (i, corners) in sides.iter().enumerate() {
            if widths[i] <= 0.0 || colors[i].a <= 0.0 || !styles[i].is_visible() {
                continue;
            }
            let mut trapezoid = BezPath::new();
            trapezoid.move_to((f64::from(corners[0].0), f64::from(corners[0].1)));
            for c in &corners[1..] {
                trapezoid.line_to((f64::from(c.0), f64::from(c.1)));
            }
            trapezoid.close_path();
            self.ctx.push_clip_path(&trapezoid);
            self.ctx.set_paint(color(colors[i]));
            self.ctx.fill_path(&ring);
            self.ctx.pop_clip();
        }
        self.ctx.set_fill_rule(Fill::NonZero);
    }

    /// `backdrop-filter`: what is drawn so far is rendered, blurred (and colour-matrixed) inside
    /// the box, and drawn again as the new base. Only with nothing open (no clip, layer or
    /// transform), as the scene can't be read back otherwise.
    fn backdrop(
        &mut self,
        r: platypus_ui::Rect,
        radii: platypus_ui::Radii,
        blur: f32,
        matrix: Option<[f32; 20]>,
    ) {
        if self.clips > 0 || self.layers > 0 || !self.transforms.is_empty() {
            self.skip("backdrop blur inside a clip, layer or transform");
            return;
        }
        self.ctx.flush();
        self.ctx.render(&mut self.pixmap, &mut self.resources);
        let (w, h) = (
            usize::from(self.pixmap.width()),
            usize::from(self.pixmap.height()),
        );
        let s = self.scale as f32;
        let x0 = ((r.x * s).floor().max(0.0) as usize).min(w);
        let y0 = ((r.y * s).floor().max(0.0) as usize).min(h);
        let x1 = (((r.x + r.width) * s).ceil().max(0.0) as usize).min(w);
        let y1 = (((r.y + r.height) * s).ceil().max(0.0) as usize).min(h);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let region = (x0, y0, x1 - x0, y1 - y0);
        let mut blurred = self.pixmap.clone();
        blur::blur_region(blurred.data_as_u8_slice_mut(), w, region, blur * s);
        if let Some(m) = &matrix {
            blur::color_matrix_region(blurred.data_as_u8_slice_mut(), w, region, m);
        }
        let base = Arc::new(self.pixmap.clone());
        let blurred = Arc::new(blurred);
        self.ctx.reset();
        self.ctx.set_transform(Affine::IDENTITY);
        let screen = Rect::new(0.0, 0.0, w as f64, h as f64);
        self.ctx.set_paint(image_paint(base, ImageQuality::Low));
        self.ctx.fill_rect(&screen);
        let mut clip = rounded(r, radii.x).to_path(0.1);
        clip.apply_affine(Affine::scale(self.scale));
        self.ctx.push_clip_path(&clip);
        self.ctx.set_paint(image_paint(blurred, ImageQuality::Low));
        self.ctx.fill_rect(&screen);
        self.ctx.pop_clip();
        self.ctx.set_transform(self.base);
    }

    /// Draws commands in order; a perspective transform's subtree is drawn offscreen and warped.
    fn run(&mut self, cmds: &[DrawCmd]) {
        let mut i = 0;
        while i < cmds.len() {
            if let DrawCmd::PushTransform(h) = &cmds[i]
                && self.masking.is_none()
                && is_perspective(h)
            {
                let mut depth = 1;
                let mut end = i + 1;
                while end < cmds.len() {
                    match cmds[end] {
                        DrawCmd::PushTransform(_) => depth += 1,
                        DrawCmd::PopTransform => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        break;
                    }
                    end += 1;
                }
                self.warped(h, &cmds[i + 1..end]);
                i = end + 1;
                continue;
            }
            self.draw(&cmds[i]);
            i += 1;
        }
    }

    /// A subtree under a perspective transform: drawn flat into its own pixmap (at the
    /// resolution it is shown at), then each window pixel samples it through the inverse map.
    /// The flat copy is kept while the commands stay the same: a card sliding past only changes
    /// its transform.
    fn warped(&mut self, h: &Homography, cmds: &[DrawCmd]) {
        let (Some(bounds), Some(inverse)) = (cmds_bounds(cmds), h.inverse()) else {
            return;
        };
        let win_w = usize::from(self.size.0);
        let win_h = usize::from(self.size.1);
        let s = self.scale as f32;
        // Where the plane lands in the window, in device pixels.
        let mapped = h.map_rect(bounds);
        let x0 = ((mapped.x * s).floor().max(0.0) as usize).min(win_w);
        let y0 = ((mapped.y * s).floor().max(0.0) as usize).min(win_h);
        let x1 = (((mapped.x + mapped.width) * s).ceil().max(0.0) as usize).min(win_w);
        let y1 = (((mapped.y + mapped.height) * s).ceil().max(0.0) as usize).min(win_h);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        // The flat copy at the window's scale, within what a pixmap can hold.
        let src_scale = s.min(8192.0 / bounds.width.max(bounds.height).max(1.0));
        let source = match self
            .flats
            .iter()
            .position(|f| f.scale == src_scale.to_bits() && f.cmds == cmds)
        {
            Some(i) => {
                // Most recently used last.
                let flat = self.flats.remove(i);
                let pixmap = flat.pixmap.clone();
                self.flats.push(flat);
                pixmap
            }
            None => {
                let flat = (
                    (bounds.width * src_scale).ceil().max(1.0) as u32,
                    (bounds.height * src_scale).ceil().max(1.0) as u32,
                );
                let mut inner = self
                    .sub
                    .take()
                    .unwrap_or_else(|| Box::new(CpuRenderer::with_threads(0)));
                std::mem::swap(&mut inner.images, &mut self.images);
                let pixmap = Arc::new(
                    inner
                        .render_layers(
                            flat,
                            Rgba::TRANSPARENT,
                            &[(src_scale, (bounds.x, bounds.y), cmds)],
                        )
                        .clone(),
                );
                std::mem::swap(&mut inner.images, &mut self.images);
                self.sub = Some(inner);
                if self.flats.len() >= MAX_FLATS {
                    self.flats.remove(0);
                }
                self.flats.push(Flat {
                    cmds: cmds.to_vec(),
                    scale: src_scale.to_bits(),
                    pixmap: pixmap.clone(),
                });
                pixmap
            }
        };

        let (sw, sh) = (usize::from(source.width()), usize::from(source.height()));
        let src = source.data();
        let texel = |x: isize, y: isize| -> [f32; 4] {
            if x < 0 || y < 0 || x >= sw as isize || y >= sh as isize {
                return [0.0; 4];
            }
            let p = src[y as usize * sw + x as usize];
            [
                f32::from(p.r),
                f32::from(p.g),
                f32::from(p.b),
                f32::from(p.a),
            ]
        };
        let out_w = x1 - x0;
        let mut out = Pixmap::new(out_w as u16, (y1 - y0) as u16);
        // Rows are independent: split them over the cores.
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8);
        let rows_per = (y1 - y0).div_ceil(workers).max(1);
        std::thread::scope(|scope| {
            for (chunk, rows) in out.data_mut().chunks_mut(out_w * rows_per).enumerate() {
                let (texel, inverse) = (&texel, &inverse);
                scope.spawn(move || {
                    for (r, row) in rows.chunks_mut(out_w).enumerate() {
                        let y = y0 + chunk * rows_per + r;
                        for (c, p) in row.iter_mut().enumerate() {
                            let x = x0 + c;
                            let (lx, ly) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
                            let Some((u, v)) = inverse.apply(lx, ly) else {
                                continue;
                            };
                            let (fx, fy) = (
                                (u - bounds.x) * src_scale - 0.5,
                                (v - bounds.y) * src_scale - 0.5,
                            );
                            let (ix, iy) = (fx.floor(), fy.floor());
                            let (tx, ty) = (fx - ix, fy - iy);
                            let (ix, iy) = (ix as isize, iy as isize);
                            let (a, b, c2, d) = (
                                texel(ix, iy),
                                texel(ix + 1, iy),
                                texel(ix, iy + 1),
                                texel(ix + 1, iy + 1),
                            );
                            let mut px = [0u8; 4];
                            for k in 0..4 {
                                let top = a[k] + (b[k] - a[k]) * tx;
                                let bottom = c2[k] + (d[k] - c2[k]) * tx;
                                px[k] = (top + (bottom - top) * ty).round().clamp(0.0, 255.0) as u8;
                            }
                            (p.r, p.g, p.b, p.a) = (px[0], px[1], px[2], px[3]);
                        }
                    }
                });
            }
        });
        // Drawn in device pixels, under the clips and layers open around the subtree.
        let transform = *self.ctx.transform();
        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx
            .set_paint(image_paint(Arc::new(out), ImageQuality::Low));
        self.ctx
            .set_paint_transform(Affine::translate((x0 as f64, y0 as f64)));
        self.ctx
            .fill_rect(&Rect::new(x0 as f64, y0 as f64, x1 as f64, y1 as f64));
        self.ctx.reset_paint_transform();
        self.ctx.set_transform(transform);
    }

    fn draw(&mut self, cmd: &DrawCmd) {
        // Inside a mask's content (not drawn): only the layer nesting matters.
        if let Some(nest) = self.masking {
            match cmd {
                DrawCmd::PushLayer { .. } => self.masking = Some(nest + 1),
                DrawCmd::PopLayer if nest == 0 => {
                    self.masking = None;
                    self.layers = self.layers.saturating_sub(1);
                    self.ctx.pop_layer();
                }
                DrawCmd::PopLayer => self.masking = Some(nest - 1),
                _ => {}
            }
            return;
        }
        match cmd {
            DrawCmd::Quad(q) => {
                let outer = rounded(q.rect, q.radii.x);
                if q.background.a > 0.0 {
                    self.fill_shape(&outer, q.background);
                }
                for fill in &q.images {
                    self.image_fill(q, fill);
                }
                if q.border_color.a > 0.0 && q.border.iter().any(|w| *w > 0.0) {
                    let [t, r, b, l] = q.border;
                    let inner_rect = platypus_ui::Rect::new(
                        q.rect.x + l,
                        q.rect.y + t,
                        (q.rect.width - l - r).max(0.0),
                        (q.rect.height - t - b).max(0.0),
                    );
                    let inner = rounded(inner_rect, q.radii.inner(q.border).x);
                    let mut ring: BezPath = outer.to_path(0.1);
                    ring.extend(inner.to_path(0.1));
                    self.ctx.set_paint(color(q.border_color));
                    self.ctx.set_fill_rule(Fill::EvenOdd);
                    self.ctx.fill_path(&ring);
                    self.ctx.set_fill_rule(Fill::NonZero);
                }
            }
            DrawCmd::BoxShadow(s) => {
                let grown = s.rect.expand(s.spread).offset(s.offset_x, s.offset_y);
                self.ctx.set_paint(color(s.color));
                self.ctx.fill_blurred_rounded_rect(
                    &rect(grown),
                    s.radii.grow(s.spread).x[0],
                    s.blur / 2.0,
                    false,
                );
            }
            DrawCmd::InsetShadow { shadow: s, border } => {
                let [t, r, b, l] = *border;
                let inner = platypus_ui::Rect::new(
                    s.rect.x + l,
                    s.rect.y + t,
                    (s.rect.width - l - r).max(0.0),
                    (s.rect.height - t - b).max(0.0),
                );
                let radii = s.radii.inner(*border);
                self.push_clip(&rounded(inner, radii.x));
                // The shadow is what lies outside the shifted, shrunk box.
                let hole = inner.offset(s.offset_x, s.offset_y).expand(-s.spread);
                self.ctx.set_paint(color(s.color));
                self.ctx.fill_blurred_rounded_rect(
                    &rect(hole),
                    (radii.x[0] - s.spread).max(0.0),
                    s.blur / 2.0,
                    true,
                );
                self.pop_clip();
            }
            DrawCmd::Text(t) => {
                // `background-clip: text`: the gradient inside the glyphs, under the colour.
                if let Some((image, area)) = &t.fill
                    && let Some((g, stretch)) = paint::gradient(image, *area)
                {
                    for run in &t.runs {
                        self.ctx.set_paint(g.clone());
                        self.ctx.set_paint_transform(stretch);
                        fill_run(&mut self.ctx, &mut self.resources, run);
                    }
                    self.ctx.reset_paint_transform();
                }
                for run in &t.runs {
                    self.ctx.set_paint(color(run.color));
                    fill_run(&mut self.ctx, &mut self.resources, run);
                }
                for d in &t.decorations {
                    self.ctx.set_paint(color(d.color));
                    self.ctx.fill_rect(&rect(d.rect));
                }
            }
            DrawCmd::PushClip(shape) => {
                if let Some(polygon) = &shape.polygon {
                    let mut path = BezPath::new();
                    for contour in &polygon.contours {
                        for (i, &(x, y)) in contour.iter().enumerate() {
                            let p = (f64::from(x), f64::from(y));
                            if i == 0 {
                                path.move_to(p);
                            } else {
                                path.line_to(p);
                            }
                        }
                        path.close_path();
                    }
                    self.ctx.set_fill_rule(if polygon.even_odd {
                        Fill::EvenOdd
                    } else {
                        Fill::NonZero
                    });
                    self.ctx.push_clip_path(&path);
                    self.ctx.set_fill_rule(Fill::NonZero);
                    self.clips += 1;
                } else {
                    self.push_clip(&rounded(shape.rect, shape.radii_x));
                }
            }
            DrawCmd::PopClip => self.pop_clip(),
            DrawCmd::PushLayer {
                opacity,
                blur,
                blend,
                matrix,
                shadows,
                ..
            } => {
                if matrix.is_some() {
                    self.skip("colour matrices on layers");
                }
                if !shadows.is_empty() {
                    self.skip("drop shadows on layers");
                }
                let filter = (*blur > 0.0)
                    .then(|| Filter::from_function(FilterFunction::Blur { radius: *blur }));
                let blend = (*blend != BlendMode::Normal)
                    .then(|| peniko::BlendMode::new(mix(*blend), peniko::Compose::SrcOver));
                self.ctx
                    .push_layer(None, blend, Some(*opacity), None, filter);
                self.layers += 1;
            }
            DrawCmd::PopLayer => {
                self.layers = self.layers.saturating_sub(1);
                self.ctx.pop_layer();
            }
            DrawCmd::PushTransform(h) => {
                // The plane's map to the window replaces the transform outside it.
                if h.0[2] != [0.0, 0.0, 1.0] {
                    self.skip("perspective (drawn as its affine part)");
                }
                let m = h.0;
                let w = if m[2][2].abs() > 1e-6 {
                    f64::from(m[2][2])
                } else {
                    1.0
                };
                let f = |v: f32| f64::from(v) / w;
                self.transforms.push(*self.ctx.transform());
                self.ctx.set_transform(
                    self.base
                        * Affine::new([
                            f(m[0][0]),
                            f(m[1][0]),
                            f(m[0][1]),
                            f(m[1][1]),
                            f(m[0][2]),
                            f(m[1][2]),
                        ]),
                );
            }
            DrawCmd::PopTransform => {
                if let Some(t) = self.transforms.pop() {
                    self.ctx.set_transform(t);
                }
            }
            DrawCmd::BackdropBlur {
                rect,
                radii,
                blur,
                matrix,
            } => self.backdrop(*rect, *radii, *blur, *matrix),
            DrawCmd::BeginMask(_) => {
                self.skip("masks (the layer is drawn unmasked)");
                self.masking = Some(0);
            }
            DrawCmd::Border {
                rect,
                radii,
                widths,
                colors,
                styles,
            } => self.border(*rect, *radii, *widths, *colors, *styles),
            DrawCmd::BorderImage { .. } => self.skip("border-image"),
        }
    }
}

/// Shows pixel buffers in a window with no GPU API.
pub struct Presenter<W: HasDisplayHandle + HasWindowHandle + Clone> {
    surface: softbuffer::Surface<W, W>,
    // Kept alive for the surface.
    _context: softbuffer::Context<W>,
}

impl<W: HasDisplayHandle + HasWindowHandle + Clone> Presenter<W> {
    pub fn new(window: W) -> Result<Self, String> {
        let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
        let surface = softbuffer::Surface::new(&context, window).map_err(|e| e.to_string())?;
        Ok(Self {
            surface,
            _context: context,
        })
    }

    /// Copies `pixmap` (opaque, premultiplied RGBA) to the window.
    pub fn present(&mut self, pixmap: &Pixmap) -> Result<(), String> {
        let (Some(w), Some(h)) = (
            NonZeroU32::new(u32::from(pixmap.width())),
            NonZeroU32::new(u32::from(pixmap.height())),
        ) else {
            return Ok(());
        };
        self.surface.resize(w, h).map_err(|e| e.to_string())?;
        let mut buffer = self.surface.buffer_mut().map_err(|e| e.to_string())?;
        for (out, p) in buffer.iter_mut().zip(pixmap.data()) {
            *out = u32::from(p.r) << 16 | u32::from(p.g) << 8 | u32::from(p.b);
        }
        buffer.present().map_err(|e| e.to_string())
    }
}
