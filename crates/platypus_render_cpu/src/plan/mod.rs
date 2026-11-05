// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A CPU executor of `platypus_render`'s frame plan: the third beside wgpu (`UiRenderer`) and GLES
//! (`platypus_render_gles`), and the same plan, glyph cache and image cache as those, so it draws what
//! they draw. [`PlanRenderer`] plans a draw list and runs its steps as the GLES executor does
//! (`Clear` when something first reads or draws on a canvas, `Draw`, `Copy`, `Blur` in two passes,
//! `Composite`), with the seven kernel families of [`kernels`] instead of shader programs.
//!
//! Nothing is compiled at run time and no driver is loaded: on a machine whose GL would be a software
//! rasteriser, an app drawing with this doesn't map Mesa or LLVM. The canvases are plain memory
//! (`canvas`), drawn by the process's few threads (`pool`) in bands of rows.
//!
//! Not done: `render_over` (a canvas seeded from the target, for backdrop filters over a compositor's
//! windows), which a window drawing only itself doesn't need.

mod canvas;
mod color;
mod geometry;
mod gradient;
pub mod kernels;
mod pool;

use std::path::PathBuf;

pub use platypus_render::Blending;
use platypus_render::glyphs::{COLOR_SIZE, GlyphCache, GlyphEntry, MASK_SIZE};
use platypus_render::image_cache::{ImageCache, ImageUpload};
use platypus_render::plan::{self, FramePlan, ImageInfo, PlanParams, PlanResources, PxRect, Step};
use platypus_render::{Color, RenderStats};
use platypus_ui::DrawCmd;

use canvas::{Canvas, Space};
use geometry::Frame;
use kernels::Textures;
use kernels::blur::Blur;
use kernels::composite::Composite;
use kernels::image::Image;

/// Largest blur kernel (taps either side), as the GPU executors.
const MAX_BLUR_RADIUS: f32 = 128.0;

pub struct PlanRenderer {
    glyphs: GlyphCache,
    images: ImageCache,
    textures: Textures,
    canvases: Vec<Canvas>,
    canvas_size: (u32, u32),
    canvas_space: Space,
    blending: Blending,
    linear_blur: bool,
}

/// What planning asks of this renderer: glyphs and images from the shared caches.
struct Resources<'a> {
    glyphs: &'a mut GlyphCache,
    images: &'a mut ImageCache,
}

impl PlanResources for Resources<'_> {
    fn glyph(
        &mut self,
        run: &platypus_ui::GlyphRun,
        glyph: u32,
        size_px: f32,
        subpixel: u8,
        stroke_px: f32,
    ) -> Option<GlyphEntry> {
        self.glyphs.glyph(run, glyph, size_px, subpixel, stroke_px)
    }

    fn image(&mut self, url: &str) -> Option<ImageInfo> {
        self.images
            .image(url)
            .map(|(key, size)| ImageInfo { size, key })
    }

    fn svg_at(&mut self, url: &str, px: (f32, f32)) -> Option<(String, (u32, u32))> {
        self.images.svg_at(url, false, px)
    }
}

impl Default for PlanRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanRenderer {
    pub fn new() -> PlanRenderer {
        PlanRenderer {
            glyphs: GlyphCache::new(),
            images: ImageCache::default(),
            textures: Textures {
                // Zeroed by the allocator: only the parts glyphs are written to become memory.
                glyph_mask: vec![0; (MASK_SIZE * MASK_SIZE) as usize],
                glyph_color: None,
                images: Default::default(),
            },
            canvases: Vec::new(),
            canvas_size: (0, 0),
            canvas_space: Space::Srgb,
            blending: Blending::default(),
            linear_blur: false,
        }
    }

    /// Where `url(...)` images are read from.
    pub fn set_asset_root(&mut self, root: PathBuf) {
        self.images.set_root(root);
    }

    /// How translucent colours combine from the next frame on (docs/rendering.md "Blending").
    pub fn set_blending(&mut self, blending: Blending) {
        self.blending = blending;
    }

