// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The engine API as the compiler sees it: the JSON manifest `ts_api` generates
//! (`Namespace::to_json`), read into a table of functions the TypeScript can call directly.
//!
//! ```json
//! {"namespace":"Demo","functions":[{"name":"add","symbol":"platypus_Demo_add",
//!   "params":[{"name":"a","type":"f64"}],"returns":"f64"}]}
//! ```
//!
//! Types are `void`, `bool`, `f64`, `f32`, `i32`, `u32`, `handle:Name` and `value:Name`.
//!
//! A namespace may also list handle `classes`: each function has a `kind` (`static`, `method`, `get`,
//! `set`), and the ones that act on an object take it as their first parameter, named `this`.

use crate::ir::ExtTy;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct ExternFn {
    pub symbol: String,
    pub params: Vec<(String, ExtTy)>,
    pub ret: ExtTy,
    /// Index of the first parameter a call may leave out (`params.len()`: none).
    pub optional_from: usize,
    /// Per parameter: the value a left-out one takes (a flag is 0 or 1), if the API says.
    pub defaults: Vec<Option<f64>>,
    /// The last parameter takes every remaining argument (`...points: Vector2[]`).
    pub rest: bool,
}

/// The functions of a handle class, by how scripts call them. Methods, getters and setters take
/// the receiver as their first parameter.
#[derive(Clone, Debug, Default)]
pub struct ClassFns {
    pub statics: HashMap<String, ExternFn>,
    pub methods: HashMap<String, ExternFn>,
    pub getters: HashMap<String, ExternFn>,
    pub setters: HashMap<String, ExternFn>,
    /// Setters of static properties (no receiver: the value is the only parameter).
    pub static_setters: HashMap<String, ExternFn>,
    /// Functions of one name declared more than once (a method taking a handle or a name and a
    /// type): (table: `static`, `method` or `set`, name) -> every version in declaration
    /// order. The table keeps the first.
    pub overloads: HashMap<(String, String), Vec<ExternFn>>,
    /// The positional constructor (`#[constructor]`): `new Material(color, emission, ..)`.
    pub ctor: Option<ExternFn>,
}

#[derive(Clone, Debug, Default)]
pub struct Manifest {
    /// Namespace -> function name -> function.
    pub namespaces: HashMap<String, HashMap<String, ExternFn>>,
    /// Handle type names (`Entity`): types scripts can name.
    pub handles: HashSet<String>,
    /// Value classes (`Vector3`): their fields in order, each a scalar type.
    pub values: HashMap<String, Vec<(String, ExtTy)>>,
    /// Handle classes (`GameObject`) by name.
    pub classes: HashMap<String, ClassFns>,
    /// Functions declared more than once under one name, picked by the argument types.
    pub overloads: HashMap<(String, String), Vec<ExternFn>>,
    /// Properties of a namespace (`Time.deltaTime`, `Gizmos.color = c`): namespace -> name -> accessors.
    pub ns_props: HashMap<String, HashMap<String, NsProp>>,
    /// Enums (`TangentMode`) by name: scripts see strings, the wire is the index.
    pub enums: HashMap<String, EnumInfo>,
    /// Result records (`SplineHit`) by name.
    pub records: HashMap<String, Vec<(String, RecField)>>,
    /// Other script names for a class or an enum: alias -> target (`AnimatorController` ->
    /// `AnimationController`).
    pub aliases: HashMap<String, String>,
    /// Functions that take a callback and return a token (`Screen.onChange`): (namespace, name) ->
    /// the name of the function that stops the listener. Scripts get a function that does it.
    pub subscriptions: HashMap<(String, String), String>,
    /// Subscriptions whose callback gets a record as JSON text: (namespace, name) -> the record's
    /// name (`input.on`: `GamepadChange`).
    pub json_callbacks: HashMap<(String, String), String>,
    /// Functions that settle a promise through their last two callbacks (`SceneManager.loadSceneAsync`):
    /// (owner namespace or class, name, owner is a class, is a method).
    pub promises: Vec<(String, String, bool, bool)>,
    /// Functions called with one object of all their parameters (`#[options(.., object)]`,
    /// `Mesh.createFlat({ positions, triangles })`): (owner namespace or class, name, owner is a
    /// class, is a method).
    pub object_args: Vec<(String, String, bool, bool)>,
    /// Namespace functions that are also globals (`#[global]`): (namespace, name).
    pub globals: Vec<(String, String)>,
}

/// An enum of the engine API.
#[derive(Clone, Debug, Default)]
pub struct EnumInfo {
    /// The names, in index order.
    pub values: Vec<String>,
    /// `platypus_<Enum>_name(i, out_len)` and `platypus_<Enum>_parse(s)`.
    pub name_symbol: String,
    pub parse_symbol: String,
}

