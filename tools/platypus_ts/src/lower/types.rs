// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Annotation -> IR type resolution, struct layouts and coercions.

use super::expr::mk;
use super::*;

/// Can `a` stand for `g` because the caller's extra arguments are ignored, or because `g` returns
/// `void` and the result of `a` is dropped (TypeScript allows any return type there)? Both are
/// sound for the one scalar return register of the calling conventions the JIT targets: the
/// caller of a `void` function never reads it, and the collector finds roots in frames, not in
/// return registers.
fn fn_ignores_extra(a: &FnTy, g: &FnTy) -> bool {
    a.params.len() <= g.params.len()
        && (a.ret == g.ret || g.ret == Ty::Void)
        && a.params
            .iter()
            .zip(&g.params)
            .all(|(x, y)| x == y || accepts_same_word(x, y))
}

/// A parameter of type `x` takes an argument of type `y` unchanged: `x` is `y` or null, held in
/// the same word (`(i = index) => ..` where `(i: number) => void` goes: a number, or the null NaN;
/// a pointer, or null; a tag, or the null tag). Not booleans, whose nullable form is another word.
fn accepts_same_word(x: &Ty, y: &Ty) -> bool {
    matches!(x, Ty::Nullable(inner) if **inner == *y)
        && (matches!(y, Ty::Num | Ty::Lit) || y.is_ptr() || matches!(y, Ty::Handle(_)))
}

fn b_strip_owned(t: &Ty) -> Ty {
    t.strip().clone()
}

impl<'a> Lowerer<'a> {
    /// Is `t` the prelude's `StyleValue` (what `number | string` means)?
    pub(crate) fn is_style_value(&self, t: &Ty) -> bool {
        matches!(t, Ty::Obj(s) if self.m.structs[*s].name == "StyleValue")
            && self.fids.contains_key("__sv_num")
    }

    pub(crate) fn key_name(&self, k: &PropertyKey<'a>) -> R<String> {
        match k {
            PropertyKey::StaticIdentifier(i) => Ok(i.name.to_string()),
            PropertyKey::StringLiteral(s) => Ok(s.value.to_string()),
            _ => bail!("computed property key"),
        }
    }

    pub(crate) fn new_struct(&mut self, name: &str, fields: Vec<Field>) -> StructId {
        self.m.structs.push(StructDef {
            name: name.to_string(),
            fields,
        });
        self.m.structs.len() - 1
    }

    /// A struct for an inline `{ x: number; y: number }` type or a literal: one per shape.
    pub(crate) fn anon_struct(&mut self, name: &str, fields: Vec<Field>) -> StructId {
        let fields = self.with_thunks(fields);
        let key = fields
            .iter()
            .map(|f| format!("{}:{}{}", f.name, f.ty, if f.optional { "?" } else { "" }))
            .collect::<Vec<_>>()
            .join(",");
        if let Some(id) = self.shapes.get(&key) {
            return *id;
        }
        let id = self.new_struct(name, fields);
        self.shapes.insert(key, id);
        id
    }

