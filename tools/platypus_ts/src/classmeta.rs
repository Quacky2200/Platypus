// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The component class table as data. The generated bindings' `classes()` is Rust text, so only a
//! program linked at build time can use it; a host that JIT-compiles a game reads this instead.

use crate::ir::*;
use crate::layout::prop_kind;

/// One registered component class. Symbols are exported function names: a JIT looks up their
/// addresses.
#[derive(Clone, Debug)]
pub struct ClassMeta {
    pub name: String,
    /// The class struct id: what `getComponent(Class)` passes.
    pub sid: u64,
    /// The struct ids of the class's ancestors.
    pub bases: Vec<u64>,
    pub create: String,
    /// The hook thunks in the order of the host's hook list (`hook_list`), `None` where the class
    /// has none.
    pub hooks: Vec<Option<String>>,
    /// The `@property` table: name, byte offset, type name.
    pub props: Vec<(String, usize, String)>,
}

/// The host's hook list: the lifecycle hooks, then the contact hooks.
pub fn hook_list() -> impl Iterator<Item = &'static str> {
    crate::lower::components::HOOKS
        .into_iter()
        .chain(crate::lower::components::CONTACT_HOOKS)
}

/// The table `layout::component_table` writes as Rust text, as data.
pub fn class_meta(m: &Module) -> Vec<ClassMeta> {
    let sym = |n: &str| format!("ts_{}{}", m.prefix, n);
    let mut out = Vec::new();
    for f in m.funcs.iter().filter(|f| f.exported) {
        let Some(class) = f.name.strip_prefix("platypus_new_") else {
            continue;
        };
        let find = |h: &str| {
            let name = format!("platypus_hook_{class}_{h}");
            m.funcs
                .iter()
                .any(|g| g.exported && g.name == name)
                .then(|| sym(&name))
        };
        let hooks = hook_list().map(find).collect();
        let sid = match f.ret.strip() {
            Ty::Obj(sid) => *sid,
            _ => 0,
        };
        let props = m
            .props
            .iter()
            .filter(|t| t.class == sid)
            .flat_map(|t| &t.entries)
            .map(|e| {
                (
                    e.name.clone(),
                    field_offset(e.index) as usize,
                    prop_kind(m, &e.ty),
                )
            })
            .collect();
        let bases = m
            .class_bases
            .iter()
            .find(|(s, _)| *s == sid)
            .map(|(_, b)| b.iter().map(|x| *x as u64).collect())
            .unwrap_or_default();
        out.push(ClassMeta {
            name: class.to_string(),
            sid: sid as u64,
            bases,
            create: sym(&f.name),
            hooks,
            props,
        });
    }
    out
}
