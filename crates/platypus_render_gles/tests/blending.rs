// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! How translucent colours combine (docs/rendering.md "Blending"), checked by pixel values on
//! both GPU executors: `examples/alpha-probe` (six probes over a `#3060c0` field) in sRGB, the
//! default, and in linear light, and a scene per area the setting reaches (opacity groups,
//! gradients, shadows, filters, images, text). The GLES half needs an EGL driver (skipped
//! without one).

use std::path::{Path, PathBuf};

use glow::HasContext;
use platypus_render_gles::egl::EglContext;
use platypus_render_gles::{GlesRenderer, Target};
use platypus_ui::opcodes::*;
use platypus_ui::{DrawCmd, UiTree};
use web_ui_core::{Commit, Value};
use web_ui_render::testing::shared_gpu;
use web_ui_render::{Blending, Color, OffscreenTarget, RenderTarget, UiRenderer, wgpu};

const SIZE: (u32, u32) = (800, 200);
const FIELD: [u8; 3] = [48, 96, 192];

fn n(v: u32) -> Value {
    Value::Number(v.into())
}

/// An absolute box of `kind` at `rect` (left, top, width, height) over the field, with its style
/// and an optional text child.
struct Item<'a> {
    rect: [f64; 4],
    style: &'a [(&'a str, &'a str)],
    text: Option<&'a str>,
}

fn item<'a>(rect: [f64; 4], style: &'a [(&'a str, &'a str)]) -> Item<'a> {
    Item {
        rect,
        style,
        text: None,
    }
}

/// The draw list of `items` over an 800x200 field of `background` at 0,0.
fn draw_list(background: &str, items: &[Item]) -> Vec<DrawCmd> {
    let mut ops = Vec::new();
    let mut next = 1;
    let mut element =
        |ops: &mut Vec<Value>, parent: u32, rect: [f64; 4], style: &[(&str, &str)]| {
            let id = next;
            next += 1;
            ops.push(Value::Array(vec![n(CREATE), n(id), "box".into()]));
            let mut set = vec![n(SET_STYLE), n(id)];
            for (k, v) in [
                ("position", Value::from("absolute")),
                ("left", rect[0].into()),
                ("top", rect[1].into()),
                ("width", rect[2].into()),
                ("height", rect[3].into()),
            ] {
                set.push(k.into());
                set.push(v);
            }
            for (k, v) in style {
                set.push((*k).into());
                set.push((*v).into());
            }
            ops.push(Value::Array(set));
            ops.push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
            id
        };
    let field = element(
        &mut ops,
        0,
        [0.0, 0.0, 800.0, 200.0],
        &[("backgroundColor", background)],
    );
    let mut texts = Vec::new();
    for it in items {
        let id = element(&mut ops, field, it.rect, it.style);
        if let Some(t) = it.text {
            texts.push((id, t));
        }
    }
    for (i, (parent, text)) in texts.into_iter().enumerate() {
        let id = 1000 + i as u32;
        ops.push(Value::Array(vec![n(CREATE_TEXT), n(id), text.into()]));
        ops.push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
    }
    let mut t = UiTree::new(SIZE.0 as f32, SIZE.1 as f32);
    let errors = t.apply_commit(&Commit { ops });
    assert!(errors.is_empty(), "{errors:?}");
    t.layout_if_needed();
    t.draw_list()
}

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/images")
}

