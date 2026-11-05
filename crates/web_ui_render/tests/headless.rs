// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use web_ui_render::testing::{assert_golden, shared_gpu};
use web_ui_render::{Color, Gpu, OffscreenTarget};

fn gpu() -> std::sync::MutexGuard<'static, Gpu> {
    shared_gpu()
}

// M1-RND-01
#[test]
fn creates_a_device() {
    let gpu = gpu();
    assert!(!gpu.adapter_name().is_empty());
}

// M1-RND-02
#[test]
fn clear_colour_reads_back_exactly() {
    let gpu = gpu();
    let target = OffscreenTarget::new(&gpu, 64, 64);
    // 0.2/0.4/0.6 map exactly to 51/102/153 in an 8-bit unorm target.
    target.clear(&gpu, Color::rgb(0.2, 0.4, 0.6));
    let pixels = target.read_pixels(&gpu).unwrap();

    assert_eq!(pixels.len(), 64 * 64 * 4);
    assert!(pixels.chunks(4).all(|p| p == [51, 102, 153, 255]));
    assert_golden("M1-RND-02", 64, 64, &pixels, 0);
}

// M1-RND-02 (row padding: 50 px * 4 bytes isn't a multiple of 256)
#[test]
fn readback_handles_unaligned_widths() {
    let gpu = gpu();
    let target = OffscreenTarget::new(&gpu, 50, 3);
    target.clear(&gpu, Color::rgb(1.0, 0.0, 0.0));
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_eq!(pixels.len(), 50 * 3 * 4);
    assert!(pixels.chunks(4).all(|p| p == [255, 0, 0, 255]));
}

// M1-RND-03
#[test]
fn zero_size_target_does_not_panic() {
    let gpu = gpu();
    let mut target = OffscreenTarget::new(&gpu, 64, 64);
    target.resize(&gpu, 0, 0);
    target.clear(&gpu, Color::rgb(1.0, 1.0, 1.0));
    assert!(target.read_pixels(&gpu).unwrap().is_empty());

    target.resize(&gpu, 8, 8);
    target.clear(&gpu, Color::rgb(0.0, 0.0, 0.0));
    assert_eq!(target.read_pixels(&gpu).unwrap().len(), 8 * 8 * 4);
}
