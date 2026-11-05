// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Images for the UI renderers, without a GPU: `url(...)` files from the game's folder decoded,
//! SVG images (and inline `<svg>` elements) rasterized with resvg at the size they're drawn so
//! they stay sharp, all cached by key. Loading queues the pixels as an [`ImageUpload`]; whichever
//! renderer owns the textures writes them (`images.rs` for wgpu), and drops the ones this cache
//! lets go of ([`ImageCache::take_dropped`]). The same split as the glyph cache.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use resvg::{tiny_skia, usvg};

use platypus_ui::is_svg;

/// An image's pixels for a renderer's texture: straight-alpha RGBA8, sRGB-encoded.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageUpload {
    /// The texture key (what a `Step::Draw` names).
    pub key: String,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// What a loaded image is: the size layout uses (an SVG's raster can be bigger).
#[derive(Clone, Copy, Debug)]
struct Entry {
    size: (u32, u32),
}

/// Images by URL. Failed loads are remembered so they're reported once.
#[derive(Default)]
pub struct ImageCache {
    root: Option<PathBuf>,
    entries: HashMap<String, Option<Entry>>,
    /// Parsed SVG documents, by URL (or by content for inline SVG).
    svgs: HashMap<String, Option<Arc<usvg::Tree>>>,
    /// usvg's options, with the bundled font for `<text>` (made on first use).
    svg_options: Option<usvg::Options<'static>>,
    /// Frames drawn, and when each `data:` SVG entry was last used: inline `<svg>`
    /// elements become a new data URL whenever they change (an animation makes one per
    /// frame), so unused ones are dropped.
    frame: u64,
    used: HashMap<String, u64>,
    uploads: Vec<ImageUpload>,
    dropped: Vec<String>,
}

/// `data:` SVG entries unused for this many frames are dropped.
const SVG_DATA_TTL: u64 = 120;

/// Rounds a raster size up to a multiple of 8 (within 8..=4096), so small size
/// changes reuse the same raster.
fn bucket(px: f32) -> u32 {
    let px = px.ceil().clamp(1.0, 4096.0) as u32;
    px.div_ceil(8) * 8
}

impl ImageCache {
    pub fn set_root(&mut self, root: PathBuf) {
        self.root = Some(root);
        self.dropped
            .extend(self.entries.drain().filter_map(|(k, e)| e.map(|_| k)));
        self.svgs.clear();
        self.used.clear();
    }

