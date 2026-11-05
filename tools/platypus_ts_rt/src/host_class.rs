// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Host classes: script classes the host makes instances of and calls hooks on (Platypus's
//! components). The runtime only carries the table; the host gives the hooks their meaning.

/// The constructor thunk of a host class: `(id, object)` -> the new instance. A class that takes no
/// object ignores the second argument.
pub type HostCreate = unsafe extern "C" fn(f64, u64) -> *mut u8;

/// One host class of a compiled program, from the JIT (`Game::host_classes`) or the AOT bindings'
/// `classes()`.
#[derive(Clone, Debug)]
pub struct HostClass {
    pub name: &'static str,
    /// The class struct id in the compiled program.
    pub sid: u64,
    /// The struct ids of the class and its ancestors (a search for a base finds subclasses).
    pub bases: &'static [u64],
    pub create: HostCreate,
    /// The hook thunks' addresses in the order of the host's hook list, `None` for a hook the
    /// class does not have. Their signatures are the host's: it declared the hooks.
    pub hooks: &'static [Option<usize>],
    /// The `@property` table: name, byte offset in the object, type name.
    pub props: &'static [(&'static str, usize, &'static str)],
}
