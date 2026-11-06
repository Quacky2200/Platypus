# Copyright 2026 The Platypus Authors
# SPDX-License-Identifier: Apache-2.0

# Registers a bun-compared test program that calls the native engine API (`platypus_engine_api`):
#   python bench/add_engine_module.py <name> <prefix> <struct_base> <first_test_id> <engine ts files...>
# The engine files are modules of js/engine/src (for example `random.ts`): the generator imports
# them in bun and puts their exports on globalThis, so bench/<name>.ts runs there against the
# original TypeScript, and compiled against the Rust API. It adds the program to build.rs (with all
# engine manifests), a module to src/lib.rs, the generator bench/gen_<name>_expected.ts and a test
# per exported function in tests/engine_api/<name>.rs. Every exported function without parameters returns
# a string. Then: `bun bench/gen_<name>_expected.ts` and
# `cargo test -p platypus_ts_tests --test engine_api <name>`.
import os
import re
import sys

name, prefix, base, first = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
engine_files = sys.argv[5:]
here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
os.chdir(here)
src = open(f"bench/{name}.ts", encoding="utf8").read()
fns = re.findall(r"export function (\w+)\(", src)
mod = f"{name}_t"


def read(p):
    return open(p, encoding="utf8", newline="").read()


def write(p, s):
    open(p, "w", encoding="utf8", newline="").write(s)


b = read("build.rs")
nl = "\r\n" if "\r\n" in b else "\n"
entry = f'        ("{name}", "{prefix}_", {base}),'
if entry not in b:
    i = b.index("\n];", b.index("const PROGRAMS")) + 1
    b = b[:i] + entry + nl + b[i:]
    b = re.sub(r'("engine2" \| "sine_demo_full"[^=]*?)( =>)', lambda m: m.group(1) + f' | "{name}"' + m.group(2), b, count=1)
    write("build.rs", b)

l = read("src/lib.rs")
nl = "\r\n" if "\r\n" in l else "\n"
if f"pub mod {mod}" not in l:
    add = f"""/// `bench/{name}.ts`, prefix `{prefix}_`.
#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::all,
    unused_imports
)]
pub mod {mod} {{
    include!(concat!(env!("OUT_DIR"), "/{name}_bindings.rs"));
}}

""".replace("\n", nl)
    m = "/// Programs that probe the collector"
    l = l.replace(m, add + m, 1)
    write("src/lib.rs", l)

imports = "\n".join(
    f'import * as e{i} from "../../../js/engine/src/{f}";' for i, f in enumerate(engine_files)
)
assigns = "\n".join(f"Object.assign(globalThis, e{i});" for i in range(len(engine_files)))
gen = f"""// Runs every exported function of {name}.ts in bun against the original engine TypeScript
// ({", ".join(engine_files)}) and writes what they return.
//   bun bench/gen_{name}_expected.ts
{imports}

{assigns}
// What the engine sets up around the game (info, clock ticks), if the program needs any.
if (await Bun.file(import.meta.dir + "/{name}_prelude.ts").exists()) await import("./{name}_prelude.ts");
const m = await import("./{name}.ts");

const out: Record<string, unknown> = {{}};
for (const [name, fn] of Object.entries(m)) {{
  if (typeof fn !== "function") continue;
  (globalThis as any).__bench_reset?.(); // a program that keeps state starts each function fresh
  const v = (fn as () => unknown)();
  out[name] = typeof v === "string" ? {{ s: Array.from({{ length: v.length }}, (_, i) => v.charCodeAt(i)) }} : v;
}}
await Bun.write(import.meta.dir + "/{name}_expected.json", JSON.stringify(out));
console.log("wrote", Object.keys(out).length, "results");
"""
write(f"bench/gen_{name}_expected.ts", gen)


def snake(n):
    return re.sub(r"([A-Z])", lambda m: "_" + m.group(1).lower(), n).lower()


tests = f"""//! `bench/{name}.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_{name}_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-{first}..).
use web_ui_rt::{{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress}};
use platypus_ts_tests::{mod}::*;
use serde_json::Value;

/// The engine state a program starts from; change it for this program.
fn reset() {{}}

fn in_every_gc_mode(f: impl Fn()) {{
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe {{ ts_{prefix}_init() }};
    reset();
    f();
    ts_rt_gc_stress(true);
    reset();
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    reset();
    f();
    ts_rt_gc_enable(true);
}}

fn expected() -> Value {{
    serde_json::from_str(include_str!("../../bench/{name}_expected.json")).unwrap()
}}

fn check_str(name: &str, got: *mut u8) {{
    let want: Vec<u16> = expected()[name]["s"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u16)
        .collect();
    assert!(!got.is_null(), "{{name}} returned null");
    let got = unsafe {{ Str::from_raw(got as *const Str) }}.to_utf16();
    assert_eq!(
        String::from_utf16_lossy(&got),
        String::from_utf16_lossy(&want),
        "{{name}}"
    );
}}
"""
for k, f in enumerate(fns):
    tests += f"""
// M9-TS-{first + k}
#[test]
fn {snake(f)}_matches_the_original() {{
    in_every_gc_mode(|| unsafe {{
        check_str("{f}", ts_{prefix}_{f}());
    }});
}}
"""
write(f"tests/engine_api/{name}.rs", tests)

mains = read("tests/engine_api/main.rs")
if f"mod {name};" not in mains:
    # Keep the header (doc comment, the shared `cases` module) and sort only the test modules.
    ls = [x for x in mains.replace("\r\n", "\n").split("\n") if x.strip()]
    head = [x for x in ls if not (x.startswith("mod ") and x != "mod cases;")]
    mods = sorted({x for x in ls if x not in head} | {f"mod {name};"})
    # The blank line keeps rustfmt from sorting `mod cases;` (and its attributes) into the list.
    write("tests/engine_api/main.rs", "\n".join(head + [""] + mods) + "\n")
print(f"{name}: {len(fns)} tests, ids {first}..{first + len(fns) - 1}")
