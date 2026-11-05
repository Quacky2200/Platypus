// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `types/platypus.d.ts` from the engine manifests: what `tsc` needs to type-check a game when there is no
//! `node_modules` (no `@platypus/engine`, `react` or `@types/react` packages).
//!
//! The manifests (the JSON `ts_api` generates, `docs/platypus-api.md`) list namespaces, handle and value
//! classes and their functions. This turns them into declarations, once as the module `@platypus/engine`
//! (so the examples' `import { Color } from "@platypus/engine"` type-checks as written). The doc comments
//! are not in the manifests, so the declarations have none. The ambient declarations (React-style
//! names, `JSX.IntrinsicElements`, `@platypus/react`, `react`) are written by `ui_dts` from the UI tables.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The TypeScript type of a manifest type. With `like`, a value class (and an array of them) in a
/// parameter is the structural `<Value>Like` (`Vector2Like`: a plain `{ x, y }` object does).
fn ts_type_like(t: &str, like: bool) -> String {
    if like {
        if let Some(n) = t.strip_prefix("value:") {
            return format!("{n}Like");
        }
        if let Some(n) = t.strip_prefix("values:") {
            return format!("readonly {n}Like[]");
        }
        if let Some(n) = t.strip_prefix("opt:value:") {
            return format!("{n}Like | null");
        }
    }
    ts_type(t)
}

/// The TypeScript type of a parameter: an array the engine only reads is `readonly`.
fn param_ts_type(t: &str, like: bool) -> String {
    let ty = ts_type_like(t, like);
    if (t.starts_with("values:")
        || t.starts_with("handles:")
        || t.starts_with("records:")
        || t == "f64s"
        || t == "strings")
        && !ty.starts_with("readonly ")
    {
        format!("readonly {ty}")
    } else {
        ty
    }
}

/// The TypeScript type of a manifest type (for the UI declarations).
pub(crate) fn manifest_ts_type(t: &str) -> String {
    ts_type(t)
}

fn ts_type(t: &str) -> String {
    match t {
        "void" => "void".into(),
        "bool" => "boolean".into(),
        "f64" | "f32" | "i32" | "u32" => "number".into(),
        "string" => "string".into(),
        "f64s" => "number[]".into(),
        "strings" => "string[]".into(),
        "list:string" => "string[]".into(),
        "list:f64" => "number[]".into(),
        "list:pair" => "[number, number][]".into(),
        "dict:f64" => "Record<string, number>".into(),
        "ptr" => "unknown".into(),
        other => {
            if let Some(inner) = other.strip_prefix("opt:") {
                format!("{} | null", ts_type(inner))
            } else if let Some(n) = other.strip_prefix("like:") {
                format!("{n}Like")
            } else if let Some(n) = other.strip_prefix("list:enum:") {
                format!("{n}[]")
            } else if let Some(n) = other
                .strip_prefix("handle:")
                .or_else(|| other.strip_prefix("value:"))
                .or_else(|| other.strip_prefix("enum:"))
                .or_else(|| other.strip_prefix("record:"))
            {
                n.to_string()
            } else if let Some(n) = other
                .strip_prefix("list:value:")
                .or_else(|| other.strip_prefix("list:record:"))
                .or_else(|| other.strip_prefix("list:handle:"))
                .or_else(|| other.strip_prefix("records:"))
            {
                format!("{n}[]")
            } else if let Some(n) = other
                .strip_prefix("values:")
                .or_else(|| other.strip_prefix("handles:"))
            {
                format!("{n}[]")
            } else if let Some(inner) = other.strip_prefix("fn:") {
                format!("() => {}", ts_type(inner))
            } else if let Some(rest) = other.strip_prefix("fn(")
                && let Some((params, ret)) = rest.split_once("):")
            {
                let ps: Vec<String> = params
                    .split(',')
                    .filter(|p| !p.is_empty())
                    .enumerate()
                    .map(|(i, p)| format!("a{i}: {}", ts_type(p)))
                    .collect();
                format!("({}) => {}", ps.join(", "), ts_type(ret))
            } else {
                "unknown".into()
            }
        }
    }
}

struct Func {
    kind: String,
    name: String,
    params: Vec<(String, String, bool)>,
    ret: String,
    doc: String,
    /// The name of the rest parameter (the last one), if any.
    rest: Option<String>,
    /// `#[options(after = p)]`: the index of `p` among the parameters without the receiver.
    options_after: Option<usize>,
    /// `#[options(name = "X")]`: the declared type of the options object.
    options_name: Option<String>,
    /// `#[options(.., only)]`: the positional form is not declared.
    options_only: bool,
    /// `#[options(.., object)]`: the declared type of the one object parameter and its keys.
    object_keys: Option<(String, OptionKeys)>,
    /// `#[global]`: also a global function.
    global: bool,
    /// `#[constructor]`: the positional constructor of the class.
    ctor: bool,
    /// Per parameter (with the receiver): its doc, and whether it is a handle.
    param_docs: Vec<String>,
    param_handle: Vec<bool>,
}

fn funcs(v: &Value, like: bool) -> Vec<Func> {
    let Some(arr) = v.as_array() else {
        return vec![];
    };
    arr.iter()
        .map(|f| {
            let mut func = func_of(f, like);
            // `#[options(.., object)]`: one object parameter whose keys are the parameters.
            if f["optionsObject"].as_bool() == Some(true)
                && let Some(name) = f["optionsName"].as_str()
            {
                let skip = usize::from(func.kind == "method");
                let keys: OptionKeys = func
                    .params
                    .iter()
                    .enumerate()
                    .skip(skip)
                    .map(|(at, (n, t, o))| {
                        (
                            n.clone(),
                            t.clone(),
                            func.param_docs.get(at).cloned().unwrap_or_default(),
                            !o,
                        )
                    })
                    .collect();
                func.params.truncate(skip);
                func.params
                    .push(("data".to_string(), name.to_string(), false));
                func.options_name = None;
                func.object_keys = Some((name.to_string(), keys));
            }
            // `#[promise]`: the resolve and reject callbacks are the engine's; scripts get a promise.
            if f["promise"].as_bool() == Some(true) && func.params.len() >= 2 {
                let resolve = f["params"][func.params.len() - 2]["type"]
                    .as_str()
                    .unwrap_or("");
                let value = resolve
                    .strip_prefix("fn(")
                    .and_then(|r| r.split_once("):"))
                    .and_then(|(ps, _)| ps.split(',').find(|p| !p.is_empty()))
                    .map_or("void".to_string(), ts_type);
                func.ret = format!("Promise<{value}>");
                func.params.truncate(func.params.len() - 2);
            }
            // `#[subscription(off = ..)]`: scripts get a function that stops the listener.
            if let Some(sub) = f["subscription"].as_object() {
                func.ret = "() => void".to_string();
                // The callback is the last parameter.
                let ts = sub
                    .get("ts")
                    .and_then(|t| t.as_str())
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .or_else(|| {
                        // `json = "GamepadChange"`: the callback gets that record.
                        sub.get("json")
                            .and_then(|t| t.as_str())
                            .filter(|t| !t.is_empty())
                            .map(|r| format!("(e: {r}) => void"))
                    });
                if let Some(ts) = ts
                    && let Some(p) = func.params.last_mut()
                {
                    p.1 = ts;
                }
            }
            // `#[ts_type = ".."]`: what a property reads as, a string-literal union where the Rust
            // type is a `String`. Writing it still takes a `string` (an unknown name is ignored).
            if let Some(t) = f["tsType"].as_str()
                && func.kind == "get"
            {
                func.ret = t.to_string();
            }
            // `#[nullable]`: a handle result, or the value a setter takes, may be `null`
            // (`#[nullable(undefined)]`: `undefined`).
            if f["nullable"].as_bool() == Some(true) {
                let none = if f["undefined"].as_bool() == Some(true) {
                    "undefined"
                } else {
                    "null"
                };
                func.ret = format!("{} | {none}", func.ret);
                if func.kind == "set"
                    && let Some(last) = func.params.last_mut()
                {
                    last.1 = format!("{} | {none}", last.1);
                }
            }
            func
        })
        .collect()
}