/// The type of a field of a record.
#[derive(Clone, Debug, PartialEq)]
pub enum RecField {
    Num,
    Bool,
    /// A value class (`Vector3`): its fields in a row.
    Value(String),
    /// An enum: the index.
    Enum(String),
    /// A string: it comes through `ts_api_result_str`, not the number buffer.
    Str,
    /// An optional key (`Option<T>`): a nullable field. Only records that travel as JSON have it.
    Opt(Box<RecField>),
    /// A list of number pairs (`[number, number][]`). JSON only.
    Pairs,
    /// An engine handle (`MeshRenderer`): its id in JSON. JSON only.
    Handle(String),
    /// A list of engine handles. JSON only.
    HandleList(String),
    /// A list of numbers (`number[]`). JSON only.
    Numbers,
    /// A list of records (`BlendChild[]`). JSON only.
    Records(String),
    /// A record inside a record (`Gamepad` in `GamepadChange`). JSON only.
    Record(String),
    /// A dictionary of numbers by name (`Record<string, number>`); a list of pairs in JSON.
    Dict,
}

impl RecField {
    /// Whether the record's numbers can come back flat (no optional field, no list).
    pub fn is_flat(&self) -> bool {
        !matches!(
            self,
            RecField::Opt(_)
                | RecField::Pairs
                | RecField::Handle(_)
                | RecField::HandleList(_)
                | RecField::Numbers
                | RecField::Records(_)
                | RecField::Record(_)
                | RecField::Dict
        )
    }

    fn parse(s: &str) -> Result<RecField, String> {
        Ok(match s {
            "f64" | "f32" | "i32" | "u32" => RecField::Num,
            "bool" => RecField::Bool,
            "string" => RecField::Str,
            "list:pair" => RecField::Pairs,
            "list:f64" => RecField::Numbers,
            "dict:f64" => RecField::Dict,
            other => {
                if let Some(n) = other.strip_prefix("record:") {
                    return Ok(RecField::Record(n.to_string()));
                }
                if let Some(n) = other.strip_prefix("records:") {
                    return Ok(RecField::Records(n.to_string()));
                }
                if let Some(inner) = other.strip_prefix("opt:") {
                    RecField::Opt(Box::new(RecField::parse(inner)?))
                } else if let Some(n) = other
                    .strip_prefix("value:")
                    .or_else(|| other.strip_prefix("like:"))
                {
                    RecField::Value(n.to_string())
                } else if let Some(n) = other.strip_prefix("enum:") {
                    RecField::Enum(n.to_string())
                } else if let Some(n) = other.strip_prefix("list:handle:") {
                    RecField::HandleList(n.to_string())
                } else if let Some(n) = other.strip_prefix("handle:") {
                    RecField::Handle(n.to_string())
                } else {
                    return Err(format!("manifest: unknown record field type `{other}`"));
                }
            }
        })
    }
}

/// A namespace property: its getter and setter (neither takes a receiver).
#[derive(Clone, Debug, Default)]
pub struct NsProp {
    pub getter: Option<ExternFn>,
    pub setter: Option<ExternFn>,
}

fn ext_ty(s: &str) -> Result<ExtTy, String> {
    Ok(match s {
        "void" => ExtTy::Void,
        "bool" => ExtTy::Bool,
        "f64" => ExtTy::F64,
        "f32" => ExtTy::F32,
        "i32" => ExtTy::I32,
        "u32" => ExtTy::U32,
        "string" => ExtTy::Str,
        other => {
            if other == "ptr" {
                ExtTy::Ptr
            } else if other == "f64s" {
                ExtTy::F64Arr
            } else if other == "strings" {
                ExtTy::StrArr
            } else if let Some(n) = other.strip_prefix("values:") {
                ExtTy::ValueArr(n.to_string())
            } else if let Some(n) = other.strip_prefix("handles:") {
                ExtTy::HandleArr(n.to_string())
            } else if let Some(inner) = other.strip_prefix("fn:") {
                ExtTy::Fn(Vec::new(), Box::new(ext_ty(inner)?))
            } else if let Some(rest) = other.strip_prefix("fn(") {
                // `fn(f64,string,handle:Scene):void`
                let (params, ret) = rest
                    .split_once("):")
                    .ok_or_else(|| format!("manifest: bad closure type `{other}`"))?;
                let ps = params
                    .split(',')
                    .filter(|p| !p.is_empty())
                    .map(ext_ty)
                    .collect::<Result<Vec<_>, _>>()?;
                ExtTy::Fn(ps, Box::new(ext_ty(ret)?))
            } else if let Some(name) = other.strip_prefix("enum:") {
                ExtTy::Enum(name.to_string())
            } else if let Some(name) = other.strip_prefix("record:") {
                ExtTy::Record(name.to_string())
            } else if other == "list:string" {
                ExtTy::StrList
            } else if let Some(inner) = other.strip_prefix("opt:") {
                ExtTy::Opt(Box::new(ext_ty(inner)?))
            } else if let Some(name) = other.strip_prefix("records:") {
                ExtTy::RecordsJson(name.to_string())
            } else if other == "list:f64" {
                ExtTy::F64List
            } else if let Some(name) = other.strip_prefix("list:handle:") {
                ExtTy::HandleList(name.to_string())
            } else if let Some(name) = other.strip_prefix("list:enum:") {
                ExtTy::EnumList(name.to_string())
            } else if let Some(name) = other.strip_prefix("list:value:") {
                ExtTy::ValueList(name.to_string())
            } else if let Some(name) = other.strip_prefix("list:record:") {
                ExtTy::RecordList(name.to_string())
            } else if let Some(name) = other.strip_prefix("handle:") {
                ExtTy::Handle(name.to_string())
            } else if let Some(name) = other.strip_prefix("value:") {
                ExtTy::Value(name.to_string())
            } else {
                return Err(format!("manifest: unknown type `{other}`"));
            }
        }
    })
}

