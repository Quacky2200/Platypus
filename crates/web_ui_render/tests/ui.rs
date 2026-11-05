// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! UI renderer tests (M2-DRW-01..08): scenes are built through the real UiTree
//! from styles, rendered offscreen, and checked with pixel assertions that
//! encode the CSS behaviour, plus golden images for regressions.

use platypus_ui::UiTree;
use platypus_ui::opcodes::*;
use web_ui_core::{Commit, Value};
use web_ui_render::testing::{assert_golden, shared_gpu};
use web_ui_render::{Blending, Color, OffscreenTarget, RenderStats, RenderTarget, UiRenderer};

/// Golden comparisons allow small differences between GPUs and drivers.
const TOLERANCE: u8 = 3;

struct Scene {
    ops: Vec<Value>,
    next: u32,
    /// A stylesheet added after the ops.
    css: &'static str,
    /// Runs on the laid-out tree before drawing (selections, time, later commits).
    prepare: Option<Prepare>,
}

/// A step run on the laid-out tree before drawing.
type Prepare = Box<dyn Fn(&mut UiTree)>;

impl Scene {
    fn new() -> Self {
        Self {
            ops: Vec::new(),
            next: 1,
            css: "",
            prepare: None,
        }
    }

    /// Sets attributes on `id`.
    fn attrs(&mut self, id: u32, attrs: &[(&str, Value)]) {
        let mut set = vec![Value::Number(SET_ATTRS.into()), Value::Number(id.into())];
        for (k, v) in attrs {
            set.push((*k).into());
            set.push(v.clone());
        }
        self.ops.push(Value::Array(set));
    }

    /// Adds an element of `kind` (`box`, `text`, `span`) under `parent`.
    fn element(&mut self, parent: u32, kind: &str, style: &[(&str, Value)]) -> u32 {
        let id = self.next;
        self.next += 1;
        let n = |v: u32| Value::Number(v.into());
        self.ops
            .push(Value::Array(vec![n(CREATE), n(id), kind.into()]));
        let mut set = vec![n(SET_STYLE), n(id)];
        for (k, v) in style {
            set.push((*k).into());
            set.push(v.clone());
        }
        self.ops.push(Value::Array(set));
        self.ops
            .push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
        id
    }

    /// Adds a text node with `text` under `parent`.
    fn text(&mut self, parent: u32, text: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        let n = |v: u32| Value::Number(v.into());
        self.ops
            .push(Value::Array(vec![n(CREATE_TEXT), n(id), text.into()]));
        self.ops
            .push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
        id
    }

    /// A `text` element containing `text`.
    fn label(&mut self, parent: u32, text: &str, style: &[(&str, Value)]) -> u32 {
        let id = self.element(parent, "text", style);
        self.text(id, text);
        id
    }

    /// Adds a box under `parent` (0 = root) and returns its id.
    fn add(&mut self, parent: u32, style: &[(&str, Value)]) -> u32 {
        let id = self.next;
        self.next += 1;
        let n = |v: u32| Value::Number(v.into());
        self.ops
            .push(Value::Array(vec![n(CREATE), n(id), "box".into()]));
        let mut set = vec![n(SET_STYLE), n(id)];
        for (k, v) in style {
            set.push((*k).into());
            set.push(v.clone());
        }
        self.ops.push(Value::Array(set));
        self.ops
            .push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
        id
    }
}

/// Absolute position and size, plus extra style.
fn at(x: f64, y: f64, w: f64, h: f64, extra: &[(&str, Value)]) -> Vec<(&'static str, Value)> {
    let mut v: Vec<(&'static str, Value)> = vec![
        ("position", "absolute".into()),
        ("left", x.into()),
        ("top", y.into()),
        ("width", w.into()),
        ("height", h.into()),
    ];
    for (k, val) in extra {
        // Style names in these tests are static strings.
        let k: &'static str = Box::leak(k.to_string().into_boxed_str());
        v.push((k, val.clone()));
    }
    v
}

struct Frame {
    pixels: Vec<u8>,
    width: u32,
    stats: RenderStats,
}

impl Frame {
    fn px(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.pixels[i..i + 4].try_into().unwrap()
    }

    fn luma(&self, x: u32, y: u32) -> f32 {
        let [r, g, b, _] = self.px(x, y);
        0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)
    }
}

fn render(scene: Scene, logical: (u32, u32), scale: f32, clear: Color) -> Frame {
    render_with_assets(scene, logical, scale, clear, None)
}

/// The goldens and pixel checks here were made in linear light (`Blending::Linear`, the default
/// before 2026-10-30) and keep it; the sRGB default is checked by the `M2-BLEND` tests.
fn render_with_assets(
    scene: Scene,
    logical: (u32, u32),
    scale: f32,
    clear: Color,
    assets: Option<&std::path::Path>,
) -> Frame {
    render_blended(scene, logical, scale, clear, assets, Blending::Linear)
}

fn render_blended(
    scene: Scene,
    logical: (u32, u32),
    scale: f32,
    clear: Color,
    assets: Option<&std::path::Path>,
    blending: Blending,
) -> Frame {
    let gpu = shared_gpu();
    let mut tree = UiTree::new(logical.0 as f32, logical.1 as f32);
    let errors = tree.apply_commit(&Commit { ops: scene.ops });
    assert!(errors.is_empty(), "{errors:?}");
    if !scene.css.is_empty() {
        let added = tree.add_stylesheet(scene.css);
        assert!(added.warnings.is_empty(), "{:?}", added.warnings);
    }

    let (w, h) = (
        (logical.0 as f32 * scale) as u32,
        (logical.1 as f32 * scale) as u32,
    );
    let target = OffscreenTarget::with_format(&gpu, w, h, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut ui = UiRenderer::new(&gpu);
    ui.set_blending(blending);
    if let Some(root) = assets {
        ui.set_asset_root(root);
    }
    // Like the app: report the natural sizes of images the tree asked about, so image
    // elements and image markers lay out before drawing.
    tree.layout_if_needed();
    if let Some(prepare) = &scene.prepare {
        prepare(&mut tree);
        tree.layout_if_needed();
    }
    for src in tree.take_image_requests() {
        let size = ui.image_size(&gpu, &src);
        tree.set_image_size(&src, size);
    }
    let cmds = tree.draw_list();
    let view = target.view().unwrap();
    let stats = ui.render(
        &gpu,
        &RenderTarget {
            view: &view,
            format: target.format(),
            size: (w, h),
            scale,
        },
        clear,
        &cmds,
    );
    Frame {
        pixels: target.read_pixels(&gpu).unwrap(),
        width: w,
        stats,
    }
}

use web_ui_render::wgpu;

const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);

fn three_boxes() -> Scene {
    let mut s = Scene::new();
    s.add(
        0,
        &at(
            0.0,
            0.0,
            32.0,
            32.0,
            &[("backgroundColor", "#ff0000".into())],
        ),
    );
    s.add(
        0,
        &at(
            32.0,
            0.0,
            32.0,
            32.0,
            &[("backgroundColor", "#00ff00".into())],
        ),
    );
    s.add(
        0,
        &at(
            64.0,
            0.0,
            32.0,
            32.0,
            &[("backgroundColor", "#0000ff".into())],
        ),
    );
    s
}

// M2-DRW-01
#[test]
fn coloured_boxes() {
    let f = render(three_boxes(), (96, 32), 1.0, BLACK);
    assert_eq!(f.px(16, 16), [255, 0, 0, 255]);
    assert_eq!(f.px(48, 16), [0, 255, 0, 255]);
    assert_eq!(f.px(80, 16), [0, 0, 255, 255]);
    // Edges are crisp on pixel boundaries.
    assert_eq!(f.px(31, 0), [255, 0, 0, 255]);
    assert_eq!(f.px(32, 0), [0, 255, 0, 255]);
    assert_eq!(f.stats.draw_calls, 1);
    assert_golden("M2-DRW-01", 96, 32, &f.pixels, TOLERANCE);
}

// M2-DRW-02
#[test]
fn rounded_corners_and_borders() {
    let mut s = Scene::new();
    s.add(
        0,
        &at(
            8.0,
            8.0,
            48.0,
            48.0,
            &[
                ("backgroundColor", "white".into()),
                ("border", "4px solid #0000ff".into()),
                ("borderRadius", 12.0.into()),
            ],
        ),
    );
    let f = render(s, (64, 64), 1.0, BLACK);
    assert_eq!(f.px(32, 32), [255, 255, 255, 255], "fill");
    assert_eq!(f.px(9, 32), [0, 0, 255, 255], "left border band (x 8..12)");
    assert_eq!(f.px(32, 54), [0, 0, 255, 255], "bottom border band");
    assert_eq!(f.px(9, 9), [0, 0, 0, 255], "outside the rounded corner");
    assert_eq!(f.px(4, 32), [0, 0, 0, 255], "outside the box");
    // The corner edge is anti-aliased: somewhere along the diagonal is partial.
    let partial = (8..20).any(|i| {
        let [_, _, b, _] = f.px(i, i);
        b > 10 && b < 245
    });
    assert!(partial, "rounded corner should be anti-aliased");
    assert_golden("M2-DRW-02", 64, 64, &f.pixels, TOLERANCE);
}

// M2-DRW-03
#[test]
fn many_boxes_are_one_draw_call() {
    let mut s = Scene::new();
    for i in 0..2000u32 {
        let (x, y) = (f64::from(i % 50) * 4.0, f64::from(i / 50) * 4.0);
        s.add(
            0,
            &at(x, y, 3.0, 3.0, &[("backgroundColor", "#888".into())]),
        );
    }
    let f = render(s, (200, 160), 1.0, BLACK);
    assert_eq!(f.stats.instances, 2000);
    assert_eq!(f.stats.draw_calls, 1);
    assert_eq!(f.stats.passes, 2, "one quad pass and the blit");
}

// M2-DRW-04
#[test]
fn scale_factor_two_renders_twice_the_pixels() {
    let one = render(three_boxes(), (96, 32), 1.0, BLACK);
    let two = render(three_boxes(), (96, 32), 2.0, BLACK);
    assert_eq!(two.width, 192);
    for (x, y) in [(16, 16), (48, 16), (80, 16), (31, 5), (32, 5)] {
        assert_eq!(two.px(x * 2, y * 2), one.px(x, y), "({x}, {y})");
        assert_eq!(two.px(x * 2 + 1, y * 2 + 1), one.px(x, y), "({x}, {y})");
    }
    assert_golden("M2-DRW-04", 192, 64, &two.pixels, TOLERANCE);
}

// M2-DRW-05
#[test]
fn per_corner_radii_and_gradients() {
    let mut s = Scene::new();
    s.add(
        0,
        &at(
            0.0,
            0.0,
            100.0,
            40.0,
            &[
                (
                    "backgroundImage",
                    "linear-gradient(to right, #ff0000, #0000ff)".into(),
                ),
                ("borderRadius", "0 20px".into()),
            ],
        ),
    );
    s.add(
        0,
        &at(
            0.0,
            50.0,
            50.0,
            50.0,
            &[(
                "backgroundImage",
                "radial-gradient(circle, white, black)".into(),
            )],
        ),
    );
    let f = render(s, (100, 100), 1.0, BLACK);

    let [r0, _, b0, _] = f.px(1, 20);
    let [r1, _, b1, _] = f.px(98, 20);
    assert!(r0 > 240 && b0 < 15, "left end is red: {:?}", f.px(1, 20));
    assert!(b1 > 240 && r1 < 15, "right end is blue: {:?}", f.px(98, 20));
    let [rm, _, bm, _] = f.px(50, 20);
    assert!(
        rm > 100 && bm > 100,
        "middle mixes both: {:?}",
        f.px(50, 20)
    );
    // Radii "0 20px": top-left square, top-right rounded.
    assert_eq!(f.px(0, 0)[3], 255);
    assert!(f.px(0, 0)[0] > 240, "top-left corner is square");
    assert_eq!(
        f.px(99, 0),
        [0, 0, 0, 255],
        "top-right corner is rounded away"
    );

    // Radial: white centre fading to black at the corners.
    assert!(f.luma(25, 75) > 240.0, "radial centre is white");
    assert!(f.luma(1, 51) < f.luma(10, 60), "darker towards the corner");
    assert_golden("M2-DRW-05", 100, 100, &f.pixels, TOLERANCE);
}

