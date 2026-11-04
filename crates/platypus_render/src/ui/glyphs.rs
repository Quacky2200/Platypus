// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The GPU-free half of the glyph atlas: glyphs rasterized with swash at physical pixel size,
//! packed into two pages (an R8 coverage page and an RGBA page for colour emoji) and cached by
//! font, glyph, size, subpixel offset and variation. Placing a glyph queues its pixels as a
//! [`GlyphUpload`]; whichever renderer owns the page textures writes them (`atlas.rs` for wgpu).

use std::collections::HashMap;

use etagere::{BucketedAtlasAllocator, size2};
use platypus_ui::GlyphRun;
use swash::scale::image::{Content, Image};
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Placement, Stroke, Transform, Vector};

/// Horizontal subpixel positions per pixel (glyphs are rasterized at 0, ¼, ½, ¾).
pub const SUBPIXEL_BINS: u8 = 4;

/// Side of the coverage page, in pixels.
pub const MASK_SIZE: u32 = 2048;
/// Side of the colour page, in pixels.
pub const COLOR_SIZE: u32 = 1024;
/// Empty space around each glyph so filtering never reads a neighbour.
const PADDING: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: u64,
    index: u32,
    glyph: u32,
    /// Size in physical pixels × 4.
    size: u32,
    subpixel: u8,
    coords: u64,
    embolden: bool,
    skew: u32,
    /// Outline stroke width in physical pixels × 4; 0 for the filled glyph.
    stroke: u32,
}

/// Where a rasterized glyph lives in the atlas and how to place it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphEntry {
    /// In the colour page (emoji) rather than the coverage page.
    pub color: bool,
    /// Top-left of the glyph image in its page, in pixels.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Offset from the pen position to the image's left edge, in pixels.
    pub left: i32,
    /// Distance from the baseline up to the image's top edge, in pixels.
    pub top: i32,
}

