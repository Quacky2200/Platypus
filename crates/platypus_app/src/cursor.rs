// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Custom cursor images (`cursor: url(x.png) 4 4, pointer`): decoding them for the
//! platform, which turns them into OS cursors.

use std::path::Path;

use platypus_platform::CursorRgba;

/// Browsers ignore cursor images larger than this (CSS pixels) and use the next one.
pub const MAX_CURSOR_SIZE: u32 = 128;

/// Reads a cursor image from the game folder or a `data:` URL.
pub fn read(root: &Path, url: &str) -> Option<Vec<u8>> {
    platypus_core::read_asset(root, url)
        .inspect_err(|e| tracing::warn!("cursor image: {e}"))
        .ok()
}

/// Decodes a PNG, JPEG or Windows `.cur` file into a cursor scaled for the window's
/// `scale` factor. The hotspot is `hotspot` (image pixels) if given, else the
/// `.cur` file's own, else the top-left corner. `None` if it can't be decoded or is
/// bigger than [`MAX_CURSOR_SIZE`].
pub fn decode(bytes: &[u8], hotspot: Option<(f32, f32)>, scale: f64) -> Option<CursorRgba> {
    // `.cur` is an `.ico` with type 2 and hotspots where `.ico` keeps colour planes.
    let is_cur = bytes.starts_with(&[0, 0, 2, 0]) && bytes.len() >= 14;
    let image = if is_cur {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Ico)
    } else {
        image::load_from_memory(bytes)
    }
    .ok()?
    .to_rgba8();
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 || w > MAX_CURSOR_SIZE || h > MAX_CURSOR_SIZE {
        return None;
    }
    let file_hotspot = is_cur.then(|| {
        let x = u16::from_le_bytes([bytes[10], bytes[11]]);
        let y = u16::from_le_bytes([bytes[12], bytes[13]]);
        (f32::from(x), f32::from(y))
    });
    let (hx, hy) = hotspot.or(file_hotspot).unwrap_or((0.0, 0.0));
    let scale = if scale.is_finite() && scale > 0.0 {
        scale as f32
    } else {
        1.0
    };
    let (sw, sh) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let image = if (sw, sh) == (w, h) {
        image
    } else {
        image::imageops::resize(&image, sw, sh, image::imageops::FilterType::Triangle)
    };
    let clamp = |v: f32, max: u32| (v * scale).round().clamp(0.0, (max - 1) as f32) as u16;
    Some(CursorRgba {
        rgba: image.into_raw(),
        width: sw as u16,
        height: sh as u16,
        hotspot: (clamp(hx, sw), clamp(hy, sh)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// A one-entry `.cur` file wrapping `png`, with hotspot (x, y).
    fn cur(png: &[u8], w: u8, h: u8, x: u16, y: u16) -> Vec<u8> {
        let mut out = vec![0, 0, 2, 0, 1, 0, w, h, 0, 0];
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&22u32.to_le_bytes());
        out.extend_from_slice(png);
        out
    }

    // M3-CUR-01
    #[test]
    fn png_cursors_use_the_css_hotspot_and_scale_with_the_window() {
        let c = decode(&png(16, 16), Some((4.0, 6.0)), 1.0).unwrap();
        assert_eq!((c.width, c.height, c.hotspot), (16, 16, (4, 6)));
        assert_eq!(c.rgba.len(), 16 * 16 * 4);
        let c = decode(&png(16, 16), Some((4.0, 6.0)), 2.0).unwrap();
        assert_eq!((c.width, c.height, c.hotspot), (32, 32, (8, 12)));
        let c = decode(&png(16, 16), None, 1.0).unwrap();
        assert_eq!(c.hotspot, (0, 0), "top-left by default");
        let c = decode(&png(16, 16), Some((40.0, -3.0)), 1.0).unwrap();
        assert_eq!(c.hotspot, (15, 0), "clamped into the image");
    }

    // M3-CUR-01
    #[test]
    fn cur_files_bring_their_own_hotspot() {
        let file = cur(&png(8, 8), 8, 8, 3, 5);
        let c = decode(&file, None, 1.0).unwrap();
        assert_eq!((c.width, c.hotspot), (8, (3, 5)));
        let c = decode(&file, Some((1.0, 1.0)), 1.0).unwrap();
        assert_eq!(c.hotspot, (1, 1), "the CSS hotspot wins");
    }

    // M3-CUR-01
    #[test]
    fn every_css_cursor_keyword_maps_to_an_os_cursor() {
        for keyword in web_ui_dom::CURSORS {
            if matches!(*keyword, "auto" | "none") {
                continue; // resolved by the tree / hides the cursor
            }
            assert!(
                keyword.parse::<platypus_platform::CursorIcon>().is_ok(),
                "{keyword} has no OS cursor"
            );
        }
    }

    // M3-CUR-01
    #[test]
    fn oversized_or_broken_images_are_skipped() {
        assert!(decode(&png(129, 16), None, 1.0).is_none());
        assert!(decode(b"not an image", None, 1.0).is_none());
    }
}