// M3-BG-01
#[test]
fn conic_and_repeating_gradients() {
    let mut s = Scene::new();
    // A pie: red for the first quarter turn, blue for the second, lime for the rest.
    s.add(
        0,
        &at(
            0.0,
            0.0,
            100.0,
            100.0,
            &[(
                "backgroundImage",
                "conic-gradient(red 0 25%, blue 0 50%, lime 0)".into(),
            )],
        ),
    );
    // 10px red and blue stripes, repeated.
    s.add(
        0,
        &at(
            0.0,
            110.0,
            100.0,
            20.0,
            &[(
                "backgroundImage",
                "repeating-linear-gradient(to right, red 0 10px, blue 10px 20px)".into(),
            )],
        ),
    );
    // Rotated a quarter turn and centred at the left edge: the right half is red.
    s.add(
        0,
        &at(
            0.0,
            140.0,
            100.0,
            40.0,
            &[(
                "backgroundImage",
                "conic-gradient(from 90deg at 0 50%, red 0 50%, blue 0)".into(),
            )],
        ),
    );
    let f = render(s, (100, 180), 1.0, BLACK);
    let red = |p: [u8; 4]| p[0] > 240 && p[1] < 15 && p[2] < 15;
    let blue = |p: [u8; 4]| p[2] > 240 && p[0] < 15 && p[1] < 15;
    let lime = |p: [u8; 4]| p[1] > 240 && p[0] < 15 && p[2] < 15;
    assert!(red(f.px(75, 25)), "top right: {:?}", f.px(75, 25));
    assert!(blue(f.px(75, 75)), "bottom right: {:?}", f.px(75, 75));
    assert!(lime(f.px(25, 50)), "left half: {:?}", f.px(25, 50));
    for (x, is_red) in [
        (5, true),
        (15, false),
        (45, true),
        (55, false),
        (85, true),
        (95, false),
    ] {
        let p = f.px(x, 120);
        assert!(
            if is_red { red(p) } else { blue(p) },
            "stripe at {x}: {p:?}"
        );
    }
    assert!(
        red(f.px(50, 170)),
        "below the centre line: {:?}",
        f.px(50, 170)
    );
    assert!(blue(f.px(50, 150)), "above it: {:?}", f.px(50, 150));
    assert_golden("M3-BG-01", 100, 180, &f.pixels, TOLERANCE);
}

// M3-BG-02
#[test]
fn background_clip_layers_space_and_gradient_text() {
    let mut s = Scene::new();
    // 1. The colour clipped to the content box: border and padding stay clear.
    s.add(
        0,
        &at(
            0.0,
            0.0,
            100.0,
            60.0,
            &[
                ("border", "5px solid white".into()),
                ("padding", 10.0.into()),
                ("backgroundColor", "red".into()),
                ("backgroundClip", "content-box".into()),
            ],
        ),
    );
    // 2. Two layers: lime in the content box over blue in the whole box.
    s.add(
        0,
        &at(
            0.0,
            65.0,
            100.0,
            50.0,
            &[
                ("padding", 10.0.into()),
                (
                    "backgroundImage",
                    "linear-gradient(lime, lime), linear-gradient(blue, blue)".into(),
                ),
                ("backgroundClip", "content-box, border-box".into()),
            ],
        ),
    );
    // 3. Two 20px checker tiles spaced across 50px: a 10px gap between them.
    s.add(
        0,
        &at(
            0.0,
            120.0,
            50.0,
            20.0,
            &[
                ("backgroundImage", "url(checker.png)".into()),
                ("backgroundSize", "20px".into()),
                ("backgroundRepeat", "space no-repeat".into()),
            ],
        ),
    );
    // 4. Gradient text: red to blue inside the glyphs only.
    let text = s.element(
        0,
        "text",
        &at(
            0.0,
            145.0,
            100.0,
            60.0,
            &[
                ("fontSize", 48.0.into()),
                ("color", "transparent".into()),
                (
                    "backgroundImage",
                    "linear-gradient(to right, red, blue)".into(),
                ),
                ("backgroundClip", "text".into()),
            ],
        ),
    );
    s.text(text, "MM");
    let f = render_with_assets(s, (100, 210), 1.0, BLACK, Some(&images_dir()));

    assert_eq!(f.px(2, 30), [255, 255, 255, 255], "border");
    assert_eq!(f.px(10, 30), [0, 0, 0, 255], "padding stays clear");
    assert_eq!(f.px(50, 30), [255, 0, 0, 255], "content");
    assert_eq!(f.px(50, 90), [0, 255, 0, 255], "top layer, content box");
    assert_eq!(f.px(5, 90), [0, 0, 255, 255], "bottom layer, border box");
    assert_eq!(f.px(5, 125), [255, 0, 0, 255], "first tile");
    assert_eq!(f.px(25, 125), [0, 0, 0, 255], "the gap");
    assert_eq!(f.px(35, 125), [255, 0, 0, 255], "second tile at 30px");
    assert_eq!(
        f.px(1, 146),
        [0, 0, 0, 255],
        "no box background behind the text"
    );
    let any = |xs: std::ops::Range<u32>, test: &dyn Fn([u8; 4]) -> bool| {
        xs.flat_map(|x| (145..205).map(move |y| (x, y)))
            .any(|(x, y)| test(f.px(x, y)))
    };
    assert!(
        any(0..30, &|p| p[0] > 150 && p[2] < 100),
        "red glyph pixels on the left"
    );
    assert!(
        any(70..100, &|p| p[2] > 150 && p[0] < 100),
        "blue glyph pixels on the right"
    );
    assert_golden("M3-BG-02", 100, 210, &f.pixels, TOLERANCE);
}

// M3-BG-03
#[test]
fn blend_modes() {
    let mut s = Scene::new();
    // Each row: a backdrop, then a box over it with `mixBlendMode`.
    let rows = [
        ("rgb(200, 100, 50)", "rgb(128, 128, 128)", "multiply"),
        ("rgb(200, 100, 50)", "rgb(0, 0, 255)", "screen"),
        ("white", "rgb(255, 0, 0)", "difference"),
        ("rgb(255, 0, 0)", "rgb(128, 128, 128)", "color"),
    ];
    for (i, (below, above, mode)) in rows.iter().enumerate() {
        let y = i as f64 * 20.0;
        s.add(
            0,
            &at(0.0, y, 100.0, 20.0, &[("backgroundColor", (*below).into())]),
        );
        s.add(
            0,
            &at(
                50.0,
                y,
                50.0,
                20.0,
                &[
                    ("backgroundColor", (*above).into()),
                    ("mixBlendMode", (*mode).into()),
                ],
            ),
        );
    }
    // background-blend-mode: a grey gradient layer multiplied into the colour,
    // isolated from the blue box beneath the element.
    s.add(
        0,
        &at(
            0.0,
            80.0,
            100.0,
            20.0,
            &[("backgroundColor", "blue".into())],
        ),
    );
    s.add(
        0,
        &at(
            50.0,
            80.0,
            50.0,
            20.0,
            &[
                ("backgroundColor", "rgb(200, 100, 50)".into()),
                (
                    "backgroundImage",
                    "linear-gradient(rgb(128, 128, 128), rgb(128, 128, 128))".into(),
                ),
                ("backgroundBlendMode", "multiply".into()),
            ],
        ),
    );
    let f = render(s, (100, 100), 1.0, BLACK);
    let near = |p: [u8; 4], want: [u8; 3]| {
        p[..3]
            .iter()
            .zip(want)
            .all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= 3)
    };
    let check = |x: u32, y: u32, want: [u8; 3], what: &str| {
        assert!(
            near(f.px(x, y), want),
            "{what}: {:?}, want {want:?}",
            f.px(x, y)
        );
    };
    check(25, 10, [200, 100, 50], "backdrop untouched");
    check(75, 10, [100, 50, 25], "multiply");
    check(75, 30, [200, 100, 255], "screen");
    check(75, 50, [0, 255, 255], "difference");
    check(
        75,
        70,
        [77, 77, 77],
        "color: grey's hue at red's luminosity",
    );
    check(
        75,
        90,
        [100, 50, 25],
        "background-blend-mode multiply, isolated",
    );
    assert_golden("M3-BG-03", 100, 100, &f.pixels, TOLERANCE);
}

// M3-BRD-01
#[test]
fn border_styles_and_side_colours() {
    let mut s = Scene::new();
    // 1. One colour per side.
    s.add(
        0,
        &at(
            0.0,
            0.0,
            60.0,
            40.0,
            &[
                ("border", "8px solid".into()),
                ("borderColor", "red lime blue yellow".into()),
            ],
        ),
    );
    // 2. Dashed, 3. dotted, 4. double, 5. inset, along 120px.
    for (i, style) in [
        "6px dashed white",
        "6px dotted white",
        "9px double white",
        "6px inset rgb(200, 200, 200)",
    ]
    .iter()
    .enumerate()
    {
        s.add(
            0,
            &at(
                0.0,
                50.0 + i as f64 * 40.0,
                120.0,
                30.0,
                &[("border", (*style).into())],
            ),
        );
    }
    // 6. Only the top side, from its own shorthand, over a rounded box.
    s.add(
        0,
        &at(
            70.0,
            0.0,
            50.0,
            40.0,
            &[
                ("borderTop", "4px dashed red".into()),
                ("borderRadius", 8.0.into()),
                ("backgroundColor", "rgb(40, 40, 40)".into()),
            ],
        ),
    );
    let f = render(s, (130, 220), 1.0, BLACK);
    assert_eq!(f.px(30, 3), [255, 0, 0, 255], "top");
    assert_eq!(f.px(56, 20), [0, 255, 0, 255], "right");
    assert_eq!(f.px(30, 36), [0, 0, 255, 255], "bottom");
    assert_eq!(f.px(3, 20), [255, 255, 0, 255], "left");

    let row = |y: u32| (10..110).map(|x| f.px(x, y)).collect::<Vec<_>>();
    let white = |p: &[u8; 4]| p[0] > 200;
    let dark = |p: &[u8; 4]| p[0] < 40;
    let dashes = row(53);
    assert!(
        dashes.iter().any(white) && dashes.iter().any(dark),
        "dashes and gaps"
    );
    let dots = row(53 + 40);
    assert!(
        dots.iter().any(white) && dots.iter().any(dark),
        "dots and gaps"
    );
    assert!(white(&f.px(60, 131)), "double: outer line");
    assert!(dark(&f.px(60, 134)), "double: the gap");
    assert!(white(&f.px(60, 137)), "double: inner line");
    let (top, bottom) = (f.luma(60, 172), f.luma(60, 197));
    assert!(
        top + 40.0 < bottom,
        "inset: top {top} darker than bottom {bottom}"
    );
    let top_row: Vec<_> = (78..112).map(|x| f.px(x, 1)).collect();
    assert!(
        top_row.iter().any(|p| p[0] > 200 && p[1] < 40),
        "red dashes on top"
    );
    assert_eq!(f.px(95, 20), [40, 40, 40, 255], "no other sides");
    assert_golden("M3-BRD-01", 130, 220, &f.pixels, TOLERANCE);
}

