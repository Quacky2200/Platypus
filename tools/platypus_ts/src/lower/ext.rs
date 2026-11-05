// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Compiler extensions: a host's own conveniences, expanded at compile time in Rust (Platypus:
//! `GameObject.createPrimitive`, `instantiate` options, hierarchy searches). An extension is
//! installed once per process under a name ([`install`]) and used by a compile whose manifests
//! name it (`{"hostClasses": {"extension": "platypus", ..}}`), so programs of other hosts never
//! see it. Every hook runs while lowering; none adds work at run time.
//!
//! Hooks get the [`Lowerer`] and use its public lowering functions (`lower_expr`, `emit_extern`,
//! `new_local`, ...). A hook that returns `None` (or `false`) leaves the construct to the compiler.

use super::{Lowerer, R};
use crate::ir::{Expr, Stmt, Ty, VarId};
use crate::manifest::Manifest;
use oxc_ast::ast::{CallExpression, Expression};
use std::sync::RwLock;

/// A host's compile-time conveniences. Every method has a default that does nothing.
#[allow(unused_variables)] // the defaults ignore their arguments
pub trait Extension: Sync + Send {
    /// Rewrites the program text before it is parsed (after the built-in desugarings); `None`:
    /// unchanged.
    fn rewrite(&self, src: &str, manifest: &Manifest) -> R<Option<String>> {
        Ok(None)
    }

    /// Is `name` a helper the rewrite appends (a `__` name) that is compiled only once code calls
    /// it, so a program that never uses it calls none of the engine API it names?
    fn is_lazy_helper(&self, name: &str) -> bool {
        false
    }

    /// The methods that fill a host-class hook slot (`onCollisionEnter2D` fills
    /// `onCollisionEnter`). Default: the hook's own name.
    fn hook_methods(&self, hook: &str) -> Vec<String> {
        vec![hook.to_string()]
    }

    /// The exported thunk of a hook (`t.hook` of host class `t.class`, implemented by `t.method`),
    /// as TypeScript; an empty string exports none. `None`: the default thunk
    /// `host_hook_<class>_<hook>(c: <class>, dt: number) { c.<method>(); }`.
    fn hook_thunk(&self, t: &HookThunk<'_>, manifest: &Manifest) -> Option<String> {
        None
    }

    /// Methods of the host object class that take a class of the program and are not the
    /// `hostClasses` methods (`getComponentInChildren(Player)`), for [`Self::class_call`].
    fn class_methods(&self) -> &[&str] {
        &[]
    }

    /// `object.m(Class)` for a method of [`Self::class_methods`].
    fn class_call<'a>(
        &self,
        l: &mut Lowerer<'a>,
        object: &Expr,
        m: &str,
        class: &str,
    ) -> R<Option<Expr>> {
        Ok(None)
    }

    /// `Owner.prop` where `Owner` is an engine class or namespace name (`GameObject.roots`).
    fn static_member<'a>(&self, l: &mut Lowerer<'a>, owner: &str, prop: &str) -> R<Option<Expr>> {
        Ok(None)
    }

    /// `obj.prop` on a handle of `class` that has no such getter (`go.children`).
    fn member<'a>(
        &self,
        l: &mut Lowerer<'a>,
        class: &str,
        obj: Expr,
        prop: &str,
    ) -> R<Option<Expr>> {
        Ok(None)
    }

    /// `obj.prop` on a handle of `class` before the compiler's own facade lookup gives up
    /// (`camera.transform` through its object).
    fn getter<'a>(
        &self,
        l: &mut Lowerer<'a>,
        class: &str,
        obj: &Expr,
        prop: &str,
    ) -> R<Option<Expr>> {
        Ok(None)
    }

    /// `Owner.m(args)` (`GameObject.instantiate(..)`, `GameObject.createPrimitive(..)`).
    fn static_call<'a>(
        &self,
        l: &mut Lowerer<'a>,
        owner: &str,
        m: &str,
        c: &'a CallExpression<'a>,
    ) -> R<Option<Expr>> {
        Ok(None)
    }

    /// `recv.m(args)` on a handle of `class` (`prefab.instantiate({ .. })`).
    fn method_call<'a>(
        &self,
        l: &mut Lowerer<'a>,
        class: &str,
        recv: &Expr,
        m: &str,
        c: &'a CallExpression<'a>,
    ) -> R<Option<Expr>> {
        Ok(None)
    }

    /// One option of an options object for a new handle that has no plain setter (`parent`,
    /// `position` through the transform). `true`: handled, its statements pushed.
    fn option<'a>(&self, l: &mut Lowerer<'a>, o: OptionArg<'a, '_>) -> R<bool> {
        Ok(false)
    }
}

/// A hook thunk to write (`Extension::hook_thunk`): `export function <name>(..)`.
pub struct HookThunk<'s> {
    /// The thunk's exported name (`host_hook_Player_onUpdate`).
    pub name: &'s str,
    pub class: &'s str,
    /// The hook slot (`onCollisionEnter`).
    pub hook: &'s str,
    /// The method that fills it (`onCollisionEnter2D`).
    pub method: &'s str,
    /// The type the method's first parameter names (`Collider`), if any.
    pub param: Option<&'s str>,
}

/// One `key: value` of an options object (`new GameObject("a", { parent })`), for
/// [`Extension::option`].
pub struct OptionArg<'a, 's> {
    /// The class of the handle being made.
    pub class: &'s str,
    /// The local holding the new handle.
    pub tmp: VarId,
    pub key: &'s str,
    pub value: &'a Expression<'a>,
    /// Where the option's statements go.
    pub stmts: &'s mut Vec<Stmt>,
    /// The source location, for errors.
    pub at: &'s str,
}

type Registry = RwLock<Vec<(&'static str, &'static dyn Extension)>>;

static REGISTRY: Registry = RwLock::new(Vec::new());

/// Installs `ext` under `name` for this process (again with the same name: replaced).
pub fn install(name: &'static str, ext: &'static dyn Extension) {
    let mut r = REGISTRY.write().unwrap_or_else(|e| e.into_inner());
    r.retain(|(n, _)| *n != name);
    r.push((name, ext));
}

/// The extension installed under `name`.
pub fn find(name: &str) -> Option<&'static dyn Extension> {
    let r = REGISTRY.read().unwrap_or_else(|e| e.into_inner());
    r.iter().find(|(n, _)| *n == name).map(|(_, e)| *e)
}

/// What an extension reads from the lowerer besides its lowering functions.
impl Lowerer<'_> {
    /// The manifests the program is compiled against, merged.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Is there a top-level function of this name (one of the program's, or a helper a rewrite
    /// appended)?
    pub fn has_function(&self, name: &str) -> bool {
        self.fids.contains_key(name)
    }

    /// Is `name` a class the program declares?
    pub fn is_program_class(&self, name: &str) -> bool {
        self.class_decls.contains_key(name)
    }

    /// Field `name` of struct `sid`: its index and type.
    pub fn struct_field(&self, sid: usize, name: &str) -> Option<(usize, Ty)> {
        let s = &self.m.structs[sid];
        let i = s.field(name)?;
        Some((i, s.fields[i].ty.clone()))
    }

    /// Is a local binding of this name in scope (it hides an engine class of the same name)?
    pub fn is_local(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }
}
