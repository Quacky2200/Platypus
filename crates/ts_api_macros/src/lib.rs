// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `#[ts_api]`: see the `ts_api` crate.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    Expr, FnArg, Ident, Item, ItemMod, Lit, Meta, Pat, ReturnType, Token, Type, Visibility,
    bracketed,
    parse::{Parse, ParseStream, Parser},
    punctuated::Punctuated,
    spanned::Spanned,
};

/// Marks a module whose `pub fn`s are the script API of one namespace.
///
/// ```ignore
/// #[ts_api(namespace = "Time", handles(GameObject = ObjectId),
///            values(Vector3 = Vec3 [x: f32, y: f32, z: f32]))]
/// pub mod time_api {
///     /// Seconds since the last frame.
///     pub fn delta_time() -> f64 { ... }
/// }
/// ```
///
/// Arguments: `namespace = "Name"` (required);
/// `handles(TsName = RustType, ...)` (types that implement `ts_api::Handle`);
/// `values(TsName = RustType [field: scalar, ...], ...)` (types that implement
/// `ts_api::ValueType<N>`; scripts see a class with those fields).
#[proc_macro_attribute]
pub fn ts_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    match expand(attr.into(), item.into()) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `Vector3 = Vec3 [x: f32, y: f32, z: f32]`
struct ValueDecl {
    ts: String,
    rust: Type,
    fields: Vec<(String, String)>,
}

impl Parse for ValueDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ts: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let rust: Type = input.parse()?;
        let content;
        bracketed!(content in input);
        let mut fields = Vec::new();
        while !content.is_empty() {
            let name: Ident = content.parse()?;
            content.parse::<Token![:]>()?;
            let ty: Ident = content.parse()?;
            if !matches!(ty.to_string().as_str(), "f32" | "f64" | "i32" | "u32") {
                return Err(syn::Error::new(
                    ty.span(),
                    "a value field is f32, f64, i32 or u32",
                ));
            }
            fields.push((name.to_string(), ty.to_string()));
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }
        if fields.is_empty() {
            return Err(syn::Error::new(ts.span(), "a value type needs fields"));
        }
        if fields.iter().any(|f| f.1 != fields[0].1) {
            return Err(syn::Error::new(
                ts.span(),
                "all fields of a value type must have the same type",
            ));
        }
        Ok(ValueDecl {
            ts: ts.to_string(),
            rust,
            fields,
        })
    }
}

/// `TangentMode = Tangent [Auto, Bezier, Linear]`: scripts see the variants as strings
/// (`"auto"`), the C ABI the index. A variant may name its string (`Windowed = "Windowed"`).
/// `extern TangentMode = ..` uses an enum another namespace declares: no helper functions or
/// symbols are made here (the module imports the declaring one's helpers).
struct EnumDecl {
    ts: String,
    rust: Type,
    /// The Rust variant names.
    variants: Vec<Ident>,
    /// The script string of a variant, when it is not the variant name with a lower-case first
    /// letter.
    explicit: Vec<Option<String>>,
    /// The `///` comment in front of the declaration.
    doc: String,
    /// Declared by another namespace's `enums(..)`.
    external: bool,
}

impl EnumDecl {
    /// The strings scripts see: the variant names with a lower-case first letter.
    fn names(&self) -> Vec<String> {
        self.variants
            .iter()
            .zip(&self.explicit)
            .map(|(v, e)| {
                if let Some(e) = e {
                    return e.clone();
                }
                let v = v.to_string();
                let mut c = v.chars();
                c.next()
                    .map(|f| f.to_lowercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            })
            .collect()
    }
}

impl Parse for EnumDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let doc = doc_of(&input.call(syn::Attribute::parse_outer)?);
        let external = input.peek(Token![extern]);
        if external {
            input.parse::<Token![extern]>()?;
        }
        let ts: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let rust: Type = input.parse()?;
        let content;
        bracketed!(content in input);
        let mut variants = Vec::new();
        let mut explicit = Vec::new();
        while !content.is_empty() {
            variants.push(content.parse::<Ident>()?);
            if content.peek(Token![=]) {
                content.parse::<Token![=]>()?;
                explicit.push(Some(content.parse::<syn::LitStr>()?.value()));
            } else {
                explicit.push(None);
            }
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }
        if variants.is_empty() {
            return Err(syn::Error::new(ts.span(), "an enum needs variants"));
        }
        Ok(EnumDecl {
            ts: ts.to_string(),
            rust,
            variants,
            explicit,
            doc,
            external,
        })
    }
}

/// `SplineHit = SplineHit [segment: f64, u: f64]`: a Rust struct with those public fields that
/// scripts get as a plain object. A field is `f64`, `bool`, `string`, a declared value class
/// (`<Value>Like` in a declaration that takes a plain object) or enum, `pairs` (a list of number
/// pairs), or an `Option<..>` of those (the key may be left out). `name: T as "key"` names the
/// script key.
struct RecordDecl {
    ts: String,
    rust: Type,
    /// (Rust field name, type text, doc, script key when it is not the camelCase of the name).
    fields: Vec<(Ident, String, String, Option<String>)>,
    /// The `///` comment in front of the declaration.
    doc: String,
}

impl Parse for RecordDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let doc = doc_of(&input.call(syn::Attribute::parse_outer)?);
        let ts: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let rust: Type = input.parse()?;
        let content;
        bracketed!(content in input);
        let mut fields = Vec::new();
        while !content.is_empty() {
            let fdoc = doc_of(&content.call(syn::Attribute::parse_outer)?);
            let name: Ident = content.parse()?;
            content.parse::<Token![:]>()?;
            let ty: Type = content.parse()?;
            let key = if content.peek(Token![as]) {
                content.parse::<Token![as]>()?;
                Some(content.parse::<syn::LitStr>()?.value())
            } else {
                None
            };
            let ty_text: String = quote!(#ty).to_string().split_whitespace().collect();
            fields.push((name, ty_text, fdoc, key));
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }
        if fields.is_empty() {
            return Err(syn::Error::new(ts.span(), "a record needs fields"));
        }
        Ok(RecordDecl {
            ts: ts.to_string(),
            rust,
            fields,
            doc,
        })
    }
}

struct Args {
    namespace: String,
    /// Value parameters are declared as `<Value>Like` in the TypeScript.
    like: bool,
    /// (TypeScript name, Rust type name).
    handles: Vec<(String, String)>,
    values: Vec<ValueDecl>,
    enums: Vec<EnumDecl>,
    records: Vec<RecordDecl>,
    /// `aliases(AnimatorController = AnimationController)`: (alias, target).
    aliases: Vec<(String, String)>,
}

fn rust_name_of(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

fn parse_args(attr: TokenStream2) -> syn::Result<Args> {
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(attr)?;
    let mut args = Args {
        namespace: String::new(),
        like: false,
        handles: Vec::new(),
        values: Vec::new(),
        enums: Vec::new(),
        records: Vec::new(),
        aliases: Vec::new(),
    };
    for m in metas {
        match m {
            Meta::NameValue(nv) if nv.path.is_ident("namespace") => match &nv.value {
                Expr::Lit(l) => match &l.lit {
                    Lit::Str(s) => args.namespace = s.value(),
                    _ => return Err(syn::Error::new(nv.value.span(), "a string is expected")),
                },
                _ => return Err(syn::Error::new(nv.value.span(), "a string is expected")),
            },
            Meta::Path(p) if p.is_ident("like") => args.like = true,
            Meta::List(l) if l.path.is_ident("enums") => {
                let decls =
                    Punctuated::<EnumDecl, Token![,]>::parse_terminated.parse2(l.tokens.clone())?;
                args.enums.extend(decls);
            }
            Meta::List(l) if l.path.is_ident("records") => {
                let decls = Punctuated::<RecordDecl, Token![,]>::parse_terminated
                    .parse2(l.tokens.clone())?;
                args.records.extend(decls);
            }
            Meta::List(l) if l.path.is_ident("aliases") => {
                let pairs = Punctuated::<syn::MetaNameValue, Token![,]>::parse_terminated
                    .parse2(l.tokens.clone())?;
                for nv in pairs {
                    let alias = nv
                        .path
                        .get_ident()
                        .ok_or_else(|| syn::Error::new(nv.path.span(), "an alias name"))?
                        .to_string();
                    let Expr::Path(p) = &nv.value else {
                        return Err(syn::Error::new(nv.value.span(), "the aliased name"));
                    };
                    let target = p
                        .path
                        .get_ident()
                        .ok_or_else(|| syn::Error::new(p.span(), "the aliased name"))?
                        .to_string();
                    args.aliases.push((alias, target));
                }
            }
            Meta::List(l) if l.path.is_ident("handles") => {
                let pairs = Punctuated::<syn::MetaNameValue, Token![,]>::parse_terminated
                    .parse2(l.tokens.clone())?;
                for nv in pairs {
                    let ts = nv
                        .path
                        .get_ident()
                        .ok_or_else(|| syn::Error::new(nv.path.span(), "a type name"))?
                        .to_string();
                    let Expr::Path(p) = &nv.value else {
                        return Err(syn::Error::new(nv.value.span(), "a Rust type name"));
                    };
                    let rust = p
                        .path
                        .segments
                        .last()
                        .ok_or_else(|| syn::Error::new(p.span(), "a Rust type name"))?
                        .ident
                        .to_string();
                    args.handles.push((ts, rust));
                }
            }
            Meta::List(l) if l.path.is_ident("values") => {
                let decls = Punctuated::<ValueDecl, Token![,]>::parse_terminated
                    .parse2(l.tokens.clone())?;
                args.values.extend(decls);
            }
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "expected `namespace = \"..\"`, `like`, `handles(..)`, `values(..)`, `enums(..)`, `records(..)` or `aliases(..)`",
                ));
            }
        }
    }
    if args.namespace.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "`namespace = \"..\"` is required",
        ));
    }
    Ok(args)
}

/// What a parameter or result is.
#[derive(Clone)]
enum Kind {
    Unit,
    Bool,
    F64,
    F32,
    I32,
    U32,
    /// (TypeScript name, the Rust type).
    Handle(String, Box<Type>),
    /// Index into `Args::values`.
    Value(usize),
    /// `&str` (a parameter) or `String` (a result); the flag is true for `String`.
    Str(bool),
    /// `&[f64]`: a script `number[]`, read in place.
    F64s,
    /// `&[String]` (false) or `Vec<String>` (true): a script `string[]`, copied.
    Strs(bool),
    /// `&[V]` for a declared value type (index into `Args::values`): a script `Vector3[]`.
    Values(usize),
    /// `&[H]` for a declared handle type (TypeScript name, the Rust type): a script `GameObject[]`.
    Handles(String, Box<Type>),
    /// `&[R]` or `Vec<R>` of a declared record that cannot travel flat (optional keys, lists,
    /// handles): a script array of objects, JSON text on the wire.
    Records(usize),
    /// `Callback<(A, B)>`: a script closure the engine keeps. The elements are scalar kinds,
    /// strings, or `HandleArg<T>` for a declared handle `T`.
    Callback(Vec<Kind>),
    /// A declared enum (index into `Args::enums`): a script string, the index on the C ABI.
    Enum(usize),
    /// A declared record (index into `Args::records`), a result.
    Record(usize),
    /// `Vec<V>` of a value type, an enum or a record, or `Vec<String>`: a result.
    List(Box<Kind>),
    /// `Option<Enum>`: a name, or `None` for a left-out or unknown one (a parameter).
    OptEnum(usize),
    /// `Option<f64>`, `Option<String>` or `Option<V>` of a value class: `null` for `None`.
    Opt(Box<Kind>),
    /// `Option<R>` of a declared record (index into `Args::records`): a parameter or a result
    /// that travels as JSON text (`null` for `None`).
    OptRecord(usize),
}