fn func_of(f: &Value, like: bool) -> Func {
    let like = like || f["like"].as_bool() == Some(true);
    Func {
        kind: f["kind"].as_str().unwrap_or("static").to_string(),
        name: f["name"].as_str().unwrap_or("").to_string(),
        params: f["params"]
            .as_array()
            .map(|ps| {
                ps.iter()
                    .map(|p| {
                        (
                            p["name"].as_str().unwrap_or("p").to_string(),
                            p["ts"].as_str().filter(|t| !t.is_empty()).map_or_else(
                                || param_ts_type(p["type"].as_str().unwrap_or("void"), like),
                                str::to_string,
                            ),
                            p["optional"].as_bool().unwrap_or(false),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        ret: ts_type(f["returns"].as_str().unwrap_or("void")),
        doc: f["doc"].as_str().unwrap_or("").to_string(),
        rest: f["params"].as_array().and_then(|ps| {
            ps.iter()
                .find(|p| p["rest"].as_bool() == Some(true))
                .and_then(|p| p["name"].as_str().map(str::to_string))
        }),
        options_after: f["optionsAfter"].as_u64().map(|n| n as usize),
        options_name: f["optionsName"].as_str().map(str::to_string),
        options_only: f["optionsOnly"].as_bool() == Some(true),
        object_keys: None,
        global: f["global"].as_bool() == Some(true),
        ctor: f["constructor"].as_bool() == Some(true),
        param_docs: f["params"]
            .as_array()
            .map(|ps| {
                ps.iter()
                    .map(|p| p["doc"].as_str().unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default(),
        param_handle: f["params"]
            .as_array()
            .map(|ps| {
                ps.iter()
                    .map(|p| p["type"].as_str().unwrap_or("").starts_with("handle:"))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn params_text(f: &Func, skip_this: bool) -> String {
    // A required parameter cannot follow an optional one: the rest after the first become optional.
    let mut seen_optional = false;
    f.params
        .iter()
        .skip(usize::from(skip_this))
        .map(|(n, t, o)| {
            seen_optional |= *o;
            if f.rest.as_deref() == Some(n.as_str()) {
                // `...points: Vector2Like[]` (the array type without `readonly`).
                let t = t.strip_prefix("readonly ").unwrap_or(t);
                return format!("...{}: {t}", safe_ident(n));
            }
            format!(
                "{}{}: {t}",
                safe_ident(n),
                if seen_optional { "?" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The JSDoc comment of a declaration at an indent (nothing for an empty doc).
fn jsdoc(doc: &str, indent: &str) -> String {
    if doc.is_empty() {
        return String::new();
    }
    let mut s = format!("{indent}/**{}", NL);
    for line in doc.lines() {
        // A comment cannot contain its own end.
        s += &format!("{indent} * {}{}", line.trim().replace("*/", "* /"), NL);
    }
    s += &format!("{indent} */{}", NL);
    s
}

const NL: &str = "\n";

/// The "options object" form of a call: the compiler fills the parameters after the leading
/// required ones (numbers, strings, objects; flags may be left out) from a trailing `{ key: value }`
/// named like them (`audio.load(path, { stream: true })`). `None` when every parameter is leading.
fn options_params(f: &Func, skip_this: bool) -> Option<String> {
    let ps: Vec<_> = f.params.iter().skip(usize::from(skip_this)).collect();
    let k = ps
        .iter()
        .take_while(|(_, t, o)| !o && t != "boolean")
        .count();
    if k == ps.len() {
        return None;
    }
    let mut parts: Vec<String> = ps[..k]
        .iter()
        .map(|(n, t, _)| format!("{}: {t}", safe_ident(n)))
        .collect();
    if let Some(name) = &f.options_name {
        parts.push(format!("options?: {name}"));
        return Some(parts.join(", "));
    }
    let keys: Vec<String> = ps[k..]
        .iter()
        .map(|(n, t, _)| format!("{n}?: {t}"))
        .collect();
    parts.push(format!("options?: {{ {} }}", keys.join("; ")));
    Some(parts.join(", "))
}

/// The keys of an options object: (key, type, doc) each.
type OptionKeys = Vec<(String, String, String, bool)>;

/// The keys of a named options object (`#[options(name = "X")]`): the parameters after the leading
/// required ones, as (key, type, doc). A handle key also takes `null` (no object).
fn option_keys(f: &Func, skip_this: bool) -> OptionKeys {
    let skip = usize::from(skip_this);
    let ps: Vec<_> = f.params.iter().skip(skip).collect();
    let k = ps
        .iter()
        .take_while(|(_, t, o)| !o && t != "boolean")
        .count();
    ps[k..]
        .iter()
        .enumerate()
        .map(|(i, (n, t, _))| {
            let at = skip + k + i;
            let ty = if f.param_handle.get(at) == Some(&true) {
                format!("{t} | null")
            } else {
                t.clone()
            };
            (
                n.clone(),
                ty,
                f.param_docs.get(at).cloned().unwrap_or_default(),
                false,
            )
        })
        .collect()
}

const RESERVED: &[&str] = &[
    "default", "function", "class", "new", "delete", "in", "of", "var", "let", "const",
];

fn is_reserved(n: &str) -> bool {
    RESERVED.contains(&n)
}

fn safe_ident(n: &str) -> String {
    if is_reserved(n) {
        format!("{n}_")
    } else {
        n.to_string()
    }
}

fn ident_at(s: &str) -> &str {
    let end = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(s.len());
    &s[..end]
}

/// The name a member line declares (`static foo(..)`, `readonly x: T`, `function f(..)`, `const c: T`).
fn member_name(line: &str) -> &str {
    let mut s = line.trim_start();
    loop {
        let mut stripped = false;
        for kw in [
            "static ",
            "readonly ",
            "function ",
            "const ",
            "let ",
            "var ",
            "get ",
            "set ",
            "export ",
            "declare ",
        ] {
            if let Some(r) = s.strip_prefix(kw) {
                s = r;
                stripped = true;
            }
        }
        if !stripped {
            break;
        }
    }
    ident_at(s)
}

/// A record: its doc and its fields (name, type, doc).
type RecordDecl = (String, Vec<(String, String, String)>);

#[derive(Default)]
struct Decls {
    handles: BTreeSet<String>,
    /// Value class -> its field names.
    values: BTreeMap<String, Vec<String>>,
    /// Class name -> member lines (deduplicated, in order).
    classes: BTreeMap<String, Vec<String>>,
    /// Namespace -> member lines.
    namespaces: BTreeMap<String, Vec<String>>,
    /// Global functions (`#[global]`): (declaration line without `declare`, doc).
    globals: BTreeMap<String, (String, String)>,
    /// Class -> the parameters of its static `create` (`new Class(..)` calls it).
    creates: BTreeMap<String, Vec<(String, String, bool)>>,
    /// Class -> the parameters of its positional constructor (`#[constructor]`).
    ctor_params: BTreeMap<String, Vec<(String, String, bool)>>,
    /// Class -> the index of the `create` parameter after which an options object may follow
    /// (`#[options(after = p)]`): the constructor is `(positional..., options?)` and nothing else.
    creates_after: BTreeMap<String, usize>,
    /// Class -> its writable properties (the keys of `new Class({ .. })`).
    props: BTreeMap<String, Vec<(String, String)>>,
    /// (class or namespace, member line) -> its doc comment.
    docs: BTreeMap<(String, String), String>,
    /// Doc comments of classes and namespaces.
    owner_docs: BTreeMap<String, String>,
    /// Enum -> (doc, names).
    enums: BTreeMap<String, (String, Vec<String>)>,
    /// Record -> (doc, [(field, type, doc)]).
    records: BTreeMap<String, RecordDecl>,
    /// Alias -> the class or enum it names.
    aliases: BTreeMap<String, String>,
    /// Named options objects (`#[options(name = "X")]`) -> (key, type, doc) of each key, merged
    /// over the functions that name them.
    option_types: BTreeMap<String, OptionKeys>,
}

fn push_unique(list: &mut Vec<String>, line: String) {
    if !list.contains(&line) {
        list.push(line);
    }
}

impl Decls {
    fn add(&mut self, json: &str) -> Result<(), String> {
        let v: Value = serde_json::from_str(json).map_err(|e| format!("manifest json: {e}"))?;
        let like = v["like"].as_bool().unwrap_or(false);
        // Named options objects found in this manifest: (type name, its keys).
        let mut named: Vec<(String, OptionKeys)> = Vec::new();
        for h in v["handles"].as_array().into_iter().flatten() {
            if let Some(h) = h.as_str() {
                self.handles.insert(h.to_string());
            }
        }
        for val in v["values"].as_array().into_iter().flatten() {
            let name = val["name"].as_str().unwrap_or("").to_string();
            let fields = val["fields"]
                .as_array()
                .map(|fs| {
                    fs.iter()
                        .map(|f| f["name"].as_str().unwrap_or("").to_string())
                        .collect()
                })
                .unwrap_or_default();
            self.values.insert(name, fields);
        }
        for e in v["enums"].as_array().into_iter().flatten() {
            let name = e["name"].as_str().unwrap_or("").to_string();
            let values = e["values"]
                .as_array()
                .map(|vs| {
                    vs.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let doc = e["doc"].as_str().unwrap_or("").to_string();
            self.enums.insert(name, (doc, values));
        }
        for a in v["aliases"].as_array().into_iter().flatten() {
            if let (Some(alias), Some(of)) = (a["name"].as_str(), a["of"].as_str()) {
                self.aliases.insert(alias.to_string(), of.to_string());
            }
        }
        for r in v["records"].as_array().into_iter().flatten() {
            let name = r["name"].as_str().unwrap_or("").to_string();
            let fields = r["fields"]
                .as_array()
                .map(|fs| {
                    fs.iter()
                        .map(|f| {
                            let raw = f["type"].as_str().unwrap_or("void");
                            // `Option<T>` is an optional key; `Option<Option<T>>` also takes `null`.
                            let (name, ty) = match raw.strip_prefix("opt:") {
                                Some(inner) => (
                                    format!("{}?", f["name"].as_str().unwrap_or("")),
                                    ts_type(inner),
                                ),
                                None => {
                                    (f["name"].as_str().unwrap_or("").to_string(), ts_type(raw))
                                }
                            };
                            (name, ty, f["doc"].as_str().unwrap_or("").to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();
            let doc = r["doc"].as_str().unwrap_or("").to_string();
            self.records.insert(name, (doc, fields));
        }
        for c in v["classes"].as_array().into_iter().flatten() {
            let name = c["name"].as_str().unwrap_or("").to_string();
            if let Some(d) = c["doc"].as_str().filter(|d| !d.is_empty()) {
                self.owner_docs.insert(name.clone(), d.to_string());
            }
            let fs = funcs(&c["functions"], like);
            let members = self.classes.entry(name.clone()).or_default();
            let props = self.props.entry(name.clone()).or_default();
            let docs = &mut self.docs;
            let mut add = |members: &mut Vec<String>, line: String, doc: &str| {
                if !doc.is_empty() && !members.contains(&line) {
                    docs.insert((name.clone(), line.clone()), doc.to_string());
                }
                push_unique(members, line);
            };
            for f in &fs {
                if let Some(k) = &f.object_keys {
                    named.push(k.clone());
                }
                match f.kind.as_str() {
                    "static" if f.ctor => {
                        self.ctor_params.insert(name.clone(), f.params.clone());
                    }
                    "static" => {
                        let options = options_params(f, false);
                        if !(f.options_only && options.is_some()) {
                            add(
                                members,
                                format!("static {}({}): {};", f.name, params_text(f, false), f.ret),
                                &f.doc,
                            );
                        }
                        if let Some(o) = options {
                            add(
                                members,
                                format!("static {}({o}): {};", f.name, f.ret),
                                &f.doc,
                            );
                            if let Some(n) = &f.options_name {
                                named.push((n.clone(), option_keys(f, false)));
                            }
                        }
                        if f.name == "create"
                            && f.object_keys.is_none()
                            && !self.creates.contains_key(&name)
                        {
                            self.creates.insert(name.clone(), f.params.clone());
                            if let Some(a) = f.options_after {
                                self.creates_after.insert(name.clone(), a);
                            }
                        }
                    }
                    "method" => {
                        let options = options_params(f, true);
                        if !(f.options_only && options.is_some()) {
                            add(
                                members,
                                format!("{}({}): {};", f.name, params_text(f, true), f.ret),
                                &f.doc,
                            );
                        }
                        if let Some(o) = options {
                            add(members, format!("{}({o}): {};", f.name, f.ret), &f.doc);
                            if let Some(n) = &f.options_name {
                                named.push((n.clone(), option_keys(f, true)));
                            }
                        }
                    }
                    "get" if f.params.is_empty() => {
                        // A static property of the class (no receiver).
                        let writable = fs
                            .iter()
                            .any(|g| g.kind == "set" && g.name == f.name && g.params.len() == 1);
                        let line = if writable {
                            format!("static {}: {};", f.name, f.ret)
                        } else {
                            format!("static readonly {}: {};", f.name, f.ret)
                        };
                        add(members, line, &f.doc);
                    }
                    "set" if f.params.len() == 1 => {
                        let has_getter = fs
                            .iter()
                            .any(|g| g.kind == "get" && g.name == f.name && g.params.is_empty());
                        if !has_getter {
                            add(
                                members,
                                format!("static {}: {};", f.name, f.params[0].1),
                                &f.doc,
                            );
                        }
                    }
                    "get" => {
                        let setter = fs
                            .iter()
                            .find(|g| g.kind == "set" && g.name == f.name && g.params.len() > 1);
                        let writable = setter.is_some();
                        // Several setters of one name (a number or a list of two): the value is
                        // the union of the types they take.
                        let mut takes: Vec<String> = Vec::new();
                        for g in fs
                            .iter()
                            .filter(|g| g.kind == "set" && g.name == f.name && g.params.len() > 1)
                        {
                            if let Some((_, t, _)) = g.params.last()
                                && !takes.contains(t)
                            {
                                takes.push(t.clone());
                            }
                        }
                        // Setters of several types: one property of the union (a pair of accessors
                        // would make `Partial<T>` read only the getter's type).
                        if takes.len() > 1 && !members.iter().any(|m| member_name(m) == f.name) {
                            let mut all = vec![f.ret.clone()];
                            for t in &takes {
                                if !all.contains(t) {
                                    all.push(t.clone());
                                }
                            }
                            let union = all.join(" | ");
                            props.push((f.name.clone(), union.clone()));
                            add(members, format!("{}: {union};", f.name), &f.doc);
                            continue;
                        }
                        // A property that reads as a narrower type than it takes: an accessor pair.
                        if setter.is_some()
                            && !takes.is_empty()
                            && takes.join(" | ") != f.ret
                            && !f.ret.ends_with("| null")
                            && !members.iter().any(|m| member_name(m) == f.name)
                        {
                            let wide = takes.join(" | ");
                            props.push((f.name.clone(), wide.clone()));
                            add(members, format!("get {}(): {};", f.name, f.ret), &f.doc);
                            add(members, format!("set {}(v: {wide});", f.name), "");
                            continue;
                        }
                        let line = if writable {
                            props.push((f.name.clone(), f.ret.clone()));
                            format!("{}: {};", f.name, f.ret)
                        } else {
                            format!("readonly {}: {};", f.name, f.ret)
                        };
                        // A property declared once; the same name as a method elsewhere is skipped.
                        if !members.iter().any(|m| {
                            m.trim_start_matches("readonly ")
                                .starts_with(&format!("{}:", f.name))
                        }) {
                            add(members, line, &f.doc);
                        }
                    }
                    "set" => {
                        let has_getter = fs
                            .iter()
                            .any(|g| g.kind == "get" && g.name == f.name && !g.params.is_empty());
                        if !has_getter {
                            let ty = f
                                .params
                                .get(1)
                                .map(|p| p.1.clone())
                                .unwrap_or_else(|| "unknown".into());
                            props.push((f.name.clone(), ty.clone()));
                            add(members, format!("{}: {ty};", f.name), &f.doc);
                        }
                    }
                    _ => {}
                }
            }
        }
        let ns_name = v["namespace"].as_str().unwrap_or("").to_string();
        let fs = funcs(&v["functions"], like);
        if !fs.is_empty() {
            if let Some(d) = v["doc"].as_str().filter(|d| !d.is_empty()) {
                self.owner_docs.insert(ns_name.clone(), d.to_string());
            }
            let members = self.namespaces.entry(ns_name.clone()).or_default();
            let docs = &mut self.docs;
            let mut add = |members: &mut Vec<String>, line: String, doc: &str| {
                if !doc.is_empty() && !members.contains(&line) {
                    docs.insert((ns_name.clone(), line.clone()), doc.to_string());
                }
                push_unique(members, line);
            };
            for f in &fs {
                // `function in(..)` is not a declaration TypeScript accepts (the class statics keep it).
                if is_reserved(&f.name) {
                    continue;
                }
                if let Some(k) = &f.object_keys {
                    named.push(k.clone());
                }
                match f.kind.as_str() {
                    "get" => {
                        let writable = fs.iter().any(|g| g.kind == "set" && g.name == f.name);
                        let kw = if writable { "let" } else { "const" };
                        let line = format!("{kw} {}: {};", f.name, f.ret);
                        if !members.iter().any(|m| {
                            m.starts_with(&format!("let {}:", f.name))
                                || m.starts_with(&format!("const {}:", f.name))
                        }) {
                            add(members, line, &f.doc);
                        }
                    }
                    "set" => {}
                    _ => {
                        if f.global {
                            self.globals.insert(
                                f.name.clone(),
                                (
                                    format!(
                                        "function {}({}): {};",
                                        f.name,
                                        params_text(f, false),
                                        f.ret
                                    ),
                                    f.doc.clone(),
                                ),
                            );
                        }
                        let options = options_params(f, false);
                        if !(f.options_only && options.is_some()) {
                            add(
                                members,
                                format!(
                                    "function {}({}): {};",
                                    f.name,
                                    params_text(f, false),
                                    f.ret
                                ),
                                &f.doc,
                            );
                        }
                        if let Some(o) = options {
                            add(
                                members,
                                format!("function {}({o}): {};", f.name, f.ret),
                                &f.doc,
                            );
                            if let Some(n) = &f.options_name {
                                named.push((n.clone(), option_keys(f, false)));
                            }
                        }
                    }
                }
            }
        }
        for (name, keys) in named {
            let entry = self.option_types.entry(name).or_default();
            for k in keys {
                if !entry.iter().any(|e| e.0 == k.0) {
                    entry.push(k);
                }
            }
        }
        Ok(())
    }

    /// `{ prop?: T; .. }` over the writable properties of a class: the keys of `new Class({ .. })`.
    fn options_type(&self, name: &str) -> String {
        let props: Vec<String> = self
            .props
            .get(name)
            .into_iter()
            .flatten()
            // A built-in member of the same name declares it otherwise.
            .filter(|(n, _)| {
                !crate::builtins::members(name).any(|b| crate::builtins::member_name(b.sig) == n)
            })
            // A value class key takes a plain object too, as its setter's parameter does
            // (`mainTextureScale: { x: 4, y: 4 }`).
            .map(|(n, t)| match self.values.contains_key(t.as_str()) {
                true => format!("{n}?: {t}Like"),
                false => format!("{n}?: {t}"),
            })
            .collect();
        format!("{{ {} }}", props.join("; "))
    }

    /// The constructor lines of a class: the built-in ones, else the value class's fields, else
    /// the handle class's `create` (and the options form the compiler also accepts).
    fn ctor_lines(&self, name: &str) -> Vec<String> {
        let built: Vec<String> = crate::builtins::ctors(name)
            .map(|b| b.sig.to_string())
            .collect();
        if !built.is_empty() {
            return built;
        }
        if let Some(fields) = self.values.get(name) {
            let ctor: Vec<String> = fields.iter().map(|f| format!("{f}: number")).collect();
            return vec![format!("constructor({});", ctor.join(", "))];
        }
        // `#[constructor]`: the positional form, then the options object.
        if let Some(pos) = self.ctor_params.get(name) {
            let mut seen_optional = false;
            let params: Vec<String> = pos
                .iter()
                .map(|(n, t, o)| {
                    seen_optional |= *o;
                    format!(
                        "{}{}: {t}",
                        safe_ident(n),
                        if seen_optional { "?" } else { "" }
                    )
                })
                .collect();
            let mut out = vec![format!("constructor({});", params.join(", "))];
            if self.props.get(name).is_some_and(|p| !p.is_empty()) {
                out.push(format!(
                    "constructor(options?: {});",
                    self.options_type(name)
                ));
            }
            return out;
        }
        let Some(create) = self.creates.get(name) else {
            return vec![];
        };
        // `#[options(after = p)]`: the parameters up to `p`, then the rest as the keys of an object.
        if let Some(&after) = self.creates_after.get(name) {
            let mut seen_optional = false;
            let mut parts: Vec<String> = Vec::new();
            for (n, t, o) in &create[..=after.min(create.len() - 1)] {
                seen_optional |= *o;
                parts.push(format!(
                    "{}{}: {t}",
                    safe_ident(n),
                    if seen_optional { "?" } else { "" }
                ));
            }
            let keys: Vec<String> = create[after + 1..]
                .iter()
                .map(|(n, t, _)| format!("{n}?: {t}"))
                .collect();
            parts.push(format!("options?: {{ {} }}", keys.join("; ")));
            return vec![format!("constructor({});", parts.join(", "))];
        }
        let opts = self.options_type(name);
        let mut seen_optional = false;
        let pos: Vec<String> = create
            .iter()
            .map(|(n, t, o)| {
                seen_optional |= *o;
                format!(
                    "{}{}: {t}",
                    safe_ident(n),
                    if seen_optional { "?" } else { "" }
                )
            })
            .collect();
        let mut out = vec![format!("constructor({});", pos.join(", "))];
        let named = self.props.get(name).is_some_and(|p| !p.is_empty());
        if named {
            let mut with = pos.clone();
            let q = if seen_optional { "?" } else { "" };
            with.push(format!("options{q}: {opts}"));
            out.push(format!("constructor({});", with.join(", ")));
            if create.is_empty() && self.props[name].iter().any(|(n, _)| n == "name") {
                out.push(format!("constructor(name: string, options?: {opts});"));
            }
        }
        out
    }

    /// `prefix` is `declare ` at the top level and `export ` inside the module block.
    fn text(&self, prefix: &str, indent: &str) -> String {
        let mut s = String::new();
        for h in &self.handles {
            if self.classes.contains_key(h) || self.values.contains_key(h) {
                continue;
            }
            s +=
                &format!("{indent}{prefix}type {h} = number & {{ readonly __handle: \"{h}\" }};\n");
        }
        // Enums are string unions, with `Enum.name(i)` and `Enum.parse(s)` to convert the index.
        for (name, (doc, values)) in &self.enums {
            s += &jsdoc(doc, indent);
            let union: Vec<String> = values.iter().map(|v| format!("\"{v}\"")).collect();
            s += &format!("{indent}{prefix}type {name} = {};\n", union.join(" | "));
            s += &format!("{indent}{prefix}namespace {name} {{\n");
            s += &format!(
                "{indent}  /** The name at an index (the order of the union). */\n{indent}  function name(index: number): {name};\n"
            );
            s += &format!(
                "{indent}  /** The index of a name, or -1. */\n{indent}  function parse(name: string): number;\n"
            );
            s += &format!("{indent}}}\n");
        }
        // Other names for a class (a type and a value) or an enum (a type).
        for (alias, target) in &self.aliases {
            s += &format!("{indent}{prefix}type {alias} = {target};\n");
            if self.classes.contains_key(target) {
                s += &format!("{indent}{prefix}const {alias}: typeof {target};\n");
            }
        }
        // The named options objects of calls: an interface with a key per optional parameter.
        for (name, keys) in &self.option_types {
            s += &format!("{indent}{prefix}interface {name} {{\n");
            for (key, ty, doc, required) in keys {
                s += &jsdoc(doc, &format!("{indent}  "));
                let q = if *required { "" } else { "?" };
                s += &format!("{indent}  {key}{q}: {ty};\n");
            }
            s += &format!("{indent}}}\n");
        }
        for (name, (doc, fields)) in &self.records {
            s += &jsdoc(doc, indent);
            s += &format!("{indent}{prefix}interface {name} {{\n");
            for (f, t, d) in fields {
                s += &jsdoc(d, &format!("{indent}  "));
                s += &format!("{indent}  {f}: {t};\n");
            }
            s += &format!("{indent}}}\n");
        }
        let mut names: BTreeSet<&String> = self.classes.keys().collect();
        names.extend(self.values.keys());
        for name in names {
            if let Some(d) = self.owner_docs.get(name) {
                s += &jsdoc(d, indent);
            }
            s += &format!("{indent}{prefix}class {name} {{\n");
            for c in self.ctor_lines(name) {
                s += &format!("{indent}  {c}\n");
            }
            if let Some(fields) = self.values.get(name) {
                for f in fields {
                    s += &format!("{indent}  {f}: number;\n");
                }
            }
            for m in self.classes.get(name).into_iter().flatten() {
                // A value field and a property of the same name: the field wins.
                if let Some(fields) = self.values.get(name)
                    && fields.iter().any(|f| {
                        m.trim_start_matches("readonly ")
                            .starts_with(&format!("{f}:"))
                    })
                {
                    continue;
                }
                // A compiler built-in of the same name replaces it.
                let same = |b: &crate::builtins::Builtin| {
                    crate::builtins::member_name(b.sig) == member_name(m)
                };
                let hit = if m.starts_with("static ") {
                    crate::builtins::statics(name).any(same)
                } else {
                    crate::builtins::members(name).any(same)
                };
                if hit {
                    continue;
                }
                if let Some(d) = self.docs.get(&(name.clone(), m.clone())) {
                    s += &jsdoc(d, &format!("{indent}  "));
                }
                s += &format!("{indent}  {m}\n");
            }
            for b in crate::builtins::members(name) {
                s += &jsdoc(b.doc, &format!("{indent}  "));
                s += &format!("{indent}  {}\n", b.sig);
            }
            for b in crate::builtins::statics(name) {
                s += &jsdoc(b.doc, &format!("{indent}  "));
                s += &format!("{indent}  static {}\n", b.sig);
            }
            // `Class.create({ .. })` sets the keys as properties, like `new Class({ .. })`.
            if self.creates.get(name).is_some_and(|c| c.is_empty())
                && self.props.get(name).is_some_and(|p| !p.is_empty())
                && !crate::builtins::statics(name)
                    .any(|b| crate::builtins::member_name(b.sig) == "create")
            {
                s += &format!(
                    "{indent}  static create(options: {}): {name};\n",
                    self.options_type(name)
                );
            }
            // A component handle (it has a `gameObject`) also has what every component has.
            let members = self.classes.get(name);
            if members.is_some_and(|m| m.iter().any(|l| l == "readonly gameObject: GameObject;")) {
                let has = |n: &str| {
                    members.is_some_and(|m| m.iter().any(|l| member_name(l) == n))
                        || crate::builtins::members(name)
                            .any(|b| crate::builtins::member_name(b.sig) == n)
                };
                if !has("transform") {
                    s += &format!("{indent}  readonly transform: Transform;\n");
                }
                if !has("getComponent") {
                    s += &format!(
                        "{indent}  getComponent<T>(cls: {{ prototype: T }}): T | undefined;\n"
                    );
                }
            }
            s += &format!("{indent}}}\n");
        }
        for (name, members) in &self.namespaces {
            if let Some(d) = self.owner_docs.get(name) {
                s += &jsdoc(d, indent);
            }
            s += &format!("{indent}{prefix}namespace {name} {{\n");
            for m in members {
                if crate::builtins::statics(name)
                    .any(|b| crate::builtins::member_name(b.sig) == member_name(m))
                {
                    continue;
                }
                if let Some(d) = self.docs.get(&(name.clone(), m.clone())) {
                    s += &jsdoc(d, &format!("{indent}  "));
                }
                s += &format!("{indent}  {m}\n");
            }
            // A class of the same name has them as statics already.
            for b in crate::builtins::statics(name).filter(|_| !self.classes.contains_key(name)) {
                s += &jsdoc(b.doc, &format!("{indent}  "));
                s += &format!(
                    "{indent}  {}\n",
                    crate::builtins::as_namespace_member(b.sig)
                );
            }
            s += &format!("{indent}}}\n");
        }
        for (line, doc) in self.globals.values() {
            s += &jsdoc(doc, indent);
            s += &format!("{indent}{prefix}{line}\n");
        }
        s
    }
}

/// Whether `text` names `word` (not as part of a longer name).
fn contains_word(text: &str, word: &str) -> bool {
    let is_name = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.match_indices(word).any(|(i, _)| {
        !text[..i].chars().next_back().is_some_and(is_name)
            && !text[i + word.len()..].chars().next().is_some_and(is_name)
    })
}

fn reexports(module: &str, names: &[String]) -> String {
    let mut s = format!("declare module \"{module}\" {{\n");
    for n in names {
        s += &format!("  export import {n} = globalThis.{n};\n");
    }
    s += "}\n";
    s
}

/// The declarations for every manifest as globals (every class, namespace, value class and handle
/// type is a top-level `declare`), then the UI declarations (`ui_dts`: React-style
/// names, `JSX`, hooks, UI components), then what the compiler gives (`builtins.rs`), then the
/// modules `@platypus/engine`, `@platypus/react` and `react` as re-exports of those globals. A game needs
/// no import; the old imports still work.
pub fn generate(manifests: &[String]) -> Result<String, String> {
    let mut d = Decls::default();
    for m in manifests {
        d.add(m)?;
    }
    let mut out = String::from(
        "// Generated by `platypus-build types` from the engine manifests. Do not edit.\n\
         // Every engine and UI name is a global, so a game type-checks with no import and no node_modules;\n\
         // `import { Vector3 } from \"@platypus/engine\"` (and `react`, `@platypus/react`) work too.\n\n",
    );
    out += &d.text("declare ", "");
    out += "\n";
    out += &crate::ui_dts::declarations();
    out += "\n// What the compiler gives a program (`builtins.rs`).\n";
    out += &crate::builtins::top_level();
    // The structural `<Value>Like` types (a plain `{ x, y, z }` object does) that the declarations
    // name.
    let mut likes: Vec<String> = Vec::new();
    for (name, fields) in &d.values {
        let like = format!("{name}Like");
        if contains_word(&out, &like) {
            let fs: Vec<String> = fields.iter().map(|f| format!("{f}: number")).collect();
            out += &format!("type {like} = {{ {} }};\n", fs.join("; "));
            likes.push(like);
        }
    }
    let mut engine: BTreeSet<String> = d.handles.iter().cloned().collect();
    engine.extend(d.classes.keys().cloned());
    engine.extend(d.values.keys().cloned());
    engine.extend(d.enums.keys().cloned());
    engine.extend(d.records.keys().cloned());
    engine.extend(d.aliases.keys().cloned());
    engine.extend(d.option_types.keys().cloned());
    engine.extend(d.namespaces.keys().filter(|n| !n.is_empty()).cloned());
    engine.extend(d.globals.keys().cloned());
    engine.extend(likes);
    engine.extend(crate::builtins::top_names().into_iter().map(String::from));
    out += "\n";
    out += &reexports("@platypus/engine", &engine.into_iter().collect::<Vec<_>>());
    out += "\n";
    out += &reexports("@platypus/react", &crate::ui_dts::platypus_react_exports());
    out += "\n";
    out += &reexports("react", &crate::ui_dts::react_exports());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = r#"{"namespace":"Time","handles":["Thing"],"values":[{"name":"V2","fields":[{"name":"x","type":"f64"},{"name":"y","type":"f64"}]}],
      "classes":[{"name":"Obj","functions":[
        {"name":"make","symbol":"s1","kind":"static","params":[{"name":"n","type":"string"}],"returns":"handle:Obj"},
        {"name":"move","symbol":"s2","kind":"method","params":[{"name":"this","type":"handle:Obj"},{"name":"to","type":"value:V2","optional":true}],"returns":"void"},
        {"name":"hp","symbol":"s3","kind":"get","params":[{"name":"this","type":"handle:Obj"}],"returns":"f64"},
        {"name":"hp","symbol":"s4","kind":"set","params":[{"name":"this","type":"handle:Obj"},{"name":"v","type":"f64"}],"returns":"void"},
        {"name":"id","symbol":"s5","kind":"get","params":[{"name":"this","type":"handle:Obj"}],"returns":"u32"}]}],
      "functions":[{"name":"deltaTime","symbol":"t1","kind":"get","params":[],"returns":"f64"},
        {"name":"on","symbol":"t2","kind":"static","params":[{"name":"cb","type":"fn(f64,string):void"}],"returns":"u32"}]}"#;

    // M9-TS-1810
    #[test]
    fn manifests_become_global_declarations_that_the_engine_module_re_exports() {
        let t = generate(&[DEMO.to_string()]).unwrap();
        assert!(t.contains("declare module \"@platypus/engine\" {"), "{t}");
        assert!(t.contains("  export import V2 = globalThis.V2;"), "{t}");
        assert!(t.contains("\ndeclare type Thing = number &"), "{t}");
        assert!(
            t.contains("\ndeclare class V2 {\n  constructor(x: number, y: number);"),
            "{t}"
        );
        assert!(t.contains("static make(n: string): Obj;"), "{t}");
        assert!(t.contains("move(to?: V2): void;"), "{t}");
        assert!(
            t.contains("hp: number;") && t.contains("readonly id: number;"),
            "{t}"
        );
        assert!(t.contains("const deltaTime: number;"), "{t}");
        assert!(
            t.contains("function on(cb: (a0: number, a1: string) => void): number;"),
            "{t}"
        );
        // The UI declarations come from the tables.
        assert!(
            t.contains("\ninterface PlatypusStyle {\n")
                && t.contains("\ndeclare function useState<T>("),
            "{t}"
        );
        assert!(t.contains("export import Obj = globalThis.Obj;"), "{t}");
        // The same input gives the same text.
        assert_eq!(t, generate(&[DEMO.to_string()]).unwrap());
    }

    const CREATABLE: &str = r#"{"namespace":"Audio","handles":[],"values":[],
      "classes":[{"name":"Thing","functions":[
        {"name":"create","symbol":"c1","kind":"static","params":[],"returns":"handle:Thing"},
        {"name":"name","symbol":"c2","kind":"get","params":[{"name":"this","type":"handle:Thing"}],"returns":"string"},
        {"name":"name","symbol":"c3","kind":"set","params":[{"name":"this","type":"handle:Thing"},{"name":"v","type":"string"}],"returns":"void"},
        {"name":"size","symbol":"c4","kind":"get","params":[{"name":"this","type":"handle:Thing"}],"returns":"f64"},
        {"name":"size","symbol":"c5","kind":"set","params":[{"name":"this","type":"handle:Thing"},{"name":"v","type":"f64"}],"returns":"void"},
        {"name":"gameObject","symbol":"c6","kind":"get","params":[{"name":"this","type":"handle:Thing"}],"returns":"handle:GameObject"}]}],
      "functions":[
        {"name":"load","symbol":"f1","kind":"static","params":[{"name":"path","type":"string"},{"name":"stream","type":"bool"}],"returns":"handle:Thing"},
        {"name":"mix","symbol":"f2","kind":"static","params":[{"name":"a","type":"f64","optional":true},{"name":"b","type":"f64"}],"returns":"void"},
        {"name":"in","symbol":"f3","kind":"static","params":[],"returns":"void"}]}"#;

    // M9-TS-2310
    #[test]
    fn a_call_gets_an_options_object_form_and_a_creatable_class_constructors() {
        let t = generate(&[CREATABLE.to_string()]).unwrap();
        // Leading required parameters stay positional; the rest are keys.
        assert!(
            t.contains("function load(path: string, options?: { stream?: boolean }): Thing;"),
            "{t}"
        );
        // A required parameter after an optional one becomes optional.
        assert!(
            t.contains("function mix(a?: number, b?: number): void;"),
            "{t}"
        );
        assert!(
            t.contains("function mix(options?: { a?: number; b?: number }): void;"),
            "{t}"
        );
        // `function in(..)` is not valid TypeScript: left out.
        assert!(!t.contains("function in("), "{t}");
        assert!(t.contains("  constructor();\n"), "{t}");
        assert!(
            t.contains("  constructor(options: { name?: string; size?: number });"),
            "{t}"
        );
        assert!(
            t.contains("  constructor(name: string, options?: { name?: string; size?: number });"),
            "{t}"
        );
        assert!(
            t.contains("  static create(options: { name?: string; size?: number }): Thing;"),
            "{t}"
        );
        // A component handle (it has a `gameObject`) gets a transform and getComponent.
        assert!(t.contains("  readonly transform: Transform;"), "{t}");
        assert!(
            t.contains("getComponent<T>(cls: { prototype: T }): T | undefined;"),
            "{t}"
        );
    }

    const BUILT: &str = r#"{"namespace":"Random","handles":["GameObject"],"values":[{"name":"Vector3","fields":[{"name":"x","type":"f64"},{"name":"y","type":"f64"},{"name":"z","type":"f64"}]}],
      "classes":[{"name":"GameObject","functions":[
        {"name":"create","symbol":"g1","kind":"static","params":[],"returns":"handle:GameObject"},
        {"name":"instantiate","symbol":"g2","kind":"static","params":[{"name":"o","type":"handle:GameObject"}],"returns":"handle:GameObject"},
        {"name":"children","symbol":"g3","kind":"get","params":[{"name":"this","type":"handle:GameObject"}],"returns":"f64"}]}],
      "functions":[{"name":"pick","symbol":"r1","kind":"static","params":[],"returns":"f64"}]}"#;

    // M9-TS-2311
    #[test]
    fn the_built_ins_replace_members_of_the_same_name_and_add_constructors() {
        let t = generate(&[BUILT.to_string()]).unwrap();
        // The native members of the same names are gone; the built-in ones are in the class.
        assert!(!t.contains("static instantiate(o: GameObject)"), "{t}");
        assert!(
            t.contains("  static instantiate(original: GameObject | Prefab,"),
            "{t}"
        );
        assert!(!t.contains("readonly children: number;"), "{t}");
        assert!(t.contains("  readonly children: GameObject[];"), "{t}");
        assert!(
            t.contains("  constructor(name?: string, options?: GameObjectOptions);"),
            "{t}"
        );
        assert!(!t.contains("  constructor();\n"), "{t}");
        // In a namespace a static is a function.
        assert!(
            t.contains("  function pick<T>(items: readonly T[]): T | undefined;"),
            "{t}"
        );
        assert!(!t.contains("function pick(): number;"), "{t}");
        // The top-level ones, and the `Like` type they name, are exported by the module.
        assert!(t.contains("\ndeclare abstract class Component {\n"), "{t}");
        assert!(
            t.contains("type Vector3Like = { x: number; y: number; z: number };"),
            "{t}"
        );
        for n in [
            "Component",
            "GameObjectOptions",
            "setTimeout",
            "Vector3Like",
        ] {
            assert!(
                t.contains(&format!("  export import {n} = globalThis.{n};")),
                "{n}: {t}"
            );
        }
    }

    const RECORDS: &str = r#"{"namespace":"Rec","doc":"Things that exist.","like":true,"handles":["Thing"],
      "values":[{"name":"Point","fields":[{"name":"x","type":"f64"},{"name":"y","type":"f64"}]}],
      "enums":[{"name":"FillMode","doc":"How to fill.","values":["solid","pingPong"],"nameSymbol":"platypus_FillMode_name","parseSymbol":"platypus_FillMode_parse"}],
      "records":[{"name":"Spot","doc":"A spot.","fields":[{"name":"at","type":"value:Point"},{"name":"weight","type":"f64","doc":"How heavy."},{"name":"hard","type":"bool"},{"name":"fill","type":"enum:FillMode"}]}],
      "classes":[{"name":"Thing","doc":"A thing.","functions":[
        {"name":"create","symbol":"c1","kind":"static","doc":"Makes one.","optionsAfter":0,"params":[{"name":"points","type":"values:Point","optional":true},{"name":"weight","type":"f64","optional":true},{"name":"fill","type":"enum:FillMode","optional":true}],"returns":"handle:Thing"},
        {"name":"spots","symbol":"c2","kind":"get","params":[{"name":"this","type":"handle:Thing"}],"returns":"list:record:Spot","doc":"Its spots."},
        {"name":"fill","symbol":"c3","kind":"get","params":[{"name":"this","type":"handle:Thing"}],"returns":"enum:FillMode"},
        {"name":"near","symbol":"c4","kind":"method","params":[{"name":"this","type":"handle:Thing"},{"name":"p","type":"value:Point"}],"returns":"record:Spot"}]}],
      "functions":[
        {"name":"many","symbol":"f1","kind":"static","doc":"The points as given.","params":[{"name":"points","type":"values:Point","rest":true}],"returns":"list:value:Point"},
        {"name":"mix","symbol":"f2","kind":"static","params":[{"name":"a","type":"value:Point"},{"name":"b","type":"values:Point"}],"returns":"void"}]}"#;

    // M9-TS-3103
    #[test]
    fn enums_records_lists_rest_docs_and_like_become_declarations() {
        let t = generate(&[RECORDS.to_string()]).unwrap();
        // Enums: a string union and the converters.
        assert!(
            t.contains(
                "/**\n * How to fill.\n */\ndeclare type FillMode = \"solid\" | \"pingPong\";"
            ),
            "{t}"
        );
        assert!(t.contains("declare namespace FillMode {"), "{t}");
        assert!(t.contains("function name(index: number): FillMode;"), "{t}");
        assert!(t.contains("function parse(name: string): number;"), "{t}");
        // Records: an interface with the field docs.
        assert!(t.contains("declare interface Spot {"), "{t}");
        assert!(t.contains("  at: Point;"), "{t}");
        assert!(t.contains("   * How heavy.\n"), "{t}");
        assert!(t.contains("  fill: FillMode;"), "{t}");
        // Lists, records and enums as results; `like` for value parameters only.
        assert!(t.contains("readonly spots: Spot[];"), "{t}");
        assert!(t.contains("readonly fill: FillMode;"), "{t}");
        assert!(t.contains("near(p: PointLike): Spot;"), "{t}");
        assert!(
            t.contains("function many(...points: PointLike[]): Point[];"),
            "{t}"
        );
        assert!(
            t.contains("function mix(a: PointLike, b: readonly PointLike[]): void;"),
            "{t}"
        );
        // Docs on the namespace, a class and their members.
        assert!(
            t.contains("/**\n * Things that exist.\n */\ndeclare namespace Rec {"),
            "{t}"
        );
        assert!(
            t.contains("/**\n * A thing.\n */\ndeclare class Thing {"),
            "{t}"
        );
        assert!(t.contains("   * Its spots.\n"), "{t}");
        assert!(t.contains("   * The points as given.\n"), "{t}");
        // `#[options(after = points)]`: the positional parameters, then one options object.
        assert!(
            t.contains(
                "  constructor(points?: readonly PointLike[], options?: { weight?: number; fill?: FillMode });"
            ),
            "{t}"
        );
        assert!(
            !t.contains("constructor(points?: readonly PointLike[], weight"),
            "{t}"
        );
        // The module re-exports the enum and the record.
        assert!(
            t.contains("  export import FillMode = globalThis.FillMode;"),
            "{t}"
        );
        assert!(t.contains("  export import Spot = globalThis.Spot;"), "{t}");
    }

    const OPTIONS: &str = r#"{"namespace":"Opt","handles":["Thing"],"values":[],"classes":[{"name":"Thing","functions":[
      {"name":"load","symbol":"o1","kind":"static","doc":"Loads one.","optionsName":"LoadOptions","optionsOnly":true,"params":[{"name":"src","type":"string"},{"name":"mode","type":"string","optional":true,"doc":"How."},{"name":"owner","type":"handle:Thing","optional":true},{"name":"names","type":"strings","optional":true}],"returns":"handle:Thing"},
      {"name":"put","symbol":"o2","kind":"static","params":[{"name":"all","type":"f64s"}],"returns":"void"}]}],"functions":[]}"#;

    // M9-TS-3342
    #[test]
    fn a_named_options_object_is_an_interface_with_docs_and_array_parameters_are_readonly() {
        let t = generate(&[OPTIONS.to_string()]).unwrap();
        assert!(
            t.contains(
                "declare interface LoadOptions {
  /**
   * How.
   */
  mode?: string;
  owner?: Thing | null;
  names?: readonly string[];
}"
            ),
            "{t}"
        );
        assert!(
            t.contains("  static load(src: string, options?: LoadOptions): Thing;"),
            "{t}"
        );
        // `only`: no positional form.
        assert!(!t.contains("static load(src: string, mode"), "{t}");
        assert!(
            t.contains("static put(all: readonly number[]): void;"),
            "{t}"
        );
        assert!(
            t.contains("  export import LoadOptions = globalThis.LoadOptions;"),
            "{t}"
        );
    }
}