// M3-BRD-02
#[test]
fn elliptical_corners() {
    let mut s = Scene::new();
    // 50% on a 100x40 box is an ellipse, not a stadium.
    s.add(
        0,
        &at(
            0.0,
            0.0,
            100.0,
            40.0,
            &[
                ("backgroundColor", "white".into()),
                ("borderRadius", "50%".into()),
            ],
        ),
    );
    // Wide, shallow corners with a border following them.
    s.add(
        0,
        &at(
            0.0,
            50.0,
            100.0,
            40.0,
            &[
                ("backgroundColor", "rgb(0, 0, 255)".into()),
                ("border", "4px solid white".into()),
                ("borderRadius", "40px / 12px".into()),
            ],
        ),
    );
    let f = render(s, (100, 90), 1.0, BLACK);
    let black = [0, 0, 0, 255];
    let white = [255, 255, 255, 255];
    assert_eq!(f.px(50, 1), white, "top of the ellipse");
    assert_eq!(f.px(1, 20), white, "left of the ellipse");
    assert_eq!(f.px(12, 5), black, "outside: a stadium would cover this");
    assert_eq!(f.px(3, 55), black, "outside the shallow corner");
    assert_eq!(
        f.px(30, 51),
        white,
        "the border runs along the long, flat corner"
    );
    assert_eq!(f.px(50, 70), [0, 0, 255, 255], "inside");
    assert_golden("M3-BRD-02", 100, 90, &f.pixels, TOLERANCE);
}

// M3-BRD-03
#[test]
fn outlines_sit_outside_the_border_box() {
    let mut s = Scene::new();
    s.add(
        0,
        &at(
            20.0,
            20.0,
            40.0,
            30.0,
            &[
                ("backgroundColor", "white".into()),
                ("outline", "3px solid red".into()),
                ("outlineOffset", 2.0.into()),
            ],
        ),
    );
    // A dashed outline hugging rounded corners, no offset.
    s.add(
        0,
        &at(
            90.0,
            20.0,
            40.0,
            30.0,
            &[
                ("backgroundColor", "white".into()),
                ("borderRadius", 8.0.into()),
                ("outline", "2px dashed lime".into()),
            ],
        ),
    );
    let f = render(s, (150, 70), 1.0, BLACK);
    assert_eq!(f.px(16, 35), [255, 0, 0, 255], "the outline, 2-5px out");
    assert_eq!(f.px(18, 35), [0, 0, 0, 255], "the offset gap");
    assert_eq!(f.px(21, 35), [255, 255, 255, 255], "the box");
    assert_eq!(f.px(40, 15), [255, 0, 0, 255], "above the box too");
    let ring: Vec<_> = (95..125).map(|x| f.px(x, 19)).collect();
    assert!(
        ring.iter().any(|p| p[1] > 200 && p[0] < 40) && ring.iter().any(|p| p[1] < 40),
        "dashes along the top"
    );
    assert_golden("M3-BRD-03", 150, 70, &f.pixels, TOLERANCE);
}

// M3-BRD-04
#[test]
fn border_images_from_images_and_gradients() {
    let mut s = Scene::new();
    // checker.png is 4x4: red top-left and bottom-right quarters, blue elsewhere.
    // Slice 1px: the corners are single texels, the edges the 2px between them.
    s.add(
        0,
        &at(
            0.0,
            0.0,
            60.0,
            40.0,
            &[
                ("border", "10px solid white".into()),
                ("borderImage", "url(checker.png) 1".into()),
            ],
        ),
    );
    // A gradient border, and one with `fill`.
    for (i, fill) in ["", " fill"].iter().enumerate() {
        s.add(
            0,
            &at(
                0.0,
                50.0 + i as f64 * 50.0,
                60.0,
                40.0,
                &[
                    ("border", "6px solid white".into()),
                    (
                        "borderImage",
                        format!("linear-gradient(to right, red, blue) 1{fill}").into(),
                    ),
                ],
            ),
        );
    }
    let f = render_with_assets(s, (60, 150), 1.0, BLACK, Some(&images_dir()));
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let black = [0, 0, 0, 255];
    assert_eq!(f.px(4, 4), red, "top-left corner");
    assert_eq!(f.px(55, 4), blue, "top-right corner");
    assert_eq!(f.px(4, 35), blue, "bottom-left corner");
    assert_eq!(f.px(55, 35), red, "bottom-right corner");
    assert_eq!(f.px(14, 4), red, "top edge: its left texel stretched");
    assert_eq!(f.px(45, 4), blue, "top edge: its right texel stretched");
    assert_eq!(
        f.px(30, 20),
        black,
        "no middle without fill, and no white border"
    );
    let [r0, _, b0, _] = f.px(10, 52);
    let [r1, _, b1, _] = f.px(50, 52);
    assert!(r0 > b0 && b1 > r1, "the gradient runs along the top edge");
    assert_eq!(f.px(30, 70), black, "no middle");
    let [rm, _, bm, _] = f.px(30, 120);
    assert!(
        rm > 60 && bm > 60,
        "fill paints the middle: {:?}",
        f.px(30, 120)
    );
    assert_golden("M3-BRD-04", 60, 150, &f.pixels, TOLERANCE);
}

// M3-CLIP-01
#[test]
fn polygon_and_path_clips() {
    let mut s = Scene::new();
    // A triangle.
    s.add(
        0,
        &at(
            0.0,
            0.0,
            60.0,
            60.0,
            &[
                ("backgroundColor", "white".into()),
                ("clipPath", "polygon(50% 0, 100% 100%, 0 100%)".into()),
            ],
        ),
    );
    // A frame with a hole (evenodd), from path data.
    s.add(
        0,
        &at(
            70.0,
            0.0,
            60.0,
            60.0,
            &[
                ("backgroundColor", "white".into()),
                (
                    "clipPath",
                    "path(evenodd, 'M0 0 H60 V60 H0 Z M15 15 H45 V45 H15 Z')".into(),
                ),
            ],
        ),
    );
    // A half-transparent group (drawn in a layer) clipped to a diamond.
    let group = s.add(
        0,
        &at(
            0.0,
            70.0,
            60.0,
            60.0,
            &[
                ("opacity", 0.5.into()),
                (
                    "clipPath",
                    "polygon(50% 0, 100% 50%, 50% 100%, 0 50%)".into(),
                ),
            ],
        ),
    );
    s.add(
        group,
        &at(0.0, 0.0, 60.0, 60.0, &[("backgroundColor", "white".into())]),
    );
    let f = render(s, (130, 130), 1.0, BLACK);
    let white = [255, 255, 255, 255];
    let black = [0, 0, 0, 255];
    assert_eq!(f.px(30, 50), white, "inside the triangle");
    assert_eq!(f.px(5, 10), black, "outside it");
    assert_eq!(f.px(75, 30), white, "the frame");
    assert_eq!(f.px(100, 30), black, "the hole");
    let half = f.px(30, 100);
    assert!(
        (100..=200).contains(&half[0]),
        "the group at half opacity: {half:?}"
    );
    assert_eq!(f.px(3, 73), black, "outside the diamond");
    assert_golden("M3-CLIP-01", 130, 130, &f.pixels, TOLERANCE);
}

// M3-TXT-10
#[test]
fn text_transform_and_ellipsis() {
    let mut s = Scene::new();
    let style = |y: f64, extra: &[(&'static str, Value)]| {
        let mut v = at(
            4.0,
            y,
            150.0,
            24.0,
            &[
                ("color", "white".into()),
                ("fontSize", 16.0.into()),
                ("whiteSpace", "nowrap".into()),
                ("overflow", "hidden".into()),
            ],
        );
        v.extend_from_slice(extra);
        v
    };
    s.label(
        0,
        "A rather long title that will not fit",
        &style(4.0, &[("textOverflow", "ellipsis".into())]),
    );
    s.label(
        0,
        "small caps-ish heading",
        &style(32.0, &[("textTransform", "uppercase".into())]),
    );
    let f = render(s, (160, 60), 1.0, BLACK);
    assert_golden("M3-TXT-10", 160, 60, &f.pixels, TOLERANCE);
}

// M3-TXT-13
#[test]
fn decoration_styles() {
    let mut s = Scene::new();
    let rows = [
        "underline",
        "underline double",
        "underline dotted",
        "underline dashed",
        "underline wavy rgb(255, 80, 80)",
        "overline line-through 3px",
    ];
    for (i, deco) in rows.iter().enumerate() {
        s.label(
            0,
            "Decorated",
            &at(
                4.0,
                4.0 + i as f64 * 30.0,
                150.0,
                28.0,
                &[
                    ("color", "white".into()),
                    ("fontSize", 18.0.into()),
                    ("textDecoration", (*deco).into()),
                    ("textUnderlineOffset", 4.0.into()),
                ],
            ),
        );
    }
    let f = render(s, (120, 184), 1.0, BLACK);
    // The wavy line is red-ish below its text.
    let wavy_row: Vec<_> = (4..100)
        .flat_map(|x| (134..150).map(move |y| (x, y)))
        .map(|(x, y)| f.px(x, y))
        .collect();
    assert!(
        wavy_row.iter().any(|p| p[0] > 200 && p[1] < 120),
        "a red wave"
    );
    assert_golden("M3-TXT-13", 120, 184, &f.pixels, TOLERANCE);
}

// M3-TXT-14
#[test]
fn outlined_text() {
    let mut s = Scene::new();
    s.label(
        0,
        "Hollow",
        &at(
            4.0,
            0.0,
            150.0,
            40.0,
            &[
                ("color", "transparent".into()),
                ("fontSize", 32.0.into()),
                ("fontWeight", 800.0.into()),
                ("WebkitTextStroke", "1px white".into()),
            ],
        ),
    );
    s.label(
        0,
        "Filled",
        &at(
            4.0,
            44.0,
            150.0,
            40.0,
            &[
                ("color", "rgb(255, 60, 60)".into()),
                ("fontSize", 32.0.into()),
                ("fontWeight", 800.0.into()),
                ("textStroke", "2px white".into()),
            ],
        ),
    );
    let f = render(s, (130, 88), 1.0, BLACK);
    let pixels: Vec<[u8; 4]> = (0..130)
        .flat_map(|x| (0..40).map(move |y| (x, y)))
        .map(|(x, y)| f.px(x, y))
        .collect();
    assert!(
        pixels.iter().any(|p| p[0] > 200 && p[1] > 200),
        "white outlines"
    );
    let filled: Vec<[u8; 4]> = (0..130)
        .flat_map(|x| (44..84).map(move |y| (x, y)))
        .map(|(x, y)| f.px(x, y))
        .collect();
    assert!(
        filled.iter().any(|p| p[0] > 200 && p[1] < 100),
        "red fill inside"
    );
    assert!(filled.iter().any(|p| p[1] > 200), "white stroke around it");
    assert_golden("M3-TXT-14", 130, 88, &f.pixels, TOLERANCE);
}

// M3-TXT-15
#[test]
fn unsmoothed_text_has_hard_edges() {
    let mut s = Scene::new();
    for (i, smooth) in ["auto", "never"].iter().enumerate() {
        s.label(
            0,
            "Pixel",
            &at(
                4.0,
                i as f64 * 30.0,
                100.0,
                28.0,
                &[
                    ("color", "white".into()),
                    ("fontSize", 20.0.into()),
                    ("fontSmooth", (*smooth).into()),
                ],
            ),
        );
    }
    let f = render(s, (80, 60), 1.0, BLACK);
    let grey = |y0: u32| {
        (0..80)
            .flat_map(|x| (y0..y0 + 28).map(move |y| (x, y)))
            .filter(|&(x, y)| (20..235).contains(&f.px(x, y)[0]))
            .count()
    };
    assert!(grey(0) > 10, "smoothed edges are grey");
    assert_eq!(grey(30), 0, "unsmoothed glyphs are all or nothing");
    assert_golden("M3-TXT-15", 80, 60, &f.pixels, TOLERANCE);
}

// M3-FX-02
#[test]
fn filter_functions_and_drop_shadows() {
    let mut s = Scene::new();
    // Colour functions on solid boxes, one 30px column each.
    let cols = [
        ("red", "grayscale(1)"),
        ("red", "brightness(0.5)"),
        ("blue", "invert(1)"),
        ("red", "hue-rotate(180deg)"),
        ("rgb(128, 128, 128)", "sepia(1)"),
    ];
    for (i, (color, filter)) in cols.iter().enumerate() {
        s.add(
            0,
            &at(
                i as f64 * 30.0,
                0.0,
                30.0,
                30.0,
                &[
                    ("backgroundColor", (*color).into()),
                    ("filter", (*filter).into()),
                ],
            ),
        );
    }
    // A drop shadow follows the shape (a circle), unlike a box shadow.
    s.add(
        0,
        &at(
            10.0,
            40.0,
            30.0,
            30.0,
            &[
                ("backgroundColor", "white".into()),
                ("borderRadius", "50%".into()),
                ("filter", "drop-shadow(8px 8px 0 lime)".into()),
            ],
        ),
    );
    // A backdrop filter greys out what's behind its box.
    s.add(
        0,
        &at(
            70.0,
            40.0,
            80.0,
            40.0,
            &[(
                "backgroundImage",
                "linear-gradient(to right, red, blue)".into(),
            )],
        ),
    );
    s.add(
        0,
        &at(
            110.0,
            40.0,
            40.0,
            40.0,
            &[("backdropFilter", "grayscale(1)".into())],
        ),
    );
    let f = render(s, (150, 90), 1.0, BLACK);
    let near = |p: [u8; 4], want: [u8; 3]| {
        p[..3]
            .iter()
            .zip(want)
            .all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= 3)
    };
    let check = |x: u32, y: u32, want: [u8; 3], what: &str| {
        assert!(
            near(f.px(x, y), want),
            "{what}: {:?}, want {want:?}",
            f.px(x, y)
        );
    };
    check(15, 15, [54, 54, 54], "grayscale red");
    check(45, 15, [128, 0, 0], "half-bright red");
    check(75, 15, [255, 255, 0], "inverted blue");
    let [r, g, b, _] = f.px(105, 15);
    assert!(
        g > r && b > r,
        "hue-rotated red turns cyan-ish: {:?}",
        f.px(105, 15)
    );
    let [r, g, b, _] = f.px(135, 15);
    assert!(r > g && g > b, "sepia grey is warm: {:?}", f.px(135, 15));
    check(25, 55, [255, 255, 255], "the circle");
    check(41, 66, [0, 255, 0], "its shadow, offset");
    check(12, 42, [0, 0, 0], "no shadow in the circle's empty corner");
    let [r, g, b, _] = f.px(130, 60);
    assert!(
        r == g && g == b,
        "grey behind the backdrop filter: {:?}",
        f.px(130, 60)
    );
    let [r, _, b, _] = f.px(90, 60);
    assert!(r > b + 50, "colour outside it: {:?}", f.px(90, 60));
    assert_golden("M3-FX-02", 150, 90, &f.pixels, TOLERANCE);
}

