// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! CSS gradients drawn by the CPU renderer.

use platypus_render_cpu::CpuRenderer;
use platypus_ui::opcodes::*;
use platypus_ui::{Rgba, UiTree};
use web_ui_core::{Commit, Value};

fn n(v: f64) -> Value {
    Value::Number(v)
}

/// A 96x96 box at the origin with `background` over white, as RGB rows.
fn draw(background: &str) -> Vec<[u8; 3]> {
    let mut t = UiTree::new(96.0, 96.0);
    let ops = vec![
        Value::Array(vec![n(CREATE as f64), n(1.0), "box".into()]),
        Value::Array(vec![
            n(SET_STYLE as f64),
            n(1.0),
            "width".into(),
            n(96.0),
            "height".into(),
            n(96.0),
            "backgroundImage".into(),
            background.into(),
        ]),
        Value::Array(vec![n(APPEND_CHILD as f64), n(0.0), n(1.0)]),
    ];
    assert!(t.apply_commit(&Commit { ops }).is_empty());
    t.layout_if_needed();
    let cmds = t.draw_list();
    let mut r = CpuRenderer::new();
    let pixmap = r.render((96, 96), Rgba::new(1.0, 1.0, 1.0, 1.0), &[(1.0, &cmds)]);
    pixmap.data().iter().map(|p| [p.r, p.g, p.b]).collect()
}

// M2-CPU-01
#[test]
fn radial_stops_start_at_their_radius_and_fade_from_transparent_premultiplied() {
    let px = draw("radial-gradient(circle, transparent 44px, #ff0000 52px)");
    // Along the diagonal from the centre (48, 48), `d` px out.
    let at = |d: f32| {
        let o = (48.0 - d / std::f32::consts::SQRT_2).round() as usize;
        px[o * 96 + o]
    };
    // Clear inside 44px (the first stop's radius, not the centre).
    for d in [0.0, 20.0, 40.0] {
        assert_eq!(at(d), [255, 255, 255], "{d}px out");
    }
    // Halfway, white and red mix without darkening: `transparent` is transparent red there.
    let mid = at(48.0);
    assert!(mid[0] >= 250 && mid[1] > 60 && mid[1] < 230, "{mid:?}");
    assert_eq!(at(60.0), [255, 0, 0]);
}
