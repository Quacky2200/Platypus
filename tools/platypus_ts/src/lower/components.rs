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
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use std::collections::HashMap;

/// The hooks the engine calls, in the order of the host's table.
pub const HOOKS: [&str; 9] = [
    "onAwake",
    "onStart",
    "onUpdate",
    "onLateUpdate",
    "onFixedUpdate",
    "onPreRender",
    "onDestroy",
    "onApplicationFocus",
    "onApplicationPause",
];

/// What a frame hook's thunk passes it (the host gives every thunk `dt`) and the parameter it
/// declares: the frame hooks get the frame length, the focus and pause hooks a flag, the rest
/// nothing.
pub fn hook_param(hook: &str) -> Option<(&'static str, &'static str)> {
    match hook {
        "onUpdate" | "onLateUpdate" | "onFixedUpdate" => Some(("dt", "dt: number")),
        "onApplicationFocus" => Some(("dt !== 0", "hasFocus: boolean")),
        "onApplicationPause" => Some(("dt !== 0", "paused: boolean")),
        _ => None,
    }
}

/// The contact hooks, called by name when the physics step reports a contact. Each takes a
/// `Collision` (the other side; a record of `platypus_physics_api`). Their thunks take the pieces, the
/// host passes plain numbers.
pub const CONTACT_HOOKS: [&str; 6] = [
    "onCollisionEnter",
    "onCollisionExit",
    "onCollisionStay",
    "onTriggerEnter",
    "onTriggerExit",
    "onTriggerStay",
];

/// The prefix of a host class's exported constructor thunk: `host_new_Player(id, object)`.
pub const HOST_NEW: &str = "host_new_";
/// The prefix of a host class's exported hook thunks: `host_hook_Player_onUpdate(this, dt)`.
pub const HOST_HOOK: &str = "host_hook_";

/// The searches up and down the hierarchy and the prefix of the TypeScript helper that does each
/// (`children_helpers`): Unity's `GetComponent[s]In{Children,Parent}`.
const HIERARCHY_SEARCHES: [(&str, &str); 4] = [
    ("getComponentInChildren", "__cic_"),
    ("getComponentsInChildren", "__cac_"),
    ("getComponentInParent", "__cip_"),
    ("getComponentsInParent", "__cap_"),
];

/// Is `method` one of the hierarchy searches?
pub fn is_hierarchy_search(method: &str) -> bool {
    HIERARCHY_SEARCHES.iter().any(|(m, _)| *m == method)
}

/// The name of the helper for `method(Class)`.
pub fn hierarchy_helper(method: &str, class: &str) -> Option<String> {
    let (_, prefix) = HIERARCHY_SEARCHES.iter().find(|(m, _)| *m == method)?;
    Some(format!("{prefix}{class}"))
}