    /// Call once per frame: drops `data:` SVGs (and their rasters) that weren't drawn
    /// for a while.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        if !self.frame.is_multiple_of(30) {
            return;
        }
        let cutoff = self.frame.saturating_sub(SVG_DATA_TTL);
        let stale: Vec<String> = self
            .used
            .iter()
            .filter(|(_, at)| **at < cutoff)
            .map(|(k, _)| k.clone())
            .collect();
        for key in stale {
            self.used.remove(&key);
            if let Some(Some(_)) = self.entries.remove(&key) {
                self.dropped.push(key.clone());
            }
            self.svgs.remove(&key);
        }
    }

    /// The cache key for `url`: `data:` SVGs (often long, and new whenever an inline
    /// `<svg>` changes) go by a hash, and are tracked for eviction.
    fn key_for(&mut self, url: &str) -> String {
        if url.starts_with("data:") && is_svg(url) {
            let key = format!("svg-data:{:016x}", hash(url));
            self.used.insert(key.clone(), self.frame);
            key
        } else {
            url.to_owned()
        }
    }

    /// An SVG drawn at `px` physical pixels: rasterized at about that size (cached)
    /// and keyed for the renderer's textures. `source` is a URL, or SVG text for `inline`.
    /// Returns the key and the image's natural size.
    pub fn svg_at(
        &mut self,
        source: &str,
        inline: bool,
        px: (f32, f32),
    ) -> Option<(String, (u32, u32))> {
        let id = if inline {
            format!("svg-inline:{:016x}", hash(source))
        } else {
            self.key_for(source)
        };
        let tree = self.svg_tree(&id, source, inline)?;
        let natural = svg_natural(&tree);
        let (w, h) = (bucket(px.0), bucket(px.1));
        let key = format!("{id}#{w}x{h}");
        if self.used.contains_key(&id) {
            self.used.insert(key.clone(), self.frame);
        }
        if !self.entries.contains_key(&key) {
            let loaded = rasterize(&tree, (w, h)).map(|pixels| {
                self.uploads.push(ImageUpload {
                    key: key.clone(),
                    width: w,
                    height: h,
                    pixels,
                });
                Entry { size: natural }
            });
            self.entries.insert(key.clone(), loaded);
        }
        self.entries.get(&key)?.as_ref()?;
        Some((key, natural))
    }

    /// The parsed SVG for `id` (a URL, or `svg-inline:<hash>` for `text`).
    fn svg_tree(&mut self, id: &str, text: &str, inline: bool) -> Option<Arc<usvg::Tree>> {
        if let Some(tree) = self.svgs.get(id) {
            return tree.clone();
        }
        let options = self.svg_options.get_or_insert_with(|| {
            let mut options = usvg::Options::default();
            let db = Arc::make_mut(&mut options.fontdb);
            // The built-in files as they are in the binary, not a 1.7 MB copy on the heap.
            for font in [platypus_ui::text::INTER, platypus_ui::text::INTER_ITALIC] {
                db.load_font_source(usvg::fontdb::Source::Binary(Arc::new(font)));
            }
            // The file's own family name ("Inter Variable"), whatever Platypus calls it.
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
        });
        let data = if inline {
            Some(text.as_bytes().to_vec())
        } else if text.trim_start().starts_with("data:") {
            platypus_core::decode_data_url(text)
        } else {
            self.root.as_ref().and_then(|root| {
                platypus_core::read_asset(root, text)
                    .inspect_err(|e| tracing::warn!("{e}"))
                    .ok()
            })
        };
        let tree = match data.map(|d| usvg::Tree::from_data(&d, options)) {
            Some(Ok(tree)) => Some(Arc::new(tree)),
            Some(Err(e)) => {
                tracing::warn!("SVG `{}` couldn't be parsed: {e}", short(id));
                None
            }
            None => {
                tracing::warn!("SVG image `{}` couldn't be loaded", short(id));
                None
            }
        };
        self.svgs.insert(id.to_owned(), tree.clone());
        tree
    }

    /// Loads the image at `url` if it isn't yet. Returns its texture key and natural size
    /// (`None` if it can't be loaded; the failure is remembered).
    pub fn image(&mut self, url: &str) -> Option<(String, (u32, u32))> {
        let key = self.key_for(url);
        if !self.entries.contains_key(&key) {
            let loaded = self.load(url, &key);
            self.entries.insert(key.clone(), loaded);
        }
        let size = self.entries.get(&key)?.as_ref()?.size;
        Some((key, size))
    }

    /// The pixels queued since the last call, for the renderer's textures.
    pub fn take_uploads(&mut self) -> Vec<ImageUpload> {
        std::mem::take(&mut self.uploads)
    }

    /// The texture keys this cache dropped since the last call (evicted `data:` SVGs, or
    /// everything when the asset folder changed).
    pub fn take_dropped(&mut self) -> Vec<String> {
        std::mem::take(&mut self.dropped)
    }

    fn load(&mut self, url: &str, key: &str) -> Option<Entry> {
        if is_svg(url) {
            // At its natural size; draws ask for a sharper copy with `svg_at`.
            let id = self.key_for(url);
            let tree = self.svg_tree(&id, url, false)?;
            let natural = svg_natural(&tree);
            let pixels = rasterize(&tree, natural)?;
            self.uploads.push(ImageUpload {
                key: key.to_owned(),
                width: natural.0,
                height: natural.1,
                pixels,
            });
            return Some(Entry { size: natural });
        }
        let decoded = if url.trim_start().starts_with("data:") {
            // Inlined by the bundler (small images referenced from CSS).
            let Some(bytes) = platypus_core::decode_data_url(url) else {
                tracing::warn!("background image data URL couldn't be decoded");
                return None;
            };
            image::load_from_memory(&bytes)
        } else {
            let Some(root) = &self.root else {
                tracing::warn!("background image `{url}` ignored: no asset folder set");
                return None;
            };
            let bytes = match platypus_core::read_asset(root, url) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("background image `{url}`: {e}");
                    return None;
                }
            };
            image::load_from_memory(&bytes)
        };
        let image = match decoded {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                tracing::warn!("background image `{}` could not be loaded: {e}", short(url));
                return None;
            }
        };
        let (w, h) = image.dimensions();
        tracing::debug!("loaded background image `{}` ({w}x{h})", short(url));
        self.uploads.push(ImageUpload {
            key: key.to_owned(),
            width: w,
            height: h,
            pixels: image.into_raw(),
        });
        Some(Entry { size: (w, h) })
    }
}

