// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What the editor's Inspector reads and writes: an object's components (registered kinds, then
//! the opaque ones kept from files) and their fields as typed records, set one at a time through
//! the kinds' `load`. Field types and enum options come from the kind's schema when it registered
//! one (`platypus_kinds::register_fields`), else from the saved value's shape.

use crate::data::{Opaque, components_of, opaque_components, remove_opaque, set_opaque};
use platypus_kinds::FieldDoc;
use platypus_scene::ObjectId;
use serde_json::{Map, Value, json};
use ts_api::Handle;

/// One component as the Inspector lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentInfo {
    pub kind: String,
    pub handle: f64,
    /// Kept from a file, its kind not registered here (a game's script): its data is edited as is.
    pub opaque: bool,
}

/// One field: its type, current value and what can be chosen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldInfo {
    pub name: String,
    /// `number`, `bool`, `string`, `enum`, `color`, `vector2`, `vector3`, `vector4`, `object`,
    /// `component`, `asset`, `json`.
    pub ty: String,
    /// The value as the file has it (JSON text): what `setField` takes back.
    pub json: String,
    pub num: f64,
    pub flag: bool,
    pub text: String,
    /// Vectors and colours.
    pub vec: Vec<f64>,
    /// An enum's choices, `|` between them.
    pub options: String,
    pub doc: String,
    /// Shown, not edited (nested data, references kept from a file as data).
    pub read_only: bool,
    /// What a reference may point at: `GameObject`, a component kind (`Rigidbody`; "" for any) or
    /// an asset kind (`Texture`). Empty for other fields.
    pub ref_kind: String,
}

/// The components on an object: registered kinds in registration order, then opaque ones.
pub fn components(object: ObjectId) -> Vec<ComponentInfo> {
    crate::ensure_kinds();
    let mut out: Vec<ComponentInfo> = components_of(object.to_raw())
        .into_iter()
        .map(|(k, h)| ComponentInfo {
            kind: k.name.clone(),
            handle: h as f64,
            opaque: false,
        })
        .collect();
    out.extend(
        opaque_components(object)
            .into_iter()
            .map(|o| ComponentInfo {
                kind: o.ty,
                handle: o.handle as f64,
                opaque: true,
            }),
    );
    out
}

fn opaque_of(object: ObjectId, handle: u64) -> Option<Opaque> {
    opaque_components(object)
        .into_iter()
        .find(|o| o.handle == handle)
}

/// The saved values of a component (its kind's `save`, or an opaque one's `properties`).
fn values(object: ObjectId, kind: &str, handle: u64) -> Option<Map<String, Value>> {
    if let Some(k) = platypus_kinds::get(kind)
        && (k.object)(handle) == Some(object.to_raw())
    {
        return Some((k.save)(handle));
    }
    let o = opaque_of(object, handle)?;
    Some(
        o.data
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
    )
}

fn numbers(v: &Value) -> Vec<f64> {
    v.as_array()
        .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
        .unwrap_or_default()
}

