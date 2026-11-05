// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Golden-image helpers for render tests (see TESTS.md "Conventions").
//!
//! References live in `tests/golden/<id>.png` at the workspace root. Set
//! `PLATYPUS_UPDATE_GOLDEN=1` to write new references instead of comparing. Every
//! render is also saved to `tests/output/<id>.png` (git-ignored) for inspection.

use std::path::{Path, PathBuf};

pub fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

/// Where test renders are written, passing or not (`tests/output`, git-ignored).
pub fn output_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/output")
}

/// Compares RGBA8 `pixels` with the reference image `id`, allowing each channel to
/// differ by up to `tolerance`. Panics with a readable message on mismatch.
pub fn assert_golden(id: &str, width: u32, height: u32, pixels: &[u8], tolerance: u8) {
    let path = golden_dir().join(format!("{id}.png"));
    // Best effort: a failed write shouldn't fail the comparison.
    let _ = std::fs::create_dir_all(output_dir()).and_then(|_| {
        write_png(
            &output_dir().join(format!("{id}.png")),
            width,
            height,
            pixels,
        )
    });

    if std::env::var_os("PLATYPUS_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden_dir()).unwrap();
        write_png(&path, width, height, pixels).unwrap();
        eprintln!("updated golden image {}", path.display());
        return;
    }

    let file = std::fs::File::open(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden image {}; run with PLATYPUS_UPDATE_GOLDEN=1 to create it",
            path.display()
        )
    });
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap();
    let mut expected = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut expected).unwrap();
    expected.truncate(info.buffer_size());

    assert_eq!(
        (info.width, info.height),
        (width, height),
        "golden image {id} has a different size"
    );
    assert_eq!(
        info.color_type,
        png::ColorType::Rgba,
        "golden image {id} must be RGBA"
    );

    let mismatches = pixels
        .chunks(4)
        .zip(expected.chunks(4))
        .enumerate()
        .filter(|(_, (a, b))| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > tolerance))
        .collect::<Vec<_>>();
    if let Some((i, (got, want))) = mismatches.first() {
        panic!(
            "golden image {id}: {} pixel(s) differ; first at ({}, {}): got {got:?}, expected {want:?}",
            mismatches.len(),
            *i as u32 % width,
            *i as u32 / width,
        );
    }
}

/// Writes RGBA8 `pixels` as a PNG (used for goldens and `platypus-app --screenshot`).
pub fn write_png(path: &Path, width: u32, height: u32, pixels: &[u8]) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(pixels))
        .map_err(std::io::Error::other)
}

/// One GPU shared by every test in a test binary, locked for the duration of a test.
///
/// Creating several devices at once from parallel test threads can deadlock in some
/// Vulkan drivers and layers (seen on Windows with an OBS capture layer installed),
/// and a single device is faster anyway.
pub fn shared_gpu() -> std::sync::MutexGuard<'static, crate::Gpu> {
    static GPU: std::sync::OnceLock<std::sync::Mutex<crate::Gpu>> = std::sync::OnceLock::new();
    GPU.get_or_init(|| {
        std::sync::Mutex::new(crate::Gpu::headless().expect("a hardware or software GPU adapter"))
    })
    .lock()
    // A failed test can poison the lock; the GPU itself is still usable.
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}
