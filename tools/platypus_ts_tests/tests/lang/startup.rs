// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `register(Class)` as an ordinary run-time call (in a branch, a loop, a function, after an
//! `await`) and the order the registrations happen in. IDs are in TESTS.md (M9-TS-3211..).
use web_ui_rt::{StartupState, registered_classes, startup_state, ts_rt_run_loop};
use web_ui_ts::jit::{Game, JitOptions, compile_and_load};

fn manifests() -> Vec<String> {
    let mut v = vec![platypus_scene::api::scene_api::MANIFEST.to_json()];
    v.extend(platypus_engine_api::platypus_math::manifests());
    v.extend(platypus_components::manifests());
    v
}

fn compile(src: &str, prefix: &'static str, base: u32) -> Result<Game, String> {
    compile_and_load(
        src,
        &JitOptions {
            prefix,
            struct_base: base,
            manifests: &manifests(),
            ..Default::default()
        },
    )
}

fn names() -> Vec<String> {
    registered_classes().into_iter().map(|(n, _)| n).collect()
}

const CLASSES: &str =
    "class A extends Component {}\nclass B extends Component {}\nclass C extends Component {}\n";

// M9-TS-3211
//
// A class registered before an `await` is there when `init` returns; one registered after it is
// there once the event loop has run, and the startup is pending in between.
#[test]
fn register_works_before_and_after_a_top_level_await() {
    let _serial = platypus_ts_tests::serial();
    let src = format!(
        "{CLASSES}register(A);\nawait Promise.resolve(1);\nregister(B);\nawait new Promise<void>((r) => setTimeout(r, 3));\nregister(C);\n"
    );
    let game = compile(&src, "st0_", 920_000).unwrap();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    game.init().unwrap();
    assert_eq!(names(), ["A"]);
    assert!(matches!(startup_state(), StartupState::Pending(_)));
    ts_rt_run_loop();
    assert_eq!(names(), ["A", "B", "C"], "registration order is call order");
    assert_eq!(startup_state(), StartupState::Done);
}

// M9-TS-3212
//
// `register` inside `if`, a loop and a function: only the calls that run register.
#[test]
fn register_works_in_a_branch_a_loop_and_a_function() {
    let _serial = platypus_ts_tests::serial();
    let src = format!(
        "{CLASSES}const flag: boolean = Math.random() < 2;\nif (flag) {{ register(B); }} else {{ register(A); }}\nfunction setup(): void {{ register(C); }}\nfor (let i = 0; i < 3; i++) {{ if (i === 1) setup(); }}\nregister(B);\n"
    );
    let game = compile(&src, "st1_", 921_000).unwrap();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    game.init().unwrap();
    // `B` is registered twice and keeps its first place; `A` never runs.
    assert_eq!(names(), ["B", "C"]);
}

// M9-TS-3213
//
// `{ spawn: false }` is kept per class and read at run time; the object form lists several.
#[test]
fn the_spawn_option_is_recorded_per_class() {
    let _serial = platypus_ts_tests::serial();
    let src = format!(
        "{CLASSES}const spawn: boolean = Math.random() > 2;\nregister({{ A, B }}, {{ spawn: false }});\nregister(C, {{ spawn }});\n"
    );
    let game = compile(&src, "st2_", 922_000).unwrap();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    game.init().unwrap();
    assert_eq!(
        registered_classes(),
        [
            ("A".to_string(), false),
            ("B".to_string(), false),
            ("C".to_string(), false)
        ]
    );
}

// M9-TS-3214
//
// Registering a name that is not a component class of the program, or something that is not a
// class name, is a compile error that says so; a program's own `register` function is left alone.
#[test]
fn register_of_something_else_is_a_clear_error() {
    let _serial = platypus_ts_tests::serial();
    let e = compile(&format!("{CLASSES}register(Nope);\n"), "st3_", 923_000)
        .err()
        .unwrap();
    assert!(e.contains("`Nope` is not a class"), "{e}");
    let e = compile(
        &format!("{CLASSES}const k = A;\nregister(k);\n"),
        "st4_",
        924_000,
    )
    .err()
    .unwrap();
    assert!(e.contains("`k` is not a class"), "{e}");
    let e = compile(
        &format!(
            "{CLASSES}register(new A());
"
        ),
        "st7_",
        927_000,
    )
    .err()
    .unwrap();
    assert!(e.contains("takes a class name"), "{e}");
    let own = "function register(x: number): number { return x + 1; }\nexport function result(): number { return register(1); }\n";
    assert!(compile(own, "st5_", 925_000).is_ok());
}

// M9-TS-3215
//
// A new run of the top level starts with no registrations (a hot reload registers again).
#[test]
fn the_registrations_start_over_with_each_run_of_the_top_level() {
    let _serial = platypus_ts_tests::serial();
    let src = format!("{CLASSES}register(A);\n");
    let game = compile(&src, "st6_", 926_000).unwrap();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    game.init().unwrap();
    game.init().unwrap();
    assert_eq!(names(), ["A"]);
}