/// A field from its saved value (`Value::Null` when not saved) and the kind's schema entry.
fn field(name: &str, value: &Value, doc: Option<&FieldDoc>) -> FieldInfo {
    let mut f = FieldInfo {
        name: name.to_owned(),
        json: value.to_string(),
        doc: doc.map(|d| d.doc.clone()).unwrap_or_default(),
        ..Default::default()
    };
    let schema = doc.map(|d| d.ty.as_str()).unwrap_or("");
    if let Some(options) = schema.strip_prefix("enum:") {
        f.ty = "enum".into();
        f.options = options.to_owned();
        f.text = value.as_str().unwrap_or_default().to_owned();
        return f;
    }
    match value {
        Value::Bool(b) => {
            f.ty = "bool".into();
            f.flag = *b;
        }
        Value::Number(n) => {
            f.ty = "number".into();
            f.num = n.as_f64().unwrap_or(0.0);
        }
        Value::String(s) => {
            f.ty = "string".into();
            f.text = s.clone();
        }
        // A sound is kept by its file (`{"$type":"Sound","src":path}`): an asset to choose.
        Value::Object(o) if o.get("$type").and_then(Value::as_str) == Some("Sound") => {
            f.ty = "asset".into();
            f.ref_kind = "Sound".into();
            f.text = o
                .get("src")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
        }
        Value::Object(o) if o.get("$type").and_then(Value::as_str).is_some() => {
            let t = o["$type"].as_str().unwrap_or_default();
            f.vec = numbers(&o["value"]);
            f.ty = match t {
                "Color" => "color",
                "Vector2" => "vector2",
                "Vector3" => "vector3",
                "Vector4" | "Quaternion" => "vector4",
                _ => {
                    f.read_only = true;
                    "json"
                }
            }
            .into();
        }
        Value::Object(o) if o.contains_key("$ref") => {
            let object = o["$ref"] == "GameObject";
            f.ty = if object { "object" } else { "component" }.into();
            f.ref_kind = if object {
                "GameObject".into()
            } else {
                o.get("kind")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| schema.strip_prefix("component:").map(str::to_owned))
                    .unwrap_or_default()
            };
            // Live placeholders are edited; one kept from a file as data (a file id) is shown.
            f.read_only = !o.contains_key("object") && !o.contains_key("handle");
            f.text = describe_ref(o);
        }
        Value::Object(o) if o.contains_key("$handle") => {
            f.ty = "asset".into();
            f.ref_kind = o["$handle"].as_str().unwrap_or_default().to_owned();
            f.text = asset_label(
                &f.ref_kind,
                o.get("id").and_then(Value::as_u64).unwrap_or(0),
            );
        }
        Value::Null => {
            // Not saved (a default) or an empty reference: the schema says what it is.
            let asset = asset_kind_of(schema);
            if let Some(kind) = schema.strip_prefix("component:") {
                f.ty = "component".into();
                f.ref_kind = kind.to_owned();
                return f;
            }
            f.ty = match schema {
                "f64" | "i32" | "mask" => "number",
                "bool" => "bool",
                "string" | "str" => "string",
                "color" | "color?" => "color",
                "v2" => "vector2",
                "v3" => "vector3",
                "object" => {
                    f.ref_kind = "GameObject".into();
                    "object"
                }
                _ if asset.is_some() => {
                    f.ref_kind = asset.unwrap_or_default().into();
                    "asset"
                }
                _ => {
                    f.read_only = true;
                    "json"
                }
            }
            .into();
        }
        _ => {
            f.ty = "json".into();
            f.read_only = true;
        }
    }
    if f.ty == "json" {
        f.text = value.to_string();
    }
    f
}

/// The asset kind a schema type names (`texture` -> `Texture`), if it is one.
fn asset_kind_of(schema: &str) -> Option<&'static str> {
    Some(match schema {
        "texture" => "Texture",
        "mesh" => "Mesh",
        "material" => "Material",
        "sprite" => "Sprite",
        "sound" => "Sound",
        "clip" => "AnimationClip",
        "controller" => "AnimationController",
        "spline" => "Spline",
        _ => return None,
    })
}

/// How an asset is shown: its file when it came from one, else its kind and number.
fn asset_label(kind: &str, id: u64) -> String {
    platypus_kinds::get_asset(kind)
        .and_then(|k| (k.describe)(id))
        .and_then(|d| d.get("src").and_then(Value::as_str).map(str::to_owned))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{kind} #{id}"))
}

/// An asset of `kind` made from a project file (`{"type": kind, "src": path}`, as a scene's
/// `assets` table has it): its placeholder.
pub fn asset_from_file(kind: &str, path: &str) -> Result<Value, String> {
    crate::ensure_kinds();
    let k = platypus_kinds::get_asset(kind).ok_or_else(|| format!("no asset kind {kind}"))?;
    let h = (k.create)(&json!({ "type": kind, "src": path }))?;
    Ok(platypus_kinds::asset_ref(&k.name, h))
}

