// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Generates `generated.rs` (into `OUT_DIR`): the plain data of platypus_render_api. A *component* is a
//! per-thread registry of data, a handle class (attach / findOn / gameObject / one property per
//! field) and a scene-file kind. An *asset* (Material, Sprite) is the same without an object:
//! `create()`, `copy()` and an asset kind (inline `$type` in scene files). The specs are in
//! `gen/components.rs`.
//!
//! A field is (ts name, rust name, type, default, doc). Types: f64, i32, bool, string, enum:a|b|c,
//! mask (a layer mask, a number), color, v2, v3, texture, mesh, material, sprite, object (a
//! GameObject). Optional hand-written members of a spec: `extra_fields` / `extra_defaults` (struct
//! fields and their values), `api` (members of the handle class), `items` (module items),
//! `extra_save` (statements; `d` is the data, `m` the property map), `extra_load` (match arms on `k`
//! and `v`), `skip_save` (fields not saved), `save_if_changed` (fields saved only when they differ
//! from the default) and `set_hook` (code run after a field is set).

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The default of a field.
pub enum Dv {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// A string; one that starts with `=` is Rust source.
    Str(&'static str),
    Tuple(&'static [f64]),
}

pub struct Field {
    pub ts: &'static str,
    pub rs: &'static str,
    pub ty: &'static str,
    pub default: Dv,
    pub doc: &'static str,
}

pub struct Component {
    pub name: &'static str,
    pub low: &'static str,
    pub reference: &'static str,
    pub store: &'static str,
    pub data: &'static str,
    pub asset: bool,
    pub shared: bool,
    pub create_name: Option<&'static str>,
    pub fields: &'static [Field],
    pub skip_save: &'static [&'static str],
    pub save_if_changed: &'static [&'static str],
    pub set_hook: &'static [(&'static str, &'static str)],
    pub extra_fields: &'static [&'static str],
    pub extra_defaults: &'static [&'static str],
    pub extra_save: Option<&'static str>,
    pub extra_load: Option<&'static str>,
    pub extra_handles: &'static [&'static str],
    pub extra_records: &'static [&'static str],
    pub extra_value_types: &'static [&'static str],
    pub items: Option<&'static str>,
    pub api: Option<&'static str>,
    pub remove_hook: Option<&'static str>,
    pub awake_hook: Option<&'static str>,
    pub attach_hook: Option<&'static str>,
    pub kind_on_filter: Option<&'static str>,
}

#[path = "gen/components.rs"]
mod components;

/// store type, api get type, getter expr (F = the field, as `d.F`), api set type, set statement,
/// to-json expr, from-json expr (`v: &Value` -> Option<store type>), fallback for a dead handle
struct Ty {
    store: &'static str,
    get: &'static str,
    getter: &'static str,
    set: &'static str,
    set_stmt: &'static str,
    to_json: &'static str,
    from_json: &'static str,
    fallback: &'static str,
}

#[allow(clippy::too_many_arguments)]
const fn ty(
    store: &'static str,
    get: &'static str,
    getter: &'static str,
    set: &'static str,
    set_stmt: &'static str,
    to_json: &'static str,
    from_json: &'static str,
    fallback: &'static str,
) -> Ty {
    Ty {
        store,
        get,
        getter,
        set,
        set_stmt,
        to_json,
        from_json,
        fallback,
    }
}

const STRING: Ty = ty(
    "String",
    "String",
    "d.F.clone()",
    "&str",
    "d.F = v.to_string()",
    "json!(d.F)",
    "v.as_str().map(str::to_string)",
    "String::new()",
);

