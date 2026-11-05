// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The CPU plan executor against the GLES one (M2-KRN-01..12): the scenes of the GLES executor's own
//! comparison with wgpu (`platypus_render_gles/tests/vs_wgpu.rs`), built through the real `UiTree`,
//! drawn by both into a plain RGBA8 target (as a window's) and compared pixel for pixel. Needs an EGL
//! driver (Mesa's llvmpipe will do: `LIBGL_ALWAYS_SOFTWARE=1`); skipped without one.
//! `PLATYPUS_CPU_DUMP=<folder>` writes both pictures of every scene there.

use std::path::{Path, PathBuf};

use glow::HasContext;
use platypus_core::{Commit, Value};
use platypus_render::{Color, UiBackend};
use platypus_render_cpu::plan::PlanRenderer;
use platypus_render_gles::egl::EglContext;
use platypus_render_gles::{GlesRenderer, Target};
use platypus_ui::UiTree;
use platypus_ui::opcodes::*;

const CLEAR: Color = Color::rgb(0.02, 0.03, 0.05);

fn n(v: u32) -> Value {
    Value::Number(v.into())
}

struct Scene {
    ops: Vec<Value>,
    next: u32,
}

impl Scene {
    fn new() -> Self {
        Self {
            ops: Vec::new(),
            next: 1,
        }
    }

    /// An absolutely placed element of `kind` under `parent` (0 = the root).
    fn at(&mut self, parent: u32, kind: &str, rect: [f64; 4], style: &[(&str, &str)]) -> u32 {
        let id = self.next;
        self.next += 1;
        self.ops
            .push(Value::Array(vec![n(CREATE), n(id), kind.into()]));
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
        self.ops.push(Value::Array(set));
        self.ops
            .push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
        id
    }

    fn text(&mut self, parent: u32, text: &str) {
        let id = self.next;
        self.next += 1;
        self.ops
            .push(Value::Array(vec![n(CREATE_TEXT), n(id), text.into()]));
        self.ops
            .push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
    }

    /// The draw list, laid out with the images' natural sizes from `image_size`.
    fn cmds(
        self,
        size: (f32, f32),
        mut image_size: impl FnMut(&str) -> Option<(u32, u32)>,
    ) -> Vec<platypus_ui::DrawCmd> {
        let mut t = UiTree::new(size.0, size.1);
        let errors = t.apply_commit(&Commit { ops: self.ops });
        assert!(errors.is_empty(), "{errors:?}");
        t.layout_if_needed();
        for src in t.take_image_requests() {
            let size = image_size(&src);
            t.set_image_size(&src, size);
        }
        t.layout_if_needed();
        t.draw_list()
    }
}

struct Gles {
    // Declared first: the renderer goes before the context it draws with.
    renderer: GlesRenderer,
    gl: glow::Context,
    _cx: EglContext,
}

fn gles() -> Option<Gles> {
    let dir = std::env::var_os("PLATYPUS_ANGLE_DIR").map(PathBuf::from);
    let cx = match EglContext::headless(dir.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let renderer = match GlesRenderer::new(cx.gl()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("skipped: the GLES renderer can't start here: {e}");
            return None;
        }
    };
    Some(Gles {
        renderer,
        gl: cx.gl(),
        _cx: cx,
    })
}

/// The GLES executor's frame in a plain RGBA8 texture, top row first.
fn gles_pixels(
    g: &mut Gles,
    cmds: &[platypus_ui::DrawCmd],
    size: (u32, u32),
    scale: f32,
) -> Vec<u8> {
    let gl = &g.gl;
    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_storage_2d(
            glow::TEXTURE_2D,
            1,
            glow::RGBA8,
            size.0 as i32,
            size.1 as i32,
        );
        let fbo = gl.create_framebuffer().unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        g.renderer.render(
            &Target {
                framebuffer: Some(fbo),
                size,
                scale,
                srgb: false,
            },
            CLEAR,
            cmds,
        );
        let pixels = g.renderer.read_back(Some(fbo), size);
        gl.delete_framebuffer(fbo);
        gl.delete_texture(texture);
        pixels
    }
}