/// Pixels of a newly placed glyph, to write into its page at (`x`, `y`): one byte per pixel in
/// the coverage page, four (RGBA) in the colour page.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphUpload {
    pub color: bool,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub struct GlyphCache {
    mask: BucketedAtlasAllocator,
    color: BucketedAtlasAllocator,
    cache: HashMap<GlyphKey, Option<GlyphEntry>>,
    scale: ScaleContext,
    /// Set when a page fills up; the atlas is cleared at the start of the next frame.
    full: bool,
    uploads: Vec<GlyphUpload>,
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphCache {
    pub fn new() -> Self {
        Self {
            mask: BucketedAtlasAllocator::new(size2(MASK_SIZE as i32, MASK_SIZE as i32)),
            color: BucketedAtlasAllocator::new(size2(COLOR_SIZE as i32, COLOR_SIZE as i32)),
            cache: HashMap::new(),
            scale: ScaleContext::new(),
            full: false,
            uploads: Vec::new(),
        }
    }

    /// Call once per frame before rasterizing: clears the atlas if it filled up
    /// last frame (everything visible is simply rasterized again).
    pub fn begin_frame(&mut self) {
        if self.full {
            tracing::debug!("glyph atlas full; clearing");
            self.mask.clear();
            self.color.clear();
            self.cache.clear();
            self.uploads.clear();
            self.full = false;
        }
    }

    /// The pixels placed since the last call, for the page textures.
    pub fn take_uploads(&mut self) -> Vec<GlyphUpload> {
        std::mem::take(&mut self.uploads)
    }

    /// The atlas entry for one glyph of `run` at `size_px` physical pixels with a
    /// subpixel offset of `subpixel / SUBPIXEL_BINS` pixels. `None` for glyphs
    /// with no image (spaces) or when the atlas is full.
    pub fn glyph(
        &mut self,
        run: &GlyphRun,
        glyph: u32,
        size_px: f32,
        subpixel: u8,
        stroke_px: f32,
    ) -> Option<GlyphEntry> {
        let key = GlyphKey {
            font: run.font.data.id(),
            index: run.font.index,
            glyph,
            size: (size_px * 4.0).round() as u32,
            subpixel,
            coords: hash_coords(&run.normalized_coords),
            embolden: run.embolden,
            skew: run.skew.unwrap_or(0.0).to_bits(),
            stroke: (stroke_px * 4.0).round() as u32,
        };
        if let Some(entry) = self.cache.get(&key) {
            return *entry;
        }
        let entry = self.rasterize(run, glyph, size_px, subpixel, stroke_px);
        if entry.is_some() || !self.full {
            // Don't remember failures caused by a full atlas.
            self.cache.insert(key, entry);
        }
        entry
    }

    fn rasterize(
        &mut self,
        run: &GlyphRun,
        glyph: u32,
        size_px: f32,
        subpixel: u8,
        stroke_px: f32,
    ) -> Option<GlyphEntry> {
        // Glyph 0 is the font's .notdef (a black box with "NO GLYPH" sideways); draw it
        // as red stacked letters instead so a missing character is easy to spot.
        let image = if glyph == 0 && stroke_px <= 0.0 {
            missing_glyph_image(size_px)
        } else {
            self.render_font_glyph(run, glyph, size_px, subpixel, stroke_px)?
        };
        let (w, h) = (image.placement.width, image.placement.height);
        if w == 0 || h == 0 {
            return None;
        }
        let color = image.content == Content::Color;
        let allocator = if color {
            &mut self.color
        } else {
            &mut self.mask
        };
        let Some(alloc) = allocator.allocate(size2(w as i32 + 2 * PADDING, h as i32 + 2 * PADDING))
        else {
            self.full = true;
            return None;
        };
        let x = (alloc.rectangle.min.x + PADDING) as u32;
        let y = (alloc.rectangle.min.y + PADDING) as u32;
        self.uploads.push(GlyphUpload {
            color,
            x,
            y,
            width: w,
            height: h,
            data: image.data,
        });
        Some(GlyphEntry {
            color,
            x,
            y,
            width: w,
            height: h,
            left: image.placement.left,
            top: image.placement.top,
        })
    }

    fn render_font_glyph(
        &mut self,
        run: &GlyphRun,
        glyph: u32,
        size_px: f32,
        subpixel: u8,
        stroke_px: f32,
    ) -> Option<Image> {
        let font = swash::FontRef::from_index(run.font.data.data(), run.font.index as usize)?;
        let mut scaler = self
            .scale
            .builder(font)
            .size(size_px)
            .hint(false)
            .normalized_coords(run.normalized_coords.iter().copied())
            .build();
        // Strokes outline the glyph's shape; colour glyphs (emoji) aren't stroked.
        let sources: &[Source] = if stroke_px > 0.0 {
            &[Source::Outline]
        } else {
            &[
                Source::ColorOutline(0),
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::Outline,
            ]
        };
        let mut render = Render::new(sources);
        if stroke_px > 0.0 {
            render.style(Stroke::new(stroke_px));
        }
        render.format(Format::Alpha).offset(Vector::new(
            f32::from(subpixel) / f32::from(SUBPIXEL_BINS),
            0.0,
        ));
        if run.embolden {
            render.embolden(size_px / 40.0);
        }
        if let Some(degrees) = run.skew {
            render.transform(Some(Transform::skew(
                Angle::from_degrees(degrees),
                Angle::ZERO,
            )));
        }
        render.render(&mut scaler, glyph.try_into().ok()?)
    }
}

const MISSING_RED: [u8; 3] = [220, 38, 38];

/// What a character the font has no glyph for looks like: the font's own "NO GLYPH"
/// letters (its `.notdef` draws them sideways in a tall box) stood up and stacked one per
/// row like a hotel sign, white on a snug red rectangle. Cached per size by `glyph`.
fn missing_glyph_image(size_px: f32) -> Image {
    stacked_notdef(size_px).unwrap_or_else(|| {
        // Not expected: the bundled font always has the glyph. A plain red box.
        let (w, h) = (
            (size_px * 0.25).round().max(2.0) as u32,
            (size_px * 1.1).round().max(2.0) as u32,
        );
        notdef_image(size_px, w, h, vec![MISSING_RED; (w * h) as usize])
    })
}

fn notdef_image(size_px: f32, w: u32, h: u32, rgb: Vec<[u8; 3]>) -> Image {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for px in &rgb {
        data.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    let mut image = Image::new();
    image.content = Content::Color;
    image.placement = Placement {
        left: (size_px * 0.1).round() as i32,
        top: h as i32,
        width: w,
        height: h,
    };
    image.data = data;
    image
}

/// Cuts the letters out of the font's `.notdef` (white holes in a black box), stands them
/// up and stacks them. `None` when the glyph isn't the expected seven letters.
fn stacked_notdef(size_px: f32) -> Option<Image> {
    let font = swash::FontRef::from_index(platypus_ui::text::INTER, 0)?;
    let mut ctx = ScaleContext::new();
    // Rendered large so the letters can be shrunk by averaging.
    let mut scaler = ctx
        .builder(font)
        .size((size_px * 2.0).max(96.0))
        .hint(false)
        .build();
    let src = Render::new(&[Source::Outline])
        .format(Format::Alpha)
        .transform(Some(Transform::rotation(Angle::from_degrees(-90.0))))
        .render(&mut scaler, 0)?;
    let (sw, sh) = (src.placement.width as usize, src.placement.height as usize);
    // 1 inside a letter (a hole in the box), 0 on the box.
    let mut mask: Vec<f32> = src
        .data
        .iter()
        .map(|&a| 1.0 - f32::from(a) / 255.0)
        .collect();
    // Letters are the connected blobs of holes.
    let mut label = vec![0usize; sw * sh];
    let mut letters: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
    for start in 0..sw * sh {
        if label[start] != 0 || mask[start] <= 0.5 {
            continue;
        }
        let id = letters.len() + 1;
        let (mut x0, mut x1, mut y0, mut y1, mut area) = (sw, 0, sh, 0, 0);
        let mut stack = vec![start];
        label[start] = id;
        while let Some(at) = stack.pop() {
            let (x, y) = (at % sw, at / sw);
            (x0, x1, y0, y1, area) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y), area + 1);
            let neighbours = [
                (x > 0).then(|| at - 1),
                (x + 1 < sw).then(|| at + 1),
                (y > 0).then(|| at - sw),
                (y + 1 < sh).then(|| at + sw),
            ];
            for n in neighbours.into_iter().flatten() {
                if label[n] == 0 && mask[n] > 0.5 {
                    label[n] = id;
                    stack.push(n);
                }
            }
        }
        // Not the empty space around the box, nor a speck.
        let outside = x0 == 0 || y0 == 0 || x1 + 1 == sw || y1 + 1 == sh;
        if !outside && area >= sh * sh / 200 {
            letters.push((x0, x1, y0, y1, id));
        } else {
            // Forget the speck so it can't count as a letter.
            letters.push((0, 0, 0, 0, 0));
        }
    }
    letters.retain(|l| l.4 != 0);
    letters.sort_by_key(|l| l.0);
    if letters.len() != 7 {
        return None;
    }
    redraw_y(&mut mask, &mut label, sw, letters[4]);
    let cap_src = letters.iter().map(|l| l.3 - l.2 + 1).max()? as f32;
    // Letter height so the stack is a little taller than the original glyph (1.11 em).
    let cap = size_px * 0.12;
    let f = cap / cap_src;
    let pad = (cap * 0.35).max(1.0);
    let pitch = cap * 1.3;
    // Space for the word break after the second letter.
    let top_of = |k: usize| pad + k as f32 * pitch + if k >= 2 { pitch * 0.5 } else { 0.0 };
    let dst: Vec<(usize, usize)> = letters
        .iter()
        .map(|l| {
            (
                (((l.1 - l.0 + 1) as f32 * f).round() as usize).max(1),
                (((l.3 - l.2 + 1) as f32 * f).round() as usize).max(1),
            )
        })
        .collect();
    let widest = dst.iter().map(|d| d.0).max()?;
    // At least as wide as the original glyph (0.34 em).
    let w = (widest as f32 + 2.0 * pad).max(size_px * 0.34).round() as u32;
    let h = (top_of(6) + cap + pad).round() as u32;
    let mut rgb = vec![MISSING_RED; (w * h) as usize];
    for (k, (&(x0, _, y0, _, id), &(dw, dh))) in letters.iter().zip(&dst).enumerate() {
        let ox = (w as usize - dw) / 2;
        let oy = (top_of(k) + (cap - dh as f32) / 2.0).round() as usize;
        for j in 0..dh {
            for i in 0..dw {
                // Average the source pixels this destination pixel covers.
                let sx0 = x0 + (i as f32 / f) as usize;
                let sx1 = (x0 + ((i + 1) as f32 / f).ceil() as usize).min(sw);
                let sy0 = y0 + (j as f32 / f) as usize;
                let sy1 = (y0 + ((j + 1) as f32 / f).ceil() as usize).min(sh);
                let (mut sum, mut n) = (0.0, 0.0);
                for y in sy0..sy1 {
                    for x in sx0..sx1 {
                        if label[y * sw + x] == id || label[y * sw + x] == 0 {
                            sum += mask[y * sw + x];
                        }
                        n += 1.0;
                    }
                }
                let cover = if n > 0.0 { sum / n } else { 0.0 };
                let (px, py) = (ox + i, oy + j);
                if px >= w as usize || py >= h as usize {
                    continue;
                }
                let at = py * w as usize + px;
                for (c, red) in rgb[at].iter_mut().zip(MISSING_RED) {
                    *c = (f32::from(red) + (255.0 - f32::from(red)) * cover) as u8;
                }
            }
        }
    }
    Some(notdef_image(size_px, w, h, rgb))
}