fn classify(ty: &Type, args: &Args) -> syn::Result<Kind> {
    if let Type::Tuple(t) = ty
        && t.elems.is_empty()
    {
        return Ok(Kind::Unit);
    }
    if let Type::Reference(r) = ty
        && r.mutability.is_none()
        && rust_name_of(&r.elem).as_deref() == Some("str")
    {
        return Ok(Kind::Str(false));
    }
    if let Type::Reference(r) = ty
        && r.mutability.is_none()
        && let Type::Slice(s) = &*r.elem
    {
        let elem = rust_name_of(&s.elem);
        if elem.as_deref() == Some("f64") {
            return Ok(Kind::F64s);
        }
        if elem.as_deref() == Some("String") {
            return Ok(Kind::Strs(false));
        }
        if let Some(i) = args
            .values
            .iter()
            .position(|v| rust_name_of(&v.rust).as_deref() == elem.as_deref())
        {
            return Ok(Kind::Values(i));
        }
        if let Some((ts, _)) = args
            .handles
            .iter()
            .find(|(_, rust)| Some(rust.as_str()) == elem.as_deref())
        {
            return Ok(Kind::Handles(ts.clone(), Box::new((*s.elem).clone())));
        }
        if let Some(i) = args
            .records
            .iter()
            .position(|r| rust_name_of(&r.rust).as_deref() == elem.as_deref())
        {
            return Ok(Kind::Records(i));
        }
        return Err(syn::Error::new(
            ty.span(),
            "a slice parameter is `&[f64]`, `&[String]` or `&[V]` for a type declared in `values(..)` or `handles(..)`",
        ));
    }
    if let Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
        && seg.ident == "Option"
        && let syn::PathArguments::AngleBracketed(ab) = &seg.arguments
        && let Some(syn::GenericArgument::Type(inner)) = ab.args.first()
    {
        return match classify(inner, args)? {
            Kind::Enum(i) => Ok(Kind::OptEnum(i)),
            k @ (Kind::F64 | Kind::Str(true) | Kind::Value(_)) => Ok(Kind::Opt(Box::new(k))),
            Kind::Record(i) => Ok(Kind::OptRecord(i)),
            _ => Err(syn::Error::new(
                inner.span(),
                "an `Option` holds an enum, `f64`, `String` or a type declared in `values(..)` or `records(..)`",
            )),
        };
    }
    if let Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
        && seg.ident == "Vec"
        && let syn::PathArguments::AngleBracketed(ab) = &seg.arguments
        && let Some(syn::GenericArgument::Type(inner)) = ab.args.first()
    {
        let k = classify(inner, args)?;
        if !matches!(
            k,
            Kind::Value(_)
                | Kind::Record(_)
                | Kind::Str(true)
                | Kind::Enum(_)
                | Kind::Handle(..)
                | Kind::F64
        ) {
            return Err(syn::Error::new(
                inner.span(),
                "a `Vec` result holds `f64`, a `String` or a type declared in `values(..)`, `handles(..)`, `enums(..)` or `records(..)`",
            ));
        }
        if let Kind::Record(i) = k
            && !record_is_flat(args, i)
        {
            return Ok(Kind::Records(i));
        }
        return Ok(Kind::List(Box::new(k)));
    }
    if let Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
        && seg.ident == "Callback"
        && let syn::PathArguments::AngleBracketed(ab) = &seg.arguments
        && let Some(syn::GenericArgument::Type(inner)) = ab.args.first()
    {
        let Type::Tuple(t) = inner else {
            return Err(syn::Error::new(
                inner.span(),
                "a `Callback` takes a tuple of arguments: `Callback<(f64, String)>`",
            ));
        };
        let mut kinds = Vec::new();
        for e in &t.elems {
            let k = if let Type::Path(ep) = e
                && let Some(es) = ep.path.segments.last()
                && es.ident == "HandleArg"
                && let syn::PathArguments::AngleBracketed(eab) = &es.arguments
                && let Some(syn::GenericArgument::Type(h)) = eab.args.first()
            {
                classify(h, args)?
            } else {
                classify(e, args)?
            };
            if !matches!(
                k,
                Kind::Bool | Kind::F64 | Kind::I32 | Kind::U32 | Kind::Str(true) | Kind::Handle(..)
            ) {
                return Err(syn::Error::new(
                    e.span(),
                    "a callback argument is f64, i32, u32, bool, String or HandleArg<T>",
                ));
            }
            kinds.push(k);
        }
        return Ok(Kind::Callback(kinds));
    }
    let Some(name) = rust_name_of(ty) else {
        return Err(syn::Error::new(
            ty.span(),
            "unsupported type in a ts_api function",
        ));
    };
    Ok(match name.as_str() {
        "bool" => Kind::Bool,
        "f64" => Kind::F64,
        "f32" => Kind::F32,
        "i32" => Kind::I32,
        "u32" => Kind::U32,
        "String" => Kind::Str(true),
        other => {
            if let Some((ts, _)) = args.handles.iter().find(|(_, rust)| rust == other) {
                Kind::Handle(ts.clone(), Box::new(ty.clone()))
            } else if let Some(i) = args
                .values
                .iter()
                .position(|v| rust_name_of(&v.rust).as_deref() == Some(other))
            {
                Kind::Value(i)
            } else if let Some(i) = args
                .enums
                .iter()
                .position(|v| rust_name_of(&v.rust).as_deref() == Some(other))
            {
                Kind::Enum(i)
            } else if let Some(i) = args
                .records
                .iter()
                .position(|v| rust_name_of(&v.rust).as_deref() == Some(other))
            {
                Kind::Record(i)
            } else {
                return Err(syn::Error::new(
                    ty.span(),
                    format!(
                        "`{other}` cannot cross the script boundary yet: use f64, f32, i32, u32, bool, () \
                         or a type declared in `handles(..)` or `values(..)`"
                    ),
                ));
            }
        }
    })
}

fn scalar_ty(name: &str) -> TokenStream2 {
    match name {
        "f32" => quote!(f32),
        "f64" => quote!(f64),
        "i32" => quote!(i32),
        _ => quote!(u32),
    }
}

fn scalar_manifest(name: &str) -> TokenStream2 {
    match name {
        "f32" => quote!(::ts_api::Ty::F32),
        "f64" => quote!(::ts_api::Ty::F64),
        "i32" => quote!(::ts_api::Ty::I32),
        _ => quote!(::ts_api::Ty::U32),
    }
}

/// The type of a property for pairing a getter with its setter: a list result and the array
/// parameter that takes it are the same type.
fn pair_ty(k: &Kind, args: &Args) -> String {
    let elem = |e: &Kind| manifest_ty(e, args).to_string();
    match k {
        // A list of records is one type however it travels (flat numbers or JSON).
        Kind::List(inner) if matches!(**inner, Kind::Record(_)) => {
            let Kind::Record(i) = **inner else {
                unreachable!()
            };
            format!("records:{}", args.records[i].ts)
        }
        Kind::Records(i) => format!("records:{}", args.records[*i].ts),
        Kind::List(inner) => format!("[{}]", elem(inner)),
        Kind::F64s => format!("[{}]", elem(&Kind::F64)),
        Kind::Strs(_) => format!("[{}]", elem(&Kind::Str(true))),
        Kind::Values(i) => format!("[{}]", elem(&Kind::Value(*i))),
        Kind::Handles(ts, rust) => format!("[{}]", elem(&Kind::Handle(ts.clone(), rust.clone()))),
        other => elem(other),
    }
}