// M3-FX-03
#[test]
fn pixelated_images_scale_without_smoothing() {
    let mut s = Scene::new();
    for (i, rendering) in ["auto", "pixelated"].iter().enumerate() {
        s.add(
            0,
            &at(
                i as f64 * 50.0,
                0.0,
                40.0,
                40.0,
                &[
                    ("backgroundImage", "url(checker.png)".into()),
                    ("backgroundSize", "40px".into()),
                    ("imageRendering", (*rendering).into()),
                ],
            ),
        );
    }
    let f = render_with_assets(s, (90, 40), 1.0, BLACK, Some(&images_dir()));
    let pure = |p: [u8; 4]| p == [255, 0, 0, 255] || p == [0, 0, 255, 255];
    // Around the middle of the top row, where the red and blue squares meet.
    let smooth: Vec<_> = (14..26).map(|x| f.px(x, 5)).collect();
    let crisp: Vec<_> = (64..76).map(|x| f.px(x, 5)).collect();
    assert!(
        smooth.iter().any(|p| !pure(*p)),
        "filtered: blends {smooth:?}"
    );
    assert!(
        crisp.iter().all(|p| pure(*p)),
        "nearest: no blends {crisp:?}"
    );
    assert_golden("M3-FX-03", 90, 40, &f.pixels, TOLERANCE);
}