/// Helper functions for the hierarchy searches (`getComponentInChildren(Class)` and the others):
/// one per method and class named, appended to the program (a class value is not a runtime value
/// here, so the walk is written per class).
pub fn children_helpers(src: &str, has_game_object: bool) -> Option<String> {
    if !has_game_object {
        return None;
    }
    let mut out = String::new();
    for (method, prefix) in HIERARCHY_SEARCHES {
        let call = format!("{method}(");
        let mut names: Vec<String> = Vec::new();
        let mut rest = src;
        while let Some(i) = rest.find(&call) {
            rest = &rest[i + call.len()..];
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
        for n in names {
            out += &match prefix {
                "__cic_" => format!(
                    "function __cic_{n}(go: GameObject): {n} | undefined {{\n  const own = go.getComponent({n});\n  if (own) return own;\n  for (let i = 0; i < go.childCount; i++) {{\n    const found = __cic_{n}(go.childAt(i));\n    if (found) return found;\n  }}\n  return undefined;\n}}\n"
                ),
                "__cac_" => format!(
                    "function __cac_{n}(go: GameObject): {n}[] {{\n  const out: {n}[] = [];\n  const own = go.getComponents({n});\n  for (let i = 0; i < own.length; i++) out.push(own[i]);\n  for (let i = 0; i < go.childCount; i++) {{\n    const sub = __cac_{n}(go.childAt(i));\n    for (let j = 0; j < sub.length; j++) out.push(sub[j]);\n  }}\n  return out;\n}}\n"
                ),
                "__cip_" => format!(
                    "function __cip_{n}(go: GameObject): {n} | undefined {{\n  let at: GameObject | null = go;\n  while (at) {{\n    const own = at.getComponent({n});\n    if (own) return own;\n    at = at.parent;\n  }}\n  return undefined;\n}}\n"
                ),
                _ => format!(
                    "function __cap_{n}(go: GameObject): {n}[] {{\n  const out: {n}[] = [];\n  let at: GameObject | null = go;\n  while (at) {{\n    const own = at.getComponents({n});\n    for (let i = 0; i < own.length; i++) out.push(own[i]);\n    at = at.parent;\n  }}\n  return out;\n}}\n"
                ),
            };
        }
    }
    (!out.is_empty()).then(|| format!("\n{out}"))
}

/// Is `name` one of the hierarchy helpers below? They are appended when the text names them (a
/// React `node.children` is enough), and lowered only once code calls them (`call_helper`), so a
/// program that never walks the scene doesn't call the `GameObject` API (and gets no tier from it).
pub fn is_scene_helper(name: &str) -> bool {
    name.starts_with("__go_") || HIERARCHY_SEARCHES.iter().any(|(_, p)| name.starts_with(p))
}

/// Helpers for the `GameObject` properties that are arrays (`children`, `GameObject.roots`), written
/// in TypeScript over the engine's count/index functions and appended when the program names them.
pub fn tree_helpers(src: &str, has_game_object: bool) -> Option<String> {
    if !has_game_object {
        return None;
    }
    let mut out = String::new();
    if src.contains(".children") {
        out += "\nfunction __go_children(go: GameObject): GameObject[] {\n  const out: GameObject[] = [];\n  for (let i = 0; i < go.childCount; i++) out.push(go.childAt(i));\n  return out;\n}\n";
    }
    if src.contains("GameObject.roots") {
        out += "\nfunction __go_roots(): GameObject[] {\n  const out: GameObject[] = [];\n  const n = GameObject.rootCount();\n  for (let i = 0; i < n; i++) out.push(GameObject.rootAt(i));\n  return out;\n}\n";
    }
    (!out.is_empty()).then_some(out)
}

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
    base: &str,
    has_game_object: bool,
    has_contacts: bool,
) -> R<Option<String>> {
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
    for name in todo {
        let go = if has_game_object {
            ", go: GameObject | null"
        } else {
            ""
        };
        let set_go = if has_game_object {
            " c.gameObject = go;"
        } else {
            ""
        };
        tail += &format!(
            "export function host_new_{name}(id: number{go}): {name} {{ const c = new {name}(); c.id = id;{set_go} return c; }}\n"
        );
        // The hooks the class or an ancestor defines.
        let mut chain = Some(name.clone());
        let mut have: Vec<&str> = Vec::new();
        let mut contact_impl: HashMap<&str, (String, Option<String>)> = HashMap::new();
        let mut guard = 0;
        while let Some(cn) = chain {
            guard += 1;
            if guard > 64 {
                return Err(format!("class {name}: inheritance cycle"));
            }
            match classes.get(&cn) {
                Some((parent, methods)) => {
                    for h in HOOKS {
                        if methods.iter().any(|m| m == h) && !have.contains(&h) {
                            have.push(h);
                        }
                    }
                    // `onCollisionEnter2D` is the same slot as `onCollisionEnter`.
                    for h in CONTACT_HOOKS {
                        let two_d = format!("{h}2D");
                        let found = [h.to_string(), two_d]
                            .into_iter()
                            .find(|n| methods.iter().any(|m| m == n));
                        if let Some(n) = found
                            && !have.contains(&h)
                        {
                            have.push(h);
                            let ty = param_types.get(&(cn.clone(), n.clone())).cloned();
                            contact_impl.insert(h, (n, ty));
                        }
                    }
                    chain = parent.clone();
                }
                None => break,
            }
        }
        if has_game_object && has_contacts {
            for h in CONTACT_HOOKS {
                if have.contains(&h) {
                    let (method, ty) = contact_impl[h].clone();
                    tail += &contact_thunk(name, h, &method, ty.as_deref());
                }
            }
        }
        for h in HOOKS {
            if have.contains(&h) {
                let call = hook_param(h).map_or("", |(arg, _)| arg);
                tail += &format!(
                    "export function host_hook_{name}_{h}(c: {name}, dt: number): void {{ c.{h}({call}); }}\n"
                );
            }
        }
    }
    out.push_str(&tail);
    Ok(Some(out))
}

/// The thunk of a contact hook: the host passes the `Collision` handle and the other side's
/// `Collider` handle (`platypus_physics_api::contacts`), and the thunk hands the hook the one it asks
/// for. As in Unity, a trigger hook gets the other `Collider` (or `Collider2D`) and a collision hook
/// a `Collision` (or `Collision2D`) unless its parameter names the other kind.
fn contact_thunk(name: &str, h: &str, method: &str, param: Option<&str>) -> String {
    let two_d = method.ends_with("2D");
    let wants = match param {
        Some(t @ ("Collider" | "Collider2D" | "Collision" | "Collision2D")) => t,
        _ if h.starts_with("onTrigger") => {
            if two_d {
                "Collider2D"
            } else {
                "Collider"
            }
        }
        _ if two_d => "Collision2D",
        _ => "Collision",
    };
    let collision_ty = match wants {
        "Collision" | "Collision2D" => wants,
        _ if two_d => "Collision2D",
        _ => "Collision",
    };
    let collider_ty = match wants {
        "Collider" | "Collider2D" => wants,
        _ if two_d => "Collider2D",
        _ => "Collider",
    };
    let arg = if wants == collision_ty {
        "collision"
    } else {
        "collider"
    };
    format!(
        "export function host_hook_{name}_{h}(c: {name}, collision: {collision_ty}, collider: {collider_ty}): void {{ c.{method}({arg}); }}\n"
    )
}
