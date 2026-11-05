// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! UI over the 3D world blends as UI alone does (docs/rendering.md "Blending"): `render_over`
//! seeds an sRGB canvas with the world, encoded, so a translucent fill over it gives the browser's
//! value; `render_overlay` onto an sRGB target is the hardware's blend, in linear light.

use platypus_ui::opcodes::*;
use platypus_ui::{DrawCmd, UiTree};
use web_ui_core::{Commit, Value};
use web_ui_render::testing::shared_gpu;
use web_ui_render::{Blending, Color, OffscreenTarget, RenderTarget, UiRenderer, wgpu};

const SIZE: (u32, u32) = (64, 32);
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn n(v: u32) -> Value {
    Value::Number(v.into())
}

/// One box filling the target with `background`.
fn fill(background: &str) -> Vec<DrawCmd> {
    let mut set = vec![n(SET_STYLE), n(1)];
    for (k, v) in [
        ("width", Value::from(64.0)),
        ("height", Value::from(32.0)),
        ("backgroundColor", Value::from(background)),
    ] {
        set.push(k.into());
        set.push(v);
    }
    let ops = vec![
        Value::Array(vec![n(CREATE), n(1), "box".into()]),
        Value::Array(set),
        Value::Array(vec![n(APPEND_CHILD), n(0), n(1)]),
    ];
    let mut t = UiTree::new(64.0, 32.0);
    assert!(t.apply_commit(&Commit { ops }).is_empty());
    t.layout_if_needed();
    t.draw_list()
}

/// The centre pixel of 25% white drawn over a `#3060c0` "world" (drawn by another renderer,
/// standing in for the world pass), over a copy of it or blended onto it.
fn over_world(blending: Blending, copy: bool) -> [u8; 3] {
    let gpu = shared_gpu();
    let mut world = UiRenderer::new(&gpu);
    let mut ui = UiRenderer::new(&gpu);
    ui.set_blending(blending);
    let target = OffscreenTarget::with_format(&gpu, SIZE.0, SIZE.1, FORMAT);
    let view = target.view().unwrap();
    let rt = RenderTarget {
        view: &view,
        format: FORMAT,
        size: SIZE,
        scale: 1.0,
    };
    let field = fill("#3060c0");
    let glass = fill("rgba(255, 255, 255, 0.25)");
    if copy {
        let backdrop = ui.backdrop_target(&gpu, SIZE, FORMAT);
        let world_rt = RenderTarget {
            view: &backdrop,
            ..rt
        };
        world.render(&gpu, &world_rt, Color::TRANSPARENT, &field);
        ui.render_over(&gpu, &rt, &backdrop, &glass);
    } else {
        world.render(&gpu, &rt, Color::TRANSPARENT, &field);
        ui.render_overlay(&gpu, &rt, &glass);
    }
    let pixels = target.read_pixels(&gpu).unwrap();
    let i = ((16 * SIZE.0 + 32) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

fn near(got: [u8; 3], want: [u8; 3]) -> bool {
    got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 1)
}

// M2-BLEND-11
#[test]
fn ui_over_a_copy_of_the_world_blends_like_ui_alone() {
    let srgb = over_world(Blending::Srgb, true);
    assert!(near(srgb, [100, 136, 208]), "sRGB over a copy: {srgb:?}");
    let linear = over_world(Blending::Linear, true);
    assert!(
        near(linear, [142, 157, 210]),
        "linear over a copy: {linear:?}"
    );
    // Blended onto the sRGB target by the hardware: linear light, whatever the setting.
    for blending in [Blending::Srgb, Blending::Linear] {
        let overlay = over_world(blending, false);
        assert!(
            near(overlay, [142, 157, 210]),
            "{blending:?} overlay: {overlay:?}"
        );
    }
}