fn type_of(key: &str) -> Ty {
    match key {
        "f64" => ty(
            "f64",
            "f64",
            "d.F",
            "f64",
            "d.F = v",
            "json!(d.F)",
            "v.as_f64()",
            "0.0",
        ),
        "i32" => ty(
            "i32",
            "f64",
            "f64::from(d.F)",
            "f64",
            "d.F = v as i32",
            "json!(d.F)",
            "v.as_i64().map(|n| n as i32)",
            "0.0",
        ),
        "bool" => ty(
            "bool",
            "bool",
            "d.F",
            "bool",
            "d.F = v",
            "json!(d.F)",
            "v.as_bool()",
            "false",
        ),
        "string" | "enum" => STRING,
        "mask" => ty(
            "u32",
            "f64",
            "f64::from(d.F)",
            "f64",
            "d.F = v as i64 as u32",
            "json!(d.F)",
            "v.as_f64().map(|n| n as i64 as u32)",
            "0.0",
        ),
        "color" => ty(
            "Rgba",
            "Rgba",
            "d.F",
            "Rgba",
            "d.F = v",
            r#"json!({ "$type": "Color", "value": [d.F.r, d.F.g, d.F.b, d.F.a] })"#,
            "color_of(v)",
            "Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }",
        ),
        "v2" => ty(
            "V2",
            "V2",
            "d.F",
            "V2",
            "d.F = v",
            r#"json!({ "$type": "Vector2", "value": [d.F.x, d.F.y] })"#,
            "v2_of(v)",
            "V2 { x: 0.0, y: 0.0 }",
        ),
        "v3" => ty(
            "V3",
            "V3",
            "d.F",
            "V3",
            "d.F = v",
            r#"json!({ "$type": "Vector3", "value": [d.F.x, d.F.y, d.F.z] })"#,
            "v3_of(v)",
            "V3::ZERO",
        ),
        "texture" => ty(
            "u32",
            "TextureRef",
            "TextureRef(d.F)",
            "TextureRef",
            "d.F = v.0",
            r#"asset_ref("Texture", u64::from(d.F))"#,
            r#"asset_handle(v, "Texture").map(|h| h as u32)"#,
            "TextureRef(0)",
        ),
        "mesh" => ty(
            "u32",
            "MeshRef",
            "MeshRef(d.F)",
            "MeshRef",
            "d.F = v.0",
            "mesh_value(d.F)",
            "mesh_of(v)",
            "MeshRef(0)",
        ),
        "material" => ty(
            "u32",
            "MaterialRef",
            "MaterialRef(d.F)",
            "MaterialRef",
            "d.F = v.0",
            r#"asset_ref("Material", u64::from(d.F))"#,
            r#"asset_handle(v, "Material").map(|h| h as u32)"#,
            "MaterialRef(0)",
        ),
        "sprite" => ty(
            "u32",
            "SpriteRef",
            "SpriteRef(d.F)",
            "SpriteRef",
            "d.F = v.0",
            r#"asset_ref("Sprite", u64::from(d.F))"#,
            r#"asset_handle(v, "Sprite").map(|h| h as u32)"#,
            "SpriteRef(0)",
        ),
        "object" => ty(
            "Option<ObjectId>",
            "ObjectId",
            "d.F.unwrap_or_else(none_object)",
            "ObjectId",
            "d.F = (v.to_raw() != 0).then_some(v)",
            "d.F.map_or(Value::Null, |o| object_ref(o.to_raw()))",
            "object_of(v)",
            "none_object()",
        ),
        // A colour that may be none (`Color | null`): `Option<Rgba>`, null in scene files.
        "color?" => ty(
            "Option<Rgba>",
            "Option<Rgba>",
            "d.F",
            "Option<Rgba>",
            "d.F = v",
            r#"d.F.map_or(Value::Null, |c| json!({ "$type": "Color", "value": [c.r, c.g, c.b, c.a] }))"#,
            "if v.is_null() { Some(None) } else { color_of(v).map(Some) }",
            "None",
        ),
        other => panic!("unknown field type `{other}`"),
    }
}

/// Handle-valued properties may be none (`null`): the declared type says so (`Texture | null`).
const NULLABLE_HANDLES: &[&str] = &["texture", "mesh", "material", "sprite", "object"];

fn value_decl(k: &str) -> Option<&'static str> {
    Some(match k {
        "color" => "Color = Rgba [r: f64, g: f64, b: f64, a: f64]",
        "v2" => "Vector2 = V2 [x: f64, y: f64]",
        "v3" => "Vector3 = V3 [x: f64, y: f64, z: f64]",
        "rect" => "Rect = Rect [x: f64, y: f64, width: f64, height: f64]",
        _ => return None,
    })
}

fn handle_decl(k: &str) -> Option<&'static str> {
    Some(match k {
        "texture" => "Texture = TextureRef",
        "mesh" => "Mesh = MeshRef",
        "material" => "Material = MaterialRef",
        "sprite" => "Sprite = SpriteRef",
        _ => return None,
    })
}

/// Python's `repr` of a float.
fn repr(f: f64) -> String {
    let s = format!("{f:?}");
    // Rust prints 1e-5 and 1e16 where Python has 1e-05 and 1e+16.
    match s.split_once('e') {
        Some((m, e)) => {
            let (sign, digits) = match e.strip_prefix('-') {
                Some(d) => ('-', d),
                None => ('+', e),
            };
            format!("{m}e{sign}{digits:0>2}")
        }
        None => s,
    }
}