/// The font draws its Y as a "4" (a thin stem down its right edge). Gives it the arms of
/// equal weight and a tail down the middle. `letter` is (x0, x1, y0, y1, label).
fn redraw_y(
    mask: &mut [f32],
    label: &mut [usize],
    sw: usize,
    letter: (usize, usize, usize, usize, usize),
) {
    let (x0, x1, y0, y1, id) = letter;
    let unit = (y1 - y0 + 1) as f32 / 5.0;
    let at = |u: f32| y0 + (u * unit).round() as usize;
    let mut fill =
        |xs: std::ops::RangeInclusive<usize>, ys: std::ops::RangeInclusive<usize>, on: bool| {
            for y in ys {
                for x in xs.clone() {
                    // Never touch a neighbour: the L's foot runs under the Y's columns.
                    let owner = label[y * sw + x];
                    if owner != 0 && owner != id {
                        continue;
                    }
                    mask[y * sw + x] = if on { 1.0 } else { 0.0 };
                    label[y * sw + x] = if on { id } else { 0 };
                }
            }
        };
    let arm = unit.round() as usize;
    // Right arm as wide as the left one, the old right-hand stem cleared.
    fill(x1 + 1 - arm..=x1, y0..=at(3.0) - 1, true);
    fill(x0..=x1, at(3.0)..=y1, false);
    // The tail, centred, from the join bar down.
    let tail = (unit * 1.4).round() as usize;
    let tx = x0 + (x1 - x0 + 1 - tail) / 2;
    fill(tx..=tx + tail - 1, at(2.0)..=y1, true);
}