struct Diff {
    max: u8,
    /// Pixels whose channels differ by more than the tolerance.
    off: usize,
    pixels: usize,
}

fn diff(a: &[u8], b: &[u8], tolerance: u8) -> Diff {
    assert_eq!(a.len(), b.len());
    let mut d = Diff {
        max: 0,
        off: 0,
        pixels: a.len() / 4,
    };
    for (pa, pb) in a.chunks(4).zip(b.chunks(4)) {
        let m = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap();
        d.max = d.max.max(m);
        if m > tolerance {
            d.off += 1;
        }
    }
    d
}

/// Draws `scene` with both executors (images from `assets`) and requires the pixels to agree: at most
/// `allowed_off` of them may differ by more than `tolerance`. Returns the CPU executor's pixels.
fn same(
    name: &str,
    scene: Scene,
    assets: Option<&Path>,
    logical: (u32, u32),
    scale: f32,
    tolerance: u8,
    allowed_off: f32,
) -> Option<Vec<u8>> {
    let mut g = gles()?;
    let mut cpu = PlanRenderer::new();
    // As Fluid draws: sRGB blending, blurs averaged in linear light.
    cpu.set_linear_blur(true);
    g.renderer.set_linear_blur(true);
    if let Some(dir) = assets {
        cpu.set_asset_root(dir.to_path_buf());
        g.renderer.set_asset_root(dir.to_path_buf());
    }
    let size = (
        (logical.0 as f32 * scale) as u32,
        (logical.1 as f32 * scale) as u32,
    );
    let cmds = scene.cmds((logical.0 as f32, logical.1 as f32), |url| {
        cpu.image_size(url)
    });
    let a = gles_pixels(&mut g, &cmds, size, scale);
    cpu.render(size, scale, CLEAR, &cmds);
    let b = cpu.pixels().expect("a frame").to_vec();
    let blank = a.chunks(4).filter(|p| *p == &a[..4]).count();
    assert!(
        blank < a.len() / 4 * 9 / 10,
        "{name}: the GLES image is blank"
    );
    if let Some(dir) = std::env::var_os("PLATYPUS_CPU_DUMP") {
        for (which, pixels) in [("gles", &a), ("cpu", &b)] {
            let path =
                PathBuf::from(&dir).join(format!("{}-{which}.png", name.replace([' ', '+'], "_")));
            image::save_buffer(path, pixels, size.0, size.1, image::ColorType::Rgba8).unwrap();
        }
    }
    let d = diff(&a, &b, tolerance);
    eprintln!(
        "{name}: max diff {}, {} of {} pixels off by more than {tolerance}",
        d.max, d.off, d.pixels
    );
    assert!(
        (d.off as f32) <= allowed_off * d.pixels as f32,
        "{name}: {} of {} pixels differ by more than {tolerance} (max {})",
        d.off,
        d.pixels,
        d.max
    );
    Some(b)
}

// M2-KRN-01
#[test]
fn boxes_borders_and_gradients_match_gles() {
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [8.0, 8.0, 60.0, 50.0],
        &[
            ("backgroundColor", "white"),
            ("border", "4px solid #0000ff"),
            ("borderRadius", "12px"),
        ],
    );
    s.at(
        0,
        "box",
        [80.0, 8.0, 60.0, 50.0],
        &[
            ("backgroundColor", "#204060"),
            ("border", "3px dashed #ff8800"),
        ],
    );
    s.at(
        0,
        "box",
        [152.0, 8.0, 60.0, 50.0],
        &[
            ("backgroundColor", "#204060"),
            ("border", "4px dotted #88ff00"),
            ("borderRadius", "8px"),
        ],
    );
    s.at(
        0,
        "box",
        [8.0, 70.0, 100.0, 40.0],
        &[
            (
                "backgroundImage",
                "linear-gradient(to right, #ff0000, #0000ff)",
            ),
            ("borderRadius", "0 20px"),
        ],
    );
    s.at(
        0,
        "box",
        [120.0, 70.0, 50.0, 50.0],
        &[("backgroundImage", "radial-gradient(circle, white, black)")],
    );
    s.at(
        0,
        "box",
        [180.0, 70.0, 50.0, 50.0],
        &[
            (
                "backgroundImage",
                "conic-gradient(red, yellow, lime, aqua, blue, red)",
            ),
            ("borderRadius", "50%"),
        ],
    );
    s.at(
        0,
        "box",
        [8.0, 124.0, 80.0, 26.0],
        &[
            ("border", "2px solid"),
            ("borderColor", "red lime blue yellow"),
        ],
    );
    same("shapes", s, None, (240, 156), 1.0, 6, 0.002);
}

