# Copyright 2026 The Platypus Authors
# SPDX-License-Identifier: Apache-2.0

# Registers a new bun-compared test module for bench/<name>.ts:
#   python3 bench/add_module.py <name> <prefix> <struct_base> <first_test_id>
# It adds ("<name>", "<prefix>_", <base>) to build.rs, a `pub mod <name>_t` to src/lib.rs, the
# generator bench/gen_<name>_expected.ts and a test per exported function in tests/<name>.rs.
# Write bench/<name>.ts first: every exported function without parameters returns a string.
# Then: `bun bench/gen_<name>_expected.ts` and `cargo test -p platypus_ts_tests --test <name>`.
import os
import re
import sys

name, prefix, base, first = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
os.chdir(here)
src = open(f"bench/{name}.ts", encoding="utf8").read()
fns = re.findall(r"export function (\w+)\(", src)
mod = f"{name}_t"


def read(p):
    return open(p, encoding="utf8", newline="").read()


def write(p, s):
    open(p, "w", encoding="utf8", newline="").write(s)


# build.rs
b = read("build.rs")
nl = "\r\n" if "\r\n" in b else "\n"
entry = f'        ("{name}", "{prefix}_", {base}),'
if entry not in b:
    marker = "    ] {"
    i = b.index(marker)
    b = b[:i] + entry + nl + b[i:]
    write("build.rs", b)

# lib.rs
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

# the oracle
gen = f"""// Runs every exported function of {name}.ts in bun and writes what they return.
//   bun bench/gen_{name}_expected.ts
import * as m from "./{name}.ts";

const out: Record<string, unknown> = {{}};
for (const [name, fn] of Object.entries(m)) {{
  if (typeof fn !== "function") continue;
  const v = (fn as () => unknown)();
  out[name] = typeof v === "string" ? {{ s: Array.from({{ length: v.length }}, (_, i) => v.charCodeAt(i)) }} : v;
}}
await Bun.write(import.meta.dir + "/{name}_expected.json", JSON.stringify(out));
console.log("wrote", Object.keys(out).length, "results");
"""
write(f"bench/gen_{name}_expected.ts", gen)

# the tests
def snake(n):
    return re.sub(r"([A-Z])", lambda m: "_" + m.group(1).lower(), n).lower()


tests = f"""//! `bench/{name}.ts` against bun (`bench/gen_{name}_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-{first}..).
use web_ui_rt::{{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress}};
use platypus_ts_tests::{mod}::*;
use serde_json::Value;

fn in_every_gc_mode(f: impl Fn()) {{
    assert_ne!(link_anchor(), 0);
    unsafe {{ ts_{prefix}_init() }};
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}}

fn expected() -> Value {{
    serde_json::from_str(include_str!("../bench/{name}_expected.json")).unwrap()
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
fn {snake(f)}_matches_javascript() {{
    in_every_gc_mode(|| unsafe {{
        check_str("{f}", ts_{prefix}_{f}());
    }});
}}
"""
write(f"tests/{name}.rs", tests)
print(f"{name}: {len(fns)} tests, ids {first}..{first + len(fns) - 1}")
