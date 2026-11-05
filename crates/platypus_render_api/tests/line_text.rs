// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `LineRenderer2D` fills and draw order, and the `TextMesh` component, as data (the host reads
//! the registries; nothing draws here).
use platypus_engine_api::color::Rgba;
use platypus_render_api::{
    LineRenderer2DRef, TextMeshRef, load_line_renderer2d, load_text_mesh, save_line_renderer2d,
    save_text_mesh,
};
use platypus_scene::ObjectId;
use ts_api::Handle;

fn object(n: u64) -> ObjectId {
    ObjectId::from_raw(n)
}

// M9-API-1507
#[test]
fn a_line_renderer_starts_unfilled_and_a_fill_colour_turns_the_fill_on() {
    platypus_render_api::reset();
    let line = LineRenderer2DRef::attach(object(1));
    assert!(line.fill_color().is_none());
    assert_eq!(line.fill_rule(), "evenodd");
    assert_eq!(line.sorting_order(), 0.0);
    line.set_fill_color(Some(Rgba {
        r: 0.5,
        g: 0.25,
        b: 1.0,
        a: 0.5,
    }));
    assert!(
        line.fill_color().is_some(),
        "writing the colour fills the shape"
    );
    line.set_fill_rule("nonzero");
    line.set_fill_rule("sideways");
    assert_eq!(line.fill_rule(), "nonzero", "an unknown rule is ignored");
    line.set_sorting_order(-4.0);
    let (_, d) = platypus_render_api::line_renderer2ds().remove(0);
    assert!(d.fill_color.is_some_and(|c| c.b == 1.0) && d.sorting_order == -4);
    // `fillColor = null`.
    line.set_fill_color(None);
    assert!(
        platypus_render_api::line_renderer2ds()[0]
            .1
            .fill_color
            .is_none()
    );
    assert!(line.fill_color().is_none());
}

// M9-API-1508
#[test]
fn a_fill_saves_as_fill_color_and_no_fill_as_null() {
    platypus_render_api::reset();
    let a = LineRenderer2DRef::attach(object(1));
    let saved = save_line_renderer2d(a);
    assert!(saved["fillColor"].is_null(), "unfilled saves null");
    assert_eq!(saved["fillRule"], "evenodd");
    a.set_fill_color(Some(Rgba {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    }));
    a.set_sorting_order(7.0);
    let saved = save_line_renderer2d(a);
    assert_eq!(saved["fillColor"]["value"][0], 1.0);
    assert_eq!(saved["sortingOrder"], 7);
    let b = LineRenderer2DRef::attach(object(2));
    assert!(load_line_renderer2d(b, &saved).is_empty());
    assert!(b.fill_color().is_some_and(|c| c.r == 1.0) && b.sorting_order() == 7.0);
    let mut cleared = saved.clone();
    cleared.insert("fillColor".into(), serde_json::Value::Null);
    load_line_renderer2d(b, &cleared);
    assert!(b.fill_color().is_none());
}

// M9-API-1509
#[test]
fn a_text_mesh_has_unity_style_defaults_and_goes_with_its_object() {
    platypus_render_api::reset();
    let t = TextMeshRef::attach(object(3));
    assert_eq!(TextMeshRef::find_on(object(3)), t);
    assert_eq!(
        (t.text(), t.font_size(), t.font(), t.font_weight()),
        (String::new(), 32.0, String::new(), 400.0)
    );
    assert_eq!(
        (
            t.alignment(),
            t.max_width(),
            t.pixels_per_unit(),
            t.resolution()
        ),
        ("center".to_string(), 0.0, 100.0, 2.0)
    );
    t.set_text("12:34");
    t.set_alignment("left");
    t.set_alignment("justify");
    t.set_max_width(300.0);
    assert_eq!(
        (t.text(), t.alignment(), t.max_width()),
        ("12:34".into(), "left".into(), 300.0)
    );
    let all = platypus_render_api::text_meshs();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].1.object, object(3));
    platypus_render_api::destroyed(&[object(3)]);
    assert!(platypus_render_api::text_meshs().is_empty());
}

// M9-API-1510
#[test]
fn a_text_mesh_saves_and_loads_with_scene_files() {
    platypus_render_api::reset();
    let a = TextMeshRef::attach(object(1));
    a.set_text("hello");
    a.set_font_size(48.0);
    a.set_color(Rgba {
        r: 1.0,
        g: 0.5,
        b: 0.0,
        a: 1.0,
    });
    let saved = save_text_mesh(a);
    assert_eq!(saved["text"], "hello");
    let b = TextMeshRef::attach(object(2));
    assert!(load_text_mesh(b, &saved).is_empty());
    assert_eq!(
        (b.text(), b.font_size(), b.color().g),
        ("hello".into(), 48.0, 0.5)
    );
    let mut odd = serde_json::Map::new();
    odd.insert("nonsense".into(), serde_json::json!(1));
    odd.insert("fontSize".into(), serde_json::json!("big"));
    assert_eq!(load_text_mesh(b, &odd).len(), 2, "both are warned about");
}