    pub(crate) fn ty(&mut self, t: &'a TSType<'a>, hint: &str) -> R<Ty> {
        Ok(match t {
            TSType::TSNumberKeyword(_) => Ty::Num,
            TSType::TSBigIntKeyword(_) => Ty::BigInt,
            TSType::TSSymbolKeyword(_) => self.named_type("Symbol_")?,
            TSType::TSBooleanKeyword(_) => Ty::Bool,
            TSType::TSStringKeyword(_) => Ty::Str,
            // `object` (a bag of properties, in practice a style): a `PlatypusStyle` when the program
            // has the UI prelude.
            TSType::TSObjectKeyword(_) if self.class_decls.contains_key("StyleEntry") => {
                self.named_type("PlatypusStyle")?
            }
            // `typeof TABLE`: the type of a top-level constant, from its initialiser.
            TSType::TSTypeQuery(q) => {
                // `typeof a.b`: the type of `a`'s property `b` (`types_index.rs`).
                if let TSTypeQueryExprName::QualifiedName(n) = &q.expr_name {
                    return self.typeof_path(n, q.span.start);
                }
                let TSTypeQueryExprName::IdentifierReference(id) = &q.expr_name else {
                    bail!("{}: unsupported `typeof` operand", self.loc(q.span.start))
                };
                self.typeof_ident(id.name.as_str())?
            }
            TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => Ty::Void,
            TSType::TSNullKeyword(_) => Ty::Null,
            TSType::TSParenthesizedType(p) => self.ty(&p.type_annotation, hint)?,
            // A type guard returns a boolean (`x is T`, `this is T`); an assertion function
            // (`asserts x`, `asserts x is T`) returns nothing.
            TSType::TSTypePredicate(p) if p.asserts => Ty::Void,
            TSType::TSTypePredicate(_) => Ty::Bool,
            // Not built yet (`docs/research/platypus-dynamic-values.md`): said plainly, with the
            // place (a bare `TreeRow` whose `T = unknown` default is used lands here too).
            TSType::TSAnyKeyword(_) | TSType::TSUnknownKeyword(_) | TSType::TSNeverKeyword(_) => {
                let (name, span) = match t {
                    TSType::TSAnyKeyword(k) => ("any", k.span),
                    TSType::TSUnknownKeyword(k) => ("unknown", k.span),
                    TSType::TSNeverKeyword(k) => ("never", k.span),
                    _ => unreachable!(),
                };
                bail!(
                    "{}: `{name}` is not supported yet (dynamic values are planned, docs/research/platypus-dynamic-values.md); write a concrete type{}",
                    self.loc(span.start),
                    if hint.is_empty() {
                        String::new()
                    } else {
                        format!(" (in {hint})")
                    }
                )
            }
            TSType::TSTypeOperatorType(o) => {
                // `keyof typeof TABLE`, `keyof Iface`: the union of the property names.
                if o.operator == TSTypeOperatorOperator::Keyof
                    && let Some(keys) = self.keyof_names(&o.type_annotation)
                {
                    for k in &keys {
                        self.tag(k);
                    }
                    return Ok(Ty::Lit);
                }
                self.ty(&o.type_annotation, hint)?
            }
            TSType::TSArrayType(a) => Ty::Arr(Box::new(self.ty(&a.element_type, hint)?)),
            TSType::TSLiteralType(l) => match &l.literal {
                TSLiteral::StringLiteral(s) => {
                    self.tag(s.value.as_str());
                    Ty::Lit
                }
                TSLiteral::NumericLiteral(_) => Ty::Num,
                TSLiteral::BooleanLiteral(_) => Ty::Bool,
                // `-1 | 1`
                TSLiteral::UnaryExpression(u)
                    if matches!(u.argument, Expression::NumericLiteral(_)) =>
                {
                    Ty::Num
                }
                _ => bail!("unsupported literal type"),
            },
            // `[number, number]`: a tuple of one element type is an array of it.
            TSType::TSTupleType(t) => {
                let mut tys = Vec::new();
                let mut optional_from: Option<usize> = None;
                for el in &t.element_types {
                    // `"c"?`: an optional element is `undefined` when the array is shorter.
                    if let TSTupleElement::TSOptionalType(o) = el {
                        optional_from.get_or_insert(tys.len());
                        tys.push(self.ty(&o.type_annotation, hint)?.nullable());
                        continue;
                    }
                    let Some(et) = el.as_ts_type() else {
                        bail!(
                            "{}: rest tuple elements are not supported",
                            self.loc(t.span.start)
                        )
                    };
                    tys.push(self.ty(et, hint)?);
                }
                if optional_from.is_some() {
                    self.tuple_ty_fields(tys)
                } else {
                    self.tuple_ty(tys)
                }
            }
            TSType::TSUnionType(u) => self.union(&u.types, hint)?,
            // `A & { extra: T }`: one struct with the fields of every part (a part that is a
            // discriminated union brings all its variants' fields, as the union's struct has them).
            TSType::TSIntersectionType(x) => {
                let mut fields: Vec<Field> = Vec::new();
                let mut disc = None;
                for part in &x.types {
                    let pt = self.ty(part, hint)?;
                    let Ty::Obj(sid) = pt.strip() else {
                        bail!(
                            "{}: an intersection of object types only (got {pt})",
                            self.loc(x.span.start)
                        )
                    };
                    if let Some(d) = self.discs.get(sid) {
                        disc = Some(d.clone());
                    }
                    // Accessor thunks (`_p_..`) are added again by `anon_struct`.
                    let part_fields: Vec<Field> = self.m.structs[*sid]
                        .fields
                        .iter()
                        .filter(|f| !f.name.starts_with("_p_"))
                        .cloned()
                        .collect();
                    for f in part_fields {
                        match fields.iter().position(|o| o.name == f.name) {
                            None => fields.push(f),
                            Some(i) if fields[i].ty == f.ty => {
                                // Required in either part: required in both.
                                fields[i].optional &= f.optional;
                            }
                            Some(i) => bail!(
                                "intersection {hint}: field {} has two different types ({} and {})",
                                f.name,
                                fields[i].ty,
                                f.ty
                            ),
                        }
                    }
                }
                let sid = self.anon_struct(hint, fields);
                if let Some(d) = disc {
                    self.discs.insert(sid, d);
                }
                Ty::Obj(sid)
            }
            TSType::TSTypeLiteral(l) => {
                // `{ [key: string]: V }` is a dictionary, like `Record<string, V>`.
                if let [TSSignature::TSIndexSignature(ix)] = l.members.as_slice()
                    && matches!(
                        ix.parameter.type_annotation.type_annotation,
                        TSType::TSStringKeyword(_)
                    )
                {
                    let v = self.ty(&ix.type_annotation.type_annotation, hint)?;
                    return Ok(Ty::Map(Box::new(Ty::Str), Box::new(v)));
                }
                let fields = self.members(&l.members, hint)?;
                Ty::Obj(self.anon_struct(hint, fields))
            }
            // `[t: number, run: () => void]`: the labels are for readers.
            TSType::TSNamedTupleMember(m) => match m.element_type.as_ts_type() {
                Some(et) if !m.optional => self.ty(et, hint)?,
                _ => bail!(
                    "{}: optional tuple elements are not supported",
                    self.loc(m.span.start)
                ),
            },
            TSType::TSFunctionType(f) => {
                let mut params = Vec::new();
                for p in &f.params.items {
                    let Some(a) = &p.type_annotation else {
                        bail!("untyped callback parameter")
                    };
                    let ty = self.ty(&a.type_annotation, hint)?;
                    // `(x?: T) => ..`: a call may leave it out, so it is null then.
                    params.push(if p.optional { ty.nullable() } else { ty });
                }
                let ret = self.ty(&f.return_type.type_annotation, hint)?;
                Ty::Fn(Box::new(FnTy { params, ret }))
            }
            // `T["k"]`, `T[K]`, `T[number]`, `T[0]` (`types_index.rs`).
            TSType::TSIndexedAccessType(i) => self.indexed_access(t, i, hint)?,
            // `` `url(${string}` ``: a string (what it matches is `tsc`'s to check).
            TSType::TSTemplateLiteralType(_) => Ty::Str,
            TSType::TSTypeReference(r) => {
                // `Intl.Collator` is the prelude class `Intl_Collator`.
                if let TSTypeName::QualifiedName(q) = &r.type_name
                    && matches!(&q.left, TSTypeName::IdentifierReference(l) if l.name == "Intl")
                {
                    return self.named_type(&format!("Intl_{}", q.right.name));
                }
                // `React.ReactNode` is `ReactNode`.
                if let TSTypeName::QualifiedName(q) = &r.type_name
                    && matches!(&q.left, TSTypeName::IdentifierReference(l) if l.name == "React")
                {
                    return self.named_type(q.right.name.as_str());
                }
                let TSTypeName::IdentifierReference(id) = &r.type_name else {
                    bail!("qualified type name")
                };
                let args: Vec<&'a TSType<'a>> = r
                    .type_arguments
                    .as_ref()
                    .map(|a| a.params.iter().collect())
                    .unwrap_or_default();
                // A type parameter of the specialisation being lowered.
                if args.is_empty()
                    && let Some(t) = self.tenv.get(id.name.as_str())
                {
                    return Ok(t.clone());
                }
                // `TreeRow` for `interface TreeRow<T = unknown>`: the defaults stand for the
                // arguments left out.
                if (!args.is_empty() || self.all_defaulted(id.name.as_str()))
                    && (self.generic_classes.contains_key(id.name.as_str())
                        || self
                            .ifaces
                            .get(id.name.as_str())
                            .is_some_and(|i| i.type_parameters.is_some())
                        || self
                            .aliases
                            .get(id.name.as_str())
                            .is_some_and(|a| a.type_parameters.is_some()))
                {
                    let mut targs = Vec::new();
                    for a in &args {
                        targs.push(self.ty(a, hint)?);
                    }
                    let keys = self.alias_key_args(id.name.as_str(), &args, &mut targs);
                    return self.generic_type_keyed(id.name.as_str(), targs, &keys);
                }
                if let Some(t) = self.alias_utility(id.name.as_str(), &args, hint)? {
                    return Ok(t);
                }
                match id.name.as_str() {
                    "Partial" | "Required" | "Readonly" | "Pick" | "Omit"
                        if !args.is_empty() && !self.class_decls.contains_key(id.name.as_str()) =>
                    {
                        self.utility_type(id.name.as_str(), &args, hint)?
                    }
                    "WeakRef" if !args.is_empty() => {
                        let sp = oxc_span::GetSpan::span(args[0]);
                        let name = self.src[sp.start as usize..sp.end as usize].trim();
                        self.named_type(&format!("WeakRef${name}"))?
                    }
                    "PromiseSettledResult" if !args.is_empty() => {
                        let sp = oxc_span::GetSpan::span(args[0]);
                        let key = asyncs::key(&self.src[sp.start as usize..sp.end as usize]);
                        self.named_type(&format!("PromiseSettledResult${key}"))?
                    }
                    "Promise" if !args.is_empty() => {
                        let sp = oxc_span::GetSpan::span(args[0]);
                        let text = &self.src[sp.start as usize..sp.end as usize];
                        let key = if self.mentions_tparam(text) {
                            // `Promise<T>` inside `f<T>`: the class of this specialisation's T.
                            let at = self.loc(sp.start);
                            self.generic_promise_key(args[0], &at)?
                        } else {
                            asyncs::key(text)
                        };
                        self.named_type(&format!("Promise${key}"))?
                    }
                    // `Generator<T>`, `Iterator<T>`, `IteratorResult<T>`: the classes written per type.
                    n if generators::FAMILY.contains(&n) && !args.is_empty() => {
                        let text = |t: &TSType<'_>| {
                            let sp = oxc_span::GetSpan::span(t);
                            self.src[sp.start as usize..sp.end as usize].to_string()
                        };
                        let key = generators::key_of(
                            &text(args[0]),
                            args.get(2).map(|a| text(a)).as_deref(),
                        );
                        let class = match n {
                            "Iterator" => format!("Iterator${key}"),
                            "IteratorResult" => format!("IteratorResult${key}"),
                            _ => format!("Generator${key}"),
                        };
                        self.named_type(&class)?
                    }
                    n if crate::ir::TypedKind::from_name(n).is_some() => {
                        Ty::Typed(crate::ir::TypedKind::from_name(n).expect("checked"))
                    }
                    // `Record<"a" | "b", V>`: a struct with one field per member.
                    "Record" if args.len() == 2 => {
                        // `Record<Weather, V>` over a numeric enum: an array indexed by the value.
                        if let TSType::TSTypeReference(k) = args[0]
                            && let TSTypeName::IdentifierReference(kid) = &k.type_name
                            && self.enums.contains_key(kid.name.as_str())
                        {
                            return Ok(Ty::Arr(Box::new(self.ty(args[1], hint)?)));
                        }
                        // `Record<string, unknown>` (a bag of style properties) is a `PlatypusStyle`.
                        if matches!(args[0], TSType::TSStringKeyword(_))
                            && matches!(args[1], TSType::TSUnknownKeyword(_))
                            && self.class_decls.contains_key("StyleEntry")
                        {
                            return self.named_type("PlatypusStyle");
                        }
                        // `Record<string, V>`: a dictionary, a `Map<string, V>`.
                        if matches!(args[0], TSType::TSStringKeyword(_)) {
                            return Ok(Ty::Map(
                                Box::new(Ty::Str),
                                Box::new(self.ty(args[1], hint)?),
                            ));
                        }
                        let Some(keys) = self
                            .lit_members(args[0])
                            .or_else(|| self.keys_of_keyof(args[0]))
                        else {
                            bail!(
                                "Record needs a union of string literals (or `string`) as its keys; \
                                 use Map<K, V> for other keys"
                            )
                        };
                        let vt = self.ty(args[1], hint)?;
                        // Too many keys for a struct (it holds at most 64 fields): a dictionary.
                        if keys.len() > 48 {
                            return Ok(Ty::Map(Box::new(Ty::Str), Box::new(vt)));
                        }
                        let fields = keys
                            .into_iter()
                            .map(|name| Field {
                                name,
                                ty: vt.clone(),
                                optional: false,
                            })
                            .collect();
                        Ty::Obj(self.anon_struct("Record", fields))
                    }
                    "Map" | "WeakMap" | "ReadonlyMap" => {
                        let (Some(k), Some(v)) = (args.first(), args.get(1)) else {
                            bail!("{} needs two type arguments", id.name)
                        };
                        Ty::Map(
                            Box::new(maps::map_key_ty(self.ty(k, hint)?)),
                            Box::new(self.ty(v, hint)?),
                        )
                    }
                    "Set" | "WeakSet" | "ReadonlySet" => {
                        let Some(k) = args.first() else {
                            bail!("{} needs a type argument", id.name)
                        };
                        Ty::Set(Box::new(maps::map_key_ty(self.ty(k, hint)?)))
                    }
                    "Array" | "ReadonlyArray" => {
                        let Some(e) = args.first() else {
                            bail!("generic collection without a type")
                        };
                        Ty::Arr(Box::new(self.ty(e, hint)?))
                    }
                    n => self.named_type(n).map_err(|e| {
                        if e.starts_with("unknown type") {
                            format!("{}: {e}", self.loc(r.span.start))
                        } else {
                            e
                        }
                    })?,
                }
            }
            other => {
                let kind: String = format!("{other:?}").chars().take(40).collect();
                bail!(
                    "unsupported type annotation ({kind}, byte {})",
                    oxc_span::GetSpan::span(other).start
                )
            }
        })
    }

    /// A tuple of these element types: an array when they agree, else an object with fields `0`, `1`, ..
    pub(crate) fn tuple_ty(&mut self, tys: Vec<Ty>) -> Ty {
        match tys.first() {
            Some(first) if tys.iter().all(|t| t == first) => Ty::Arr(Box::new(first.clone())),
            _ => self.tuple_ty_fields(tys),
        }
    }

    fn tuple_ty_fields(&mut self, tys: Vec<Ty>) -> Ty {
        let fields = tys
            .iter()
            .enumerate()
            .map(|(i, t)| Field {
                name: i.to_string(),
                ty: t.clone(),
                optional: false,
            })
            .collect();
        Ty::Obj(self.anon_struct("Tuple", fields))
    }

    pub(crate) fn members(&mut self, members: &'a [TSSignature<'a>], hint: &str) -> R<Vec<Field>> {
        let mut fields = Vec::new();
        for m in members {
            // `run(a: T): R;` is the property `run: (a: T) => R`.
            if let TSSignature::TSMethodSignature(ms) = m {
                let name = self.key_name(&ms.key)?;
                let mut params = Vec::new();
                for p in &ms.params.items {
                    let Some(a) = &p.type_annotation else {
                        bail!("untyped method parameter")
                    };
                    let ty = self.ty(&a.type_annotation, &format!("{hint}_{name}"))?;
                    params.push(if p.optional { ty.nullable() } else { ty });
                }
                let ret = match &ms.return_type {
                    Some(r) => self.ty(&r.type_annotation, &format!("{hint}_{name}"))?,
                    None => Ty::Void,
                };
                let mut ty = Ty::Fn(Box::new(FnTy { params, ret }));
                if ms.optional {
                    ty = ty.nullable();
                }
                fields.push(Field {
                    name,
                    ty,
                    optional: ms.optional,
                });
                continue;
            }
            let TSSignature::TSPropertySignature(p) = m else {
                bail!(
                    "{}: unsupported member in object type (index and call signatures are not supported)",
                    self.loc(oxc_span::GetSpan::span(m).start)
                )
            };
            let name = self.key_name(&p.key)?;
            let Some(a) = &p.type_annotation else {
                bail!("property {name} without a type")
            };
            let mut ty = self.ty(&a.type_annotation, &format!("{hint}_{name}"))?;
            if p.optional {
                ty = ty.nullable();
            }
            fields.push(Field {
                name,
                ty,
                optional: p.optional,
            });
        }
        Ok(fields)
    }

    /// The property names of `typeof NAME` (a top-level object literal), an interface or a type
    /// literal; `None` when they are not known here.
    pub(crate) fn keyof_names(&mut self, t: &'a TSType<'a>) -> Option<Vec<String>> {
        match t {
            TSType::TSParenthesizedType(p) => self.keyof_names(&p.type_annotation),
            TSType::TSTypeQuery(q) => {
                let TSTypeQueryExprName::IdentifierReference(id) = &q.expr_name else {
                    return None;
                };
                let mut init = *self.const_inits.get(id.name.as_str())?;
                loop {
                    match init {
                        Expression::TSAsExpression(a) => init = &a.expression,
                        Expression::TSSatisfiesExpression(a) => init = &a.expression,
                        Expression::ParenthesizedExpression(p) => init = &p.expression,
                        _ => break,
                    }
                }
                let Expression::ObjectExpression(o) = init else {
                    return None;
                };
                let mut out = Vec::new();
                for p in &o.properties {
                    let ObjectPropertyKind::ObjectProperty(p) = p else {
                        return None;
                    };
                    out.push(self.key_name(&p.key).ok()?);
                }
                Some(out)
            }
            TSType::TSTypeLiteral(l) => self.member_names(&l.members),
            TSType::TSTypeReference(r) => {
                let TSTypeName::IdentifierReference(id) = &r.type_name else {
                    return None;
                };
                // `keyof T` for a type parameter: the fields of what this specialisation binds.
                if r.type_arguments.is_none()
                    && let Some(t) = self.tenv.get(id.name.as_str())
                {
                    return self.field_names(t);
                }
                // `keyof PlatypusStyle`: the declared style properties.
                if let Some(i) = self.style_props_iface(id.name.as_str()) {
                    return self.member_names(&i.body.body);
                }
                if let Some(i) = self.ifaces.get(id.name.as_str()).copied() {
                    if !i.extends.is_empty() {
                        return None;
                    }
                    return self.member_names(&i.body.body);
                }
                let a = self.aliases.get(id.name.as_str()).copied()?;
                self.keyof_names(&a.type_annotation)
            }
            _ => None,
        }
    }

    /// The names a key type stands for when it is `keyof typeof TABLE` / `keyof Iface`, written
    /// directly or through aliases (`type Tint = keyof typeof TINTS`).
    pub(crate) fn keys_of_keyof(&mut self, t: &'a TSType<'a>) -> Option<Vec<String>> {
        match t {
            TSType::TSParenthesizedType(p) => self.keys_of_keyof(&p.type_annotation),
            TSType::TSTypeOperatorType(o) if o.operator == TSTypeOperatorOperator::Keyof => {
                self.keyof_names(&o.type_annotation)
            }
            TSType::TSTypeReference(r) => {
                let TSTypeName::IdentifierReference(id) = &r.type_name else {
                    return None;
                };
                if let Some(k) = self.tenv.keys.get(id.name.as_str()) {
                    return Some(k.clone());
                }
                let a = self.aliases.get(id.name.as_str()).copied()?;
                self.keys_of_keyof(&a.type_annotation)
            }
            _ => None,
        }
    }

    fn member_names(&mut self, members: &'a [TSSignature<'a>]) -> Option<Vec<String>> {
        let mut out = Vec::new();
        for m in members {
            let TSSignature::TSPropertySignature(p) = m else {
                return None;
            };
            out.push(self.key_name(&p.key).ok()?);
        }
        Some(out)
    }

    /// Like `lit_members`, ignoring `null` / `undefined` members of the union.
    pub(crate) fn lit_members_opt(&self, t: &'a TSType<'a>) -> Option<Vec<String>> {
        match t {
            TSType::TSParenthesizedType(p) => self.lit_members_opt(&p.type_annotation),
            TSType::TSUnionType(u) => {
                let mut out = Vec::new();
                for m in &u.types {
                    if matches!(m, TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_)) {
                        continue;
                    }
                    out.extend(self.lit_members_opt(m)?);
                }
                Some(out)
            }
            t => self.lit_members(t),
        }
    }

    /// A string literal that is not a member of the string-literal union it is written for
    /// (`const m: "a" | "b" = "c"`) is an error, as it is in `tsc`.
    pub(crate) fn check_lit_member(&self, ann: &'a TSType<'a>, e: &Expression<'a>) -> R<()> {
        let Some(members) = self.lit_members_opt(ann) else {
            return Ok(());
        };
        match e {
            Expression::ParenthesizedExpression(p) => self.check_lit_member(ann, &p.expression),
            Expression::ConditionalExpression(c) => {
                self.check_lit_member(ann, &c.consequent)?;
                self.check_lit_member(ann, &c.alternate)
            }
            Expression::StringLiteral(s) if !members.iter().any(|m| m == s.value.as_str()) => {
                let all: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
                bail!(
                    "{}: type \"{}\" is not assignable to type {}",
                    self.loc(s.span.start),
                    s.value,
                    all.join(" | ")
                )
            }
            _ => Ok(()),
        }
    }

    /// The members of a union of string literals (through aliases), or `None` for any other type.
    pub(crate) fn lit_members(&self, t: &'a TSType<'a>) -> Option<Vec<String>> {
        match t {
            TSType::TSParenthesizedType(p) => self.lit_members(&p.type_annotation),
            TSType::TSLiteralType(l) => match &l.literal {
                TSLiteral::StringLiteral(s) => Some(vec![s.value.to_string()]),
                _ => None,
            },
            TSType::TSUnionType(u) => {
                let mut out = Vec::new();
                for m in &u.types {
                    out.extend(self.lit_members(m)?);
                }
                Some(out)
            }
            TSType::TSTypeReference(r) => {
                let TSTypeName::IdentifierReference(id) = &r.type_name else {
                    return None;
                };
                // A key parameter of this specialisation (`K` of `T[K]`): the literals it is.
                if let Some(k) = self.tenv.keys.get(id.name.as_str()) {
                    return Some(k.clone());
                }
                // An enum of the engine API (`FogMode`) is the union of its names.
                if !self.aliases.contains_key(id.name.as_str())
                    && let Some(e) = self.manifest.enums.get(id.name.as_str())
                {
                    return Some(e.values.clone());
                }
                // `Exclude<Part, "center">`, `Extract<Part, "x" | "y">`: the members left or kept.
                if matches!(id.name.as_str(), "Exclude" | "Extract")
                    && !self.aliases.contains_key(id.name.as_str())
                    && let Some(a) = &r.type_arguments
                    && let [from, by] = a.params.as_slice()
                {
                    let by = self.lit_members(by)?;
                    let keep = id.name == "Extract";
                    let mut out = self.lit_members(from)?;
                    out.retain(|m| by.contains(m) == keep);
                    return Some(out);
                }
                let a = self.aliases.get(id.name.as_str())?;
                self.lit_members(&a.type_annotation)
            }
            // `Thing["kind"]`, `Alias["a" | "b"]`, `PlatypusStyle["position"]`: the literals of the
            // properties' types (`types_index.rs`).
            TSType::TSIndexedAccessType(i)
                if !matches!(i.index_type, TSType::TSNumberKeyword(_)) =>
            {
                self.indexed_lit_members(i)
            }
            // `(typeof NAMES)[number]` where `NAMES = ["a", "b"] as const`.
            TSType::TSIndexedAccessType(i)
                if matches!(i.index_type, TSType::TSNumberKeyword(_)) =>
            {
                let mut obj_ty = &i.object_type;
                while let TSType::TSParenthesizedType(p) = obj_ty {
                    obj_ty = &p.type_annotation;
                }
                let TSType::TSTypeQuery(q) = obj_ty else {
                    return None;
                };
                let TSTypeQueryExprName::IdentifierReference(id) = &q.expr_name else {
                    return None;
                };
                let mut init = *self.const_inits.get(id.name.as_str())?;
                loop {
                    match init {
                        Expression::TSAsExpression(a) => init = &a.expression,
                        Expression::TSSatisfiesExpression(a) => init = &a.expression,
                        Expression::ParenthesizedExpression(p) => init = &p.expression,
                        _ => break,
                    }
                }
                let Expression::ArrayExpression(arr) = init else {
                    return None;
                };
                let mut out = Vec::new();
                for el in &arr.elements {
                    match el.as_expression()? {
                        Expression::StringLiteral(s) => out.push(s.value.to_string()),
                        _ => return None,
                    }
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// Whether a generic interface or alias has a default for every type parameter, so its bare
    /// name is a type (`TreeRow` for `TreeRow<T = unknown>`).
    fn all_defaulted(&self, name: &str) -> bool {
        let params = match (self.ifaces.get(name), self.aliases.get(name)) {
            (Some(i), _) => i.type_parameters.as_deref(),
            (None, Some(a)) => a.type_parameters.as_deref(),
            _ => None,
        };
        params.is_some_and(|p| p.params.iter().all(|t| t.default.is_some()))
    }

    /// `Box<number>`, `Pair<A, B>`, `Opt<T>`: the specialisation of a generic class, interface or
    /// alias, made once per list of type arguments.
    pub(crate) fn generic_type(&mut self, base: &str, targs: Vec<Ty>) -> R<Ty> {
        self.generic_type_keyed(base, targs, &Vec::new())
    }

    /// [`Self::generic_type`] with the literals of the key parameters (`Get<P, "x">` for
    /// `type Get<T, K extends keyof T> = T[K]`).
    pub(crate) fn generic_type_keyed(
        &mut self,
        base: &str,
        targs: Vec<Ty>,
        keys: &generics::TKeys,
    ) -> R<Ty> {
        if self.generic_classes.contains_key(base) {
            let tparams = self.generic_classes[base].tparams.clone();
            let targs = self.complete_targs(base, &tparams, targs)?;
            let name = self.instantiate_class(base, targs)?;
            return self.named_type(&name);
        }
        let (tparams, is_iface) = match self.ifaces.get(base) {
            Some(i) => (generics::tparam_list(i.type_parameters.as_deref()), true),
            None => (
                generics::tparam_list(self.aliases[base].type_parameters.as_deref()),
                false,
            ),
        };
        let targs = self.complete_targs(base, &tparams, targs)?;
        let name = self.inst_name_keyed(base, &targs, keys);
        if let Some(t) = self.named.get(&name) {
            return Ok(t.clone());
        }
        let mut env = generics::TypeEnv::from(
            tparams
                .iter()
                .map(|(n, _)| n.clone())
                .zip(targs.iter().cloned())
                .collect::<HashMap<_, _>>(),
        );
        env.keys = keys.iter().cloned().collect();
        let env: generics::TEnv = std::rc::Rc::new(env);
        let saved = std::mem::replace(&mut self.tenv, env);
        let r = if is_iface {
            let i = self.ifaces[base];
            if !i.extends.is_empty() {
                self.tenv = saved;
                bail!("interface {base}: extends is not supported");
            }
            let id = self.new_struct(&name, vec![]);
            self.named.insert(name.clone(), Ty::Obj(id));
            self.targs_of_struct
                .insert(id, (base.to_string(), targs.clone()));
            self.members(&i.body.body, &name).map(|fields| {
                self.m.structs[id].fields = fields;
                Ty::Obj(id)
            })
        } else {
            let a = self.aliases[base];
            self.ty(&a.type_annotation, &name)
        };
        self.tenv = saved;
        let t = r?;
        // What it was made from, for inference (`RefObject<T>` matched against a value of it).
        if let Ty::Obj(sid) = t.strip() {
            self.targs_of_struct
                .entry(*sid)
                .or_insert_with(|| (base.to_string(), targs.clone()));
        }
        self.named.insert(name, t.clone());
        Ok(t)
    }

    /// The members of an interface, those of the interfaces it extends first.
    pub(crate) fn iface_fields(&mut self, name: &str) -> R<Vec<Field>> {
        let i = self.ifaces[name];
        let mut fields: Vec<Field> = Vec::new();
        for h in &i.extends {
            let TSTypeName::IdentifierReference(id) = &h.type_name else {
                bail!("interface {name}: extends a qualified name")
            };
            if h.type_arguments.is_some() || !self.ifaces.contains_key(id.name.as_str()) {
                bail!(
                    "interface {name}: extends {} (only interfaces of the program without type arguments)",
                    id.name
                )
            }
            for f in self.iface_fields(id.name.as_str())? {
                match fields.iter().position(|o| o.name == f.name) {
                    Some(k) => fields[k] = f,
                    None => fields.push(f),
                }
            }
        }
        for f in self.members(&i.body.body, name)? {
            match fields.iter().position(|o| o.name == f.name) {
                Some(k) => fields[k] = f,
                None => fields.push(f),
            }
        }
        Ok(fields)
    }

    /// Whether a written type names a type parameter of the specialisation being lowered.
    pub(crate) fn mentions_tparam(&self, text: &str) -> bool {
        !self.tenv.is_empty()
            && text
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
                .any(|w| self.tenv.contains_key(w))
    }

    /// The properties of an object type literal typed as string literals (`kind: "a"`,
    /// `kind: "strong" | "em"`): each name and its values.
    fn literal_tags(members: &[TSSignature<'_>]) -> Vec<(String, Vec<String>)> {
        let lit = |t: &TSType<'_>| match t {
            TSType::TSLiteralType(l) => match &l.literal {
                TSLiteral::StringLiteral(s) => Some(s.value.to_string()),
                _ => None,
            },
            _ => None,
        };
        let mut out = Vec::new();
        for m in members {
            let TSSignature::TSPropertySignature(p) = m else {
                continue;
            };
            let (PropertyKey::StaticIdentifier(k), Some(a)) = (&p.key, &p.type_annotation) else {
                continue;
            };
            let values: Option<Vec<String>> = match &a.type_annotation {
                TSType::TSUnionType(u) => u.types.iter().map(lit).collect(),
                t => lit(t).map(|v| vec![v]),
            };
            if let Some(v) = values {
                out.push((k.name.to_string(), v));
            }
        }
        out
    }

    /// An object type literal (not a `{ [key: string]: V }` dictionary), a union of them, or an
    /// intersection: an alias of one is a struct that may name itself.
    fn object_shaped(t: &TSType<'_>) -> bool {
        match t {
            TSType::TSTypeLiteral(l) => {
                !matches!(l.members.as_slice(), [TSSignature::TSIndexSignature(_)])
            }
            TSType::TSUnionType(u) => {
                u.types.len() > 1
                    && u.types
                        .iter()
                        .all(|t| matches!(t, TSType::TSTypeLiteral(_)))
            }
            TSType::TSIntersectionType(_) => true,
            TSType::TSParenthesizedType(p) => Self::object_shaped(&p.type_annotation),
            _ => false,
        }
    }

    pub(crate) fn named_type(&mut self, name: &str) -> R<Ty> {
        if let Some(t) = self.named.get(name) {
            if self.alias_resolving.contains(name) {
                self.alias_hits.insert(name.to_string());
            }
            return Ok(t.clone());
        }
        // The program's own declarations shadow the engine's names (`class Scene extends Component`).
        if self.class_decls.contains_key(name) {
            return self.class_type(name);
        }
        let own = self.ifaces.contains_key(name) || self.aliases.contains_key(name);
        if !own && self.manifest.values.contains_key(name) {
            return Ok(Ty::Obj(self.value_struct(name)));
        }
        if !own && self.manifest.handles.contains(name) {
            return Ok(Ty::Handle(name.to_string()));
        }
        // An enum of the engine API is a string (the type `tsc` sees is the union of its names).
        if !own && self.manifest.enums.contains_key(name) {
            return Ok(Ty::Str);
        }
        if !own && self.manifest.records.contains_key(name) {
            return Ok(Ty::Obj(self.record_struct(name)));
        }
        if self.ifaces.contains_key(name) {
            let id = self.new_struct(name, vec![]);
            self.named.insert(name.to_string(), Ty::Obj(id));
            if self.dyn_ifaces.contains(name) {
                self.iface_sids.insert(id, name.to_string());
            }
            let fields = self.iface_fields(name)?;
            let fields = self.with_thunks(fields);
            self.m.structs[id].fields = fields;
            return Ok(Ty::Obj(id));
        }
        if let Some(a) = self.aliases.get(name).copied() {
            // An object-shaped alias may name itself (`type Block = .. { children: Block[] }`):
            // its struct exists before its fields are worked out, as an interface's does.
            if Self::object_shaped(&a.type_annotation) {
                let id = self.new_struct(name, vec![]);
                self.named.insert(name.to_string(), Ty::Obj(id));
                self.alias_resolving.insert(name.to_string());
                let t = self.ty(&a.type_annotation, name);
                self.alias_resolving.remove(name);
                let recursive = self.alias_hits.remove(name);
                let t = t?;
                let Ty::Obj(sid) = t.strip().clone() else {
                    self.named.insert(name.to_string(), t.clone());
                    return Ok(t);
                };
                // Not recursive after all: the shape's own struct, shared with the same
                // inline type elsewhere (the placeholder stays unused).
                if !recursive {
                    // The placeholder must not pass for the type (layouts are found by name).
                    self.m.structs[id].name = format!("__unused_{name}");
                    self.named.insert(name.to_string(), t.clone());
                    return Ok(t);
                }
                self.m.structs[id].fields = self.m.structs[sid].fields.clone();
                if let Some(d) = self.discs.get(&sid).cloned() {
                    self.discs.insert(id, d);
                }
                let t = if matches!(t, Ty::Nullable(_)) {
                    Ty::Obj(id).nullable()
                } else {
                    Ty::Obj(id)
                };
                self.named.insert(name.to_string(), t.clone());
                return Ok(t);
            }
            let t = self.ty(&a.type_annotation, name)?;
            self.named.insert(name.to_string(), t.clone());
            return Ok(t);
        }
        bail!("unknown type {name}")
    }

    fn union(&mut self, types: &'a [TSType<'a>], hint: &str) -> R<Ty> {
        let mut nullable = false;
        let mut rest: Vec<&'a TSType<'a>> = Vec::new();
        for t in types {
            match t {
                TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => nullable = true,
                t => rest.push(t),
            }
        }
        let ty = if rest.iter().all(|t| matches!(t, TSType::TSTypeLiteral(_))) && rest.len() > 1 {
            // A discriminated union of object types: one struct holding every variant's fields.
            // A field whose type differs between variants (`children: Inline[]` in one,
            // `Block[]` in another) is a tagged union of those types; a test of the tag field
            // (`b.kind === "quote"`, a `switch`) narrows it (`Disc`, `tagged.rs`).
            let mut variants: Vec<Vec<Field>> = Vec::new();
            let mut tags: Vec<Vec<(String, Vec<String>)>> = Vec::new();
            for t in &rest {
                let TSType::TSTypeLiteral(l) = t else {
                    unreachable!()
                };
                variants.push(self.members(&l.members, hint)?);
                tags.push(Self::literal_tags(&l.members));
            }
            let mut fields: Vec<Field> = Vec::new();
            // Per field with differing types: its distinct types, and each variant's.
            let mut differing: Vec<(String, Vec<Ty>, Vec<Option<usize>>)> = Vec::new();
            for (v, vf) in variants.iter().enumerate() {
                for f in vf {
                    match fields.iter().position(|o| o.name == f.name) {
                        None => fields.push(f.clone()),
                        Some(i) if fields[i].ty == f.ty => {}
                        Some(i) => {
                            let d = match differing.iter().position(|d| d.0 == f.name) {
                                Some(d) => d,
                                None => {
                                    // Every variant before this one that has it had the first type.
                                    let mut per = vec![None; variants.len()];
                                    for (w, wf) in variants[..v].iter().enumerate() {
                                        if wf.iter().any(|g| g.name == f.name) {
                                            per[w] = Some(0);
                                        }
                                    }
                                    differing.push((
                                        f.name.clone(),
                                        vec![fields[i].ty.clone()],
                                        per,
                                    ));
                                    differing.len() - 1
                                }
                            };
                            let _ = i;
                            let d = &mut differing[d];
                            let k = match d.1.iter().position(|t| *t == f.ty) {
                                Some(k) => k,
                                None => {
                                    d.1.push(f.ty.clone());
                                    d.1.len() - 1
                                }
                            };
                            d.2[v] = Some(k);
                        }
                    }
                }
                // A variant after the first difference that has the first type again.
                for d in &mut differing {
                    if d.2[v].is_none()
                        && let Some(f) = vf.iter().find(|f| f.name == d.0)
                    {
                        d.2[v] = d.1.iter().position(|t| *t == f.ty);
                    }
                }
            }
            // A field some variant lacks may be missing (`"file" in segment` tells).
            for f in &mut fields {
                if variants
                    .iter()
                    .any(|vf| !vf.iter().any(|g| g.name == f.name))
                {
                    f.optional = true;
                }
            }
            let mut disc = None;
            if !differing.is_empty() {
                // The tag: a field every variant gives string literal values.
                let tag = tags.first().and_then(|first| {
                    first
                        .iter()
                        .map(|(n, _)| n.clone())
                        .find(|n| tags.iter().all(|t| t.iter().any(|(m, _)| m == n)))
                });
                let Some(tag) = tag else {
                    let (name, tys, _) = &differing[0];
                    bail!(
                        "union {hint}: field {name} has two different types ({} and {}) and the variants have no tag field to tell them apart",
                        tys[0],
                        tys[1]
                    )
                };
                let mut out_fields = Vec::new();
                for (name, tys, per) in differing {
                    let u = self.make_union(tys);
                    if let Some(f) = fields.iter_mut().find(|f| f.name == name) {
                        f.ty = u;
                    }
                    out_fields.push((name, per));
                }
                let variant_tags = tags
                    .iter()
                    .map(|t| {
                        t.iter()
                            .find(|(n, _)| *n == tag)
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default()
                    })
                    .collect();
                disc = Some(super::Disc {
                    tag,
                    variants: variant_tags,
                    fields: out_fields,
                });
            }
            let sid = self.anon_struct(hint, fields);
            if let Some(d) = disc {
                self.discs.insert(sid, d);
            }
            Ty::Obj(sid)
        } else {
            let mut tys = Vec::new();
            for t in &rest {
                tys.push(self.ty(t, hint)?);
            }
            if tys.is_empty() {
                Ty::Null
            } else {
                self.union_of(tys)?
            }
        };
        Ok(if nullable { ty.nullable() } else { ty })
    }

    /// The union of these types (none of them `null`): one type when they agree, `StyleValue`
    /// for `number | string`, else a tagged union (`tagged.rs`).
    pub(crate) fn union_of(&mut self, mut tys: Vec<Ty>) -> R<Ty> {
        Ok(if tys.iter().all(|t| *t == tys[0]) {
            tys.swap_remove(0)
        } else if self.class_decls.contains_key("StyleValue")
            && tys.contains(&Ty::Num)
            && tys.iter().all(|t| matches!(t, Ty::Num | Ty::Str | Ty::Lit))
        {
            // `number | string`: the prelude's `StyleValue` (CSS lengths and keywords).
            self.named_type("StyleValue")?
        } else {
            // Different types: a tagged union (`tagged.rs`). A string literal beside a
            // `string` is a `string`.
            let has_str = tys.contains(&Ty::Str);
            let mut members: Vec<Ty> = Vec::new();
            for t in tys {
                let t = if t == Ty::Lit && has_str { Ty::Str } else { t };
                if !members.contains(&t) {
                    members.push(t);
                }
            }
            if members.len() == 1 {
                members.remove(0)
            } else {
                self.make_union(members)
            }
        })
    }

    // ---- unify and coerce ----

    /// Can a value of type `from` be used as `to` without changing the pointer? A subclass for
    /// its base class, and arrays of those.
    pub(crate) fn assignable(&self, from: &Ty, to: &Ty) -> bool {
        match (from.strip(), to.strip()) {
            (a, b) if a == b => true,
            (Ty::Obj(x), Ty::Obj(y)) => self.is_subclass(*x, *y) || self.iface_accepts(*x, *y),
            (Ty::Arr(x), Ty::Arr(y)) => self.assignable(x, y),
            _ => false,
        }
    }

    pub(crate) fn unify(&self, a: &Ty, b: &Ty) -> R<Ty> {
        if a == b {
            return Ok(a.clone());
        }
        Ok(match (a, b) {
            (Ty::Null, t) | (t, Ty::Null) => t.clone().nullable(),
            (Ty::Lit, Ty::Str) | (Ty::Str, Ty::Lit) => Ty::Str,
            // `m ? m[0] : "none"`: a string that may be missing beside a literal.
            (Ty::Lit, t) | (t, Ty::Lit) if t.strip() == &Ty::Str => t.clone(),
            _ if a.strip() == b.strip() => a.strip().clone().nullable(),
            // Two classes of one family: the base class.
            _ if self.assignable(a, b) => b.strip().clone().nullable(),
            _ if self.assignable(b, a) => a.strip().clone().nullable(),
            // Two classes with a common base class: that base class.
            (x, y) if self.common_base(x.strip(), y.strip()).is_some() => {
                let base = self.common_base(x.strip(), y.strip()).expect("checked");
                let t = Ty::Obj(base);
                if a.is_nullable() || b.is_nullable() {
                    t.nullable()
                } else {
                    t
                }
            }
            _ => bail!("cannot unify {} and {}", self.describe(a), self.describe(b)),
        })
    }

    /// A type for an error message: an object shape is written out with its fields (its id
    /// means nothing to the reader), anything else as it prints.
    fn describe(&self, t: &Ty) -> String {
        match t.strip() {
            Ty::Obj(sid) if self.m.structs.get(*sid).is_some() => {
                let s = &self.m.structs[*sid];
                if s.fields.is_empty() {
                    return "{}".into();
                }
                let fields: Vec<String> = s
                    .fields
                    .iter()
                    .map(|f| {
                        let opt = if f.optional { "?" } else { "" };
                        format!("{}{opt}: {}", f.name, f.ty)
                    })
                    .collect();
                format!("{{ {} }}", fields.join(", "))
            }
            _ => t.to_string(),
        }
    }

    /// `e` as a `ReactNode` (compiled as `UiElement[]`) when `to` is one and `e` is a single node:
    /// an element is a list of it, text and numbers a fragment holding them, `null`, `undefined`
    /// and booleans nothing (as React renders them).
    fn react_node_of(&mut self, e: &Expr, to: &Ty) -> R<Option<Expr>> {
        let node = Ty::Handle("UiElement".into());
        // Text or a number where one element goes (`["x", <b/>]` as a `ReactNode`): a fragment
        // holding it.
        if *to.strip() == node
            && matches!(e.ty, Ty::Str | Ty::Lit | Ty::Num)
            && let Some(frag) = self
                .manifest
                .namespaces
                .get("Ui")
                .and_then(|n| n.get("fragment"))
                .cloned()
        {
            let frag = self.emit_extern(&frag, vec![]);
            let v = self.widen_lit(e.clone());
            let cands = self
                .manifest
                .overloads
                .get(&("Ui".to_string(), "child".to_string()))
                .cloned()
                .unwrap_or_default();
            return self
                .pick_overload("Ui", "child", &cands, vec![frag, v], "ReactNode")
                .map(Some);
        }
        // A component returning a `ReactNode` where an element goes: a fragment of the nodes.
        if *to.strip() == node && *e.ty.strip() == Ty::Arr(Box::new(node.clone())) {
            let Some(frag) = self
                .manifest
                .namespaces
                .get("Ui")
                .and_then(|n| n.get("fragment"))
                .cloned()
            else {
                return Ok(None);
            };
            let frag = self.emit_extern(&frag, vec![]);
            let cands = self
                .manifest
                .overloads
                .get(&("Ui".to_string(), "child".to_string()))
                .cloned()
                .unwrap_or_default();
            let list = if e.ty.is_nullable() {
                mk_expr_cast(e.clone(), e.ty.strip().clone())
            } else {
                e.clone()
            };
            return self
                .pick_overload("Ui", "child", &cands, vec![frag, list], "ReactNode")
                .map(Some);
        }
        if *to.strip() != Ty::Arr(Box::new(node.clone())) {
            return Ok(None);
        }
        let arr = to.strip().clone();
        let new_list = |n: usize| {
            mk(
                ExprKind::Rt(RtFn::ArrNew, vec![super::expr::num(n as f64)]),
                arr.clone(),
            )
        };
        let one = match e.ty.strip() {
            Ty::Handle(h) if h == "UiElement" => e.clone(),
            Ty::Str | Ty::Lit | Ty::Num if !e.ty.is_nullable() => {
                let Some(frag) = self
                    .manifest
                    .namespaces
                    .get("Ui")
                    .and_then(|n| n.get("fragment"))
                    .cloned()
                else {
                    return Ok(None);
                };
                let frag = self.emit_extern(&frag, vec![]);
                let v = self.widen_lit(e.clone());
                let cands = self
                    .manifest
                    .overloads
                    .get(&("Ui".to_string(), "child".to_string()))
                    .cloned()
                    .unwrap_or_default();
                self.pick_overload("Ui", "child", &cands, vec![frag, v], "ReactNode")?
            }
            Ty::Null | Ty::Void | Ty::Bool => {
                let mut stmts = Vec::new();
                if !matches!(e.ty, Ty::Null | Ty::Void) {
                    stmts.push(Stmt::Eval(e.clone()));
                }
                return Ok(Some(mk(ExprKind::Seq(stmts, Box::new(new_list(0))), arr)));
            }
            _ => return Ok(None),
        };
        if e.ty.is_nullable() {
            return Ok(None);
        }
        let t = self.new_local(arr.clone());
        let stmts = vec![
            Stmt::Assign(t, new_list(1)),
            Stmt::Eval(mk(
                ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(t), one]),
                Ty::Void,
            )),
        ];
        Ok(Some(mk(
            ExprKind::Seq(stmts, Box::new(self.var_expr(t))),
            arr,
        )))
    }

    pub(crate) fn coerce(&mut self, e: Expr, to: &Ty) -> R<Expr> {
        if e.ty == *to {
            return Ok(e);
        }
        // Where a `ReactNode` (`UiElement[]`) goes: an element, text, a number or nothing.
        if let Some(n) = self.react_node_of(&e, to)? {
            return Ok(n);
        }
        // A member where its union goes, and a union where one of its members goes
        // (`tagged.rs`).
        if !self.unions.is_empty() {
            if let Some(members) = self.union_members(to)
                && self.union_members(&e.ty).is_none()
                && !matches!(e.ty, Ty::Null)
                && let Some(idx) = self.member_index(&members, &e.ty)
            {
                let w = self.wrap_in_union(e, to, idx)?;
                return Ok(if to.is_nullable() && !w.ty.is_nullable() {
                    mk_expr_cast(w, to.clone())
                } else {
                    w
                });
            }
            if let Some(members) = self.union_members(&e.ty)
                && self.union_members(to).is_none()
                && !matches!(to.strip(), Ty::Void)
                && let Some(idx) = members.iter().position(|m| m.strip() == to.strip())
            {
                let p = self.project_union(e, idx);
                return Ok(if p.ty == *to {
                    p
                } else {
                    mk_expr_cast(p, to.clone())
                });
            }
        }
        // A plain object where a type with some of its fields (or `Partial`s of them) goes:
        // a copy. (Writes through the copy do not reach the original.)
        if let (Ty::Obj(a), Ty::Obj(b)) = (e.ty.strip(), to.strip())
            && a != b
            && (e.ty.is_nullable() <= to.is_nullable())
            && !self.assignable(&e.ty, to)
            && self.can_copy_fields(*a, *b)
        {
            let (a, b) = (*a, *b);
            if !to.is_nullable() {
                return Ok(self.copy_fields(e, a, b));
            }
            // Where `T | null` goes, null stays null (also behind a `!`, which tests nothing).
            let tmp = self.new_local(e.ty.clone());
            let t = self.var_expr(tmp);
            let is_null = mk(ExprKind::IsNull(Box::new(t.clone())), Ty::Bool);
            let plain = mk(ExprKind::Cast(Box::new(t)), Ty::Obj(a));
            let copy = self.copy_fields(plain, a, b);
            let copy = mk(ExprKind::Cast(Box::new(copy)), to.clone());
            return Ok(mk(
                ExprKind::Seq(
                    vec![Stmt::Assign(tmp, e)],
                    Box::new(mk(
                        ExprKind::Cond(
                            Box::new(is_null),
                            Box::new(mk(ExprKind::Null, to.clone())),
                            Box::new(copy),
                        ),
                        to.clone(),
                    )),
                ),
                to.clone(),
            ));
        }
        // A record (`Record<"a" | "b", V>`, an object of one type) where a `Record<string, V>` goes:
        // a map with its entries (a copy).
        if let (Ty::Obj(sid), Ty::Map(k, _)) = (&e.ty, to.strip())
            && **k == Ty::Str
            && !self.class_of_sid.contains_key(sid)
            && !self.is_dyn(*sid)
        {
            let (sid, to) = (*sid, to.strip().clone());
            return self.struct_to_map(e, sid, &to);
        }
        // An array of union members where an array of strings goes (and the other way): a copy
        // with each element converted (`next(["a", "b"] as const, engineEnumString)`).
        if let (Ty::Arr(a), Ty::Arr(b)) = (e.ty.strip(), to.strip())
            && ((**a == Ty::Lit && **b == Ty::Str) || (**a == Ty::Str && **b == Ty::Lit))
        {
            let (from_el, to_el) = ((**a).clone(), (**b).clone());
            let out_ty = to.strip().clone();
            let src = self.new_local(e.ty.clone());
            let out = self.new_local(out_ty.clone());
            let mut stmts = vec![
                Stmt::Assign(src, e),
                Stmt::Assign(
                    out,
                    Expr {
                        kind: ExprKind::Rt(
                            RtFn::ArrNew,
                            vec![Expr {
                                kind: ExprKind::Num(0.0),
                                ty: Ty::Num,
                            }],
                        ),
                        ty: out_ty.clone(),
                    },
                ),
            ];
            let it = self.begin_iter(self.var_expr(src))?;
            stmts.extend(it.pre.clone());
            let x = self.var_expr(it.elem);
            let converted = self.coerce(x, &to_el)?;
            let _ = from_el;
            let mut body = it.head.clone();
            body.push(Stmt::Eval(Expr {
                kind: ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(out), converted]),
                ty: Ty::Void,
            }));
            stmts.push(Stmt::Loop(it.label, body));
            let result = Expr {
                kind: ExprKind::Seq(stmts, Box::new(self.var_expr(out))),
                ty: out_ty,
            };
            return Ok(if to.is_nullable() {
                mk_expr_cast(result, to.clone())
            } else {
                result
            });
        }
        // An array of nothing but nulls (`Array.from({ length }, () => null)`) where an array of
        // nullable objects goes: the slots are all zero already.
        if let (Ty::Arr(a), Ty::Arr(b)) = (e.ty.strip(), to.strip())
            && **a == Ty::Null
            && b.is_nullable()
            && b.strip().is_ptr()
        {
            return Ok(Expr {
                kind: ExprKind::Cast(Box::new(e)),
                ty: to.clone(),
            });
        }
        // A null of another type; the expression still runs (`return bad()` where `bad` sets a
        // flag and returns null).
        let null_of = |e: Expr, to: &Ty| {
            let null = Expr {
                kind: ExprKind::Null,
                ty: to.clone(),
            };
            if matches!(e.kind, ExprKind::Null) {
                null
            } else {
                Expr {
                    kind: ExprKind::Seq(vec![Stmt::Eval(e)], Box::new(null)),
                    ty: to.clone(),
                }
            }
        };
        match (&e.ty, to) {
            (Ty::Null, t) if t.is_nullable() => Ok(null_of(e, to)),
            // `sprite = null`: an engine handle that is "none" (tsc has checked the declared type).
            (Ty::Null, Ty::Handle(_)) => Ok(null_of(e, to)),
            (a, b) if a.strip() == b.strip() => Ok(Expr {
                kind: ExprKind::Cast(Box::new(e)),
                ty: to.clone(),
            }),
            // A function that takes fewer parameters where more are given (`onClick={() => ..}` for
            // `(e) => void`): the extra arguments are ignored, as in JavaScript. The caller cleans
            // the argument registers, so the same closure works unchanged.
            (a, b)
                if matches!((a.strip(), b.strip()), (Ty::Fn(x), Ty::Fn(g)) if fn_ignores_extra(x, g))
                    && (b.is_nullable() || !a.is_nullable()) =>
            {
                Ok(Expr {
                    kind: ExprKind::Cast(Box::new(e)),
                    ty: to.clone(),
                })
            }
            // A component that may return nothing (`return null`) where an element is expected: a
            // null handle renders nothing.
            (Ty::Nullable(inner), Ty::Handle(n))
                if n == "UiElement" && matches!(&**inner, Ty::Handle(m) if m == n) =>
            {
                Ok(Expr {
                    kind: ExprKind::Cast(Box::new(e)),
                    ty: to.clone(),
                })
            }
            // A number or a string where a `number | string` (`StyleValue`) goes.
            (Ty::Num | Ty::Str | Ty::Lit, b) if self.is_style_value(b.strip()) => {
                let (f, e) = match &e.ty {
                    Ty::Num => ("__sv_num", e),
                    Ty::Str => ("__sv_str", e),
                    _ => (
                        "__sv_str",
                        Expr {
                            kind: ExprKind::TagStr(Box::new(e)),
                            ty: Ty::Str,
                        },
                    ),
                };
                let call = Expr {
                    kind: ExprKind::Call(self.fids[f], vec![e]),
                    ty: b_strip_owned(to),
                };
                if to.is_nullable() {
                    Ok(Expr {
                        kind: ExprKind::Cast(Box::new(call)),
                        ty: to.clone(),
                    })
                } else {
                    Ok(call)
                }
            }
            // A subclass object where its base class is expected: the same pointer.
            (a, b) if self.assignable(a.strip(), b.strip()) => Ok(Expr {
                kind: ExprKind::Cast(Box::new(e)),
                ty: to.clone(),
            }),
            // Text from outside (an engine property, a map's keys) where a union member is
            // expected: the member with that text (none when no union of the program has it).
            (Ty::Str, t) if t.strip() == &Ty::Lit => {
                let tag = Expr {
                    kind: ExprKind::StrTag(Box::new(e)),
                    ty: Ty::Lit,
                };
                if to.is_nullable() {
                    Ok(Expr {
                        kind: ExprKind::Cast(Box::new(tag)),
                        ty: to.clone(),
                    })
                } else {
                    Ok(tag)
                }
            }
            // Text that may be missing where a union member goes (`style: forced` after
            // `forced ? ..`): the member with that text, or null.
            (Ty::Nullable(inner), t) if **inner == Ty::Str && t.strip() == &Ty::Lit => {
                let tmp = self.new_local(e.ty.clone());
                let v = self.var_expr(tmp);
                let is_null = mk(ExprKind::IsNull(Box::new(v.clone())), Ty::Bool);
                let tag = mk(
                    ExprKind::StrTag(Box::new(mk(ExprKind::Cast(Box::new(v)), Ty::Str))),
                    Ty::Lit,
                );
                let none = if to.is_nullable() {
                    mk(ExprKind::Null, to.clone())
                } else {
                    // TypeScript narrowed it to a string; an unknown text is no member either.
                    mk(ExprKind::StrTag(Box::new(self.str_lit(""))), Ty::Lit)
                };
                let tag = if to.is_nullable() {
                    mk(ExprKind::Cast(Box::new(tag)), to.clone())
                } else {
                    tag
                };
                Ok(mk(
                    ExprKind::Seq(
                        vec![Stmt::Assign(tmp, e)],
                        Box::new(mk(
                            ExprKind::Cond(Box::new(is_null), Box::new(none), Box::new(tag)),
                            to.clone(),
                        )),
                    ),
                    to.clone(),
                ))
            }
            // A union member that may be missing where a `string` goes: its text, or null (a
            // test before it narrowed it in TypeScript when the target is a plain `string`).
            (Ty::Nullable(inner), t) if **inner == Ty::Lit && t.strip() == &Ty::Str => {
                let tmp = self.new_local(e.ty.clone());
                let v = self.var_expr(tmp);
                let is_null = mk(ExprKind::IsNull(Box::new(v.clone())), Ty::Bool);
                let text = mk(
                    ExprKind::TagStr(Box::new(mk(ExprKind::Cast(Box::new(v)), Ty::Lit))),
                    Ty::Str,
                );
                let (text, none) = if to.is_nullable() {
                    (
                        mk(ExprKind::Cast(Box::new(text)), to.clone()),
                        mk(ExprKind::Null, to.clone()),
                    )
                } else {
                    (text, self.str_lit(""))
                };
                Ok(mk(
                    ExprKind::Seq(
                        vec![Stmt::Assign(tmp, e)],
                        Box::new(mk(
                            ExprKind::Cond(Box::new(is_null), Box::new(none), Box::new(text)),
                            to.clone(),
                        )),
                    ),
                    to.clone(),
                ))
            }
            // A union member where a `string` is expected.
            (Ty::Lit, t) if t.strip() == &Ty::Str => {
                let s = Expr {
                    kind: ExprKind::TagStr(Box::new(e)),
                    ty: Ty::Str,
                };
                if t.is_nullable() {
                    Ok(Expr {
                        kind: ExprKind::Cast(Box::new(s)),
                        ty: to.clone(),
                    })
                } else {
                    Ok(s)
                }
            }
            (a, Ty::Void) => bail!("cannot use {a} as void"),
            (a, b) => bail!(
                "type mismatch: {} where {} is expected",
                self.ty_key(a),
                self.ty_key(b)
            ),
        }
    }

    /// Truthiness as a `Bool`.
    pub(crate) fn bool_of(&mut self, e: Expr) -> Expr {
        let ex = |k| Expr {
            kind: k,
            ty: Ty::Bool,
        };
        // A union of different types: the member it holds decides (0, "" and null are false).
        if self.union_members(&e.ty).is_some() {
            return self.union_truthy(e);
        }
        match &e.ty {
            Ty::Bool => e,
            Ty::Nullable(inner) if matches!(**inner, Ty::Num) => {
                let null = ex(ExprKind::IsNull(Box::new(e.clone())));
                ex(ExprKind::And(
                    Box::new(ex(ExprKind::Un(UnOp::Not, Box::new(null)))),
                    Box::new(ex(ExprKind::Truthy(Box::new(e)))),
                ))
            }
            // `boolean | undefined` is true only when it is `true`.
            Ty::Nullable(inner) if matches!(**inner, Ty::Bool) => ex(ExprKind::Cast(Box::new(e))),
            // A nullable string or bigint is true when it is there and not empty / zero.
            Ty::Nullable(inner) if matches!(**inner, Ty::Str | Ty::BigInt) => {
                let null = ex(ExprKind::IsNull(Box::new(e.clone())));
                let inner_ty = (**inner).clone();
                let plain = Expr {
                    kind: ExprKind::Cast(Box::new(e)),
                    ty: inner_ty,
                };
                ex(ExprKind::And(
                    Box::new(ex(ExprKind::Un(UnOp::Not, Box::new(null)))),
                    Box::new(self.bool_of(plain)),
                ))
            }
            Ty::Num => ex(ExprKind::Truthy(Box::new(e))),
            // A bigint is false only at zero.
            Ty::BigInt => ex(ExprKind::Cmp(
                CmpOp::Ne,
                Box::new(Expr {
                    kind: ExprKind::Rt(RtFn::Str("ts_rt_bigint_sign"), vec![e]),
                    ty: Ty::Num,
                }),
                Box::new(Expr {
                    kind: ExprKind::Num(0.0),
                    ty: Ty::Num,
                }),
            )),
            // A union member is true unless its text is empty.
            Ty::Lit => match e.kind {
                ExprKind::Tag(t) => ex(ExprKind::Bool(!self.m.tags[t as usize].is_empty())),
                _ => self.bool_of(Expr {
                    kind: ExprKind::TagStr(Box::new(e)),
                    ty: Ty::Str,
                }),
            },
            // A string is true when it is not empty.
            Ty::Str => ex(ExprKind::Cmp(
                CmpOp::Ne,
                Box::new(Expr {
                    kind: ExprKind::Rt(RtFn::Str("ts_rt_str_length"), vec![e]),
                    ty: Ty::Num,
                }),
                Box::new(Expr {
                    kind: ExprKind::Num(0.0),
                    ty: Ty::Num,
                }),
            )),
            // A handle is an id; zero is no object.
            Ty::Handle(_) => {
                let zero = Expr {
                    kind: ExprKind::Null,
                    ty: e.ty.clone(),
                };
                ex(ExprKind::Cmp(CmpOp::Ne, Box::new(e), Box::new(zero)))
            }
            t if t.is_nullable() => {
                let null = ex(ExprKind::IsNull(Box::new(e)));
                ex(ExprKind::Un(UnOp::Not, Box::new(null)))
            }
            _ => ex(ExprKind::Bool(true)),
        }
    }
}

fn mk_expr_cast(e: Expr, ty: Ty) -> Expr {
    Expr {
        kind: ExprKind::Cast(Box::new(e)),
        ty,
    }
}
