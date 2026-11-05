// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `render_over`: UI drawn over pixels someone else put in the target (a compositor's windows),
//! with the canvas seeded from them so `backdrop-filter` blurs them. Needs an EGL driver; skipped
//! without one.

use glow::HasContext;
use platypus_render_gles::{GlesRenderer, Target};
use platypus_ui::UiTree;
use platypus_ui::opcodes::*;
use web_ui_core::{Commit, Value};

const W: u32 = 40;
const H: u32 = 20;

fn n(v: u32) -> Value {
    Value::Number(v.into())
}

/// One absolutely placed box covering the target with `style`.
fn cmds(style: &[(&str, &str)]) -> Vec<platypus_ui::DrawCmd> {
    let mut set = vec![n(SET_STYLE), n(1)];
    for (k, v) in [
        ("position", Value::from("absolute")),
        ("left", 0.0.into()),
        ("top", 0.0.into()),
        ("width", (W as f64).into()),
        ("height", (H as f64).into()),
    ] {
        set.push(k.into());
        set.push(v);
    }
    for (k, v) in style {
        set.push((*k).into());
        set.push((*v).into());
    }
    let ops = vec![
        Value::Array(vec![n(CREATE), n(1), "box".into()]),
        Value::Array(set),
        Value::Array(vec![n(APPEND_CHILD), n(0), n(1)]),
    ];
    let mut t = UiTree::new(W as f32, H as f32);
    let errors = t.apply_commit(&Commit { ops });
    assert!(errors.is_empty(), "{errors:?}");
    t.draw_list()
}

/// A plain `RGBA8` target (like a compositor's framebuffer: no sRGB encoding on write), its left
/// half red and its right half blue.
unsafe fn target(gl: &glow::Context) -> glow::Framebuffer {
    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_storage_2d(glow::TEXTURE_2D, 1, glow::RGBA8, W as i32, H as i32);
        let fbo = gl.create_framebuffer().unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(0, 0, (W / 2) as i32, H as i32);
        gl.clear_color(1.0, 0.0, 0.0, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.scissor((W / 2) as i32, 0, (W / 2) as i32, H as i32);
        gl.clear_color(0.0, 0.0, 1.0, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.disable(glow::SCISSOR_TEST);
        fbo
    }
}

unsafe fn pixel(gl: &glow::Context, fbo: glow::Framebuffer, x: i32, y: i32) -> [u8; 4] {
    let mut p = [0u8; 4];
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.read_pixels(
            x,
            y,
            1,
            1,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut p)),
        );
    }
    p
}

use crate::common::headless as context;

// M2-GLES-14
#[test]
fn backdrop_blur_over_a_seeded_target_blurs_what_was_there() {
    let Some(cx) = context() else { return };
    let gl = cx.gl();
    let mut renderer = GlesRenderer::new(cx.gl()).expect("renderer");
    let blurred = cmds(&[("backdropFilter", "blur(4px)")]);
    unsafe {
        // Seeded: the edge between red and blue is blurred into purple.
        let fbo = target(&gl);
        let t = Target {
            framebuffer: Some(fbo),
            size: (W, H),
            scale: 1.0,
            srgb: false,
        };
        renderer.render_over(&t, &blurred);
        let edge = pixel(&gl, fbo, (W / 2) as i32, (H / 2) as i32);
        assert!(
            edge[0] > 40 && edge[2] > 40,
            "the edge should mix red and blue: {edge:?}"
        );
        // Far from the edge the colour is the target's own, round-tripped through the canvas.
        let left = pixel(&gl, fbo, 1, (H / 2) as i32);
        assert!(
            left[0] > 200 && left[2] < 60,
            "the left stays red: {left:?}"
        );

        // A plain overlay has nothing behind the box to blur: the target is untouched.
        let fbo = target(&gl);
        let t = Target {
            framebuffer: Some(fbo),
            ..t
        };
        renderer.render_overlay(&t, &blurred);
        let edge = pixel(&gl, fbo, (W / 2) as i32, (H / 2) as i32);
        assert_eq!(edge, [0, 0, 255, 255], "render_overlay sees no backdrop");
    }
}

// M2-GLES-19
#[test]
fn a_frame_after_release_canvases_is_the_same() {
    let Some(cx) = context() else { return };
    let gl = cx.gl();
    let mut renderer = GlesRenderer::new(cx.gl()).expect("renderer");
    let blurred = cmds(&[("backdropFilter", "blur(4px)")]);
    let draw = |renderer: &mut GlesRenderer| unsafe {
        let fbo = target(&gl);
        let t = Target {
            framebuffer: Some(fbo),
            size: (W, H),
            scale: 1.0,
            srgb: false,
        };
        renderer.render_over(&t, &blurred);
        [
            pixel(&gl, fbo, 1, (H / 2) as i32),
            pixel(&gl, fbo, (W / 2) as i32, (H / 2) as i32),
        ]
    };
    let first = draw(&mut renderer);
    assert!(
        renderer.canvas_bytes() >= (W * H * 4 * 2) as usize,
        "canvases and the seed"
    );
    assert!(renderer.release_canvases() >= (W * H * 4 * 2) as usize);
    assert_eq!(renderer.canvas_bytes(), 0);
    assert_eq!(draw(&mut renderer), first, "made again, the same pixels");
}

// M2-GLES-14
#[test]
fn a_seeded_target_without_ui_keeps_its_pixels() {
    let Some(cx) = context() else { return };
    let gl = cx.gl();
    let mut renderer = GlesRenderer::new(cx.gl()).expect("renderer");
    unsafe {
        let fbo = target(&gl);
        let t = Target {
            framebuffer: Some(fbo),
            size: (W, H),
            scale: 1.0,
            srgb: false,
        };
        renderer.render_over(&t, &[]);
        assert_eq!(pixel(&gl, fbo, 2, 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&gl, fbo, (W - 2) as i32, 2), [0, 0, 255, 255]);
    }
}
