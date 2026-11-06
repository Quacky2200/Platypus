// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Platypus's components over the TypeScript runtime's host classes: the class table, the
//! Unity-order lifecycle (`NativeHost`), contacts and `@property` values. The runtime knows only
//! [`web_ui_rt::HostClass`]; this crate gives its hooks their meaning and provides the
//! `ts_rt_host_*` functions compiled code calls (pass [`symbols`] to the JIT).

pub mod compile;
mod host;
pub use host::*;

include!(concat!(env!("OUT_DIR"), "/symbols.rs"));

/// What the compiler is told about components, the way every plugin tells it about its API: a
/// manifest (`hostClasses`, and the `declarations` for `types/platypus.d.ts`). Reading it
/// registers the `ts_rt_host_*` functions with `ts_api`, so a JIT finds them, and installs the
/// compiler extension (`compile.rs`) the manifest names.
pub fn manifests() -> Vec<String> {
    ts_api::register_symbols(symbols());
    web_ui_ts::lower::ext::install(compile::EXTENSION, &compile::Platypus);
    let host = serde_json::json!({
        "declarations": compile::declarations(),
        "hostClasses": {
            "extension": compile::EXTENSION,
            "base": "Component",
            "object": "GameObject",
            "objectField": "gameObject",
            "prelude": PRELUDE,
            "methods": {
                "get": "getComponent",
                "getAll": "getComponents",
                "add": "addComponent",
                "remove": "removeComponent",
                "findAny": "findObjectOfType",
            },
            "hooks": HOOK_LIST,
        }
    });
    vec![host.to_string()]
}

/// The base class compiled in front of programs with components.
const PRELUDE: &str = "class Component {\n  id: number = 0;\n  gameObject: GameObject | null = null;\n  get transform(): Transform {\n    return this.gameObject!.transform;\n  }\n}\n";

/// Keeps the exported functions linked into an executable that reaches them only from compiled code.
pub fn link_anchor() -> usize {
    symbols().iter().map(|(_, a)| a).sum()
}