// M3-FX-04
#[test]
fn masks_from_gradients() {
    let mut s = Scene::new();
    let masked = |y: f64, extra: &[(&'static str, Value)]| {
        let mut v = at(0.0, y, 100.0, 30.0, &[("backgroundColor", "white".into())]);
        v.extend_from_slice(extra);
        v
    };
    // Alpha: opaque on the left, gone on the right.
    s.add(
        0,
        &masked(
            0.0,
            &[(
                "maskImage",
                "linear-gradient(to right, black, transparent)".into(),
            )],
        ),
    );
    // Luminance: white shows, black hides.
    s.add(
        0,
        &masked(
            35.0,
            &[
                (
                    "maskImage",
                    "linear-gradient(to right, white, black)".into(),
                ),
                ("maskMode", "luminance".into()),
            ],
        ),
    );
    // A repeated dot mask over a box and its child.
    let dots = s.add(
        0,
        &masked(
            70.0,
            &[
                (
                    "maskImage",
                    "radial-gradient(circle, black 40%, transparent 42%)".into(),
                ),
                ("maskSize", "10px 10px".into()),
            ],
        ),
    );
    s.add(
        dots,
        &at(50.0, 0.0, 50.0, 30.0, &[("backgroundColor", "red".into())]),
    );
    let f = render(s, (100, 100), 1.0, BLACK);
    assert!(f.luma(3, 15) > 240.0, "alpha: left shows");
    // Platypus composites in linear light, so the faint edge of the fade reads brighter
    // than a browser's sRGB blend would.
    assert!(f.luma(97, 15) < 50.0, "alpha: right nearly gone");
    assert!(f.luma(3, 50) > 240.0, "luminance: white shows");
    assert!(f.luma(97, 50) < 50.0, "luminance: black nearly hides");
    assert!(f.luma(5, 75) > 200.0, "a dot centre");
    assert!(f.luma(0, 70) < 30.0, "between dots");
    assert_eq!(
        f.px(55, 75)[0],
        255,
        "the child is masked too: a dot of red"
    );
    assert_eq!(f.px(50, 70), [0, 0, 0, 255], "and hidden between dots");
    assert_golden("M3-FX-04", 100, 100, &f.pixels, TOLERANCE);
}

// M3-BG-04
#[test]
fn gradients_follow_background_size_position_and_repeat() {
    let mut s = Scene::new();
    // A 20px tile, repeated: hard red/blue halves every 10px.
    s.add(
        0,
        &at(
            0.0,
            0.0,
            100.0,
            20.0,
            &[
                (
                    "backgroundImage",
                    "linear-gradient(to right, red 50%, blue 50%)".into(),
                ),
                ("backgroundSize", "20px 20px".into()),
            ],
        ),
    );
    // One tile, not repeated, at the right.
    s.add(
        0,
        &at(
            0.0,
            30.0,
            100.0,
            20.0,
            &[
                ("backgroundColor", "white".into()),
                ("backgroundImage", "linear-gradient(lime, lime)".into()),
                ("backgroundSize", "20px 20px".into()),
                ("backgroundPosition", "right".into()),
                ("backgroundRepeat", "no-repeat".into()),
            ],
        ),
    );
    let f = render(s, (100, 50), 1.0, BLACK);
    for (x, red) in [(5, true), (15, false), (45, true), (55, false), (85, true)] {
        let p = f.px(x, 10);
        assert!(if red { p[0] > 240 } else { p[2] > 240 }, "x {x}: {p:?}");
    }
    assert_eq!(f.px(90, 40), [0, 255, 0, 255], "the tile at the right");
    assert_eq!(f.px(70, 40), [255, 255, 255, 255], "nothing outside it");
    assert_golden("M3-BG-04", 100, 50, &f.pixels, TOLERANCE);
}

// M3-XF-02
#[test]
fn transforms_draw_rotated_skewed_and_in_perspective() {
    let mut s = Scene::new();
    // 1. Rotated, rounded, bordered, with text that turns with it.
    let a = s.add(
        0,
        &at(
            20.0,
            20.0,
            80.0,
            50.0,
            &[
                ("backgroundColor", "rgb(40, 120, 220)".into()),
                ("border", "3px solid white".into()),
                ("borderRadius", 10.0.into()),
                ("transform", "rotate(30deg)".into()),
            ],
        ),
    );
    s.label(
        a,
        "Turn",
        &[
            ("color", "white".into()),
            ("fontSize", 18.0.into()),
            ("margin", 8.0.into()),
        ],
    );
    // 2. Skewed gradient.
    s.add(
        0,
        &at(
            140.0,
            20.0,
            80.0,
            50.0,
            &[
                (
                    "backgroundImage",
                    "linear-gradient(to right, orange, purple)".into(),
                ),
                ("transform", "skewX(-20deg)".into()),
            ],
        ),
    );
    // 3. A card turned in perspective (the parent's `perspective`), with a child
    //    clipped by the card's rounded overflow.
    let stage = s.add(
        0,
        &at(0.0, 100.0, 120.0, 90.0, &[("perspective", "200px".into())]),
    );
    let card = s.add(
        stage,
        &at(
            20.0,
            10.0,
            80.0,
            70.0,
            &[
                ("backgroundColor", "rgb(30, 160, 90)".into()),
                ("borderRadius", 12.0.into()),
                ("overflow", "hidden".into()),
                ("transform", "rotateY(50deg)".into()),
            ],
        ),
    );
    s.add(
        card,
        &at(
            40.0,
            35.0,
            80.0,
            80.0,
            &[("backgroundColor", "yellow".into())],
        ),
    );
    // 4. Scaled text from its top-left corner.
    s.label(
        0,
        "Big",
        &at(
            140.0,
            110.0,
            60.0,
            24.0,
            &[
                ("color", "white".into()),
                ("fontSize", 16.0.into()),
                ("transform", "scale(2)".into()),
                ("transformOrigin", "left top".into()),
            ],
        ),
    );
    let f = render(s, (240, 200), 1.0, BLACK);
    assert_golden("M3-XF-02", 240, 200, &f.pixels, TOLERANCE);
    let blue = |p: [u8; 4]| p[2] > 180 && p[0] < 90;
    assert!(
        blue(f.px(52, 58)),
        "inside the rotated box: {:?}",
        f.px(52, 58)
    );
    assert_eq!(
        f.px(22, 22),
        [0, 0, 0, 255],
        "its layout corner is empty once turned"
    );
    let [r, g, b, _] = f.px(40, 130);
    assert!(
        g > r && g > b,
        "the card in perspective: {:?}",
        f.px(40, 130)
    );
    assert_eq!(
        f.px(25, 112),
        [0, 0, 0, 255],
        "the turned card's near edge moved in"
    );
    let text: Vec<_> = (140..236)
        .flat_map(|x| (110..158).map(move |y| (x, y)))
        .collect();
    assert!(
        text.iter().any(|&(x, y)| f.px(x, y)[0] > 200 && y > 134),
        "scaled text reaches below its layout box"
    );
}

// M3-SEL-03
#[test]
fn text_selection_and_placeholder_styles() {
    use platypus_ui::editor::PointerSelect;
    use platypus_ui::events::Modifiers;
    let mut s = Scene::new();
    s.css = ".warm::selection { background: #ffd54a; color: #202020 }
        input::placeholder { color: #ff8a80; font-style: italic }";
    let field_style = |y: f64| {
        at(
            10.0,
            y,
            220.0,
            28.0,
            &[
                ("color", "white".into()),
                ("fontSize", 16.0.into()),
                ("padding", 4.0.into()),
                ("backgroundColor", "#303848".into()),
                ("borderRadius", 6.0.into()),
            ],
        )
    };
    // A focused field with all its text selected: the default highlight.
    let filled = s.element(0, "input", &field_style(6.0));
    s.attrs(filled, &[("value", "Selected field text".into())]);
    // Static text dragged across, restyled by an ancestor's ::selection.
    let warm = s.element(0, "box", &at(0.0, 42.0, 320.0, 30.0, &[]));
    s.attrs(warm, &[("className", "warm".into())]);
    let line = s.label(
        warm,
        "Dragged across with ::selection",
        &at(
            10.0,
            0.0,
            300.0,
            24.0,
            &[("color", "white".into()), ("fontSize", 18.0.into())],
        ),
    );
    // An empty field shows its ::placeholder style.
    let empty = s.element(0, "input", &field_style(76.0));
    s.attrs(empty, &[("placeholder", "Player name".into())]);
    s.prepare = Some(Box::new(move |tree: &mut UiTree| {
        let id = web_ui_core::NodeId;
        tree.set_state(
            id(filled),
            platypus_ui::ElementState {
                focus: true,
                ..Default::default()
            },
        );
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        tree.editor_key(id(filled), "a", "KeyA", None, ctrl)
            .unwrap();
        // Caret blink off, so only the selection shows.
        tree.set_time(600.0);
        let press = PointerSelect::Press {
            clicks: 1,
            shift: false,
        };
        assert!(tree.select_text(id(line), 75.0, 52.0, press));
        tree.select_text(id(line), 270.0, 52.0, PointerSelect::Drag);
        let picked = tree.text_selection().unwrap().1;
        assert!(picked.contains("across"), "{picked:?}");
    }));
    let f = render(s, (320, 110), 1.0, BLACK);
    assert_golden("M3-SEL-03", 320, 110, &f.pixels, TOLERANCE);
    // The ::selection background is yellow under the dragged words.
    let [r, g, b, _] = f.px(200, 46);
    assert!(r > 200 && g > 170 && b < 120, "{:?}", f.px(200, 46));
}

// M3-ANI-06
#[test]
fn animation_filmstrip_and_mid_transition() {
    let mut s = Scene::new();
    s.css = "@keyframes morph {
            from { background-color: #e04040; transform: rotate(0deg); border-radius: 0px }
            to { background-color: #4060ff; transform: rotate(90deg); border-radius: 20px }
        }
        .frame { animation: morph 1s linear paused }";
    // One animation sampled at 0, 25, 50, 75 and ~100% via negative delays.
    for (i, delay) in ["0s", "-0.25s", "-0.5s", "-0.75s", "-0.999s"]
        .iter()
        .enumerate()
    {
        let b = s.add(
            0,
            &at(
                10.0 + i as f64 * 60.0,
                10.0,
                40.0,
                40.0,
                &[("animationDelay", (*delay).into())],
            ),
        );
        s.attrs(b, &[("className", "frame".into())]);
    }
    // A transition caught halfway: width 40 -> 280, green -> white, linear.
    let bar = s.add(
        0,
        &at(
            10.0,
            70.0,
            40.0,
            24.0,
            &[
                ("backgroundColor", "#30c060".into()),
                (
                    "transition",
                    "width 1s linear, background-color 1s linear".into(),
                ),
            ],
        ),
    );
    s.prepare = Some(Box::new(move |tree: &mut UiTree| {
        let n = |v: u32| Value::Number(v.into());
        tree.set_time(0.0);
        tree.layout_if_needed();
        let errors = tree.apply_commit(&Commit {
            ops: vec![Value::Array(vec![
                n(SET_STYLE),
                n(bar),
                "width".into(),
                280.0.into(),
                "backgroundColor".into(),
                "white".into(),
            ])],
        });
        assert!(errors.is_empty(), "{errors:?}");
        tree.layout_if_needed();
        tree.set_time(500.0);
    }));
    let f = render(s, (320, 104), 1.0, BLACK);
    assert_golden("M3-ANI-06", 320, 104, &f.pixels, TOLERANCE);
    // Halfway: the bar is 160 wide and between green and white.
    assert!(f.luma(165, 82) > 100.0 && f.luma(175, 82) < 20.0);
}

// M3-GAME-01
#[test]
fn image_tints() {
    let mut s = Scene::new();
    let tiled = |x: f64, tint: &str| {
        at(
            x,
            10.0,
            60.0,
            60.0,
            &[
                ("backgroundImage", "url(gray.png)".into()),
                ("backgroundImageTint", tint.into()),
            ],
        )
    };
    s.add(0, &tiled(10.0, "none"));
    s.add(0, &tiled(80.0, "#ff4040"));
    s.add(0, &tiled(150.0, "rgba(64, 128, 255, 0.5)"));
    s.add(
        0,
        &at(
            220.0,
            10.0,
            60.0,
            60.0,
            &[
                ("borderWidth", 12.0.into()),
                ("borderImage", "url(gray.png) 4 round".into()),
                ("borderImageTint", "#40ff80".into()),
            ],
        ),
    );
    let img = s.element(
        0,
        "image",
        &at(
            290.0,
            10.0,
            60.0,
            60.0,
            &[("backgroundImageTint", "#ffd040".into())],
        ),
    );
    s.attrs(img, &[("src", "gray.png".into())]);
    let f = render_with_assets(s, (360, 80), 1.0, BLACK, Some(&images_dir()));
    assert_golden("M3-GAME-01", 360, 80, &f.pixels, TOLERANCE);
    // The red-tinted tiles have no green or blue left.
    let [r, g, b, _] = f.px(85, 15);
    assert!(r > 200 && g < 90 && b < 90, "{:?}", f.px(85, 15));
}

// M3-SVG-01
#[test]
fn svg_images_stay_sharp_at_any_size() {
    let mut s = Scene::new();
    // At its natural 32px, as a background.
    s.add(
        0,
        &at(
            10.0,
            10.0,
            32.0,
            32.0,
            &[("backgroundImage", "url(icon.svg)".into())],
        ),
    );
    // Scaled up 3x as an image element: rasterized at 96px, not stretched.
    let img = s.element(0, "image", &at(60.0, 10.0, 96.0, 96.0, &[]));
    s.attrs(img, &[("src", "icon.svg".into())]);
    // A data: URL, tiled.
    let data = "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='8' height='8'%3E%3Ccircle cx='4' cy='4' r='3' fill='%2340c0ff'/%3E%3C/svg%3E\")";
    s.add(
        0,
        &at(170.0, 10.0, 80.0, 96.0, &[("backgroundImage", data.into())]),
    );
    let f = render_with_assets(s, (260, 116), 1.0, BLACK, Some(&images_dir()));
    assert_golden("M3-SVG-01", 260, 116, &f.pixels, TOLERANCE);
    // The big icon's check mark edge is crisp: dark and bright pixels sit close.
    let center = f.px(95, 30);
    assert!(center[0] > 200, "gradient fill: {center:?}");
    let [r, g, b, _] = f.px(174, 14);
    assert!(
        b > 200 && r < 120 && g > 120,
        "tiled data: SVG dots: {:?}",
        [r, g, b]
    );
}

// M3-SVG-03
#[test]
fn inline_svg_with_css_styling() {
    let mut s = Scene::new();
    s.css = "svg { color: #7cf09a }
        .ring { fill: none; stroke: currentColor; stroke-width: 3; stroke-dasharray: 6 3 }
        .blob { fill: url(#sky); clip-path: url(#half) }
        .badge { fill: #ffd54a; stroke: #202020; stroke-width: 1.5; transform: rotate(15deg); transform-origin: 75px 25px }
        text { fill: white; font-size: 11px }";
    let svg = s.element(0, "svg", &at(10.0, 10.0, 200.0, 100.0, &[]));
    s.attrs(svg, &[("viewBox", "0 0 100 50".into())]);
    let defs = s.element(svg, "defs", &[]);
    let grad = s.element(defs, "linearGradient", &[]);
    s.attrs(
        grad,
        &[("id", "sky".into()), ("x2", "0".into()), ("y2", "1".into())],
    );
    for (offset, color) in [("0", "#5ab0ff"), ("1", "#8040e0")] {
        let stop = s.element(grad, "stop", &[]);
        s.attrs(
            stop,
            &[("offset", offset.into()), ("stopColor", color.into())],
        );
    }
    let clip = s.element(defs, "clipPath", &[]);
    s.attrs(clip, &[("id", "half".into())]);
    let r = s.element(clip, "rect", &[]);
    s.attrs(
        r,
        &[
            ("x", 0.0.into()),
            ("y", 0.0.into()),
            ("width", 100.0.into()),
            ("height", 32.0.into()),
        ],
    );
    let ring = s.element(svg, "circle", &[]);
    s.attrs(
        ring,
        &[
            ("cx", 25.0.into()),
            ("cy", 25.0.into()),
            ("r", 20.0.into()),
            ("className", "ring".into()),
        ],
    );
    let blob = s.element(svg, "ellipse", &[]);
    s.attrs(
        blob,
        &[
            ("id", "blob".into()),
            ("cx", 25.0.into()),
            ("cy", 25.0.into()),
            ("rx", 14.0.into()),
            ("ry", 14.0.into()),
            ("className", "blob".into()),
        ],
    );
    let badge = s.element(svg, "polygon", &[]);
    s.attrs(
        badge,
        &[
            ("points", "60,10 90,10 90,40 60,40".into()),
            ("className", "badge".into()),
        ],
    );
    let label = s.element(svg, "text", &[]);
    s.attrs(label, &[("x", 62.0.into()), ("y", 29.0.into())]);
    s.text(label, "SVG");
    let f = render(s, (220, 120), 2.0, BLACK);
    assert_golden("M3-SVG-03", 440, 240, &f.pixels, TOLERANCE);
    // The dashed ring is green (currentColor) and the clip cuts the blob's bottom.
    let ring = (100..140)
        .map(|x| f.px(x, 40))
        .max_by_key(|p| p[1])
        .unwrap();
    assert!(ring[1] > 150 && ring[0] < 180, "ring: {ring:?}");
    assert!(f.luma(120, 165) < 20.0, "clipped: {:?}", f.px(120, 165));
    assert!(f.px(120, 120)[2] > 150, "the gradient above the clip edge");
}

// M3-DLG-02
#[test]
fn modal_dialog_over_a_blurred_backdrop() {
    let mut s = Scene::new();
    s.css = "dialog::backdrop { background: rgba(0, 0, 0, 0.4); backdrop-filter: blur(3px) }";
    // A busy page: stripes and a label.
    for i in 0..10 {
        let color = if i % 2 == 0 { "#e04040" } else { "#40c0e0" };
        s.add(
            0,
            &at(
                f64::from(i) * 20.0,
                0.0,
                20.0,
                120.0,
                &[("backgroundColor", color.into())],
            ),
        );
    }
    s.label(
        0,
        "Game",
        &at(
            8.0,
            90.0,
            80.0,
            24.0,
            &[("color", "white".into()), ("fontSize", 20.0.into())],
        ),
    );
    let dialog = s.element(
        0,
        "dialog",
        &[
            ("width", 110.0.into()),
            ("height", 50.0.into()),
            ("backgroundColor", "white".into()),
            ("borderRadius", 8.0.into()),
            ("justifyContent", "center".into()),
            ("alignItems", "center".into()),
        ],
    );
    s.label(
        dialog,
        "Paused",
        &[("fontSize", 18.0.into()), ("color", "#202020".into())],
    );
    s.attrs(dialog, &[("open", true.into()), ("modal", true.into())]);
    let f = render(s, (200, 120), 1.0, BLACK);
    assert_golden("M3-DLG-02", 200, 120, &f.pixels, TOLERANCE);
    // The dialog is centred and white; the page behind is darkened and blurred, so a
    // stripe edge no longer jumps from red to blue.
    assert_eq!(f.px(100, 38)[..3], [255, 255, 255]);
    let [r, _, b, _] = f.px(10, 10);
    assert!(r < 200 && b < 100, "darkened red: {:?}", f.px(10, 10));
    let edge = |x| f.px(x, 10)[0] as i32;
    assert!(
        (edge(19) - edge(20)).abs() < 60,
        "blurred: {} {}",
        edge(19),
        edge(20)
    );
}

// M3-LST-01
#[test]
fn list_markers() {
    let mut s = Scene::new();
    let list = |s: &mut Scene, x: f64, style: &[(&'static str, Value)], text_items: bool| {
        let mut st = at(
            x,
            4.0,
            110.0,
            120.0,
            &[("color", "white".into()), ("fontSize", 14.0.into())],
        );
        st.extend_from_slice(style);
        let l = s.element(0, "list", &st);
        for word in ["Alpha", "Beta", "Gamma"] {
            if text_items {
                s.label(l, word, &[]);
            } else {
                let item = s.add(l, &[("height", 22.0.into())]);
                s.label(item, word, &[]);
            }
        }
    };
    list(&mut s, 0.0, &[], false);
    list(&mut s, 110.0, &[("listStyleType", "decimal".into())], false);
    list(
        &mut s,
        220.0,
        &[
            ("listStyleType", "upper-roman".into()),
            ("listStylePosition", "inside".into()),
        ],
        false,
    );
    list(
        &mut s,
        330.0,
        &[("listStyleType", "'\u{2192}'".into())],
        true,
    );
    list(
        &mut s,
        440.0,
        &[("listStyleImage", "url(checker.png)".into())],
        false,
    );
    list(&mut s, 550.0, &[("listStyleType", "none".into())], false);
    let f = render_with_assets(s, (660, 90), 1.0, BLACK, Some(&images_dir()));
    assert_golden("M3-LST-01", 660, 90, &f.pixels, TOLERANCE);
    // Markers sit left of the first item's text, in the list's padding.
    let ink = |x0: u32, x1: u32| {
        (x0..x1)
            .flat_map(|x| (4..26).map(move |y| (x, y)))
            .any(|(x, y)| f.luma(x, y) > 120.0)
    };
    assert!(ink(4, 24), "a disc in the padding");
    assert!(ink(114, 134), "a number in the padding");
    // `none`: no marker and no indent, so the text starts at the list's edge.
    assert!(ink(550, 556), "`none`: flush with the list's left edge");
}

// M2-DRW-06
#[test]
fn outer_and_inset_shadows() {
    let mut s = Scene::new();
    s.add(
        0,
        &at(
            0.0,
            0.0,
            200.0,
            100.0,
            &[("backgroundColor", "#eeeeee".into())],
        ),
    );
    s.add(
        0,
        &at(
            30.0,
            20.0,
            40.0,
            40.0,
            &[
                ("backgroundColor", "white".into()),
                ("borderRadius", 8.0.into()),
                ("boxShadow", "0 8px 16px rgba(0,0,0,0.6)".into()),
            ],
        ),
    );
    s.add(
        0,
        &at(
            120.0,
            20.0,
            60.0,
            60.0,
            &[
                ("backgroundColor", "white".into()),
                ("boxShadow", "inset 0 0 12px 2px rgba(0,0,0,0.8)".into()),
            ],
        ),
    );
    let f = render(s, (200, 100), 1.0, BLACK);
    let bg = f.luma(5, 5);
    assert!(f.luma(50, 64) < bg - 20.0, "shadow below the box");
    assert!(
        f.luma(50, 64) < f.luma(50, 85),
        "shadow fades with distance"
    );
    assert!(
        (f.luma(100, 5) - bg).abs() < 2.0,
        "far from the box is untouched"
    );
    assert_eq!(
        f.px(50, 40),
        [255, 255, 255, 255],
        "the box covers its own shadow"
    );
    // Inset: dark at the inner edge, white in the middle.
    // At the inner edge the hole (shrunk by the 2px spread) is ~2px away; with
    // sigma = 6 its blurred coverage is ~0.37, so the shadow is ~0.8 * 0.63 = 50%
    // black over white: sRGB ~188.
    let edge = f.luma(120, 50);
    assert!(
        (175.0..205.0).contains(&edge),
        "inset shadow at the edge: {edge}"
    );
    assert!(
        f.luma(120, 50) < f.luma(126, 50) && f.luma(126, 50) < f.luma(134, 50),
        "inset shadow fades inwards"
    );
    assert!(f.luma(150, 50) > 250.0, "inset shadow fades to the centre");
    assert_golden("M2-DRW-06", 200, 100, &f.pixels, TOLERANCE);
}

// M2-DRW-07
#[test]
fn filter_blur_blurs_the_box_and_children() {
    let mut s = Scene::new();
    s.add(
        0,
        &at(
            10.0,
            20.0,
            40.0,
            40.0,
            &[("backgroundColor", "white".into())],
        ),
    );
    let blurred = s.add(
        0,
        &at(
            80.0,
            20.0,
            40.0,
            40.0,
            &[
                ("backgroundColor", "white".into()),
                ("filter", "blur(4px)".into()),
            ],
        ),
    );
    // A child inside the blurred box is blurred too.
    s.add(
        blurred,
        &at(10.0, 10.0, 20.0, 20.0, &[("backgroundColor", "red".into())]),
    );
    let f = render(s, (140, 80), 1.0, BLACK);

    assert_eq!(f.stats.layers, 1);
    // Sharp box: hard edge.
    assert_eq!(f.px(9, 40), [0, 0, 0, 255]);
    assert_eq!(f.px(10, 40), [255, 255, 255, 255]);
    // Blurred box: the edge is soft on both sides.
    let outside = f.luma(78, 40);
    let inside = f.luma(82, 40);
    assert!(
        outside > 20.0 && outside < 200.0,
        "blur spills outside: {outside}"
    );
    assert!(
        inside > 50.0 && inside < 250.0,
        "blur softens inside: {inside}"
    );
    // The red child is softened but still red-dominant at its centre.
    let [r, g, _, _] = f.px(100, 40);
    assert!(r > g + 60, "child is still red: {:?}", f.px(100, 40));
    assert!(
        g > 10,
        "child edges blend with the white box: {:?}",
        f.px(100, 40)
    );
    // Well outside the layer bounds nothing changes.
    assert_eq!(f.px(135, 5), [0, 0, 0, 255]);
    assert_golden("M2-DRW-07", 140, 80, &f.pixels, TOLERANCE);
}

// M2-DRW-08
#[test]
fn backdrop_blur_blurs_only_behind_the_box() {
    let mut s = Scene::new();
    // Black and white stripes, 4px wide.
    for i in 0..30 {
        s.add(
            0,
            &at(
                f64::from(i) * 4.0,
                0.0,
                4.0,
                80.0,
                &[(
                    "backgroundColor",
                    (if i % 2 == 0 { "white" } else { "black" }).into(),
                )],
            ),
        );
    }
    // Frosted glass over the middle, with rounded corners.
    s.add(
        0,
        &at(
            30.0,
            20.0,
            60.0,
            40.0,
            &[
                ("backdropFilter", "blur(6px)".into()),
                ("backgroundColor", "rgba(255,255,255,0.1)".into()),
                ("borderRadius", 10.0.into()),
            ],
        ),
    );
    let f = render(s, (120, 80), 1.0, BLACK);
    let contrast = |y: u32, xs: std::ops::Range<u32>| {
        let l: Vec<f32> = xs.map(|x| f.luma(x, y)).collect();
        l.iter().cloned().fold(f32::MIN, f32::max) - l.iter().cloned().fold(f32::MAX, f32::min)
    };
    assert!(
        contrast(5, 40..80) > 200.0,
        "stripes outside the glass stay sharp"
    );
    assert!(
        contrast(40, 40..80) < 40.0,
        "stripes behind the glass are blurred"
    );
    // The rounded corner clips the blur: just inside the box's corner square but
    // outside the radius, stripes are still sharp.
    assert!(
        [(30, 20), (31, 20), (30, 21)]
            .iter()
            .any(|&(x, y)| f.luma(x, y) > 250.0 || f.luma(x, y) < 5.0),
        "corner outside the radius is unblurred"
    );
    assert_golden("M2-DRW-08", 120, 80, &f.pixels, TOLERANCE);
}

// M2-DRW-11
#[test]
fn radial_gradients_fade_from_transparent_premultiplied() {
    let mut s = Scene::new();
    s.add(
        0,
        &at(
            0.0,
            0.0,
            96.0,
            96.0,
            &[(
                "backgroundImage",
                "radial-gradient(circle, transparent 44px, #ff0000 52px)".into(),
            )],
        ),
    );
    let f = render(s, (96, 96), 1.0, Color::rgb(1.0, 1.0, 1.0));
    // Along the diagonal from the centre (48, 48), `d` px out.
    let at = |d: f32| {
        let o = (48.0 - d / std::f32::consts::SQRT_2).round() as u32;
        f.px(o, o)
    };
    for d in [0.0, 20.0, 40.0] {
        assert_eq!(at(d), [255, 255, 255, 255], "{d}px out");
    }
    // Halfway, white and red mix without darkening: `transparent` is transparent red there.
    let mid = at(48.0);
    assert!(mid[0] >= 250 && mid[1] > 60 && mid[1] < 230, "{mid:?}");
    assert_eq!(at(60.0), [255, 0, 0, 255]);
}

// M2-DRW-10
#[test]
fn backdrop_blur_reads_only_inside_the_box() {
    let mut s = Scene::new();
    // Blue behind the top glass, orange behind the bottom one (Fluid's stacked menu glass).
    s.add(
        0,
        &at(
            0.0,
            0.0,
            120.0,
            28.0,
            &[("backgroundColor", "#2060ff".into())],
        ),
    );
    s.add(
        0,
        &at(
            0.0,
            28.0,
            120.0,
            52.0,
            &[("backgroundColor", "#ff8020".into())],
        ),
    );
    for (top, height, alpha) in [(0.0, 28.0, 0.35), (28.0, 52.0, 0.62)] {
        s.add(
            0,
            &at(
                0.0,
                top,
                120.0,
                height,
                &[
                    ("backdropFilter", "blur(10px) saturate(1.6)".into()),
                    (
                        "backgroundColor",
                        format!("rgba(246,246,250,{alpha})").as_str().into(),
                    ),
                ],
            ),
        );
    }
    let f = render(s, (120, 80), 1.0, BLACK);
    // The bottom glass is as orange at its top edge as in its middle: no blue from above.
    let (edge, middle) = (f.px(60, 28), f.px(60, 54));
    for c in 0..3 {
        assert!(
            edge[c].abs_diff(middle[c]) <= 2,
            "top row {edge:?} vs middle {middle:?}"
        );
    }
    // And the top glass stays blue down to its bottom edge.
    let (edge, middle) = (f.px(60, 27), f.px(60, 14));
    for c in 0..3 {
        assert!(
            edge[c].abs_diff(middle[c]) <= 2,
            "bottom row {edge:?} vs middle {middle:?}"
        );
    }
}

// M2-DRW-12: at the box's edge the blur reflects the backdrop (as Chrome does) rather than
// repeating the edge pixel. A white column 3 px inside the left edge is read twice by the
// edge pixel's taps (directly, and from the tap that reflects onto it), so the edge is
// brighter than clamping would make it.
#[test]
fn backdrop_blur_mirrors_the_backdrop_at_the_box_edge() {
    let mut s = Scene::new();
    s.add(
        0,
        &at(33.0, 0.0, 1.0, 80.0, &[("backgroundColor", "white".into())]),
    );
    s.add(
        0,
        &at(
            30.0,
            0.0,
            60.0,
            80.0,
            &[
                ("backdropFilter", "blur(2px)".into()),
                ("backgroundColor", "rgba(0,0,0,0)".into()),
            ],
        ),
    );
    let f = render(s, (120, 80), 1.0, BLACK);
    // Taps with weights w(3) + w(4) of 5.0: 0.092 of white in linear light, about 86 once
    // encoded (clamping gives 0.065, about 73).
    let edge = f.luma(30, 40);
    assert!(edge > 80.0 && edge < 92.0, "edge pixel {edge}");
}

// M3d (Fluid gaps): `radial-gradient(circle at 100% 0, ...)` is centred on the top-right
// corner, and a pixel-stop ring inside an overflow-hidden parent paints.
#[test]
fn radial_gradients_place_their_centre_and_paint_pixel_stops() {
    let mut s = Scene::new();
    s.add(
        0,
        &at(
            0.0,
            0.0,
            60.0,
            40.0,
            &[(
                "backgroundImage",
                "radial-gradient(circle at 100% 0, white 0px, black 40px)".into(),
            )],
        ),
    );
    // A 12px ring (clear inside 5.5px, white beyond 6.5px) in a 6px clip: its corner is white.
    let ring = s.add(
        0,
        &at(0.0, 80.0, 6.0, 6.0, &[("overflow", "hidden".into())]),
    );
    s.add(
        ring,
        &at(
            0.0,
            0.0,
            12.0,
            12.0,
            &[(
                "backgroundImage",
                "radial-gradient(circle, transparent 5.5px, white 6.5px)".into(),
            )],
        ),
    );
    let f = render(s, (80, 100), 1.0, BLACK);
    assert!(
        f.luma(58, 2) > 200.0,
        "the centre is at the top-right corner: {} {} {}",
        f.luma(58, 2),
        f.luma(38, 2),
        f.luma(2, 2)
    );
    let mid = f.luma(38, 2);
    assert!(mid > 100.0 && mid < 160.0, "mid-gradient grey: {mid}");
    assert!(
        f.luma(0, 80) > 200.0,
        "the ring's outer part paints: {}",
        f.luma(0, 80)
    );
    assert!(f.luma(5, 85) < 20.0, "the ring is clear inside 5.5px");
}

// Fluid gap: absolute children placed by percentage and negative insets are painted where
// their insets put them, outside their parent when the parent doesn't clip.
#[test]
fn absolute_children_with_percentage_and_negative_insets_paint_in_place() {
    let mut s = Scene::new();
    let parent = s.add(
        0,
        &at(0.0, 0.0, 40.0, 40.0, &[("backgroundColor", "black".into())]),
    );
    // left: 100% puts it just right of the parent.
    s.add(
        parent,
        &at(
            0.0,
            0.0,
            10.0,
            10.0,
            &[("left", "100%".into()), ("backgroundColor", "red".into())],
        ),
    );
    // right: -8px and bottom: 0% put it 8px past the right edge, flush with the bottom.
    s.add(
        parent,
        &at(
            0.0,
            0.0,
            12.0,
            12.0,
            &[
                ("left", "auto".into()),
                ("top", "auto".into()),
                ("right", (-8.0).into()),
                ("bottom", "0%".into()),
                ("backgroundColor", "blue".into()),
            ],
        ),
    );
    let f = render(s, (60, 60), 1.0, BLACK);
    let red = |p: [u8; 4]| p[0] > 200 && p[1] < 50 && p[2] < 50;
    let blue = |p: [u8; 4]| p[2] > 200 && p[0] < 50 && p[1] < 50;
    assert!(
        red(f.px(45, 5)),
        "left: 100% paints right of the parent: {:?}",
        f.px(45, 5)
    );
    assert!(
        blue(f.px(42, 34)),
        "right: -8px, bottom: 0% paints in place: {:?}",
        f.px(42, 34)
    );
}

// Fluid gap: box shadows inside an `overflow: hidden` parent. A spread shadow and a shadow
// with a negative offset as large as half its box both paint in the visible part of the clip.
#[test]
fn box_shadows_paint_inside_an_overflow_clip() {
    let mut s = Scene::new();
    let parent = s.add(
        0,
        &at(
            0.0,
            0.0,
            40.0,
            40.0,
            &[
                ("overflow", "hidden".into()),
                ("backgroundColor", "black".into()),
            ],
        ),
    );
    // A spread shadow: 6px of red around the 12px child, reaching 2px inside the parent's edge.
    s.add(
        parent,
        &at(
            4.0,
            4.0,
            12.0,
            12.0,
            &[
                ("backgroundColor", "white".into()),
                ("boxShadow", "0 0 0 6px red".into()),
            ],
        ),
    );
    // A shadow offset 6px left (half the box) of a 12px child: blue, 2px inside the left edge.
    s.add(
        parent,
        &at(
            4.0,
            24.0,
            12.0,
            12.0,
            &[
                ("backgroundColor", "white".into()),
                ("boxShadow", "-6px 0 0 blue".into()),
            ],
        ),
    );
    let f = render(s, (40, 40), 1.0, BLACK);
    let red = |p: [u8; 4]| p[0] > 200 && p[1] < 50 && p[2] < 50;
    let blue = |p: [u8; 4]| p[2] > 200 && p[0] < 50 && p[1] < 50;
    assert!(
        red(f.px(2, 10)),
        "spread shadow in the clip: {:?}",
        f.px(2, 10)
    );
    assert!(
        blue(f.px(1, 30)),
        "negative-offset shadow in the clip: {:?}",
        f.px(1, 30)
    );
}

// M2-VIS-08 on the GPU: group opacity composites the group once.
#[test]
fn group_opacity_blends_the_group_as_a_whole() {
    let mut s = Scene::new();
    let parent = s.add(
        0,
        &at(
            0.0,
            0.0,
            40.0,
            40.0,
            &[
                ("backgroundColor", "#ff0000".into()),
                ("opacity", 0.5.into()),
            ],
        ),
    );
    s.add(
        parent,
        &at(
            10.0,
            10.0,
            20.0,
            20.0,
            &[("backgroundColor", "#0000ff".into())],
        ),
    );
    let f = render(s, (40, 40), 1.0, BLACK);
    // Group opacity: the blue child fully covers red inside the group, then the
    // whole group is 50% over black. (Per-quad alpha would show red through blue.)
    let [r, _, b, _] = f.px(20, 20);
    assert!(r < 5, "no red shows through the child: {:?}", f.px(20, 20));
    assert!(
        (180..=192).contains(&b),
        "blue at 50% over black (sRGB ~188): {:?}",
        f.px(20, 20)
    );
    let [r, _, _, _] = f.px(5, 5);
    assert!(
        (180..=192).contains(&r),
        "red at 50% over black: {:?}",
        f.px(5, 5)
    );
}

fn images_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/images")
}

// M2-DRW-05 (images): size, position, repeat and clipping to the box shape.
#[test]
fn background_images() {
    let mut s = Scene::new();
    // checker.png is 4x4: 2x2 red/blue squares. Sized to 20px and repeated.
    s.add(
        0,
        &at(
            0.0,
            0.0,
            40.0,
            40.0,
            &[
                ("backgroundImage", "url(checker.png)".into()),
                ("backgroundSize", "20px".into()),
            ],
        ),
    );
    // wide.png is 8x4 (left green, right yellow): `contain` letterboxes it.
    s.add(
        0,
        &at(
            40.0,
            0.0,
            40.0,
            40.0,
            &[
                ("backgroundColor", "#333333".into()),
                ("backgroundImage", "url('wide.png')".into()),
                ("backgroundSize", "contain".into()),
                ("backgroundPosition", "center".into()),
                ("backgroundRepeat", "no-repeat".into()),
            ],
        ),
    );
    // `cover` crops it to the middle; a circle clips the corners.
    s.add(
        0,
        &at(
            80.0,
            0.0,
            40.0,
            40.0,
            &[
                ("backgroundImage", "url(wide.png)".into()),
                ("backgroundSize", "cover".into()),
                ("backgroundPosition", "center".into()),
                ("borderRadius", "50%".into()),
            ],
        ),
    );
    // Missing and unsafe URLs fall back to the background colour.
    s.add(
        0,
        &at(
            0.0,
            40.0,
            20.0,
            20.0,
            &[
                ("backgroundColor", "white".into()),
                ("backgroundImage", "url(missing.png)".into()),
            ],
        ),
    );
    s.add(
        0,
        &at(
            20.0,
            40.0,
            20.0,
            20.0,
            &[
                ("backgroundColor", "white".into()),
                ("backgroundImage", "url(../images/wide.png)".into()),
            ],
        ),
    );
    let f = render_with_assets(s, (120, 60), 1.0, BLACK, Some(&images_dir()));

    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let green = [0, 255, 0, 255];
    let yellow = [255, 255, 0, 255];
    // Tiles repeat every 20px: red, blue, red...
    assert_eq!(f.px(5, 5), red);
    assert_eq!(f.px(15, 5), blue);
    assert_eq!(f.px(25, 5), red);
    assert_eq!(f.px(5, 15), blue);
    assert_eq!(f.px(35, 35), red);
    // contain: a 40x20 band in the middle, background colour above and below.
    assert_eq!(f.px(50, 20), green);
    assert_eq!(f.px(70, 20), yellow);
    assert_eq!(f.px(60, 4), [0x33, 0x33, 0x33, 255]);
    assert_eq!(f.px(60, 36), [0x33, 0x33, 0x33, 255]);
    // No-repeat edges don't pick up colour from the opposite edge of the image.
    assert_eq!(f.px(40, 20), green, "left edge of a no-repeat image");
    assert_eq!(f.px(79, 20), yellow, "right edge of a no-repeat image");
    // cover: the middle half of the image fills the circle.
    assert_eq!(f.px(90, 20), green);
    assert_eq!(f.px(110, 20), yellow);
    assert_eq!(f.px(81, 1), [0, 0, 0, 255], "outside the circle");
    // Fallbacks.
    assert_eq!(f.px(10, 50), [255, 255, 255, 255]);
    assert_eq!(f.px(30, 50), [255, 255, 255, 255]);
    assert_golden("M2-DRW-05-images", 120, 60, &f.pixels, TOLERANCE);
}

fn text_sampler() -> Scene {
    let mut s = Scene::new();
    let col = |bg: &str| -> Vec<(&'static str, Value)> {
        vec![
            ("flexDirection", "column".into()),
            ("padding", 8.0.into()),
            ("gap", 4.0.into()),
            ("width", 200.0.into()),
            ("backgroundColor", bg.to_string().into()),
        ]
    };
    let row = s.add(0, &[("width", "100%".into())]);
    for (bg, fg) in [("#101318", "#f2f2f2"), ("#f4f4f4", "#1b1b1b")] {
        let c = s.add(row, &col(bg));
        s.label(c, "Platypus engine 16px", &[("color", fg.into())]);
        s.label(
            c,
            "Bold 700 weight",
            &[("color", fg.into()), ("fontWeight", "bold".into())],
        );
        s.label(
            c,
            "Italic text here",
            &[("color", fg.into()), ("fontStyle", "italic".into())],
        );
        s.label(
            c,
            "Large 28",
            &[("color", fg.into()), ("fontSize", 28.0.into())],
        );
        s.label(
            c,
            "Small 11px text",
            &[("color", fg.into()), ("fontSize", 11.0.into())],
        );
        s.label(
            c,
            "underline + strike",
            &[
                ("color", fg.into()),
                ("textDecoration", "underline line-through".into()),
            ],
        );
        s.label(
            c,
            "Shadowed",
            &[
                ("color", fg.into()),
                ("fontSize", 22.0.into()),
                ("textShadow", "2px 2px 4px rgba(255,80,80,0.9)".into()),
            ],
        );
        let mixed = s.element(c, "text", &[("color", fg.into())]);
        s.text(mixed, "Mixed ");
        let span = s.element(
            mixed,
            "span",
            &[("color", "#3fa9f5".into()), ("fontWeight", 800.0.into())],
        );
        s.text(span, "spans");
        s.text(mixed, " in one line");
    }
    s
}

// M3-TXT-01, M3-TXT-04, M3-TXT-05: rendered text (checked by eye, then golden).
#[test]
fn text_rendering() {
    for (scale, id) in [(1.0, "M3-TXT-01"), (2.0, "M3-TXT-01-2x")] {
        let f = render(text_sampler(), (400, 260), scale, BLACK);
        // Some pixels in the first label are close to the text colour.
        let s = scale as u32;
        let bright = (8 * s..120 * s)
            .flat_map(|x| (8 * s..26 * s).map(move |y| (x, y)))
            .filter(|&(x, y)| f.luma(x, y) > 220.0)
            .count();
        assert!(
            bright > (20 * s * s) as usize,
            "glyph pixels drawn at {scale}x: {bright}"
        );
        assert_golden(id, 400 * s, 260 * s, &f.pixels, TOLERANCE);
    }
}

// M3-EVT-05: overflow clipping (with inner rounded corners) and clip-path, on images,
// gradients, text, blurred layers and group opacity. Checked by eye, then golden.
#[test]
fn overflow_and_clip_path_clip_what_they_contain() {
    let mut s = Scene::new();
    // 1. A rounded, bordered card with overflow: hidden around a larger image.
    let card = s.add(
        0,
        &at(
            5.0,
            5.0,
            50.0,
            50.0,
            &[
                ("backgroundColor", "#202020".into()),
                ("borderRadius", 16.0.into()),
                ("border", "3px solid white".into()),
                ("overflow", "hidden".into()),
            ],
        ),
    );
    s.add(
        card,
        &at(
            -10.0,
            -10.0,
            80.0,
            80.0,
            &[
                ("backgroundImage", "url(checker.png)".into()),
                ("backgroundSize", "20px".into()),
            ],
        ),
    );
    // 2. clip-path: circle() on a gradient box with text inside.
    let circle = s.add(
        0,
        &at(
            65.0,
            5.0,
            50.0,
            50.0,
            &[
                (
                    "backgroundImage",
                    "linear-gradient(90deg, #ff8800, #0088ff)".into(),
                ),
                ("clipPath", "circle(50%)".into()),
            ],
        ),
    );
    s.label(
        circle,
        "Clip",
        &[("color", "white".into()), ("fontSize", 18.0.into())],
    );
    // 3. clip-path: ellipse() around a blurred child (a layer, clipped when composited).
    let ellipse = s.add(
        0,
        &at(
            125.0,
            5.0,
            50.0,
            50.0,
            &[("clipPath", "ellipse(25px 12px)".into())],
        ),
    );
    s.add(
        ellipse,
        &at(
            0.0,
            0.0,
            50.0,
            50.0,
            &[
                ("backgroundColor", "#40ff40".into()),
                ("filter", "blur(4px)".into()),
            ],
        ),
    );
    // 4. Nested: inset() with round corners, then overflow: hidden with a translucent
    //    group sticking out of it.
    let outer = s.add(
        0,
        &at(
            185.0,
            5.0,
            50.0,
            50.0,
            &[
                ("backgroundColor", "#303060".into()),
                ("clipPath", "inset(0 0 10px 0 round 12px)".into()),
            ],
        ),
    );
    let inner = s.add(
        outer,
        &at(
            10.0,
            10.0,
            30.0,
            30.0,
            &[("overflow", "hidden".into()), ("borderRadius", 15.0.into())],
        ),
    );
    let group = s.add(
        inner,
        &at(
            -20.0,
            -20.0,
            70.0,
            70.0,
            &[("opacity", 0.5.into()), ("backgroundColor", "white".into())],
        ),
    );
    s.add(
        group,
        &at(30.0, 30.0, 30.0, 30.0, &[("backgroundColor", "red".into())]),
    );
    let f = render_with_assets(s, (240, 60), 1.0, BLACK, Some(&images_dir()));

    let black = [0, 0, 0, 255];
    // 1. The image fills the card's padding box but not its outer or inner corners.
    assert_eq!(f.px(6, 6), black, "outside the card's rounded corner");
    assert_eq!(
        f.px(3, 30),
        black,
        "the image doesn't spill out of the card"
    );
    assert_eq!(f.px(30, 57), black, "nor below it");
    assert_eq!(f.px(10, 30), [255, 0, 0, 255], "the image shows inside");
    assert_eq!(
        f.px(30, 6),
        [255, 255, 255, 255],
        "the border isn't clipped"
    );
    // 2. The gradient is cut to a circle.
    assert_eq!(f.px(67, 7), black, "outside the circle");
    assert_ne!(f.px(90, 30), black);
    // 3. The blurred child only shows inside the ellipse.
    assert_eq!(f.px(150, 12), black, "above the ellipse");
    assert!(
        f.px(130, 20)[1] < 40,
        "outside the ellipse, near its corner"
    );
    assert!(f.px(150, 30)[1] > 200, "inside");
    // 4. Nested clips intersect.
    assert_eq!(f.px(210, 52), black, "below the inset");
    assert_eq!(
        f.px(197, 17),
        [0x30, 0x30, 0x60, 255],
        "outside the round child clip"
    );
    assert!(
        f.px(210, 25)[0] > 0x80,
        "the translucent group shows inside"
    );
    assert_eq!(f.stats.clips, 5, "card, circle, ellipse, inset, inner");
    assert_golden("M3-EVT-05-clip", 240, 60, &f.pixels, TOLERANCE);
}

// M3-EVT-05: scrolled content stays clipped to a rounded scroller, with overlay
// scrollbars on both axes (one thin and coloured). Checked by eye, then golden.
#[test]
fn scrolled_content_and_scrollbars() {
    let mut s = Scene::new();
    let scroller = s.add(
        0,
        &at(
            5.0,
            5.0,
            90.0,
            70.0,
            &[
                ("backgroundColor", "#202430".into()),
                ("borderRadius", 10.0.into()),
                ("overflow", "auto".into()),
                ("flexDirection", "column".into()),
            ],
        ),
    );
    let colors = ["#e05050", "#50b060", "#5070e0", "#e0b040", "#a050c0"];
    for c in colors {
        s.add(
            scroller,
            &[
                ("height", 30.0.into()),
                ("width", 140.0.into()),
                ("flexShrink", 0.0.into()),
                ("backgroundColor", c.into()),
            ],
        );
    }
    let thin = s.add(
        0,
        &at(
            105.0,
            5.0,
            50.0,
            70.0,
            &[
                ("backgroundColor", "#202430".into()),
                ("overflow", "scroll".into()),
                ("scrollbarWidth", "thin".into()),
                ("scrollbarColor", "#ff8a5e #ffffff22".into()),
            ],
        ),
    );
    s.add(
        thin,
        &[
            ("height", 200.0.into()),
            ("width", 40.0.into()),
            ("flexShrink", 0.0.into()),
        ],
    );

    let gpu = shared_gpu();
    let mut tree = UiTree::new(160.0, 80.0);
    let errors = tree.apply_commit(&Commit { ops: s.ops });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
    let scroller = web_ui_core::NodeId(scroller);
    assert!(tree.scroll_to(scroller, 20.0, 45.0, true));
    assert!(tree.scroll_to(web_ui_core::NodeId(thin), 0.0, 65.0, true));
    let cmds = tree.draw_list();
    let target = OffscreenTarget::with_format(&gpu, 160, 80, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let mut ui = UiRenderer::new(&gpu);
    ui.set_blending(Blending::Linear);
    ui.render(
        &gpu,
        &RenderTarget {
            view: &view,
            format: target.format(),
            size: (160, 80),
            scale: 1.0,
        },
        BLACK,
        &cmds,
    );
    let pixels = target.read_pixels(&gpu).unwrap();
    let f = Frame {
        pixels,
        width: 160,
        stats: RenderStats::default(),
    };
    // Scrolled by 45: row 1 (green, 30..60) is at the top, from y = 5 - 15 = -10 to 20.
    assert_eq!(f.px(40, 12), [0x50, 0xb0, 0x60, 255]);
    // Row 2 (blue) is next.
    assert_eq!(f.px(40, 30), [0x50, 0x70, 0xe0, 255]);
    // Nothing spills out of the rounded scroller.
    assert_eq!(f.px(6, 6), [0, 0, 0, 255], "the rounded corner clips");
    assert_eq!(f.px(40, 78), [0, 0, 0, 255]);
    // The vertical thumb is drawn at the right edge, over the content.
    assert!(f.luma(88, 40) > 100.0, "vertical thumb");
    assert_golden("M3-EVT-05-scroll", 160, 80, &f.pixels, TOLERANCE);
}

// M3-INP-01: text fields: a filled input, a placeholder, a focused input with a
// selection and caret, and a wrapping textarea. Checked by eye, then golden.
#[test]
fn text_fields() {
    use platypus_ui::{Input, Interaction};
    let mut s = Scene::new();
    let field = |s: &mut Scene, kind: &str, x: f64, y: f64, w: f64, attrs: &[(&str, Value)]| {
        let mut style = at(
            x,
            y,
            w,
            0.0,
            &[
                ("color", "#e8ecf2".into()),
                ("backgroundColor", "#1c222c".into()),
                ("border", "1px solid #3a4558".into()),
                ("borderRadius", 6.0.into()),
                ("padding", "4px 8px".into()),
                ("fontSize", 14.0.into()),
            ],
        );
        // Fields size their height from the font.
        style.retain(|(k, _)| *k != "height");
        let id = s.element(0, kind, &style);
        let n = |v: u32| Value::Number(v.into());
        let mut set = vec![n(SET_ATTRS), n(id)];
        for (k, v) in attrs {
            set.push((*k).into());
            set.push(v.clone());
        }
        s.ops.push(Value::Array(set));
        id
    };
    field(
        &mut s,
        "input",
        8.0,
        8.0,
        140.0,
        &[("value", "Iron".into())],
    );
    field(
        &mut s,
        "input",
        8.0,
        44.0,
        140.0,
        &[("placeholder", "Your name".into())],
    );
    let focused = field(
        &mut s,
        "input",
        8.0,
        80.0,
        140.0,
        &[("value", "selected text".into())],
    );
    field(
        &mut s,
        "textarea",
        160.0,
        8.0,
        150.0,
        &[
            (
                "value",
                "A textarea wraps its lines and keeps them in view.".into(),
            ),
            ("rows", 4.0.into()),
        ],
    );

    let gpu = shared_gpu();
    let mut tree = UiTree::new(320.0, 116.0);
    let errors = tree.apply_commit(&Commit { ops: s.ops });
    assert!(errors.is_empty(), "{errors:?}");
    let mut ui = Interaction::new();
    ui.handle(
        &mut tree,
        Input::Focus(web_ui_core::NodeId(focused)),
        &mut |_| false,
    );
    ui.handle(
        &mut tree,
        Input::Modifiers(platypus_ui::events::Modifiers {
            shift: true,
            ctrl: true,
            ..Default::default()
        }),
        &mut |_| false,
    );
    // Ctrl+Shift+Left from the end selects the last word.
    ui.handle(
        &mut tree,
        Input::KeyDown {
            key: "ArrowLeft".into(),
            code: "ArrowLeft".into(),
            repeat: false,
            text: None,
        },
        &mut |_| false,
    );
    let cmds = tree.draw_list();
    let target = OffscreenTarget::with_format(&gpu, 320, 116, wgpu::TextureFormat::Rgba8UnormSrgb);
    let view = target.view().unwrap();
    let mut ui = UiRenderer::new(&gpu);
    ui.set_blending(Blending::Linear);
    ui.render(
        &gpu,
        &RenderTarget {
            view: &view,
            format: target.format(),
            size: (320, 116),
            scale: 1.0,
        },
        BLACK,
        &cmds,
    );
    let pixels = target.read_pixels(&gpu).unwrap();
    assert_golden("M3-INP-01-fields", 320, 116, &pixels, TOLERANCE);
}
