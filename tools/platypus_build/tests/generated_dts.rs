// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Guard rails for "every declaration is generated from Rust" (docs/research/platypus-generated-dts-plan.md):
//! the compiler built-ins declare no engine name again, and a snapshot of the API surface `tsc` sees.
//! (The guard against hand-written `.d.ts` files, `M9-TS-3100`, moved to web-ui with the compiler.)

use oxc_allocator::Allocator;
use oxc_ast::ast::{Statement, TSNamespaceDeclarationBody};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The top-level names the manifests declare (namespaces, classes, value classes, handle types,
/// enums, records, aliases, globals).
fn manifest_names(ms: &[String]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for m in ms {
        let v: serde_json::Value = serde_json::from_str(m).unwrap();
        if let Some(n) = v["namespace"].as_str().filter(|n| !n.is_empty()) {
            names.insert(n.to_string());
        }
        for h in v["handles"].as_array().into_iter().flatten() {
            names.extend(h.as_str().map(str::to_string));
        }
        for key in ["classes", "values", "enums", "records", "aliases"] {
            for x in v[key].as_array().into_iter().flatten() {
                names.extend(x["name"].as_str().map(str::to_string));
            }
        }
        for f in v["functions"].as_array().into_iter().flatten() {
            if f["global"].as_bool() == Some(true) {
                names.extend(f["name"].as_str().map(str::to_string));
            }
        }
    }
    names
}

// M9-TS-3101
#[test]
fn the_built_ins_declare_no_engine_name_again_and_their_owners_exist() {
    use platypus_ts::builtins::{Builtins, Place};
    let ms: Vec<String> = platypus_build::manifests()
        .into_iter()
        .filter(|m| !m.contains("\"namespace\":\"Ui\""))
        .collect();
    // The compiler's built-ins and the manifests' own `declarations` (Platypus's components).
    let builtins = Builtins::from_manifests(&ms).unwrap();
    let engine = manifest_names(&ms);
    let twice: Vec<_> = builtins
        .top_names()
        .into_iter()
        .filter(|n| engine.contains(*n))
        .collect();
    assert!(
        twice.is_empty(),
        "declared by a manifest and by builtins.rs: {twice:?}"
    );
    // A member of a class or namespace no manifest has would be dropped silently.
    let mut ownerless = Vec::new();
    for b in builtins.all() {
        if let Place::Static(o) | Place::Member(o) | Place::Ctor(o) = b.place
            && !engine.contains(o)
        {
            ownerless.push((o, b.sig));
        }
    }
    assert!(
        ownerless.is_empty(),
        "built-ins of no engine class: {ownerless:?}"
    );
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One normalised line per top-level declaration, and per member of a class, interface or
/// namespace (sorted, comments and layout dropped).
fn surface(src: &str) -> String {
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, src, SourceType::d_ts()).parse();
    assert!(ret.diagnostics.is_empty(), "{:?}", ret.diagnostics);
    let text = |span: oxc_span::Span| collapse(&src[span.start as usize..span.end as usize]);
    let mut lines: Vec<String> = Vec::new();
    for s in &ret.program.body {
        match s {
            Statement::ClassDeclaration(c) => {
                let head = text(oxc_span::Span::new(c.span.start, c.body.span.start));
                let mut members: Vec<String> = c.body.body.iter().map(|m| text(m.span())).collect();
                members.sort();
                lines.push(head.clone());
                lines.extend(members.into_iter().map(|m| format!("{head} :: {m}")));
            }
            Statement::TSInterfaceDeclaration(i) => {
                let head = text(oxc_span::Span::new(i.span.start, i.body.span.start));
                let mut members: Vec<String> = i.body.body.iter().map(|m| text(m.span())).collect();
                members.sort();
                lines.push(head.clone());
                lines.extend(members.into_iter().map(|m| format!("{head} :: {m}")));
            }
            Statement::TSNamespaceDeclaration(m) => {
                let TSNamespaceDeclarationBody::TSModuleBlock(b) = &m.body else {
                    lines.push(text(m.span));
                    continue;
                };
                let head = text(oxc_span::Span::new(m.span.start, b.span.start));
                let mut members: Vec<String> = b.body.iter().map(|m| text(m.span())).collect();
                members.sort();
                lines.push(head.clone());
                lines.extend(members.into_iter().map(|m| format!("{head} :: {m}")));
            }
            Statement::TSExternalModuleDeclaration(m) => {
                let Some(b) = &m.body else {
                    lines.push(text(m.span));
                    continue;
                };
                let head = text(oxc_span::Span::new(m.span.start, b.span.start));
                let mut members: Vec<String> = b.body.iter().map(|m| text(m.span())).collect();
                members.sort();
                lines.push(head.clone());
                lines.extend(members.into_iter().map(|m| format!("{head} :: {m}")));
            }
            other => lines.push(text(other.span())),
        }
    }
    lines.sort();
    lines.dedup();
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn snapshot_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/api_surface.txt")
}

// M9-TS-3102
#[test]
fn the_generated_api_surface_matches_the_blessed_snapshot() {
    let dts = platypus_build::types_dts().unwrap();
    let now = surface(&dts);
    let path = snapshot_path();
    if std::env::var("PLATYPUS_BLESS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &now).unwrap();
        return;
    }
    let blessed = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing: run with PLATYPUS_BLESS=1", path.display()));
    if blessed != now {
        let a: BTreeSet<&str> = blessed.lines().collect();
        let b: BTreeSet<&str> = now.lines().collect();
        let removed: Vec<_> = a.difference(&b).take(40).collect();
        let added: Vec<_> = b.difference(&a).take(40).collect();
        panic!(
            "the API surface changed (review, then PLATYPUS_BLESS=1 to accept)\nremoved:\n{}\nadded:\n{}",
            removed
                .iter()
                .map(|s| format!("  - {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
            added
                .iter()
                .map(|s| format!("  + {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

// M9-TS-3164
//
// Declared types match what games are written against, so they type-check as written: handle-valued properties and options may be `null`, a property that is one of a few
// names reads as that union (and still takes any string, as the engine ignores an unknown name),
// and a namespace property that is one of a few names is the union.
#[test]
fn declared_types_are_the_ones_games_are_written_against() {
    let dts = platypus_build::types_dts().unwrap();
    for want in [
        "mainTexture?: Texture | null",
        "get type(): \"directional\" | \"point\" | \"spot\";",
        "set type(v: string);",
        "let softShadowQuality: \"low\" | \"medium\" | \"high\";",
    ] {
        assert!(dts.contains(want), "the declarations lack `{want}`");
    }
}