// M2-KRN-02
#[test]
fn shadows_match_gles() {
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [20.0, 20.0, 70.0, 50.0],
        &[
            ("backgroundColor", "#c0c0c0"),
            ("borderRadius", "10px"),
            ("boxShadow", "0 8px 16px rgba(0,0,0,0.6)"),
        ],
    );
    s.at(
        0,
        "box",
        [120.0, 20.0, 70.0, 50.0],
        &[
            ("backgroundColor", "#c0c0c0"),
            ("borderRadius", "10px"),
            ("boxShadow", "inset 0 0 12px 2px rgba(0,0,0,0.8)"),
        ],
    );
    same("shadows", s, None, (220, 100), 1.0, 6, 0.002);
}

// M2-KRN-03
#[test]
fn clips_match_gles() {
    let mut s = Scene::new();
    let parent = s.at(
        0,
        "box",
        [20.0, 20.0, 80.0, 60.0],
        &[
            ("overflow", "hidden"),
            ("borderRadius", "20px"),
            ("backgroundColor", "#303030"),
        ],
    );
    s.at(
        parent,
        "box",
        [-20.0, 30.0, 150.0, 60.0],
        &[("backgroundColor", "orange")],
    );
    s.at(
        parent,
        "box",
        [40.0, -10.0, 30.0, 90.0],
        &[("backgroundColor", "teal")],
    );
    same("clips", s, None, (120, 100), 1.0, 6, 0.002);
}

// M2-KRN-04
#[test]
fn text_matches_gles() {
    let mut s = Scene::new();
    let a = s.at(
        0,
        "text",
        [8.0, 6.0, 200.0, 24.0],
        &[("color", "white"), ("fontSize", "18")],
    );
    s.text(a, "Hello CPU 123 !?");
    let b = s.at(
        0,
        "text",
        [8.0, 36.0, 200.0, 40.0],
        &[("color", "#ffcc44"), ("fontSize", "32")],
    );
    s.text(b, "Platypus UI");
    let c = s.at(
        0,
        "box",
        [8.0, 84.0, 200.0, 26.0],
        &[("backgroundColor", "#e8e8e8")],
    );
    let d = s.at(
        c,
        "text",
        [4.0, 2.0, 190.0, 22.0],
        &[("color", "#101010"), ("fontSize", "16")],
    );
    s.text(d, "dark text on light");
    same("text", s, None, (216, 116), 1.0, 6, 0.002);
}

// M2-KRN-05
#[test]
fn text_and_boxes_at_scale_two_and_a_transformed_plane_match_gles() {
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [10.0, 10.0, 60.0, 40.0],
        &[("backgroundColor", "crimson"), ("borderRadius", "8px")],
    );
    let t = s.at(
        0,
        "text",
        [10.0, 56.0, 100.0, 20.0],
        &[("color", "white"), ("fontSize", "14")],
    );
    s.text(t, "scale two");
    s.at(
        0,
        "box",
        [90.0, 20.0, 50.0, 40.0],
        &[("backgroundColor", "gold"), ("transform", "rotate(12deg)")],
    );
    s.at(
        0,
        "box",
        [100.0, 64.0, 40.0, 20.0],
        &[
            ("backgroundColor", "teal"),
            ("transform", "perspective(80px) rotateY(30deg)"),
        ],
    );
    same("scale 2 + transform", s, None, (150, 90), 2.0, 6, 0.005);
}

