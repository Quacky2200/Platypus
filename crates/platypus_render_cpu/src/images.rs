// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `url(...)` images for the CPU renderer. Like a browser's, they are decoded lazily and only at
//! the size they are shown at: the encoded file is all that is kept per image, a decoded copy
//! (downscaled to about its drawn size, or an SVG rasterized at it) is made when it is drawn, and
//! copies not drawn for a while are dropped.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use image::imageops::FilterType;
use resvg::{tiny_skia, usvg};
use vello_cpu::peniko::ImageAlphaType;
use vello_cpu::{PixelMetadata, Pixmap};

/// Draws a decoded copy may go unused for before it is dropped.
const KEEP_DRAWS: u64 = 180;

enum Source {
    /// The encoded file (PNG, JPEG, ...).
    Raster(Arc<Vec<u8>>),
    Svg(Arc<usvg::Tree>),
}

struct Entry {
    source: Source,
    /// Size in CSS pixels.
    natural: (u32, u32),
}

/// A decoded copy at one size, and the draw it was last used in.
struct Level {
    pixmap: Arc<Pixmap>,
    used: u64,
}

#[derive(Default)]
pub struct Images {
    root: Option<PathBuf>,
    entries: HashMap<String, Option<Entry>>,
    /// Decoded copies by `url#WxH`.
    levels: HashMap<String, Level>,
    svg_options: Option<usvg::Options<'static>>,
    /// Draws so far (the clock for [`KEEP_DRAWS`]).
    draw: u64,
}

fn short(url: &str) -> &str {
    match url.char_indices().nth(60) {
        Some((i, _)) => &url[..i],
        None => url,
    }
}

fn to_pixmap(rgba: Vec<u8>, width: u32, height: u32, alpha: ImageAlphaType) -> Option<Pixmap> {
    let (w, h) = (u16::try_from(width).ok()?, u16::try_from(height).ok()?);
    Some(Pixmap::from_parts(
        rgba,
        w,
        h,
        PixelMetadata {
            may_have_transparency: true,
            alpha_type: alpha,
        },
    ))
}

impl Images {
    pub fn set_root(&mut self, root: PathBuf) {
        self.root = Some(root);
        self.entries.clear();
        self.levels.clear();
    }

    /// Call once per drawn frame: drops decoded copies that have not been drawn for a while.
    pub fn begin_draw(&mut self) {
        self.draw += 1;
        if self.draw.is_multiple_of(30) {
            let now = self.draw;
            self.levels.retain(|_, l| now - l.used <= KEEP_DRAWS);
        }
    }

