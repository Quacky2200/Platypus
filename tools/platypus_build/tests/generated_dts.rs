// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Guard rails for "every declaration is generated from Rust" (docs/research/platypus-generated-dts-plan.md):
//! no hand-written `.d.ts` file (and the TypeScript compiled into games only on a short list), the
//! compiler built-ins declare no engine name again, and a snapshot of the API surface `tsc` sees.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Statement, TSNamespaceDeclarationBody};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if matches!(
                name.as_str(),
                "target" | "node_modules" | "build" | "types" | ".git" | ".claude"
            ) {
                continue;
            }
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn rel(p: &Path) -> String {
    p.strip_prefix(repo())
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Hand-written TypeScript compiled into games, each with why it is not Rust yet. (`facade.ts`, the
/// engine adapters, is gone: the compiler writes those functions from the manifests.)
const ALLOWED_TS: &[(&str, &str)] = &[
    (
        "tools/platypus_ts/src/lower/ui.ts",
        "the hook helpers (useState, useReducer, useRef, useMemo, useContext, ...): generic over the program's own types, so they are          monomorphised per use by the compiler and cannot be one native function; TODO: lower them in Rust (jsx.rs) as IR          over the native slots of platypus_ts_ui once the compiler can emit generic calls into a native library",
    ),
    (
        "tools/platypus_ts/src/lower/temporal.ts",
        "the Temporal runtime library (decision 2: moved to Rust long term, not in this pass)",
    ),
];

// M9-TS-3100
#[test]
fn no_new_hand_written_declaration_files_appear() {
    let mut files = Vec::new();
    walk(&repo().join("tools"), &mut files);
    let found: BTreeSet<String> = files
        .iter()
        .map(|p| rel(p))
        .filter(|r| r.ends_with(".d.ts"))
        .collect();
    assert!(
        found.is_empty(),
        "hand-written .d.ts files are not allowed (declare it in #[ts_api], platypus_ui::tables or tools/platypus_ts/src/builtins.rs): {found:?}"
    );
    // No Rust source may include a declaration file either.
    let mut included = BTreeSet::new();
    for p in &files {
        if p.extension().is_some_and(|e| e == "rs") {
            let Ok(t) = std::fs::read_to_string(p) else {
                continue;
            };
            for l in t.lines() {
                if l.contains("include_str!(") && l.contains(".d.ts\"") {
                    let name = l.split('"').nth(1).unwrap_or("");
                    included.insert(name.rsplit('/').next().unwrap_or("").to_string());
                }
            }
        }
    }
    assert!(
        included.is_empty(),
        "include_str! of a declaration file: {included:?}"
    );
    let ts: BTreeSet<String> = files
        .iter()
        .map(|p| rel(p))
        .filter(|r| {
            r.starts_with("tools/platypus_ts/src/lower/")
                && r.ends_with(".ts")
                && !r.ends_with(".d.ts")
        })
        .collect();
    let allowed_ts: BTreeSet<String> = ALLOWED_TS.iter().map(|(p, _)| p.to_string()).collect();
    assert_eq!(
        ts, allowed_ts,
        "TypeScript compiled into games: list it in ALLOWED_TS with a reason and a TODO, or move it to Rust"
    );
    let py: BTreeSet<String> = files
        .iter()
        .map(|p| rel(p))
        .filter(|r| r.ends_with(".py") && r.starts_with("tools/platypus_ts/"))
        .collect();
    assert!(
        py.is_empty(),
        "no Python step under tools/platypus_ts: {py:?}"
    );
}

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
    use platypus_ts::builtins::{BUILTINS, Place, top_names};
    let ms: Vec<String> = platypus_build::manifests()
        .into_iter()
        .filter(|m| !m.contains("\"namespace\":\"Ui\""))
        .collect();
    let engine = manifest_names(&ms);
    let twice: Vec<_> = top_names()
        .into_iter()
        .filter(|n| engine.contains(*n))
        .collect();
    assert!(
        twice.is_empty(),
        "declared by a manifest and by builtins.rs: {twice:?}"
    );
    // A member of a class or namespace no manifest has would be dropped silently.
    let mut ownerless = Vec::new();
    for b in BUILTINS {
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
