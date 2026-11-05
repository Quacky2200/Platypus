// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Platypus's components over the TypeScript runtime's host classes: the class table, the
//! Unity-order lifecycle (`NativeHost`), contacts and `@property` values. The runtime knows only
//! [`platypus_ts_rt::HostClass`]; this crate gives its hooks their meaning and provides the
//! `ts_rt_component_*` functions compiled code calls (pass [`symbols`] to the JIT).

mod host;
pub use host::*;

include!(concat!(env!("OUT_DIR"), "/symbols.rs"));

/// What the compiler is told about components, the way every plugin tells it about its API: a
/// manifest (`hostClasses`). Reading it registers the `ts_rt_component_*` functions with
/// `ts_api`, so a JIT finds them.
pub fn manifests() -> Vec<String> {
    ts_api::register_symbols(symbols());
    let hooks: Vec<String> = HOOK_LIST.iter().map(|h| format!("\"{h}\"")).collect();
    vec![format!(
        "{{\"hostClasses\":{{\"base\":\"Component\",\"object\":\"GameObject\",\"hooks\":[{}]}}}}",
        hooks.join(",")
    )]
}

/// Keeps the exported functions linked into an executable that reaches them only from compiled code.
pub fn link_anchor() -> usize {
    symbols().iter().map(|(_, a)| a).sum()
}