/// A folder with a checkerboard PNG (with transparent cells) and an SVG.
fn assets() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("platypus_cpu_assets_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut img = image::RgbaImage::new(16, 16);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = match ((x / 4) + (y / 4)) % 3 {
            0 => image::Rgba([230, 60, 60, 255]),
            1 => image::Rgba([40, 80, 200, 255]),
            _ => image::Rgba([255, 255, 255, 0]),
        };
    }
    img.save(dir.join("checker.png")).unwrap();
    std::fs::write(
        dir.join("logo.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><circle cx="16" cy="16" r="14" fill="orange"/><rect x="4" y="4" width="10" height="10" fill="blue"/></svg>"#,
    )
    .unwrap();
    dir
}

// M2-KRN-06
#[test]
fn background_images_match_gles() {
    let dir = assets();
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [6.0, 6.0, 40.0, 40.0],
        &[
            ("backgroundImage", "url(checker.png)"),
            ("backgroundRepeat", "no-repeat"),
        ],
    );
    s.at(
        0,
        "box",
        [54.0, 6.0, 60.0, 40.0],
        &[
            ("backgroundImage", "url(checker.png)"),
            ("backgroundSize", "20px 20px"),
        ],
    );
    s.at(
        0,
        "box",
        [122.0, 6.0, 40.0, 40.0],
        &[
            ("backgroundImage", "url(checker.png)"),
            ("backgroundSize", "cover"),
            ("borderRadius", "14px"),
        ],
    );
    s.at(
        0,
        "box",
        [6.0, 54.0, 56.0, 40.0],
        &[
            ("backgroundImage", "url(checker.png)"),
            ("backgroundSize", "48px 32px"),
            ("imageRendering", "pixelated"),
            ("backgroundRepeat", "no-repeat"),
        ],
    );
    s.at(
        0,
        "box",
        [70.0, 54.0, 40.0, 40.0],
        &[
            ("backgroundImage", "url(logo.svg)"),
            ("backgroundSize", "contain"),
            ("backgroundRepeat", "no-repeat"),
        ],
    );
    s.at(
        0,
        "box",
        [118.0, 54.0, 44.0, 40.0],
        &[
            ("backgroundImage", "url(logo.svg)"),
            ("backgroundSize", "20px 20px"),
            ("backgroundColor", "#303040"),
        ],
    );
    same("images", s, Some(&dir), (170, 100), 1.0, 6, 0.005);
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [4.0, 4.0, 50.0, 40.0],
        &[
            ("backgroundImage", "url(checker.png)"),
            ("backgroundSize", "16px 16px"),
            ("borderRadius", "8px"),
        ],
    );
    s.at(
        0,
        "box",
        [60.0, 4.0, 36.0, 40.0],
        &[
            ("backgroundImage", "url(logo.svg)"),
            ("backgroundSize", "contain"),
            ("backgroundRepeat", "no-repeat"),
        ],
    );
    same("images x2", s, Some(&dir), (100, 48), 2.0, 6, 0.005);
}

// M2-KRN-07
#[test]
fn opacity_groups_and_small_blurs_match_gles() {
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [0.0, 0.0, 200.0, 60.0],
        &[("backgroundColor", "#204060")],
    );
    let g = s.at(0, "box", [10.0, 10.0, 80.0, 40.0], &[("opacity", "0.5")]);
    s.at(
        g,
        "box",
        [0.0, 0.0, 50.0, 40.0],
        &[("backgroundColor", "red")],
    );
    s.at(
        g,
        "box",
        [30.0, 10.0, 50.0, 30.0],
        &[("backgroundColor", "blue")],
    );
    // Radii of 6 and 5 taps: the exact Gaussian, as GLES.
    s.at(
        0,
        "box",
        [110.0, 8.0, 40.0, 44.0],
        &[
            ("backgroundColor", "white"),
            ("borderRadius", "10px"),
            ("filter", "blur(2px)"),
        ],
    );
    s.at(
        0,
        "box",
        [160.0, 8.0, 30.0, 44.0],
        &[
            ("backgroundColor", "orange"),
            ("filter", "blur(1.5px)"),
            ("opacity", "0.8"),
        ],
    );
    same("opacity + small blur", s, None, (200, 64), 1.0, 6, 0.005);
}

