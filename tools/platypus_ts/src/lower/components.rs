// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Components for the host: `class Player extends Component` and `register(Player)`.
//!
//! `Component` is a prelude class (an `id` and the object it is on). `register(Player)` is erased
//! from the program and replaced by exported functions the engine calls without any script engine:
//!
//! - `host_new_Player(id, gameObject)` makes the instance and fills the base fields;
//! - `host_hook_Player_onUpdate(this, dt)` for every hook the class has, its own or inherited.
//!
//! Every hook thunk takes `(this, dt)` so the host calls them all alike; `layout.rs` turns these
//! exports into a class table. Design: `docs/research/platypus-ts-engine-boundary.md` section 5.

use super::R;
use super::ext::{Extension, HookThunk};
use crate::manifest::{HostClasses, Manifest};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use std::collections::HashMap;

/// The prefix of a host class's exported constructor thunk: `host_new_Player(id, object)`.
pub const HOST_NEW: &str = "host_new_";
/// The prefix of a host class's exported hook thunks: `host_hook_Player_onUpdate(this, dt)`.
pub const HOST_HOOK: &str = "host_hook_";

/// What the program's code (not its strings or comments) does with components.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    /// A class extends `Component`.
    pub extends: bool,
    /// Something calls `register(..)`.
    pub registers: bool,
}

impl Wanted {
    /// Does the program need the `Component` prelude class?
    pub fn any(self) -> bool {
        self.extends || self.registers
    }
}

/// Finds `class X extends <base>` and `register(..)` calls in the code.
struct UseFinder<'b> {
    base: &'b str,
    found: Wanted,
}

impl<'a> Visit<'a> for UseFinder<'_> {
    fn visit_class(&mut self, c: &Class<'a>) {
        if let Some(Expression::Identifier(p)) = c.heritage.as_ref().map(|h| &h.expression)
            && p.name == self.base
        {
            self.found.extends = true;
        }
        walk::walk_class(self, c);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(f) = &call.callee
            && f.name == "register"
        {
            self.found.registers = true;
        }
        walk::walk_call_expression(self, call);
    }
}

/// Does the program use host classes (`base` is the base class)? Only code counts: a program
/// whose strings or comments show `extends Component` (the md-reader's bundled docs) gets no
/// prelude class, and so no tier from it.
pub fn wanted(src: &str, base: &str) -> Wanted {
    let extends = format!("extends {base}");
    if !(src.contains(&extends) || src.contains("register(")) {
        return Wanted::default();
    }
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, src, SourceType::ts()).parse();
    if !ret.diagnostics.is_empty() {
        // The real parse reports it; until then, assume the text means it.
        return Wanted {
            extends: src.contains(&extends),
            registers: src.contains("register("),
        };
    }
    let mut f = UseFinder {
        base,
        found: Wanted::default(),
    };
    f.visit_program(&ret.program);
    f.found
}

/// One `register(..)` call: where it is, the classes it names and its `spawn` option as source text.
struct RegisterCall {
    span: (u32, u32),
    names: Vec<String>,
    spawn: Option<String>,
    error: Option<String>,
}

/// Finds every `register(..)` call, wherever it is (a branch, a loop, a function, after an `await`).
struct RegisterFinder<'s> {
    src: &'s str,
    calls: Vec<RegisterCall>,
}

