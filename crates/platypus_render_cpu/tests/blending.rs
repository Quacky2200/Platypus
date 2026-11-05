// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The CPU renderer blends translucent colours on sRGB values, like browsers (`vello_cpu` works
//! on 8-bit sRGB): `examples/alpha-probe`'s six probes over a `#3060c0` field.

use platypus_render_cpu::CpuRenderer;
use platypus_ui::opcodes::*;
use platypus_ui::{Rgba, UiTree};
use web_ui_core::{Commit, Value};

fn n(v: f64) -> Value {
    Value::Number(v)
}

/// An absolute box at `rect` under `parent` with `style`.
fn element(ops: &mut Vec<Value>, id: u32, parent: u32, rect: [f64; 4], style: &[(&str, &str)]) {
    ops.push(Value::Array(vec![
        n(CREATE as f64),
        n(id.into()),
        "box".into(),
    ]));
    let mut set = vec![n(SET_STYLE as f64), n(id.into())];
    for (k, v) in [
        ("position", Value::from("absolute")),
        ("left", n(rect[0])),
        ("top", n(rect[1])),
        ("width", n(rect[2])),
        ("height", n(rect[3])),
    ] {
        set.push(k.into());
        set.push(v);
    }
    for (k, v) in style {
        set.push((*k).into());
        set.push((*v).into());
    }
    ops.push(Value::Array(set));
    ops.push(Value::Array(vec![
        n(APPEND_CHILD as f64),
        n(parent.into()),
        n(id.into()),
    ]));
}

// M2-BLEND-09
#[test]
fn the_alpha_probe_blends_in_srgb() {
    let mut ops = Vec::new();
    element(
        &mut ops,
        1,
        0,
        [0.0, 0.0, 800.0, 200.0],
        &[("backgroundColor", "#3060c0")],
    );
    let probes: [(f64, &[(&str, &str)]); 6] = [
        (20.0, &[("backgroundColor", "rgba(255, 255, 255, 0.25)")]),
        (140.0, &[("border", "10px solid rgba(255, 255, 255, 0.25)")]),
        (260.0, &[("backgroundColor", "rgba(255, 0, 0, 0.5)")]),
        (
            380.0,
            &[
                ("backgroundColor", "rgba(255, 0, 0, 0.5)"),
                ("border", "10px solid rgba(0, 0, 0, 0.25)"),
            ],
        ),
        (
            500.0,
            &[
                ("border", "10px solid rgba(255, 255, 255, 0.25)"),
                ("borderRadius", "18"),
            ],
        ),
        (
            620.0,
            &[
                ("backgroundColor", "rgba(255, 255, 255, 0.25)"),
                ("backdropFilter", "blur(4px)"),
            ],
        ),
    ];
    for (i, (x, style)) in probes.iter().enumerate() {
        element(&mut ops, 2 + i as u32, 1, [*x, 50.0, 100.0, 100.0], style);
    }
    let mut t = UiTree::new(800.0, 200.0);
    assert!(t.apply_commit(&Commit { ops }).is_empty());
    t.layout_if_needed();
    let cmds = t.draw_list();
    let mut r = CpuRenderer::new();
    let pixmap = r.render((800, 200), Rgba::new(0.0, 0.0, 0.0, 1.0), &[(1.0, &cmds)]);
    let at = |x: usize| {
        let p = pixmap.data()[100 * 800 + x];
        [p.r, p.g, p.b]
    };
    for (x, expect) in [
        (70, [100, 136, 208]),
        (145, [100, 136, 208]),
        (190, [48, 96, 192]),
        (310, [152, 48, 96]),
        (385, [114, 36, 72]),
        (430, [152, 48, 96]),
        (505, [100, 136, 208]),
        (670, [100, 136, 208]),
    ] {
        let got = at(x);
        assert!(
            got.iter().zip(expect).all(|(a, b)| a.abs_diff(b) <= 1),
            "x={x}: {got:?}, expected {expect:?}"
        );
    }
}