fn default_lit(t: &str, d: &Dv) -> String {
    if let Dv::Str(s) = d
        && let Some(raw) = s.strip_prefix('=')
    {
        return raw.to_string();
    }
    let nums = |d: &Dv| -> Vec<String> {
        match d {
            Dv::Tuple(v) => v.iter().map(|x| repr(*x)).collect(),
            other => panic!(
                "a tuple default is expected for `{t}`, got {}",
                dv_name(other)
            ),
        }
    };
    match t {
        "color?" => "None".into(),
        "color" => {
            let n = nums(d);
            format!(
                "Rgba {{ r: {}, g: {}, b: {}, a: {} }}",
                n[0], n[1], n[2], n[3]
            )
        }
        "v2" => {
            let n = nums(d);
            format!("V2 {{ x: {}, y: {} }}", n[0], n[1])
        }
        "v3" => {
            let n = nums(d);
            format!("V3 {{ x: {}, y: {}, z: {} }}", n[0], n[1], n[2])
        }
        "texture" | "mesh" | "material" | "sprite" => "0".into(),
        "object" => "None".into(),
        "bool" => match d {
            Dv::Bool(b) => b.to_string(),
            Dv::Int(n) => (*n != 0).to_string(),
            other => panic!("a bool default, got {}", dv_name(other)),
        },
        _ if t == "string" || t.starts_with("enum:") => match d {
            Dv::Str(s) => format!("\"{s}\".to_string()"),
            other => panic!("a string default, got {}", dv_name(other)),
        },
        "mask" | "i32" => match d {
            Dv::Int(n) => n.to_string(),
            other => panic!("an integer default, got {}", dv_name(other)),
        },
        _ => match d {
            Dv::Float(f) => repr(*f),
            Dv::Int(n) => repr(*n as f64),
            other => panic!("a number default, got {}", dv_name(other)),
        },
    }
}

fn dv_name(d: &Dv) -> &'static str {
    match d {
        Dv::None => "none",
        Dv::Bool(_) => "a bool",
        Dv::Int(_) => "an int",
        Dv::Float(_) => "a float",
        Dv::Str(_) => "a string",
        Dv::Tuple(_) => "a tuple",
    }
}

