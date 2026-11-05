// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The JIT: the same code generation as the object writer, into the running process
//! (`platypus_ts::jit`). IDs are in TESTS.md (M9-TS-1900.., M9-API-600..).
use platypus_components::NativeHost;
use platypus_scene::Scene;
use platypus_scene::api::scene_api as api;
use platypus_ts::jit::{JitOptions, compile_and_load};
use platypus_ts_rt::link_anchor;
use ts_api::Handle;

/// Compiled programs share the thread's heap and globals: one test at a time.
fn bench(name: &str) -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bench");
    std::fs::read_to_string(dir.join(name)).unwrap()
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100_0000_01b3)
    })
}

// M9-TS-1900
//
// Making code generation generic over the Cranelift module type must not change the object file:
// the sizes and hashes below were taken from the output before the change (and again after the
// rename to Platypus, where each `iron_*` symbol became `platypus_*`, 4 bytes longer).
#[test]
fn the_object_writer_output_is_unchanged_by_the_generic_codegen() {
    for (file, len, hash) in [
        ("sine.ts", 5887, 0x5410_9d5b_d8e5_4895),
        // strings.ts grew with the prelude's `Error.stack` getter (2026-10-29).
        ("strings.ts", 30620, 0x4758_1889_d9e7_0b39),
        ("classes.ts", 16581, 0x80d1_77d9_ba9b_c258),
    ] {
        let out = platypus_ts::compile_source(&bench(file), "speed").unwrap();
        assert_eq!(out.object.len(), len, "{file}");
        assert_eq!(fnv(&out.object), hash, "{file}");
    }
}