impl<'a> Visit<'a> for RegisterFinder<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(f) = &call.callee
            && f.name == "register"
        {
            let mut names = Vec::new();
            let mut spawn = None;
            let mut error = None;
            match call.arguments.first() {
                // `register(Player)`
                Some(Argument::Identifier(arg)) => names.push(arg.name.to_string()),
                // `register({ Player, Enemy })`: the engine's form, several classes at once.
                Some(Argument::ObjectExpression(o)) => {
                    for p in &o.properties {
                        match p {
                            ObjectPropertyKind::ObjectProperty(p) => match &p.key {
                                PropertyKey::StaticIdentifier(k) => names.push(k.name.to_string()),
                                _ => error = Some("register({ .. }) takes class names".to_string()),
                            },
                            _ => error = Some("register({ .. }) takes class names".to_string()),
                        }
                    }
                }
                _ => {
                    error = Some(
                        "register(..) takes a class name: a class is not a value in a compiled program, so `register(cls)` of a variable or an expression is not supported"
                            .to_string(),
                    )
                }
            }
            // `{ spawn: false }`: the value is read at run time, so it may be any expression.
            if let Some(Argument::ObjectExpression(o)) = call.arguments.get(1) {
                for p in &o.properties {
                    if let ObjectPropertyKind::ObjectProperty(p) = p
                        && let PropertyKey::StaticIdentifier(k) = &p.key
                        && k.name == "spawn"
                    {
                        let sp = p.value.span();
                        spawn = Some(self.src[sp.start as usize..sp.end as usize].to_string());
                    }
                }
            }
            let sp = call.span;
            self.calls.push(RegisterCall {
                span: (sp.start, sp.end),
                names,
                spawn,
                error,
            });
        }
        walk::walk_call_expression(self, call);
    }
}