/// `tpl` with every `{key}` replaced.
fn t(tpl: &str, kv: &[(&str, &str)]) -> String {
    let mut s = tpl.to_string();
    for (k, v) in kv {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

fn field_key(ty: &str) -> &str {
    if ty.starts_with("enum:") { "enum" } else { ty }
}

fn gen_component(c: &Component) -> Vec<String> {
    let (name, low, reference, store, data) = (c.name, c.low, c.reference, c.store, c.data);
    let kv: &[(&str, &str)] = &[
        ("name", name),
        ("low", low),
        ("ref", reference),
        ("store", store),
        ("data", data),
    ];
    let asset = c.asset;
    let mut o: Vec<String> = Vec::new();
    let mut kinds: BTreeSet<String> = BTreeSet::new();
    for f in c.fields {
        kinds.insert(if f.ty.starts_with("enum:") {
            "enum".into()
        } else {
            f.ty.trim_end_matches('?').to_string()
        });
    }
    for k in c.extra_value_types {
        kinds.insert(k.to_string());
    }
    let obj = if asset { "" } else { "object: ObjectId" };
    let w = |o: &mut Vec<String>, tpl: &str| o.push(t(tpl, kv));

    w(&mut o, "");
    w(&mut o, "// ---- {name} ----");
    w(&mut o, "");
    w(&mut o, "numbered_handle!(");
    w(
        &mut o,
        "    /// A `{name}` by number (1-based: 0 is \"none\").",
    );
    w(&mut o, "    {ref}");
    w(&mut o, ");");
    w(&mut o, "");
    w(&mut o, "/// The data of one `{name}`.");
    w(&mut o, "#[derive(Clone, Debug, PartialEq)]");
    w(&mut o, "pub struct {data} {");
    if !asset {
        w(&mut o, "    pub object: ObjectId,");
    }
    for f in c.fields {
        o.push(format!(
            "    pub {}: {},",
            f.rs,
            type_of(field_key(f.ty)).store
        ));
    }
    for line in c.extra_fields {
        o.push(format!("    {line}"));
    }
    w(&mut o, "}");
    w(&mut o, "");
    w(&mut o, "impl {data} {");
    o.push(format!("    fn new({obj}) -> Self {{"));
    w(&mut o, "        {data} {");
    if !asset {
        w(&mut o, "            object,");
    }
    for f in c.fields {
        o.push(format!(
            "            {}: {},",
            f.rs,
            default_lit(f.ty, &f.default)
        ));
    }
    for line in c.extra_defaults {
        o.push(format!("            {line}"));
    }
    w(&mut o, "        }");
    w(&mut o, "    }");
    w(&mut o, "}");
    w(&mut o, "");
    w(&mut o, "thread_local! {");
    w(
        &mut o,
        "    static {store}: RefCell<Vec<Option<{data}>>> = const { RefCell::new(Vec::new()) };",
    );
    w(
        &mut o,
        "    static {store}_VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };",
    );
    w(&mut o, "}");
    w(&mut o, "");
    w(
        &mut o,
        "/// Counts changes to the `{name}s` (any mutable access, made or removed): the renderer's",
    );
    w(
        &mut o,
        "/// sync skips the registry while it stays the same.",
    );
    w(&mut o, "pub fn {low}s_version() -> u64 {");
    w(&mut o, "    {store}_VERSION.with(std::cell::Cell::get)");
    w(&mut o, "}");
    w(&mut o, "");
    w(&mut o, "fn bump_{low}s() {");
    w(&mut o, "    {store}_VERSION.with(|c| c.set(c.get() + 1));");
    w(&mut o, "}");
    w(&mut o, "");
    w(
        &mut o,
        "/// Runs `f` on a `{name}`'s data (counts as a change: see [`read_{low}`]).",
    );
    w(
        &mut o,
        "pub fn with_{low}<R>(r: {ref}, f: impl FnOnce(&mut {data}) -> R) -> Option<R> {",
    );
    w(&mut o, "    let out = {store}.with(|v| {");
    w(&mut o, "        let mut v = v.borrow_mut();");
    w(
        &mut o,
        "        v.get_mut((r.0 as usize).checked_sub(1)?)?.as_mut().map(f)",
    );
    w(&mut o, "    });");
    w(&mut o, "    if out.is_some() {");
    w(&mut o, "        bump_{low}s();");
    w(&mut o, "    }");
    w(&mut o, "    out");
    w(&mut o, "}");
    w(&mut o, "");
    w(&mut o, "/// Reads a `{name}`'s data.");
    w(
        &mut o,
        "pub fn read_{low}<R>(r: {ref}, f: impl FnOnce(&{data}) -> R) -> Option<R> {",
    );
    w(&mut o, "    {store}.with(|v| {");
    w(&mut o, "        let v = v.borrow();");
    w(
        &mut o,
        "        v.get((r.0 as usize).checked_sub(1)?)?.as_ref().map(f)",
    );
    w(&mut o, "    })");
    w(&mut o, "}");
    w(&mut o, "");
    w(
        &mut o,
        "/// Runs `f` on every live `{name}` in the order they were made, without copying them (`f`",
    );
    w(&mut o, "/// must not reach the `{name}s` itself).");
    w(
        &mut o,
        "pub fn for_each_{low}(mut f: impl FnMut({ref}, &{data})) {",
    );
    w(&mut o, "    {store}.with(|v| {");
    w(
        &mut o,
        "        for (i, d) in v.borrow().iter().enumerate() {",
    );
    w(&mut o, "            if let Some(d) = d {");
    w(&mut o, "                f({ref}(i as u32 + 1), d);");
    w(&mut o, "            }");
    w(&mut o, "        }");
    w(&mut o, "    })");
    w(&mut o, "}");
    w(&mut o, "");
    o.push(format!(
        "/// The live `{name}s`, in the order they were made{}.",
        if asset { "" } else { " (for the renderer)" }
    ));
    w(&mut o, "pub fn {low}s() -> Vec<({ref}, {data})> {");
    w(&mut o, "    {store}.with(|v| {");
    w(&mut o, "        v.borrow()");
    w(&mut o, "            .iter()");
    w(&mut o, "            .enumerate()");
    w(
        &mut o,
        "            .filter_map(|(i, d)| d.clone().map(|d| ({ref}(i as u32 + 1), d)))",
    );
    w(&mut o, "            .collect()");
    w(&mut o, "    })");
    w(&mut o, "}");
    w(&mut o, "");
    if asset {
        w(&mut o, "pub fn create_{low}() -> {ref} {");
        w(&mut o, "    bump_{low}s();");
        w(&mut o, "    {store}.with(|v| {");
        w(&mut o, "        let mut v = v.borrow_mut();");
        w(&mut o, "        v.push(Some({data}::new()));");
        w(&mut o, "        {ref}(v.len() as u32)");
        w(&mut o, "    })");
        w(&mut o, "}");
        w(&mut o, "");
        w(
            &mut o,
            "/// Frees one (users keep their handle and see the defaults).",
        );
        w(&mut o, "pub fn remove_{low}(r: {ref}) {");
    } else {
        w(
            &mut o,
            "/// The `{name}s` on an object, in the order they were added.",
        );
        w(&mut o, "pub fn {low}s_on(object: ObjectId) -> Vec<{ref}> {");
        w(&mut o, "    let mut on = Vec::new();");
        w(&mut o, "    for_each_{low}(|r, d| {");
        w(&mut o, "        if d.object == object {");
        w(&mut o, "            on.push(r);");
        w(&mut o, "        }");
        w(&mut o, "    });");
        w(&mut o, "    on");
        w(&mut o, "}");
        w(&mut o, "");
        w(&mut o, "pub fn attach_{low}(object: ObjectId) -> {ref} {");
        w(&mut o, "    bump_{low}s();");
        w(&mut o, "    platypus_kinds::components_changed();");
        w(&mut o, "    let r = {store}.with(|v| {");
        w(&mut o, "        let mut v = v.borrow_mut();");
        w(&mut o, "        v.push(Some({data}::new(object)));");
        w(&mut o, "        {ref}(v.len() as u32)");
        w(&mut o, "    });");
        if let Some(h) = c.attach_hook {
            o.push(format!("    {h}"));
        }
        w(&mut o, "    r");
        w(&mut o, "}");
        w(&mut o, "");
        w(&mut o, "pub fn remove_{low}(r: {ref}) {");
        if let Some(h) = c.remove_hook {
            o.push(format!("    {h}"));
        }
    }
    w(&mut o, "    {store}.with(|v| {");
    w(
        &mut o,
        "        if let Some(slot) = v.borrow_mut().get_mut((r.0 as usize).wrapping_sub(1)) {",
    );
    w(&mut o, "            *slot = None;");
    w(&mut o, "        }");
    w(&mut o, "    });");
    w(&mut o, "    bump_{low}s();");
    if !asset {
        w(&mut o, "    platypus_kinds::components_changed();");
    }
    w(&mut o, "}");
    w(&mut o, "");
    if !asset {
        w(&mut o, "/// Removes the `{name}s` of a destroyed object.");
        w(&mut o, "pub fn remove_{low}s_on(object: ObjectId) {");
        w(&mut o, "    for r in {low}s_on(object) {");
        w(&mut o, "        remove_{low}(r);");
        w(&mut o, "    }");
        w(&mut o, "}");
        w(&mut o, "");
    }
    w(&mut o, "pub fn reset_{low}s() {");
    w(&mut o, "    {store}.with(|v| v.borrow_mut().clear());");
    w(&mut o, "    bump_{low}s();");
    w(&mut o, "}");
    if let Some(items) = c.items.filter(|i| !i.is_empty()) {
        w(&mut o, "");
        o.push(items.trim_end().to_string());
    }
    // saving and loading
    w(&mut o, "");
    w(&mut o, "/// The saved properties of a `{name}`.");
    w(
        &mut o,
        "pub fn save_{low}(r: {ref}) -> Map<String, Value> {",
    );
    w(&mut o, "    let mut m = Map::new();");
    if !c.save_if_changed.is_empty() {
        o.push(format!(
            "    let default = {data}::new({});",
            if asset { "" } else { "none_object()" }
        ));
    }
    w(&mut o, "    read_{low}(r, |d| {");
    for f in c.fields {
        if c.skip_save.contains(&f.ts) {
            continue;
        }
        let tj = type_of(field_key(f.ty))
            .to_json
            .replace("d.F", &format!("d.{}", f.rs));
        let mut ins = format!("m.insert(\"{}\".into(), {tj});", f.ts);
        if c.save_if_changed.contains(&f.ts) {
            ins = format!("if d.{} != default.{} {{ {ins} }}", f.rs, f.rs);
        }
        o.push(format!("        {ins}"));
    }
    if let Some(s) = c.extra_save {
        o.push(format!("        {s}"));
    }
    w(&mut o, "    });");
    w(&mut o, "    m");
    w(&mut o, "}");
    w(&mut o, "");
    w(
        &mut o,
        "/// Applies saved properties to a `{name}`; returns warnings.",
    );
    w(
        &mut o,
        "pub fn load_{low}(r: {ref}, props: &Map<String, Value>) -> Vec<String> {",
    );
    w(&mut o, "    let mut warnings = Vec::new();");
    w(&mut o, "    with_{low}(r, |d| {");
    w(&mut o, "        for (k, v) in props {");
    w(&mut o, "            let ok = match k.as_str() {");
    for f in c.fields {
        if c.skip_save.contains(&f.ts) {
            continue;
        }
        if field_key(f.ty) == "enum" {
            let allowed: Vec<&str> = f.ty.split(':').nth(1).unwrap().split('|').collect();
            let list = allowed
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(", ");
            o.push(format!(
                "                \"{}\" => v.as_str().filter(|s| [{list}].contains(s)).map(|s| d.{} = s.to_string()),",
                f.ts, f.rs
            ));
        } else {
            o.push(format!(
                "                \"{}\" => {}.map(|x| d.{} = x),",
                f.ts,
                type_of(field_key(f.ty)).from_json,
                f.rs
            ));
        }
    }
    if let Some(s) = c.extra_load {
        o.push(format!("                {s},"));
    }
    w(&mut o, "                other => {");
    w(
        &mut o,
        "                    warnings.push(format!(\"no property `{other}`; dropped\"));",
    );
    w(&mut o, "                    continue;");
    w(&mut o, "                }");
    w(&mut o, "            };");
    w(&mut o, "            if ok.is_none() {");
    w(
        &mut o,
        "                warnings.push(format!(\"property `{k}` has a value of the wrong shape\"));",
    );
    w(&mut o, "            }");
    w(&mut o, "        }");
    w(&mut o, "    });");
    w(&mut o, "    warnings");
    w(&mut o, "}");
    // the script API
    let values: Vec<&str> = kinds
        .iter()
        .filter(|k| value_decl(k).is_some())
        .map(String::as_str)
        .collect();
    let handles: Vec<&str> = kinds
        .iter()
        .filter(|k| handle_decl(k).is_some_and(|d| d.split(' ').next().unwrap() != name))
        .map(String::as_str)
        .collect();
    let mut hl: Vec<String> = vec![format!("{name} = {reference}")];
    if !asset {
        hl.push("GameObject = ObjectId".into());
    }
    hl.extend(handles.iter().map(|k| handle_decl(k).unwrap().to_string()));
    hl.extend(c.extra_handles.iter().map(|s| s.to_string()));
    let mut seen = BTreeSet::new();
    hl.retain(|h| seen.insert(h.clone()));
    let mut attr = format!("namespace = \"{name}\", handles({})", hl.join(", "));
    if !values.is_empty() {
        attr += &format!(
            ", values({})",
            values
                .iter()
                .map(|k| value_decl(k).unwrap())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !c.extra_records.is_empty() {
        attr += &format!(", records({})", c.extra_records.join(", "));
    }
    w(&mut o, "");
    o.push(format!("#[ts_api({attr})]"));
    w(&mut o, "pub mod {low}_api {");
    w(&mut o, "    use super::*;");
    w(&mut o, "");
    w(&mut o, "    impl {ref} {");
    if asset {
        w(
            &mut o,
            "        /// A new {name} with the defaults (`new {name}({ .. })` sets properties from the object).",
        );
        o.push(format!(
            "        pub fn {}() -> {reference} {{",
            c.create_name.unwrap_or("create")
        ));
        w(&mut o, "            super::create_{low}()");
        w(&mut o, "        }");
        w(&mut o, "");
        w(&mut o, "        /// A copy.");
        w(&mut o, "        pub fn copy(self) -> {ref} {");
        w(&mut o, "            let copy = super::create_{low}();");
        w(
            &mut o,
            "            if let Some(data) = read_{low}(self, |d| d.clone()) {",
        );
        w(&mut o, "                with_{low}(copy, |d| *d = data);");
        w(&mut o, "            }");
        w(&mut o, "            copy");
        w(&mut o, "        }");
        w(&mut o, "");
        w(&mut o, "        /// Frees it.");
        w(&mut o, "        pub fn destroy(self) {");
        w(&mut o, "            super::remove_{low}(self);");
        w(&mut o, "        }");
    } else {
        w(
            &mut o,
            "        /// Adds a `{name}` to a GameObject (what `go.addComponent({name})` does).",
        );
        w(&mut o, "        pub fn attach(object: ObjectId) -> {ref} {");
        w(&mut o, "            super::attach_{low}(object)");
        w(&mut o, "        }");
        w(&mut o, "");
        w(
            &mut o,
            "        /// The first `{name}` on an object, or none.",
        );
        w(
            &mut o,
            "        pub fn find_on(object: ObjectId) -> {ref} {",
        );
        w(
            &mut o,
            "            super::{low}s_on(object).first().copied().unwrap_or({ref}(0))",
        );
        w(&mut o, "        }");
        w(&mut o, "");
        w(&mut o, "        #[property]");
        w(&mut o, "        pub fn game_object(self) -> ObjectId {");
        w(
            &mut o,
            "            read_{low}(self, |d| d.object).unwrap_or_else(none_object)",
        );
        w(&mut o, "        }");
    }
    for f in c.fields {
        let key = field_key(f.ty);
        let ty = type_of(key);
        let rs_plain = f.rs.strip_prefix("r#").unwrap_or(f.rs);
        w(&mut o, "");
        if !f.doc.is_empty() {
            o.push(format!("        /// {}", f.doc));
        }
        w(&mut o, "        #[property]");
        let mut tsu = String::new();
        if key == "enum" {
            let allowed: Vec<&str> = f.ty.split(':').nth(1).unwrap().split('|').collect();
            tsu = allowed
                .iter()
                .map(|a| format!("\\\"{a}\\\""))
                .collect::<Vec<_>>()
                .join(" | ");
            o.push(format!("        #[ts_type = \"{tsu}\"]"));
        } else if NULLABLE_HANDLES.contains(&key) {
            w(&mut o, "        #[nullable]");
        }
        o.push(format!("        pub fn {}(self) -> {} {{", f.rs, ty.get));
        let gx = ty.getter.replace("d.F", &format!("d.{}", f.rs));
        o.push(t(
            &format!(
                "            read_{{low}}(self, |d| {gx}).unwrap_or_else(|| {})",
                ty.fallback
            ),
            kv,
        ));
        w(&mut o, "        }");
        w(&mut o, "");
        if key == "enum" {
            let allowed: Vec<&str> = f.ty.split(':').nth(1).unwrap().split('|').collect();
            o.push(format!(
                "        /// A name other than {} is ignored.",
                allowed
                    .iter()
                    .map(|a| format!("`{a}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            o.push(format!("        #[ts_type = \"{tsu}\"]"));
            o.push(format!("        pub fn set_{rs_plain}(self, v: &str) {{"));
            o.push(format!(
                "            if [{}].contains(&v) {{",
                allowed
                    .iter()
                    .map(|a| format!("\"{a}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            o.push(t(
                &format!(
                    "                with_{{low}}(self, |d| d.{} = v.to_string());",
                    f.rs
                ),
                kv,
            ));
            w(&mut o, "            }");
            w(&mut o, "        }");
        } else {
            if NULLABLE_HANDLES.contains(&key) {
                w(&mut o, "        #[nullable]");
            }
            o.push(format!(
                "        pub fn set_{rs_plain}(self, v: {}) {{",
                ty.set
            ));
            let hook = c
                .set_hook
                .iter()
                .find(|(k, _)| *k == f.ts)
                .map_or("", |(_, h)| *h);
            let stmt = ty.set_stmt.replace("d.F", &format!("d.{}", f.rs));
            o.push(t(
                &format!("            with_{{low}}(self, |d| {{ {stmt}; {hook} }});"),
                kv,
            ));
            w(&mut o, "        }");
        }
    }
    if let Some(api) = c.api.filter(|a| !a.is_empty()) {
        o.push(api.trim_end_matches('\n').to_string());
    }
    w(&mut o, "    }");
    w(&mut o, "}");
    // the kind
    w(&mut o, "");
    if asset {
        let shared = if c.shared { "true" } else { "false" };
        w(&mut o, "fn {low}_asset_kind() -> AssetKind {");
        w(&mut o, "    AssetKind {");
        w(&mut o, "        name: \"{name}\".into(),");
        w(&mut o, "        also: Vec::new(),");
        o.push(format!("        shared: {shared},"));
        w(&mut o, "        create: Box::new(|def| {");
        w(&mut o, "            let r = create_{low}();");
        w(
            &mut o,
            "            let props = def.as_object().cloned().unwrap_or_default();",
        );
        w(&mut o, "            for w in load_{low}(r, &props) {");
        w(&mut o, "                eprintln!(\"{name}: {w}\");");
        w(&mut o, "            }");
        w(&mut o, "            Ok(u64::from(r.0))");
        w(&mut o, "        }),");
        w(&mut o, "        describe: Box::new(|h| {");
        w(&mut o, "            read_{low}({ref}(h as u32), |_| ())?;");
        w(
            &mut o,
            "            Some(Value::Object(save_{low}({ref}(h as u32))))",
        );
        w(&mut o, "        }),");
        w(&mut o, "    }");
        w(&mut o, "}");
    } else {
        w(&mut o, "fn {low}_kind() -> Kind {");
        w(&mut o, "    Kind {");
        w(&mut o, "        name: \"{name}\".into(),");
        w(
            &mut o,
            "        create: Box::new(|o| Ok(u64::from(attach_{low}(ObjectId::from_raw(o)).0))),",
        );
        w(&mut o, "        on: Box::new(|o| {");
        w(&mut o, "            {low}s_on(ObjectId::from_raw(o))");
        w(&mut o, "                .into_iter()");
        if let Some(f) = c.kind_on_filter {
            o.push(format!("                .filter(|r| {f})"));
        }
        w(&mut o, "                .map(|r| u64::from(r.0))");
        w(&mut o, "                .collect()");
        w(&mut o, "        }),");
        w(
            &mut o,
            "        save: Box::new(|h| save_{low}({ref}(h as u32))),",
        );
        w(
            &mut o,
            "        load: Box::new(|h, props| load_{low}({ref}(h as u32), props)),",
        );
        if let Some(h) = c.awake_hook {
            w(&mut o, "        awake: Box::new(|hs| {");
            w(&mut o, "            for h in hs {");
            w(&mut o, "                let r = {ref}(*h as u32);");
            o.push(format!("                {h}"));
            w(&mut o, "            }");
            w(&mut o, "        }),");
        } else {
            w(&mut o, "        awake: Box::new(|_| {}),");
        }
        w(
            &mut o,
            "        object: Box::new(|h| read_{low}({ref}(h as u32), |d| d.object.to_raw())),",
        );
        w(
            &mut o,
            "        destroy: Box::new(|h| remove_{low}({ref}(h as u32))),",
        );
        w(&mut o, "    }");
        w(&mut o, "}");
    }
    o
}

/// A string as a JSON (and Rust raw-string-safe) literal.
fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A field's default as the saved value (JSON text).
fn default_json(f: &Field) -> String {
    let wrap = |t: &str, v: &[f64]| {
        let parts: Vec<String> = v.iter().map(|x| format!("{x:?}")).collect();
        format!("{{\"$type\":\"{t}\",\"value\":[{}]}}", parts.join(","))
    };
    match f.default {
        Dv::None => "null".into(),
        Dv::Bool(b) => b.to_string(),
        Dv::Int(i) => i.to_string(),
        Dv::Float(x) if x.is_finite() => format!("{x:?}"),
        Dv::Float(_) => "null".into(),
        // Rust source: not a value the editor can show before it is saved.
        Dv::Str(s) if s.starts_with('=') => "null".into(),
        Dv::Str(s) => json_str(s),
        Dv::Tuple(t) => match f.ty {
            "color" | "color?" => wrap("Color", t),
            "v2" => wrap("Vector2", t),
            "v3" => wrap("Vector3", t),
            _ => format!(
                "[{}]",
                t.iter()
                    .map(|x| format!("{x:?}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        },
    }
}

/// The `register_fields` list of a component (the editor's Inspector).
fn field_docs(c: &Component) -> String {
    let items: Vec<String> = c
        .fields
        .iter()
        .filter(|f| !c.skip_save.contains(&f.ts))
        .map(|f| {
            format!(
                "platypus_kinds::FieldDoc {{ name: {}.into(), ty: {}.into(), default: serde_json::from_str({}).unwrap_or_default(), doc: {}.into() }}",
                json_str(f.ts),
                json_str(f.ty),
                json_str(&default_json(f)),
                json_str(f.doc)
            )
        })
        .collect();
    format!("vec![{}]", items.join(", "))
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=gen/components.rs");
    let mut out: Vec<String> = Vec::new();
    out.push("// Generated by build.rs from gen/components.rs; do not edit.".into());
    out.push("".into());
    out.push("use super::*;".into());
    for c in components::COMPONENTS {
        out.extend(gen_component(c));
    }
    out.push("".into());
    out.push("/// Registers the scene-file kinds and asset kinds of the generated types.".into());
    out.push("pub fn register_generated_kinds() {".into());
    for c in components::COMPONENTS {
        if c.asset {
            out.push(format!(
                "    platypus_kinds::register_asset({}_asset_kind());",
                c.low
            ));
        } else {
            out.push(format!("    platypus_kinds::register({}_kind());", c.low));
            out.push(format!(
                "    platypus_kinds::register_fields(\"{}\", {});",
                c.name,
                field_docs(c)
            ));
        }
    }
    out.push("}".into());
    out.push("".into());
    out.push("/// The manifests of the generated types.".into());
    out.push("pub fn generated_manifests() -> Vec<String> {".into());
    out.push("    vec![".into());
    for c in components::COMPONENTS {
        out.push(format!("        {}_api::MANIFEST.to_json(),", c.low));
    }
    out.push("    ]".into());
    out.push("}".into());
    out.push("".into());
    out.push("/// Removes the generated components of a destroyed object.".into());
    out.push("pub fn remove_generated_on(object: ObjectId) {".into());
    for c in components::COMPONENTS {
        if !c.asset {
            out.push(format!("    remove_{}s_on(object);", c.low));
        }
    }
    out.push("}".into());
    out.push("".into());
    out.push("/// Forgets every generated component and asset.".into());
    out.push("pub fn reset_generated() {".into());
    for c in components::COMPONENTS {
        out.push(format!("    reset_{}s();", c.low));
    }
    out.push("}".into());
    let dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(dir.join("generated.rs"), out.join("\n") + "\n").expect("write generated.rs");
}
