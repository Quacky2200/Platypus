// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// SPDX-License-Identifier: Apache-2.0

//! Flow layout drawn by the UI renderer (M3-FLT-20..): floats with text wrapping around
//! them, checked by golden images (and by eye when they change).

use platypus_ui::UiTree;
use platypus_ui::opcodes::*;
use web_ui_core::{Commit, Value};
use web_ui_render::testing::{assert_golden, shared_gpu};
use web_ui_render::{Color, OffscreenTarget, RenderTarget, UiRenderer, wgpu};

const TOLERANCE: u8 = 3;

const LOREM: &str = "The quick brown fox jumps over the lazy dog while the five boxing wizards \
    jump quickly. Pack my box with five dozen liquor jugs, and sphinx of black quartz, judge my \
    vow. How vexingly quick daft zebras jump over a field of bright flowers in the summer sun.";

#[derive(Default)]
struct Scene {
    ops: Vec<Value>,
    next: u32,
    css: &'static str,
}

type Style<'a> = &'a [(&'a str, Value)];

impl Scene {
    fn new() -> Self {
        Self {
            ops: Vec::new(),
            next: 1,
            css: "",
        }
    }

    fn element(&mut self, parent: u32, kind: &str, style: Style) -> u32 {
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

    fn text(&mut self, parent: u32, text: &str) {
        let id = self.next;
        self.next += 1;
        let n = |v: u32| Value::Number(v.into());
        self.ops
            .push(Value::Array(vec![n(CREATE_TEXT), n(id), text.into()]));
        self.ops
            .push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
    }

    fn set_class(&mut self, id: u32, class: &str) {
        let n = |v: u32| Value::Number(v.into());
        self.ops.push(Value::Array(vec![
            n(SET_ATTRS),
            n(id),
            "className".into(),
            class.into(),
        ]));
    }

    fn label(&mut self, parent: u32, text: &str, style: Style) {
        let id = self.element(parent, "text", style);
        self.text(id, text);
    }
}

fn px(v: f64) -> Value {
    Value::Number(v)
}

fn render(scene: Scene, size: (u32, u32)) -> Vec<u8> {
    let gpu = shared_gpu();
    let mut tree = UiTree::new(size.0 as f32, size.1 as f32);
    let errors = tree.apply_commit(&Commit { ops: scene.ops });
    assert!(errors.is_empty(), "{errors:?}");
    if !scene.css.is_empty() {
        let added = tree.add_stylesheet(scene.css);
        assert!(added.warnings.is_empty(), "{:?}", added.warnings);
    }
    tree.layout_if_needed();
    let target =
        OffscreenTarget::with_format(&gpu, size.0, size.1, wgpu::TextureFormat::Rgba8UnormSrgb);
    // The goldens were made in linear light.
    let mut ui = UiRenderer::new(&gpu);
    ui.set_blending(web_ui_render::Blending::Linear);
    let cmds = tree.draw_list();
    let view = target.view().unwrap();
    ui.render(
        &gpu,
        &RenderTarget {
            view: &view,
            format: target.format(),
            size,
            scale: 1.0,
        },
        Color::rgb(0.0, 0.0, 0.0),
        &cmds,
    );
    target.read_pixels(&gpu).unwrap()
}

// M3-FLT-20
#[test]
fn text_wraps_around_left_and_right_floats() {
    let mut s = Scene::new();
    let page = s.element(
        0,
        "box",
        &[
            ("display", "block".into()),
            ("width", px(380.0)),
            ("padding", px(10.0)),
            ("backgroundColor", "#f4f1ea".into()),
        ],
    );
    s.label(
        page,
        "Floats",
        &[
            ("color", "#222".into()),
            ("fontSize", px(22.0)),
            ("fontWeight", "bold".into()),
            ("marginBottom", px(6.0)),
        ],
    );
    // A picture on the left and a pull-quote on the right; the paragraph flows between and
    // under them.
    s.element(
        page,
        "box",
        &[
            ("float", "left".into()),
            ("width", px(90.0)),
            ("height", px(110.0)),
            ("margin", "0 12px 6px 0".into()),
            ("backgroundColor", "#3a7ca5".into()),
            ("borderRadius", px(8.0)),
        ],
    );
    let quote = s.element(
        page,
        "box",
        &[
            ("float", "right".into()),
            ("display", "block".into()),
            ("width", px(100.0)),
            ("margin", "0 0 6px 12px".into()),
            ("padding", px(8.0)),
            ("backgroundColor", "#d9a441".into()),
            ("borderRadius", px(6.0)),
        ],
    );
    s.label(
        quote,
        "\"Jump quickly.\"",
        &[("color", "#222".into()), ("fontSize", px(14.0))],
    );
    s.label(
        page,
        LOREM,
        &[
            ("color", "#333".into()),
            ("fontSize", px(14.0)),
            ("lineHeight", px(1.4)),
        ],
    );
    s.element(
        page,
        "box",
        &[
            ("clear", "both".into()),
            ("height", px(14.0)),
            ("marginTop", px(8.0)),
            ("backgroundColor", "#7a9e7e".into()),
        ],
    );
    let pixels = render(s, (400, 300));
    assert_golden("M3-FLT-20", 400, 300, &pixels, TOLERANCE);
}

/// The stylesheet of `examples/table`: a grid whose rows are `display: contents` boxes, so
/// one grid sizes the columns from every row's content.
const TABLE_CSS: &str = include_str!("../../../examples/table/src/ui.css");

// M3-TBL-01
#[test]
fn a_grid_of_display_contents_rows_makes_a_table() {
    let mut s = Scene::new();
    s.css = TABLE_CSS;
    let page = s.element(
        0,
        "box",
        &[
            ("display", "block".into()),
            ("width", px(560.0)),
            ("padding", px(12.0)),
            ("backgroundColor", "#f6f7fa".into()),
            ("fontSize", px(13.0)),
        ],
    );
    let table = s.element(page, "box", &[]);
    s.set_class(table, "table");
    let rows: [[&str; 5]; 6] = [
        ["Item", "Kind", "Notes", "Qty", "Price"],
        [
            "Iron sword",
            "Weapon",
            "Forged at the river forge; blunts quickly on stone.",
            "1",
            "120",
        ],
        ["Potion", "Consumable", "Heals.", "12", "15"],
        [
            "Oak shield",
            "Armour",
            "A long note that has to wrap inside its column because the table is only as wide as the page lets it be.",
            "3",
            "80",
        ],
        ["Rope", "Tool", "50 m", "200", "2"],
        ["Gem", "Treasure", "Very shiny", "7", "400"],
    ];
    for (i, row) in rows.iter().enumerate() {
        let r = s.element(table, "box", &[]);
        s.set_class(r, if i == 0 { "row head" } else { "row" });
        for (c, text) in row.iter().enumerate() {
            let cell = s.element(r, "box", &[]);
            s.set_class(cell, if c >= 3 { "cell num" } else { "cell" });
            let label = s.element(cell, "text", &[]);
            s.text(label, text);
        }
    }
    let foot = s.element(table, "box", &[]);
    s.set_class(foot, "row foot");
    for (class, text) in [("cell span-3", "Total value"), ("cell num span-2", "3520")] {
        let cell = s.element(foot, "box", &[]);
        s.set_class(cell, class);
        let label = s.element(cell, "text", &[]);
        s.text(label, text);
    }
    let pixels = render(s, (584, 290));
    assert_golden("M3-TBL-01", 584, 290, &pixels, TOLERANCE);
}