/// The program with every `register(Class)` call turned into a run-time call that records the
/// class (`__registerClass`, so registration works in a branch, a loop or after an `await`) and
/// the thunks appended. `None` when there is nothing to register. `base` is the host-class base.
pub fn desugar(
    src: &str,
    host: &HostClasses,
    ext: Option<&dyn Extension>,
    manifest: &Manifest,
) -> R<Option<String>> {
    let base = host.base.as_str();
    if !src.contains("register(") {
        return Ok(None);
    }
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, src, SourceType::ts()).parse();
    if !ret.diagnostics.is_empty() {
        return Ok(None); // the real parse reports it
    }
    let mut classes: HashMap<String, (Option<String>, Vec<String>)> = HashMap::new();
    let mut registered: Vec<String> = Vec::new();
    let mut abstract_classes: Vec<String> = Vec::new();
    // (class, method) -> the type its first parameter names.
    let mut param_types: HashMap<(String, String), String> = HashMap::new();
    for st in &ret.program.body {
        let class = match st {
            Statement::ClassDeclaration(c) => Some(c),
            Statement::ExportDeclaration(x) => match &x.declaration {
                Declaration::ClassDeclaration(c) => Some(c),
                _ => None,
            },
            _ => None,
        };
        if let Some(c) = class
            && let Some(id) = &c.id
        {
            let parent = match c.heritage.as_ref().map(|h| &h.expression) {
                Some(Expression::Identifier(p)) => Some(p.name.to_string()),
                _ => None,
            };
            let mut methods = Vec::new();
            for el in &c.body.body {
                if let ClassElement::MethodDefinition(m) = el
                    && m.kind == MethodDefinitionKind::Method
                    && !m.r#static
                    && let PropertyKey::StaticIdentifier(k) = &m.key
                {
                    methods.push(k.name.to_string());
                    // The type its first parameter names (`other: Collider`).
                    let ann = m
                        .value
                        .params
                        .items
                        .first()
                        .and_then(|p| p.type_annotation.as_ref())
                        .and_then(|t| match &t.type_annotation {
                            TSType::TSTypeReference(r) => match &r.type_name {
                                TSTypeName::IdentifierReference(i) => Some(i.name.to_string()),
                                _ => None,
                            },
                            _ => None,
                        });
                    if let Some(a) = ann {
                        param_types.insert((id.name.to_string(), k.name.to_string()), a);
                    }
                }
            }
            if c.r#abstract {
                abstract_classes.push(id.name.to_string());
            }
            classes.insert(id.name.to_string(), (parent, methods));
        }
    }
    // A program that declares its own `register` keeps it.
    let own_register = ret.program.body.iter().any(|st| match st {
        Statement::FunctionDeclaration(f) => f.id.as_ref().is_some_and(|i| i.name == "register"),
        Statement::VariableDeclaration(d) => d
            .declarations
            .iter()
            .any(|v| matches!(&v.id, BindingPattern::BindingIdentifier(i) if i.name == "register")),
        _ => false,
    });
    if own_register {
        return Ok(None);
    }
    let mut finder = RegisterFinder {
        src,
        calls: Vec::new(),
    };
    finder.visit_program(&ret.program);
    for c in &finder.calls {
        if let Some(e) = &c.error {
            return Err(e.clone());
        }
        for n in &c.names {
            if !registered.contains(n) {
                registered.push(n.clone());
            }
        }
    }
    if registered.is_empty() {
        return Ok(None);
    }
    let mut out = src.to_string();
    // Calls go back to front so the offsets of the earlier ones hold.
    let mut calls = finder.calls;
    calls.sort_by_key(|c| std::cmp::Reverse(c.span.0));
    for c in &calls {
        for n in &c.names {
            if !classes.contains_key(n) {
                return Err(format!(
                    "register({n}): `{n}` is not a class of this program"
                ));
            }
        }
        let spawn = match &c.spawn {
            Some(e) => format!("(({e}) ? 1 : 0)"),
            None => "1".to_string(),
        };
        // The call may span lines: keep their number.
        let lines = src[c.span.0 as usize..c.span.1 as usize]
            .matches("\n")
            .count();
        let text = format!(
            "__registerClass(\"{}\", {spawn}){}",
            c.names.join(","),
            "\n".repeat(lines)
        );
        out.replace_range(c.span.0 as usize..c.span.1 as usize, &text);
    }
    let mut tail = String::from("\n");
    // Every other component class is still creatable (`go.addComponent(Turntable)`): it gets
    // the same thunks; whether the host starts a class is decided by the `register(..)` calls
    // that run.
    let mut unregistered: Vec<String> = Vec::new();
    let mut names: Vec<&String> = classes.keys().collect();
    names.sort();
    for n in names {
        if abstract_classes.contains(n) || registered.contains(n) {
            continue;
        }
        let mut cur = classes[n].0.clone();
        let mut guard = 0;
        while let Some(p) = cur {
            guard += 1;
            if p == base {
                unregistered.push(n.clone());
                break;
            }
            if guard > 64 {
                break;
            }
            cur = classes.get(&p).and_then(|c| c.0.clone());
        }
    }
    let todo: Vec<&String> = registered.iter().chain(unregistered.iter()).collect();
    // The object instances are made on, and the base field that holds it.
    let object = host.object.as_deref().zip(host.object_field.as_deref());
    for name in todo {
        let (go, set_go) = match object {
            Some((class, field)) => (format!(", go: {class} | null"), format!(" c.{field} = go;")),
            None => (String::new(), String::new()),
        };
        tail += &format!(
            "export function {HOST_NEW}{name}(id: number{go}): {name} {{ const c = new {name}(); c.id = id;{set_go} return c; }}\n"
        );
        // Each hook slot the class or an ancestor fills: the method and its first parameter's type.
        let mut filled: Vec<(&str, String, Option<String>)> = Vec::new();
        let mut chain = Some(name.clone());
        let mut guard = 0;
        while let Some(cn) = chain {
            guard += 1;
            if guard > 64 {
                return Err(format!("class {name}: inheritance cycle"));
            }
            let Some((parent, methods)) = classes.get(&cn) else {
                break;
            };
            for h in &host.hooks {
                if filled.iter().any(|(f, _, _)| f == h) {
                    continue;
                }
                let names = ext.map_or_else(|| vec![h.clone()], |e| e.hook_methods(h));
                if let Some(m) = names.into_iter().find(|n| methods.contains(n)) {
                    let ty = param_types.get(&(cn.clone(), m.clone())).cloned();
                    filled.push((h, m, ty));
                }
            }
            chain = parent.clone();
        }
        for h in &host.hooks {
            let Some((_, method, param)) = filled.iter().find(|(f, _, _)| f == h) else {
                continue;
            };
            let thunk = format!("{HOST_HOOK}{name}_{h}");
            let t = HookThunk {
                name: &thunk,
                class: name,
                hook: h,
                method,
                param: param.as_deref(),
            };
            tail += &ext
                .and_then(|e| e.hook_thunk(&t, manifest))
                .unwrap_or_else(|| {
                    format!(
                        "export function {thunk}(c: {name}, dt: number): void {{ c.{method}(); }}\n"
                    )
                });
        }
    }
    out.push_str(&tail);
    Ok(Some(out))
}