// M2-KRN-08
#[test]
fn large_blurs_are_close_to_the_gaussian() {
    // Radii past `EXACT_RADIUS` take three box passes instead of GLES's taps: the same spread, a
    // slightly different shape at the tails.
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [0.0, 0.0, 240.0, 120.0],
        &[("backgroundColor", "#204060")],
    );
    s.at(
        0,
        "box",
        [30.0, 30.0, 60.0, 60.0],
        &[
            ("backgroundColor", "white"),
            ("borderRadius", "10px"),
            ("filter", "blur(8px)"),
        ],
    );
    s.at(
        0,
        "box",
        [140.0, 20.0, 70.0, 80.0],
        &[("backgroundColor", "orange"), ("filter", "blur(14px)")],
    );
    same("large blur", s, None, (240, 120), 1.0, 10, 0.02);
}

// M2-KRN-09
#[test]
fn drop_shadows_and_colour_filters_match_gles() {
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [0.0, 0.0, 240.0, 120.0],
        &[("backgroundColor", "#40506a")],
    );
    s.at(
        0,
        "box",
        [10.0, 10.0, 30.0, 30.0],
        &[
            ("backgroundColor", "gold"),
            ("borderRadius", "15px"),
            ("filter", "drop-shadow(8px 8px 0 lime)"),
        ],
    );
    s.at(
        0,
        "box",
        [60.0, 10.0, 30.0, 30.0],
        &[
            ("backgroundColor", "white"),
            ("borderRadius", "6px"),
            ("filter", "drop-shadow(4px 6px 2px rgba(0,0,0,0.9))"),
        ],
    );
    let filters = [
        "grayscale(1)",
        "brightness(1.6)",
        "contrast(0.5)",
        "hue-rotate(90deg)",
        "sepia(1)",
        "invert(1)",
        "saturate(3)",
        "opacity(0.4)",
    ];
    for (i, f) in filters.iter().enumerate() {
        let x = 10.0 + (i % 4) as f64 * 40.0;
        let y = 52.0 + (i / 4) as f64 * 34.0;
        s.at(
            0,
            "box",
            [x, y, 32.0, 28.0],
            &[
                (
                    "backgroundImage",
                    "linear-gradient(to right, #ff3030, #3060ff)",
                ),
                ("filter", f),
            ],
        );
    }
    same("drop shadow + filters", s, None, (240, 120), 1.0, 6, 0.005);
    // A fractional device scale shifts the shadow between texels: the linear sampling matters.
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [0.0, 0.0, 80.0, 60.0],
        &[("backgroundColor", "#40506a")],
    );
    s.at(
        0,
        "box",
        [10.0, 10.0, 30.0, 30.0],
        &[
            ("backgroundColor", "gold"),
            ("borderRadius", "15px"),
            ("filter", "drop-shadow(5px 5px 1px black)"),
        ],
    );
    same("drop shadow x1.5", s, None, (80, 60), 1.5, 6, 0.005);
}

// M2-KRN-10
#[test]
fn blend_modes_match_gles() {
    let modes = [
        "multiply",
        "screen",
        "overlay",
        "darken",
        "lighten",
        "color-dodge",
        "color-burn",
        "hard-light",
        "soft-light",
        "difference",
        "exclusion",
        "hue",
        "saturation",
        "color",
        "luminosity",
    ];
    let mut s = Scene::new();
    for (i, mode) in modes.iter().enumerate() {
        let x = 4.0 + (i % 5) as f64 * 44.0;
        let y = 4.0 + (i / 5) as f64 * 32.0;
        s.at(
            0,
            "box",
            [x, y, 40.0, 28.0],
            &[(
                "backgroundImage",
                "linear-gradient(to right, rgb(200,100,50), rgb(40,160,220))",
            )],
        );
        s.at(
            0,
            "box",
            [x + 12.0, y, 28.0, 28.0],
            &[
                ("backgroundColor", "rgb(180, 90, 200)"),
                ("mixBlendMode", mode),
            ],
        );
    }
    same("blend modes", s, None, (228, 100), 1.0, 6, 0.005);
}