fn describe_ref(o: &Map<String, Value>) -> String {
    let object = match (o.get("object"), o.get("kind"), o.get("handle")) {
        (Some(raw), _, _) => raw.as_u64().unwrap_or(0),
        // A component placeholder names its kind and handle; its object is the kind's to say.
        (None, Some(kind), Some(handle)) => kind
            .as_str()
            .and_then(platypus_kinds::get)
            .and_then(|k| (k.object)(handle.as_u64().unwrap_or(0)))
            .unwrap_or(0),
        _ => 0,
    };
    let name = platypus_scene::api::with_scene(|s| {
        s.name(ObjectId::from_raw(object))
            .map(str::to_owned)
            .unwrap_or_default()
    })
    .unwrap_or_default();
    match o.get("kind").and_then(Value::as_str) {
        Some(kind) => format!("{kind} on {name}"),
        None => name,
    }
}

/// A component's fields: the schema's in its order (with defaults for ones not saved), then any
/// other saved values.
pub fn fields(object: ObjectId, kind: &str, handle: u64) -> Vec<FieldInfo> {
    crate::ensure_kinds();
    let Some(saved) = values(object, kind, handle) else {
        return Vec::new();
    };
    let schema = platypus_kinds::fields_of(kind);
    let mut out: Vec<FieldInfo> = schema
        .iter()
        .map(|d| {
            let v = saved
                .get(&d.name)
                .cloned()
                .unwrap_or_else(|| d.default.clone());
            field(&d.name, &v, Some(d))
        })
        .collect();
    for (k, v) in &saved {
        if !schema.iter().any(|d| &d.name == k) {
            out.push(field(k, v, None));
        }
    }
    out
}

/// Sets one field from JSON text. A vector or colour may be given as a plain array: it takes the
/// saved value's form. Returns why it failed.
pub fn set_field(
    object: ObjectId,
    kind: &str,
    handle: u64,
    name: &str,
    json: &str,
) -> Result<(), String> {
    crate::ensure_kinds();
    let mut value: Value = serde_json::from_str(json).map_err(|e| format!("{name}: {e}"))?;
    let saved = values(object, kind, handle).ok_or("no such component")?;
    if value.is_array()
        && let Some(Value::Object(old)) = saved.get(name)
        && let Some(t) = old.get("$type")
    {
        value = json!({ "$type": t, "value": value });
    }
    if value.is_array()
        && !saved.contains_key(name)
        && let Some(d) = platypus_kinds::fields_of(kind)
            .into_iter()
            .find(|d| d.name == name)
    {
        let t = match d.ty.as_str() {
            "color" | "color?" => Some("Color"),
            "v2" => Some("Vector2"),
            "v3" => Some("Vector3"),
            _ => None,
        };
        if let Some(t) = t {
            value = json!({ "$type": t, "value": value });
        }
    }
    if let Some(k) = platypus_kinds::get(kind)
        && (k.object)(handle) == Some(object.to_raw())
    {
        let mut props = Map::new();
        props.insert(name.to_owned(), value);
        let warnings = (k.load)(handle, &props);
        return match warnings.first() {
            Some(w) => Err(format!("{kind}.{name}: {w}")),
            None => Ok(()),
        };
    }
    let mut o = opaque_of(object, handle).ok_or("no such component")?;
    let props = o
        .data
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(p) = props.as_object_mut() {
        p.insert(name.to_owned(), value);
    }
    set_opaque(object, o);
    Ok(())
}

/// Adds a component of a registered kind (woken at once); its handle.
pub fn add(object: ObjectId, kind: &str) -> Result<u64, String> {
    crate::ensure_kinds();
    let k = platypus_kinds::get(kind).ok_or_else(|| format!("no component kind {kind}"))?;
    let h = (k.create)(object.to_raw())?;
    (k.awake)(&[h]);
    Ok(h)
}

/// Removes a component (a registered one, or an opaque one kept from a file).
pub fn remove(object: ObjectId, kind: &str, handle: u64) -> Result<(), String> {
    crate::ensure_kinds();
    if let Some(k) = platypus_kinds::get(kind)
        && (k.object)(handle) == Some(object.to_raw())
    {
        (k.destroy)(handle);
        return Ok(());
    }
    if remove_opaque(object, handle) {
        Ok(())
    } else {
        Err("no such component".into())
    }
}

/// The kinds a component can be added of, by name.
pub fn kinds() -> Vec<String> {
    crate::ensure_kinds();
    let mut names: Vec<String> = platypus_kinds::all()
        .iter()
        .map(|k| k.name.clone())
        .collect();
    names.sort();
    names
}