/// The manifest `Ty` as tokens.
fn manifest_ty(k: &Kind, args: &Args) -> TokenStream2 {
    match k {
        Kind::Unit => quote!(::ts_api::Ty::Void),
        Kind::Bool => quote!(::ts_api::Ty::Bool),
        Kind::F64 => quote!(::ts_api::Ty::F64),
        Kind::F32 => quote!(::ts_api::Ty::F32),
        Kind::I32 => quote!(::ts_api::Ty::I32),
        Kind::U32 => quote!(::ts_api::Ty::U32),
        Kind::Str(_) => quote!(::ts_api::Ty::Str),
        Kind::Handle(ts, _) => quote!(::ts_api::Ty::Handle(#ts)),
        Kind::Value(i) => {
            let ts = &args.values[*i].ts;
            quote!(::ts_api::Ty::Value(#ts))
        }
        Kind::F64s => quote!(::ts_api::Ty::F64s),
        Kind::Strs(_) => quote!(::ts_api::Ty::Strs),
        Kind::Callback(ks) => {
            let names: Vec<String> = ks
                .iter()
                .map(|k| match k {
                    Kind::Bool => "bool".to_string(),
                    Kind::Str(_) => "string".to_string(),
                    Kind::Handle(ts, _) => format!("handle:{ts}"),
                    _ => "f64".to_string(),
                })
                .collect();
            let sig = format!("fn({}):void", names.join(","));
            let params: Vec<String> = ks
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    let t = match k {
                        Kind::Bool => "boolean".to_string(),
                        Kind::Str(_) => "string".to_string(),
                        Kind::Handle(ts, _) => ts.clone(),
                        _ => "number".to_string(),
                    };
                    format!("{}: {t}", (b'a' + i as u8) as char)
                })
                .collect();
            let ts = format!("({}) => void", params.join(", "));
            quote!(::ts_api::Ty::Callback(#sig, #ts))
        }
        Kind::Values(i) => {
            let ts = &args.values[*i].ts;
            let arr = format!("{ts}[]");
            quote!(::ts_api::Ty::Values(#ts, #arr))
        }
        Kind::Handles(ts, _) => {
            let arr = format!("{ts}[]");
            quote!(::ts_api::Ty::Handles(#ts, #arr))
        }
        Kind::Records(i) => {
            let n = &args.records[*i].ts;
            let arr = format!("{n}[]");
            quote!(::ts_api::Ty::Records(#n, #arr))
        }
        Kind::Enum(i) | Kind::OptEnum(i) => {
            let ts = &args.enums[*i].ts;
            quote!(::ts_api::Ty::Enum(#ts))
        }
        Kind::OptRecord(i) => {
            let name = format!("record:{}", args.records[*i].ts);
            let ts = format!("{} | null", args.records[*i].ts);
            quote!(::ts_api::Ty::Opt(#name, #ts))
        }
        Kind::Opt(inner) => {
            let (name, ts) = match &**inner {
                Kind::F64 => ("f64".to_string(), "number | null".to_string()),
                Kind::Str(_) => ("string".to_string(), "string | null".to_string()),
                Kind::Value(i) => (
                    format!("value:{}", args.values[*i].ts),
                    format!("{} | null", args.values[*i].ts),
                ),
                _ => unreachable!("classify only builds Opt of f64, strings and values"),
            };
            quote!(::ts_api::Ty::Opt(#name, #ts))
        }
        Kind::Record(i) => {
            let ts = &args.records[*i].ts;
            quote!(::ts_api::Ty::Record(#ts))
        }
        Kind::List(inner) => {
            let (name, ts) = match &**inner {
                Kind::Value(i) => (
                    format!("value:{}", args.values[*i].ts),
                    args.values[*i].ts.clone(),
                ),
                Kind::Record(i) => (
                    format!("record:{}", args.records[*i].ts),
                    args.records[*i].ts.clone(),
                ),
                Kind::Str(_) => ("string".to_string(), "string".to_string()),
                Kind::Enum(i) => (
                    format!("enum:{}", args.enums[*i].ts),
                    args.enums[*i].ts.clone(),
                ),
                Kind::Handle(ts, _) => (format!("handle:{ts}"), ts.clone()),
                Kind::F64 => ("f64".to_string(), "number".to_string()),
                _ => {
                    unreachable!("classify only builds lists of strings, values, enums and records")
                }
            };
            let arr = format!("{ts}[]");
            quote!(::ts_api::Ty::List(#name, #arr))
        }
    }
}

/// The C ABI type of a value (a value type is passed as its fields, see `shim_param`).
fn abi_ty(k: &Kind) -> TokenStream2 {
    match k {
        Kind::Unit | Kind::Value(_) | Kind::Record(_) => quote!(()),
        Kind::Callback(_) => quote!(*const u8),
        Kind::Enum(_) | Kind::OptEnum(_) => quote!(i32),
        Kind::OptRecord(_) => quote!(*const u8),
        Kind::List(_) => quote!(u32),
        Kind::Opt(inner) => match &**inner {
            Kind::F64 => quote!(f64),
            Kind::Str(_) => quote!(*const u8),
            _ => quote!(()),
        },
        Kind::Bool => quote!(bool),
        Kind::F64 => quote!(f64),
        Kind::F32 => quote!(f32),
        Kind::I32 => quote!(i32),
        Kind::U32 => quote!(u32),
        Kind::Handle(..) => quote!(u64),
        Kind::Str(_)
        | Kind::F64s
        | Kind::Strs(_)
        | Kind::Values(_)
        | Kind::Handles(..)
        | Kind::Records(_) => {
            quote!(*const u8)
        }
    }
}

/// Turns the ABI value `x` into the Rust parameter type.
fn from_abi(k: &Kind, x: TokenStream2) -> TokenStream2 {
    match k {
        Kind::Handle(_, rust) => quote!(<#rust as ::ts_api::Handle>::from_raw(#x)),
        _ => x,
    }
}

/// One entry of `#[optional(..)]`: a name, and perhaps `= value`.
struct OptItem {
    name: Ident,
    default: Option<f64>,
}

impl syn::parse::Parse for OptItem {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let default = if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            let e: Expr = input.parse()?;
            let (neg, lit) = match &e {
                Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => (true, &*u.expr),
                other => (false, other),
            };
            let Expr::Lit(l) = lit else {
                return Err(syn::Error::new(e.span(), "a number or true / false"));
            };
            let v = match &l.lit {
                Lit::Bool(b) => f64::from(u8::from(b.value)),
                Lit::Int(i) => i.base10_parse::<f64>()?,
                Lit::Float(f) => f.base10_parse::<f64>()?,
                other => return Err(syn::Error::new(other.span(), "a number or true / false")),
            };
            Some(if neg { -v } else { v })
        } else {
            None
        };
        Ok(OptItem { name, default })
    }
}

/// `tangent_mode` for `TangentMode`.
fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The helper function `<snake>_<what>` generated for an enum (`parse`, `name`, `index`,
/// `from_index`).
fn enum_fn(ts: &str, what: &str) -> Ident {
    format_ident!("{}_{}", snake(ts), what)
}

/// The first variant of an enum: what a left-out or unknown value is.
fn enum_first(e: &EnumDecl) -> TokenStream2 {
    let rust = &e.rust;
    let v = &e.variants[0];
    quote!(#rust::#v)
}

fn to_abi(k: &Kind, x: TokenStream2, args: &Args) -> TokenStream2 {
    match k {
        Kind::Handle(..) => quote!(::ts_api::Handle::to_raw(#x)),
        Kind::Enum(i) => {
            let index = enum_fn(&args.enums[*i].ts, "index");
            quote!(#index(#x))
        }
        Kind::Opt(inner) if matches!(**inner, Kind::F64) => quote!(#x.unwrap_or(f64::NAN)),
        _ => x,
    }
}

fn camel(s: &str) -> String {
    let s = s.strip_prefix("r#").unwrap_or(s);
    let mut out = String::new();
    let mut up = false;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The attributes this macro reads from a function (they are removed from the output).
fn is_ours(a: &syn::Attribute) -> bool {
    [
        "property",
        "optional",
        "method",
        "getter",
        "js_name",
        "options",
        "rest",
        "nullable",
        "ts_type",
        "like",
        "subscription",
        "promise",
        "ts_param",
        "global",
        "setter",
        "constructor",
    ]
    .iter()
    .any(|n| a.path().is_ident(n))
}

/// `#[js_name = "graphicsDeviceVendorID"]`: the script name when `snake_case` to `camelCase` does
/// not spell it.
fn js_name_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|a| {
        if a.path().is_ident("js_name")
            && let Meta::NameValue(nv) = &a.meta
            && let Expr::Lit(l) = &nv.value
            && let Lit::Str(s) = &l.lit
        {
            Some(s.value())
        } else {
            None
        }
    })
}

/// `#[ts_type = "\"low\" | \"high\""]`: the TypeScript type of a property when the Rust type
/// (`String`) is wider (a string-literal union).
fn ts_type_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|a| {
        if a.path().is_ident("ts_type")
            && let Meta::NameValue(nv) = &a.meta
            && let Expr::Lit(l) = &nv.value
            && let Lit::Str(s) = &l.lit
        {
            Some(s.value())
        } else {
            None
        }
    })
}

fn doc_of(attrs: &[syn::Attribute]) -> String {
    let mut lines = Vec::new();
    for a in attrs {
        if a.path().is_ident("doc")
            && let Meta::NameValue(nv) = &a.meta
            && let Expr::Lit(l) = &nv.value
            && let Lit::Str(s) = &l.lit
        {
            lines.push(s.value().trim().to_string());
        }
    }
    lines.join("\n")
}

/// How a function is called from a script.
#[derive(Clone, Copy, PartialEq)]
enum Tag {
    Static,
    Method,
    Get,
    Set,
}

struct Api {
    /// The Rust function.
    name: Ident,
    doc: String,
    /// The script name (for a property, the property's).
    js_name: String,
    /// camelCase of the Rust name: the symbol's last part.
    stem: String,
    /// The handle class (TypeScript name, Rust type) of an `impl` function.
    class: Option<(String, Type)>,
    tag: Tag,
    /// An `impl` function that takes `self` (a static of the class has none).
    receiver: bool,
    /// A method of a value class: the receiver (first parameter) travels as the value's fields.
    value_recv: Option<(Ident, Kind)>,
    /// Not including the receiver.
    params: Vec<(Ident, Kind)>,
    /// Names of the trailing parameters a call may leave out (`#[optional(a, b = 1.5)]`).
    optional: Vec<String>,
    /// The defaults given (`b = 1.5`; a flag is 0 or 1).
    defaults: Vec<(String, f64)>,
    /// `#[options(after = p)]`: the object comes right after parameter `p` and fills the ones
    /// after it (`new Spline(points, { closed: true })`).
    options_after: Option<String>,
    /// `#[options(name = "TextureOptions")]`: the declared type of the options object.
    options_name: Option<String>,
    /// `#[options(.., only)]`: only the options form is declared.
    options_only: bool,
    /// `#[options(.., object)]`: one object holds every parameter, the required ones too.
    options_object: bool,
    /// `/// name: text` lines of the doc: the doc of a parameter (the key of an options object).
    param_docs: Vec<(String, String)>,
    /// `#[rest(p)]`: the last parameter takes every remaining argument.
    rest: Option<String>,
    /// `#[nullable]`: the TypeScript type of a handle result (or of the setter's value) is `T | null`.
    nullable: bool,
    /// `#[nullable(undefined)]`: declared `T | undefined` instead of `T | null`.
    undefined: bool,
    /// `#[ts_type = ".."]`: the declared TypeScript type of a property (a string-literal union).
    ts_type: Option<String>,
    /// `#[like]`: the value parameters are declared `<Value>Like`.
    like: bool,
    /// `#[subscription(off = "off_x", ts = "..")]`: the function takes a callback and returns a
    /// token; scripts get a function that stops the listener (`off_x(token)`). (Rust name of the
    /// stopping function, the declared TypeScript type of the callback when it is not the
    /// callback's own.)
    subscription: Option<(String, Option<String>, Option<String>)>,
    /// `#[global]`: a namespace function that is also a global function of the same name.
    global: bool,
    /// `#[constructor]`: the positional `new Class(a, b, ..)` of the class.
    constructor: bool,
    /// `#[setter(prop)]`: a method that is another version of the setter of `prop`.
    setter_of: Option<String>,
    /// `#[promise]`: the last two parameters are the resolve and reject callbacks; scripts call it
    /// without them and get a `Promise` (resolved with the first callback's argument).
    promise: bool,
    /// `#[ts_param(points = "..")]`: the declared TypeScript type of a parameter, when it is wider
    /// than the Rust type's (the runtime accepts more).
    ts_params: Vec<(String, String)>,
    ret: Kind,
}

fn api_of(
    sig: &syn::Signature,
    attrs: &[syn::Attribute],
    args: &Args,
    class: Option<(String, Type)>,
) -> syn::Result<Api> {
    if sig.asyncness.is_some() || !sig.generics.params.is_empty() {
        return Err(syn::Error::new(
            sig.span(),
            "a ts_api function cannot be async or generic",
        ));
    }
    let mut has_receiver = false;
    let mut params = Vec::new();
    for a in &sig.inputs {
        match a {
            FnArg::Receiver(r) => {
                if class.is_none() || r.reference.is_some() || r.mutability.is_some() {
                    return Err(syn::Error::new(
                        r.span(),
                        "a class function takes `self` by value (the handle is Copy)",
                    ));
                }
                has_receiver = true;
            }
            FnArg::Typed(pt) => {
                let Pat::Ident(id) = &*pt.pat else {
                    return Err(syn::Error::new(
                        pt.pat.span(),
                        "a plain parameter name is expected",
                    ));
                };
                let k = match classify(&pt.ty, args)? {
                    // `Vec<String>` as a parameter is a `string[]` the function owns.
                    Kind::List(inner) if matches!(*inner, Kind::Str(true)) => Kind::Strs(true),
                    Kind::Opt(inner) if matches!(*inner, Kind::Str(_)) => {
                        return Err(syn::Error::new(
                            pt.ty.span(),
                            "`Option<String>` is only a result (a parameter that may be left out is `#[optional(..)]`)",
                        ));
                    }
                    k => k,
                };
                params.push((id.ident.clone(), k));
            }
        }
    }
    let ret = match &sig.output {
        ReturnType::Default => Kind::Unit,
        ReturnType::Type(_, t) => classify(t, args)?,
    };
    let property = attrs.iter().any(|a| a.path().is_ident("property"));
    let tag = match (has_receiver, property) {
        // A namespace property: `#[property] pub fn color() -> Color` (and `set_color(v)`).
        // A static property of a handle class: `impl Window { #[property] pub fn main() -> Window }`.
        (false, true) if params.is_empty() => Tag::Get,
        (_, true) if !has_receiver || !params.is_empty() => {
            return Err(syn::Error::new(
                sig.span(),
                "#[property] marks a getter: `fn name(self) -> T`",
            ));
        }
        (true, true) => Tag::Get,
        (true, false) => Tag::Method,
        (false, _) => Tag::Static,
    };
    let stem = camel(sig.ident.to_string().trim_start_matches("r#"));
    let mut optional = Vec::new();
    let mut defaults = Vec::new();
    for at in attrs.iter().filter(|a| a.path().is_ident("optional")) {
        let items = at.parse_args_with(Punctuated::<OptItem, Token![,]>::parse_terminated)?;
        for it in items {
            if !params.iter().any(|(p, _)| *p == it.name) {
                return Err(syn::Error::new(it.name.span(), "no such parameter"));
            }
            optional.push(it.name.to_string());
            if let Some(d) = it.default {
                defaults.push((it.name.to_string(), d));
            }
        }
    }
    // Only trailing parameters can be left out (a promise's resolve and reject callbacks follow).
    let promise = attrs.iter().any(|a| a.path().is_ident("promise"));
    let global = attrs.iter().any(|a| a.path().is_ident("global"));
    let constructor = attrs.iter().any(|a| a.path().is_ident("constructor"));
    // `#[setter(start_lifetime)]`: this method is another version of that property's setter.
    let mut setter_of = None;
    for at in attrs.iter().filter(|a| a.path().is_ident("setter")) {
        let id: Ident = at.parse_args()?;
        setter_of = Some(id.to_string());
    }
    let user_len = params.len() - if promise { 2.min(params.len()) } else { 0 };
    let first_opt = params
        .iter()
        .position(|(p, _)| optional.contains(&p.to_string()));
    if let Some(i) = first_opt
        && params[i..user_len.max(i)]
            .iter()
            .any(|(p, _)| !optional.contains(&p.to_string()))
    {
        return Err(syn::Error::new(
            sig.span(),
            "#[optional(..)] parameters must be the last ones",
        ));
    }
    let options = attrs.iter().any(|a| a.path().is_ident("options"));
    let mut options_after = None;
    let mut options_name = None;
    let mut options_only = false;
    let mut options_object = false;
    for at in attrs.iter().filter(|a| a.path().is_ident("options")) {
        if let Meta::List(l) = &at.meta {
            let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(l.tokens.clone())?;
            for m in metas {
                match m {
                    Meta::Path(p) if p.is_ident("only") => options_only = true,
                    Meta::Path(p) if p.is_ident("object") => options_object = true,
                    Meta::NameValue(nv) if nv.path.is_ident("after") => {
                        let Expr::Path(p) = &nv.value else {
                            return Err(syn::Error::new(nv.value.span(), "a parameter name"));
                        };
                        let name = p
                            .path
                            .get_ident()
                            .map(|i| i.to_string())
                            .unwrap_or_default();
                        if !params.iter().any(|(p, _)| *p == name) {
                            return Err(syn::Error::new(nv.value.span(), "no such parameter"));
                        }
                        options_after = Some(name);
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("name") => {
                        let Expr::Lit(l) = &nv.value else {
                            return Err(syn::Error::new(nv.value.span(), "a type name string"));
                        };
                        let Lit::Str(s) = &l.lit else {
                            return Err(syn::Error::new(nv.value.span(), "a type name string"));
                        };
                        options_name = Some(s.value());
                    }
                    other => {
                        return Err(syn::Error::new(
                            other.span(),
                            "expected `after = param`, `name = \"Type\"`, `only` or `object`",
                        ));
                    }
                }
            }
        }
    }
    // `/// title: The title bar text.`: the doc of a parameter (an option key), taken out of the
    // function's own doc.
    let mut doc = doc_of(attrs);
    let mut param_docs = Vec::new();
    if options {
        let mut kept = Vec::new();
        for line in doc.lines() {
            if let Some((name, text)) = line.split_once(": ")
                && params.iter().any(|(p, _)| p == name.trim())
            {
                param_docs.push((name.trim().to_string(), text.trim().to_string()));
            } else {
                kept.push(line);
            }
        }
        doc = kept.join("\n");
    }
    let mut rest = None;
    for at in attrs.iter().filter(|a| a.path().is_ident("rest")) {
        let id: Ident = at.parse_args()?;
        if params.last().is_none_or(|(p, _)| *p != id) {
            return Err(syn::Error::new(
                id.span(),
                "#[rest(..)] names the last parameter",
            ));
        }
        rest = Some(id.to_string());
    }
    if options && optional.is_empty() {
        return Err(syn::Error::new(
            sig.span(),
            "#[options] needs #[optional(..)] parameters to read from the object",
        ));
    }
    if options
        && params
            .iter()
            .any(|(p, k)| optional.contains(&p.to_string()) && matches!(k, Kind::Value(_)))
    {
        return Err(syn::Error::new(
            sig.span(),
            "#[options] cannot have optional value-class parameters (a value is itself a plain object)",
        ));
    }
    if promise {
        let ok = params.len() >= 2
            && matches!(&params[params.len() - 2].1, Kind::Callback(ks) if ks.len() <= 1)
            && matches!(&params[params.len() - 1].1, Kind::Callback(ks) if matches!(ks.as_slice(), [Kind::Str(_)]))
            && matches!(ret, Kind::Unit);
        if !ok {
            return Err(syn::Error::new(
                sig.span(),
                "a promise function ends with `resolve: Callback<(T,)>` and `reject: Callback<(String,)>` and returns nothing",
            ));
        }
    }
    let mut ts_params: Vec<(String, String)> = Vec::new();
    for at in attrs.iter().filter(|a| a.path().is_ident("ts_param")) {
        for nv in
            at.parse_args_with(Punctuated::<syn::MetaNameValue, Token![,]>::parse_terminated)?
        {
            let (Some(name), Expr::Lit(l)) = (nv.path.get_ident(), &nv.value) else {
                return Err(syn::Error::new(
                    nv.span(),
                    "expected `param = \"TypeScript type\"`",
                ));
            };
            let Lit::Str(t) = &l.lit else {
                return Err(syn::Error::new(nv.value.span(), "a TypeScript type string"));
            };
            if !params.iter().any(|(p, _)| p == name) {
                return Err(syn::Error::new(name.span(), "no such parameter"));
            }
            ts_params.push((name.to_string(), t.value()));
        }
    }
    let mut subscription = None;
    for at in attrs.iter().filter(|a| a.path().is_ident("subscription")) {
        let mut off = None;
        let mut ts = None;
        let mut json = None;
        let metas = at.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
        for m in metas {
            match m {
                Meta::NameValue(nv) if nv.path.is_ident("off") => {
                    let Expr::Lit(l) = &nv.value else {
                        return Err(syn::Error::new(
                            nv.value.span(),
                            "the name of the function that stops it",
                        ));
                    };
                    let Lit::Str(s) = &l.lit else {
                        return Err(syn::Error::new(
                            nv.value.span(),
                            "the name of the function that stops it",
                        ));
                    };
                    off = Some(s.value());
                }
                Meta::NameValue(nv) if nv.path.is_ident("ts") => {
                    let Expr::Lit(l) = &nv.value else {
                        return Err(syn::Error::new(nv.value.span(), "a TypeScript type string"));
                    };
                    let Lit::Str(s) = &l.lit else {
                        return Err(syn::Error::new(nv.value.span(), "a TypeScript type string"));
                    };
                    ts = Some(s.value());
                }
                // `json = "GamepadEvent"`: the callback gets a record, which arrives as its JSON
                // text (a string argument).
                Meta::NameValue(nv) if nv.path.is_ident("json") => {
                    let Expr::Lit(l) = &nv.value else {
                        return Err(syn::Error::new(nv.value.span(), "a record name string"));
                    };
                    let Lit::Str(s) = &l.lit else {
                        return Err(syn::Error::new(nv.value.span(), "a record name string"));
                    };
                    json = Some(s.value());
                }
                other => {
                    return Err(syn::Error::new(
                        other.span(),
                        "expected `off = \"stop_fn\"`, `ts = \"(..) => void\"` or `json = \"Record\"`",
                    ));
                }
            }
        }
        let Some(off) = off else {
            return Err(syn::Error::new(
                at.span(),
                "#[subscription(off = \"..\")] names the function that stops it",
            ));
        };
        if !(params
            .last()
            .is_some_and(|p| matches!(p.1, Kind::Callback(_)))
            && matches!(ret, Kind::U32 | Kind::F64))
        {
            return Err(syn::Error::new(
                sig.span(),
                "a subscription ends with a `Callback` and returns its token (`u32`)",
            ));
        }
        subscription = Some((off, ts, json));
    }
    Ok(Api {
        name: sig.ident.clone(),
        doc,
        js_name: js_name_attr(attrs).unwrap_or_else(|| stem.clone()),
        stem,
        class,
        tag,
        receiver: has_receiver,
        value_recv: None,
        params,
        optional,
        defaults,
        options_after,
        options_name,
        options_only,
        options_object,
        param_docs,
        rest,
        nullable: attrs.iter().any(|a| a.path().is_ident("nullable")),
        undefined: attrs.iter().any(|a| {
            a.path().is_ident("nullable")
                && matches!(&a.meta, Meta::List(l) if l.tokens.to_string() == "undefined")
        }),
        ts_type: ts_type_attr(attrs),
        like: attrs.iter().any(|a| a.path().is_ident("like")),
        subscription,
        promise,
        global,
        constructor,
        setter_of,
        ts_params,
        ret,
    })
}

/// `#[method(Vector3)] pub fn add(a: Vec3, b: Vec3) -> Vec3` and `#[getter(Vector3)]`: a free
/// function whose first parameter is the value, seen by scripts as a method or a getter of the
/// value class (`a.add(b)`, `a.magnitude`).
fn value_method(
    sig: &syn::Signature,
    attrs: &[syn::Attribute],
    args: &Args,
) -> syn::Result<Option<Api>> {
    let Some(at) = attrs
        .iter()
        .find(|a| a.path().is_ident("method") || a.path().is_ident("getter"))
    else {
        return Ok(None);
    };
    let getter = at.path().is_ident("getter");
    let ts: Ident = at.parse_args()?;
    let ts = ts.to_string();
    let Some(vi) = args.values.iter().position(|v| v.ts == ts) else {
        return Err(syn::Error::new(
            at.span(),
            format!("`{ts}` is not declared in `values(..)`"),
        ));
    };
    let mut a = api_of(sig, attrs, args, None)?;
    if a.params.is_empty() || !matches!(a.params[0].1, Kind::Value(i) if i == vi) {
        return Err(syn::Error::new(
            sig.span(),
            format!("the first parameter of a `{ts}` method is the `{ts}` itself"),
        ));
    }
    if getter && a.params.len() != 1 {
        return Err(syn::Error::new(
            sig.span(),
            "#[getter(..)] takes only the value: `fn name(v: Vec3) -> T`",
        ));
    }
    a.value_recv = Some(a.params.remove(0));
    a.class = Some((ts, args.values[vi].rust.clone()));
    a.tag = if getter { Tag::Get } else { Tag::Method };
    Ok(Some(a))
}

/// The shim's parameters, and the Rust argument built from them, for one API parameter. A value
/// type is passed as its fields, one scalar each (`p_0, p_1, p_2`).
fn shim_param(name: &Ident, k: &Kind, args: &Args) -> (Vec<TokenStream2>, TokenStream2) {
    match k {
        Kind::Value(i) => {
            let v = &args.values[*i];
            let part = scalar_ty(&v.fields[0].1);
            let n = v.fields.len();
            let names: Vec<Ident> = (0..n).map(|j| format_ident!("{}_{}", name, j)).collect();
            let params = names.iter().map(|p| quote!(#p: #part)).collect();
            let rust = &v.rust;
            let conv = quote! {
                <#rust as ::ts_api::ValueType<#n>>::from_parts([ #( #names as f64 ),* ])
            };
            (params, conv)
        }
        Kind::Enum(i) => {
            let e = &args.enums[*i];
            let from = enum_fn(&e.ts, "from_index");
            let first = enum_first(e);
            (
                vec![quote!(#name: i32)],
                quote!(#from(#name).unwrap_or(#first)),
            )
        }
        Kind::OptEnum(i) => {
            let e = &args.enums[*i];
            let from = enum_fn(&e.ts, "from_index");
            (vec![quote!(#name: i32)], quote!(#from(#name)))
        }
        Kind::OptRecord(i) => {
            let from = format_ident!("__from_json_{}", args.records[*i].ts);
            (
                vec![quote!(#name: *const u8)],
                quote!(unsafe { ::ts_api::script_json_record(#name, #from) }),
            )
        }
        Kind::Opt(inner) => match &**inner {
            Kind::Value(i) => {
                let v = &args.values[*i];
                let part = scalar_ty(&v.fields[0].1);
                let n = v.fields.len();
                let names: Vec<Ident> = (0..n).map(|j| format_ident!("{}_{}", name, j)).collect();
                let params = names.iter().map(|p| quote!(#p: #part)).collect();
                let rust = &v.rust;
                let conv = quote! {
                    {
                        let parts = [ #( #names as f64 ),* ];
                        if parts[0].is_nan() {
                            ::std::option::Option::None
                        } else {
                            ::std::option::Option::Some(
                                <#rust as ::ts_api::ValueType<#n>>::from_parts(parts)
                            )
                        }
                    }
                };
                (params, conv)
            }
            _ => (
                vec![quote!(#name: f64)],
                quote!(if #name.is_nan() {
                    ::std::option::Option::None
                } else {
                    ::std::option::Option::Some(#name)
                }),
            ),
        },
        Kind::Str(owned) => {
            // The script string's object; `script_str` reads it (no copy unless it has a lone
            // surrogate).
            let conv = if *owned {
                quote!(unsafe { ::ts_api::script_str(#name) }.into_owned())
            } else {
                quote!(&*unsafe { ::ts_api::script_str(#name) })
            };
            (vec![quote!(#name: *const u8)], conv)
        }
        Kind::Callback(ks) => {
            let tys: Vec<TokenStream2> = ks
                .iter()
                .map(|k| match k {
                    Kind::Handle(_, rust) => quote!(::ts_api::callback::HandleArg<#rust>),
                    Kind::Str(_) => quote!(::std::string::String),
                    Kind::Bool => quote!(bool),
                    Kind::I32 => quote!(i32),
                    Kind::U32 => quote!(u32),
                    _ => quote!(f64),
                })
                .collect();
            (
                vec![quote!(#name: *const u8)],
                quote!(unsafe { ::ts_api::Callback::<( #( #tys, )* )>::from_script(#name) }),
            )
        }
        Kind::F64s => (
            vec![quote!(#name: *const u8)],
            quote!(unsafe { ::ts_api::script_f64s(#name) }),
        ),
        Kind::Strs(owned) => {
            let read = quote!(unsafe { ::ts_api::script_strs(#name) });
            (
                vec![quote!(#name: *const u8)],
                if *owned { read } else { quote!(&#read) },
            )
        }
        Kind::Values(i) => {
            let v = &args.values[*i];
            let n = v.fields.len();
            let rust = &v.rust;
            (
                vec![quote!(#name: *const u8)],
                quote!(&unsafe { ::ts_api::script_values::<#n, #rust>(#name) }),
            )
        }
        Kind::Records(i) => {
            let from = format_ident!("__from_json_{}", args.records[*i].ts);
            (
                vec![quote!(#name: *const u8)],
                quote!(&unsafe { ::ts_api::script_json_records(#name, #from) }),
            )
        }
        Kind::Handles(_, rust) => (
            vec![quote!(#name: *const u8)],
            quote! {
                &unsafe { ::ts_api::script_handles(#name) }
                    .into_iter()
                    .map(<#rust as ::ts_api::Handle>::from_raw)
                    .collect::<::std::vec::Vec<_>>()
            },
        ),
        other => {
            let t = abi_ty(other);
            (vec![quote!(#name: #t)], from_abi(other, quote!(#name)))
        }
    }
}

// ---- record fields: the types a field may have, and the JSON / JavaScript code for each ----

/// The type of a record field.
#[derive(Clone)]
enum RecTy {
    F64,
    Bool,
    Str,
    /// A declared value class (index into `Args::values`); `true` when written `<Value>Like` (a
    /// plain `{ x, y, z }` object does in the declaration).
    Value(usize, bool),
    /// A declared enum (index into `Args::enums`).
    Enum(usize),
    /// `Option<T>`: the key may be left out (`Option<Option<T>>`: or be `null`).
    Opt(Box<RecTy>),
    /// A list of number pairs (`Vec<(f64, f64)>`), `[number, number][]`.
    Pairs,
    /// A declared handle type (index into `Args::handles`): its id in JSON.
    Handle(usize),
    /// `Vec<H>` of a declared handle type.
    Handles(usize),
    /// `Vec<f64>`: a list of numbers, `number[]`.
    Numbers,
    /// `Vec<R>` of a declared record (index into `Args::records`).
    Records(usize),
    /// A declared record (index into `Args::records`) inside a record.
    Record(usize),
    /// `dict`: a `Vec<(String, f64)>` shown as `Record<string, number>` (a list of
    /// `[name, value]` pairs on the wire, in order).
    Dict,
}

fn parse_rec_ty(s: &str, args: &Args) -> Result<RecTy, String> {
    if let Some(inner) = s.strip_prefix("Option<").and_then(|r| r.strip_suffix('>')) {
        return Ok(RecTy::Opt(Box::new(parse_rec_ty(inner, args)?)));
    }
    Ok(match s {
        "f64" => RecTy::F64,
        "bool" => RecTy::Bool,
        "string" => RecTy::Str,
        "pairs" => RecTy::Pairs,
        "Vec<f64>" => RecTy::Numbers,
        "dict" => RecTy::Dict,
        other
            if other.starts_with("Vec<")
                && args
                    .records
                    .iter()
                    .any(|r| r.ts == other.trim_start_matches("Vec<").trim_end_matches('>')) =>
        {
            let inner = other.trim_start_matches("Vec<").trim_end_matches('>');
            RecTy::Records(args.records.iter().position(|r| r.ts == inner).unwrap())
        }
        other if other.starts_with("Vec<") => {
            let inner = other.trim_start_matches("Vec<").trim_end_matches('>');
            match args.handles.iter().position(|(ts, _)| ts == inner) {
                Some(i) => RecTy::Handles(i),
                None => {
                    return Err(format!(
                        "record list field `{other}` holds a handle type declared in `handles(..)`"
                    ));
                }
            }
        }
        other => {
            if let Some(i) = args.records.iter().position(|r| r.ts == other) {
                RecTy::Record(i)
            } else if let Some(i) = args.values.iter().position(|v| v.ts == other) {
                RecTy::Value(i, false)
            } else if let Some(i) = other
                .strip_suffix("Like")
                .and_then(|b| args.values.iter().position(|v| v.ts == b))
            {
                RecTy::Value(i, true)
            } else if let Some(i) = args.enums.iter().position(|e| e.ts == other) {
                RecTy::Enum(i)
            } else if let Some(i) = args.handles.iter().position(|(ts, _)| ts == other) {
                RecTy::Handle(i)
            } else {
                return Err(format!(
                    "record field type `{other}` is f64, bool, string, pairs, a declared value class (or `<Value>Like`) or enum, or an `Option` of those"
                ));
            }
        }
    })
}

/// Whether a declared record (index into `Args::records`) can be a flat result: every field is a
/// flat type (a field type that does not parse is reported where the record is expanded).
fn record_is_flat(args: &Args, i: usize) -> bool {
    args.records[i]
        .fields
        .iter()
        .all(|f| parse_rec_ty(&f.1, args).is_ok_and(|t| rec_flat(&t)))
}

/// Whether the numbers and strings of a record can travel flat (no optional fields or lists).
fn rec_flat(t: &RecTy) -> bool {
    !matches!(
        t,
        RecTy::Opt(_)
            | RecTy::Pairs
            | RecTy::Handle(_)
            | RecTy::Handles(_)
            | RecTy::Numbers
            | RecTy::Records(_)
            | RecTy::Record(_)
            | RecTy::Dict
    )
}

/// The manifest name of a field type.
fn rec_ty_name(t: &RecTy, args: &Args) -> String {
    match t {
        RecTy::F64 => "f64".into(),
        RecTy::Bool => "bool".into(),
        RecTy::Str => "string".into(),
        RecTy::Value(i, false) => format!("value:{}", args.values[*i].ts),
        RecTy::Value(i, true) => format!("like:{}", args.values[*i].ts),
        RecTy::Enum(i) => format!("enum:{}", args.enums[*i].ts),
        RecTy::Opt(inner) => format!("opt:{}", rec_ty_name(inner, args)),
        RecTy::Pairs => "list:pair".into(),
        RecTy::Numbers => "list:f64".into(),
        RecTy::Records(i) => format!("records:{}", args.records[*i].ts),
        RecTy::Record(i) => format!("record:{}", args.records[*i].ts),
        RecTy::Dict => "dict:f64".into(),
        RecTy::Handle(i) => format!("handle:{}", args.handles[*i].0),
        RecTy::Handles(i) => format!("list:handle:{}", args.handles[*i].0),
    }
}

/// The TypeScript type of a field type.
fn rec_ty_ts(t: &RecTy, args: &Args) -> String {
    match t {
        RecTy::F64 => "number".into(),
        RecTy::Bool => "boolean".into(),
        RecTy::Str => "string".into(),
        RecTy::Value(i, false) => args.values[*i].ts.clone(),
        RecTy::Value(i, true) => format!("{}Like", args.values[*i].ts),
        RecTy::Enum(i) => args.enums[*i].ts.clone(),
        RecTy::Opt(inner) => format!("{} | null", rec_ty_ts(inner, args)),
        RecTy::Pairs => "[number, number][]".into(),
        RecTy::Numbers => "number[]".into(),
        RecTy::Records(i) => format!("{}[]", args.records[*i].ts),
        RecTy::Record(i) => args.records[*i].ts.clone(),
        RecTy::Dict => "Record<string, number>".into(),
        RecTy::Handle(i) => args.handles[*i].0.clone(),
        RecTy::Handles(i) => format!("{}[]", args.handles[*i].0),
    }
}

/// The manifest `Ty` of a field type.
fn rec_ty_manifest(t: &RecTy, args: &Args) -> TokenStream2 {
    match t {
        RecTy::F64 => quote!(::ts_api::Ty::F64),
        RecTy::Bool => quote!(::ts_api::Ty::Bool),
        RecTy::Str => quote!(::ts_api::Ty::Str),
        RecTy::Value(i, false) => {
            let n = &args.values[*i].ts;
            quote!(::ts_api::Ty::Value(#n))
        }
        RecTy::Value(i, true) => {
            let n = &args.values[*i].ts;
            let like = format!("{n}Like");
            quote!(::ts_api::Ty::Like(#n, #like))
        }
        RecTy::Enum(i) => {
            let n = &args.enums[*i].ts;
            quote!(::ts_api::Ty::Enum(#n))
        }
        RecTy::Opt(inner) => {
            let name = rec_ty_name(inner, args);
            let ts = rec_ty_ts(t, args);
            quote!(::ts_api::Ty::Opt(#name, #ts))
        }
        RecTy::Pairs => quote!(::ts_api::Ty::List("pair", "[number, number][]")),
        RecTy::Numbers => quote!(::ts_api::Ty::List("f64", "number[]")),
        RecTy::Records(i) => {
            let n = &args.records[*i].ts;
            let arr = format!("{n}[]");
            quote!(::ts_api::Ty::Records(#n, #arr))
        }
        RecTy::Record(i) => {
            let n = &args.records[*i].ts;
            quote!(::ts_api::Ty::Record(#n))
        }
        RecTy::Dict => quote!(::ts_api::Ty::Dict("f64", "Record<string, number>")),
        RecTy::Handle(i) => {
            let n = &args.handles[*i].0;
            quote!(::ts_api::Ty::Handle(#n))
        }
        RecTy::Handles(i) => {
            let name = format!("handle:{}", args.handles[*i].0);
            let arr = format!("{}[]", args.handles[*i].0);
            quote!(::ts_api::Ty::List(#name, #arr))
        }
    }
}

/// Statements that push a field's numbers to `out` and strings to `strs` (flat types only);
/// `x` is a place of the field's type.
fn rec_flat_stmt(t: &RecTy, x: TokenStream2, args: &Args) -> TokenStream2 {
    match t {
        RecTy::F64 => quote!(out.push(#x);),
        RecTy::Bool => quote!(out.push(if #x { 1.0 } else { 0.0 });),
        RecTy::Str => quote!(strs.push(#x.clone());),
        RecTy::Value(i, _) => {
            let v = &args.values[*i];
            let (rust, n) = (&v.rust, v.fields.len());
            quote!(out.extend_from_slice(&<#rust as ::ts_api::ValueType<#n>>::to_parts(#x));)
        }
        RecTy::Enum(i) => {
            let index = enum_fn(&args.enums[*i].ts, "index");
            quote!(out.push(#index(#x) as f64);)
        }
        RecTy::Opt(_)
        | RecTy::Pairs
        | RecTy::Handle(_)
        | RecTy::Handles(_)
        | RecTy::Numbers
        | RecTy::Records(_)
        | RecTy::Record(_)
        | RecTy::Dict => {
            quote!(compile_error!("not a flat record field");)
        }
    }
}

/// The JSON value of a field of type `t` held by the reference `x` (not optional).
fn rec_json_value(t: &RecTy, x: TokenStream2, args: &Args) -> TokenStream2 {
    match t {
        RecTy::F64 | RecTy::Bool => quote!(::ts_api::serde_json::json!(*#x)),
        RecTy::Str => quote!(::ts_api::serde_json::json!(#x)),
        RecTy::Value(i, _) => {
            let v = &args.values[*i];
            let (rust, n) = (&v.rust, v.fields.len());
            quote!(::ts_api::serde_json::json!(
                <#rust as ::ts_api::ValueType<#n>>::to_parts(*#x).to_vec()
            ))
        }
        RecTy::Enum(i) => {
            let name = enum_fn(&args.enums[*i].ts, "name");
            quote!(::ts_api::serde_json::json!(#name(*#x)))
        }
        RecTy::Numbers => quote!(::ts_api::serde_json::json!(#x.clone())),
        RecTy::Records(i) => {
            let to = format_ident!("__to_json_{}", args.records[*i].ts);
            quote!(::ts_api::serde_json::Value::Array(#x.iter().map(#to).collect()))
        }
        RecTy::Record(i) => {
            let to = format_ident!("__to_json_{}", args.records[*i].ts);
            quote!(#to(#x))
        }
        RecTy::Dict => quote!(::ts_api::serde_json::Value::Array(
            #x.iter()
                .map(|(k, v)| ::ts_api::serde_json::json!([k, v]))
                .collect()
        )),
        RecTy::Pairs => quote!(::ts_api::serde_json::Value::Array(
            #x.iter()
                .map(|p| ::ts_api::serde_json::json!([p.0, p.1]))
                .collect()
        )),
        RecTy::Handle(_) => {
            quote!(::ts_api::serde_json::json!(::ts_api::Handle::to_raw(*#x)))
        }
        RecTy::Handles(_) => quote!(::ts_api::serde_json::Value::Array(
            #x.iter()
                .map(|h| ::ts_api::serde_json::json!(::ts_api::Handle::to_raw(*h)))
                .collect()
        )),
        RecTy::Opt(_) => quote!(compile_error!("nested option")),
    }
}

/// Statements that put a field into the JSON map `m` under `key` (`x` is a reference).
fn rec_json_stmt(t: &RecTy, key: &str, x: TokenStream2, args: &Args) -> TokenStream2 {
    match t {
        RecTy::Opt(inner) => match &**inner {
            RecTy::Opt(inner2) => {
                let v = rec_json_value(inner2, quote!(y), args);
                quote! {
                    match #x {
                        ::std::option::Option::Some(::std::option::Option::Some(y)) => {
                            m.insert(#key.to_string(), #v);
                        }
                        ::std::option::Option::Some(::std::option::Option::None) => {
                            m.insert(#key.to_string(), ::ts_api::serde_json::Value::Null);
                        }
                        ::std::option::Option::None => {}
                    }
                }
            }
            _ => {
                let v = rec_json_value(inner, quote!(y), args);
                quote! {
                    if let ::std::option::Option::Some(y) = #x {
                        m.insert(#key.to_string(), #v);
                    }
                }
            }
        },
        _ => {
            let v = rec_json_value(t, x, args);
            quote!(m.insert(#key.to_string(), #v);)
        }
    }
}

/// An expression: the field's value read from the JSON value `x` (a `&Value`), as a `Result`.
fn rec_parse_value(t: &RecTy, key: &str, x: TokenStream2, args: &Args) -> TokenStream2 {
    let bad = quote!(|| ::std::format!("`{}` has the wrong type", #key));
    match t {
        RecTy::F64 => quote!(#x.as_f64().ok_or_else(#bad)),
        RecTy::Bool => quote!(#x.as_bool().ok_or_else(#bad)),
        RecTy::Str => quote!(#x.as_str().map(::std::string::ToString::to_string).ok_or_else(#bad)),
        RecTy::Value(i, _) => {
            let v = &args.values[*i];
            let (rust, n) = (&v.rust, v.fields.len());
            let names: Vec<&String> = v.fields.iter().map(|(n, _)| n).collect();
            quote! {
                ::ts_api::json_value_parts::<#n>(#x, [ #( #names ),* ])
                    .map(<#rust as ::ts_api::ValueType<#n>>::from_parts)
                    .ok_or_else(#bad)
            }
        }
        RecTy::Enum(i) => {
            let parse = enum_fn(&args.enums[*i].ts, "parse");
            quote!(#x.as_str().and_then(#parse).ok_or_else(#bad))
        }
        RecTy::Pairs => quote!(::ts_api::json_pairs(#x).ok_or_else(#bad)),
        RecTy::Record(i) => {
            let from = format_ident!("__from_json_{}", args.records[*i].ts);
            quote!(#from(#x))
        }
        RecTy::Dict => quote! {
            #x.as_array()
                .and_then(|a| {
                    a.iter()
                        .map(|p| {
                            let pair = p.as_array()?;
                            Some((pair.first()?.as_str()?.to_string(), pair.get(1)?.as_f64()?))
                        })
                        .collect::<::std::option::Option<::std::vec::Vec<(::std::string::String, f64)>>>()
                })
                .ok_or_else(#bad)
        },
        RecTy::Records(i) => {
            let from = format_ident!("__from_json_{}", args.records[*i].ts);
            quote! {
                #x.as_array()
                    .ok_or_else(#bad)
                    .and_then(|a| {
                        a.iter()
                            .map(#from)
                            .collect::<::std::result::Result<::std::vec::Vec<_>, ::std::string::String>>()
                    })
            }
        }
        RecTy::Numbers => quote! {
            #x.as_array()
                .and_then(|a| {
                    a.iter()
                        .map(::ts_api::serde_json::Value::as_f64)
                        .collect::<::std::option::Option<::std::vec::Vec<f64>>>()
                })
                .ok_or_else(#bad)
        },
        RecTy::Handle(i) => {
            let rust = format_ident!("{}", args.handles[*i].1);
            quote!(#x.as_u64().map(<#rust as ::ts_api::Handle>::from_raw).ok_or_else(#bad))
        }
        RecTy::Handles(i) => {
            let rust = format_ident!("{}", args.handles[*i].1);
            quote! {
                #x.as_array()
                    .and_then(|a| {
                        a.iter()
                            .map(|y| y.as_u64().map(<#rust as ::ts_api::Handle>::from_raw))
                            .collect::<::std::option::Option<::std::vec::Vec<_>>>()
                    })
                    .ok_or_else(#bad)
            }
        }
        RecTy::Opt(_) => quote!(compile_error!("nested option")),
    }
}

/// An expression: the field read from the JSON object `obj`, as a `Result`.
fn rec_parse_field(t: &RecTy, key: &str, args: &Args) -> TokenStream2 {
    match t {
        RecTy::Opt(inner) => match &**inner {
            RecTy::Opt(inner2) => {
                let v = rec_parse_value(inner2, key, quote!(x), args);
                quote! {
                    match obj.get(#key) {
                        ::std::option::Option::None => ::std::result::Result::Ok(::std::option::Option::None),
                        ::std::option::Option::Some(x) if x.is_null() => {
                            ::std::result::Result::Ok(::std::option::Option::Some(::std::option::Option::None))
                        }
                        ::std::option::Option::Some(x) => #v.map(|y| ::std::option::Option::Some(::std::option::Option::Some(y))),
                    }
                }
            }
            _ => {
                let v = rec_parse_value(inner, key, quote!(x), args);
                quote! {
                    match obj.get(#key) {
                        ::std::option::Option::None | ::std::option::Option::Some(::ts_api::serde_json::Value::Null) => {
                            ::std::result::Result::Ok(::std::option::Option::None)
                        }
                        ::std::option::Option::Some(x) => #v.map(::std::option::Option::Some),
                    }
                }
            }
        },
        _ => {
            let v = rec_parse_value(t, key, quote!(x), args);
            quote! {
                match obj.get(#key) {
                    ::std::option::Option::Some(x) => #v,
                    ::std::option::Option::None => {
                        ::std::result::Result::Err(::std::format!("`{}` is missing", #key))
                    }
                }
            }
        }
    }
}

fn kind_tokens(t: Tag) -> TokenStream2 {
    match t {
        Tag::Static => quote!(::ts_api::FnKind::Static),
        Tag::Method => quote!(::ts_api::FnKind::Method),
        Tag::Get => quote!(::ts_api::FnKind::Get),
        Tag::Set => quote!(::ts_api::FnKind::Set),
    }
}

fn expand(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let args = parse_args(attr)?;
    let module: ItemMod = syn::parse2(item)?;
    let Some((_, items)) = &module.content else {
        return Err(syn::Error::new(
            module.span(),
            "#[ts_api] needs an inline module: `pub mod name { ... }`",
        ));
    };
    // The module's items as they will be emitted: `#[property]` markers are ours, not Rust's.
    let mut out_items: Vec<Item> = items.clone();
    let mut apis: Vec<Api> = Vec::new();
    // The doc comment of each handle class: the one on its first `impl` block.
    let mut class_docs: std::collections::BTreeMap<String, String> = Default::default();
    for it in out_items.iter_mut() {
        match it {
            Item::Fn(f) if matches!(f.vis, Visibility::Public(_)) => {
                match value_method(&f.sig, &f.attrs, &args)? {
                    Some(a) => {
                        apis.push(a);
                        f.attrs.retain(|a| !is_ours(a));
                    }
                    None => {
                        apis.push(api_of(&f.sig, &f.attrs, &args, None)?);
                        f.attrs.retain(|a| !is_ours(a));
                    }
                }
            }
            Item::Impl(imp) if imp.trait_.is_none() => {
                let Some(rust) = rust_name_of(&imp.self_ty) else {
                    continue;
                };
                let Some((ts, _)) = args.handles.iter().find(|(_, r)| *r == rust) else {
                    continue;
                };
                let class = Some((ts.clone(), (*imp.self_ty).clone()));
                let d = doc_of(&imp.attrs);
                if !d.is_empty() {
                    class_docs.entry(ts.clone()).or_insert(d);
                }
                for ii in imp.items.iter_mut() {
                    if let syn::ImplItem::Fn(f) = ii
                        && matches!(f.vis, Visibility::Public(_))
                    {
                        apis.push(api_of(&f.sig, &f.attrs, &args, class.clone())?);
                        f.attrs.retain(|a| !is_ours(a));
                    }
                }
            }
            _ => {}
        }
    }
    // `set_x(self, v)` next to a `#[property] fn x(self) -> T` is the property's setter.
    // (owner, instance member?, name, type); a static property of a class is not an instance one.
    let getters: Vec<(String, bool, String, String)> = apis
        .iter()
        .filter(|a| a.tag == Tag::Get)
        .map(|a| {
            (
                a.class
                    .as_ref()
                    .map_or_else(|| args.namespace.clone(), |c| c.0.clone()),
                a.receiver || a.value_recv.is_some(),
                a.js_name.clone(),
                pair_ty(&a.ret, &args),
            )
        })
        .collect();
    for a in apis.iter_mut() {
        let class = &a
            .class
            .as_ref()
            .map_or_else(|| args.namespace.clone(), |c| c.0.clone());
        let rust = a.name.to_string();
        if let Some(prop) = &a.setter_of {
            if a.tag != Tag::Method || a.params.len() != 1 {
                return Err(syn::Error::new(
                    a.name.span(),
                    "#[setter(..)] marks a method `fn name(self, value)`",
                ));
            }
            a.tag = Tag::Set;
            a.js_name = camel(prop);
            continue;
        }
        // A method of a handle class, or a plain function of the namespace.
        let settable = if a.class.is_some() && a.receiver {
            a.tag == Tag::Method
        } else {
            a.tag == Tag::Static
        };
        if settable
            && a.value_recv.is_none()
            && let Some(prop) = rust.strip_prefix("set_")
            && a.params.len() == 1
        {
            let prop = camel(prop);
            let ty = pair_ty(&a.params[0].1, &args);
            let instance = a.class.is_some() && a.receiver;
            if getters
                .iter()
                .any(|(c, i, n, t)| c == class && *i == instance && *n == prop && *t == ty)
            {
                a.tag = Tag::Set;
                a.js_name = prop;
            }
        }
    }

    // A static function and a static property of one class must not share a script name.
    for a in apis.iter().filter(|a| a.tag == Tag::Static) {
        let owner = a.class.as_ref().map(|c| &c.0);
        for g in apis.iter().filter(|g| g.tag == Tag::Get && !g.receiver) {
            if g.class.as_ref().map(|c| &c.0) == owner
                && g.value_recv.is_none()
                && g.js_name == a.js_name
            {
                return Err(syn::Error::new(
                    a.name.span(),
                    format!(
                        "`{}` is both a static function and a static property of the same class",
                        a.js_name
                    ),
                ));
            }
        }
    }

    let ns = &args.namespace;
    let handle_names: Vec<&String> = args.handles.iter().map(|(ts, _)| ts).collect();
    let value_defs: Vec<TokenStream2> = args
        .values
        .iter()
        .map(|v| {
            let ts = &v.ts;
            let fields: Vec<TokenStream2> = v
                .fields
                .iter()
                .map(|(n, t)| {
                    let ty = scalar_manifest(t);
                    quote!(::ts_api::ValueField { name: #n, ty: #ty })
                })
                .collect();
            quote!(::ts_api::ValueDef { name: #ts, fields: &[ #( #fields ),* ] })
        })
        .collect();

    let mut ns_entries = Vec::new();
    let mut class_order: Vec<String> = Vec::new();
    let mut class_entries: Vec<Vec<TokenStream2>> = Vec::new();
    let mut shims = Vec::new();
    // `("platypus_X_f", platypus_X_f as usize)` for each shim: the table a JIT resolves imports from.
    let mut symbol_entries: Vec<TokenStream2> = Vec::new();
    for a in &apis {
        let name = &a.name;
        let rust_name = name.to_string().trim_start_matches("r#").to_string();
        let js_name = &a.js_name;
        let owner = a
            .class
            .as_ref()
            .map(|(ts, _)| ts.clone())
            .unwrap_or_else(|| ns.clone());
        let symbol = format!("platypus_{owner}_{}", a.stem);
        let symbol_ident = format_ident!("{}", symbol);
        symbol_entries.push(quote!((#symbol, #symbol_ident as *const () as usize)));
        let doc = &a.doc;
        let has_receiver = a.class.is_some() && a.receiver && a.value_recv.is_none();

        // The manifest entry (a receiver is listed first, as `this`).
        let mut pnames: Vec<String> = Vec::new();
        let mut ptys: Vec<TokenStream2> = Vec::new();
        if has_receiver {
            let ts = &a.class.as_ref().expect("receiver implies a class").0;
            pnames.push("this".into());
            ptys.push(quote!(::ts_api::Ty::Handle(#ts)));
        }
        if let Some((_, k)) = &a.value_recv {
            pnames.push("this".into());
            ptys.push(manifest_ty(k, &args));
        }
        let mut popts: Vec<bool> = vec![false; pnames.len()];
        let mut prests: Vec<bool> = vec![false; pnames.len()];
        let mut pdocs: Vec<String> = vec![String::new(); pnames.len()];
        let mut pdefaults: Vec<TokenStream2> =
            vec![quote!(::std::option::Option::None); pnames.len()];
        let mut pts: Vec<String> = vec![String::new(); pnames.len()];
        for (n, k) in &a.params {
            pnames.push(camel(&n.to_string()));
            ptys.push(manifest_ty(k, &args));
            popts.push(a.optional.contains(&n.to_string()));
            prests.push(a.rest.as_deref() == Some(n.to_string().as_str()));
            pdocs.push(
                a.param_docs
                    .iter()
                    .find(|(p, _)| n == p)
                    .map(|(_, d)| d.clone())
                    .unwrap_or_default(),
            );
            pts.push(
                a.ts_params
                    .iter()
                    .find(|(p, _)| n == p)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default(),
            );
            pdefaults.push(match a.defaults.iter().find(|d| *n == d.0) {
                Some((_, v)) => quote!(::std::option::Option::Some(#v)),
                None => quote!(::std::option::Option::None),
            });
        }
        let ret_m = manifest_ty(&a.ret, &args);
        let kind = kind_tokens(a.tag);
        let nullable = a.nullable;
        let undefined = a.undefined;
        let options_only = a.options_only;
        let options_object = a.options_object;
        let options_name = match &a.options_name {
            Some(n) => quote!(::std::option::Option::Some(#n)),
            None => quote!(::std::option::Option::None),
        };
        let like = a.like;
        let promise = a.promise;
        let global = a.global;
        let constructor = a.constructor;
        let subscription = match &a.subscription {
            Some((off, ts, json)) => {
                let off = camel(off);
                let ts = ts.clone().unwrap_or_default();
                let json = json.clone().unwrap_or_default();
                quote!(::std::option::Option::Some((#off, #ts, #json)))
            }
            None => quote!(::std::option::Option::None),
        };
        let ts_type = match &a.ts_type {
            Some(t) => quote!(::std::option::Option::Some(#t)),
            None => quote!(::std::option::Option::None),
        };
        let options_after = match &a.options_after {
            Some(p) => {
                let i = a
                    .params
                    .iter()
                    .position(|(n, _)| *n == *p)
                    .expect("checked in api_of");
                quote!(::std::option::Option::Some(#i))
            }
            None => quote!(::std::option::Option::None),
        };
        let entry = quote! {
            ::ts_api::Function {
                kind: #kind,
                name: #js_name,
                rust_name: #rust_name,
                symbol: #symbol,
                doc: #doc,
                params: &[ #( ::ts_api::Param { name: #pnames, ty: #ptys, optional: #popts, default: #pdefaults, rest: #prests, doc: #pdocs, ts: #pts } ),* ],
                ret: #ret_m,
                options_after: #options_after,
                nullable: #nullable,
                undefined: #undefined,
                options_name: #options_name,
                options_only: #options_only,
                options_object: #options_object,
                ts_type: #ts_type,
                like: #like,
                subscription: #subscription,
                promise: #promise,
                global: #global,
                constructor: #constructor,
            }
        };
        match &a.class {
            None => ns_entries.push(entry),
            Some((ts, _)) => {
                let i = match class_order.iter().position(|c| c == ts) {
                    Some(i) => i,
                    None => {
                        class_order.push(ts.clone());
                        class_entries.push(Vec::new());
                        class_order.len() - 1
                    }
                };
                class_entries[i].push(entry);
            }
        }

        // The extern "C" shim.
        let mut abi_params: Vec<TokenStream2> = Vec::new();
        let mut call_args: Vec<TokenStream2> = Vec::new();
        let receiver = if has_receiver {
            abi_params.push(quote!(this: u64));
            let (_, rust) = a.class.as_ref().expect("receiver implies a class");
            Some(quote!(<#rust as ::ts_api::Handle>::from_raw(this)))
        } else {
            None
        };
        for (n, k) in a.value_recv.iter().chain(&a.params) {
            let (ps, arg) = shim_param(n, k, &args);
            abi_params.extend(ps);
            call_args.push(arg);
        }
        let call = match (&receiver, &a.class) {
            (None, Some(_)) if a.value_recv.is_some() => quote!(#name( #( #call_args ),* )),
            (Some(r), _) => quote!(#r.#name( #( #call_args ),* )),
            (None, Some((_, rust))) => quote!(<#rust>::#name( #( #call_args ),* )),
            (None, None) => quote!(#name( #( #call_args ),* )),
        };
        if let Kind::Opt(inner) = &a.ret
            && let Kind::Value(i) = &**inner
        {
            // An optional value: NaN in every field is `None`.
            let v = &args.values[*i];
            let part = scalar_ty(&v.fields[0].1);
            let n = v.fields.len();
            let rust = &v.rust;
            let idx: Vec<usize> = (0..n).collect();
            shims.push(quote! {
                /// # Safety
                /// `out` points to room for the value's fields.
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn #symbol_ident( #( #abi_params, )* out: *mut #part ) {
                    let parts = match #call {
                        ::std::option::Option::Some(v) => {
                            <#rust as ::ts_api::ValueType<#n>>::to_parts(v)
                        }
                        ::std::option::Option::None => [f64::NAN; #n],
                    };
                    #( unsafe { *out.add(#idx) = parts[#idx] as #part; } )*
                }
            });
        } else if let Kind::Value(i) = &a.ret {
            // A value result is written through a trailing pointer, one scalar per field.
            let v = &args.values[*i];
            let part = scalar_ty(&v.fields[0].1);
            let n = v.fields.len();
            let rust = &v.rust;
            let idx: Vec<usize> = (0..n).collect();
            shims.push(quote! {
                /// # Safety
                /// `out` points to room for the value's fields.
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn #symbol_ident( #( #abi_params, )* out: *mut #part ) {
                    let parts = <#rust as ::ts_api::ValueType<#n>>::to_parts(#call);
                    #( unsafe { *out.add(#idx) = parts[#idx] as #part; } )*
                }
            });
        } else if let Kind::Record(i) = &a.ret
            && !record_is_flat(&args, *i)
        {
            // A record with lists or optional keys: its JSON text.
            let to = format_ident!("__to_json_{}", args.records[*i].ts);
            shims.push(quote! {
                /// # Safety
                /// `out_len` is valid for a write.
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn #symbol_ident( #( #abi_params, )* out_len: *mut i64 ) -> *const u8 {
                    let text = #to(&#call).to_string();
                    unsafe { ::ts_api::return_string(text, out_len) }
                }
            });
        } else if let Kind::Record(i) = &a.ret {
            // A record result: its numbers stay in `ts_api`'s per-thread buffer.
            let flat = format_ident!("__flat_{}", args.records[*i].ts);
            shims.push(quote! {
                #[unsafe(no_mangle)]
                pub extern "C" fn #symbol_ident( #( #abi_params ),* ) {
                    let r = #call;
                    let mut out = ::std::vec::Vec::new();
                    let mut strs = ::std::vec::Vec::new();
                    #flat(&r, &mut out, &mut strs);
                    ::ts_api::return_flat(out);
                    ::ts_api::return_strings(strs);
                }
            });
        } else if let Kind::List(inner) = &a.ret
            && matches!(**inner, Kind::Str(_))
        {
            // A list of strings: the strings stay in `ts_api`'s per-thread list and the caller
            // reads them with `ts_api_result_str(i)`; the shim returns how many there are.
            let unsafe_kw = if a.params.iter().any(|(_, k)| {
                matches!(
                    k,
                    Kind::Str(_)
                        | Kind::Callback(_)
                        | Kind::F64s
                        | Kind::Strs(_)
                        | Kind::Values(_)
                        | Kind::OptRecord(_)
                        | Kind::Handles(..)
                        | Kind::Records(_)
                )
            }) {
                quote!(unsafe)
            } else {
                quote!()
            };
            shims.push(quote! {
                /// # Safety
                /// Array and string arguments are live script objects (or null).
                #[unsafe(no_mangle)]
                pub #unsafe_kw extern "C" fn #symbol_ident( #( #abi_params ),* ) -> u32 {
                    let items: ::std::vec::Vec<::std::string::String> = #call;
                    let count = items.len() as u32;
                    ::ts_api::return_strings(items);
                    count
                }
            });
        } else if let Kind::List(inner) = &a.ret {
            // A list result: the numbers of every element, one after the other, stay in the
            // buffer; the shim returns how many elements there are.
            let push = match &**inner {
                Kind::Value(i) => {
                    let v = &args.values[*i];
                    let n = v.fields.len();
                    let rust = &v.rust;
                    quote!(out.extend_from_slice(&<#rust as ::ts_api::ValueType<#n>>::to_parts(*item));)
                }
                Kind::Record(i) => {
                    let flat = format_ident!("__flat_{}", args.records[*i].ts);
                    quote!(#flat(item, &mut out, &mut strs);)
                }
                Kind::Enum(i) => {
                    let index = enum_fn(&args.enums[*i].ts, "index");
                    quote!(out.push(#index(*item) as f64);)
                }
                Kind::Handle(..) => quote!(ids.push(::ts_api::Handle::to_raw(*item));),
                Kind::F64 => quote!(out.push(*item);),
                _ => unreachable!("classify only builds lists of values, enums and records"),
            };
            let unsafe_kw = if a.params.iter().any(|(_, k)| {
                matches!(
                    k,
                    Kind::Str(_)
                        | Kind::Callback(_)
                        | Kind::F64s
                        | Kind::Strs(_)
                        | Kind::Values(_)
                        | Kind::OptRecord(_)
                        | Kind::Handles(..)
                        | Kind::Records(_)
                )
            }) {
                quote!(unsafe)
            } else {
                quote!()
            };
            shims.push(quote! {
                /// # Safety
                /// Array and string arguments are live script objects (or null).
                #[unsafe(no_mangle)]
                pub #unsafe_kw extern "C" fn #symbol_ident( #( #abi_params ),* ) -> u32 {
                    let items = #call;
                    let mut out = ::std::vec::Vec::new();
                    let mut strs: ::std::vec::Vec<::std::string::String> = ::std::vec::Vec::new();
                    #[allow(unused_mut)]
                    let mut ids: ::std::vec::Vec<u64> = ::std::vec::Vec::new();
                    for item in &items {
                        #push
                    }
                    ::ts_api::return_flat(out);
                    ::ts_api::return_strings(strs);
                    ::ts_api::return_ids(ids);
                    items.len() as u32
                }
            });
        } else if let Kind::Records(i) = &a.ret {
            // A list of records that cannot travel flat: JSON text.
            let to = format_ident!("__to_json_{}", args.records[*i].ts);
            shims.push(quote! {
                /// # Safety
                /// `out_len` is valid for a write; string and array arguments are live script objects.
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn #symbol_ident( #( #abi_params, )* out_len: *mut i64 ) -> *const u8 {
                    let items = #call;
                    let text = ::ts_api::serde_json::Value::Array(items.iter().map(#to).collect()).to_string();
                    unsafe { ::ts_api::return_string(text, out_len) }
                }
            });
        } else if let Kind::OptRecord(i) = &a.ret {
            // An optional record result: its JSON text (`null` for none).
            let to = format_ident!("__to_json_{}", args.records[*i].ts);
            shims.push(quote! {
                /// # Safety
                /// `out_len` is valid for a write.
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn #symbol_ident( #( #abi_params, )* out_len: *mut i64 ) -> *const u8 {
                    let text = match #call {
                        ::std::option::Option::Some(r) => #to(&r).to_string(),
                        ::std::option::Option::None => ::std::string::String::from("null"),
                    };
                    unsafe { ::ts_api::return_string(text, out_len) }
                }
            });
        } else if let Kind::Opt(inner) = &a.ret
            && matches!(**inner, Kind::Str(_))
        {
            // An optional string: `None` is an empty text and a flag the caller reads.
            shims.push(quote! {
                /// # Safety
                /// `out_len` is valid for a write.
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn #symbol_ident( #( #abi_params, )* out_len: *mut i64 ) -> *const u8 {
                    unsafe { ::ts_api::return_string_opt(#call, out_len) }
                }
            });
        } else if let Kind::Str(_) = &a.ret {
            // A string result: the text stays in `ts_api`'s per-thread buffer and the caller
            // copies it into a script string at once.
            shims.push(quote! {
                /// # Safety
                /// `out_len` is valid for a write.
                #[unsafe(no_mangle)]
                pub unsafe extern "C" fn #symbol_ident( #( #abi_params, )* out_len: *mut i64 ) -> *const u8 {
                    unsafe { ::ts_api::return_string(#call, out_len) }
                }
            });
        } else {
            let abi_ret = abi_ty(&a.ret);
            let body = to_abi(&a.ret, call, &args);
            // A string parameter is a pointer to a script string: the shim is unsafe to call.
            if a.params.iter().any(|(_, k)| {
                matches!(
                    k,
                    Kind::Str(_)
                        | Kind::Callback(_)
                        | Kind::F64s
                        | Kind::Strs(_)
                        | Kind::Values(_)
                        | Kind::OptRecord(_)
                        | Kind::Handles(..)
                        | Kind::Records(_)
                )
            }) {
                shims.push(quote! {
                    /// # Safety
                    /// String arguments are live script strings (or null).
                    #[unsafe(no_mangle)]
                    pub unsafe extern "C" fn #symbol_ident( #( #abi_params ),* ) -> #abi_ret {
                        #body
                    }
                });
            } else {
                shims.push(quote! {
                    #[unsafe(no_mangle)]
                    pub extern "C" fn #symbol_ident( #( #abi_params ),* ) -> #abi_ret {
                        #body
                    }
                });
            }
        }
    }
    // ---- enums: manifest entries, helper functions, `platypus_<Enum>_name` / `_parse` shims ----
    let mut enum_defs: Vec<TokenStream2> = Vec::new();
    let mut enum_items: Vec<TokenStream2> = Vec::new();
    for e in args.enums.iter().filter(|e| !e.external) {
        let ts = &e.ts;
        let names = e.names();
        let rust = &e.rust;
        let variants = &e.variants;
        let idx: Vec<i32> = (0..variants.len() as i32).collect();
        let doc = if e.doc.is_empty() {
            items
                .iter()
                .find_map(|it| match it {
                    Item::Enum(en) if Some(en.ident.to_string()) == rust_name_of(&e.rust) => {
                        Some(doc_of(&en.attrs))
                    }
                    _ => None,
                })
                .unwrap_or_default()
        } else {
            e.doc.clone()
        };
        enum_defs
            .push(quote!(::ts_api::EnumDef { name: #ts, doc: #doc, values: &[ #( #names ),* ] }));
        let parse = enum_fn(ts, "parse");
        let name = enum_fn(ts, "name");
        let index = enum_fn(ts, "index");
        let from_index = enum_fn(ts, "from_index");
        let names_const = format_ident!("{}_NAMES", snake(ts).to_uppercase());
        let name_sym = format_ident!("platypus_{}_name", ts);
        let parse_sym = format_ident!("platypus_{}_parse", ts);
        symbol_entries
            .push(quote!((concat!("platypus_", #ts, "_name"), #name_sym as *const () as usize)));
        symbol_entries
            .push(quote!((concat!("platypus_", #ts, "_parse"), #parse_sym as *const () as usize)));
        enum_items.push(quote! {
            /// The names scripts see, in index order.
            pub const #names_const: &[&str] = &[ #( #names ),* ];

            /// The value with this script name.
            pub fn #parse(s: &str) -> ::std::option::Option<#rust> {
                match s {
                    #( #names => ::std::option::Option::Some(#rust::#variants), )*
                    _ => ::std::option::Option::None,
                }
            }

            /// The script name of a value.
            pub fn #name(v: #rust) -> &'static str {
                match v {
                    #( #rust::#variants => #names, )*
                }
            }

            /// The index of a value: what travels across the C ABI.
            pub fn #index(v: #rust) -> i32 {
                match v {
                    #( #rust::#variants => #idx, )*
                }
            }

            /// The value at an index, or none.
            pub fn #from_index(i: i32) -> ::std::option::Option<#rust> {
                match i {
                    #( #idx => ::std::option::Option::Some(#rust::#variants), )*
                    _ => ::std::option::Option::None,
                }
            }

            /// `TypeName.name(i)` for compiled code: the script name of an index, or "".
            ///
            /// # Safety
            /// `out_len` is valid for a write.
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn #name_sym(i: i32, out_len: *mut i64) -> *const u8 {
                let text = #from_index(i).map_or("", #name);
                unsafe { ::ts_api::return_string(text, out_len) }
            }

            /// `TypeName.parse(s)` for compiled code: the index of a script name, or -1.
            ///
            /// # Safety
            /// `s` is a live script string (or null).
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn #parse_sym(s: *const u8) -> i32 {
                #parse(&unsafe { ::ts_api::script_str(s) }).map_or(-1, #index)
            }
        });
    }

    // ---- records: manifest entries, `__flat_<R>` (the numbers) and the JSON forms ----
    let mut record_defs: Vec<TokenStream2> = Vec::new();
    let mut record_items: Vec<TokenStream2> = Vec::new();
    for r in &args.records {
        let ts = &r.ts;
        let rust = &r.rust;
        let (item_doc, item_field_docs) = items
            .iter()
            .find_map(|it| match it {
                Item::Struct(st) if Some(st.ident.to_string()) == rust_name_of(&r.rust) => {
                    let docs: Vec<(String, String)> = st
                        .fields
                        .iter()
                        .filter_map(|f| f.ident.as_ref().map(|i| (i.to_string(), doc_of(&f.attrs))))
                        .collect();
                    Some((doc_of(&st.attrs), docs))
                }
                _ => None,
            })
            .unwrap_or_default();
        let rec_doc = if r.doc.is_empty() {
            item_doc
        } else {
            r.doc.clone()
        };
        // The declaration's docs win over the struct's.
        let mut field_docs = item_field_docs;
        for (n, _, d, _) in &r.fields {
            if !d.is_empty() {
                field_docs.retain(|(k, _)| *n != *k);
                field_docs.push((n.to_string(), d.clone()));
            }
        }
        let flat = format_ident!("__flat_{}", ts);
        let from_json = format_ident!("__from_json_{}", ts);
        let to_json = format_ident!("__to_json_{}", ts);
        let mut fields = Vec::new();
        let mut flat_stmts = Vec::new();
        let mut json_stmts = Vec::new();
        let mut parse_fields = Vec::new();
        let mut flat_ok = true;
        for (fname, fty, _, fkey) in &r.fields {
            let key = fkey.clone().unwrap_or_else(|| camel(&fname.to_string()));
            let doc = field_docs
                .iter()
                .find(|(n, _)| *fname == *n)
                .map(|(_, d)| d.clone())
                .unwrap_or_default();
            let ty = match parse_rec_ty(fty, &args) {
                Ok(t) => t,
                Err(e) => return Err(syn::Error::new(fname.span(), e)),
            };
            let mty = rec_ty_manifest(&ty, &args);
            fields.push(quote!(::ts_api::RecordField { name: #key, ty: #mty, doc: #doc }));
            flat_ok &= rec_flat(&ty);
            if rec_flat(&ty) {
                flat_stmts.push(rec_flat_stmt(&ty, quote!(r.#fname), &args));
            }
            json_stmts.push(rec_json_stmt(&ty, &key, quote!((&r.#fname)), &args));
            let parse = rec_parse_field(&ty, &key, &args);
            parse_fields.push(quote!(#fname: #parse?));
        }
        if flat_ok {
            record_items.push(quote! {
                #[allow(dead_code)]
                fn #flat(
                    r: &#rust,
                    out: &mut ::std::vec::Vec<f64>,
                    strs: &mut ::std::vec::Vec<::std::string::String>,
                ) {
                    let _ = (&out, &strs);
                    #( #flat_stmts )*
                }
            });
        }
        // The JSON forms: how a record travels as an argument or an optional result.
        record_items.push(quote! {
            #[allow(dead_code, unused_variables)]
            fn #from_json(v: &::ts_api::serde_json::Value) -> ::std::result::Result<#rust, ::std::string::String> {
                let ::std::option::Option::Some(obj) = v.as_object() else {
                    return ::std::result::Result::Err(::std::string::String::from("not an object"));
                };
                ::std::result::Result::Ok(#rust { #( #parse_fields ),* })
            }

            #[allow(dead_code)]
            fn #to_json(r: &#rust) -> ::ts_api::serde_json::Value {
                let mut m = ::ts_api::serde_json::Map::new();
                #( #json_stmts )*
                ::ts_api::serde_json::Value::Object(m)
            }
        });
        record_defs.push(
            quote!(::ts_api::RecordDef { name: #ts, doc: #rec_doc, fields: &[ #( #fields ),* ] }),
        );
    }
    let module_doc = doc_of(&module.attrs);
    let like = args.like;
    let alias_names: Vec<&String> = args.aliases.iter().map(|(a, _)| a).collect();
    let alias_targets: Vec<&String> = args.aliases.iter().map(|(_, t)| t).collect();
    let class_defs: Vec<TokenStream2> = class_order
        .iter()
        .zip(&class_entries)
        .map(|(c, fs)| {
            let doc = class_docs.get(c).cloned().unwrap_or_default();
            quote!(::ts_api::ClassDef { name: #c, doc: #doc, functions: &[ #( #fs ),* ] })
        })
        .collect();

    let attrs = &module.attrs;
    let vis = &module.vis;
    let ident = &module.ident;
    Ok(quote! {
        #(#attrs)*
        #vis mod #ident {
            #(#out_items)*

            /// What this namespace exports to scripts.
            pub const MANIFEST: ::ts_api::Namespace = ::ts_api::Namespace {
                symbols: ::ts_api::SymbolTable(platypus_symbols),
                name: #ns,
                doc: #module_doc,
                like: #like,
                handles: &[ #( #handle_names ),* ],
                enums: &[ #( #enum_defs ),* ],
                records: &[ #( #record_defs ),* ],
                aliases: &[ #( (#alias_names, #alias_targets) ),* ],
                values: &[ #( #value_defs ),* ],
                classes: &[ #( #class_defs ),* ],
                functions: &[ #( #ns_entries ),* ],
            };

            #( #shims )*

            #( #enum_items )*
            #( #record_items )*

            /// The shims' names and addresses, for a JIT.
            pub fn platypus_symbols() -> Vec<(&'static str, usize)> {
                vec![ #( #symbol_entries ),* ]
            }
        }
    })
}
