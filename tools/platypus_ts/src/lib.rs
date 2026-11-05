// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `platypus_ts`: an ahead-of-time compiler for Platypus's TypeScript gameplay code.
//! TypeScript (oxc) -> typed Platypus IR -> Cranelift object file. See `docs/research/platypus-ts-spike.md`.

pub mod assetpaths;
pub mod builtins;
pub mod classmeta;
pub mod codegen;
pub mod debug;
pub mod debug_eval;
pub mod frames;
pub mod ir;
pub mod jit;
pub mod layout;
pub mod lower;
pub mod manifest;
pub mod modules;
pub mod sroa;
pub mod throws;
pub mod timing;
pub mod types_dts;
pub mod ui_dts;
#[cfg(windows)]
pub mod unwind;

/// The syntax tree compiler extensions (`lower::ext`) are handed, at the compiler's own version.
pub use oxc_ast;

/// What a compile produced.
pub struct Output {
    pub object: Vec<u8>,
    pub bindings: String,
    pub ir: String,
    /// The engine symbols the compiled code calls (unused top-level functions left out), for the
    /// host's own analysis (Platypus: `platypus_core::renderer::usage_of_symbols`).
    pub externs: Vec<String>,
    /// Engine names the compiled code uses without an extern call (the host-class base).
    pub uses: Vec<String>,
    /// Wall-clock time of each phase.
    pub lower_time: std::time::Duration,
    pub codegen_time: std::time::Duration,
}

/// TypeScript source -> object file bytes, Rust bindings and an IR dump.
pub fn compile_source(src: &str, opt: &str) -> Result<Output, String> {
    compile_with_prefix(src, opt, "")
}

/// Like `compile_source`, naming exported symbols `ts_<prefix><name>`.
pub fn compile_with_prefix(src: &str, opt: &str, prefix: &str) -> Result<Output, String> {
    compile_with(src, opt, prefix, 0)
}

/// Bundles the program that starts at `entry` (see `modules`), then compiles it like
/// `compile_with_manifests`. Returns the bundle too (its CSS list, files and source text).
pub fn compile_program(
    entry: &std::path::Path,
    opt: &str,
    prefix: &str,
    struct_base: u32,
    manifests: &[String],
) -> Result<(Output, modules::Bundle), String> {
    let bundle = modules::bundle(entry)?;
    let out = compile_with_manifests(&bundle.source, opt, prefix, struct_base, manifests)?;
    Ok((out, bundle))
}

/// Like `compile_with_prefix`, with object headers numbered from `struct_base`. Modules that
/// share a process (and so a GC heap) need disjoint ranges: leave 1000 or more between them.
pub fn compile_with(
    src: &str,
    opt: &str,
    prefix: &str,
    struct_base: u32,
) -> Result<Output, String> {
    compile_with_manifests(src, opt, prefix, struct_base, &[])
}

/// Like `compile_with`, with engine API manifests (the JSON `ts_api` generates): the TypeScript
/// can call `Namespace.function(...)` and the call is a direct call of the function's symbol.
pub fn compile_with_manifests(
    src: &str,
    opt: &str,
    prefix: &str,
    struct_base: u32,
    manifests: &[String],
) -> Result<Output, String> {
    compile_for_target(
        src,
        opt,
        prefix,
        struct_base,
        manifests,
        &codegen::ObjectTarget::default(),
    )
}

/// Like `compile_with_manifests`, writing the object for `target` (another triple, or
/// position-independent code for linking into a Linux executable).
pub fn compile_for_target(
    src: &str,
    opt: &str,
    prefix: &str,
    struct_base: u32,
    manifests: &[String],
    target: &codegen::ObjectTarget,
) -> Result<Output, String> {
    // Lowering recurses with the source's nesting; the main thread's 1 MiB is too little on Windows.
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(s, || {
                compile_inner(src, opt, prefix, struct_base, manifests, target)
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "compiler panicked".to_string())?
    })
}