    fn options(&mut self) -> &usvg::Options<'static> {
        self.svg_options.get_or_insert_with(|| {
            let mut options = usvg::Options::default();
            let db = Arc::make_mut(&mut options.fontdb);
            // The built-in files as they are in the binary, not a 1.7 MB copy on the heap.
            for font in [platypus_ui::text::INTER, platypus_ui::text::INTER_ITALIC] {
                db.load_font_source(usvg::fontdb::Source::Binary(Arc::new(font)));
            }
            let family = db
                .faces()
                .next()
                .and_then(|f| f.families.first())
                .map_or_else(
                    || platypus_ui::text::DEFAULT_FAMILY.to_owned(),
                    |f| f.0.clone(),
                );
            db.set_sans_serif_family(family.clone());
            db.set_serif_family(family.clone());
            options.font_family = family;
            options
        })
    }

    fn bytes(&self, url: &str) -> Option<Vec<u8>> {
        if url.trim_start().starts_with("data:") {
            let bytes = platypus_core::decode_data_url(url);
            if bytes.is_none() {
                tracing::warn!("image data URL couldn't be decoded");
            }
            return bytes;
        }
        let Some(root) = &self.root else {
            tracing::warn!("image `{url}` ignored: no asset folder set");
            return None;
        };
        platypus_core::read_asset(root, url)
            .inspect_err(|e| tracing::warn!("image `{url}`: {e}"))
            .ok()
    }

    /// Reads the file and its size (from the header: nothing is decoded yet).
    fn load(&mut self, url: &str) -> Option<Entry> {
        let bytes = self.bytes(url)?;
        if platypus_ui::is_svg(url) {
            let tree = match usvg::Tree::from_data(&bytes, self.options()) {
                Ok(t) => Arc::new(t),
                Err(e) => {
                    tracing::warn!("SVG `{}` couldn't be parsed: {e}", short(url));
                    return None;
                }
            };
            let size = tree.size();
            let natural = (
                (size.width().ceil() as u32).max(1),
                (size.height().ceil() as u32).max(1),
            );
            return Some(Entry {
                source: Source::Svg(tree),
                natural,
            });
        }
        let natural = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.into_dimensions().ok());
        let Some(natural) = natural else {
            tracing::warn!("image `{}` could not be read", short(url));
            return None;
        };
        Some(Entry {
            source: Source::Raster(Arc::new(bytes)),
            natural,
        })
    }

    fn entry(&mut self, url: &str) -> Option<&Entry> {
        if !self.entries.contains_key(url) {
            let loaded = self.load(url);
            self.entries.insert(url.to_owned(), loaded);
        }
        self.entries.get(url)?.as_ref()
    }

    /// The image's natural size in CSS pixels.
    pub fn size(&mut self, url: &str) -> Option<(u32, u32)> {
        self.entry(url).map(|e| e.natural)
    }

    /// The pixels to draw `url` with at `px` physical pixels (a raster is downscaled to about
    /// that size, an SVG rasterized at it), and the image's natural size.
    pub fn get(&mut self, url: &str, px: (f32, f32)) -> Option<(Arc<Pixmap>, (u32, u32))> {
        let (natural, source) = {
            let e = self.entry(url)?;
            let source = match &e.source {
                Source::Raster(b) => Source::Raster(b.clone()),
                Source::Svg(t) => Source::Svg(t.clone()),
            };
            (e.natural, source)
        };
        // Sizes round up to a multiple of 16, so small size changes reuse the copy.
        let round = |v: f32| (v.ceil().clamp(1.0, 8192.0) as u32).div_ceil(16) * 16;
        let size = match source {
            Source::Raster(_) if px.0 > 0.0 && round(px.0) < natural.0 => {
                let w = round(px.0);
                let h = (w as f32 * natural.1 as f32 / natural.0 as f32)
                    .round()
                    .max(1.0) as u32;
                (w, h)
            }
            Source::Raster(_) => natural,
            Source::Svg(_) => (round(px.0), round(px.1)),
        };
        let key = format!("{url}#{}x{}", size.0, size.1);
        let draw = self.draw;
        if let Some(level) = self.levels.get_mut(&key) {
            level.used = draw;
            return Some((level.pixmap.clone(), natural));
        }
        let pixmap = Arc::new(match source {
            Source::Raster(bytes) => decode(&bytes, size, natural, url)?,
            Source::Svg(tree) => rasterize(&tree, size)?,
        });
        self.levels.insert(
            key,
            Level {
                pixmap: pixmap.clone(),
                used: draw,
            },
        );
        Some((pixmap, natural))
    }
}

/// Decodes a raster, downscaled to `size` when that is smaller than its natural size.
fn decode(bytes: &[u8], size: (u32, u32), natural: (u32, u32), url: &str) -> Option<Pixmap> {
    let image = match image::load_from_memory(bytes) {
        Ok(i) => i.to_rgba8(),
        Err(e) => {
            tracing::warn!("image `{}` could not be loaded: {e}", short(url));
            return None;
        }
    };
    let image = if size == natural || size.0 >= image.width() {
        image
    } else {
        image::imageops::resize(&image, size.0, size.1, FilterType::Triangle)
    };
    let (w, h) = image.dimensions();
    to_pixmap(image.into_raw(), w, h, ImageAlphaType::Alpha)
}

fn rasterize(tree: &usvg::Tree, (w, h): (u32, u32)) -> Option<Pixmap> {
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let size = tree.size();
    let transform = tiny_skia::Transform::from_scale(
        w as f32 / size.width().max(f32::EPSILON),
        h as f32 / size.height().max(f32::EPSILON),
    );
    resvg::render(tree, transform, &mut pixmap.as_mut());
    // tiny-skia's pixels are premultiplied RGBA8.
    let (w, h) = (pixmap.width(), pixmap.height());
    to_pixmap(pixmap.take(), w, h, ImageAlphaType::AlphaPremultiplied)
}