impl Manifest {
    /// Adds one namespace's JSON.
    pub fn add_json(&mut self, json: &str) -> Result<(), String> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("manifest: {e}"))?;
        let ns = v["namespace"]
            .as_str()
            .ok_or("manifest: missing `namespace`")?
            .to_string();
        for val in v["values"].as_array().into_iter().flatten() {
            let vname = val["name"]
                .as_str()
                .ok_or("manifest: a value needs a name")?;
            let mut fields = Vec::new();
            for f in val["fields"]
                .as_array()
                .ok_or("manifest: a value needs fields")?
            {
                let fname = f["name"].as_str().ok_or("manifest: a field needs a name")?;
                let fty = ext_ty(f["type"].as_str().ok_or("manifest: a field needs a type")?)?;
                fields.push((fname.to_string(), fty));
            }
            self.values.insert(vname.to_string(), fields);
        }
        for e in v["enums"].as_array().into_iter().flatten() {
            let ename = e["name"].as_str().ok_or("manifest: an enum needs a name")?;
            let values: Vec<String> = e["values"]
                .as_array()
                .ok_or("manifest: an enum needs values")?
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect();
            let info = EnumInfo {
                values,
                name_symbol: e["nameSymbol"].as_str().unwrap_or("").to_string(),
                parse_symbol: e["parseSymbol"].as_str().unwrap_or("").to_string(),
            };
            // `TangentMode.name(i)` and `TangentMode.parse(s)`, callable from scripts.
            let funcs = self.namespaces.entry(ename.to_string()).or_default();
            funcs.insert(
                "name".into(),
                ExternFn {
                    symbol: info.name_symbol.clone(),
                    params: vec![("index".into(), ExtTy::I32)],
                    ret: ExtTy::Str,
                    optional_from: 1,
                    defaults: vec![None],
                    rest: false,
                },
            );
            funcs.insert(
                "parse".into(),
                ExternFn {
                    symbol: info.parse_symbol.clone(),
                    params: vec![("name".into(), ExtTy::Str)],
                    ret: ExtTy::I32,
                    optional_from: 1,
                    defaults: vec![None],
                    rest: false,
                },
            );
            self.enums.insert(ename.to_string(), info);
        }
        for a in v["aliases"].as_array().into_iter().flatten() {
            if let (Some(alias), Some(of)) = (a["name"].as_str(), a["of"].as_str()) {
                self.aliases.insert(alias.to_string(), of.to_string());
            }
        }
        for r in v["records"].as_array().into_iter().flatten() {
            let rname = r["name"]
                .as_str()
                .ok_or("manifest: a record needs a name")?;
            let mut fields = Vec::new();
            for f in r["fields"]
                .as_array()
                .ok_or("manifest: a record needs fields")?
            {
                let fname = f["name"].as_str().ok_or("manifest: a field needs a name")?;
                let fty =
                    RecField::parse(f["type"].as_str().ok_or("manifest: a field needs a type")?)?;
                fields.push((fname.to_string(), fty));
            }
            self.records.insert(rname.to_string(), fields);
        }
        let mut fns = HashMap::new();
        for f in v["functions"]
            .as_array()
            .ok_or("manifest: missing `functions`")?
        {
            let (name, kind, func) = self.parse_fn(f)?;
            if f["promise"].as_bool() == Some(true) {
                self.promises.push((ns.clone(), name.clone(), false, false));
            }
            if f["optionsObject"].as_bool() == Some(true) {
                self.object_args
                    .push((ns.clone(), name.clone(), false, false));
            }
            if f["global"].as_bool() == Some(true) {
                self.globals.push((ns.clone(), name.clone()));
            }
            if let Some(off) = f["subscription"]["off"].as_str() {
                self.subscriptions
                    .insert((ns.clone(), name.clone()), off.to_string());
                if let Some(r) = f["subscription"]["json"].as_str().filter(|r| !r.is_empty()) {
                    self.json_callbacks
                        .insert((ns.clone(), name.clone()), r.to_string());
                }
            }
            if kind == "get" || kind == "set" {
                let p = self
                    .ns_props
                    .entry(ns.clone())
                    .or_default()
                    .entry(name)
                    .or_default();
                if kind == "get" {
                    p.getter = Some(func);
                } else {
                    p.setter = Some(func);
                }
                continue;
            }
            if let Some(prev) = fns.insert(name.clone(), func.clone()) {
                let list = self.overloads.entry((ns.clone(), name)).or_default();
                if list.is_empty() {
                    list.push(prev);
                }
                list.push(func);
            }
        }
        for c in v["classes"].as_array().into_iter().flatten() {
            let cname = c["name"].as_str().ok_or("manifest: a class needs a name")?;
            // A class named like a value class adds methods to the value, not a handle type.
            if !self.values.contains_key(cname) {
                self.handles.insert(cname.to_string());
            }
            let mut class = ClassFns::default();
            for f in c["functions"]
                .as_array()
                .ok_or("manifest: a class needs `functions`")?
            {
                let (name, kind, func) = self.parse_fn(f)?;
                if f["promise"].as_bool() == Some(true) {
                    self.promises
                        .push((cname.to_string(), name.clone(), true, kind == "method"));
                }
                if f["constructor"].as_bool() == Some(true) {
                    class.ctor = Some(func.clone());
                }
                if f["optionsObject"].as_bool() == Some(true) {
                    self.object_args.push((
                        cname.to_string(),
                        name.clone(),
                        true,
                        kind == "method",
                    ));
                }
                // A getter or setter without `this` is a static property of the class (`Window.main`);
                // compiled code reads a static getter like a no-argument static (`ext_static_prop`).
                let has_this = func.params.first().is_some_and(|p| p.0 == "this");
                let (table, table_name) = match kind.as_str() {
                    "static" => (&mut class.statics, "static"),
                    "method" => (&mut class.methods, "method"),
                    "get" if !has_this => (&mut class.statics, "static"),
                    "set" if !has_this => (&mut class.static_setters, "static_set"),
                    "get" => (&mut class.getters, "get"),
                    "set" => (&mut class.setters, "set"),
                    other => return Err(format!("manifest: unknown function kind `{other}`")),
                };
                // A second function of the same name is another version of it.
                if let Some(first) = table.get(&name).cloned() {
                    let list = class
                        .overloads
                        .entry((table_name.to_string(), name))
                        .or_default();
                    if list.is_empty() {
                        list.push(first);
                    }
                    list.push(func);
                } else {
                    table.insert(name, func);
                }
            }
            self.classes.insert(cname.to_string(), class);
        }
        // An enum named like the namespace (`PrimitiveType`) already put `name` / `parse` there.
        self.namespaces.entry(ns).or_default().extend(fns);
        Ok(())
    }

    /// One function entry: its script name, kind (`static` when absent) and signature.
    fn parse_fn(&mut self, f: &serde_json::Value) -> Result<(String, String, ExternFn), String> {
        let name = f["name"]
            .as_str()
            .ok_or("manifest: a function needs a name")?;
        let symbol = f["symbol"]
            .as_str()
            .ok_or("manifest: a function needs a symbol")?
            .to_string();
        let kind = f["kind"].as_str().unwrap_or("static").to_string();
        let mut params = Vec::new();
        let mut optional_from = usize::MAX;
        let mut defaults: Vec<Option<f64>> = Vec::new();
        let mut rest = false;
        for p in f["params"].as_array().ok_or("manifest: missing `params`")? {
            rest |= p["rest"].as_bool() == Some(true);
            let pn = p["name"].as_str().unwrap_or("arg").to_string();
            let pt = ext_ty(
                p["type"]
                    .as_str()
                    .ok_or("manifest: a parameter needs a type")?,
            )?;
            if let ExtTy::Handle(h) = &pt {
                self.handles.insert(h.clone());
            }
            if p["optional"].as_bool() == Some(true) && optional_from > params.len() {
                optional_from = params.len();
            }
            defaults.push(p["default"].as_f64());
            params.push((pn, pt));
        }
        let ret = ext_ty(f["returns"].as_str().ok_or("manifest: missing `returns`")?)?;
        if let ExtTy::Handle(h) = &ret {
            self.handles.insert(h.clone());
        }
        Ok((
            name.to_string(),
            kind,
            ExternFn {
                symbol,
                optional_from: optional_from.min(params.len()),
                defaults,
                params,
                ret,
                rest,
            },
        ))
    }
}
