// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The `platypus-build` program itself: which commands print the empty-field warnings.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn game(tag: &str, project: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("platypus_build_cli_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/main.ts"), "export const a = 1;").unwrap();
    std::fs::write(d.join("project.json"), project).unwrap();
    d
}

fn run(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_platypus-build"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

const EMPTY_COMPANY: &str = "\"company\" is empty (the exe's version info will have no company)";

// M9-TS-2724
#[test]
fn build_and_package_warn_about_empty_fields_but_check_does_not() {
    let g = game("warn", r#"{ "name": "g", "company": "" }"#);
    let parent = g.parent().unwrap().to_path_buf();
    let games = parent.join(format!("platypus_build_cli_games_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&games);
    std::fs::create_dir_all(&games).unwrap();

    let b = run(&["build", g.to_str().unwrap(), "--no-tsc"], &parent);
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let err = String::from_utf8_lossy(&b.stderr).into_owned();
    assert!(
        err.contains("warning: ") && err.contains("project.json: ") && err.contains(EMPTY_COMPANY),
        "{err}"
    );

    let host = env!("CARGO_BIN_EXE_platypus-build");
    let out = g.join("out/g.exe");
    let p = run(
        &[
            "package",
            g.to_str().unwrap(),
            "--no-tsc",
            "--host",
            host,
            "--out",
            out.to_str().unwrap(),
        ],
        &parent,
    );
    let err = String::from_utf8_lossy(&p.stderr).into_owned();
    assert!(p.status.success(), "{err}");
    assert!(err.contains(EMPTY_COMPANY), "{err}");
    assert!(out.is_file());

    // `check` builds the same game and stays quiet about it.
    std::fs::rename(&g, games.join("g")).unwrap();
    let c = run(&["check", games.to_str().unwrap()], &parent);
    let (o, e) = (
        String::from_utf8_lossy(&c.stdout).into_owned(),
        String::from_utf8_lossy(&c.stderr).into_owned(),
    );
    assert!(c.status.success(), "{o}{e}");
    assert!(o.contains("1 built, 0 failed"), "{o}");
    assert!(!o.contains("is empty") && !e.contains("is empty"), "{o}{e}");
    let _ = std::fs::remove_dir_all(&games);
}

// M9-TS-3383
#[test]
fn prebuild_runs_in_the_game_folder_before_compiling_and_can_be_skipped() {
    // The entry imports a file only the prebuild command writes.
    let g = game(
        "prebuild",
        r#"{ "name": "g", "prebuild": "echo export const made = 1;> src/gen.ts" }"#,
    );
    std::fs::write(
        g.join("src/main.ts"),
        "import { made } from \"./gen.ts\";\nexport const a = made;",
    )
    .unwrap();
    let parent = g.parent().unwrap().to_path_buf();
    let b = run(&["build", g.to_str().unwrap(), "--no-tsc"], &parent);
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    assert!(g.join("src/gen.ts").is_file());
    // Skipped, a missing file is the build's error; a failing command stops it with its text.
    std::fs::remove_file(g.join("src/gen.ts")).unwrap();
    let s = run(
        &["build", g.to_str().unwrap(), "--no-tsc", "--no-prebuild"],
        &parent,
    );
    assert!(!s.status.success());
    std::fs::write(
        g.join("project.json"),
        r#"{ "name": "g", "prebuild": "exit 3" }"#,
    )
    .unwrap();
    let f = run(&["build", g.to_str().unwrap(), "--no-tsc"], &parent);
    let err = String::from_utf8_lossy(&f.stderr).into_owned();
    assert!(
        !f.status.success() && err.contains("prebuild `exit 3` failed"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&g);
}

// M3-INS-08
#[test]
fn add_ui_inspector_compiles_the_overlay_in_and_marks_the_bundle() {
    let g = game("inspector", r#"{ "name": "g" }"#);
    let parent = g.parent().unwrap().to_path_buf();
    let pbundle = g.join("build/game.pbundle");

    // The fast path: no overlay, no mark.
    let b = run(&["build", g.to_str().unwrap(), "--no-tsc"], &parent);
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let text = std::fs::read_to_string(&pbundle).unwrap();
    assert!(!text.contains("uiInspector") && !text.contains("UiInspOverlay"));
    assert!(
        !g.join("build/game.css").exists(),
        "no inspector styles without the flag"
    );

    // With the flag the overlay is part of the program, and the host is told.
    let b = run(
        &[
            "build",
            g.to_str().unwrap(),
            "--no-tsc",
            "--add-ui-inspector",
        ],
        &parent,
    );
    let err = String::from_utf8_lossy(&b.stderr).into_owned();
    assert!(b.status.success(), "{err}");
    assert!(
        err.contains("--add-ui-inspector"),
        "warns it is a development build: {err}"
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&pbundle).unwrap()).unwrap();
    assert_eq!(v["uiInspector"], true);
    assert!(
        v["source"]
            .as_str()
            .unwrap()
            .contains("function UiInspOverlay")
    );
    // Its styles are a real CSS file added to game.css, not part of the program.
    let css = std::fs::read_to_string(g.join("build/game.css")).unwrap();
    assert!(
        css.contains(".uii-card") && css.contains("prefers-color-scheme"),
        "{css}"
    );
    assert!(!v["source"].as_str().unwrap().contains("--uii-bg"));
    assert_eq!(v["css"], "game.css");
    let _ = std::fs::remove_dir_all(&g);
}
