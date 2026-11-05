// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Runs the data-driven layout fixtures in `tests/layout/*.json` (M2-LAY-01..03, M2-LAY-05).
//!
//! Fixtures go through the full commit pipeline (CREATE → SET_STYLE → APPEND_CHILD)
//! and are generic over [`LayoutEngine`], so a Yoga backend can run the same cases.

use std::collections::HashMap;
use std::path::PathBuf;

use platypus_ui::opcodes::*;
use platypus_ui::{LayoutEngine, ROOT, TaffyLayout, UiTree};
use serde_json::Value as Json;
use web_ui_core::{Commit, NodeId, Value};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/layout")
}

fn to_value(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::Number(n.as_f64().unwrap()),
        Json::String(s) => Value::String(s.clone()),
        Json::Array(a) => Value::Array(a.iter().map(to_value).collect()),
        Json::Object(_) => panic!("objects aren't style values"),
    }
}

/// Emits ops for `node` and its subtree, giving each fixture name a numeric ID.
fn emit(node: &Json, parent: u32, names: &mut HashMap<String, u32>, ops: &mut Vec<Value>) {
    let id = names.len() as u32 + 1;
    let name = node["id"].as_str().expect("every fixture node needs an id");
    names.insert(name.to_owned(), id);

    let num = |n: u32| Value::Number(n.into());
    ops.push(Value::Array(vec![num(CREATE), num(id), "box".into()]));
    if let Some(style) = node.get("style").and_then(Json::as_object) {
        let mut op = vec![num(SET_STYLE), num(id)];
        for (prop, value) in style {
            op.push(prop.as_str().into());
            op.push(to_value(value));
        }
        ops.push(Value::Array(op));
    }
    for child in node
        .get("children")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        emit(child, id, names, ops);
    }
    ops.push(Value::Array(vec![num(APPEND_CHILD), num(parent), num(id)]));
}

fn run_fixtures<L: LayoutEngine>(make: impl Fn() -> L) {
    let mut paths: Vec<_> = std::fs::read_dir(fixtures_dir())
        .expect("tests/layout exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no layout fixtures found");

    let mut failures = Vec::new();
    for path in &paths {
        let name = path.file_stem().unwrap().to_string_lossy();
        let fixture: Json = serde_json::from_str(&std::fs::read_to_string(path).unwrap())
            .unwrap_or_else(|e| panic!("{name}: invalid JSON: {e}"));
        let root = fixture["root"].as_array().expect("root: [w, h]");
        let (w, h) = (
            root[0].as_f64().unwrap() as f32,
            root[1].as_f64().unwrap() as f32,
        );

        let mut tree = UiTree::with_layout(make(), w, h);
        let mut names = HashMap::new();
        let mut ops = Vec::new();
        emit(&fixture["tree"], ROOT.0, &mut names, &mut ops);
        let errors = tree.apply_commit(&Commit { ops });
        assert!(errors.is_empty(), "{name}: {errors:?}");
        tree.layout_if_needed();

        for (node, expected) in fixture["expect"].as_object().expect("expect: {}") {
            let id = NodeId(names[node.as_str()]);
            let want: Vec<f32> = expected
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            let r = tree.rect(id).unwrap();
            let got = [r.x, r.y, r.width, r.height];
            if got.iter().zip(&want).any(|(g, w)| (g - w).abs() > 0.01) {
                failures.push(format!("{name}/{node}: expected {want:?}, got {got:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "layout mismatches:\n{}",
        failures.join("\n")
    );
}

// M2-LAY-01..03 and M2-LAY-05 (the same fixtures will run against Yoga).
#[test]
fn taffy_passes_layout_fixtures() {
    run_fixtures(TaffyLayout::new);
}