// M2-KRN-11
#[test]
fn backdrop_filters_match_gles() {
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [0.0, 0.0, 200.0, 80.0],
        &[(
            "backgroundImage",
            "linear-gradient(to right, #ff4040, #40ff80, #4060ff)",
        )],
    );
    for i in 0..5 {
        s.at(
            0,
            "box",
            [i as f64 * 40.0, 0.0, 8.0, 80.0],
            &[("backgroundColor", "white")],
        );
    }
    s.at(
        0,
        "box",
        [20.0, 15.0, 70.0, 50.0],
        &[
            ("backdropFilter", "blur(2px)"),
            ("borderRadius", "12px"),
            ("backgroundColor", "rgba(255,255,255,0.15)"),
        ],
    );
    s.at(
        0,
        "box",
        [110.0, 15.0, 70.0, 50.0],
        &[("backdropFilter", "grayscale(1)")],
    );
    same("backdrop filters", s, None, (200, 80), 1.0, 6, 0.005);
    // Fluid's stacked menu glass: a large blur, and no bleed between the two boxes.
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [0.0, 0.0, 120.0, 28.0],
        &[("backgroundColor", "#2060ff")],
    );
    s.at(
        0,
        "box",
        [0.0, 28.0, 120.0, 52.0],
        &[("backgroundColor", "#ff8020")],
    );
    for (rect, color) in [
        ([0.0, 0.0, 120.0, 28.0], "rgba(246,246,250,0.35)"),
        ([0.0, 28.0, 120.0, 52.0], "rgba(246,246,250,0.62)"),
    ] {
        s.at(
            0,
            "box",
            rect,
            &[
                ("backdropFilter", "blur(10px) saturate(1.6)"),
                ("backgroundColor", color),
            ],
        );
    }
    let Some(px) = same("stacked backdrop glass", s, None, (120, 80), 1.0, 10, 0.02) else {
        return;
    };
    let at = |x: usize, y: usize| &px[(y * 120 + x) * 4..(y * 120 + x) * 4 + 3];
    for (edge, middle) in [(at(60, 28), at(60, 54)), (at(60, 27), at(60, 14))] {
        assert!(
            edge.iter().zip(middle).all(|(a, b)| a.abs_diff(*b) <= 2),
            "edge {edge:?} vs middle {middle:?}"
        );
    }
}

// M2-KRN-12
#[test]
fn masks_and_layers_inside_clips_match_gles() {
    let mut s = Scene::new();
    s.at(
        0,
        "box",
        [0.0, 0.0, 240.0, 70.0],
        &[("backgroundColor", "#303848")],
    );
    s.at(
        0,
        "box",
        [5.0, 5.0, 60.0, 60.0],
        &[
            ("backgroundColor", "orange"),
            ("maskImage", "linear-gradient(to right, black, transparent)"),
        ],
    );
    s.at(
        0,
        "box",
        [75.0, 5.0, 60.0, 60.0],
        &[
            ("backgroundColor", "white"),
            ("maskImage", "linear-gradient(to right, white, black)"),
            ("maskMode", "luminance"),
        ],
    );
    let dots = s.at(
        0,
        "box",
        [145.0, 5.0, 60.0, 60.0],
        &[
            ("backgroundColor", "teal"),
            (
                "maskImage",
                "radial-gradient(circle, black 40%, transparent 42%)",
            ),
            ("maskSize", "10px 10px"),
        ],
    );
    s.at(
        dots,
        "box",
        [10.0, 10.0, 30.0, 30.0],
        &[("backgroundColor", "crimson")],
    );
    let clip = s.at(
        0,
        "box",
        [212.0, 5.0, 26.0, 60.0],
        &[("overflow", "hidden"), ("borderRadius", "13px")],
    );
    let layer = s.at(
        clip,
        "box",
        [-10.0, 10.0, 60.0, 40.0],
        &[("opacity", "0.6")],
    );
    s.at(
        layer,
        "box",
        [0.0, 0.0, 60.0, 40.0],
        &[("backgroundColor", "lime")],
    );
    same("masks + clipped layer", s, None, (240, 70), 1.0, 6, 0.005);
}