fn hash_coords(coords: &[i16]) -> u64 {
    // FNV-1a: tiny and good enough for a handful of variation coordinates.
    coords.iter().fold(0xcbf2_9ce4_8422_2325, |h, c| {
        (h ^ u64::from(*c as u16)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // M2-PLN-08
    #[test]
    fn missing_glyph_is_stacked_letters_at_least_the_notdef_size() {
        for size in [32.0f32, 64.0, 128.0] {
            assert!(
                stacked_notdef(size).is_some(),
                "all seven letters are found at {size}"
            );
            let p = missing_glyph_image(size).placement;
            // Inter's .notdef is 0.34 em wide and 1.11 em tall.
            assert!(p.width as f32 >= size * 0.33, "{size}: wide enough");
            assert!(p.height as f32 >= size * 1.1, "{size}: tall enough");
        }
    }

    // M2-PLN-08
    #[test]
    fn redrawing_the_y_keeps_the_neighbouring_letter_and_adds_a_tail() {
        let (sw, sh) = (30, 15);
        let mut mask = vec![0.0f32; sw * sh];
        let mut label = vec![0usize; sw * sh];
        // An L (label 1) whose foot runs under the Y's columns, and the Y (label 2, 10 wide,
        // a thin stem down its right edge like the font's own).
        let mut put = |x: usize, y: usize, id: usize| {
            mask[y * sw + x] = 1.0;
            label[y * sw + x] = id;
        };
        for y in 0..15 {
            for x in 0..3 {
                put(x, y, 1);
            }
        }
        for y in 12..15 {
            for x in 0..15 {
                put(x, y, 1);
            }
        }
        for y in 0..9 {
            for x in 10..13 {
                put(x, y, 2);
            }
            put(19, y, 2);
        }
        for y in 9..15 {
            put(18, y, 2);
            put(19, y, 2);
        }
        let l_before: Vec<usize> = (0..sw * sh).filter(|&i| label[i] == 1).collect();
        redraw_y(&mut mask, &mut label, sw, (10, 19, 0, 14, 2));
        let l_after: Vec<usize> = (0..sw * sh).filter(|&i| label[i] == 1).collect();
        assert_eq!(l_before, l_after, "the L keeps every pixel, foot included");
        assert!(l_after.iter().all(|&i| mask[i] == 1.0));
        // The tail is in the middle of the Y, down to its last row; its old right stem is gone.
        assert_eq!(label[11 * sw + 15], 2);
        assert_eq!(label[11 * sw + 19], 0);
    }
}