/// The engine symbols the program calls and the names it uses otherwise, from lowering alone
/// (no code generation): what `Output::externs` and `Output::uses` hold, for a build that skips
/// compiling.
/// Lowers `src` with every function, also those nothing calls (a build skips them, so a construct
/// the compiler lacks in one would only show once it is called): the first error, if any.
pub fn check_unused(src: &str, manifests: &[String]) -> Result<(), String> {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(s, || {
                lower::lower_source_opts(src, manifests, true).map(|_| ())
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "compiler panicked".to_string())?
    })
}

pub fn extern_symbols(
    src: &str,
    manifests: &[String],
) -> Result<(Vec<String>, Vec<String>), String> {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(s, || {
                let module = lower::lower_source_with(src, manifests)?;
                let externs = module.externs.iter().map(|e| e.symbol.clone()).collect();
                Ok((externs, module.uses))
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "compiler panicked".to_string())?
    })
}

fn compile_inner(
    src: &str,
    opt: &str,
    prefix: &str,
    struct_base: u32,
    manifests: &[String],
    target: &codegen::ObjectTarget,
) -> Result<Output, String> {
    let t0 = std::time::Instant::now();
    let mut module = lower::lower_source_with(src, manifests)?;
    module.prefix = prefix.to_string();
    module.struct_base = struct_base;
    sroa::run(&mut module);
    let lower_time = t0.elapsed();
    let t1 = std::time::Instant::now();
    let compiled = codegen::compile_for(&module, opt, target)?;
    let codegen_time = t1.elapsed();
    Ok(Output {
        object: compiled.object,
        bindings: layout::rust_bindings(&module),
        ir: ir::dump(&module),
        externs: module.externs.iter().map(|e| e.symbol.clone()).collect(),
        uses: module.uses.clone(),
        lower_time,
        codegen_time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // M9-TS-3523
    #[test]
    fn a_generic_promise_function_does_not_make_a_then_for_every_pair_of_types() {
        // Forty types and one generic promise function: `then` copies are made for the pairs the
        // program calls, not for 40 x 40 of them (the editor's object was once 50 times too big).
        let mut src = String::new();
        for i in 0..40 {
            src += &format!(
                "interface T{i} {{ v{i}: number }}\nexport function make{i}(): T{i} {{ return {{ v{i}: {i} }}; }}\n"
            );
        }
        src += "function later<T>(v: T): Promise<T> { return Promise.resolve(v); }\n\
                export function t(): Promise<number> { return later<number>(1).then((n) => n + 1); }\n";
        let m = lower::lower_source(&src).unwrap();
        let lowered = m
            .funcs
            .iter()
            .filter(|f| f.name.starts_with("__then") && f.body.len() > 1)
            .count();
        assert!(lowered < 20, "{lowered} `then` copies lowered");
    }

    // M9-TS-3534
    #[test]
    fn an_object_can_be_position_independent_and_for_another_triple() {
        let src = "let n = 0; export function bump(): number { n = n + 1; return n; }";
        let object = |triple: Option<&str>, pic: bool| {
            let target = codegen::ObjectTarget {
                triple: triple.map(str::to_string),
                pic,
            };
            compile_for_target(src, "speed", "t_", 0, &[], &target)
                .unwrap()
                .object
        };
        let plain = object(None, false);
        let pic = object(None, true);
        assert_ne!(plain, pic);
        assert!(pic.windows(10).any(|w| w == b"ts_t_bump\0"));
        // musl differs from glibc only in the libc a host links: the same code on x86-64.
        if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
            assert_eq!(object(Some("x86_64-unknown-linux-musl"), true), pic);
        }
        let bad = compile_for_target(
            src,
            "speed",
            "t_",
            0,
            &[],
            &codegen::ObjectTarget {
                triple: Some("not-a-triple".into()),
                pic: true,
            },
        );
        assert!(bad.is_err());
    }

    // M9-TS-14
    #[test]
    fn a_discriminated_union_becomes_one_struct_with_every_variants_fields() {
        let m = lower::lower_source(
            r#"export type Ev = { kind: "a"; x: number } | { kind: "b"; y: boolean };
               export function mk(): Ev { return { kind: "b", y: true }; }"#,
        )
        .unwrap();
        let ev = m.structs.iter().find(|s| s.name == "Ev").unwrap();
        let names: Vec<_> = ev.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["kind", "x", "y"]);
        assert_eq!(m.tags, ["", "a", "b"]);
    }

    // M9-TS-15
    #[test]
    fn unsupported_code_is_an_error_with_a_line_number() {
        let e = lower::lower_source(
            "export function f(x: number): number {\n  return +new Proxy({}, {});\n}",
        )
        .err()
        .unwrap();
        assert!(e.contains("line 2"), "{e}");
        let e = lower::lower_source("export function g(x: any): number { return 1; }")
            .err()
            .unwrap();
        assert!(e.contains("line 1: `any` is not supported yet"), "{e}");
    }

    // M9-TS-2892
    #[test]
    fn errors_after_multi_line_jsx_are_reported_at_their_own_line() {
        let src = "export function A() {
  return (
    <box>
      <text>hi</text>
    </box>
  );
}
export function bad(): number {
  return +new Proxy({}, {});
}";
        let e = lower::lower_source(src).err().unwrap();
        assert!(e.contains("line 9"), "{e}");
    }

    // M9-TS-2893
    #[test]
    fn a_comment_naming_a_facade_trigger_does_not_pull_its_chunk_in() {
        // `LineRenderer2D` is a trigger, but the chunk is written over an engine class this
        // program's API does not have.
        let src = "// LineRenderer2D, Spline and Shapes. are only words here
export function f(): number { return 1; }";
        assert!(lower::lower_source(src).is_ok());
    }

    // M9-TS-16
    #[test]
    fn callbacks_are_inlined_and_the_object_has_exported_symbols() {
        let out = compile_source("export function total(xs: number[]): number { let s = 0; xs.forEach((x) => { s += x; }); return s; }", "speed").unwrap();
        assert!(!out.ir.contains("callc"), "{}", out.ir);
        assert!(out.object.windows(8).any(|w| w == b"ts_total"));
    }

    // M9-TS-55
    #[test]
    fn a_let_with_a_literal_initialiser_is_a_global_not_a_folded_constant() {
        let m = lower::lower_source(
            "let n = 0; const k = 2; export function f(): number { n = n + k; return n; }",
        )
        .unwrap();
        // `n` is a slot written by `f`; `k` is folded away.
        assert_eq!(m.globals.len(), 1);
        assert_eq!(m.globals[0].name, "n");
        let f = m.funcs.iter().find(|f| f.name == "f").unwrap();
        // Line markers (`Stmt::Line`) between statements are for crash reports.
        let first = f.body.iter().find(|s| !matches!(s, ir::Stmt::Line(_)));
        assert!(
            matches!(first, Some(ir::Stmt::StoreGlobal(0, _))),
            "{:?}",
            f.body
        );
    }

    // M9-TS-56
    #[test]
    fn assigning_a_const_is_an_error() {
        let e =
            lower::lower_source("const k = 2; export function f(): number { k = 3; return k; }")
                .err()
                .unwrap();
        assert!(e.contains("constant"), "{e}");
        let e = lower::lower_source(
            "const o = { v: 1 }; export function f(): number { o = { v: 2 }; return o.v; }",
        )
        .err()
        .unwrap();
        assert!(e.contains("constant"), "{e}");
    }

    // M9-TS-70
    #[test]
    fn bad_enums_and_jumps_are_errors_with_a_reason() {
        let e = lower::lower_source(
            "export function f(n: number): number { enum E { A = n } return E.A; }",
        );
        assert!(e.is_err());
        let e = lower::lower_source("enum E { A, B } export function f(): number { return E.C; }")
            .err()
            .unwrap();
        assert!(e.contains("no member `C`"), "{e}");
        let e = lower::lower_source(
            "enum E { A = \"a\", B } export function f(): number { return 1; }",
        )
        .err()
        .unwrap();
        assert!(e.contains("initialiser"), "{e}");
        let e = lower::lower_source("export function f(): number { continue; return 1; }")
            .err()
            .unwrap();
        assert!(e.contains("continue outside a loop"), "{e}");
        let e = lower::lower_source(
            "export function f(): number { sw: switch (1) { case 1: for (;;) { continue sw; } } return 1; }",
        )
        .err()
        .unwrap();
        assert!(e.contains("not a loop"), "{e}");
    }

    // M9-TS-182
    #[test]
    fn string_methods_that_need_more_work_are_errors_with_a_reason() {
        for call in ["\"a\".anchor(\"b\")", "\"a\".big()"] {
            let e = lower::lower_source(&format!(
                "export function f(): number {{ return {call} ? 1 : 0; }}"
            ))
            .err()
            .unwrap();
            assert!(e.contains("not supported yet"), "{call}: {e}");
        }
        // A literal with a lone surrogate is a string (WTF-8), not an error; as a union member it is one.
        assert!(lower::lower_source("export function f(): string { return \"\\uD800\"; }").is_ok());
        let m =
            lower::lower_source("export function f(): string { return \"a\\uD800b\"; }").unwrap();
        assert_eq!(m.strs, [b"a\xED\xA0\x80b".to_vec()]);
    }

    // M9-TS-191
    #[test]
    fn only_functions_that_can_throw_are_checked_after_a_call() {
        let m = lower::lower_source(
            "function a(): number { return 1; }              function b(x: number): number { if (x > 0) { throw new Error(\"x\"); } return 1; }              function c(): number { return b(1); }              function d(): number { return a(); }              export function e(): number { return d() + c(); }",
        )
        .unwrap();
        let may = |name: &str| m.may_throw[m.funcs.iter().position(|f| f.name == name).unwrap()];
        assert!(
            !may("a") && !may("d"),
            "callers of non-throwing code pay nothing"
        );
        assert!(may("b") && may("c") && may("e"));
        assert!(m.any_throw);
        let m = lower::lower_source("export function f(): number { return 1; }").unwrap();
        assert!(!m.any_throw && m.may_throw.iter().all(|t| !t));
    }

    // M9-TS-197
    #[test]
    fn a_throwing_function_nothing_reaches_does_not_make_calls_checked() {
        let m = lower::lower_source(
            "function unused(): void { throw new Error(\"never\"); }              export function f(): number { const g = (x: number): number => x + 1; return g(1); }",
        )
        .unwrap();
        assert!(!m.any_throw, "closure calls stay unchecked");
        let m = lower::lower_source(
            "export function f(): number { const g = (x: number): number => { if (x > 5) { throw new Error(\"x\"); } return x; }; return g(1); }",
        )
        .unwrap();
        assert!(m.any_throw);
    }

    // M9-TS-192
    #[test]
    fn throw_and_try_mistakes_are_errors_with_a_reason() {
        let e = lower::lower_source("export function f(): number { throw \"x\"; }")
            .err()
            .unwrap();
        assert!(e.contains("Error objects only"), "{e}");
        // A `try` needs a `catch` or a `finally` (the parser rejects the rest).
    }

    // M9-TS-205
    #[test]
    fn object_and_console_helpers_without_a_static_answer_are_errors_with_a_reason() {
        let pre = "interface P { a: number; b: string } const p: P = { a: 1, b: \"x\" };";
        for (body, want) in [
            ("return Object.entries(p).length;", "different types"),
            ("return Object.values(p).length;", "different types"),
            ("console.log(p); return 1;", "console.log of an object"),
        ] {
            let e = lower::lower_source(&format!("{pre} export function f(): number {{ {body} }}"))
                .err()
                .unwrap_or_default();
            assert!(e.contains(want), "{body}: {e}");
        }
    }

    // M9-TS-214
    #[test]
    fn collection_mistakes_are_errors_with_a_reason() {
        for (body, want) in [
            ("const m = new Map(); return m.size;", "needs a type"),
            (
                "const m = new Map<string, number>(); for (const e of m.entries()) { return 1; } return 0;",
                "[key, value]",
            ),
            (
                "const m = new Map<string, boolean>(); return m.get(\"a\") ? 1 : 0;",
                "Map.get over bool",
            ),
            (
                "const s = new Set<number[]>(); const m = new Map<Map<string, number>, number>(); return m.size + s.size;",
                "",
            ),
        ] {
            let e = lower::lower_source(&format!("export function f(): number {{ {body} }}"));
            if want.is_empty() {
                assert!(e.is_ok(), "{body}: {e:?}");
            } else {
                let e = e.err().unwrap_or_default();
                assert!(e.contains(want), "{body}: {e}");
            }
        }
    }

    // M9-TS-221
    #[test]
    fn json_without_a_type_or_with_unsupported_arguments_is_an_error_with_a_reason() {
        for (body, want) in [
            ("const x = JSON.parse(\"1\"); return 1;", "needs a type"),
            (
                "const x: number = JSON.parse(\"1\", (k, v) => v); return x;",
                "reviver",
            ),
            (
                "return JSON.stringify([1], (k, v) => v).length;",
                "replacer",
            ),
            (
                "const f = () => 1; return JSON.stringify(f).length;",
                "not supported",
            ),
        ] {
            let e = lower::lower_source(&format!("export function f(): number {{ {body} }}"))
                .err()
                .unwrap_or_default();
            assert!(e.contains(want), "{body}: {e}");
        }
    }

    // M9-TS-231
    #[test]
    fn an_invalid_regex_literal_is_a_compile_error_with_the_engines_message() {
        let e = lower::lower_source("export function f(): boolean { return /(/.test(\"a\"); }")
            .err()
            .unwrap();
        assert!(e.contains("Invalid regular expression: missing )"), "{e}");
        let e = lower::lower_source("export function f(): boolean { return /a/gg.test(\"a\"); }")
            .err()
            .unwrap_or_default();
        assert!(!e.is_empty(), "duplicate flags are rejected");
    }

    // M9-TS-239
    #[test]
    fn a_replace_callback_needs_its_groups_known_at_compile_time() {
        let e = lower::lower_source(
            "export function f(re: RegExp): string { return \"a\".replace(re, (m, g) => g); }",
        )
        .err()
        .unwrap();
        assert!(e.contains("groups must be known"), "{e}");
        let e = lower::lower_source(
            "export function f(): string { return \"a\".replace(/(a)/g, (m, g, o, s, x, y) => g); }",
        )
        .err()
        .unwrap();
        assert!(e.contains("at most"), "{e}");
        // The match alone needs no group list, and a `const` declared from a literal has one.
        assert!(
            lower::lower_source(
                "export function f(re: RegExp): string { return \"a\".replace(re, (m) => m); }"
            )
            .is_ok()
        );
        assert!(lower::lower_source(
            "export function f(): string { const re = /(a)/g; return \"a\".replace(re, (m, g) => g ?? \"\"); }"
        )
        .is_ok());
    }

    // M9-TS-253
    #[test]
    fn date_formatting_options_must_be_literals_and_dates_subtract() {
        let e = lower::lower_source(
            "export function f(m: string): string { return new Date(0).toLocaleString(\"en\", { month: m }); }",
        )
        .err()
        .unwrap();
        assert!(e.contains("must be a literal"), "{e}");
        for body in [
            "new Date(0).toLocaleDateString(\"de\", { dateStyle: \"short\" })",
            "new Date(0).toLocaleTimeString()",
            "new Intl.DateTimeFormat(\"fr\").format(new Date(0))",
        ] {
            assert!(
                lower::lower_source(&format!("export function f(): string {{ return {body}; }}"))
                    .is_ok(),
                "{body}"
            );
        }
        assert!(
            lower::lower_source(
                "export function f(): number { return new Date(0) - new Date(1); }"
            )
            .is_ok()
        );
    }

    // M9-TS-263
    #[test]
    fn intl_options_must_be_literals() {
        let e = lower::lower_source(
            "export function f(o: string): number { return \"a\".localeCompare(\"b\", \"en\", { sensitivity: o }); }",
        )
        .err()
        .unwrap();
        assert!(e.contains("must be a literal"), "{e}");
        let e = lower::lower_source(
            "export function f(o: Intl.Collator): number { return o.resolvedOptions().numeric ? 1 : 0; }",
        )
        .err()
        .unwrap_or_default();
        assert!(!e.is_empty(), "resolvedOptions is not built");
        assert!(
            lower::lower_source(
                "export function f(): number { return new Intl.Collator(\"de\", { numeric: true }).compare(\"a\", \"b\"); }"
            )
            .is_ok()
        );
    }

    // M9-TS-369
    #[test]
    fn bigint_mixing_and_unary_plus_are_compile_errors() {
        for body in [
            "export function f(a: bigint): bigint { return a + 1; }",
            "export function f(a: bigint, b: number): bigint { return a * b; }",
            "export function f(a: bigint): number { return +a; }",
            "export function f(a: bigint): bigint { return a >>> 1n; }",
        ] {
            let e = lower::lower_source(body).err().unwrap_or_default();
            assert!(!e.is_empty(), "{body}");
        }
        assert!(
            lower::lower_source(
                "export function f(a: bigint): boolean { return a < 5 && a != 2n; }"
            )
            .is_ok()
        );
    }

    // M9-TS-393
    #[test]
    fn generator_limits_are_compile_errors_with_a_reason() {
        let e = lower::lower_source(
            "function* g() { yield 1; } export function f(): number { return 1; }",
        )
        .err()
        .unwrap();
        assert!(e.contains("return type"), "{e}");
        let e = lower::lower_source(
            "function* g(): Generator<number> { const x = (yield* [1]) + 1; yield x; } export function f(): number { return 1; }",
        )
        .err()
        .unwrap();
        assert!(e.contains("yield*"), "{e}");
        assert!(
            lower::lower_source(
                "function* g(): Generator<number> { yield 1; } export function f(): number { for (const x of g()) return x; return 0; }"
            )
            .is_ok()
        );
    }

    // M9-TS-430
    #[test]
    fn async_limits_are_compile_errors_with_a_reason() {
        let tail = " export function f(): number { return 1; }";
        for (src, want) in [
            (
                "async function* g(): Promise<number> { return 1; }",
                "generator",
            ),
            ("function g(): number { await 1; return 1; }", "await"),
            ("const p = new Promise((r) => r(1));", "type written"),
            ("async function g() { return 1; }", "needs its type written"),
            (
                "async function g(): number { return 1; }",
                "must return `Promise<T>`",
            ),
            (
                "async function g(): Promise<Promise<number>> { return Promise.resolve(1); }",
                "promise of a promise",
            ),
        ] {
            let e = lower::lower_source(&format!("{src}{tail}"))
                .err()
                .unwrap_or_default();
            assert!(e.contains(want), "{src}: {e}");
        }
    }

    // M9-TS-84
    #[test]
    fn calls_with_the_wrong_number_of_arguments_are_errors() {
        let src = "function f(a: number, b: number): number { return a + b; }";
        let e = lower::lower_source(&format!(
            "{src} export function g(): number {{ return f(1); }}"
        ))
        .err()
        .unwrap();
        assert!(e.contains("arguments"), "{e}");
        let e = lower::lower_source(&format!(
            "{src} export function g(): number {{ return f(1, 2, 3); }}"
        ))
        .err()
        .unwrap();
        assert!(e.contains("at most 2"), "{e}");
        let e = lower::lower_source(
            "function f(a: number, b: number): number { return a + b; } \
             export function g(xs: number[]): number { return f(...xs, 1); }",
        )
        .err()
        .unwrap();
        assert!(e.contains("must be the last argument"), "{e}");
    }

    // M9-TS-110
    #[test]
    fn array_calls_that_cannot_match_javascript_are_errors_that_say_why() {
        let e = lower::lower_source(
            "export function f(xs: boolean[]): boolean[] { return xs.sort(); }",
        )
        .err()
        .unwrap();
        assert!(e.contains("sort over"), "{e}");
        // `reduce` with no initial value starts from the first element (M9-TS-3386).
        assert!(
            lower::lower_source(
                "export function f(xs: number[]): number { return xs.reduce((a, x) => a + x); }",
            )
            .is_ok()
        );
        let e = lower::lower_source(
            "export function f(xs: boolean[]): boolean | undefined { return xs.pop(); }",
        )
        .err()
        .unwrap();
        assert!(e.contains("pop"), "{e}");
    }

    // M9-TS-574
    #[test]
    fn generics_mistakes_are_errors_with_a_reason() {
        let e = lower::lower_source(
            "function mk<T>(): T[] { return []; } export function f(): number { return mk().length; }",
        )
        .err()
        .unwrap();
        assert!(e.contains("cannot infer the type argument `T`"), "{e}");
        let e = lower::lower_source("export function id<T>(x: T): T { return x; }")
            .err()
            .unwrap();
        assert!(e.contains("cannot be exported"), "{e}");
        let e = lower::lower_source(
            "function two<T>(a: T, b: T): T { return a; } export function f(): number { return two(1, \"a\"); }",
        )
        .err()
        .unwrap();
        assert!(e.contains("conflicting types for `T`"), "{e}");
        let e = lower::lower_source(
            "class A { m<T>(x: T): T { return x; } static s<T>(x: T): T { return x; } } export function f(): number { return 1; }",
        )
        .err()
        .unwrap();
        assert!(e.contains("only plain instance methods"), "{e}");
    }

    // M9-TS-592
    #[test]
    fn field_types_are_inferred_from_constants_arithmetic_and_object_literals() {
        let m = lower::lower_source(
            "const W = 1008; const C = { x: W / 2, y: 3 }; class A { w = W / 3; c = { x: C.x, y: C.y }; s = 'a' + 'b'; } export function f(): number { const a = new A(); return a.w + a.c.x + a.s.length; }",
        );
        assert!(m.is_ok(), "{:?}", m.err());
        let e = lower::lower_source(
            "class B { v = someCall(); } export function f(): number { return 1; }",
        )
        .err()
        .unwrap();
        assert!(e.contains("needs a type annotation"), "{e}");
    }

    // M9-TS-119
    #[test]
    fn class_features_that_are_not_built_yet_are_errors_that_say_so() {
        let ext = lower::lower_source(
            "class B extends Widget { y = 2; } export function f(): number { return new B().y; }",
        )
        .err()
        .unwrap();
        assert!(ext.contains("not a class of this program"), "{ext}");
        let dec = lower::lower_source(
            "class A { @sealed x = 1; } export function f(): number { return new A().x; }",
        )
        .err()
        .unwrap();
        assert!(dec.contains("only `@property`"), "{dec}");
        let method = lower::lower_source(
            "class A { @property m(): number { return 1; } } export function f(): number { return new A().m(); }",
        )
        .err()
        .unwrap();
        assert!(method.contains("decorators"), "{method}");
        let value = lower::lower_source(
            "class A { m(): number { return 1; } } export function f(): number { return new A().nope; }",
        )
        .err()
        .unwrap();
        assert!(value.contains("no field `nope`"), "{value}");
        let ann = lower::lower_source(
            "class A { x = [1]; } export function f(): number { return 1 + new A().x.length; }",
        )
        .err()
        .unwrap();
        assert!(ann.contains("needs a type annotation"), "{ann}");
        let this = lower::lower_source("export function f(): number { return this.x; }")
            .err()
            .unwrap();
        assert!(this.contains("`this`"), "{this}");
    }

    // M9-TS-134
    #[test]
    fn inheritance_mistakes_are_errors_that_say_what_is_wrong() {
        let abs = lower::lower_source(
            "abstract class A { abstract m(): number; } export function f(): number { return new A().m(); }",
        )
        .err()
        .unwrap();
        assert!(abs.contains("abstract class A"), "{abs}");
        let order = lower::lower_source(
            "class B extends A { } class A { x = 1; } export function f(): number { return new B().x; }",
        )
        .err()
        .unwrap();
        assert!(order.contains("declare the base class"), "{order}");
        let retype = lower::lower_source(
            "class A { x = 1; } class B extends A { x = true; } export function f(): number { return 1; }",
        )
        .err()
        .unwrap();
        assert!(retype.contains("changes its type"), "{retype}");
        let inst = lower::lower_source(
            "export function f(x: number): boolean { return x instanceof Array; }",
        )
        .err()
        .unwrap();
        assert!(inst.contains("instanceof"), "{inst}");
    }

    // M9-TS-146
    #[test]
    fn calls_of_the_engine_api_are_checked_against_the_manifest() {
        let manifest = r#"{"namespace":"Eng","handles":["Thing"],"functions":[
            {"name":"add","symbol":"platypus_Eng_add","params":[{"name":"a","type":"f64"},{"name":"b","type":"f64"}],"returns":"f64"},
            {"name":"make","symbol":"platypus_Eng_make","params":[],"returns":"handle:Thing"}]}"#
            .to_string();
        let ok = lower::lower_source_with(
            "export function f(): number { const t: Thing = Eng.make(); return Eng.add(1, 2) + (t === Eng.make() ? 1 : 0); }",
            std::slice::from_ref(&manifest),
        )
        .unwrap();
        // `make` is used twice but declared once.
        let mut symbols: Vec<&str> = ok.externs.iter().map(|d| d.symbol.as_str()).collect();
        symbols.sort();
        assert_eq!(symbols, ["platypus_Eng_add", "platypus_Eng_make"]);
        let missing = lower::lower_source_with(
            "export function f(): number { return Eng.nope(1); }",
            std::slice::from_ref(&manifest),
        )
        .err()
        .unwrap();
        assert!(missing.contains("no function `nope`"), "{missing}");
        let count = lower::lower_source_with(
            "export function f(): number { return Eng.add(1); }",
            std::slice::from_ref(&manifest),
        )
        .err()
        .unwrap();
        assert!(count.contains("1 arguments, 2 expected"), "{count}");
        let bad =
            lower::lower_source_with("export function f(): number { return 1; }", &["{".into()])
                .err()
                .unwrap();
        assert!(bad.contains("manifest"), "{bad}");
    }

    // M9-TS-150
    #[test]
    fn value_classes_of_the_manifest_are_checked() {
        let manifest = r#"{"namespace":"V","handles":[],"values":[
            {"name":"Pair","fields":[{"name":"a","type":"f32"},{"name":"b","type":"f32"}]}],
            "functions":[
            {"name":"swap","symbol":"platypus_V_swap","params":[{"name":"p","type":"value:Pair"}],"returns":"value:Pair"}]}"#
            .to_string();
        let ok = lower::lower_source_with(
            "export function f(): number { const q = V.swap(new Pair(1, 2)); return q.a * 10 + q.b; }",
            std::slice::from_ref(&manifest),
        )
        .unwrap();
        // The parameter is flattened to its two fields, and the result comes back through a pointer.
        assert_eq!(ok.externs[0].params.len(), 2);
        assert_eq!(ok.externs[0].out.as_ref().map(|o| o.1), Some(2));
        let few = lower::lower_source_with(
            "export function f(): number { return new Pair(1).a; }",
            std::slice::from_ref(&manifest),
        )
        .err()
        .unwrap();
        assert!(few.contains("1 arguments, 2 expected"), "{few}");
        let wrong = lower::lower_source_with(
            "export function f(): number { return V.swap(3).a; }",
            std::slice::from_ref(&manifest),
        )
        .err()
        .unwrap();
        assert!(wrong.contains("type mismatch"), "{wrong}");
    }
}