/// An SVG's natural size in CSS pixels (its `width`/`height`, else its `viewBox`).
fn svg_natural(tree: &usvg::Tree) -> (u32, u32) {
    let size = tree.size();
    (
        (size.width().ceil() as u32).max(1),
        (size.height().ceil() as u32).max(1),
    )
}

/// Renders `tree` stretched to `w` × `h` pixels, as straight-alpha RGBA.
fn rasterize(tree: &usvg::Tree, (w, h): (u32, u32)) -> Option<Vec<u8>> {
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let size = tree.size();
    let transform =
        tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(tree, transform, &mut pixmap.as_mut());
    // tiny-skia works premultiplied; textures here hold straight alpha.
    let mut pixels = pixmap.take();
    for p in pixels.as_chunks_mut::<4>().0 {
        let a = p[3];
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
            }
        }
    }
    Some(pixels)
}

/// A stable hash of inline SVG text (FNV-1a), for cache keys.
fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A URL shortened for log messages (data URLs can be huge).
fn short(url: &str) -> &str {
    match url.char_indices().nth(60) {
        Some((i, _)) => &url[..i],
        None => url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct MapFs(HashMap<String, Vec<u8>>);

    impl platypus_core::AssetFs for MapFs {
        fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
            self.0.get(path).cloned().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("asset \"{path}\" not found in assets.dat"),
                )
            })
        }
        fn open_range(&self, _: &str) -> std::io::Result<Arc<dyn platypus_core::RangeRead>> {
            Err(std::io::ErrorKind::Unsupported.into())
        }
        fn exists(&self, path: &str) -> bool {
            self.0.contains_key(path)
        }
        fn list(&self) -> Vec<String> {
            self.0.keys().cloned().collect()
        }
        fn pack_of(&self, _: &str) -> String {
            "assets".into()
        }
    }

    // M9-API-1149
    #[test]
    fn svg_images_load_from_the_asset_file_system_not_the_folder() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="6"><rect width="8" height="6"/></svg>"#;
        let fs = MapFs(HashMap::from([("assets/a.svg".to_owned(), svg.to_vec())]));
        let mut cache = ImageCache::default();
        cache.set_root(PathBuf::from("no-such-folder"));
        platypus_core::with_asset_fs(Arc::new(fs), || {
            let tree = cache
                .svg_tree("assets/a.svg", "assets/a.svg", false)
                .unwrap();
            assert_eq!(svg_natural(&tree), (8, 6));
            assert!(
                cache
                    .svg_tree("assets/b.svg", "assets/b.svg", false)
                    .is_none()
            );
        });
        // Without a file system the (missing) folder is asked, as before.
        let mut cache = ImageCache::default();
        cache.set_root(PathBuf::from("no-such-folder"));
        assert!(
            cache
                .svg_tree("assets/a.svg", "assets/a.svg", false)
                .is_none()
        );
    }
}