// M9-TS-1901
//
// A plain program compiled in the process: an exported function is called through its address.
#[test]
fn a_program_compiled_in_process_runs() {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let game = compile_and_load(
        "export function add(a: number, b: number): number { return a * 2 + b; }\n\
         export function fib(n: number): number { return n < 2 ? n : fib(n - 1) + fib(n - 2); }",
        &JitOptions {
            prefix: "jit1_",
            struct_base: 90_000,
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    let add: unsafe extern "C" fn(f64, f64) -> f64 =
        unsafe { std::mem::transmute(game.function("add").unwrap()) };
    let fib: unsafe extern "C" fn(f64) -> f64 =
        unsafe { std::mem::transmute(game.function("fib").unwrap()) };
    assert_eq!(unsafe { add(3.0, 4.0) }, 10.0);
    assert_eq!(unsafe { fib(15.0) }, 610.0);
    assert!(game.function_names().contains(&"add".to_string()));
}

// M9-TS-1902
//
// Strings, arrays and the runtime's collector work from JIT code (imports of `ts_rt_*` are
// resolved from the generated table).
#[test]
fn jit_code_uses_the_runtime() {
    let _serial = platypus_ts_tests::serial();
    let game = compile_and_load(
        "export function words(n: number): number {\n\
           const parts: string[] = [];\n\
           for (let i = 0; i < n; i++) parts.push(`w${i}`);\n\
           return parts.join(\",\").length + Math.floor(n / 3) + Math.ceil(n / 3);\n\
         }",
        &JitOptions {
            prefix: "jit2_",
            struct_base: 91_000,
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    let f: unsafe extern "C" fn(f64) -> f64 =
        unsafe { std::mem::transmute(game.function("words").unwrap()) };
    // "w0,w1,...,w9" is 10 * 2 + 9 = 29, plus 3 and 4.
    assert_eq!(unsafe { f(10.0) }, 36.0);
}

// M9-TS-1903
//
// A missing symbol is reported by name before anything runs.
#[test]
fn an_unresolved_engine_function_is_an_error_naming_it() {
    let manifest = r#"{"namespace":"Nope","functions":[{"name":"gone","symbol":"platypus_Nope_gone","params":[],"returns":"f64"}]}"#;
    let e = compile_and_load(
        "export function f(): number { return Nope.gone(); }",
        &JitOptions {
            prefix: "jit3_",
            struct_base: 92_000,
            manifests: &[manifest.to_string()],
            ..Default::default()
        },
    );
    match e {
        Ok(_) => panic!("expected an error"),
        Err(e) => assert!(e.contains("platypus_Nope_gone"), "{e}"),
    }
}

fn scene_manifests() -> Vec<String> {
    let mut v = vec![platypus_scene::api::scene_api::MANIFEST.to_json()];
    v.extend(platypus_engine_api::platypus_math::manifests());
    v.extend(platypus_components::manifests());
    v
}

// M9-TS-1904
//
// The component program of `bench/components.ts` (engine calls into the real scene) JIT-compiled:
// the class table is read at run time and `NativeHost` drives the components.
#[test]
fn a_jit_compiled_component_table_drives_the_native_host() {
    let _serial = platypus_ts_tests::serial();
    let game = compile_and_load(
        &bench("components.ts"),
        &JitOptions {
            prefix: "jit4_",
            struct_base: 93_000,
            manifests: &scene_manifests(),
            ..Default::default()
        },
    )
    .unwrap();
    let names: Vec<_> = game.class_meta().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Mover", "Fast", "Faulty", "Physics"]);
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    game.init().unwrap();
    let classes = game.classes().unwrap();
    // The host's hook list: nine lifecycle hooks, then the contact hooks.
    let have =
        |c: usize| -> Vec<bool> { classes[c].hooks[..9].iter().map(|h| h.is_some()).collect() };
    assert_eq!(
        have(0),
        [true, true, true, true, false, false, true, false, false]
    );
    let go = api::create();
    let mut host = NativeHost::new(classes);
    let id = host.spawn("Mover", Some(go.to_raw())).unwrap();
    for _ in 0..3 {
        assert!(host.update(0.5, &|_| true).is_empty());
    }
    assert_eq!(api::local_position(go).x, 3.0);
    host.destroy(id);
    api::destroy(go);
}

// M9-API-600
//
// Reading a namespace's manifest registers the addresses of its shims, and every symbol any
// manifest names resolves: nothing is listed by hand.
#[test]
fn every_manifest_function_symbol_resolves_after_reading_the_manifests() {
    let mut all = scene_manifests();
    all.extend(platypus_engine_api::manifests());
    all.extend(platypus_scenes::manifests());
    all.extend(platypus_physics_api::manifests());
    all.extend(platypus_render_api::manifests());
    all.extend(platypus_components::manifests());
    let mut missing = Vec::new();
    let mut n = 0;
    for m in &all {
        let v: serde_json::Value = serde_json::from_str(m).unwrap();
        let mut stack = vec![&v];
        while let Some(x) = stack.pop() {
            match x {
                serde_json::Value::Object(o) => {
                    if let Some(s) = o.get("symbol").and_then(|s| s.as_str()) {
                        n += 1;
                        if ts_api::symbol(s).is_none() {
                            missing.push(s.to_string());
                        }
                    }
                    stack.extend(o.values());
                }
                serde_json::Value::Array(a) => stack.extend(a),
                _ => {}
            }
        }
    }
    assert!(n > 500, "only {n} symbols were found");
    assert!(missing.is_empty(), "{missing:?}");
}

// M9-API-601
//
// The runtime's own table (generated by its build script) holds the runtime functions.
#[test]
fn the_runtime_symbol_table_lists_its_exports() {
    let t = platypus_ts_rt::symbols();
    assert!(t.len() > 200, "{}", t.len());
    for want in ["ts_rt_obj_new", "ts_rt_str_trim_gc", "ts_rt_tan"] {
        assert!(t.iter().any(|(n, a)| *n == want && *a != 0), "{want}");
    }
}

// M9-TS-2009
//
// A component class that no `register(..)` names is still in the class table (so
// `addComponent(Class)` finds it) and is not among the classes the register calls named.
#[test]
fn an_unregistered_component_class_is_in_the_table_but_not_registered() {
    let _serial = platypus_ts_tests::serial();
    let src = "class Started extends Component { n: number = 1; onUpdate(dt: number): void { this.n++; } }\n\
               class OnlyAdded extends Started { m: number = 2; }\n\
               register({ Started });\n";
    let game = compile_and_load(
        src,
        &JitOptions {
            prefix: "jit5_",
            struct_base: 94_000,
            manifests: &scene_manifests(),
            ..Default::default()
        },
    )
    .unwrap();
    let meta: Vec<_> = game.class_meta().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(meta, ["Started", "OnlyAdded"]);
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    game.init().unwrap();
    // Which classes are registered is what the `register(..)` calls did when they ran.
    assert_eq!(
        platypus_ts_rt::registered_classes(),
        [("Started".to_string(), true)]
    );
    assert!(game.classes().is_ok());
}

// M9-TS-2874
//
// A program that imports a module of `src/modules` by its bare name runs.
#[test]
fn a_program_importing_a_src_modules_file_by_name_runs() {
    let _serial = platypus_ts_tests::serial();
    let files: std::collections::HashMap<&str, &str> = [
        (
            "src/main.ts",
            "import { lerp } from \"mathx\";\nexport function mid(a: number, b: number): number { return lerp(a, b, 0.5); }",
        ),
        (
            "src/modules/mathx/index.ts",
            "export function lerp(a: number, b: number, t: number): number { return a + (b - a) * t; }",
        ),
    ]
    .into_iter()
    .collect();
    let bundle =
        platypus_ts::modules::bundle_with("src/main.ts", &|p| files.get(p).map(|s| s.to_string()))
            .unwrap();
    let game = compile_and_load(
        &bundle.source,
        &JitOptions {
            prefix: "jitm_",
            struct_base: 95_000,
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    let f: unsafe extern "C" fn(f64, f64) -> f64 =
        unsafe { std::mem::transmute(game.function("mid").unwrap()) };
    assert_eq!(unsafe { f(2.0, 6.0) }, 4.0);
}

// M9-TS-2876
//
// A top-level function nothing refers to is not compiled: a construct the compiler lacks in it
// does not fail the build. One that is called (directly, through another function, or as a
// value) still fails, with its line.
#[test]
fn unused_functions_with_unsupported_constructs_do_not_fail_the_build() {
    let _serial = platypus_ts_tests::serial();
    let unused = "function unusedA(): number {\n  with (Math) { return 1; }\n}\n\
                  function unusedB(): number { return unusedA() + 1; }\n";
    let game = compile_and_load(
        &format!("{unused}export function main(): number {{ return 7; }}"),
        &JitOptions {
            prefix: "jitu_",
            struct_base: 96_000,
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    let f: unsafe extern "C" fn() -> f64 =
        unsafe { std::mem::transmute(game.function("main").unwrap()) };
    assert_eq!(unsafe { f() }, 7.0);

    for use_it in [
        "export function main(): number { return unusedB(); }",
        "function run(g: () => number): number { return g(); }\nexport function main(): number { return run(unusedA); }",
    ] {
        let e = platypus_ts::compile_source(&format!("{unused}{use_it}"), "speed")
            .err()
            .expect("a used function must fail");
        assert!(e.contains("unsupported statement"), "{e}");
        assert!(e.contains("line 2"), "{e}");
    }
}

/// Compiles `source` and calls its `main(): number`.
fn run_main(source: &str, prefix: &'static str, struct_base: u32) -> f64 {
    let _serial = platypus_ts_tests::serial();
    let game = compile_and_load(
        source,
        &JitOptions {
            prefix,
            struct_base,
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    let f: unsafe extern "C" fn() -> f64 =
        unsafe { std::mem::transmute(game.function("main").unwrap()) };
    unsafe { f() }
}

// M9-TS-2890
//
// Interface method signatures are function-valued properties (optional ones may be missing),
// named tuple elements are plain elements, and an object literal's `at(t) { .. }` is the
// property `at: function (t) { .. }` with its parameters typed by the interface.
#[test]
fn method_signatures_named_tuples_and_method_shorthand_compile_and_run() {
    let src = "interface Act { name: string; at(t: number): number; show?(n: number): number }\n\
               type Cue = [t: number, run: (n: number) => number];\n\
               const act: Act = { name: \"a\", at(t) { return t + 1; } };\n\
               const cues: Cue[] = [[0.5, (n) => n * 2]];\n\
               export function main(): number {\n\
                 const shown = act.show !== undefined ? 100 : 0;\n\
                 return act.at(2) + cues[0]![1](cues[0]![0]) + shown;\n\
               }";
    assert_eq!(run_main(src, "jitm_", 97_000), 4.0);
}

// M9-TS-2891
//
// A closure inside a closure in a top-level initialiser can use the outer one's locals (the
// scan that boxes captured variables ran only for function bodies).
#[test]
fn a_closure_in_a_closure_of_a_top_level_initialiser_shares_its_locals() {
    let src = "const fs: ((n: number) => number)[] = [\n\
                 (a: number) => {\n\
                   let p = a * 2;\n\
                   const g = (b: number) => p + b;\n\
                   p += 1;\n\
                   return g(1);\n\
                 },\n\
               ];\n\
               export function main(): number { return fs[0]!(5); }";
    assert_eq!(run_main(src, "jitn_", 98_000), 12.0);
}