    /// Whether blurs average in linear light on sRGB canvases (as `GlesRenderer::set_linear_blur`).
    pub fn set_linear_blur(&mut self, on: bool) {
        self.linear_blur = on;
    }

    /// The natural size of the image at `url`, loading it if needed.
    pub fn image_size(&mut self, url: &str) -> Option<(u32, u32)> {
        self.images.image(url).map(|(_, size)| size)
    }

    /// Which instruction-set build the kernels run with here (for logs).
    pub fn simd(&self) -> &'static str {
        kernels::simd_name()
    }

    /// Threads the kernels draw with (the process's pool).
    pub fn threads(&self) -> usize {
        pool::shared().threads()
    }

    /// Draws `cmds` over `clear` (`size` physical pixels, `scale` per logical pixel); the frame is then
    /// [`pixels`](Self::pixels).
    pub fn render(
        &mut self,
        size: (u32, u32),
        scale: f32,
        clear: Color,
        cmds: &[DrawCmd],
    ) -> RenderStats {
        if size.0 == 0 || size.1 == 0 {
            return RenderStats::default();
        }
        self.glyphs.begin_frame();
        self.images.begin_frame();
        let mut resources = Resources {
            glyphs: &mut self.glyphs,
            images: &mut self.images,
        };
        let frame = plan::plan(
            cmds,
            &PlanParams {
                size,
                scale,
                clear,
                seeded: false,
                blending: self.blending,
            },
            &mut resources,
        );
        self.execute(size, scale, &frame)
    }

    /// The last frame: premultiplied RGBA8, top row first, as a target that doesn't encode sRGB on
    /// write gets it from the GLES executor (sRGB values). `None` before the first frame.
    pub fn pixels(&self) -> Option<&[u8]> {
        self.canvases.first().map(|c| c.px.as_slice())
    }

    /// Bytes the frame-sized canvases hold.
    pub fn canvas_bytes(&self) -> usize {
        self.canvases.iter().map(|c| c.px.len()).sum()
    }

    /// Lets go of the canvases (an idle window); the next frame makes them again. Returns the bytes.
    pub fn release_canvases(&mut self) -> usize {
        let bytes = self.canvas_bytes();
        self.canvases.clear();
        self.canvas_size = (0, 0);
        bytes
    }

    /// Takes the glyph pixels and images the caches made since the last frame.
    fn take_uploads(&mut self) {
        for u in self.glyphs.take_uploads() {
            let (page, side, bytes) = if u.color {
                let page = self
                    .textures
                    .glyph_color
                    .get_or_insert_with(|| vec![0; (COLOR_SIZE * COLOR_SIZE * 4) as usize]);
                (page, COLOR_SIZE as usize, 4)
            } else {
                (&mut self.textures.glyph_mask, MASK_SIZE as usize, 1)
            };
            let w = u.width as usize * bytes;
            for row in 0..u.height as usize {
                let at = ((u.y as usize + row) * side + u.x as usize) * bytes;
                page[at..at + w].copy_from_slice(&u.data[row * w..row * w + w]);
            }
        }
        for key in self.images.take_dropped() {
            self.textures.images.remove(&key);
        }
        for ImageUpload {
            key,
            width,
            height,
            pixels,
        } in self.images.take_uploads()
        {
            self.textures.images.insert(
                key,
                Image {
                    width,
                    height,
                    px: pixels,
                },
            );
        }
    }

    /// (Re)makes `count` canvases of `size`, keeping the ones that fit.
    fn make_canvases(&mut self, count: usize, size: (u32, u32), space: Space) {
        if self.canvas_size != size || self.canvas_space != space {
            self.canvases.clear();
            self.canvas_size = size;
            self.canvas_space = space;
        }
        while self.canvases.len() < count {
            self.canvases.push(Canvas::new(size.0, size.1));
        }
    }

    /// Runs a plan for a target of `size` into the canvases; canvas 0 ends as the frame.
    pub fn execute(&mut self, size: (u32, u32), scale: f32, frame: &FramePlan) -> RenderStats {
        let mut stats = RenderStats {
            layers: frame.layers,
            instances: frame.instances.len() as u32,
            clips: frame.clips.len() as u32,
            ..RenderStats::default()
        };
        let space = match frame.blending {
            Blending::Srgb => Space::Srgb,
            Blending::Linear => Space::Linear,
        };
        self.make_canvases(frame.textures.max(1), size, space);
        self.take_uploads();
        let srgb = frame.blending == Blending::Srgb;
        let tables = Frame {
            scale,
            srgb,
            clips: &frame.clips,
            gradients: &frame.gradients,
        };
        let pool = pool::shared();
        let mut pending: Vec<Option<Color>> = vec![None; self.canvases.len()];
        for step in &frame.steps {
            match step {
                Step::Clear { target, color } => pending[*target] = Some(*color),
                Step::Draw {
                    target,
                    range,
                    image,
                } => {
                    let image = match image {
                        Some((key, pixelated)) => match self.textures.images.get(key) {
                            Some(img) => Some((img, *pixelated)),
                            None => continue,
                        },
                        None => None,
                    };
                    if let Some(c) = pending[*target].take() {
                        self.canvases[*target].clear(c, space);
                        stats.passes += 1;
                    }
                    let step = kernels::DrawStep {
                        frame: &tables,
                        space,
                        textures: &self.textures,
                        instances: &frame.instances[range.start as usize..range.end as usize],
                        image,
                        size,
                    };
                    let target = &mut self.canvases[*target];
                    if let Some(r) = kernels::draw(&step, target.rows(), pool) {
                        target.touch(r);
                    }
                    stats.draw_calls += 1;
                    stats.passes += 1;
                }
                Step::Copy { src, dst } => {
                    settle(&mut self.canvases, &mut pending, *src, space, &mut stats);
                    pending[*dst] = None;
                    if src != dst {
                        let (from, to) = pair(&mut self.canvases, *src, *dst);
                        to.copy_from(from);
                    }
                }
                Step::Blur {
                    tex,
                    temp,
                    area,
                    sigma,
                    contained,
                } => {
                    settle(&mut self.canvases, &mut pending, *tex, space, &mut stats);
                    pending[*temp] = None;
                    if tex == temp || area.w == 0 || area.h == 0 {
                        continue;
                    }
                    let radius = (3.0 * sigma).ceil().min(MAX_BLUR_RADIUS);
                    let (tall, bounds) = plan::blur_reach(area, radius, *contained, size);
                    let blur = Blur {
                        area: *area,
                        tall,
                        bounds: bounds.map(|v| v as u32),
                        kernel: kernels::blur::kernel(*sigma, radius as u32),
                        linear: srgb && self.linear_blur,
                        space,
                    };
                    // GLES clears `temp` first: what is outside the rows it writes reads as empty.
                    self.canvases[*temp].clear(Color::TRANSPARENT, space);
                    let (from, to) = pair(&mut self.canvases, *tex, *temp);
                    let rows = to.rows();
                    bands(pool, tall, |y0, y1| {
                        // SAFETY: each band has its own rows of `temp`; `tex` is only read.
                        unsafe { blur.horizontal(from, rows, y0, y1) }
                    });
                    to.touch(PxRect {
                        x: area.x,
                        w: area.w,
                        ..tall
                    });
                    let (from, to) = pair(&mut self.canvases, *temp, *tex);
                    let rows = to.rows();
                    bands(pool, *area, |y0, y1| {
                        // SAFETY: each band has its own rows of `tex`; `temp` is only read.
                        unsafe { blur.vertical(from, rows, y0, y1) }
                    });
                    to.touch(*area);
                    stats.passes += 2;
                }
                Step::Composite {
                    src,
                    dst,
                    area,
                    opacity,
                    clip,
                    blend,
                    matrix,
                    tint,
                    shift,
                    mask,
                } => {
                    settle(&mut self.canvases, &mut pending, *src, space, &mut stats);
                    if let Some((m, _)) = mask {
                        settle(&mut self.canvases, &mut pending, *m, space, &mut stats);
                    }
                    if let Some((_, backdrop)) = blend {
                        settle(
                            &mut self.canvases,
                            &mut pending,
                            *backdrop,
                            space,
                            &mut stats,
                        );
                    }
                    if let Some(c) = pending[*dst].take() {
                        self.canvases[*dst].clear(c, space);
                    }
                    if area.w == 0 || area.h == 0 {
                        continue;
                    }
                    let reads = [Some(*src), mask.map(|(m, _)| m), blend.map(|(_, b)| b)];
                    if reads.contains(&Some(*dst)) {
                        tracing::warn!(
                            "CPU plan renderer: a composite reads the canvas it writes; skipped"
                        );
                        continue;
                    }
                    let rows = self.canvases[*dst].rows();
                    let canvases = &self.canvases;
                    let c = Composite {
                        frame: &tables,
                        space,
                        src: &canvases[*src],
                        blend: blend.map(|(mode, b)| (mode, &canvases[b])),
                        mask: mask.map(|(m, mode)| (&canvases[m], mode)),
                        area: *area,
                        opacity: *opacity,
                        clip: *clip,
                        matrix: *matrix,
                        tint: *tint,
                        shift: *shift,
                    };
                    bands(pool, *area, |y0, y1| {
                        // SAFETY: each band has its own rows of `dst`, which nothing here reads.
                        unsafe { c.rows(rows, y0, y1) }
                    });
                    self.canvases[*dst].touch(*area);
                    stats.passes += 1;
                }
            }
        }
        // A canvas nothing drew into still shows its clear.
        settle(&mut self.canvases, &mut pending, 0, space, &mut stats);
        if space == Space::Linear {
            finish_linear(&mut self.canvases[0]);
        }
        stats.passes += 1;
        stats
    }
}