/// The wgpu renderer's pixels (RGBA8, top row first) through an sRGB target, as the app's.
fn wgpu_pixels(cmds: &[DrawCmd], blending: Blending) -> Vec<u8> {
    let gpu = shared_gpu();
    let mut ui = UiRenderer::new(&gpu);
    ui.set_asset_root(assets());
    ui.set_blending(blending);
    let target =
        OffscreenTarget::with_format(&gpu, SIZE.0, SIZE.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    ui.render(
        &gpu,
        &RenderTarget {
            view: &view,
            format: target.format(),
            size: SIZE,
            scale: 1.0,
        },
        Color::rgb(0.0, 0.0, 0.0),
        cmds,
    );
    target.read_pixels(&gpu).unwrap()
}

/// The GLES renderer's pixels, top row first, into an `SRGB8_ALPHA8` target (`srgb_target`) or a
/// plain `RGBA8` one like the window's default framebuffer. `None` without an EGL driver.
fn gles_pixels(cmds: &[DrawCmd], blending: Blending, srgb_target: bool) -> Option<Vec<u8>> {
    gles_pixels_with(cmds, blending, srgb_target, false)
}

fn gles_pixels_with(
    cmds: &[DrawCmd],
    blending: Blending,
    srgb_target: bool,
    linear_blur: bool,
) -> Option<Vec<u8>> {
    // One context at a time in the process (`common`).
    let _one = crate::common::one();
    let dir = std::env::var_os("PLATYPUS_ANGLE_DIR").map(PathBuf::from);
    let cx = match EglContext::headless(dir.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let mut renderer = match GlesRenderer::new(cx.gl()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("skipped: the GLES renderer can't start here: {e}");
            return None;
        }
    };
    renderer.set_blending(blending);
    renderer.set_linear_blur(linear_blur);
    web_ui_render::UiBackend::set_asset_root(&mut renderer, assets());
    let gl = cx.gl();
    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        let format = if srgb_target {
            glow::SRGB8_ALPHA8
        } else {
            glow::RGBA8
        };
        gl.tex_storage_2d(glow::TEXTURE_2D, 1, format, SIZE.0 as i32, SIZE.1 as i32);
        let fbo = gl.create_framebuffer().unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        renderer.render(
            &Target {
                framebuffer: Some(fbo),
                size: SIZE,
                scale: 1.0,
                srgb: srgb_target,
            },
            Color::rgb(0.0, 0.0, 0.0),
            cmds,
        );
        Some(renderer.read_back(Some(fbo), SIZE))
    }
}

fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * SIZE.0 + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

/// Requires the pixel at (x, 100) to be `expect`, each channel within `tolerance`.
fn check(name: &str, pixels: &[u8], x: u32, expect: [u8; 3], tolerance: u8) {
    let got = px(pixels, x, 100);
    assert!(
        got.iter()
            .zip(expect)
            .all(|(a, b)| a.abs_diff(b) <= tolerance),
        "{name} at x={x}: {got:?}, expected {expect:?}"
    );
}