/// Applies a clear still pending on `canvas`, before something reads it.
fn settle(
    canvases: &mut [Canvas],
    pending: &mut [Option<Color>],
    canvas: usize,
    space: Space,
    stats: &mut RenderStats,
) {
    if let Some(c) = pending[canvas].take() {
        canvases[canvas].clear(c, space);
        stats.passes += 1;
    }
}

/// Canvas `read` to read and canvas `write` to write (two different ones).
fn pair(canvases: &mut [Canvas], read: usize, write: usize) -> (&Canvas, &mut Canvas) {
    assert_ne!(read, write);
    if read < write {
        let (a, b) = canvases.split_at_mut(write);
        (&a[read], &mut b[0])
    } else {
        let (a, b) = canvases.split_at_mut(read);
        (&b[0], &mut a[write])
    }
}

/// Runs `f(y0, y1)` over `area`'s rows in bands on the pool (one band when the area is small).
fn bands(pool: &pool::Pool, area: PxRect, f: impl Fn(u32, u32) + Sync) {
    const BAND: u32 = 32;
    let (top, bottom) = (area.y, area.y + area.h);
    if u64::from(area.w) * u64::from(area.h) < 16_384 || pool.threads() == 1 {
        f(top, bottom);
        return;
    }
    let count = area.h.div_ceil(BAND) as usize;
    pool.run(count, &|i| {
        let y0 = top + i as u32 * BAND;
        f(y0, (y0 + BAND).min(bottom));
    });
}

/// A linear canvas onto a target that doesn't encode sRGB on write: the straight colour encoded, as
/// the GLES blit's mode 1.
fn finish_linear(c: &mut Canvas) {
    for p in c.px.as_chunks_mut::<4>().0 {
        let v = Space::Linear.load(*p);
        let a = v[3];
        let out = if a > 0.0 {
            [
                color::encode(v[0] / a) * a,
                color::encode(v[1] / a) * a,
                color::encode(v[2] / a) * a,
                a,
            ]
        } else {
            [0.0; 4]
        };
        p.copy_from_slice(&out.map(color::to_byte));
    }
}