/// Every executor's pixels for `cmds` in `blending`: wgpu, then GLES into an sRGB and a plain
/// target when there is an EGL driver.
fn all_pixels(cmds: &[DrawCmd], blending: Blending) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = vec![("wgpu", wgpu_pixels(cmds, blending))];
    for (name, srgb) in [("gles (sRGB target)", true), ("gles (plain target)", false)] {
        if let Some(p) = gles_pixels(cmds, blending, srgb) {
            out.push((name, p));
        }
    }
    if let Some(dir) = std::env::var_os("PLATYPUS_BLEND_DUMP") {
        for (name, pixels) in &out {
            let file = format!(
                "{}-{blending:?}.png",
                name.split(' ').next().unwrap_or(name)
            );
            image::save_buffer(
                PathBuf::from(&dir).join(file),
                pixels,
                SIZE.0,
                SIZE.1,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
    out
}

/// `examples/alpha-probe`: six 100x100 probes at top 50 over the field.
fn probe() -> Vec<DrawCmd> {
    let at = |x| [x, 50.0, 100.0, 100.0];
    draw_list(
        "#3060c0",
        &[
            item(
                at(20.0),
                &[("backgroundColor", "rgba(255, 255, 255, 0.25)")],
            ),
            item(
                at(140.0),
                &[("border", "10px solid rgba(255, 255, 255, 0.25)")],
            ),
            item(at(260.0), &[("backgroundColor", "rgba(255, 0, 0, 0.5)")]),
            item(
                at(380.0),
                &[
                    ("backgroundColor", "rgba(255, 0, 0, 0.5)"),
                    ("border", "10px solid rgba(0, 0, 0, 0.25)"),
                ],
            ),
            item(
                at(500.0),
                &[
                    ("border", "10px solid rgba(255, 255, 255, 0.25)"),
                    ("borderRadius", "18"),
                ],
            ),
            item(
                at(620.0),
                &[
                    ("backgroundColor", "rgba(255, 255, 255, 0.25)"),
                    ("backdropFilter", "blur(4px)"),
                ],
            ),
        ],
    )
}

/// The probe's sample points (x at y = 100) and what a browser shows there.
const PROBE_SRGB: [(&str, u32, [u8; 3]); 8] = [
    ("1: white fill at 25%", 70, [100, 136, 208]),
    ("2: white border at 25%", 145, [100, 136, 208]),
    ("2: inside the border", 190, FIELD),
    ("3: red fill at 50%", 310, [152, 48, 96]),
    ("4: black border at 25% over the fill", 385, [114, 36, 72]),
    ("4: inside the border", 430, [152, 48, 96]),
    ("5: rounded border, mid left edge", 505, [100, 136, 208]),
    ("6: fill over a backdrop blur", 670, [100, 136, 208]),
];

// M2-BLEND-03
#[test]
fn the_alpha_probe_blends_in_srgb_by_default_on_every_executor() {
    let cmds = probe();
    for (executor, pixels) in all_pixels(&cmds, Blending::default()) {
        for (name, x, expect) in PROBE_SRGB {
            check(&format!("{executor} {name}"), &pixels, x, expect, 1);
        }
    }
}

// M2-BLEND-04
#[test]
fn the_alpha_probe_keeps_the_linear_light_numbers_with_linear_blending() {
    let cmds = probe();
    let linear = [
        (70, [142, 157, 210]),
        (145, [142, 157, 210]),
        (190, FIELD),
        (310, [191, 69, 141]),
        (385, [167, 59, 123]),
        (430, [191, 69, 141]),
        (505, [142, 157, 210]),
        (670, [142, 157, 210]),
    ];
    for (executor, pixels) in all_pixels(&cmds, Blending::Linear) {
        for (x, expect) in linear {
            check(&format!("{executor} linear"), &pixels, x, expect, 1);
        }
    }
}

// M2-BLEND-05
#[test]
fn opacity_groups_filters_and_gradients_fade_in_srgb() {
    // Each is white at half strength over the field: (255 + field) / 2 in sRGB.
    let half = [152, 176, 224];
    let cmds = draw_list(
        "#3060c0",
        &[
            // An opacity group: the child is opaque, the group at 50%.
            item(
                [20.0, 50.0, 100.0, 100.0],
                &[("opacity", "0.5"), ("backgroundColor", "white")],
            ),
            // `filter: opacity()` goes through the colour matrix.
            item(
                [140.0, 50.0, 100.0, 100.0],
                &[("filter", "opacity(50%)"), ("backgroundColor", "white")],
            ),
            // A gradient from transparent white to white: half-way it is white at 50%.
            item(
                [260.0, 50.0, 200.0, 100.0],
                &[(
                    "backgroundImage",
                    "linear-gradient(to right, rgba(255, 255, 255, 0), white)",
                )],
            ),
            // An opaque gradient is the same in both modes (it is interpolated in sRGB).
            item(
                [500.0, 50.0, 200.0, 100.0],
                &[(
                    "backgroundImage",
                    "linear-gradient(to right, #ff0000, #0000ff)",
                )],
            ),
        ],
    );
    for (executor, pixels) in all_pixels(&cmds, Blending::Srgb) {
        check(&format!("{executor} opacity group"), &pixels, 70, half, 1);
        check(&format!("{executor} filter opacity"), &pixels, 190, half, 1);
        check(
            &format!("{executor} translucent gradient"),
            &pixels,
            360,
            half,
            2,
        );
        check(
            &format!("{executor} opaque gradient"),
            &pixels,
            600,
            [128, 0, 128],
            2,
        );
    }
    for (executor, pixels) in all_pixels(&cmds, Blending::Linear) {
        // Linear light: half of white over the field encodes much lighter.
        check(
            &format!("{executor} linear group"),
            &pixels,
            70,
            [190, 199, 228],
            2,
        );
        check(
            &format!("{executor} linear opaque gradient"),
            &pixels,
            600,
            [128, 0, 128],
            2,
        );
    }
}

// M2-BLEND-06
#[test]
fn shadows_and_blurs_darken_and_average_in_srgb() {
    // A hard spread shadow of black at 50%: half the field in sRGB.
    let cmds = draw_list(
        "#3060c0",
        &[item(
            [40.0, 50.0, 60.0, 100.0],
            &[("boxShadow", "0 0 0 20px rgba(0, 0, 0, 0.5)")],
        )],
    );
    for (executor, pixels) in all_pixels(&cmds, Blending::Srgb) {
        check(&format!("{executor} shadow"), &pixels, 30, [24, 48, 96], 1);
    }
    // A blurred white box on black, sampled just inside its edge (the pixel centre is half a
    // pixel in: 53% white): the blur averages sRGB values, so that is 136, where linear light
    // encodes the same mix to 193.
    let edge = draw_list(
        "black",
        &[item(
            [400.0, 0.0, 400.0, 200.0],
            &[("backgroundColor", "white"), ("filter", "blur(6px)")],
        )],
    );
    for (executor, pixels) in all_pixels(&edge, Blending::Srgb) {
        check(
            &format!("{executor} blurred edge"),
            &pixels,
            400,
            [136, 136, 136],
            3,
        );
    }
    for (executor, pixels) in all_pixels(&edge, Blending::Linear) {
        check(
            &format!("{executor} linear blurred edge"),
            &pixels,
            400,
            [193, 193, 193],
            3,
        );
    }
}

// M2-BLEND-12
#[test]
fn a_linear_blur_on_srgb_canvases_averages_in_linear_light() {
    // A backdrop blur over a black/white edge, sampled just inside the white: averaging sRGB
    // values gives about 136, linear light about 193 (as M2-BLEND-06's blurred edge).
    let edge = draw_list(
        "black",
        &[
            item([400.0, 0.0, 400.0, 200.0], &[("backgroundColor", "white")]),
            item(
                [200.0, 0.0, 400.0, 200.0],
                &[("backdropFilter", "blur(6px)")],
            ),
        ],
    );
    let Some(browser) = gles_pixels_with(&edge, Blending::Srgb, true, false) else {
        return;
    };
    check("sRGB blur", &browser, 400, [136, 136, 136], 4);
    for srgb in [true, false] {
        let Some(pixels) = gles_pixels_with(&edge, Blending::Srgb, srgb, true) else {
            return;
        };
        let name = format!("linear blur (sRGB target: {srgb})");
        check(&name, &pixels, 400, [193, 193, 193], 4);
        // Away from the edge black and white stay so, inside the box and out.
        check(&name, &pixels, 250, [0, 0, 0], 1);
        check(&name, &pixels, 550, [255, 255, 255], 1);
        check(&name, &pixels, 700, [255, 255, 255], 1);
    }
    // Linear canvases blur in linear light whatever the setting.
    if let Some(pixels) = gles_pixels_with(&edge, Blending::Linear, true, false) {
        check("linear canvas", &pixels, 400, [193, 193, 193], 4);
    }
}

// M2-BLEND-07
#[test]
fn images_keep_their_colours_on_srgb_canvases() {
    // `gray.png`: 16x16 of 140 and 255 grey, drawn texel for texel.
    let file = image::open(assets().join("gray.png")).unwrap().to_rgba8();
    let cmds = draw_list(
        "#3060c0",
        &[item(
            [0.0, 92.0, 16.0, 16.0],
            &[
                ("backgroundImage", "url(gray.png)"),
                ("imageRendering", "pixelated"),
            ],
        )],
    );
    for blending in [Blending::Srgb, Blending::Linear] {
        for (executor, pixels) in all_pixels(&cmds, blending) {
            for x in 0..16 {
                let want = file.get_pixel(x, 8).0;
                let got = px(&pixels, x, 100);
                assert!(
                    got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 1),
                    "{executor} {blending:?} x={x}: {got:?}, the file has {want:?}"
                );
            }
        }
    }
}

// M2-BLEND-08
#[test]
fn text_coverage_blends_in_srgb() {
    let words = "Hamburgefonstiv quick brown fox";
    let text = |color| {
        [Item {
            rect: [10.0, 80.0, 780.0, 40.0],
            style: color,
            text: Some(words),
        }]
    };
    let light: &[(&str, &str)] = &[("color", "white"), ("fontSize", "24")];
    let dark: &[(&str, &str)] = &[("color", "black"), ("fontSize", "24")];
    let ink = |pixels: &[u8]| -> u64 { pixels.chunks(4).map(|p| u64::from(p[0])).sum() };
    let on_black = draw_list("black", &text(light));
    let on_white = draw_list("white", &text(dark));
    let srgb_light = all_pixels(&on_black, Blending::Srgb);
    let linear_light = all_pixels(&on_black, Blending::Linear);
    let srgb_dark = all_pixels(&on_white, Blending::Srgb);
    let linear_dark = all_pixels(&on_white, Blending::Linear);
    for i in 0..srgb_light.len() {
        let name = srgb_light[i].0;
        // Edges of white text on black: coverage c is c * 255 in sRGB, lighter in linear light.
        let (s, l) = (ink(&srgb_light[i].1), ink(&linear_light[i].1));
        assert!(
            s > 0 && s < l,
            "{name}: white text {s} vs {l} in linear light"
        );
        // Black text on white is darker in sRGB too (linear light needed a boost for it).
        let (s, l) = (ink(&srgb_dark[i].1), ink(&linear_dark[i].1));
        assert!(s < l, "{name}: black text {s} vs {l} in linear light");
        // Fully covered pixels stay the text colour.
        assert!(srgb_light[i].1.chunks(4).any(|p| p[0] == 255), "{name}");
    }
}
