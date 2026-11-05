// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Expression lowering.

use super::*;
use crate::manifest::ExternFn;

pub(crate) fn mk(kind: ExprKind, ty: Ty) -> Expr {
    Expr { kind, ty }
}

pub(crate) fn num(n: f64) -> Expr {
    mk(ExprKind::Num(n), Ty::Num)
}

/// JavaScript's ToInt32, with the same saturating conversion the generated code uses.
fn to_i32(x: f64) -> i32 {
    if x.is_finite() { (x as i64) as i32 } else { 0 }
}

/// `**` with JavaScript's special cases (the runtime's `ts_rt_pow` does the same).
fn js_pow(x: f64, y: f64) -> f64 {
    if y.is_nan() || (x.abs() == 1.0 && y.is_infinite()) {
        f64::NAN
    } else {
        x.powf(y)
    }
}

/// A bit operation on two constants.
fn fold_bits(op: BinOp, a: f64, b: f64) -> f64 {
    let (x, y) = (to_i32(a), to_i32(b));
    let n = (y & 31) as u32;
    match op {
        BinOp::BitAnd => (x & y) as f64,
        BinOp::BitOr => (x | y) as f64,
        BinOp::BitXor => (x ^ y) as f64,
        BinOp::Shl => x.wrapping_shl(n) as f64,
        BinOp::Shr => (x >> n) as f64,
        _ => ((x as u32) >> n) as f64,
    }
}

/// One piece of an array being built: a value, or an array whose elements are spread in.
#[derive(Clone)]
pub(crate) enum Elem {
    One(Expr),
    Spread(Expr),
}

/// A place that can be read and written (assignment targets, `++`, compound assignment).
pub(crate) enum Place {
    Var(VarId, Ty),
    /// A captured variable: the cell and the value's type.
    Cell(Expr, Ty),
    /// A module-level `let`/`var`.
    Global(GlobalId, Ty),
    /// `xs.length`: reading is `Len`, writing truncates or grows.
    ArrLen(Expr),
    /// A property with a getter and/or setter: the receiver, the two functions and the value type.
    Accessor(Expr, Option<classes::Target>, classes::Target, Ty),
    Field(Expr, StructId, usize, Ty),
    /// A field of a plain object that a literal gave a setter closure (`set v(n) { .. }`): the
    /// object, its struct, the field, the hidden field with the setter, the value type. A read
    /// goes through the getter closure when there is one.
    Thunked(Expr, StructId, usize, usize, Ty),
    Index(Expr, Expr, Ty),
    /// `obj[key]` where `key` is a string or a union member naming one of the fields (`state[key]`
    /// with `key: keyof typeof state`): the object, its struct, the key, the one type of the fields.
    Keyed(Expr, StructId, Expr, Ty, Vec<usize>),
    /// `dict[key]` of a `Record<string, V>` (a map): the map, the key, the value type.
    MapEntry(Expr, Expr, Ty),
    /// A property of an engine handle class: the receiver, its getter (if any), its setter and
    /// the value type.
    Ext(Expr, Option<ExternFn>, ExternFn, Ty),
    /// A property of a namespace (no receiver): its getter (if any), its setter and the value type.
    NsProp(Option<ExternFn>, ExternFn, Ty),
}

impl<'a> Lowerer<'a> {
    pub(crate) fn loc(&self, offset: u32) -> String {
        format!("line {}", self.line(offset))
    }

    /// A value used as a number (`number | null` narrowed away).
    pub(crate) fn as_num(&self, e: Expr) -> R<Expr> {
        // A Date is its time value (`end - start`, `+date`).
        let e = self.date_value(e);
        match e.ty.strip() {
            Ty::Num if e.ty == Ty::Num => Ok(e),
            Ty::Num => Ok(mk(ExprKind::Cast(Box::new(e)), Ty::Num)),
            t => bail!("expected a number, found {t}"),
        }
    }

    /// Is this call `JSON.parse(..)`? (It needs the expected type.)
    pub(crate) fn is_json_parse(c: &CallExpression<'_>) -> bool {
        matches!(&c.callee, Expression::StaticMemberExpression(m)
            if m.property.name == "parse"
                && matches!(&m.object, Expression::Identifier(id) if id.name == "JSON"))
    }

    pub(crate) fn lower_expr(&mut self, e: &'a Expression<'a>, exp: Option<&Ty>) -> R<Expr> {
        if self.unions.is_empty() {
            return self.lower_expr_inner(e, exp);
        }
        // A literal that goes where a union is expected is the one member it can be.
        let hint = self.union_hint(e, exp);
        let v = self.lower_expr_inner(e, hint.as_ref().or(exp))?;
        Ok(self.narrow_value(e, v))
    }

    fn lower_expr_inner(&mut self, e: &'a Expression<'a>, exp: Option<&Ty>) -> R<Expr> {
        // `const order: SineType[] = xs.map(x => x.type)`: the callback's literals stay literals.
        if let (Some(Ty::Arr(el)), Expression::CallExpression(c)) = (exp, e)
            && **el == Ty::Lit
            && matches!(&c.callee, Expression::StaticMemberExpression(m) if m.property.name == "map")
        {
            self.map_keeps_lit = true;
        }
        match e {
            Expression::NumericLiteral(n) => Ok(num(n.value)),
            Expression::BigIntLiteral(b) => self.lower_bigint_literal(b),
            Expression::BooleanLiteral(b) => Ok(mk(ExprKind::Bool(b.value), Ty::Bool)),
            Expression::NullLiteral(_) => Ok(mk(ExprKind::Null, Ty::Null)),
            Expression::StringLiteral(s) => {
                // A literal with a lone surrogate cannot be a union member (tags are UTF-8 text).
                if s.lone_surrogates || matches!(exp.map(|t| t.strip()), Some(Ty::Str)) {
                    let bytes = strings::literal_bytes(s.value.as_str(), s.lone_surrogates);
                    return Ok(self.str_bytes(bytes));
                }
                let t = self.tag(s.value.as_str());
                Ok(mk(ExprKind::Tag(t), Ty::Lit))
            }
            Expression::TemplateLiteral(t) => self.lower_template(t),
            Expression::RegExpLiteral(r) => self.lower_regexp_literal(r),
            Expression::Identifier(id) => self.lower_ident(id.name.as_str()),
            Expression::ParenthesizedExpression(p) => self.lower_expr(&p.expression, exp),
            Expression::TSAsExpression(a) => {
                // `JSON.parse(text) as T`: the cast gives the type to build.
                let mut inner = &a.expression;
                while let Expression::ParenthesizedExpression(p) = inner {
                    inner = &p.expression;
                }
                // `["a", "b"] as const`: the string literals stay union members.
                if matches!(&a.type_annotation, TSType::TSTypeReference(r)
                    if matches!(&r.type_name, TSTypeName::IdentifierReference(id) if id.name == "const"))
                    && let Expression::ArrayExpression(arr) = inner
                    && !arr.elements.is_empty()
                    && arr
                        .elements
                        .iter()
                        .all(|e| matches!(e, ArrayExpressionElement::StringLiteral(_)))
                {
                    return self.lower_array_lit(arr, Some(&Ty::Arr(Box::new(Ty::Lit))));
                }
                if let Expression::CallExpression(c) = inner
                    && Self::is_json_parse(c)
                {
                    let ty = self.ty(&a.type_annotation, "JSON")?;
                    return self.lower_json_parse(c, Some(&ty));
                }
                // `"on" as Mode | undefined`, `o.mode as Mode`: a union of string literals is a
                // real type here, so the literal is lowered as a member and nullability follows
                // the asserted type.
                if let Ok(to) = self.ty(&a.type_annotation, "as")
                    && to.strip() == &Ty::Lit
                {
                    let v = self.lower_expr(&a.expression, Some(&to))?;
                    if v.ty.strip() == &Ty::Lit {
                        return self.coerce(v, &to);
                    }
                    return Ok(v);
                }
                // `null as T | null`, `[] as T[]`, `{} as T`: the asserted type is the literal's.
                if matches!(
                    inner,
                    Expression::NullLiteral(_)
                        | Expression::ArrayExpression(_)
                        | Expression::ObjectExpression(_)
                ) && let Ok(to) = self.ty(&a.type_annotation, "as")
                    && !matches!(to, Ty::Lit)
                {
                    let v = self.lower_expr(inner, Some(&to))?;
                    return Ok(self.coerce(v.clone(), &to).unwrap_or(v));
                }
                let v = self.lower_expr(&a.expression, exp)?;
                // `(table as Record<string, V>)[name]`: a record as a dictionary.
                if let Ok(to @ Ty::Map(..)) = self.ty(&a.type_annotation, "as")
                    && matches!(v.ty, Ty::Obj(_))
                {
                    return self.coerce(v, &to);
                }
                // `x as Sub` for classes: the same pointer, seen as the subclass.
                if let Ok(Ty::Obj(t)) = self.ty(&a.type_annotation, "as")
                    && self.class_of_sid.contains_key(&t)
                    && matches!(v.ty.strip(), Ty::Obj(_))
                {
                    let ty = if v.ty.is_nullable() {
                        Ty::Obj(t).nullable()
                    } else {
                        Ty::Obj(t)
                    };
                    return Ok(mk(ExprKind::Cast(Box::new(v)), ty));
                }
                // `xs.flat(depth) as number[]`: an array of a union as an array of one member.
                if let Ok(to @ Ty::Arr(_)) = self.ty(&a.type_annotation, "as")
                    && let Some(e) = self.union_array_cast(&v, &to)?
                {
                    return Ok(e);
                }
                Ok(v)
            }
            Expression::TSSatisfiesExpression(s) => {
                // `{...} as const satisfies Record<string, V>`: every property is a V.
                let mut inner = &s.expression;
                while let Expression::TSAsExpression(a) = inner {
                    inner = &a.expression;
                }
                if let (TSType::TSTypeReference(r), Expression::ObjectExpression(o)) =
                    (&s.type_annotation, inner)
                    && let (TSTypeName::IdentifierReference(n), Some(args)) =
                        (&r.type_name, &r.type_arguments)
                    && n.name == "Record"
                    && args.params.len() == 2
                {
                    let v = self.ty(&args.params[1], "Record")?;
                    return self.lower_object(o, None, Some(&v));
                }
                self.lower_expr(&s.expression, exp)
            }
            Expression::TSNonNullExpression(n) => {
                let e = self.lower_expr(&n.expression, exp)?;
                let t = e.ty.strip().clone();
                Ok(mk(ExprKind::Cast(Box::new(e)), t))
            }
            Expression::ArrowFunctionExpression(a) => {
                self.lower_closure(super::closure::ClosureNode::Arrow(a), exp)
            }
            Expression::FunctionExpression(f) => {
                self.lower_closure(super::closure::ClosureNode::Func(f), exp)
            }
            Expression::ThisExpression(_) => match self.lookup("this") {
                Some(b) => Ok(self.read_binding(&b)),
                None => bail!(
                    "{}: `this` outside a method",
                    self.loc(oxc_span::GetSpan::span(e).start)
                ),
            },
            Expression::PrivateFieldExpression(p) => {
                let obj = self.lower_expr(&p.object, None)?;
                self.member_of(obj, &classes::private_name(p.field.name.as_str()))
            }
            Expression::StaticMemberExpression(m) => self.lower_static_member(m),
            Expression::ComputedMemberExpression(m) => self.lower_computed_member(m),
            Expression::ChainExpression(c) => self.lower_chain(c),
            Expression::CallExpression(c) if Self::is_json_parse(c) => {
                self.lower_json_parse(c, exp)
            }
            Expression::CallExpression(c) => {
                self.call_exp = exp.cloned();
                self.lower_call(c)
            }
            Expression::NewExpression(n) => self.lower_new(n, exp),
            Expression::ObjectExpression(o) => self.lower_object(o, exp, None),
            Expression::ArrayExpression(a) => self.lower_array_lit(a, exp),
            Expression::UnaryExpression(u) => self.lower_unary(u),
            Expression::BinaryExpression(b) => self.lower_binary(b),
            Expression::LogicalExpression(l) => self.lower_logical(l, exp),
            Expression::ConditionalExpression(c) => {
                let test = self.lower_cond(&c.test)?;
                // `"uv" in data ? data.uv : []` with a test known now (`in` over a struct): only the
                // arm taken is lowered, so the other may name a field the type does not have.
                if let Some(taken) = Self::const_bool(&test) {
                    let arm = if taken { &c.consequent } else { &c.alternate };
                    let v = self.lower_expr(arm, exp)?;
                    let ty = v.ty.clone();
                    return Ok(mk(ExprKind::Seq(vec![Stmt::Eval(test)], Box::new(v)), ty));
                }
                let (yes, no) = if self.unions.is_empty() {
                    (vec![], vec![])
                } else {
                    (
                        self.narrowings(&c.test, true),
                        self.narrowings(&c.test, false),
                    )
                };
                // `ok ? [a, b] : []`: an empty array takes the other branch's type.
                let is_empty = |e: &Expression<'_>| matches!(e, Expression::ArrayExpression(a) if a.elements.is_empty());
                let (a, b) = if exp.is_none() && is_empty(&c.consequent) && !is_empty(&c.alternate)
                {
                    let b = self.with_narrowed(no, |s| s.lower_expr(&c.alternate, exp))?;
                    let a =
                        self.with_narrowed(yes, |s| s.lower_expr(&c.consequent, Some(&b.ty)))?;
                    (a, b)
                } else {
                    let a = self.with_narrowed(yes, |s| s.lower_expr(&c.consequent, exp))?;
                    let hint = if exp.is_none() && is_empty(&c.alternate) {
                        Some(a.ty.clone())
                    } else {
                        None
                    };
                    let b = self
                        .with_narrowed(no, |s| s.lower_expr(&c.alternate, exp.or(hint.as_ref())))?;
                    (a, b)
                };
                // `on ? "pointer" : undefined` with no union expected is a `string | undefined`.
                let (a, b) = if (a.ty == Ty::Null || b.ty == Ty::Null)
                    && exp.is_none_or(|t| t.strip() != &Ty::Lit)
                {
                    (self.widen_lit(a), self.widen_lit(b))
                } else {
                    (a, b)
                };
                // `big ? 100 : "50%"` where a `number | string` is expected: both go to the union.
                let ty = match exp.map(|t| t.strip()) {
                    Some(sv) if self.is_style_value(sv) && a.ty != b.ty => {
                        if a.ty.is_nullable()
                            || b.ty.is_nullable()
                            || a.ty == Ty::Null
                            || b.ty == Ty::Null
                        {
                            sv.clone().nullable()
                        } else {
                            sv.clone()
                        }
                    }
                    _ => match self.unify(&a.ty, &b.ty) {
                        Ok(t) => t,
                        // `show ? points.map(..) : []`: both go to the type the context wants.
                        Err(e) => match exp {
                            Some(t) => t.clone(),
                            // `ball ? "50%" : 4` as a style value with no type around it.
                            None if matches!(
                                (a.ty.strip(), b.ty.strip()),
                                (Ty::Num, Ty::Str | Ty::Lit) | (Ty::Str | Ty::Lit, Ty::Num)
                            ) && self.fids.contains_key("__sv_num") =>
                            {
                                let sv = self
                                    .m
                                    .structs
                                    .iter()
                                    .position(|d| d.name == "StyleValue")
                                    .ok_or(e)?;
                                Ty::Obj(sv)
                            }
                            // `t.plain ? t.text : <Span/>`: nodes of different kinds make a
                            // `ReactNode`.
                            None if [a.ty.strip(), b.ty.strip()].iter().all(|t| {
                                matches!(t, Ty::Str | Ty::Lit | Ty::Num | Ty::Null)
                                    || matches!(t, Ty::Handle(h) if h == "UiElement")
                                    || **t == Ty::Arr(Box::new(Ty::Handle("UiElement".into())))
                            }) && [a.ty.strip(), b.ty.strip()]
                                .iter()
                                .any(|t| matches!(t, Ty::Handle(_) | Ty::Arr(_))) =>
                            {
                                Ty::Arr(Box::new(Ty::Handle("UiElement".into())))
                            }
                            None => return Err(e),
                        },
                    },
                };
                let (a, b) = (self.coerce_ext(a, &ty)?, self.coerce_ext(b, &ty)?);
                Ok(mk(
                    ExprKind::Cond(Box::new(test), Box::new(a), Box::new(b)),
                    ty,
                ))
            }
            Expression::AssignmentExpression(a) => {
                let (mut stmts, value) = self.lower_assign(a)?;
                let ty = value.ty.clone();
                Ok(mk(
                    ExprKind::Seq(std::mem::take(&mut stmts), Box::new(value)),
                    ty,
                ))
            }
            Expression::UpdateExpression(u) => {
                let (stmts, value) = self.lower_update(u)?;
                let ty = value.ty.clone();
                Ok(mk(ExprKind::Seq(stmts, Box::new(value)), ty))
            }
            Expression::SequenceExpression(s) => {
                let mut stmts = Vec::new();
                let n = s.expressions.len();
                for e in &s.expressions[..n - 1] {
                    let x = self.lower_expr(e, None)?;
                    stmts.push(Stmt::Eval(x));
                }
                let last = self.lower_expr(&s.expressions[n - 1], exp)?;
                let ty = last.ty.clone();
                Ok(mk(ExprKind::Seq(stmts, Box::new(last)), ty))
            }
            _ => bail!(
                "{}: unsupported expression",
                self.loc(oxc_span::GetSpan::span(e).start)
            ),
        }
    }

    fn lower_ident(&mut self, name: &str) -> R<Expr> {
        if let Some(b) = self.lookup(name) {
            return Ok(self.read_binding(&b));
        }
        match self.consts.get(name).cloned() {
            Some(ConstInfo::Inline(e)) => return Ok(e),
            Some(ConstInfo::Global(g, ty, false)) => return Ok(mk(ExprKind::Global(g), ty)),
            Some(ConstInfo::Global(g, ty, true)) => return Ok(mk(ExprKind::GlobalVar(g), ty)),
            None => {}
        }
        match name {
            "Infinity" => Ok(num(f64::INFINITY)),
            "NaN" => Ok(num(f64::NAN)),
            "undefined" => Ok(mk(ExprKind::Null, Ty::Null)),
            _ if self.fids.contains_key(name) => self.function_value(name),
            _ => bail!("unknown name `{name}`"),
        }
    }

    fn lower_static_member(&mut self, m: &'a StaticMemberExpression<'a>) -> R<Expr> {
        let prop = m.property.name.as_str();
        // `Time.deltaTime`: the getter of a namespace property.
        if let Expression::Identifier(id) = &m.object
            && self.lookup(id.name.as_str()).is_none()
            && let Some(g) = self
                .manifest
                .ns_props
                .get(id.name.as_str())
                .and_then(|p| p.get(prop))
                .and_then(|p| p.getter.clone())
        {
            return Ok(self.emit_extern(&g, vec![]));
        }
        // `Camera.main`, `Screen.windowSize`: a facade getter, or a static function with no arguments.
        if let Expression::Identifier(id) = &m.object
            && self.lookup(id.name.as_str()).is_none()
            && !self.classes.contains_key(id.name.as_str())
            && let Some(e) = self.ext_static_prop(id.name.as_str(), prop)?
        {
            return Ok(e);
        }
        if let Expression::Identifier(id) = &m.object
            && id.name == "Math"
            && self.lookup("Math").is_none()
        {
            return match prop {
                "PI" => Ok(num(std::f64::consts::PI)),
                "SQRT2" => Ok(num(std::f64::consts::SQRT_2)),
                "E" => Ok(num(std::f64::consts::E)),
                "LN2" => Ok(num(std::f64::consts::LN_2)),
                "LN10" => Ok(num(std::f64::consts::LN_10)),
                "LOG2E" => Ok(num(std::f64::consts::LOG2_E)),
                "LOG10E" => Ok(num(std::f64::consts::LOG10_E)),
                "SQRT1_2" => Ok(num(std::f64::consts::FRAC_1_SQRT_2)),
                // `xs.map(Math.abs)`
                _ => match self.math_value(prop)? {
                    Some(e) => Ok(e),
                    None => bail!("Math.{prop} is not a value"),
                },
            };
        }
        if let Expression::Identifier(id) = &m.object
            && id.name == "Number"
            && self.lookup("Number").is_none()
        {
            return match prop {
                "EPSILON" => Ok(num(f64::EPSILON)),
                "MAX_SAFE_INTEGER" => Ok(num(9007199254740991.0)),
                "MIN_SAFE_INTEGER" => Ok(num(-9007199254740991.0)),
                "MAX_VALUE" => Ok(num(f64::MAX)),
                "MIN_VALUE" => Ok(num(f64::from_bits(1))),
                "POSITIVE_INFINITY" => Ok(num(f64::INFINITY)),
                "NEGATIVE_INFINITY" => Ok(num(f64::NEG_INFINITY)),
                "NaN" => Ok(num(f64::NAN)),
                _ => bail!("Number.{prop} is not a value"),
            };
        }
        // `GameObject.roots`: the objects at the world root.
        if let Expression::Identifier(id) = &m.object
            && id.name == "GameObject"
            && prop == "roots"
            && self.lookup("GameObject").is_none()
            && self.fids.contains_key("__go_roots")
        {
            return self.call_helper("__go_roots", vec![]);
        }
        if let Expression::Identifier(id) = &m.object
            && self.lookup(id.name.as_str()).is_none()
            && let Some(v) = self.enum_member(id.name.as_str(), prop)
        {
            return v;
        }
        if let Expression::Identifier(id) = &m.object
            && self.lookup(id.name.as_str()).is_none()
            && self.classes.contains_key(id.name.as_str())
        {
            // `static get current()`
            if let Some(f) = self.classes[id.name.as_str()]
                .statics
                .get(&format!("get {prop}"))
                .cloned()
            {
                return self.call_declared(&f, &[], None);
            }
            let found = self.consts.get(&format!("{}.{prop}", id.name)).cloned();
            // `const f = Class.method`
            if found.is_none()
                && let Some(e) = self.static_fn_value(id.name.as_str(), prop)?
            {
                return Ok(e);
            }
            return match found {
                Some(ConstInfo::Inline(e)) => Ok(e),
                Some(ConstInfo::Global(g, ty, false)) => Ok(mk(ExprKind::Global(g), ty)),
                Some(ConstInfo::Global(g, ty, true)) => Ok(mk(ExprKind::GlobalVar(g), ty)),
                None => bail!("class {} has no static value `{prop}`", id.name),
            };
        }
        // `const hex = Color.hex`: a static function of the engine API.
        if let Expression::Identifier(id) = &m.object
            && self.lookup(id.name.as_str()).is_none()
            && !self.consts.contains_key(id.name.as_str())
            && let Some(e) = self.static_fn_value(id.name.as_str(), prop)?
        {
            return Ok(e);
        }
        let mut obj = self.lower_expr(&m.object, None)?;
        if m.optional {
            obj = self.chain_guard(obj)?;
        }
        self.member_of(obj, prop)
    }

    /// A method (or getter) a value class got from the engine API: `a.add(b)`, `v.magnitude`.
    pub(crate) fn value_member(&self, sid: StructId, name: &str, getter: bool) -> Option<ExternFn> {
        let class = self
            .value_structs
            .iter()
            .find_map(|(n, s)| (*s == sid).then_some(n))?;
        let k = self.manifest.classes.get(class)?;
        let table = if getter { &k.getters } else { &k.methods };
        table.get(name).cloned()
    }

    /// The type of a field every member of a union of objects has with that one type (`None`
    /// when a member lacks it, is not an object, or has it with another type).
    fn common_field(&self, members: &[Ty], prop: &str) -> Option<Ty> {
        let mut ty: Option<Ty> = None;
        for m in members {
            let Ty::Obj(s) = m.strip() else {
                return None;
            };
            let d = &self.m.structs[*s];
            let f = &d.fields[d.field(prop)?];
            match &ty {
                None => ty = Some(f.ty.clone()),
                Some(t) if *t == f.ty => {}
                Some(_) => return None,
            }
        }
        ty
    }

    /// `obj.prop` on an already-lowered object.
    pub(crate) fn member_of(&mut self, obj: Expr, prop: &str) -> R<Expr> {
        // A string literal (or a constant of one) used as a `string`: `"abc".length`.
        let obj = self.widen_lit(obj);
        match obj.ty.strip().clone() {
            Ty::Obj(sid) if self.is_dyn(sid) => self.iface_member(obj, sid, prop),
            // `pair.length` of a tuple is its number of elements.
            Ty::Obj(sid) if prop == "length" && self.is_tuple(sid) => {
                let n = self.m.structs[sid].fields.len();
                Ok(mk(
                    ExprKind::Seq(vec![Stmt::Eval(obj)], Box::new(num(n as f64))),
                    Ty::Num,
                ))
            }
            Ty::Obj(sid) => {
                let Some(i) = self.m.structs[sid].field(prop) else {
                    if Some(sid) == self.match_sid() && prop == "length" {
                        let view = self.match_view(obj);
                        return Ok(mk(ExprKind::Len(Box::new(view)), Ty::Num));
                    }
                    if let Some(g) = self.value_member(sid, prop, true) {
                        return Ok(self.emit_extern(&g, vec![obj]));
                    }
                    if let Some((stat, target)) = self.target_for(sid, Which::Getter, prop) {
                        return self.call_getter(&stat, &target, obj);
                    }
                    if self.resolve_in(sid, Which::Method, prop).is_some() {
                        return self.bound_method(obj, sid, prop);
                    }
                    // Only a subclass has it: tsc narrowed the object, so use it as that class.
                    let sub = self.downcast_for(sid, |s, c| {
                        s.has_field(c, prop)
                            || s.resolve_in(c, Which::Getter, prop).is_some()
                            || s.resolve_in(c, Which::Method, prop).is_some()
                    });
                    if let Some(sub) = sub {
                        return self.member_of(Self::downcast(obj, sub), prop);
                    }
                    if prop == "length"
                        && let Some(e) = self.union_length(obj.clone())
                    {
                        return Ok(e);
                    }
                    // `node.id` of a `DockGroup | DockSplit`: a field every member has, of one
                    // type, read from whichever member it is.
                    if let Some(members) = self.unions.get(&sid).cloned()
                        && let Some(ty) = self.common_field(&members, prop)
                    {
                        return self.union_map(obj, ty, |s, m| s.member_of(m, prop));
                    }
                    bail!("{} has no field `{prop}`", self.m.structs[sid].name)
                };
                Ok(self.thunked_read(obj, sid, i, prop))
            }
            Ty::Arr(_) | Ty::Typed(_) if prop == "length" => {
                Ok(mk(ExprKind::Len(Box::new(obj)), Ty::Num))
            }
            Ty::Typed(_) if prop == "byteOffset" => {
                Ok(self.rt_str("ts_rt_typed_byte_offset", vec![obj], Ty::Num))
            }
            Ty::Typed(k) if prop == "buffer" => {
                let bytes = self.rt_str(
                    "ts_rt_typed_bytes_view_gc",
                    vec![obj, num(f64::from(k.code()))],
                    Ty::Typed(crate::ir::TypedKind::U8),
                );
                self.call_helper("__wrapBuffer", vec![bytes])
            }
            Ty::Typed(k) if prop == "byteLength" => {
                let len = mk(ExprKind::Len(Box::new(obj)), Ty::Num);
                Ok(mk(
                    ExprKind::Bin(BinOp::Mul, Box::new(len), Box::new(num(k.size() as f64))),
                    Ty::Num,
                ))
            }
            Ty::Str if prop == "length" => Ok(self.str_length(obj)),
            Ty::Map(..) | Ty::Set(_) if prop == "size" => Ok(self.collection_size(obj)),
            // `m.groups.year`: a regular expression's named groups are read by name.
            Ty::Map(k, v) if *k == Ty::Str && (v.is_ptr() || *v == Ty::Num || *v == Ty::Lit) => {
                self.map_prop(obj, prop)
            }
            Ty::Handle(class) => {
                if let Some(e) = self.ext_getter(&class, prop, &obj)? {
                    return Ok(e);
                }
                let getter = self
                    .manifest
                    .classes
                    .get(&class)
                    .and_then(|k| k.getters.get(prop))
                    .cloned();
                match getter {
                    Some(g) => Ok(self.emit_extern(&g, vec![obj])),
                    None if class == "GameObject" => self.game_object_member(obj, prop),
                    None => bail!("{class} has no property `{prop}`"),
                }
            }
            t => bail!("no property `{prop}` on {t}"),
        }
    }

    fn lower_computed_member(&mut self, m: &'a ComputedMemberExpression<'a>) -> R<Expr> {
        let mut arr = self.lower_expr(&m.object, None)?;
        // `dict[key]` of a `Record<string, V>`: the entry, or undefined.
        if let Ty::Map(k, _) = arr.ty.strip().clone()
            && *k == Ty::Str
            && !m.optional
        {
            let key = self.lower_expr(&m.expression, Some(&Ty::Str))?;
            let key = self.widen_lit(key);
            return self.map_get_value(arr, key, "a Record<string, V> lookup");
        }
        // `names[kind]` where `names` is a `Record<Kind, V>` and `kind` a union member.
        if let Ty::Obj(sid) = arr.ty.strip().clone()
            && Some(sid) != self.match_sid()
            && !m.optional
        {
            // `pair[1]` on a tuple.
            if self.is_tuple(sid)
                && let Expression::NumericLiteral(n) = &m.expression
                && n.value >= 0.0
                && (n.value as usize) < self.m.structs[sid].fields.len()
            {
                let i = n.value as usize;
                let ty = self.m.structs[sid].fields[i].ty.clone();
                return Ok(mk(ExprKind::Field(Box::new(arr), sid, i), ty));
            }
            let key = self.lower_expr(&m.expression, None)?;
            if matches!(key.ty, Ty::Lit | Ty::Str) && !self.m.structs[sid].fields.is_empty() {
                let idxs = self.key_indices(sid, &m.expression)?;
                return self.record_index_over(arr, sid, key, &idxs);
            }
            bail!("cannot index an object by {}", key.ty)
        }
        // `re.exec(s)?.[0]`: the null test comes before reading the match's groups.
        if m.optional {
            arr = self.chain_guard(arr)?;
            if Some(arr.ty.strip()) == self.match_sid().map(Ty::Obj).as_ref()
                && arr.ty.is_nullable()
            {
                let t = arr.ty.strip().clone();
                arr = mk(ExprKind::Cast(Box::new(arr)), t);
            }
        }
        arr = self.match_view(arr);
        // The guarded object goes into a local first, so the null test (which leaves the chain)
        // runs as a statement before the index is read.
        if m.optional {
            let tmp = self.new_local(arr.ty.clone());
            let read = self.lower_index_of(self.var_expr(tmp), m)?;
            let ty = read.ty.clone();
            return Ok(mk(
                ExprKind::Seq(vec![Stmt::Assign(tmp, arr)], Box::new(read)),
                ty,
            ));
        }
        self.lower_index_of(arr, m)
    }

    /// `arr[i]` / `s[i]` once the object is lowered (and a match array is its list).
    fn lower_index_of(&mut self, arr: Expr, m: &'a ComputedMemberExpression<'a>) -> R<Expr> {
        // `s[i]`: the character, or undefined past the ends.
        let arr = self.widen_lit(arr);
        if arr.ty.strip() == &Ty::Str {
            let idx = self.lower_expr(&m.expression, Some(&Ty::Num))?;
            let idx = self.as_num(idx)?;
            let (ts, ti) = (self.new_local(arr.ty.clone()), self.new_local(Ty::Num));
            let (sv, iv) = (self.var_expr(ts), self.var_expr(ti));
            let ch = self.rt_str("ts_rt_str_at_gc", vec![sv, iv.clone()], Ty::Str.nullable());
            let negative = mk(
                ExprKind::Cmp(CmpOp::Lt, Box::new(iv), Box::new(num(0.0))),
                Ty::Bool,
            );
            let none = mk(ExprKind::Null, Ty::Str.nullable());
            return Ok(mk(
                ExprKind::Seq(
                    vec![Stmt::Assign(ts, arr), Stmt::Assign(ti, idx)],
                    Box::new(mk(
                        ExprKind::Cond(Box::new(negative), Box::new(none), Box::new(ch)),
                        Ty::Str.nullable(),
                    )),
                ),
                Ty::Str.nullable(),
            ));
        }
        let idx = self.lower_expr(&m.expression, Some(&Ty::Num))?;
        let idx = self.as_num(idx)?;
        let ty = match arr.ty.strip() {
            Ty::Typed(_) => Ty::Num,
            Ty::Arr(el) => match &**el {
                // Out of range reads null (a zero pointer, or the null NaN for numbers).
                e if e.is_ptr() || *e == Ty::Num || matches!(e.strip(), Ty::Handle(_)) => {
                    e.clone().nullable()
                }
                e => e.clone(),
            },
            t => bail!("cannot index {t}"),
        };
        Ok(mk(ExprKind::Index(Box::new(arr), Box::new(idx)), ty))
    }

    /// `rec[key]` for a record struct: the field whose name is the key's tag.
    /// The fields a key written as `key_ast` can name: those of its union of string literals when
    /// it is a parameter written as one, else all of them (but not accessor thunks or a union's slots).
    fn key_indices(&self, sid: StructId, key_ast: &Expression<'_>) -> R<Vec<usize>> {
        let fields = &self.m.structs[sid].fields;
        let mut idxs: Vec<usize> = (0..fields.len())
            .filter(|i| !fields[*i].name.starts_with("_p_") && !fields[*i].name.starts_with('$'))
            .collect();
        if let Expression::Identifier(id) = key_ast
            && let Some(ms) = self.cur.lit_sets.get(id.name.as_str())
        {
            idxs.retain(|i| ms.contains(&fields[*i].name));
        }
        if idxs.is_empty() {
            bail!(
                "{}: the key names none of the fields",
                self.loc(oxc_span::GetSpan::span(key_ast).start)
            )
        }
        Ok(idxs)
    }

    /// `rec[key]`: the field (among `idxs`) whose name is the key; the union of their types when
    /// they differ (`o[k]` for `k: "n" | "s"` is `number | string`, as in TypeScript).
    fn record_index_over(
        &mut self,
        obj: Expr,
        sid: StructId,
        key: Expr,
        idxs: &[usize],
    ) -> R<Expr> {
        let fields = self.m.structs[sid].fields.clone();
        let tys: Vec<Ty> = idxs.iter().map(|i| fields[*i].ty.clone()).collect();
        let ty = if tys.iter().all(|t| *t == tys[0]) {
            tys[0].clone()
        } else {
            self.union_nullable(tys, "")?
        };
        let by_str = key.ty.strip() == &Ty::Str;
        let (to, tk) = (
            self.new_local(obj.ty.clone()),
            self.new_local(if by_str { Ty::Str } else { Ty::Lit }),
        );
        let (ov, kv) = (self.var_expr(to), self.var_expr(tk));
        let field = |this: &mut Self, i: usize| -> R<Expr> {
            let f = mk(
                ExprKind::Field(Box::new(ov.clone()), sid, i),
                fields[i].ty.clone(),
            );
            this.coerce(f, &ty)
        };
        let mut acc = field(self, *idxs.last().expect("one field at least"))?;
        for &i in idxs[..idxs.len() - 1].iter().rev() {
            let name = fields[i].name.clone();
            let want = if by_str {
                self.str_lit(&name)
            } else {
                let t = self.tag(&name);
                mk(ExprKind::Tag(t), Ty::Lit)
            };
            let is = mk(
                ExprKind::Cmp(CmpOp::Eq, Box::new(kv.clone()), Box::new(want)),
                Ty::Bool,
            );
            acc = mk(
                ExprKind::Cond(Box::new(is), Box::new(field(self, i)?), Box::new(acc)),
                ty.clone(),
            );
        }
        Ok(mk(
            ExprKind::Seq(
                vec![Stmt::Assign(to, obj), Stmt::Assign(tk, key)],
                Box::new(acc),
            ),
            ty,
        ))
    }

    /// For `a?.b`: evaluates `a` once; if it is null the whole chain yields null.
    fn chain_guard(&mut self, obj: Expr) -> R<Expr> {
        let Some(&(label, res)) = self.cur.chain.last() else {
            bail!("optional access outside a chain")
        };
        let tmp = self.new_local(obj.ty.clone());
        let t = self.var_expr(tmp);
        let null = mk(ExprKind::Null, Ty::Null);
        let check = Stmt::If(
            mk(ExprKind::IsNull(Box::new(t.clone())), Ty::Bool),
            vec![Stmt::Assign(res, null), Stmt::Break(label)],
            vec![],
        );
        let ty = t.ty.clone();
        Ok(mk(
            ExprKind::Seq(vec![Stmt::Assign(tmp, obj), check], Box::new(t)),
            ty,
        ))
    }

    fn lower_chain(&mut self, c: &'a ChainExpression<'a>) -> R<Expr> {
        let res = self.new_local(Ty::Void);
        let label = self.new_label();
        self.cur.chain.push((label, res));
        let inner = match &c.expression {
            ChainElement::StaticMemberExpression(m) => self.lower_static_member(m),
            ChainElement::ComputedMemberExpression(m) => self.lower_computed_member(m),
            ChainElement::CallExpression(c) => self.lower_call(c),
            ChainElement::TSNonNullExpression(n) => {
                let e = self.lower_expr(&n.expression, None)?;
                let t = e.ty.strip().clone();
                Ok(mk(ExprKind::Cast(Box::new(e)), t))
            }
            _ => bail!("unsupported optional chain"),
        };
        self.cur.chain.pop();
        // A union member that may be missing is a `string | undefined` (a tag has no null).
        let inner = self.widen_lit(inner?);
        let ty = inner.ty.clone().nullable();
        self.cur.locals[res as usize] = ty.clone();
        let v = self.coerce(inner, &ty)?;
        let block = Stmt::Block(label, vec![Stmt::Assign(res, v)]);
        Ok(mk(
            ExprKind::Seq(vec![block], Box::new(self.var_expr(res))),
            ty,
        ))
    }

    fn lower_unary(&mut self, u: &'a UnaryExpression<'a>) -> R<Expr> {
        if u.operator == UnaryOperator::Delete {
            return self.lower_delete(&u.argument, u.span.start);
        }
        if u.operator == UnaryOperator::Typeof {
            return self.lower_typeof(&u.argument);
        }
        if u.operator == UnaryOperator::LogicalNot {
            let b = self.lower_cond(&u.argument)?;
            return Ok(mk(ExprKind::Un(UnOp::Not, Box::new(b)), Ty::Bool));
        }
        let a = self.lower_expr(&u.argument, None)?;
        match u.operator {
            UnaryOperator::UnaryNegation if a.ty.strip() == &Ty::BigInt => {
                Ok(self.bigint_unary(true, a))
            }
            UnaryOperator::BitwiseNot if a.ty.strip() == &Ty::BigInt => {
                Ok(self.bigint_unary(false, a))
            }
            UnaryOperator::UnaryPlus if a.ty.strip() == &Ty::BigInt => {
                bail!(
                    "{}: unary + on a bigint (JavaScript throws a TypeError); use Number(x)",
                    self.loc(u.span.start)
                )
            }
            UnaryOperator::UnaryNegation => {
                let a = self.as_num(a)?;
                Ok(match a.kind {
                    ExprKind::Num(n) => num(-n),
                    _ => mk(ExprKind::Un(UnOp::Neg, Box::new(a)), Ty::Num),
                })
            }
            UnaryOperator::UnaryPlus => self.as_num(a),
            // ~x is ToInt32(x) ^ -1.
            UnaryOperator::BitwiseNot => self.arith(BinOp::BitXor, a, num(-1.0)),
            // `void f()`: the call, then nothing.
            UnaryOperator::Void => {
                let unit = mk(ExprKind::Bool(false), Ty::Void);
                Ok(mk(
                    ExprKind::Seq(vec![Stmt::Eval(a)], Box::new(unit)),
                    Ty::Void,
                ))
            }
            _ => bail!("{}: unsupported unary operator", self.loc(u.span.start)),
        }
    }

    /// `Namespace.fn(args)` for a function of the engine API: a direct call of its symbol, with
    /// the arguments checked against the manifest and numbers converted at the call.
    /// The struct of a manifest value class (`Vector3`): one `number` field per field, in order.
    pub(crate) fn value_struct(&mut self, name: &str) -> StructId {
        if let Some(s) = self.value_structs.get(name) {
            return *s;
        }
        let fields: Vec<Field> = self.manifest.values[name]
            .iter()
            .map(|(n, _)| Field {
                name: n.clone(),
                ty: Ty::Num,
                optional: false,
            })
            .collect();
        let sid = self.new_struct(name, fields);
        self.value_structs.insert(name.to_string(), sid);
        self.named.insert(name.to_string(), Ty::Obj(sid));
        sid
    }

    /// The IR type scripts see for a boundary type.
    pub(crate) fn ext_ir(&mut self, t: &ExtTy) -> Ty {
        match t {
            ExtTy::Value(n) => Ty::Obj(self.value_struct(n)),
            ExtTy::ValueArr(n) => Ty::Arr(Box::new(Ty::Obj(self.value_struct(n)))),
            ExtTy::ValueList(n) => Ty::Arr(Box::new(Ty::Obj(self.value_struct(n)))),
            ExtTy::Record(n) => Ty::Obj(self.record_struct(n)),
            ExtTy::RecordList(n) => Ty::Arr(Box::new(Ty::Obj(self.record_struct(n)))),
            ExtTy::RecordsJson(n) => Ty::Arr(Box::new(Ty::Obj(self.record_struct(n)))),
            ExtTy::Opt(inner) => self.ext_ir(inner).nullable(),
            other => other.to_ir(),
        }
    }

    /// The struct of a result record (`SplineHit`): a plain object with the manifest's fields.
    pub(crate) fn record_struct(&mut self, name: &str) -> StructId {
        if let Some(Ty::Obj(s)) = self.named.get(name) {
            return *s;
        }
        let fields: Vec<(String, crate::manifest::RecField)> = self.manifest.records[name].clone();
        let sid = self.new_struct(name, vec![]);
        self.named.insert(name.to_string(), Ty::Obj(sid));
        let fields = fields
            .into_iter()
            .map(|(n, f)| Field {
                name: n,
                optional: matches!(f, crate::manifest::RecField::Opt(_)),
                ty: self.rec_field_ty(&f),
            })
            .collect();
        self.m.structs[sid].fields = fields;
        sid
    }

    /// The IR type of a record field.
    pub(crate) fn rec_field_ty(&mut self, f: &crate::manifest::RecField) -> Ty {
        use crate::manifest::RecField;
        match f {
            RecField::Num => Ty::Num,
            RecField::Bool => Ty::Bool,
            RecField::Value(v) => Ty::Obj(self.value_struct(v)),
            RecField::Enum(_) | RecField::Str => Ty::Str,
            RecField::Opt(inner) => self.rec_field_ty(inner).nullable(),
            // `[number, number][]`: a tuple of equal types is an array.
            RecField::Pairs => Ty::Arr(Box::new(Ty::Arr(Box::new(Ty::Num)))),
            RecField::Handle(h) => Ty::Handle(h.clone()),
            RecField::HandleList(h) => Ty::Arr(Box::new(Ty::Handle(h.clone()))),
            RecField::Numbers => Ty::Arr(Box::new(Ty::Num)),
            RecField::Records(r) => Ty::Arr(Box::new(Ty::Obj(self.record_struct(r)))),
            RecField::Record(r) => Ty::Obj(self.record_struct(r)),
            RecField::Dict => Ty::Map(Box::new(Ty::Str), Box::new(Ty::Num)),
        }
    }

    /// The version of a setter that takes a list, when the setter has one (`startLifetime` has a
    /// number and a `[min, max]` version).
    fn list_setter_of(&self, setter: &ExternFn) -> Option<ExternFn> {
        self.manifest
            .classes
            .values()
            .flat_map(|k| k.overloads.iter())
            .filter(|((table, _), _)| table == "set")
            .map(|(_, versions)| versions)
            .find(|versions| versions.iter().any(|f| f.symbol == setter.symbol))
            .and_then(|versions| {
                versions
                    .iter()
                    .find(|f| f.params.get(1).is_some_and(|p| p.1.is_list_arg()))
                    .cloned()
            })
    }

    /// Whether a record's numbers can come back flat (no optional key, no list).
    pub(crate) fn record_is_flat(&self, rn: &str) -> bool {
        self.manifest.records[rn].iter().all(|(_, f)| f.is_flat())
    }

    /// `ts_api_result(i)`: the `i`-th number of the last list or record result.
    fn flat_result(&mut self, i: Expr) -> Expr {
        let f = ExternFn {
            rest: false,
            optional_from: usize::MAX,
            defaults: Vec::new(),
            symbol: "ts_api_result".into(),
            params: vec![("i".into(), ExtTy::U32)],
            ret: ExtTy::F64,
        };
        self.emit_extern(&f, vec![i])
    }

    /// The index of an enum name, for the wire: folded at compile time for a string literal, else
    /// `Enum.parse(s)`. A name that is not one of them is -1, which the engine reads as the first.
    fn enum_index(&mut self, en: &str, v: Expr) -> Expr {
        let text: Option<Vec<u8>> = match &v.kind {
            ExprKind::Str(i) => Some(self.m.strs[*i as usize].clone()),
            ExprKind::TagStr(t) => match &t.kind {
                ExprKind::Tag(k) => Some(self.m.tags[*k as usize].clone().into_bytes()),
                _ => None,
            },
            _ => None,
        };
        if let Some(text) = text {
            let at = self.manifest.enums[en]
                .values
                .iter()
                .position(|n| n.as_bytes() == text.as_slice());
            return num(at.map_or(-1.0, |i| i as f64));
        }
        let parse = self.manifest.namespaces[en]["parse"].clone();
        self.emit_extern(&parse, vec![v])
    }

    /// The object of a value class or record whose numbers start at `base` in the result buffer.
    /// `Record(name)` builds the record's struct (a value inside it, or an enum name); a value
    /// class is the fields in a row.
    fn read_flat_object(
        &mut self,
        rec: &ExtTy,
        base: &Expr,
        str_base: &Expr,
        stmts: &mut Vec<Stmt>,
    ) -> Expr {
        let add = |a: &Expr, k: usize| -> Expr {
            if k == 0 {
                a.clone()
            } else {
                mk(
                    ExprKind::Bin(BinOp::Add, Box::new(a.clone()), Box::new(num(k as f64))),
                    Ty::Num,
                )
            }
        };
        match rec {
            ExtTy::ValueList(vn) | ExtTy::Value(vn) => {
                let sid = self.value_struct(vn);
                let ty = Ty::Obj(sid);
                let obj = self.new_local(ty.clone());
                stmts.push(Stmt::Assign(obj, mk(ExprKind::NewObj(sid), ty.clone())));
                for k in 0..self.m.structs[sid].fields.len() {
                    let e = self.flat_result(add(base, k));
                    stmts.push(Stmt::StoreField(self.var_expr(obj), sid, k, e));
                }
                self.var_expr(obj)
            }
            ExtTy::RecordList(rn) | ExtTy::Record(rn) => {
                let fields = self.manifest.records[rn].clone();
                let sid = self.record_struct(rn);
                let ty = Ty::Obj(sid);
                let obj = self.new_local(ty.clone());
                stmts.push(Stmt::Assign(obj, mk(ExprKind::NewObj(sid), ty.clone())));
                let mut off = 0;
                let mut soff = 0;
                for (k, (_, f)) in fields.iter().enumerate() {
                    let value = match f {
                        // A string: the `soff`-th of the result's strings.
                        crate::manifest::RecField::Str => {
                            let e = self.str_result(add(str_base, soff));
                            soff += 1;
                            e
                        }
                        crate::manifest::RecField::Num => {
                            let e = self.flat_result(add(base, off));
                            off += 1;
                            e
                        }
                        crate::manifest::RecField::Bool => {
                            let e = self.flat_result(add(base, off));
                            off += 1;
                            mk(ExprKind::Truthy(Box::new(e)), Ty::Bool)
                        }
                        crate::manifest::RecField::Value(vn) => {
                            let width = self.manifest.values[vn].len();
                            let inner = self.read_flat_object(
                                &ExtTy::Value(vn.clone()),
                                &add(base, off),
                                str_base,
                                stmts,
                            );
                            off += width;
                            inner
                        }
                        crate::manifest::RecField::Opt(_)
                        | crate::manifest::RecField::Pairs
                        | crate::manifest::RecField::Handle(_)
                        | crate::manifest::RecField::HandleList(_)
                        | crate::manifest::RecField::Numbers
                        | crate::manifest::RecField::Records(_)
                        | crate::manifest::RecField::Record(_)
                        | crate::manifest::RecField::Dict => {
                            unreachable!(
                                "a record with optional keys or lists is not a flat result"
                            )
                        }
                        crate::manifest::RecField::Enum(en) => {
                            let idx = self.flat_result(add(base, off));
                            off += 1;
                            let name = self.manifest.namespaces[en.as_str()]["name"].clone();
                            self.emit_extern(&name, vec![idx])
                        }
                    };
                    stmts.push(Stmt::StoreField(self.var_expr(obj), sid, k, value));
                }
                self.var_expr(obj)
            }
            other => unreachable!("read_flat_object({other:?})"),
        }
    }

    /// `ts_api_result_str(i)`: the `i`-th string of the last result.
    fn str_result(&mut self, i: Expr) -> Expr {
        let item = ExternFn {
            rest: false,
            optional_from: usize::MAX,
            defaults: Vec::new(),
            symbol: "ts_api_result_str".into(),
            params: vec![("i".into(), ExtTy::U32)],
            ret: ExtTy::Str,
        };
        self.emit_extern(&item, vec![i])
    }

    /// How many strings one element of a list (or one record) takes in the result's string list.
    fn str_width(&self, t: &ExtTy) -> usize {
        match t {
            ExtTy::RecordList(rn) | ExtTy::Record(rn) => self.manifest.records[rn]
                .iter()
                .filter(|(_, f)| matches!(f, crate::manifest::RecField::Str))
                .count(),
            _ => 0,
        }
    }

    /// How many numbers one element of a list (or one record) takes in the result buffer.
    fn flat_width(&self, t: &ExtTy) -> usize {
        match t {
            ExtTy::ValueList(vn) | ExtTy::Value(vn) => self.manifest.values[vn].len(),
            ExtTy::RecordList(rn) | ExtTy::Record(rn) => self.manifest.records[rn]
                .iter()
                .map(|(_, f)| match f {
                    crate::manifest::RecField::Value(vn) => self.manifest.values[vn].len(),
                    crate::manifest::RecField::Str => 0,
                    _ => 1,
                })
                .sum(),
            _ => 0,
        }
    }

    /// `new Vector3(x, y, z)`: the fields in order.
    pub(crate) fn new_value(&mut self, name: &str, args: &'a [Argument<'a>], at: u32) -> R<Expr> {
        // A value class with a static `create` is built by it (`new DateTime(2024, 5, 3)`).
        if let Some(create) = self
            .manifest
            .namespaces
            .get(name)
            .and_then(|n| n.get("create"))
            .cloned()
        {
            return self.call_ext_args(create, None, &format!("new {name}"), at, args);
        }
        let sid = self.value_struct(name);
        let n = self.m.structs[sid].fields.len();
        // `new Color(...rgba)`: the fields from the elements of an array or tuple.
        if args.len() == 1
            && let Argument::SpreadElement(sp) = &args[0]
        {
            let arr = self.lower_expr(&sp.argument, None)?;
            let ty = Ty::Obj(sid);
            let tmp = self.new_local(ty.clone());
            let src = self.new_local(arr.ty.clone());
            let mut stmts = vec![
                Stmt::Assign(tmp, mk(ExprKind::NewObj(sid), ty.clone())),
                Stmt::Assign(src, arr.clone()),
            ];
            for i in 0..n {
                // A missing element is 0 (a colour's alpha is 1).
                let default = if name == "Color" && i == 3 { 1.0 } else { 0.0 };
                let v = match arr.ty.strip().clone() {
                    Ty::Obj(t) if self.is_tuple(t) => {
                        if i < self.m.structs[t].fields.len() {
                            let fty = self.m.structs[t].fields[i].ty.clone();
                            let read = mk(ExprKind::Field(Box::new(self.var_expr(src)), t, i), fty);
                            self.coerce(read, &Ty::Num)?
                        } else {
                            num(default)
                        }
                    }
                    Ty::Arr(el) if *el == Ty::Num => {
                        let at = mk(
                            ExprKind::Index(Box::new(self.var_expr(src)), Box::new(num(i as f64))),
                            Ty::Num.nullable(),
                        );
                        let t = self.new_local(Ty::Num.nullable());
                        let tv = self.var_expr(t);
                        let is_null = mk(ExprKind::IsNull(Box::new(tv.clone())), Ty::Bool);
                        mk(
                            ExprKind::Seq(
                                vec![Stmt::Assign(t, at)],
                                Box::new(mk(
                                    ExprKind::Cond(
                                        Box::new(is_null),
                                        Box::new(num(default)),
                                        Box::new(mk(ExprKind::Cast(Box::new(tv)), Ty::Num)),
                                    ),
                                    Ty::Num,
                                )),
                            ),
                            Ty::Num,
                        )
                    }
                    t => bail!("{}: new {name}(...{t})", self.loc(at)),
                };
                stmts.push(Stmt::StoreField(self.var_expr(tmp), sid, i, v));
            }
            return Ok(mk(ExprKind::Seq(stmts, Box::new(self.var_expr(tmp))), ty));
        }
        // `new Color(r, g, b)`: opaque.
        if name == "Color" && args.len() == 3 {
            let ty = Ty::Obj(sid);
            let tmp = self.new_local(ty.clone());
            let mut stmts = vec![Stmt::Assign(tmp, mk(ExprKind::NewObj(sid), ty.clone()))];
            for (i, a) in args.iter().enumerate() {
                let Some(e) = a.as_expression() else {
                    bail!("new {name}: spread argument")
                };
                let v = self.lower_expr(e, Some(&Ty::Num))?;
                let v = self.as_num(v)?;
                stmts.push(Stmt::StoreField(self.var_expr(tmp), sid, i, v));
            }
            stmts.push(Stmt::StoreField(self.var_expr(tmp), sid, 3, num(1.0)));
            return Ok(mk(ExprKind::Seq(stmts, Box::new(self.var_expr(tmp))), ty));
        }
        if args.len() != n {
            bail!(
                "{}: new {name}: {} arguments, {n} expected",
                self.loc(at),
                args.len()
            )
        }
        let ty = Ty::Obj(sid);
        let tmp = self.new_local(ty.clone());
        let mut stmts = vec![Stmt::Assign(tmp, mk(ExprKind::NewObj(sid), ty.clone()))];
        for (i, a) in args.iter().enumerate() {
            let Some(e) = a.as_expression() else {
                bail!("new {name}: spread argument")
            };
            let v = self.lower_expr(e, Some(&Ty::Num))?;
            let v = self.as_num(v)?;
            stmts.push(Stmt::StoreField(self.var_expr(tmp), sid, i, v));
        }
        Ok(mk(ExprKind::Seq(stmts, Box::new(self.var_expr(tmp))), ty))
    }

    /// `Namespace.fn(args)` for a function of the engine API: a direct call of its symbol, with
    /// the arguments checked against the manifest and numbers converted at the call. A value
    /// class argument is passed as its fields; a value class result is written into a new object.
    fn lower_extern_call(&mut self, ns: &str, name: &str, c: &'a CallExpression<'a>) -> R<Expr> {
        // `X.name$n(..)` inside the facade is the native function of that name.
        let name = name.strip_suffix("$n").unwrap_or(name);
        if self.manifest.is_host_object(ns)
            && self.manifest.is_host_method(name, |h| &h.find_any)
            && let Some(e) = self.lower_find_object_of_type(c)?
        {
            return Ok(e);
        }
        if ns == "Ui" && name == "load" {
            return self.lower_ui_load(c);
        }
        if ns == "Ui"
            && matches!(name, "store" | "stateNew" | "dep")
            && let Some(e) = self.lower_ui_handle_slot(name, c)?
        {
            return Ok(e);
        }
        if ns == "Ui" && name == "runEffect" {
            return self.lower_ui_run_effect(c);
        }
        if ns == "Ui" && name == "ref" {
            return self.lower_ui_ref(c);
        }
        if ns == "Ui" && (name == "styleObj" || name == "attrObj") {
            return self.lower_ui_fields(name == "attrObj", c);
        }
        if ns == "Ui"
            && (name == "child" || name == "key")
            && let Some(e) = self.lower_ui_opt(name, c)?
        {
            return Ok(e);
        }
        if ns == "Ui"
            && (name == "style" || name == "attr")
            && let Some(e) = self.lower_ui_set(name == "attr", c)?
        {
            return Ok(e);
        }
        if let Some(cands) = self
            .manifest
            .overloads
            .get(&(ns.to_string(), name.to_string()))
            .cloned()
        {
            return self.lower_overloaded(ns, name, &cands, c);
        }
        // A namespace named like a handle class (`AnimationCurve`) also holds the class statics.
        let found = self.manifest.namespaces[ns].get(name).cloned().or_else(|| {
            self.manifest
                .classes
                .get(ns)
                .and_then(|k| k.statics.get(name))
                .cloned()
        });
        let Some(mut f) = found else {
            bail!("{}: {ns} has no function `{name}`", self.loc(c.span.start))
        };
        // A class static declared more than once: the version for these arguments.
        if !self.manifest.namespaces[ns].contains_key(name)
            && let Some(cands) = self
                .manifest
                .classes
                .get(ns)
                .and_then(|k| k.overloads.get(&("static".to_string(), name.to_string())))
                .cloned()
        {
            f = self.pick_ext_overload(&cands, 0, &c.arguments);
        }
        self.call_ext_fn(f, None, &format!("{ns}.{name}"), c)
    }

    /// How an argument's type is passed to the boundary, for choosing among overloads.
    fn ext_class(&self, t: &Ty) -> Option<ExtTy> {
        Some(match t {
            Ty::Num => ExtTy::F64,
            Ty::Bool => ExtTy::Bool,
            Ty::Str | Ty::Lit => ExtTy::Str,
            Ty::Handle(n) => ExtTy::Handle(n.clone()),
            Ty::Arr(e) if matches!(&**e, Ty::Handle(_)) => {
                let Ty::Handle(n) = &**e else { unreachable!() };
                ExtTy::HandleArr(n.clone())
            }
            Ty::Arr(e) if **e == Ty::Num => ExtTy::F64Arr,
            Ty::Arr(e) if matches!(e.strip(), Ty::Str) => ExtTy::StrArr,
            Ty::Arr(e)
                if matches!(&**e, Ty::Obj(sid)
                    if self.manifest.values.contains_key(&self.m.structs[*sid].name)) =>
            {
                let Ty::Obj(sid) = &**e else { unreachable!() };
                ExtTy::ValueArr(self.m.structs[*sid].name.clone())
            }
            Ty::Nullable(inner) => match &**inner {
                Ty::Num => ExtTy::F64,
                Ty::Handle(n) => ExtTy::Handle(n.clone()),
                // A list that may be missing (`children?: ReactNode`): null is an empty list.
                Ty::Arr(e) if matches!(&**e, Ty::Handle(_)) => {
                    let Ty::Handle(n) = &**e else { unreachable!() };
                    ExtTy::HandleArr(n.clone())
                }
                other if other.is_ptr() => ExtTy::Ptr,
                _ => return None,
            },
            // A value class (`Vector3`) is passed as itself.
            Ty::Obj(sid)
                if self
                    .manifest
                    .values
                    .contains_key(&self.m.structs[*sid].name) =>
            {
                ExtTy::Value(self.m.structs[*sid].name.clone())
            }
            other if other.is_ptr() => ExtTy::Ptr,
            _ => return None,
        })
    }

    /// The version of a class function declared more than once (`addState(name, clip)` and
    /// `addState(name, { blendParameter, children })`) that takes this call's arguments: the
    /// first whose parameters (after `skip` leading ones, the receiver) fit the arguments by
    /// count and type. The first version when none does, so its error is the one reported.
    pub(crate) fn pick_ext_overload(
        &mut self,
        cands: &[ExternFn],
        skip: usize,
        args: &'a [Argument<'a>],
    ) -> ExternFn {
        let exprs: Vec<&'a Expression<'a>> =
            args.iter().filter_map(|a| a.as_expression()).collect();
        if exprs.len() == args.len() {
            for f in cands {
                let params = &f.params[skip.min(f.params.len())..];
                let required = f.optional_from.saturating_sub(skip).min(params.len());
                if exprs.len() > params.len() || exprs.len() < required {
                    continue;
                }
                let fits = exprs
                    .iter()
                    .zip(params)
                    .all(|(e, (_, pt))| self.arg_fits(e, pt));
                if fits {
                    return f.clone();
                }
            }
        }
        cands[0].clone()
    }

    /// Whether the argument `e` can go where the boundary type `pt` is expected, by its static type
    /// (an object literal fits a record).
    fn arg_fits(&mut self, e: &'a Expression<'a>, pt: &ExtTy) -> bool {
        if let Expression::ParenthesizedExpression(p) = e {
            return self.arg_fits(&p.expression, pt);
        }
        let inner = match pt {
            ExtTy::Opt(i) => Some(&**i),
            _ => None,
        };
        match e {
            Expression::ObjectExpression(_) => {
                return matches!(inner.unwrap_or(pt), ExtTy::Record(_) | ExtTy::Value(_));
            }
            Expression::NullLiteral(_) => return matches!(pt, ExtTy::Opt(_) | ExtTy::Handle(_)),
            // `undefined` is "left out" for any parameter.
            Expression::Identifier(id) if id.name == "undefined" => return true,
            _ => {}
        }
        let Ok(v) = self.lower_expr(e, None) else {
            return false;
        };
        let ty = match v.ty.strip().clone() {
            Ty::Nullable(i) => i.strip().clone(),
            t => t,
        };
        let target = inner.unwrap_or(pt);
        match target {
            ExtTy::Str | ExtTy::Enum(_) => matches!(ty, Ty::Str | Ty::Lit),
            ExtTy::F64 | ExtTy::F32 | ExtTy::I32 | ExtTy::U32 => ty == Ty::Num,
            ExtTy::Bool => ty == Ty::Bool,
            ExtTy::Handle(_) => matches!(ty, Ty::Handle(_)),
            ExtTy::Record(_) => matches!(&ty, Ty::Obj(sid)
                if !self.manifest.values.contains_key(&self.m.structs[*sid].name)),
            other => self.ext_class(&ty).as_ref() == Some(other),
        }
    }

    /// A boundary function with several versions: the one whose parameters take the argument types.
    fn lower_overloaded(
        &mut self,
        ns: &str,
        name: &str,
        cands: &[ExternFn],
        c: &'a CallExpression<'a>,
    ) -> R<Expr> {
        let mut vals = Vec::new();
        for a in &c.arguments {
            let Some(e) = a.as_expression() else {
                bail!("{ns}.{name}: spread argument")
            };
            let v = self.lower_expr(e, None)?;
            vals.push(self.widen_lit(v));
        }
        let at = self.loc(c.span.start);
        self.pick_overload(ns, name, cands, vals, &at)
    }

    /// The version of a boundary function whose parameters take the (lowered) argument types.
    pub(crate) fn pick_overload(
        &mut self,
        ns: &str,
        name: &str,
        cands: &[ExternFn],
        vals: Vec<Expr>,
        at: &str,
    ) -> R<Expr> {
        let classes: Vec<Option<ExtTy>> = vals.iter().map(|v| self.ext_class(&v.ty)).collect();
        for f in cands {
            if f.params.len() == vals.len()
                && f.params
                    .iter()
                    .zip(&classes)
                    .all(|((_, p), c)| c.as_ref() == Some(p))
            {
                return Ok(self.emit_extern(f, vals));
            }
        }
        // A string or array with no version of its own goes where any script pointer does.
        let loose: Vec<Option<ExtTy>> = classes
            .iter()
            .map(|c| match c {
                Some(
                    ExtTy::Str
                    | ExtTy::HandleArr(_)
                    | ExtTy::F64Arr
                    | ExtTy::StrArr
                    | ExtTy::ValueArr(_),
                ) => Some(ExtTy::Ptr),
                other => other.clone(),
            })
            .collect();
        for f in cands {
            if f.params.len() == vals.len()
                && f.params
                    .iter()
                    .zip(&loose)
                    .all(|((_, p), c)| c.as_ref() == Some(p))
            {
                return Ok(self.emit_extern(f, vals));
            }
        }
        let have: Vec<String> = vals.iter().map(|v| v.ty.to_string()).collect();
        bail!("{at}: {ns}.{name} has no version for ({})", have.join(", "))
    }

    /// `Ui.stateNew(v)`, `Ui.store(slot, v)` and `Ui.dep(v)` for an engine handle (`useState(camera)`):
    /// its id, kept untraced. `None` when the value is not a handle.
    fn lower_ui_handle_slot(&mut self, name: &str, c: &'a CallExpression<'a>) -> R<Option<Expr>> {
        let Some(last) = c.arguments.last().and_then(|a| a.as_expression()) else {
            return Ok(None);
        };
        let mark = self.cur.locals.len();
        let probe = self.lower_expr(last, None);
        self.cur.locals.truncate(mark);
        let probe = probe.map(|p| p.ty);
        // `useState<Part | null>(null)`: a tag that may be missing is kept as its text, or null.
        if matches!(&probe, Ok(Ty::Nullable(t)) if **t == Ty::Lit) && name != "dep" {
            let mut args = Vec::new();
            for a in &c.arguments {
                let Some(e) = a.as_expression() else {
                    return Ok(None);
                };
                let v = self.lower_expr(e, None)?;
                args.push(if v.ty == Ty::Lit.nullable() {
                    self.coerce(v, &Ty::Str.nullable())?
                } else {
                    self.coerce(v, &Ty::Num)?
                });
            }
            let (symbol, params, ret) = if name == "stateNew" {
                ("ts_ui_state_ptr", vec![("init", ExtTy::Ptr)], ExtTy::F64)
            } else {
                (
                    "ts_ui_store_ptr",
                    vec![("slot", ExtTy::F64), ("v", ExtTy::Ptr)],
                    ExtTy::Void,
                )
            };
            let f = ExternFn {
                rest: false,
                optional_from: usize::MAX,
                defaults: Vec::new(),
                symbol: symbol.into(),
                params: params.into_iter().map(|(n, t)| (n.into(), t)).collect(),
                ret,
            };
            return Ok(Some(self.emit_extern(&f, args)));
        }
        let Ok(Ty::Handle(class)) = probe.map(|p| p.strip().clone()) else {
            return Ok(None);
        };
        let handle = ExtTy::Handle(class.clone());
        let (symbol, params, ret) = match name {
            "stateNew" => ("ts_ui_state_handle", vec![("init", handle)], ExtTy::F64),
            "store" => (
                "ts_ui_store_handle",
                vec![("slot", ExtTy::F64), ("v", handle)],
                ExtTy::Void,
            ),
            _ => ("ts_ui_dep_ptr", vec![("v", handle)], ExtTy::Void),
        };
        let mut args = Vec::new();
        for (a, (_, t)) in c.arguments.iter().zip(&params) {
            let Some(e) = a.as_expression() else {
                return Ok(None);
            };
            let want = self.ext_ir(t);
            let v = self.lower_expr(e, Some(&want))?;
            args.push(self.coerce(v, &want)?);
        }
        let f = ExternFn {
            rest: false,
            optional_from: usize::MAX,
            defaults: Vec::new(),
            symbol: symbol.into(),
            params: params.into_iter().map(|(n, t)| (n.into(), t)).collect(),
            ret,
        };
        Ok(Some(self.emit_extern(&f, args)))
    }

    /// `Ui.load<T>(slot)`: reads a hook slot as a `T` (a number, a boolean or a script pointer).
    fn lower_ui_load(&mut self, c: &'a CallExpression<'a>) -> R<Expr> {
        let at = self.loc(c.span.start);
        let (Some(targ), Some(arg)) = (
            c.type_arguments.as_ref().and_then(|t| t.params.first()),
            c.arguments.first().and_then(|a| a.as_expression()),
        ) else {
            bail!("{at}: Ui.load<T>(slot)")
        };
        let t = self.ty(targ, "Ui.load")?;
        let slot = self.lower_expr(arg, Some(&Ty::Num))?;
        let slot = self.coerce(slot, &Ty::Num)?;
        // A tag that may be missing is kept as its text, or null (`lower_ui_handle_slot`).
        if t == Ty::Lit.nullable() {
            let f = ExternFn {
                rest: false,
                optional_from: usize::MAX,
                defaults: Vec::new(),
                symbol: "ts_ui_load_ptr".into(),
                params: vec![("slot".into(), ExtTy::F64)],
                ret: ExtTy::Ptr,
            };
            let mut s = self.emit_extern(&f, vec![slot]);
            s.ty = Ty::Str.nullable();
            let tmp = self.new_local(s.ty.clone());
            let tv = self.var_expr(tmp);
            let is_null = mk(ExprKind::IsNull(Box::new(tv.clone())), Ty::Bool);
            let text = mk(ExprKind::Cast(Box::new(tv)), Ty::Str);
            let tag = mk(
                ExprKind::Cast(Box::new(mk(ExprKind::StrTag(Box::new(text)), Ty::Lit))),
                t.clone(),
            );
            let none = mk(ExprKind::Null, t.clone());
            let pick = mk(
                ExprKind::Cond(Box::new(is_null), Box::new(none), Box::new(tag)),
                t.clone(),
            );
            return Ok(mk(
                ExprKind::Seq(vec![Stmt::Assign(tmp, s)], Box::new(pick)),
                t,
            ));
        }
        let (symbol, ret) = match t.strip() {
            Ty::Num => ("ts_ui_load_num", ExtTy::F64),
            Ty::Bool => ("ts_ui_load_bool", ExtTy::Bool),
            Ty::Handle(n) => ("ts_ui_load_handle", ExtTy::Handle(n.clone())),
            other if other.is_ptr() => ("ts_ui_load_ptr", ExtTy::Ptr),
            // A string-literal union is stored as its text (`Ui.stateNew` widens it).
            Ty::Lit => ("ts_ui_load_ptr", ExtTy::Ptr),
            other => bail!("{at}: Ui.load<{other}> is not supported"),
        };
        if *t.strip() == Ty::Lit {
            let f = ExternFn {
                rest: false,
                optional_from: usize::MAX,
                defaults: Vec::new(),
                symbol: symbol.into(),
                params: vec![("slot".into(), ExtTy::F64)],
                ret,
            };
            let mut s = self.emit_extern(&f, vec![slot]);
            s.ty = Ty::Str;
            return Ok(mk(ExprKind::StrTag(Box::new(s)), Ty::Lit));
        }
        let f = ExternFn {
            rest: false,
            optional_from: usize::MAX,
            defaults: Vec::new(),
            symbol: symbol.into(),
            params: vec![("slot".into(), ExtTy::F64)],
            ret,
        };
        let mut e = self.emit_extern(&f, vec![slot]);
        e.ty = t;
        Ok(e)
    }

    /// `Ui.styleObj(h, obj)` / `Ui.attrObj(h, obj)` (a dynamic `style={x}` or `{...x}` in JSX): the
    /// fields of a typed object become style properties or attributes. An optional field that is
    /// null is left out; a number, string or boolean goes through as itself; anything else (a
    /// function, a nested object) is an error that names the field.
    fn lower_ui_fields(&mut self, attr: bool, c: &'a CallExpression<'a>) -> R<Expr> {
        let at = self.loc(c.span.start);
        let what = if attr { "{...props}" } else { "style={...}" };
        let (Some(he), Some(oe)) = (
            c.arguments.first().and_then(|a| a.as_expression()),
            c.arguments.get(1).and_then(|a| a.as_expression()),
        ) else {
            bail!("{at}: Ui.styleObj(h, obj)")
        };
        let h = self.lower_expr(he, None)?;
        // `style={..}` is a `PlatypusStyle` (a list of property entries) when the ambient types are there.
        let bag = if !attr && self.fids.contains_key("__apply_style") {
            Some(Ty::Arr(Box::new(self.named_type("StyleEntry")?)))
        } else {
            None
        };
        // `{...(cond ? handlers : {})}`: the arm taken sets its fields (`{}` sets none), as a
        // spread of either object would.
        if attr && let Expression::ConditionalExpression(_) = oe.get_inner_expression() {
            let th = self.new_local(h.ty.clone());
            let mut stmts = vec![Stmt::Assign(th, h)];
            stmts.extend(self.ui_fields_of(th, oe, attr, &at, what)?);
            let result = self.var_expr(th);
            let ty = result.ty.clone();
            return Ok(mk(ExprKind::Seq(stmts, Box::new(result)), ty));
        }
        let o = self.lower_expr(oe, bag.as_ref())?;
        if let Some(bag) = &bag
            && o.ty.strip() == bag
        {
            let o = self.coerce(o, &bag.clone().nullable())?;
            return Ok(mk(
                ExprKind::Call(self.fids["__apply_style"], vec![h, o]),
                Ty::Handle("UiElement".into()),
            ));
        }
        let th = self.new_local(h.ty.clone());
        let mut stmts = vec![Stmt::Assign(th, h)];
        stmts.extend(self.ui_fields_from(th, o, attr, &at, what)?);
        let result = self.var_expr(th);
        let ty = result.ty.clone();
        Ok(mk(ExprKind::Seq(stmts, Box::new(result)), ty))
    }

    /// The statements that set the fields of the object `oe` on the element in `th`; a
    /// conditional sets those of the arm taken, and an empty `{}` none.
    fn ui_fields_of(
        &mut self,
        th: VarId,
        oe: &'a Expression<'a>,
        attr: bool,
        at: &str,
        what: &str,
    ) -> R<Vec<Stmt>> {
        match oe.get_inner_expression() {
            Expression::ConditionalExpression(c) => {
                let test = self.lower_cond(&c.test)?;
                let (yes, no) = if self.unions.is_empty() {
                    (vec![], vec![])
                } else {
                    (
                        self.narrowings(&c.test, true),
                        self.narrowings(&c.test, false),
                    )
                };
                let a =
                    self.with_narrowed(yes, |s| s.ui_fields_of(th, &c.consequent, attr, at, what))?;
                let b =
                    self.with_narrowed(no, |s| s.ui_fields_of(th, &c.alternate, attr, at, what))?;
                Ok(vec![Stmt::If(test, a, b)])
            }
            Expression::ObjectExpression(o) if o.properties.is_empty() => Ok(vec![]),
            _ => {
                let o = self.lower_expr(oe, None)?;
                self.ui_fields_from(th, o, attr, at, what)
            }
        }
    }

    /// The statements that set each field of the typed object `o` on the element in `th`.
    fn ui_fields_from(
        &mut self,
        th: VarId,
        o: Expr,
        attr: bool,
        at: &str,
        what: &str,
    ) -> R<Vec<Stmt>> {
        let Ty::Obj(sid) = o.ty.strip().clone() else {
            bail!(
                "{at}: {what} needs an object with known fields, not {}",
                o.ty
            )
        };
        let to = self.new_local(o.ty.clone());
        let mut stmts = vec![Stmt::Assign(to, o)];
        let fields = self.m.structs[sid].fields.clone();
        for (i, f) in fields.iter().enumerate() {
            let field = mk(
                ExprKind::Field(Box::new(self.var_expr(to)), sid, i),
                f.ty.clone(),
            );
            let set = self.ui_set_stmts(th, &f.name, field, attr, at, what)?;
            stmts.extend(set);
        }
        Ok(stmts)
    }

    /// The statements that set one style property or attribute `key` of the element in `th` to
    /// `field` (a pure expression): a number, a string, a boolean (attributes), a `number | string`
    /// (`StyleValue`), or any of those that may be null, which leaves the property out.
    pub(crate) fn ui_set_stmts(
        &mut self,
        th: VarId,
        key: &str,
        field: Expr,
        attr: bool,
        at: &str,
        what: &str,
    ) -> R<Vec<Stmt>> {
        let element = || ExtTy::Handle("UiElement".into());
        let ext = |symbol: &str, vt: ExtTy| ExternFn {
            rest: false,
            optional_from: usize::MAX,
            defaults: Vec::new(),
            symbol: symbol.into(),
            params: vec![
                ("h".into(), element()),
                ("key".into(), ExtTy::Str),
                ("v".into(), vt),
            ],
            ret: element(),
        };
        // `{...handlers}` where `handlers.onDrop` is a function: the element listens to the event
        // (a missing handler is skipped by `__handler`).
        if attr && super::jsx::is_handler_prop(key) && matches!(field.ty.strip(), Ty::Fn(_)) {
            let Some(on) = self
                .manifest
                .namespaces
                .get("Ui")
                .and_then(|ns| ns.get("on"))
                .cloned()
            else {
                bail!("{at}: {what}: `{key}` needs the UI")
            };
            let f = self.call_helper("__handler", vec![field])?;
            let kind = self.str_lit(&super::jsx::handler_kind(key));
            let hv = self.var_expr(th);
            return Ok(vec![Stmt::Eval(self.emit_extern(&on, vec![hv, kind, f]))]);
        }
        let mut plain = mk(
            ExprKind::Cast(Box::new(field.clone())),
            field.ty.strip().clone(),
        );
        plain = self.widen_lit(plain);
        let mut out = Vec::new();
        if let Ty::Obj(sid) = plain.ty.clone()
            && self.is_style_value(&plain.ty)
        {
            // `n`, `s`, `isStr` are fields 0, 1, 2 of the prelude class.
            let tv = self.new_local(plain.ty.clone());
            out.push(Stmt::Assign(tv, plain));
            let get = |s: &Self, i: usize, ty: Ty| {
                mk(ExprKind::Field(Box::new(s.var_expr(tv)), sid, i), ty)
            };
            let (n, st, is_str) = (
                get(self, 0, Ty::Num),
                get(self, 1, Ty::Str),
                get(self, 2, Ty::Bool),
            );
            let (num_sym, str_sym) = if attr {
                ("ts_ui_attr_num", "ts_ui_attr_str")
            } else {
                ("ts_ui_style_num", "ts_ui_style_str")
            };
            let hv = self.var_expr(th);
            let k = self.str_lit(key);
            let cn = self.emit_extern(&ext(num_sym, ExtTy::F64), vec![hv, k, n]);
            let hv = self.var_expr(th);
            let k = self.str_lit(key);
            let cs = self.emit_extern(&ext(str_sym, ExtTy::Str), vec![hv, k, st]);
            out.push(Stmt::If(is_str, vec![Stmt::Eval(cs)], vec![Stmt::Eval(cn)]));
        } else {
            let (symbol, vt) = match (&plain.ty, attr) {
                (Ty::Num, false) => ("ts_ui_style_num", ExtTy::F64),
                (Ty::Str, false) => ("ts_ui_style_str", ExtTy::Str),
                (Ty::Num, true) => ("ts_ui_attr_num", ExtTy::F64),
                (Ty::Str, true) => ("ts_ui_attr_str", ExtTy::Str),
                (Ty::Bool, true) => ("ts_ui_attr_bool", ExtTy::Bool),
                (other, _) => {
                    bail!("{at}: {what}: `{key}` has type {other}, which cannot be set this way")
                }
            };
            let hv = self.var_expr(th);
            let k = self.str_lit(key);
            let call = self.emit_extern(&ext(symbol, vt), vec![hv, k, plain]);
            out.push(Stmt::Eval(call));
        }
        Ok(if field.ty.is_nullable() {
            vec![Stmt::If(
                mk(ExprKind::IsNull(Box::new(field)), Ty::Bool),
                vec![],
                out,
            )]
        } else {
            out
        })
    }

    /// `Ui.child(h, x)` where `x` may be undefined (`{error}` for `error?: string`): nothing is
    /// added then. `None` when `x` is not a nullable string or number.
    fn lower_ui_opt(&mut self, fname: &str, c: &'a CallExpression<'a>) -> R<Option<Expr>> {
        let at = self.loc(c.span.start);
        let [h, v] = c.arguments.as_slice() else {
            return Ok(None);
        };
        let (Some(h), Some(v)) = (h.as_expression(), v.as_expression()) else {
            return Ok(None);
        };
        let ve = self.lower_expr(v, None)?;
        if !matches!(&ve.ty, Ty::Nullable(inner) if matches!(&**inner, Ty::Str | Ty::Num | Ty::Lit))
        {
            // Not ours: lower again through the plain path with the value already lowered.
            let he = self.lower_expr(h, None)?;
            let cands = self
                .manifest
                .overloads
                .get(&("Ui".to_string(), fname.to_string()))
                .cloned()
                .unwrap_or_default();
            // `{lists.map(..)}` of `ReactNode[]`: lists in lists are flattened, as React does.
            if fname == "child" && Self::is_nested_node_list(&ve.ty) {
                return self.ui_child_lists(he, ve, &cands, &at).map(Some);
            }
            let vals = vec![he, self.widen_lit(ve)];
            return self.pick_overload("Ui", fname, &cands, vals, &at).map(Some);
        }
        let he = self.lower_expr(h, None)?;
        let th = self.new_local(he.ty.clone());
        let tv = self.new_local(ve.ty.clone());
        let plain = mk(
            ExprKind::Cast(Box::new(self.var_expr(tv))),
            ve.ty.strip().clone(),
        );
        let plain = self.widen_lit(plain);
        let cands = self
            .manifest
            .overloads
            .get(&("Ui".to_string(), fname.to_string()))
            .cloned()
            .unwrap_or_default();
        let call = self.pick_overload("Ui", fname, &cands, vec![self.var_expr(th), plain], &at)?;
        let stmts = vec![
            Stmt::Assign(th, he),
            Stmt::Assign(tv, ve),
            Stmt::If(
                mk(ExprKind::IsNull(Box::new(self.var_expr(tv))), Ty::Bool),
                vec![],
                vec![Stmt::Eval(call)],
            ),
        ];
        let result = self.var_expr(th);
        let ty = result.ty.clone();
        Ok(Some(mk(ExprKind::Seq(stmts, Box::new(result)), ty)))
    }

    /// A list of lists of elements (`ReactNode[]`, any depth).
    fn is_nested_node_list(t: &Ty) -> bool {
        match t.strip() {
            Ty::Arr(inner) => match inner.strip() {
                Ty::Arr(e) => {
                    matches!(e.strip(), Ty::Handle(h) if h == "UiElement")
                        || Self::is_nested_node_list(inner)
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// `Ui.child(h, lists)`: each inner list added in turn.
    fn ui_child_lists(&mut self, he: Expr, lists: Expr, cands: &[ExternFn], at: &str) -> R<Expr> {
        let th = self.new_local(he.ty.clone());
        let mut stmts = vec![Stmt::Assign(th, he)];
        let it = self.begin_iter(lists)?;
        stmts.extend(it.pre.clone());
        let mut body = it.head.clone();
        let inner = self.var_expr(it.elem);
        let added = if Self::is_nested_node_list(&inner.ty) {
            self.ui_child_lists(self.var_expr(th), inner, cands, at)?
        } else {
            self.pick_overload("Ui", "child", cands, vec![self.var_expr(th), inner], at)?
        };
        body.push(Stmt::Assign(th, added));
        stmts.push(Stmt::Loop(it.label, body));
        let ty = self.cur.locals[th as usize].clone();
        Ok(mk(ExprKind::Seq(stmts, Box::new(self.var_expr(th))), ty))
    }

    /// `Ui.style(h, key, v)` / `Ui.attr(h, key, v)` whose value has no version of its own: a
    /// `number | string`, or a value that may be undefined (`cursor: on ? "pointer" : undefined`).
    fn lower_ui_set(&mut self, attr: bool, c: &'a CallExpression<'a>) -> R<Option<Expr>> {
        let at = self.loc(c.span.start);
        let [h, k, v] = c.arguments.as_slice() else {
            return Ok(None);
        };
        let (Some(h), Some(k), Some(v)) = (h.as_expression(), k.as_expression(), v.as_expression())
        else {
            return Ok(None);
        };
        let Expression::StringLiteral(key) = k else {
            return Ok(None);
        };
        // Lower the value once: if it needs no special path, the plain call is built from it.
        let ve = self.lower_expr(v, None)?;
        let special = ve.ty.is_nullable() || self.is_style_value(ve.ty.strip());
        let he = self.lower_expr(h, None)?;
        let fname = if attr { "attr" } else { "style" };
        if !special {
            let cands = self
                .manifest
                .overloads
                .get(&("Ui".to_string(), fname.to_string()))
                .cloned()
                .unwrap_or_default();
            let vals = vec![he, self.str_lit(key.value.as_str()), self.widen_lit(ve)];
            return self.pick_overload("Ui", fname, &cands, vals, &at).map(Some);
        }
        let th = self.new_local(he.ty.clone());
        let tv = self.new_local(ve.ty.clone());
        let mut stmts = vec![Stmt::Assign(th, he), Stmt::Assign(tv, ve)];
        let value = self.var_expr(tv);
        let set = self.ui_set_stmts(th, key.value.as_str(), value, attr, &at, "Ui")?;
        stmts.extend(set);
        let result = self.var_expr(th);
        let ty = result.ty.clone();
        Ok(Some(mk(ExprKind::Seq(stmts, Box::new(result)), ty)))
    }

    /// `Ui.runEffect(f)`: calls an effect function; a function it returns is its cleanup.
    /// `Ui.ref(h, r)`: a ref object (`useRef`) gets the node written into `current`; a function
    /// (`ref={(el) => ..}`, React's callback ref) is called with the node and with null.
    fn lower_ui_ref(&mut self, c: &'a CallExpression<'a>) -> R<Expr> {
        let at = self.loc(c.span.start);
        let [h, v] = c.arguments.as_slice() else {
            bail!("{at}: Ui.ref(h, ref)")
        };
        let (Some(h), Some(v)) = (h.as_expression(), v.as_expression()) else {
            bail!("{at}: Ui.ref(h, ref)")
        };
        let he = self.lower_expr(h, None)?;
        let node = Ty::Nullable(Box::new(Ty::Handle("UiNode".into())));
        let hint = matches!(
            v.get_inner_expression(),
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
        )
        .then(|| {
            Ty::Fn(Box::new(FnTy {
                params: vec![node],
                ret: Ty::Void,
            }))
        });
        let ve = self.lower_expr(v, hint.as_ref())?;
        let name = if matches!(ve.ty.strip(), Ty::Fn(_)) {
            "refFn"
        } else {
            "ref"
        };
        let Some(f) = self.manifest.namespaces["Ui"].get(name).cloned() else {
            bail!("{at}: the Ui manifest has no `{name}`")
        };
        Ok(self.emit_extern(&f, vec![he, ve]))
    }

    fn lower_ui_run_effect(&mut self, c: &'a CallExpression<'a>) -> R<Expr> {
        let at = self.loc(c.span.start);
        let Some(arg) = c.arguments.first().and_then(|a| a.as_expression()) else {
            bail!("{at}: Ui.runEffect(f)")
        };
        let saved = std::mem::replace(&mut self.infer_ret, true);
        let hint = Ty::Fn(Box::new(FnTy {
            params: vec![],
            ret: Ty::Void,
        }));
        let f = self.lower_expr(arg, Some(&hint));
        self.infer_ret = saved;
        let f = f?;
        let Ty::Fn(sig) = f.ty.strip().clone() else {
            bail!("{at}: an effect must be a function")
        };
        let tf = self.new_local(f.ty.clone());
        let mut stmts = vec![Stmt::Assign(tf, f)];
        let call = mk(
            ExprKind::CallClosure(Box::new(self.var_expr(tf)), vec![]),
            sig.ret.clone(),
        );
        match sig.ret.strip() {
            Ty::Void => stmts.push(Stmt::Eval(call)),
            Ty::Fn(_) => {
                let r = self.new_local(sig.ret.clone());
                stmts.push(Stmt::Assign(r, call));
                let set = ExternFn {
                    rest: false,
                    optional_from: usize::MAX,
                    defaults: Vec::new(),
                    symbol: "ts_ui_set_cleanup".into(),
                    params: vec![("f".into(), ExtTy::Ptr)],
                    ret: ExtTy::Void,
                };
                let r = self.var_expr(r);
                stmts.push(Stmt::Eval(self.emit_extern(&set, vec![r])));
            }
            other => bail!("{at}: an effect returns nothing or a cleanup function, not {other}"),
        }
        let unit = mk(ExprKind::Bool(false), Ty::Void);
        Ok(mk(ExprKind::Seq(stmts, Box::new(unit)), Ty::Void))
    }

    /// A call of a boundary function with the call's arguments, after an optional receiver
    /// (the first parameter of a class method).
    pub(crate) fn call_ext_fn(
        &mut self,
        f: ExternFn,
        recv: Option<Expr>,
        what: &str,
        c: &'a CallExpression<'a>,
    ) -> R<Expr> {
        self.call_ext_args(f, recv, what, c.span.start, &c.arguments)
    }

    /// `call_ext_fn` over an argument list (a call's, or a `new`'s).
    pub(crate) fn call_ext_args(
        &mut self,
        f: ExternFn,
        recv: Option<Expr>,
        what: &str,
        span_start: u32,
        arguments: &'a [Argument<'a>],
    ) -> R<Expr> {
        let skip = usize::from(recv.is_some());
        let at = self.loc(span_start);
        let params: Vec<(String, ExtTy)> = f.params[skip..].to_vec();
        let mut given: Vec<&'a Expression<'a>> = Vec::new();
        // `...points` of a rest parameter: its argument is an array that goes in whole.
        let mut spread_rest: Option<&'a Expression<'a>> = None;
        for (n, a) in arguments.iter().enumerate() {
            if f.rest
                && let Argument::SpreadElement(sp) = a
                && n + 1 == params.len()
                && n + 1 == arguments.len()
            {
                spread_rest = Some(&sp.argument);
                continue;
            }
            let Some(e) = a.as_expression() else {
                bail!("{at}: {what}: spread argument")
            };
            given.push(e);
        }
        // A rest parameter takes any number of arguments (none: an empty array).
        let rest_at = if f.rest { Some(params.len() - 1) } else { None };
        // A trailing `{ key: value }` fills the parameters after the positional ones by name
        // (`audio.load(path, { stream: true })`); a parameter left out is false, 0 or "".
        let mut options: Option<&'a ObjectExpression<'a>> = None;
        if let Some(Expression::ObjectExpression(o)) = given.last().copied()
            && rest_at.is_none()
            && given.len() <= params.len()
            // An object where a value class goes is the value (`{ x, y }`) unless its keys are
            // parameter names and not the value's fields.
            && match &params[given.len() - 1].1 {
                ExtTy::Value(vn) => !o.properties.iter().any(|p| {
                    matches!(p, ObjectPropertyKind::ObjectProperty(p)
                        if matches!(&p.key, PropertyKey::StaticIdentifier(k)
                            if self.manifest.values.get(vn.as_str())
                                .is_some_and(|f| f.iter().any(|(n, _)| n == k.name.as_str()))))
                }),
                _ => true,
            }
            && o.properties.iter().all(|p| {
                matches!(p, ObjectPropertyKind::ObjectProperty(p)
                    if matches!(&p.key, PropertyKey::StaticIdentifier(k)
                        if params[given.len() - 1..].iter().any(|(n, _)| n == k.name.as_str())))
            })
        {
            options = Some(o);
            given.pop();
        }
        let mut opt_from = f.optional_from.saturating_sub(skip);
        if let Some(r) = rest_at {
            opt_from = opt_from.min(r);
        }
        if (given.len() > params.len() && rest_at.is_none())
            || (options.is_none() && given.len() < params.len() && given.len() < opt_from && {
                // A flag may be left out (false); a missing number or string is an error.
                !params[given.len()..]
                    .iter()
                    .all(|(_, t)| matches!(t, ExtTy::Bool))
            })
        {
            bail!(
                "{at}: {what}: {} arguments, {} expected",
                arguments.len(),
                params.len()
            )
        }
        let mut args: Vec<Expr> = recv.into_iter().collect();
        for (i, (pname, pt)) in params.iter().enumerate() {
            if rest_at == Some(i) {
                let want = self.ext_ir(pt);
                let arr = if let Some(sp) = spread_rest {
                    let v = self.lower_expr(sp, Some(&want))?;
                    self.coerce_ext(v, &want)?
                } else {
                    let Ty::Arr(elem) = want.clone() else {
                        bail!("{at}: {what}: a rest parameter is an array")
                    };
                    let out = self.new_local(want.clone());
                    let mut stmts = vec![Stmt::Assign(
                        out,
                        mk(ExprKind::Rt(RtFn::ArrNew, vec![num(0.0)]), want.clone()),
                    )];
                    for e in given.iter().skip(i) {
                        let v = self.lower_expr(e, Some(&elem))?;
                        let v = self.coerce_ext(v, &elem)?;
                        stmts.push(Stmt::Eval(mk(
                            ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(out), v]),
                            Ty::Void,
                        )));
                    }
                    mk(
                        ExprKind::Seq(stmts, Box::new(self.var_expr(out))),
                        want.clone(),
                    )
                };
                args.push(arr);
                continue;
            }
            let expr: Option<&'a Expression<'a>> = if i < given.len() {
                Some(given[i])
            } else {
                options.and_then(|o| {
                    o.properties.iter().find_map(|p| match p {
                        ObjectPropertyKind::ObjectProperty(p)
                            if matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == pname.as_str()) =>
                        {
                            Some(&p.value)
                        }
                        _ => None,
                    })
                })
            };
            // `undefined` for an optional parameter is the same as leaving it out (`grid(t, 2, 1,
            // undefined, 10)`).
            let expr = expr.filter(|e| {
                !(i >= opt_from
                    && matches!(e, Expression::Identifier(id) if id.name == "undefined")
                    && matches!(
                        pt,
                        ExtTy::Value(_)
                            | ExtTy::F64
                            | ExtTy::F32
                            | ExtTy::Handle(_)
                            | ExtTy::F64Arr
                            | ExtTy::StrArr
                            | ExtTy::ValueArr(_)
                            | ExtTy::HandleArr(_)
                    ))
            });
            let Some(e) = expr else {
                // Left out: the default the API names, else false, 0 or an empty string.
                if let Some(Some(d)) = f.defaults.get(skip + i) {
                    match pt {
                        ExtTy::Bool => {
                            args.push(mk(ExprKind::Bool(*d != 0.0), Ty::Bool));
                            continue;
                        }
                        ExtTy::F64 | ExtTy::F32 | ExtTy::I32 | ExtTy::U32 => {
                            args.push(num(*d));
                            continue;
                        }
                        _ => {}
                    }
                }
                args.push(match pt {
                    ExtTy::Bool => mk(ExprKind::Bool(false), Ty::Bool),
                    // Left out, or an unknown name: the engine reads the first value.
                    ExtTy::Str | ExtTy::Enum(_) => self.str_lit(""),
                    // An optional number is NaN: the function tells "left out" from 0.
                    ExtTy::F64 | ExtTy::F32 if i >= opt_from => num(f64::NAN),
                    ExtTy::F64 | ExtTy::F32 | ExtTy::I32 | ExtTy::U32 => num(0.0),
                    ExtTy::Handle(_) if i >= opt_from => mk(ExprKind::Null, self.ext_ir(pt)),
                    // A left-out optional record is JSON `null`: none.
                    ExtTy::Opt(inner) if matches!(**inner, ExtTy::Record(_)) => {
                        self.str_lit("null")
                    }
                    // A left-out nullable (`normal?: Vector3 | null`) is null.
                    ExtTy::Opt(_) if i >= opt_from => mk(ExprKind::Null, self.ext_ir(pt)),
                    // An optional value class has NaN in every field: the function tells "left
                    // out" from a value.
                    ExtTy::Value(vn) if i >= opt_from => {
                        let sid = self.value_struct(vn);
                        let nf = self.m.structs[sid].fields.len();
                        let ty = Ty::Obj(sid);
                        let tmp = self.new_local(ty.clone());
                        let mut stmts =
                            vec![Stmt::Assign(tmp, mk(ExprKind::NewObj(sid), ty.clone()))];
                        for k in 0..nf {
                            stmts.push(Stmt::StoreField(self.var_expr(tmp), sid, k, num(f64::NAN)));
                        }
                        mk(ExprKind::Seq(stmts, Box::new(self.var_expr(tmp))), ty)
                    }
                    // An optional array is empty.
                    ExtTy::F64Arr
                    | ExtTy::StrArr
                    | ExtTy::ValueArr(_)
                    | ExtTy::HandleArr(_)
                    | ExtTy::RecordsJson(_)
                        if i >= opt_from =>
                    {
                        let ty = self.ext_ir(pt);
                        mk(ExprKind::Rt(RtFn::ArrNew, vec![num(0.0)]), ty)
                    }
                    other => bail!("{at}: {what}: parameter `{pname}` ({other:?}) has no default"),
                });
                continue;
            };
            // An object literal for an `Option<Record>`: the keys written are the keys sent.
            if let ExtTy::Opt(inner) = pt
                && let ExtTy::Record(rn) = &**inner
                && let Some(text) = self.lower_record_literal(e, rn)?
            {
                args.push(text);
                continue;
            }
            if *pt == ExtTy::Ptr {
                // Any script pointer goes through as it is.
                let v = self.lower_expr(e, None)?;
                if !v.ty.is_ptr() {
                    bail!("{at}: {what}: expected an object, found {}", v.ty)
                }
                args.push(v);
                continue;
            }
            let want = self.ext_ir(pt);
            // `[[x, y], ..]` for a list of value classes: lowered as numbers, then converted.
            let rows = matches!(pt, ExtTy::ValueArr(_))
                && matches!(e, Expression::ArrayExpression(a)
                    if a.elements.iter().any(|x| matches!(x.as_expression(), Some(Expression::ArrayExpression(_)))));
            let mut v = self.lower_expr(e, if rows { None } else { Some(&want) })?;
            // A value class argument may be another class with the same fields (a `Vector2Int`
            // where a `Vector2` goes): the fields are read by position.
            if let (Ty::Obj(a), Ty::Obj(b)) = (v.ty.strip(), &want)
                && a != b
                && self.manifest.values.contains_key(&self.m.structs[*a].name)
                && self.m.structs[*a].fields.len() == self.m.structs[*b].fields.len()
            {
                v.ty = want.clone();
            }
            args.push(self.coerce_ext(v, &want)?);
        }
        Ok(self.emit_extern(&f, args))
    }

    /// `coerce` for an argument of the engine API: a string where a handle is expected becomes
    /// the handle's `fromString` (`mesh: "cube"` is `Mesh.fromString("cube")`).
    pub(crate) fn coerce_ext(&mut self, v: Expr, want: &Ty) -> R<Expr> {
        if let Ty::Handle(class) = want.strip()
            && matches!(v.ty.strip(), Ty::Str | Ty::Lit)
            && let Some(from) = self
                .manifest
                .classes
                .get(class.as_str())
                .and_then(|k| k.statics.get("fromString"))
                .cloned()
        {
            let s = self.coerce(v, &Ty::Str)?;
            return Ok(self.emit_extern(&from, vec![s]));
        }
        // `null` where the API takes a handle is "no object" (`parent: null`, `sprite: null`).
        if matches!(want.strip(), Ty::Handle(_)) && matches!(v.ty.strip(), Ty::Null) {
            return Ok(mk(ExprKind::Null, want.clone()));
        }
        // `{ x, y }` objects (or an array of them) where a value class (`Vector2`) is expected:
        // a copy with the fields it names.
        match (v.ty.strip().clone(), want.strip().clone()) {
            (Ty::Obj(a), Ty::Obj(b)) if a != b && self.can_copy_fields(a, b) => {
                return Ok(self.copy_fields(v, a, b));
            }
            // A typed array where the engine takes `number[]`: a copy.
            (Ty::Typed(k), Ty::Arr(e)) if *e == Ty::Num => {
                let arr = want.strip().clone();
                return Ok(self.rt_str("ts_rt_typed_to_array_gc", vec![Self::kind_num(k), v], arr));
            }
            // `[[x, y], ..]` where a list of value classes is expected (`Vector2`): each row's
            // numbers are the fields in order.
            (Ty::Arr(ea), Ty::Arr(eb))
                if matches!(ea.strip(), Ty::Arr(n) if **n == Ty::Num)
                    && matches!(eb.strip(), Ty::Obj(b)
                        if self.manifest.values.contains_key(&self.m.structs[*b].name)) =>
            {
                let Ty::Obj(b) = eb.strip().clone() else {
                    unreachable!()
                };
                let width = self.m.structs[b].fields.len();
                let src = self.new_local(v.ty.clone());
                let out_ty = want.strip().clone();
                let out = self.new_local(out_ty.clone());
                let mut stmts = vec![
                    Stmt::Assign(src, v),
                    Stmt::Assign(
                        out,
                        mk(ExprKind::Rt(RtFn::ArrNew, vec![num(0.0)]), out_ty.clone()),
                    ),
                ];
                let it = self.begin_iter(self.var_expr(src))?;
                stmts.extend(it.pre);
                let row = self.var_expr(it.elem);
                let value_ty = Ty::Obj(b);
                let obj = self.new_local(value_ty.clone());
                let mut body = it.head;
                body.push(Stmt::Assign(obj, mk(ExprKind::NewObj(b), value_ty.clone())));
                for k in 0..width {
                    let n = mk(
                        ExprKind::Index(Box::new(row.clone()), Box::new(num(k as f64))),
                        Ty::Num,
                    );
                    body.push(Stmt::StoreField(self.var_expr(obj), b, k, n));
                }
                body.push(Stmt::Eval(mk(
                    ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(out), self.var_expr(obj)]),
                    Ty::Void,
                )));
                stmts.push(Stmt::Loop(it.label, body));
                return Ok(mk(
                    ExprKind::Seq(stmts, Box::new(self.var_expr(out))),
                    out_ty,
                ));
            }
            (Ty::Arr(ea), Ty::Arr(eb)) => {
                if let (Ty::Obj(a), Ty::Obj(b)) = (ea.strip().clone(), eb.strip().clone())
                    && a != b
                    && self.can_copy_fields(a, b)
                {
                    let src = self.new_local(v.ty.clone());
                    let out_ty = want.strip().clone();
                    let out = self.new_local(out_ty.clone());
                    let mut stmts = vec![
                        Stmt::Assign(src, v),
                        Stmt::Assign(
                            out,
                            mk(ExprKind::Rt(RtFn::ArrNew, vec![num(0.0)]), out_ty.clone()),
                        ),
                    ];
                    let it = self.begin_iter(self.var_expr(src))?;
                    stmts.extend(it.pre);
                    let x = self.var_expr(it.elem);
                    let copy = self.copy_fields(x, a, b);
                    let mut body = it.head;
                    body.push(Stmt::Eval(mk(
                        ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(out), copy]),
                        Ty::Void,
                    )));
                    stmts.push(Stmt::Loop(it.label, body));
                    return Ok(mk(
                        ExprKind::Seq(stmts, Box::new(self.var_expr(out))),
                        out_ty,
                    ));
                }
            }
            _ => {}
        }
        self.coerce(v, want)
    }

    /// Every field of `to` is a field of `from` with the same type (or `from`'s type made
    /// nullable, for a `Partial`): a plain object can then be copied into a `to`.
    pub(crate) fn can_copy_fields(&self, from: StructId, to: StructId) -> bool {
        if self.class_of_sid.contains_key(&to) || self.is_dyn(from) || self.is_dyn(to) {
            return false;
        }
        let (f, t) = (&self.m.structs[from], &self.m.structs[to]);
        !t.fields.is_empty()
            && t.fields.iter().all(|tf| {
                f.field(&tf.name).is_some_and(|i| {
                    let ft = &f.fields[i].ty;
                    *ft == tf.ty || (tf.ty.is_nullable() && *ft == *tf.ty.strip())
                })
            })
    }

    pub(crate) fn copy_fields(&mut self, v: Expr, from: StructId, to: StructId) -> Expr {
        let src = self.new_local(v.ty.clone());
        let ty = Ty::Obj(to);
        let dst = self.new_local(ty.clone());
        let mut stmts = vec![
            Stmt::Assign(src, v),
            Stmt::Assign(dst, mk(ExprKind::NewObj(to), ty.clone())),
        ];
        for i in 0..self.m.structs[to].fields.len() {
            let name = self.m.structs[to].fields[i].name.clone();
            let j = self.m.structs[from].field(&name).expect("checked");
            let fty = self.m.structs[to].fields[i].ty.clone();
            let from_ty = self.m.structs[from].fields[j].ty.clone();
            let mut read = mk(
                ExprKind::Field(Box::new(self.var_expr(src)), from, j),
                from_ty.clone(),
            );
            if from_ty != fty {
                read = mk(ExprKind::Cast(Box::new(read)), fty);
            }
            stmts.push(Stmt::StoreField(self.var_expr(dst), to, i, read));
        }
        mk(ExprKind::Seq(stmts, Box::new(self.var_expr(dst))), ty)
    }

    /// The call of a boundary function with arguments already lowered to the script types. A
    /// value class argument is passed as its fields; a value class result is written into a
    /// new object.
    pub(crate) fn emit_extern(&mut self, f: &ExternFn, args_in: Vec<Expr>) -> Expr {
        let any_value = f
            .params
            .iter()
            .any(|(_, t)| matches!(t, ExtTy::Value(_) | ExtTy::Opt(_)));
        let mut pre: Vec<Stmt> = Vec::new();
        let mut args: Vec<Expr> = Vec::new();
        let mut flat: Vec<ExtTy> = Vec::new();
        for (v, (_, pt)) in args_in.into_iter().zip(&f.params) {
            // With a value argument every argument goes into a temporary first, so they are
            // still evaluated in order.
            let v = if any_value {
                let t = self.new_local(v.ty.clone());
                pre.push(Stmt::Assign(t, v));
                self.var_expr(t)
            } else {
                v
            };
            match pt {
                ExtTy::Value(vn) => {
                    let sid = self.value_struct(vn);
                    for (i, (_, ft)) in self.manifest.values[vn].clone().iter().enumerate() {
                        args.push(mk(ExprKind::Field(Box::new(v.clone()), sid, i), Ty::Num));
                        flat.push(ft.clone());
                    }
                }
                ExtTy::Enum(en) => {
                    args.push(self.enum_index(en, v));
                    flat.push(ExtTy::I32);
                }
                // A list of records: JSON text.
                ExtTy::RecordsJson(_) => {
                    let text = self
                        .json_text_of(v)
                        .expect("a record list argument has a JSON form");
                    args.push(text);
                    flat.push(ExtTy::Str);
                }
                // `Option<f64>`: NaN for null. `Option<V>`: NaN in every field for null.
                ExtTy::Opt(inner) => match &**inner {
                    // `Option<Record>`: the record as JSON text ("null" for none).
                    ExtTy::Record(_) => {
                        // Already JSON text (an object literal), or a record's object.
                        let text = if v.ty.strip() == &Ty::Str {
                            v
                        } else {
                            self.json_text_of(v)
                                .expect("a record argument has a JSON form")
                        };
                        args.push(text);
                        flat.push(ExtTy::Str);
                    }
                    ExtTy::Value(vn) => {
                        let sid = self.value_struct(vn);
                        let is_null = mk(ExprKind::IsNull(Box::new(v.clone())), Ty::Bool);
                        let obj = mk(ExprKind::Cast(Box::new(v.clone())), Ty::Obj(sid));
                        for (i, (_, ft)) in self.manifest.values[vn].clone().iter().enumerate() {
                            let field = mk(ExprKind::Field(Box::new(obj.clone()), sid, i), Ty::Num);
                            args.push(mk(
                                ExprKind::Cond(
                                    Box::new(is_null.clone()),
                                    Box::new(num(f64::NAN)),
                                    Box::new(field),
                                ),
                                Ty::Num,
                            ));
                            flat.push(ft.clone());
                        }
                    }
                    _ => {
                        let is_null = mk(ExprKind::IsNull(Box::new(v.clone())), Ty::Bool);
                        let n = mk(ExprKind::Cast(Box::new(v)), Ty::Num);
                        args.push(mk(
                            ExprKind::Cond(Box::new(is_null), Box::new(num(f64::NAN)), Box::new(n)),
                            Ty::Num,
                        ));
                        flat.push(ExtTy::F64);
                    }
                },
                other => {
                    args.push(v);
                    flat.push(other.clone());
                }
            }
        }
        let (ret_ext, out) = match &f.ret {
            ExtTy::Value(vn) => {
                let fields = &self.manifest.values[vn];
                (ExtTy::Void, Some((fields[0].1.clone(), fields.len())))
            }
            ExtTy::Enum(_) => (ExtTy::I32, None),
            // A record with lists or optional keys is JSON text.
            ExtTy::Record(rn) if !self.record_is_flat(rn) => (ExtTy::Str, None),
            ExtTy::Record(_) => (ExtTy::Void, None),
            ExtTy::RecordsJson(_) => (ExtTy::Str, None),
            ExtTy::ValueList(_)
            | ExtTy::RecordList(_)
            | ExtTy::StrList
            | ExtTy::EnumList(_)
            | ExtTy::F64List
            | ExtTy::HandleList(_) => (ExtTy::U32, None),
            ExtTy::Opt(inner) => match &**inner {
                ExtTy::Value(vn) => {
                    let fields = &self.manifest.values[vn];
                    (ExtTy::Void, Some((fields[0].1.clone(), fields.len())))
                }
                // `Option<Record>`: its JSON text.
                ExtTy::Record(_) => (ExtTy::Str, None),
                other => (other.clone(), None),
            },
            other => (other.clone(), None),
        };
        let decl = ExternDecl {
            symbol: f.symbol.clone(),
            params: flat,
            ret: ret_ext,
            out,
        };
        let idx = match self.m.externs.iter().position(|d| d.symbol == decl.symbol) {
            Some(i) => i,
            None => {
                self.m.externs.push(decl);
                self.m.externs.len() - 1
            }
        };
        if let ExtTy::Value(vn) = &f.ret {
            let sid = self.value_struct(vn);
            let ty = Ty::Obj(sid);
            let dest = self.new_local(ty.clone());
            pre.push(Stmt::Assign(dest, mk(ExprKind::NewObj(sid), ty.clone())));
            pre.push(Stmt::Eval(mk(
                ExprKind::CallExternInto(idx, args, Box::new(self.var_expr(dest))),
                Ty::Void,
            )));
            return mk(ExprKind::Seq(pre, Box::new(self.var_expr(dest))), ty);
        }
        // `Option<V>`: the value, or null when its first field is NaN.
        if let ExtTy::Opt(inner) = &f.ret
            && let ExtTy::Value(vn) = &**inner
        {
            let sid = self.value_struct(vn);
            let ty = Ty::Obj(sid);
            let dest = self.new_local(ty.clone());
            pre.push(Stmt::Assign(dest, mk(ExprKind::NewObj(sid), ty.clone())));
            pre.push(Stmt::Eval(mk(
                ExprKind::CallExternInto(idx, args, Box::new(self.var_expr(dest))),
                Ty::Void,
            )));
            let first = mk(
                ExprKind::Field(Box::new(self.var_expr(dest)), sid, 0),
                Ty::Num,
            );
            let nan = mk(
                ExprKind::Cmp(CmpOp::Ne, Box::new(first.clone()), Box::new(first)),
                Ty::Bool,
            );
            let nty = ty.clone().nullable();
            let result = mk(
                ExprKind::Cond(
                    Box::new(nan),
                    Box::new(mk(ExprKind::Null, nty.clone())),
                    Box::new(mk(
                        ExprKind::Cast(Box::new(self.var_expr(dest))),
                        nty.clone(),
                    )),
                ),
                nty.clone(),
            );
            return mk(ExprKind::Seq(pre, Box::new(result)), nty);
        }
        match &f.ret {
            // `Option<f64>`: NaN is null.
            ExtTy::Opt(inner) if matches!(**inner, ExtTy::F64) => {
                let t = self.new_local(Ty::Num);
                pre.push(Stmt::Assign(
                    t,
                    mk(ExprKind::CallExtern(idx, args), Ty::Num),
                ));
                let nty = Ty::Num.nullable();
                let nan = mk(
                    ExprKind::Cmp(
                        CmpOp::Ne,
                        Box::new(self.var_expr(t)),
                        Box::new(self.var_expr(t)),
                    ),
                    Ty::Bool,
                );
                let result = mk(
                    ExprKind::Cond(
                        Box::new(nan),
                        Box::new(mk(ExprKind::Null, nty.clone())),
                        Box::new(mk(ExprKind::Cast(Box::new(self.var_expr(t))), nty.clone())),
                    ),
                    nty.clone(),
                );
                return mk(ExprKind::Seq(pre, Box::new(result)), nty);
            }
            // A list of records: the call returns JSON text.
            ExtTy::RecordsJson(rn) => {
                let ty = Ty::Arr(Box::new(Ty::Obj(self.record_struct(rn))));
                let text = mk(ExprKind::CallExtern(idx, args), Ty::Str);
                let parsed = self
                    .json_value_with(text, ty.clone(), true)
                    .expect("a record list result has a JSON form");
                return mk(ExprKind::Seq(pre, Box::new(parsed)), ty);
            }
            // `Option<Record>`: the call returns JSON text; parse it into the record's object.
            ExtTy::Opt(inner) if matches!(**inner, ExtTy::Record(_)) => {
                let ExtTy::Record(rn) = &**inner else {
                    unreachable!()
                };
                let nty = Ty::Obj(self.record_struct(rn)).nullable();
                let text = mk(ExprKind::CallExtern(idx, args), Ty::Str);
                let parsed = self
                    .json_value_with(text, nty.clone(), true)
                    .expect("a record result has a JSON form");
                return mk(ExprKind::Seq(pre, Box::new(parsed)), nty);
            }
            // `Option<String>`: the call leaves a flag that says it was none.
            ExtTy::Opt(inner) if matches!(**inner, ExtTy::Str) => {
                let t = self.new_local(Ty::Str);
                pre.push(Stmt::Assign(
                    t,
                    mk(ExprKind::CallExtern(idx, args), Ty::Str),
                ));
                let none_fn = ExternFn {
                    rest: false,
                    optional_from: usize::MAX,
                    defaults: Vec::new(),
                    symbol: "ts_api_result_none".into(),
                    params: Vec::new(),
                    ret: ExtTy::Bool,
                };
                let none = self.emit_extern(&none_fn, vec![]);
                let nty = Ty::Str.nullable();
                let result = mk(
                    ExprKind::Cond(
                        Box::new(none),
                        Box::new(mk(ExprKind::Null, nty.clone())),
                        Box::new(mk(ExprKind::Cast(Box::new(self.var_expr(t))), nty.clone())),
                    ),
                    nty.clone(),
                );
                return mk(ExprKind::Seq(pre, Box::new(result)), nty);
            }
            // A list of enums: the call returns the count and the indexes are in the buffer; each
            // becomes its name.
            ExtTy::EnumList(en) => {
                let arr_ty = Ty::Arr(Box::new(Ty::Str));
                let count = self.new_local(Ty::Num);
                pre.push(Stmt::Assign(
                    count,
                    mk(ExprKind::CallExtern(idx, args), Ty::Num),
                ));
                let arr = self.new_local(arr_ty.clone());
                pre.push(Stmt::Assign(
                    arr,
                    mk(ExprKind::Rt(RtFn::ArrNew, vec![num(0.0)]), arr_ty.clone()),
                ));
                let i = self.new_local(Ty::Num);
                pre.push(Stmt::Assign(i, num(0.0)));
                let label = self.new_label();
                let iv = self.var_expr(i);
                let done = mk(
                    ExprKind::Cmp(
                        CmpOp::Ge,
                        Box::new(iv.clone()),
                        Box::new(self.var_expr(count)),
                    ),
                    Ty::Bool,
                );
                let mut body = vec![Stmt::If(done, vec![Stmt::Break(label)], vec![])];
                let index = self.flat_result(iv.clone());
                let name_fn = self.manifest.namespaces[en.as_str()]["name"].clone();
                let text = self.emit_extern(&name_fn, vec![index]);
                body.push(Stmt::Eval(mk(
                    ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(arr), text]),
                    Ty::Void,
                )));
                body.push(Stmt::Assign(
                    i,
                    mk(
                        ExprKind::Bin(BinOp::Add, Box::new(iv), Box::new(num(1.0))),
                        Ty::Num,
                    ),
                ));
                pre.push(Stmt::Loop(label, body));
                let result = self.var_expr(arr);
                return mk(ExprKind::Seq(pre, Box::new(result)), arr_ty);
            }
            // A list of numbers or handles: the call returns the count; the numbers are in the
            // buffer and the ids behind `ts_api_result_id`.
            ExtTy::F64List | ExtTy::HandleList(_) => {
                let arr_ty = self.ext_ir(&f.ret);
                let count = self.new_local(Ty::Num);
                pre.push(Stmt::Assign(
                    count,
                    mk(ExprKind::CallExtern(idx, args), Ty::Num),
                ));
                let arr = self.new_local(arr_ty.clone());
                pre.push(Stmt::Assign(
                    arr,
                    mk(ExprKind::Rt(RtFn::ArrNew, vec![num(0.0)]), arr_ty.clone()),
                ));
                let i = self.new_local(Ty::Num);
                pre.push(Stmt::Assign(i, num(0.0)));
                let label = self.new_label();
                let iv = self.var_expr(i);
                let done = mk(
                    ExprKind::Cmp(
                        CmpOp::Ge,
                        Box::new(iv.clone()),
                        Box::new(self.var_expr(count)),
                    ),
                    Ty::Bool,
                );
                let mut body = vec![Stmt::If(done, vec![Stmt::Break(label)], vec![])];
                let item = match &f.ret {
                    ExtTy::HandleList(hn) => {
                        let id = ExternFn {
                            rest: false,
                            optional_from: usize::MAX,
                            defaults: Vec::new(),
                            symbol: "ts_api_result_id".into(),
                            params: vec![("i".into(), ExtTy::U32)],
                            ret: ExtTy::Handle(hn.clone()),
                        };
                        self.emit_extern(&id, vec![iv.clone()])
                    }
                    _ => self.flat_result(iv.clone()),
                };
                body.push(Stmt::Eval(mk(
                    ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(arr), item]),
                    Ty::Void,
                )));
                body.push(Stmt::Assign(
                    i,
                    mk(
                        ExprKind::Bin(BinOp::Add, Box::new(iv), Box::new(num(1.0))),
                        Ty::Num,
                    ),
                ));
                pre.push(Stmt::Loop(label, body));
                let result = self.var_expr(arr);
                return mk(ExprKind::Seq(pre, Box::new(result)), arr_ty);
            }
            // The name of the index the call returns.
            ExtTy::Enum(en) => {
                let call = mk(ExprKind::CallExtern(idx, args), Ty::Num);
                let name = self.manifest.namespaces[en.as_str()]["name"].clone();
                let named = self.emit_extern(&name, vec![call]);
                let ty = named.ty.clone();
                return if pre.is_empty() {
                    named
                } else {
                    mk(ExprKind::Seq(pre, Box::new(named)), ty)
                };
            }
            // A record that is not flat: the call returns JSON text.
            ExtTy::Record(rn) if !self.record_is_flat(rn) => {
                let ty = Ty::Obj(self.record_struct(rn));
                let text = mk(ExprKind::CallExtern(idx, args), Ty::Str);
                let parsed = self
                    .json_value_with(text, ty.clone(), true)
                    .expect("a record result has a JSON form");
                return mk(ExprKind::Seq(pre, Box::new(parsed)), ty);
            }
            // One record: the call leaves its numbers in the buffer; read them into an object.
            ExtTy::Record(_) => {
                pre.push(Stmt::Eval(mk(ExprKind::CallExtern(idx, args), Ty::Void)));
                let zero = num(0.0);
                let obj = self.read_flat_object(&f.ret, &zero, &zero, &mut pre);
                let ty = obj.ty.clone();
                return mk(ExprKind::Seq(pre, Box::new(obj)), ty);
            }
            // A list of strings: the call returns the count; each string is copied out of the
            // engine's list into a new script string and pushed.
            ExtTy::StrList => {
                let arr_ty = self.ext_ir(&f.ret);
                let count = self.new_local(Ty::Num);
                pre.push(Stmt::Assign(
                    count,
                    mk(ExprKind::CallExtern(idx, args), Ty::Num),
                ));
                let arr = self.new_local(arr_ty.clone());
                pre.push(Stmt::Assign(
                    arr,
                    mk(ExprKind::Rt(RtFn::ArrNew, vec![num(0.0)]), arr_ty.clone()),
                ));
                let i = self.new_local(Ty::Num);
                pre.push(Stmt::Assign(i, num(0.0)));
                let label = self.new_label();
                let iv = self.var_expr(i);
                let done = mk(
                    ExprKind::Cmp(
                        CmpOp::Ge,
                        Box::new(iv.clone()),
                        Box::new(self.var_expr(count)),
                    ),
                    Ty::Bool,
                );
                let mut body = vec![Stmt::If(done, vec![Stmt::Break(label)], vec![])];
                let item = ExternFn {
                    rest: false,
                    optional_from: usize::MAX,
                    defaults: Vec::new(),
                    symbol: "ts_api_result_str".into(),
                    params: vec![("i".into(), ExtTy::U32)],
                    ret: ExtTy::Str,
                };
                let text = self.emit_extern(&item, vec![iv.clone()]);
                body.push(Stmt::Eval(mk(
                    ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(arr), text]),
                    Ty::Void,
                )));
                body.push(Stmt::Assign(
                    i,
                    mk(
                        ExprKind::Bin(BinOp::Add, Box::new(iv), Box::new(num(1.0))),
                        Ty::Num,
                    ),
                ));
                pre.push(Stmt::Loop(label, body));
                let result = self.var_expr(arr);
                return mk(ExprKind::Seq(pre, Box::new(result)), arr_ty);
            }
            // A list: the call returns the count; read element after element into an array.
            ExtTy::ValueList(_) | ExtTy::RecordList(_) => {
                let arr_ty = self.ext_ir(&f.ret);
                let count = self.new_local(Ty::Num);
                pre.push(Stmt::Assign(
                    count,
                    mk(ExprKind::CallExtern(idx, args), Ty::Num),
                ));
                let arr = self.new_local(arr_ty.clone());
                pre.push(Stmt::Assign(
                    arr,
                    mk(ExprKind::Rt(RtFn::ArrNew, vec![num(0.0)]), arr_ty.clone()),
                ));
                let i = self.new_local(Ty::Num);
                pre.push(Stmt::Assign(i, num(0.0)));
                let width = self.flat_width(&f.ret);
                let str_width = self.str_width(&f.ret);
                let label = self.new_label();
                let iv = self.var_expr(i);
                let base = mk(
                    ExprKind::Bin(
                        BinOp::Mul,
                        Box::new(iv.clone()),
                        Box::new(num(width as f64)),
                    ),
                    Ty::Num,
                );
                let str_base = mk(
                    ExprKind::Bin(
                        BinOp::Mul,
                        Box::new(iv.clone()),
                        Box::new(num(str_width as f64)),
                    ),
                    Ty::Num,
                );
                let done = mk(
                    ExprKind::Cmp(
                        CmpOp::Ge,
                        Box::new(iv.clone()),
                        Box::new(self.var_expr(count)),
                    ),
                    Ty::Bool,
                );
                let mut body = vec![Stmt::If(done, vec![Stmt::Break(label)], vec![])];
                let elem = self.read_flat_object(&f.ret, &base, &str_base, &mut body);
                body.push(Stmt::Eval(mk(
                    ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(arr), elem]),
                    Ty::Void,
                )));
                body.push(Stmt::Assign(
                    i,
                    mk(
                        ExprKind::Bin(BinOp::Add, Box::new(iv), Box::new(num(1.0))),
                        Ty::Num,
                    ),
                ));
                pre.push(Stmt::Loop(label, body));
                let result = self.var_expr(arr);
                return mk(ExprKind::Seq(pre, Box::new(result)), arr_ty);
            }
            _ => {}
        }
        let ret_ty = self.ext_ir(&f.ret);
        let call = mk(ExprKind::CallExtern(idx, args), ret_ty.clone());
        if pre.is_empty() {
            call
        } else {
            mk(ExprKind::Seq(pre, Box::new(call)), ret_ty)
        }
    }

    /// `x instanceof C` for a class `C` of this program: a header test against the table of `C`
    /// and its subclasses.
    fn lower_instanceof(&mut self, b: &'a BinaryExpression<'a>) -> R<Expr> {
        let Expression::Identifier(id) = &b.right else {
            bail!(
                "{}: `instanceof` needs a class name",
                self.loc(b.span.start)
            )
        };
        if self.generic_classes.contains_key(id.name.as_str()) {
            // `x instanceof Box`: any specialisation of Box, now or made later (fixed up at the end).
            let l = self.lower_expr(&b.left, None)?;
            if !matches!(l.ty.strip(), Ty::Obj(_)) {
                bail!("{}: `instanceof` on {}", self.loc(b.span.start), l.ty)
            }
            self.m.itables.push(Vec::new());
            let table = self.m.itables.len() - 1;
            self.generic_itables.push((table, id.name.to_string()));
            return Ok(mk(ExprKind::InstanceOf(Box::new(l), table), Ty::Bool));
        }
        if !self.classes.contains_key(id.name.as_str()) {
            bail!(
                "{}: `instanceof {}` is only supported for classes of this program",
                self.loc(b.span.start),
                id.name
            )
        }
        let sid = self.classes_sid(id.name.as_str());
        let l = self.lower_expr(&b.left, None)?;
        if !matches!(l.ty.strip(), Ty::Obj(_)) {
            bail!("{}: `instanceof` on {}", self.loc(b.span.start), l.ty)
        }
        // A union of classes: the tag says which member it is.
        if self.union_members(&l.ty).is_some() {
            let family: Vec<StructId> = self
                .class_of_sid
                .keys()
                .copied()
                .filter(|s| self.is_subclass(*s, sid))
                .collect();
            return Ok(
                self.union_is_member(l, |m| matches!(m.strip(), Ty::Obj(s) if family.contains(s)))
            );
        }
        let mut members: Vec<StructId> = self
            .class_of_sid
            .keys()
            .copied()
            .filter(|s| self.is_subclass(*s, sid))
            .collect();
        members.sort_unstable();
        let table = match self.m.itables.iter().position(|t| *t == members) {
            Some(i) => i,
            None => {
                self.m.itables.push(members);
                self.m.itables.len() - 1
            }
        };
        Ok(mk(ExprKind::InstanceOf(Box::new(l), table), Ty::Bool))
    }

    fn lower_binary(&mut self, b: &'a BinaryExpression<'a>) -> R<Expr> {
        use BinaryOperator as B;
        if b.operator == B::Instanceof {
            return self.lower_instanceof(b);
        }
        if b.operator == B::In {
            let obj = self.lower_expr(&b.right, None)?;
            return self.lower_in(&b.left, obj, b.span.start);
        }
        let l = self.lower_expr(&b.left, None)?;
        // A bare string literal compares against the other side's literal type.
        let r = self.lower_expr(&b.right, Some(l.ty.strip()))?;
        let cmp = |op| Some(op);
        let cop = match b.operator {
            B::StrictEquality | B::Equality => cmp(CmpOp::Eq),
            B::StrictInequality | B::Inequality => cmp(CmpOp::Ne),
            B::LessThan => cmp(CmpOp::Lt),
            B::LessEqualThan => cmp(CmpOp::Le),
            B::GreaterThan => cmp(CmpOp::Gt),
            B::GreaterEqualThan => cmp(CmpOp::Ge),
            _ => None,
        };
        if let Some(op) = cop {
            // `<` and friends compare time values; `===` compares the objects.
            let (l, r) = if matches!(op, CmpOp::Eq | CmpOp::Ne) {
                (l, r)
            } else {
                (self.date_value(l), self.date_value(r))
            };
            return self.lower_compare(op, l, r, b.span.start);
        }
        if b.operator == B::Addition && (Self::is_stringy(&l.ty) || Self::is_stringy(&r.ty)) {
            return self.str_concat(l, r);
        }
        // A date added to anything is the concatenation of its string.
        if b.operator == B::Addition && (self.is_date(&l) || self.is_date(&r)) {
            return self.str_concat(l, r);
        }
        let op = match b.operator {
            B::Addition => BinOp::Add,
            B::Subtraction => BinOp::Sub,
            B::Multiplication => BinOp::Mul,
            B::Division => BinOp::Div,
            B::Remainder => BinOp::Rem,
            B::BitwiseAnd => BinOp::BitAnd,
            B::BitwiseOR => BinOp::BitOr,
            B::BitwiseXOR => BinOp::BitXor,
            B::ShiftLeft => BinOp::Shl,
            B::ShiftRight => BinOp::Shr,
            B::ShiftRightZeroFill => BinOp::UShr,
            B::Exponential => BinOp::Pow,
            _ => bail!("{}: unsupported operator", self.loc(b.span.start)),
        };
        self.arith(op, l, r)
    }

    pub(crate) fn arith(&mut self, op: BinOp, l: Expr, r: Expr) -> R<Expr> {
        if let Some(big) = self.bigint_binary(op, &l, &r)? {
            return Ok(big);
        }
        let (l, r) = (self.as_num(l)?, self.as_num(r)?);
        // Constant folding, so `const UNITS = 10 / 729` is a constant rather than a global.
        if let (ExprKind::Num(a), ExprKind::Num(b)) = (&l.kind, &r.kind) {
            let v = match op {
                BinOp::Add => Some(a + b),
                BinOp::Sub => Some(a - b),
                BinOp::Mul => Some(a * b),
                BinOp::Div => Some(a / b),
                BinOp::Rem => Some(a % b),
                BinOp::Pow => Some(js_pow(*a, *b)),
                BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr
                | BinOp::UShr => Some(fold_bits(op, *a, *b)),
                _ => None,
            };
            if let Some(v) = v {
                return Ok(num(v));
            }
        }
        Ok(mk(ExprKind::Bin(op, Box::new(l), Box::new(r)), Ty::Num))
    }

    pub(crate) fn lower_compare(&mut self, op: CmpOp, l: Expr, r: Expr, at: u32) -> R<Expr> {
        if let Some(big) = self.bigint_compare(op, &l, &r)? {
            return Ok(big);
        }
        let eq = matches!(op, CmpOp::Eq | CmpOp::Ne);
        if eq && (l.ty == Ty::Null || r.ty == Ty::Null) {
            let mut other = if l.ty == Ty::Null { r } else { l };
            // An engine handle is 0 for "no object" (`go.parent`, `GameObject.find`): the manifest
            // types them as handles, so `=== null` tests the 0.
            if matches!(other.ty, Ty::Handle(_)) {
                other.ty = other.ty.nullable();
            }
            let is_null = if other.ty.is_nullable() {
                mk(ExprKind::IsNull(Box::new(other)), Ty::Bool)
            } else {
                mk(ExprKind::Bool(false), Ty::Bool)
            };
            return Ok(if op == CmpOp::Eq {
                is_null
            } else {
                mk(ExprKind::Un(UnOp::Not, Box::new(is_null)), Ty::Bool)
            });
        }
        // An optional boolean against a boolean (`props.on === false`): null is neither.
        if eq {
            let nb = |t: &Ty| matches!(t, Ty::Nullable(i) if **i == Ty::Bool);
            if (nb(&l.ty) && r.ty == Ty::Bool) || (nb(&r.ty) && l.ty == Ty::Bool) {
                let (lt, rt) = (self.new_local(l.ty.clone()), self.new_local(r.ty.clone()));
                let (lv, rv) = (self.var_expr(lt), self.var_expr(rt));
                let both = |this: &mut Self, v: Expr| -> R<(Expr, Expr)> {
                    let present = if v.ty.is_nullable() {
                        let is_null = mk(ExprKind::IsNull(Box::new(v.clone())), Ty::Bool);
                        mk(ExprKind::Un(UnOp::Not, Box::new(is_null)), Ty::Bool)
                    } else {
                        mk(ExprKind::Bool(true), Ty::Bool)
                    };
                    Ok((present, this.coerce(v, &Ty::Bool)?))
                };
                let (lp, lb) = both(self, lv)?;
                let (rp, rb) = both(self, rv)?;
                let same = mk(
                    ExprKind::Cmp(CmpOp::Eq, Box::new(lb), Box::new(rb)),
                    Ty::Bool,
                );
                let present = mk(ExprKind::And(Box::new(lp), Box::new(rp)), Ty::Bool);
                let all = mk(ExprKind::And(Box::new(present), Box::new(same)), Ty::Bool);
                let all = if op == CmpOp::Eq {
                    all
                } else {
                    mk(ExprKind::Un(UnOp::Not, Box::new(all)), Ty::Bool)
                };
                return Ok(mk(
                    ExprKind::Seq(
                        vec![Stmt::Assign(lt, l), Stmt::Assign(rt, r)],
                        Box::new(all),
                    ),
                    Ty::Bool,
                ));
            }
        }
        // A union member against a `string` is compared as a string.
        let (lt, rt) = (l.ty.strip(), r.ty.strip());
        let (l, r) = if (lt == &Ty::Lit && rt == &Ty::Str)
            || (lt == &Ty::Str && rt == &Ty::Lit)
            || (!eq && lt == &Ty::Lit && rt == &Ty::Lit)
        {
            let (l, r) = (self.widen_lit(l), self.widen_lit(r));
            (l, r)
        } else {
            (l, r)
        };
        let (a, b) = (l.ty.strip().clone(), r.ty.strip().clone());
        if !eq && a == Ty::Str && b == Ty::Str {
            return Ok(self.str_order(op, l, r));
        }
        if a != b && !self.assignable(&a, &b) && !self.assignable(&b, &a) {
            bail!("{}: cannot compare {a} with {b}", self.loc(at));
        }
        if !eq && a != Ty::Num {
            bail!("{}: ordering comparison on {a}", self.loc(at));
        }
        Ok(mk(ExprKind::Cmp(op, Box::new(l), Box::new(r)), Ty::Bool))
    }

    /// An expression used as a condition (`if`, loops, `?:`, `!`): its truthiness. `&&` and `||`
    /// stay boolean here, whatever their operands, which is what a test wants.
    pub(crate) fn lower_cond(&mut self, e: &'a Expression<'a>) -> R<Expr> {
        match e {
            Expression::ParenthesizedExpression(p) => self.lower_cond(&p.expression),
            Expression::LogicalExpression(l) if l.operator != LogicalOperator::Coalesce => {
                let a = self.lower_cond(&l.left)?;
                let n = if self.unions.is_empty() {
                    vec![]
                } else {
                    self.narrowings(&l.left, l.operator == LogicalOperator::And)
                };
                let b = self.with_narrowed(n, |s| s.lower_cond(&l.right))?;
                let k = if l.operator == LogicalOperator::And {
                    ExprKind::And(Box::new(a), Box::new(b))
                } else {
                    ExprKind::Or(Box::new(a), Box::new(b))
                };
                Ok(mk(k, Ty::Bool))
            }
            Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
                let b = self.lower_cond(&u.argument)?;
                Ok(mk(ExprKind::Un(UnOp::Not, Box::new(b)), Ty::Bool))
            }
            _ => {
                let v = self.lower_expr(e, None)?;
                Ok(self.bool_of(v))
            }
        }
    }

    fn lower_logical(&mut self, l: &'a LogicalExpression<'a>, exp: Option<&Ty>) -> R<Expr> {
        match l.operator {
            LogicalOperator::Coalesce => {
                let a = self.lower_expr(&l.left, None)?;
                let want = a.ty.strip().clone();
                // `active.get(k) ?? []` over sets and maps: the empty collection.
                let empty_array =
                    matches!(&l.right, Expression::ArrayExpression(arr) if arr.elements.is_empty());
                let b = if matches!(want, Ty::Set(_) | Ty::Map(..)) && empty_array {
                    self.new_collection(&want)?
                } else if let (Ty::Typed(k), true) = (&want, empty_array) {
                    // `data.uv ?? []` over a typed array: an empty one.
                    let at = self.loc(l.span.start);
                    self.typed_from(*k, num(0.0), &at)?
                } else {
                    self.lower_expr(&l.right, Some(&want))?
                };
                // `arg("--steps") ?? 1`: a string or a number, a union of the two.
                let scalar = |t: &Ty| matches!(t, Ty::Str | Ty::Lit | Ty::Num | Ty::Bool);
                if scalar(&want)
                    && scalar(b.ty.strip())
                    && want != *b.ty.strip()
                    && !matches!(
                        (&want, b.ty.strip()),
                        (Ty::Str, Ty::Lit) | (Ty::Lit, Ty::Str)
                    )
                    && exp.is_none_or(|t| t.strip() != &want)
                {
                    let wa = if want == Ty::Lit {
                        Ty::Str
                    } else {
                        want.clone()
                    };
                    let b = self.widen_lit(b);
                    let ty = self.make_union(vec![wa.clone(), b.ty.strip().clone()]);
                    let tmp = self.new_local(a.ty.clone());
                    let t = self.var_expr(tmp);
                    let plain = mk(ExprKind::Cast(Box::new(t.clone())), want.clone());
                    let plain = self.widen_lit(plain);
                    let kept = self.coerce(plain, &ty)?;
                    let alt = self.coerce(b, &ty)?;
                    let cond = mk(
                        ExprKind::Cond(
                            Box::new(mk(ExprKind::IsNull(Box::new(t)), Ty::Bool)),
                            Box::new(alt),
                            Box::new(kept),
                        ),
                        ty.clone(),
                    );
                    return Ok(mk(
                        ExprKind::Seq(vec![Stmt::Assign(tmp, a)], Box::new(cond)),
                        ty,
                    ));
                }
                let ty = if b.ty.is_nullable() {
                    want.clone().nullable()
                } else {
                    want.clone()
                };
                let tmp = self.new_local(a.ty.clone());
                let t = self.var_expr(tmp);
                let kept = self.coerce(t.clone(), &ty)?;
                let alt = self.coerce(b, &ty)?;
                let cond = mk(
                    ExprKind::Cond(
                        Box::new(mk(ExprKind::IsNull(Box::new(t)), Ty::Bool)),
                        Box::new(alt),
                        Box::new(kept),
                    ),
                    ty.clone(),
                );
                Ok(mk(
                    ExprKind::Seq(vec![Stmt::Assign(tmp, a)], Box::new(cond)),
                    ty,
                ))
            }
            op => {
                let _ = exp;
                let a = self.lower_expr(&l.left, None)?;
                let n = if self.unions.is_empty() {
                    vec![]
                } else {
                    self.narrowings(&l.left, op == LogicalOperator::And)
                };
                let b = self.with_narrowed(n, |s| s.lower_expr(&l.right, None))?;
                // `x || 5`, `a && b` on values: the operand, as JavaScript gives it, when both
                // sides have one type. Mixed sides (`obj && obj.x > 0`) stay boolean, which is
                // what every condition wants.
                let (ak, bk) = (a.ty.strip().clone(), b.ty.strip().clone());
                let value_typed = ak != Ty::Bool
                    && (ak == bk
                        || (ak == Ty::Lit && bk == Ty::Str)
                        || (ak == Ty::Str && bk == Ty::Lit));
                if value_typed {
                    // A string literal beside a string is a string.
                    let (a, b) = (
                        if ak == Ty::Lit { self.widen_lit(a) } else { a },
                        if bk == Ty::Lit { self.widen_lit(b) } else { b },
                    );
                    let or = op == LogicalOperator::Or;
                    let mut ty = self.unify(&a.ty, &b.ty)?;
                    // `x || 5` with a nullable x is never null: a null x is falsy.
                    if or && !b.ty.is_nullable() {
                        ty = ty.strip().clone();
                    }
                    let tmp = self.new_local(a.ty.clone());
                    let t = self.var_expr(tmp);
                    let test = self.bool_of(t.clone());
                    let kept = if a.ty.is_nullable() && !ty.is_nullable() {
                        mk(ExprKind::Cast(Box::new(t)), ty.clone())
                    } else {
                        self.coerce(t, &ty)?
                    };
                    let other = self.coerce(b, &ty)?;
                    let (yes, no) = if or { (kept, other) } else { (other, kept) };
                    let cond = mk(
                        ExprKind::Cond(Box::new(test), Box::new(yes), Box::new(no)),
                        ty.clone(),
                    );
                    return Ok(mk(
                        ExprKind::Seq(vec![Stmt::Assign(tmp, a)], Box::new(cond)),
                        ty,
                    ));
                }
                // `name && lookup(name)`: a string (or number, or boolean) and an object. The
                // falsy left operands (`""`, `0`, `false`, `undefined`) make the result null.
                if op == LogicalOperator::And
                    && matches!(ak, Ty::Str | Ty::Lit | Ty::Num | Ty::Bool)
                    && (bk.is_ptr() || matches!(bk, Ty::Handle(_)))
                {
                    let nty = b.ty.strip().clone().nullable();
                    let tmp = self.new_local(a.ty.clone());
                    let test = self.bool_of(self.var_expr(tmp));
                    let yes = mk(ExprKind::Cast(Box::new(b)), nty.clone());
                    let no = mk(ExprKind::Null, nty.clone());
                    return Ok(mk(
                        ExprKind::Seq(
                            vec![Stmt::Assign(tmp, a)],
                            Box::new(mk(
                                ExprKind::Cond(Box::new(test), Box::new(yes), Box::new(no)),
                                nty.clone(),
                            )),
                        ),
                        nty,
                    ));
                }
                // `s.slice(i) || null`: the operand when it is truthy, else null.
                if op == LogicalOperator::Or && bk == Ty::Null && ak != Ty::Bool && ak != Ty::Null {
                    let a = if ak == Ty::Lit { self.widen_lit(a) } else { a };
                    let nty = a.ty.strip().clone().nullable();
                    let tmp = self.new_local(a.ty.clone());
                    let test = self.bool_of(self.var_expr(tmp));
                    let t = self.var_expr(tmp);
                    let yes = self.coerce(t, &nty)?;
                    let no = mk(ExprKind::Null, nty.clone());
                    return Ok(mk(
                        ExprKind::Seq(
                            vec![Stmt::Assign(tmp, a)],
                            Box::new(mk(
                                ExprKind::Cond(Box::new(test), Box::new(yes), Box::new(no)),
                                nty.clone(),
                            )),
                        ),
                        nty,
                    ));
                }
                // `cond && f()` with a void `f` (a statement, or an arrow's body): the call runs,
                // and its `undefined` is falsy.
                let b = if bk == Ty::Void {
                    mk(
                        ExprKind::Seq(
                            vec![Stmt::Eval(b)],
                            Box::new(mk(ExprKind::Bool(false), Ty::Bool)),
                        ),
                        Ty::Bool,
                    )
                } else {
                    b
                };
                let (a, b) = (self.bool_of(a), self.bool_of(b));
                let k = if op == LogicalOperator::And {
                    ExprKind::And(Box::new(a), Box::new(b))
                } else {
                    ExprKind::Or(Box::new(a), Box::new(b))
                };
                Ok(mk(k, Ty::Bool))
            }
        }
    }

    // ---- objects and arrays ----

    /// The string literal an object literal gives its property `tag` (`kind: "quote"`).
    fn literal_tag(o: &ObjectExpression<'_>, tag: &str) -> Option<String> {
        o.properties.iter().find_map(|p| match p {
            ObjectPropertyKind::ObjectProperty(p)
                if matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == tag) =>
            {
                match &p.value {
                    Expression::StringLiteral(s) => Some(s.value.to_string()),
                    _ => None,
                }
            }
            _ => None,
        })
    }

    /// The computed properties `[key]: value` of an object literal set on `base` (the object made
    /// without them): `key` is a name whose union of string literals names fields of one type.
    fn set_computed_keys(&mut self, base: Expr, props: &'a [ObjectPropertyKind<'a>]) -> R<Expr> {
        let Ty::Obj(sid) = base.ty.strip().clone() else {
            bail!("computed keys on {}", base.ty)
        };
        let ty = base.ty.clone();
        let obj = self.new_local(ty.clone());
        let mut stmts = vec![Stmt::Assign(obj, base)];
        for p in props {
            let ObjectPropertyKind::ObjectProperty(p) = p else {
                unreachable!("only computed properties")
            };
            let at = self.loc(p.span.start);
            let (PropertyKey::Identifier(id), Some(key_expr)) = (&p.key, p.key.as_expression())
            else {
                bail!("{at}: computed key")
            };
            let Some(names) = self.cur.lit_sets.get(id.name.as_str()).cloned() else {
                bail!(
                    "{at}: a computed key in an object literal must be a union of the object's field names"
                )
            };
            let fields = self.m.structs[sid].fields.clone();
            let picked: Vec<usize> = (0..fields.len())
                .filter(|i| names.contains(&fields[*i].name))
                .collect();
            let Some(&first) = picked.first() else {
                bail!("{at}: `{}` names none of the fields", id.name)
            };
            let fty = fields[first].ty.clone();
            if picked.iter().any(|i| fields[*i].ty != fty) {
                bail!(
                    "{at}: the fields `{}` can name have different types",
                    id.name
                )
            }
            let key = self.lower_expr(key_expr, None)?;
            let kv = self.new_local(key.ty.clone());
            stmts.push(Stmt::Assign(kv, key));
            let v = self.lower_expr(&p.value, Some(&fty))?;
            let v = self.coerce(v, &fty)?;
            let vv = self.new_local(fty.clone());
            stmts.push(Stmt::Assign(vv, v));
            for i in picked {
                let tag = self.tag(&fields[i].name);
                let is = mk(
                    ExprKind::Cmp(
                        CmpOp::Eq,
                        Box::new(self.var_expr(kv)),
                        Box::new(mk(ExprKind::Tag(tag), Ty::Lit)),
                    ),
                    Ty::Bool,
                );
                let set = Stmt::StoreField(self.var_expr(obj), sid, i, self.var_expr(vv));
                stmts.push(Stmt::If(is, vec![set], vec![]));
            }
        }
        Ok(mk(ExprKind::Seq(stmts, Box::new(self.var_expr(obj))), ty))
    }

    pub(crate) fn lower_object(
        &mut self,
        o: &'a ObjectExpression<'a>,
        exp: Option<&Ty>,
        record: Option<&Ty>,
    ) -> R<Expr> {
        // `{ ...f, [key]: !f[key] }` with `key` a union of field names: the object without the
        // computed properties, then the field the key names set from each.
        let skip = std::mem::take(&mut self.skip_computed);
        // `{ ...split, children }` where a `DockGroup | DockSplit` goes: the object is the member
        // its spread is (the caller wraps it in the union).
        if let Some(members) = exp.and_then(|t| self.union_members(t)) {
            // Else the member whose fields are the keys written (`{ text, file, line, col }` is
            // the link segment, not `{ text }`).
            let keys: Option<Vec<String>> = o
                .properties
                .iter()
                .map(|p| match p {
                    ObjectPropertyKind::ObjectProperty(p) if !p.computed => {
                        p.key.static_name().map(|n| n.to_string())
                    }
                    _ => None,
                })
                .collect();
            let mut member = keys.and_then(|keys| {
                members
                    .iter()
                    .find(|m| match m.strip() {
                        Ty::Obj(s) => {
                            let fs = &self.m.structs[*s].fields;
                            keys.iter().all(|k| fs.iter().any(|f| f.name == *k))
                                && fs.iter().all(|f| f.optional || keys.contains(&f.name))
                        }
                        _ => false,
                    })
                    .map(|m| m.strip().clone())
            });
            for p in &o.properties {
                if let ObjectPropertyKind::SpreadProperty(s) = p {
                    let mark = self.cur.locals.len();
                    let probe = self.lower_expr(&s.argument, None);
                    self.cur.locals.truncate(mark);
                    if let Ok(v) = probe
                        && let Some(m) = members.iter().find(|m| m.strip() == v.ty.strip())
                    {
                        member = Some(m.strip().clone());
                    }
                }
            }
            self.skip_computed = skip;
            return self.lower_object(o, member.as_ref(), record);
        }
        let computed = |p: &ObjectPropertyKind<'_>| {
            matches!(p, ObjectPropertyKind::ObjectProperty(p)
                if p.computed && p.kind == PropertyKind::Init
                    && matches!(&p.key, PropertyKey::Identifier(_)))
        };
        if !skip
            && let Some(first) = o.properties.iter().position(computed)
            && o.properties[first..].iter().all(computed)
            && let Some(Ty::Obj(_)) = exp.map(|t| t.strip())
        {
            self.skip_computed = true;
            let base = self.lower_object(o, exp, record);
            return self.set_computed_keys(base?, &o.properties[first..]);
        }
        let exp_sid = match exp.map(|t| t.strip()) {
            Some(Ty::Obj(s)) => Some(*s),
            _ => None,
        };
        // An interface some class implements is only a type (`ifaces.rs`): the object gets a
        // struct of its own, with the types of the interface for its values.
        let want_sid = exp_sid;
        let exp_sid = exp_sid.filter(|s| !self.is_dyn(*s));
        // `{ a: 1, b: 2 }` where a `Record<string, V>` (a map) is expected.
        if let Some(t @ Ty::Map(k, _)) = exp.map(|t| t.strip())
            && **k == Ty::Str
        {
            let t = t.clone();
            return self.lower_record_map(o, &t);
        }
        // `{ [Weather.Rain]: "Rain" }` where a `Record<Weather, string>` (an array) is expected.
        if let Some(Ty::Arr(el)) = exp.map(|t| t.strip())
            && o.properties
                .iter()
                .all(|p| matches!(p, ObjectPropertyKind::ObjectProperty(p) if p.computed))
            && !o.properties.is_empty()
        {
            let el = (**el).clone();
            return self.lower_enum_record(o, el);
        }
        // `{ width: 10, ...other }` where a `PlatypusStyle` (a list of `StyleEntry`) is expected.
        if let Some(Ty::Arr(el)) = exp.map(|t| t.strip())
            && let Ty::Obj(esid) = el.strip()
            && self.m.structs[*esid].name == "StyleEntry"
            && self.fids.contains_key("__se_num")
        {
            return self.lower_style_bag(o, exp.expect("checked").strip().clone());
        }
        // Lower every value first (with the field type as context when the struct is known), in
        // source order: a later spread or property wins.
        enum Item<'b> {
            Spread(Expr),
            Prop(String, Expr),
            /// `...dict` into an object of a known type: copied by key at run time.
            Dict(Expr),
            /// A method or accessor that uses `this`: lowered once the object's struct exists.
            Pending(&'b ObjectProperty<'b>),
        }
        let mut items: Vec<Item<'a>> = Vec::new();
        for p in &o.properties {
            match p {
                ObjectPropertyKind::SpreadProperty(s) => {
                    let e = self.lower_expr(&s.argument, exp)?;
                    if let Ty::Map(k, _) = e.ty.strip()
                        && **k == Ty::Str
                    {
                        if exp_sid.is_some() {
                            items.push(Item::Dict(e));
                            continue;
                        }
                        // No object type is expected: a dictionary spread makes a dictionary.
                        let ty = e.ty.strip().clone();
                        return self.lower_record_map(o, &ty);
                    }
                    if !matches!(e.ty.strip(), Ty::Obj(_)) {
                        bail!(
                            "{}: spread of {}, not an object",
                            self.loc(s.span.start),
                            e.ty
                        )
                    }
                    items.push(Item::Spread(e));
                }
                ObjectPropertyKind::ObjectProperty(p) => {
                    if skip && p.computed {
                        continue;
                    }
                    if self.uses_this(p) {
                        items.push(Item::Pending(p));
                        continue;
                    }
                    // `get scene() { .. }`: the closure goes in the hidden field, the real one
                    // holds a placeholder.
                    if p.kind == PropertyKind::Get
                        && !p.computed
                        && let Expression::FunctionExpression(f) = &p.value
                    {
                        let name = self.key_name(&p.key)?;
                        let hidden = accessors::thunk_name(&name);
                        let want = match want_sid {
                            Some(s) => self.m.structs[s]
                                .field(&hidden)
                                .map(|i| self.m.structs[s].fields[i].ty.strip().clone()),
                            None => None,
                        };
                        let c = self.lower_closure(closure::ClosureNode::Func(f), want.as_ref())?;
                        let Ty::Fn(sig) = c.ty.strip().clone() else {
                            unreachable!("a closure")
                        };
                        let placeholder = self.default_value(&sig.ret);
                        items.push(Item::Prop(name, placeholder));
                        items.push(Item::Prop(hidden, c));
                        continue;
                    }
                    // `set v(n) { .. }`: the closure goes in the hidden field; the real one holds
                    // a placeholder (a write calls the closure).
                    if p.kind == PropertyKind::Set
                        && !p.computed
                        && let Expression::FunctionExpression(f) = &p.value
                    {
                        let name = self.key_name(&p.key)?;
                        let hidden = accessors::setter_name(&name);
                        let want = match want_sid {
                            Some(s) => self.m.structs[s]
                                .field(&hidden)
                                .map(|i| self.m.structs[s].fields[i].ty.strip().clone()),
                            None => None,
                        };
                        let c = self.lower_closure(closure::ClosureNode::Func(f), want.as_ref())?;
                        let Ty::Fn(sig) = c.ty.strip().clone() else {
                            unreachable!("a closure")
                        };
                        let Some(value_ty) = sig.params.first() else {
                            bail!("{}: a setter takes one parameter", self.loc(p.span.start))
                        };
                        let placeholder = self.default_value(value_ty);
                        items.push(Item::Prop(name, placeholder));
                        items.push(Item::Prop(hidden, c));
                        continue;
                    }
                    // `at(t) { .. }` is the property `at: function (t) { .. }` (no `this`).
                    if (p.computed && !matches!(p.key, PropertyKey::StringLiteral(_)))
                        || p.kind != PropertyKind::Init
                    {
                        bail!(
                            "{}: computed property or accessor in an object literal",
                            self.loc(p.span.start)
                        );
                    }
                    let name = self.key_name(&p.key)?;
                    let want = match want_sid {
                        Some(s) => self.m.structs[s]
                            .field(&name)
                            .map(|i| self.m.structs[s].fields[i].ty.clone()),
                        None => record.cloned(),
                    };
                    // `{ kind: "quote", children: [..] }` where a discriminated union with
                    // per-variant field types goes: the literal's tag says which type it has.
                    let want = match want_sid.and_then(|s| self.discs.get(&s).cloned()) {
                        Some(d) => match Self::literal_tag(o, &d.tag) {
                            Some(tag) => d
                                .fields
                                .iter()
                                .find(|(f, _)| *f == name)
                                .and_then(|(_, per)| {
                                    let v = d.variants.iter().position(|t| t.contains(&tag))?;
                                    let k = per[v]?;
                                    let members = self.union_members(want.as_ref()?)?;
                                    members.get(k).cloned()
                                })
                                .or(want),
                            None => want,
                        },
                        None => want,
                    };
                    let mut v = self.lower_expr(&p.value, want.as_ref())?;
                    // A property of a fresh object is a `string`, not the literal it was written as.
                    // (`"y" as Kind` keeps the union it was asserted to.)
                    if exp_sid.is_none()
                        && record.is_none()
                        && !(want_sid.is_some() && want.is_some())
                        && !matches!(&p.value, Expression::TSAsExpression(_))
                        && Self::is_fresh_literal(&p.value)
                    {
                        v = self.widen_lit(v);
                    }
                    items.push(Item::Prop(name, v));
                }
            }
        }
        let has_pending = items.iter().any(|i| matches!(i, Item::Pending(_)));
        let sid = match exp_sid {
            Some(s) => s,
            None => {
                // The fields in order of first appearance; a later value takes the type.
                let mut fields: Vec<Field> = Vec::new();
                let mut upsert = |name: String, ty: Ty, optional: bool| match fields
                    .iter_mut()
                    .find(|f| f.name == name)
                {
                    Some(f) => {
                        f.ty = ty;
                        f.optional = optional;
                    }
                    None => fields.push(Field { name, ty, optional }),
                };
                let mut only_spread = None;
                for it in &items {
                    match it {
                        Item::Spread(e) => {
                            let Ty::Obj(s) = e.ty.strip() else {
                                unreachable!("checked")
                            };
                            only_spread = Some(*s);
                            for f in &self.m.structs[*s].fields {
                                upsert(f.name.clone(), f.ty.clone(), f.optional);
                            }
                        }
                        Item::Prop(n, v) => upsert(
                            n.clone(),
                            record.cloned().unwrap_or_else(|| v.ty.clone()),
                            false,
                        ),
                        Item::Dict(_) => {}
                        Item::Pending(p) => {
                            let n = self.key_name(&p.key)?;
                            let ty = self.pending_placeholder(p)?;
                            upsert(n, ty, false)
                        }
                    }
                }
                if has_pending {
                    // The methods' types are not known yet: a struct of its own, patched below.
                    let fields = self.with_thunks(fields);
                    self.new_struct("Anon", fields)
                } else {
                    let single_spread = items.len() == 1 && matches!(items[0], Item::Spread(_));
                    // `{ ...base, name }`: the type of `base` when it already has every property.
                    let covers = only_spread.filter(|s| {
                        !self.class_of_sid.contains_key(s)
                            && items.iter().all(|it| match it {
                                Item::Spread(e) => matches!(e.ty.strip(), Ty::Obj(t) if t == s),
                                Item::Prop(n, v) => self.m.structs[*s].field(n).is_some_and(|i| {
                                    self.m.structs[*s].fields[i].ty.strip() == v.ty.strip()
                                }),
                                Item::Pending(_) | Item::Dict(_) => false,
                            })
                    });
                    match only_spread {
                        // `{ ...a }` is a copy with the type of `a`.
                        Some(s) if single_spread => s,
                        Some(_) if covers.is_some() => covers.expect("checked"),
                        _ => self.anon_struct("Anon", fields),
                    }
                }
            }
        };
        // Methods and accessors that use `this`: `this` is the object, a cell filled once it
        // exists (as the closures of a class capture theirs).
        let mut this_cell: Option<VarId> = None;
        if has_pending {
            let cell_ty = Ty::Cell(Box::new(Ty::Obj(sid)));
            let cell = self.new_local(cell_ty.clone());
            this_cell = Some(cell);
            self.cur.scopes.push(HashMap::new());
            let scope = self.cur.scopes.len() - 1;
            self.cur.scopes[scope].insert("this".to_string(), Binding::Boxed(cell, Ty::Obj(sid)));
            let mut done: Vec<Item<'a>> = Vec::new();
            for it in items {
                match it {
                    Item::Pending(p) => {
                        for (n, v) in self.lower_this_prop(p, sid, exp_sid.is_none())? {
                            done.push(Item::Prop(n, v));
                        }
                    }
                    other => done.push(other),
                }
            }
            items = done;
            self.cur.scopes.pop();
        }
        let tmp = self.new_local(Ty::Obj(sid));
        let t = self.var_expr(tmp);
        let mut stmts = vec![Stmt::Assign(tmp, mk(ExprKind::NewObj(sid), Ty::Obj(sid)))];
        let mut pre: Vec<Stmt> = Vec::new();
        if let Some(cell) = this_cell {
            let zero = self.default_value(&Ty::Obj(sid));
            pre.push(Stmt::Assign(
                cell,
                mk(
                    ExprKind::NewCell(Box::new(zero)),
                    Ty::Cell(Box::new(Ty::Obj(sid))),
                ),
            ));
            stmts.push(Stmt::CellSet(self.var_expr(cell), t.clone()));
        }
        for it in items {
            match it {
                Item::Spread(s) => {
                    let Ty::Obj(ssid) = s.ty.strip().clone() else {
                        unreachable!("checked")
                    };
                    let src = self.new_local(s.ty.clone());
                    stmts.push(Stmt::Assign(src, s));
                    for fi in 0..self.m.structs[ssid].fields.len() {
                        let (name, fty, optional) = {
                            let f = &self.m.structs[ssid].fields[fi];
                            (f.name.clone(), f.ty.clone(), f.optional)
                        };
                        let Some(i) = self.m.structs[sid].field(&name) else {
                            continue;
                        };
                        let want = self.m.structs[sid].fields[i].ty.clone();
                        let read = mk(
                            ExprKind::Field(Box::new(self.var_expr(src)), ssid, fi),
                            fty.clone(),
                        );
                        let read = self.coerce(read, &want)?;
                        let store = Stmt::StoreField(t.clone(), sid, i, read);
                        // An optional property that is not there (a `Partial`) leaves what an
                        // earlier part of the literal set.
                        if optional && fty.is_nullable() {
                            let absent = mk(
                                ExprKind::IsNull(Box::new(mk(
                                    ExprKind::Field(Box::new(self.var_expr(src)), ssid, fi),
                                    fty,
                                ))),
                                Ty::Bool,
                            );
                            stmts.push(Stmt::If(absent, vec![], vec![store]));
                        } else {
                            stmts.push(store);
                        }
                    }
                }
                Item::Pending(_) => unreachable!("lowered above"),
                Item::Dict(d) => {
                    let Ty::Map(_, vty) = d.ty.strip().clone() else {
                        unreachable!("checked")
                    };
                    self.assign_map_into_struct(&mut stmts, t.clone(), sid, d, &vty)?;
                }
                Item::Prop(name, v) => {
                    let Some(i) = self.m.structs[sid].field(&name) else {
                        bail!("{} has no field `{name}`", self.m.structs[sid].name)
                    };
                    let fty = self.m.structs[sid].fields[i].ty.clone();
                    let v = self.coerce(v, &fty)?;
                    stmts.push(Stmt::StoreField(t.clone(), sid, i, v));
                }
            }
        }
        // Fields that are nullable and not given start as null, not as a zero number.
        let given: Vec<String> = Vec::new();
        let _ = given;
        for i in 0..self.m.structs[sid].fields.len() {
            let fty = self.m.structs[sid].fields[i].ty.clone();
            if fty.is_nullable()
                && !stmts
                    .iter()
                    .any(|s| matches!(s, Stmt::StoreField(_, _, j, _) if *j == i))
            {
                stmts.insert(
                    1,
                    Stmt::StoreField(t.clone(), sid, i, mk(ExprKind::Null, fty)),
                );
            }
        }
        pre.extend(stmts);
        Ok(mk(ExprKind::Seq(pre, Box::new(t)), Ty::Obj(sid)))
    }

    /// Does this property of an object literal bind `this` to the object: a method, getter or
    /// setter (a function expression) whose body mentions `this`.
    fn uses_this(&self, p: &ObjectProperty<'_>) -> bool {
        !p.computed
            && matches!(&p.value, Expression::FunctionExpression(f)
                if self.closure_free.get(&f.span.start).is_some_and(|v| v.iter().any(|n| n == "this")))
    }

    /// The type a method or accessor that uses `this` has in the object's struct until its body
    /// has been lowered: from its annotations (a getter's return type, a setter's parameter, a
    /// method's signature), with a number or nothing where it has none.
    fn pending_placeholder(&mut self, p: &'a ObjectProperty<'a>) -> R<Ty> {
        let Expression::FunctionExpression(f) = &p.value else {
            unreachable!("uses_this")
        };
        let mut params = Vec::new();
        for q in &f.params.items {
            params.push(match &q.type_annotation {
                Some(a) => self.ty(&a.type_annotation, "method")?,
                None => Ty::Num,
            });
        }
        let ret = match &f.return_type {
            Some(r) => self.ty(&r.type_annotation, "method")?,
            None => Ty::Void,
        };
        Ok(match p.kind {
            PropertyKind::Get => match &f.return_type {
                Some(_) => ret,
                None => Ty::Num,
            },
            PropertyKind::Set => params.first().cloned().unwrap_or(Ty::Num),
            PropertyKind::Init => Ty::Fn(Box::new(FnTy { params, ret })),
        })
    }

    /// A method or accessor that uses `this`, lowered with `this` bound to the object being made
    /// (the binding is in scope). With `patch`, the struct fields it fills get its real types.
    fn lower_this_prop(
        &mut self,
        p: &'a ObjectProperty<'a>,
        sid: StructId,
        patch: bool,
    ) -> R<Vec<(String, Expr)>> {
        let Expression::FunctionExpression(f) = &p.value else {
            unreachable!("uses_this")
        };
        let name = self.key_name(&p.key)?;
        let field_ty = |this: &Self, n: &str| {
            this.m.structs[sid]
                .field(n)
                .map(|i| this.m.structs[sid].fields[i].ty.strip().clone())
        };
        let set_ty = |this: &mut Self, n: &str, ty: Ty| {
            if let Some(i) = this.m.structs[sid].field(n) {
                this.m.structs[sid].fields[i].ty = ty;
            }
        };
        match p.kind {
            PropertyKind::Get => {
                let hidden = accessors::thunk_name(&name);
                let want = if patch { None } else { field_ty(self, &hidden) };
                let c = self.lower_closure(closure::ClosureNode::Func(f), want.as_ref())?;
                let Ty::Fn(sig) = c.ty.strip().clone() else {
                    unreachable!("a closure")
                };
                if patch {
                    set_ty(self, &name, sig.ret.clone());
                    set_ty(self, &hidden, Ty::Fn(sig.clone()).nullable());
                }
                let placeholder = self.default_value(&sig.ret);
                Ok(vec![(name, placeholder), (hidden, c)])
            }
            PropertyKind::Set => {
                let hidden = accessors::setter_name(&name);
                let want = if patch { None } else { field_ty(self, &hidden) };
                let c = self.lower_closure(closure::ClosureNode::Func(f), want.as_ref())?;
                let Ty::Fn(sig) = c.ty.strip().clone() else {
                    unreachable!("a closure")
                };
                let Some(value_ty) = sig.params.first().cloned() else {
                    bail!("{}: a setter takes one parameter", self.loc(p.span.start))
                };
                if patch {
                    set_ty(self, &name, value_ty.clone());
                    set_ty(self, &hidden, Ty::Fn(sig.clone()).nullable());
                }
                let placeholder = self.default_value(&value_ty);
                Ok(vec![(name, placeholder), (hidden, c)])
            }
            PropertyKind::Init => {
                let want = if patch { None } else { field_ty(self, &name) };
                let c = self.lower_closure(closure::ClosureNode::Func(f), want.as_ref())?;
                if patch {
                    set_ty(self, &name, c.ty.clone());
                }
                Ok(vec![(name, c)])
            }
        }
    }

    /// `{ [Weather.Rain]: v, ... }` as an array indexed by the enum values; keys nobody gave hold the
    /// element type's zero.
    fn lower_enum_record(&mut self, o: &'a ObjectExpression<'a>, el: Ty) -> R<Expr> {
        let mut slots: Vec<Option<Expr>> = Vec::new();
        for p in &o.properties {
            let ObjectPropertyKind::ObjectProperty(p) = p else {
                unreachable!("checked")
            };
            let Some(k) = p.key.as_expression() else {
                bail!("{}: a computed key", self.loc(p.span.start))
            };
            let key = self.lower_expr(k, None)?;
            let ExprKind::Num(n) = key.kind else {
                bail!(
                    "{}: a key of a record over an enum must be an enum member",
                    self.loc(p.span.start)
                )
            };
            let i = n as usize;
            let v = self.lower_expr(&p.value, Some(&el))?;
            let v = self.coerce(v, &el)?;
            if slots.len() <= i {
                slots.resize(i + 1, None);
            }
            slots[i] = Some(v);
        }
        let elems = slots
            .into_iter()
            .map(|s| Elem::One(s.unwrap_or_else(|| self.default_value(&el))))
            .collect();
        self.build_array(elems, el)
    }

    /// The `StyleEntry` for a style property `name` with value `v` (`None` for `null`).
    fn style_entry(&mut self, name: &str, v: Expr, ety: &Ty, at: &str) -> R<Option<Expr>> {
        let v = self.widen_lit(v);
        let (f, v) = match v.ty.clone() {
            Ty::Null => return Ok(None),
            Ty::Num => ("__se_num", v),
            Ty::Str => ("__se_str", v),
            Ty::Nullable(inner) if *inner == Ty::Num => ("__se_onum", v),
            Ty::Nullable(inner) if *inner == Ty::Str => ("__se_ostr", v),
            t if self.is_style_value(t.strip()) => (
                if t.is_nullable() {
                    "__se_osv"
                } else {
                    "__se_sv"
                },
                v,
            ),
            t => bail!(
                "{at}: style property `{name}` has type {t}; a style takes numbers and strings"
            ),
        };
        let key = self.str_lit(name);
        Ok(Some(mk(
            ExprKind::Call(self.fids[f], vec![key, v]),
            ety.clone(),
        )))
    }

    /// An object literal as a `PlatypusStyle`: a `StyleEntry` per property, spreads concatenated, so
    /// a later entry of the same name wins when they are applied. A value that may be undefined
    /// makes an entry that applies nothing.
    fn lower_style_bag(&mut self, o: &'a ObjectExpression<'a>, aty: Ty) -> R<Expr> {
        let Ty::Arr(ety) = aty.clone() else {
            unreachable!("checked")
        };
        let mut elems = Vec::new();
        for p in &o.properties {
            match p {
                ObjectPropertyKind::SpreadProperty(s) => {
                    let e = self.lower_expr(&s.argument, Some(&aty))?;
                    // `{ ...base, opacity }` where `base` is a plain object: an entry per field.
                    if let Ty::Obj(osid) = e.ty.strip().clone()
                        && !self.is_style_value(e.ty.strip())
                    {
                        let src = self.new_local(e.ty.clone());
                        let at = self.loc(s.span.start);
                        let mut entries = Vec::new();
                        for fi in 0..self.m.structs[osid].fields.len() {
                            let (name, fty) = {
                                let f = &self.m.structs[osid].fields[fi];
                                (f.name.clone(), f.ty.clone())
                            };
                            let read =
                                mk(ExprKind::Field(Box::new(self.var_expr(src)), osid, fi), fty);
                            if let Some(entry) = self.style_entry(&name, read, &ety, &at)? {
                                entries.push(entry);
                            }
                        }
                        if let Some(first) = entries.first_mut() {
                            let inner = std::mem::replace(first, num(0.0));
                            let ty = inner.ty.clone();
                            *first = mk(
                                ExprKind::Seq(vec![Stmt::Assign(src, e)], Box::new(inner)),
                                ty,
                            );
                        }
                        elems.extend(entries.into_iter().map(Elem::One));
                        continue;
                    }
                    let e = if e.ty.is_nullable() {
                        // Spreading `undefined` adds nothing.
                        let tmp = self.new_local(e.ty.clone());
                        let empty = self.build_array(vec![], (*ety).clone())?;
                        let isnull = mk(ExprKind::IsNull(Box::new(self.var_expr(tmp))), Ty::Bool);
                        let plain = mk(ExprKind::Cast(Box::new(self.var_expr(tmp))), aty.clone());
                        mk(
                            ExprKind::Seq(
                                vec![Stmt::Assign(tmp, e)],
                                Box::new(mk(
                                    ExprKind::Cond(
                                        Box::new(isnull),
                                        Box::new(empty),
                                        Box::new(plain),
                                    ),
                                    aty.clone(),
                                )),
                            ),
                            aty.clone(),
                        )
                    } else {
                        e
                    };
                    elems.push(Elem::Spread(self.coerce(e, &aty)?));
                }
                ObjectPropertyKind::ObjectProperty(p) => {
                    if p.computed || p.method {
                        bail!("{}: computed or method property", self.loc(p.span.start));
                    }
                    let name = self.key_name(&p.key)?;
                    let v = self.lower_expr(&p.value, None)?;
                    let at = self.loc(p.span.start);
                    if let Some(entry) = self.style_entry(&name, v, &ety, &at)? {
                        elems.push(Elem::One(entry));
                    }
                }
            }
        }
        self.build_array(elems, (*ety).clone())
    }

    /// The value of a condition that is a constant (`Bool`, or statements then a `Bool`).
    pub(crate) fn const_bool(e: &Expr) -> Option<bool> {
        match &e.kind {
            ExprKind::Bool(b) => Some(*b),
            ExprKind::Seq(_, last) => Self::const_bool(last),
            _ => None,
        }
    }

    /// A string literal written here (or a choice between some): TypeScript widens its type to
    /// `string` in a mutable position. A value that already has a union type keeps it.
    pub(crate) fn is_fresh_literal(e: &Expression<'_>) -> bool {
        match e {
            Expression::ParenthesizedExpression(p) => Self::is_fresh_literal(&p.expression),
            Expression::ConditionalExpression(c) => {
                Self::is_fresh_literal(&c.consequent) && Self::is_fresh_literal(&c.alternate)
            }
            Expression::LogicalExpression(l) => {
                Self::is_fresh_literal(&l.left) || Self::is_fresh_literal(&l.right)
            }
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => true,
            Expression::NullLiteral(_) | Expression::Identifier(_) => false,
            _ => false,
        }
    }

    fn lower_array_lit(&mut self, a: &'a ArrayExpression<'a>, exp: Option<&Ty>) -> R<Expr> {
        // `[1, "a"]` where a tuple type (an object with fields `0`, `1`, ..) is expected.
        if let Some(Ty::Obj(sid)) = exp.map(|t| t.strip())
            && self.is_tuple(*sid)
            && a.elements.len() <= self.m.structs[*sid].fields.len()
            && self.m.structs[*sid].fields[a.elements.len()..]
                .iter()
                .all(|f| f.ty.is_nullable())
        {
            let sid = *sid;
            let mut vals = Vec::new();
            for (i, item) in a.elements.iter().enumerate() {
                let Some(e) = item.as_expression() else {
                    bail!("{}: spread in a tuple", self.loc(a.span.start))
                };
                let want = self.m.structs[sid].fields[i].ty.clone();
                let v = self.lower_expr(e, Some(&want))?;
                vals.push(self.coerce(v, &want)?);
            }
            // Elements left out (`[x, y]` for `[number, number, "c"?]`) are undefined.
            for i in a.elements.len()..self.m.structs[sid].fields.len() {
                let want = self.m.structs[sid].fields[i].ty.clone();
                vals.push(mk(ExprKind::Null, want));
            }
            return self.build_tuple(sid, vals);
        }
        let mut el = match exp.map(|t| t.strip()) {
            Some(Ty::Arr(e)) => Some((**e).clone()),
            _ => None,
        };
        let el_given = el.is_some();
        let mut elems = Vec::new();
        for item in &a.elements {
            if let ArrayExpressionElement::SpreadElement(sp) = item {
                let hint = el.clone().map(|e| Ty::Arr(Box::new(e)));
                let mut v = self.lower_expr(&sp.argument, hint.as_ref())?;
                v = self.match_view(v);
                // `[...tuple]`: its elements.
                if let Ty::Obj(sid) = v.ty.strip().clone()
                    && self.is_tuple(sid)
                {
                    for e in self.tuple_elements(v, sid) {
                        if el.is_none() {
                            el = Some(e.ty.clone());
                        }
                        elems.push(Elem::One(e));
                    }
                    continue;
                }
                // `[..."text"]`: the code points.
                v = self.string_chars(v);
                // `[...set]`: the set's elements now.
                if let Some(snapshot) = self.collection_snapshot(v.clone()) {
                    v = snapshot;
                }
                // `[...iterable]`: the elements of its iterator.
                if self.is_iterable(&v.ty) {
                    v = self.iterable_to_array(v)?;
                }
                let vt = match v.ty.strip() {
                    Ty::Arr(e) => (**e).clone(),
                    Ty::Typed(_) => Ty::Num,
                    t => bail!("{}: cannot spread {t}", self.loc(sp.span.start)),
                };
                if el.is_none() {
                    el = Some(vt);
                }
                elems.push(Elem::Spread(v));
                continue;
            }
            let Some(e) = item.as_expression() else {
                bail!("{}: hole in an array literal", self.loc(a.span.start))
            };
            let v = self.lower_expr(e, el.as_ref())?;
            // `["a", "b"]` is a `string[]` unless the context says otherwise.
            let v = if el.is_none() && Self::is_fresh_literal(e) {
                self.widen_lit(v)
            } else {
                v
            };
            if el.is_none() {
                el = Some(v.ty.clone());
            }
            elems.push(Elem::One(v));
        }
        // `[0, 1.5, false]` with mixed element types is a tuple: an object with fields `0`, `1`, ..
        let tupleable =
            !el_given && elems.len() >= 2 && elems.iter().all(|e| matches!(e, Elem::One(_)));
        if tupleable {
            match self.build_array(elems.clone(), el.clone().unwrap_or(Ty::Num)) {
                Ok(e) => return Ok(e),
                Err(_) => {
                    let vals: Vec<Expr> = elems
                        .into_iter()
                        .map(|e| match e {
                            Elem::One(v) => v,
                            Elem::Spread(v) => v,
                        })
                        .collect();
                    let fields: Vec<Field> = vals
                        .iter()
                        .enumerate()
                        .map(|(i, v)| Field {
                            name: i.to_string(),
                            ty: v.ty.clone(),
                            optional: false,
                        })
                        .collect();
                    let sid = self.anon_struct("Tuple", fields);
                    return self.build_tuple(sid, vals);
                }
            }
        }
        self.build_array(elems, el.unwrap_or(Ty::Num))
    }

    /// A tuple type: an anonymous object whose fields are named `0`, `1`, ..
    pub(crate) fn is_tuple(&self, sid: StructId) -> bool {
        let d = &self.m.structs[sid];
        d.name == "Tuple"
            && d.fields
                .iter()
                .enumerate()
                .all(|(i, f)| f.name == i.to_string())
    }

    /// The values as a tuple (an object with a field per element), or as an array when they all
    /// have one type: what `[a, b]` is in a type annotation.
    pub(crate) fn tuple_or_array(&mut self, vals: Vec<Expr>) -> R<Expr> {
        let tys: Vec<Ty> = vals.iter().map(|v| v.ty.clone()).collect();
        match tys.first() {
            None => self.build_array(vec![], Ty::Num),
            Some(first) if tys.iter().all(|t| t == first) => {
                let el = first.clone();
                self.build_array(vals.into_iter().map(Elem::One).collect(), el)
            }
            _ => {
                let fields = tys
                    .iter()
                    .enumerate()
                    .map(|(i, t)| Field {
                        name: i.to_string(),
                        ty: t.clone(),
                        optional: false,
                    })
                    .collect();
                let sid = self.anon_struct("Tuple", fields);
                self.build_tuple(sid, vals)
            }
        }
    }

    /// A tuple value read element by element: it goes in a temporary first (set by the first
    /// element's expression).
    pub(crate) fn tuple_elements(&mut self, v: Expr, sid: StructId) -> Vec<Expr> {
        let tmp = self.new_local(v.ty.clone());
        let n = self.m.structs[sid].fields.len();
        (0..n)
            .map(|i| {
                let ty = self.m.structs[sid].fields[i].ty.clone();
                let read = mk(
                    ExprKind::Field(Box::new(self.var_expr(tmp)), sid, i),
                    ty.clone(),
                );
                if i == 0 {
                    mk(
                        ExprKind::Seq(vec![Stmt::Assign(tmp, v.clone())], Box::new(read)),
                        ty,
                    )
                } else {
                    read
                }
            })
            .collect()
    }

    pub(crate) fn build_tuple(&mut self, sid: StructId, vals: Vec<Expr>) -> R<Expr> {
        let ty = Ty::Obj(sid);
        let tmp = self.new_local(ty.clone());
        let mut stmts = vec![Stmt::Assign(tmp, mk(ExprKind::NewObj(sid), ty.clone()))];
        for (i, v) in vals.into_iter().enumerate() {
            let fty = self.m.structs[sid].fields[i].ty.clone();
            let v = self.coerce(v, &fty)?;
            stmts.push(Stmt::StoreField(self.var_expr(tmp), sid, i, v));
        }
        Ok(mk(ExprKind::Seq(stmts, Box::new(self.var_expr(tmp))), ty))
    }

    /// A new array of `el` from values and spread arrays, in order:
    /// `t = new []; t.push(v); for (x of src) t.push(x); ...`.
    pub(crate) fn build_array(&mut self, elems: Vec<Elem>, el: Ty) -> R<Expr> {
        let ty = Ty::Arr(Box::new(el.clone()));
        let tmp = self.new_local(ty.clone());
        let t = self.var_expr(tmp);
        let n = elems.iter().filter(|e| matches!(e, Elem::One(_))).count();
        let mut stmts = vec![Stmt::Assign(
            tmp,
            mk(ExprKind::Rt(RtFn::ArrNew, vec![num(n as f64)]), ty.clone()),
        )];
        let push = |t: &Expr, v: Expr| {
            Stmt::Eval(mk(
                ExprKind::Rt(RtFn::ArrPush, vec![t.clone(), v]),
                Ty::Void,
            ))
        };
        for e in elems {
            match e {
                Elem::One(v) => {
                    let v = self.coerce(v, &el)?;
                    stmts.push(push(&t, v));
                }
                Elem::Spread(src) => {
                    let it = self.begin_iter(src)?;
                    stmts.extend(it.pre);
                    let x = self.var_expr(it.elem);
                    let x = self.coerce(x, &el)?;
                    let mut body = it.head;
                    body.push(push(&t, x));
                    stmts.push(Stmt::Loop(it.label, body));
                }
            }
        }
        Ok(mk(ExprKind::Seq(stmts, Box::new(t)), ty))
    }

    fn lower_new(&mut self, n: &'a NewExpression<'a>, exp: Option<&Ty>) -> R<Expr> {
        if let Expression::StaticMemberExpression(m) = &n.callee
            && matches!(&m.object, Expression::Identifier(o) if o.name == "Intl")
            && matches!(
                m.property.name.as_str(),
                "Collator"
                    | "NumberFormat"
                    | "DateTimeFormat"
                    | "PluralRules"
                    | "ListFormat"
                    | "RelativeTimeFormat"
                    | "Segmenter"
                    | "Locale"
                    | "DisplayNames"
            )
        {
            return self.lower_new_intl(m.property.name.as_str(), &n.arguments, n.span.start);
        }
        // `new (a.constructor as new (n: number) => T)(len)`: another one like `a` (a typed array,
        // or an object of a class of the program).
        {
            let mut callee = &n.callee;
            loop {
                match callee {
                    Expression::ParenthesizedExpression(p) => callee = &p.expression,
                    Expression::TSAsExpression(a) => callee = &a.expression,
                    Expression::TSNonNullExpression(a) => callee = &a.expression,
                    _ => break,
                }
            }
            if let Expression::StaticMemberExpression(m) = callee
                && m.property.name == "constructor"
            {
                let recv = self.lower_expr(&m.object, None)?;
                let made = match recv.ty.strip().clone() {
                    Ty::Typed(k) => self.lower_new_typed(k, n)?,
                    Ty::Obj(sid) if self.class_of_sid.contains_key(&sid) => {
                        let class = self.class_of_sid[&sid].clone();
                        self.new_object(&class, &n.arguments, n.span.start)?
                    }
                    t => bail!(
                        "{}: `.constructor` of {t}: only typed arrays and objects of the program's classes",
                        self.loc(n.span.start)
                    ),
                };
                let ty = made.ty.clone();
                return Ok(mk(
                    ExprKind::Seq(vec![Stmt::Eval(recv)], Box::new(made)),
                    ty,
                ));
            }
        }
        let Expression::Identifier(id) = &n.callee else {
            bail!("{}: unsupported `new`", self.loc(n.span.start))
        };
        match id.name.as_str() {
            // `new Array(n)`: n zeroed slots (a hole reads as undefined in JS; here it is 0 or null).
            "Array" => {
                let el = match (&n.type_arguments, exp.map(|t| t.strip())) {
                    (Some(targs), _) if targs.params.len() == 1 => {
                        self.ty(&targs.params[0], "Array")?
                    }
                    (_, Some(Ty::Arr(e))) => (**e).clone(),
                    _ => Ty::Num,
                };
                let ty = Ty::Arr(Box::new(el));
                let len = match n.arguments.first().and_then(|a| a.as_expression()) {
                    Some(e) => {
                        let v = self.lower_expr(e, Some(&Ty::Num))?;
                        self.as_num(v)?
                    }
                    None => num(0.0),
                };
                if n.arguments.len() > 1 {
                    bail!(
                        "{}: `new Array(a, b, ...)`: use an array literal",
                        self.loc(n.span.start)
                    )
                }
                let (nv, tmp) = (self.new_local(Ty::Num), self.new_local(ty.clone()));
                let stmts = vec![
                    Stmt::Assign(nv, len),
                    Stmt::Assign(
                        tmp,
                        mk(
                            ExprKind::Rt(RtFn::ArrNew, vec![self.var_expr(nv)]),
                            ty.clone(),
                        ),
                    ),
                    Stmt::Eval(mk(
                        ExprKind::Rt(RtFn::ArrSetLen, vec![self.var_expr(tmp), self.var_expr(nv)]),
                        Ty::Void,
                    )),
                ];
                Ok(mk(ExprKind::Seq(stmts, Box::new(self.var_expr(tmp))), ty))
            }
            name if crate::ir::TypedKind::from_name(name).is_some() => {
                let kind = crate::ir::TypedKind::from_name(name).expect("checked");
                self.lower_new_typed(kind, n)
            }
            "Map" | "Set" | "WeakMap" | "WeakSet" => self.lower_new_collection(&id.name, n, exp),
            "WeakRef"
                if self.lookup("WeakRef").is_none() && !self.classes.contains_key("WeakRef") =>
            {
                let at = self.loc(n.span.start);
                let Some(arg) = n.arguments.first().and_then(|a| a.as_expression()) else {
                    bail!("{at}: new WeakRef needs an object")
                };
                let v = self.lower_expr(arg, None)?;
                let Ty::Obj(sid) = v.ty.strip().clone() else {
                    bail!("{at}: a WeakRef holds an object")
                };
                let name = self.m.structs[sid].name.clone();
                self.call_helper(&format!("__newWeakRef${name}"), vec![v])
            }
            "RegExp" => self.lower_new_regexp(&n.arguments, n.span.start),
            "Date" if self.date_sid().is_some() => self.lower_new_date(&n.arguments, n.span.start),
            other if self.manifest.values.contains_key(other) => {
                self.new_value(other, &n.arguments, n.span.start)
            }
            other if self.is_creatable(other) => self.lower_new_handle(other, n),
            other if self.classes.contains_key(other) => {
                self.new_object(other, &n.arguments, n.span.start)
            }
            other if self.generic_classes.contains_key(other) => {
                let targs = self.new_targs(other, n, exp)?;
                let tparams = self.generic_classes[other].tparams.clone();
                let targs = self.complete_targs(other, &tparams, targs)?;
                let inst = self.instantiate_class(other, targs)?;
                self.new_object(&inst, &n.arguments, n.span.start)
            }
            other => bail!("{}: `new {other}` is not supported", self.loc(n.span.start)),
        }
    }

    // ---- assignment ----

    pub(crate) fn place_of(
        &mut self,
        t: &'a SimpleAssignmentTarget<'a>,
        pre: &mut Vec<Stmt>,
    ) -> R<Place> {
        match t {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                self.place_ident(id.name.as_str())
            }
            SimpleAssignmentTarget::StaticMemberExpression(m) => self.place_static(m, pre),
            SimpleAssignmentTarget::PrivateFieldExpression(p) => self.place_private(p, pre),
            SimpleAssignmentTarget::ComputedMemberExpression(m) => self.place_computed(m, pre),
            // `x[i]! += d`: the assertion only matters to `tsc`.
            SimpleAssignmentTarget::TSNonNullExpression(n) => {
                self.place_of_expr(&n.expression, pre)
            }
            SimpleAssignmentTarget::TSAsExpression(n) => self.place_of_expr(&n.expression, pre),
            SimpleAssignmentTarget::TSSatisfiesExpression(n) => {
                self.place_of_expr(&n.expression, pre)
            }
            SimpleAssignmentTarget::TSTypeAssertion(n) => self.place_of_expr(&n.expression, pre),
        }
    }

    /// An expression that is an assignment target under a `!` or a cast.
    fn place_of_expr(&mut self, e: &'a Expression<'a>, pre: &mut Vec<Stmt>) -> R<Place> {
        match e {
            Expression::ParenthesizedExpression(p) => self.place_of_expr(&p.expression, pre),
            Expression::TSNonNullExpression(n) => self.place_of_expr(&n.expression, pre),
            Expression::TSAsExpression(n) => self.place_of_expr(&n.expression, pre),
            Expression::Identifier(id) => self.place_ident(id.name.as_str()),
            Expression::StaticMemberExpression(m) => self.place_static(m, pre),
            Expression::PrivateFieldExpression(p) => self.place_private(p, pre),
            Expression::ComputedMemberExpression(m) => self.place_computed(m, pre),
            _ => bail!("unsupported assignment target"),
        }
    }

    fn place_ident(&mut self, name: &str) -> R<Place> {
        match self.lookup(name) {
            Some(Binding::Direct(v)) => Ok(Place::Var(v, self.cur.locals[v as usize].clone())),
            Some(Binding::Boxed(cell, ty)) => Ok(Place::Cell(self.var_expr(cell), ty)),
            None => match self.consts.get(name) {
                Some(ConstInfo::Global(g, ty, true)) => Ok(Place::Global(*g, ty.clone())),
                Some(_) => bail!("assignment to constant `{}`", name),
                None => bail!("assignment to unknown name `{}`", name),
            },
        }
    }

    fn place_static(&mut self, m: &'a StaticMemberExpression<'a>, pre: &mut Vec<Stmt>) -> R<Place> {
        // `Gizmos.color = ...`: the setter of a namespace property.
        if let Expression::Identifier(id) = &m.object
            && self.lookup(id.name.as_str()).is_none()
            && let Some(p) = self
                .manifest
                .ns_props
                .get(id.name.as_str())
                .and_then(|p| p.get(m.property.name.as_str()))
                .cloned()
        {
            let Some(setter) = p.setter else {
                bail!("{}.{} is read only", id.name, m.property.name)
            };
            let ty = self.ext_ir(&setter.params[0].1);
            return Ok(Place::NsProp(p.getter, setter, ty));
        }
        // `Class.count = ...`: a static field.
        if let Expression::Identifier(id) = &m.object
            && self.lookup(id.name.as_str()).is_none()
            && self.classes.contains_key(id.name.as_str())
        {
            let key = format!("{}.{}", id.name, m.property.name);
            if self.classes[id.name.as_str()]
                .statics
                .contains_key(&format!("set {}", m.property.name))
            {
                bail!("assignment to the static accessor `{key}` is not supported yet");
            }
            return match self.consts.get(&key) {
                Some(ConstInfo::Global(g, ty, true)) => Ok(Place::Global(*g, ty.clone())),
                Some(_) => bail!("assignment to the readonly static `{key}`"),
                None => bail!("no static value `{key}`"),
            };
        }
        let obj = self.lower_expr(&m.object, None)?;
        if m.property.name == "length" && matches!(obj.ty.strip(), Ty::Arr(_)) {
            let tmp = self.new_local(obj.ty.clone());
            pre.push(Stmt::Assign(tmp, obj));
            return Ok(Place::ArrLen(self.var_expr(tmp)));
        }
        self.field_place(obj, m.property.name.as_str(), pre)
    }

    fn place_private(
        &mut self,
        p: &'a PrivateFieldExpression<'a>,
        pre: &mut Vec<Stmt>,
    ) -> R<Place> {
        let obj = self.lower_expr(&p.object, None)?;
        self.field_place(obj, &classes::private_name(p.field.name.as_str()), pre)
    }

    fn place_computed(
        &mut self,
        m: &'a ComputedMemberExpression<'a>,
        pre: &mut Vec<Stmt>,
    ) -> R<Place> {
        let arr = self.lower_expr(&m.object, None)?;
        // `pair[0] = x` on a tuple (an object with a field per element).
        if let Ty::Obj(sid) = arr.ty.strip().clone()
            && self.is_tuple(sid)
        {
            let Expression::NumericLiteral(n) = &m.expression else {
                bail!("a tuple is indexed by a number written in the source")
            };
            let i = n.value as usize;
            if n.value < 0.0 || i >= self.m.structs[sid].fields.len() {
                bail!("tuple index {} is out of range", n.value)
            }
            let ty = self.m.structs[sid].fields[i].ty.clone();
            let tmp = self.new_local(arr.ty.clone());
            pre.push(Stmt::Assign(tmp, arr));
            return Ok(Place::Field(self.var_expr(tmp), sid, i, ty));
        }
        // `dict[key] = v` on a `Record<string, V>`.
        if let Ty::Map(k, v) = arr.ty.strip().clone()
            && *k == Ty::Str
        {
            let key = self.lower_expr(&m.expression, Some(&Ty::Str))?;
            let key = self.widen_lit(key);
            let (tm, tk) = (self.new_local(arr.ty.clone()), self.new_local(Ty::Str));
            pre.push(Stmt::Assign(tm, arr));
            pre.push(Stmt::Assign(tk, key));
            return Ok(Place::MapEntry(self.var_expr(tm), self.var_expr(tk), *v));
        }
        // `state[key] = v` with `key: keyof typeof state`: the field the key names.
        if let Ty::Obj(sid) = arr.ty.strip().clone()
            && !self.m.structs[sid].fields.is_empty()
        {
            let key = self.lower_expr(&m.expression, None)?;
            if matches!(key.ty, Ty::Lit | Ty::Str) {
                let idxs = self.key_indices(sid, &m.expression)?;
                let ty = self.m.structs[sid].fields[idxs[0]].ty.clone();
                // `o[k] = v` for fields of different types: `v` would need to be all of them at
                // once (TypeScript types the target as their intersection).
                if idxs.iter().any(|i| self.m.structs[sid].fields[*i].ty != ty) {
                    bail!(
                        "{}: assigning through a key that names fields of different types (narrow the key first)",
                        self.loc(m.span.start)
                    )
                }
                let (to, tk) = (
                    self.new_local(arr.ty.clone()),
                    self.new_local(key.ty.clone()),
                );
                pre.push(Stmt::Assign(to, arr));
                pre.push(Stmt::Assign(tk, key));
                return Ok(Place::Keyed(
                    self.var_expr(to),
                    sid,
                    self.var_expr(tk),
                    ty,
                    idxs,
                ));
            }
            bail!("cannot index an object by {}", key.ty)
        }
        let idx = self.lower_expr(&m.expression, Some(&Ty::Num))?;
        let idx = self.as_num(idx)?;
        let ty = match arr.ty.strip() {
            Ty::Typed(_) => Ty::Num,
            Ty::Arr(e) => (**e).clone(),
            t => bail!("cannot index {t}"),
        };
        let (ta, ti) = (self.new_local(arr.ty.clone()), self.new_local(Ty::Num));
        pre.push(Stmt::Assign(ta, arr));
        pre.push(Stmt::Assign(ti, idx));
        Ok(Place::Index(self.var_expr(ta), self.var_expr(ti), ty))
    }

    /// `obj.prop` as an assignment target: a field, or a property with a setter.
    fn field_place(&mut self, obj: Expr, prop: &str, pre: &mut Vec<Stmt>) -> R<Place> {
        if let Ty::Obj(sid) = obj.ty.strip().clone()
            && self.is_dyn(sid)
        {
            let tmp = self.new_local(obj.ty.clone());
            pre.push(Stmt::Assign(tmp, obj));
            let recv = self.var_expr(tmp);
            return self.iface_place(recv, sid, prop);
        }
        // `dict.name = v` on a `Record<string, V>`.
        if let Ty::Map(k, v) = obj.ty.strip().clone()
            && *k == Ty::Str
        {
            let tmp = self.new_local(obj.ty.clone());
            pre.push(Stmt::Assign(tmp, obj));
            let key = self.str_lit(prop);
            return Ok(Place::MapEntry(self.var_expr(tmp), key, *v));
        }
        if let Ty::Handle(class) = obj.ty.strip().clone() {
            if let Some((fid, ty)) = self.ext_setter(&class, prop)? {
                let getter = self.ext_getter(&class, prop, &obj)?;
                let tmp = self.new_local(obj.ty.clone());
                pre.push(Stmt::Assign(tmp, obj));
                // A facade getter, if any, is called when the old value is read (`+=`).
                let _ = getter;
                return Ok(Place::Accessor(
                    self.var_expr(tmp),
                    None,
                    classes::Target::Direct(fid),
                    ty,
                ));
            }
            let k = self.manifest.classes.get(&class);
            let getter = k.and_then(|k| k.getters.get(prop)).cloned();
            let setter = k.and_then(|k| k.setters.get(prop)).cloned();
            let setter = setter.or_else(|| self.set_method(&class, prop));
            // `playback.onEnded = f`: the method that takes the closure (`onEnded(f)`).
            let setter = setter.or_else(|| {
                k.and_then(|k| k.methods.get(prop))
                    .filter(|f| f.params.len() == 2 && matches!(f.params[1].1, ExtTy::Fn(..)))
                    .cloned()
            });
            let Some(setter) = setter else {
                bail!("{class}.{prop} cannot be assigned")
            };
            let ty = self.ext_ir(&setter.params[1].1);
            let tmp = self.new_local(obj.ty.clone());
            pre.push(Stmt::Assign(tmp, obj));
            return Ok(Place::Ext(self.var_expr(tmp), getter, setter, ty));
        }
        if let Ty::Obj(sid) = obj.ty.strip().clone()
            && !self.has_field(sid, prop)
        {
            if let Some((_, setter)) = self.target_for(sid, Which::Setter, prop) {
                let getter = self.target_for(sid, Which::Getter, prop).map(|g| g.1);
                let ty = self.setter_value_ty(&setter);
                let tmp = self.new_local(obj.ty.clone());
                pre.push(Stmt::Assign(tmp, obj));
                return Ok(Place::Accessor(self.var_expr(tmp), getter, setter, ty));
            }
            // A field or setter only a subclass has: tsc narrowed the object, so use it as that class.
            let sub = self.downcast_for(sid, |s, c| {
                s.has_field(c, prop) || s.resolve_in(c, Which::Setter, prop).is_some()
            });
            if let Some(sub) = sub {
                return self.field_place(Self::downcast(obj, sub), prop, pre);
            }
        }
        // A plain object whose literal gave this property a setter closure.
        if let Ty::Obj(sid) = obj.ty.strip().clone()
            && let Some(si) = self.m.structs[sid].field(&accessors::setter_name(prop))
            && let Some(i) = self.m.structs[sid].field(prop)
        {
            let ty = self.m.structs[sid].fields[i].ty.clone();
            let tmp = self.new_local(obj.ty.clone());
            pre.push(Stmt::Assign(tmp, obj));
            return Ok(Place::Thunked(self.var_expr(tmp), sid, i, si, ty));
        }
        let f = self.member_of(obj, prop)?;
        let ExprKind::Field(obj, sid, idx) = f.kind else {
            bail!("`{prop}` is not a field")
        };
        let tmp = self.new_local(obj.ty.clone());
        pre.push(Stmt::Assign(tmp, *obj));
        Ok(Place::Field(self.var_expr(tmp), sid, idx, f.ty))
    }

    pub(crate) fn place_ty(p: &Place) -> &Ty {
        static NUM: Ty = Ty::Num;
        match p {
            Place::ArrLen(_) => &NUM,
            Place::Accessor(_, _, _, t) | Place::Ext(_, _, _, t) | Place::NsProp(_, _, t) => t,
            Place::Var(_, t)
            | Place::Cell(_, t)
            | Place::Global(_, t)
            | Place::Field(_, _, _, t)
            | Place::Thunked(_, _, _, _, t)
            | Place::Keyed(_, _, _, t, _)
            | Place::MapEntry(_, _, t)
            | Place::Index(_, _, t) => t,
        }
    }

    pub(crate) fn place_read(&mut self, p: &Place) -> Expr {
        match p {
            Place::Var(v, _) => self.var_expr(*v),
            Place::Cell(c, t) => mk(ExprKind::CellGet(Box::new(c.clone())), t.clone()),
            Place::Global(g, t) => mk(ExprKind::GlobalVar(*g), t.clone()),
            Place::ArrLen(a) => mk(ExprKind::Len(Box::new(a.clone())), Ty::Num),
            Place::Accessor(o, g, _, t) => match g {
                Some(g) => Self::target_call(g, vec![o.clone()], t.clone()),
                None => mk(ExprKind::Null, t.clone()),
            },
            Place::Ext(o, g, _, t) => match g {
                Some(g) => self.emit_extern(g, vec![o.clone()]),
                None => mk(ExprKind::Null, t.clone()),
            },
            Place::NsProp(g, _, t) => match g {
                Some(g) => self.emit_extern(g, vec![]),
                None => mk(ExprKind::Null, t.clone()),
            },
            Place::Field(o, s, i, t) => mk(ExprKind::Field(Box::new(o.clone()), *s, *i), t.clone()),
            Place::Thunked(o, s, i, _, _) => {
                let name = self.m.structs[*s].fields[*i].name.clone();
                self.thunked_read(o.clone(), *s, *i, &name)
            }
            Place::Index(a, i, t) => mk(
                ExprKind::Index(Box::new(a.clone()), Box::new(i.clone())),
                t.clone(),
            ),
            // The fields of a keyed place have one type (checked where it is made), so the read
            // converts nothing and cannot fail.
            Place::Keyed(o, sid, k, _, idxs) => self
                .record_index_over(o.clone(), *sid, k.clone(), idxs)
                .expect("a keyed place's fields have one type"),
            Place::MapEntry(m, k, v) => {
                let got = self.map_get_value(m.clone(), k.clone(), "a Record<string, V> lookup");
                match got {
                    Ok(e) => e,
                    // A value type that cannot be told from a missing key: reading is not possible.
                    Err(_) => mk(ExprKind::Null, v.clone().nullable()),
                }
            }
        }
    }

    /// `a[i] = v` on an array: inside the array it stores, at the length it grows the array by
    /// one (as JavaScript does), anywhere else (past the end, negative, NaN) it throws a
    /// `RangeError`: JavaScript would make holes or a property, which arrays here cannot hold.
    fn array_store(&mut self, a: &Expr, i: &Expr, v: Expr) -> Stmt {
        let Some(&fid) = self.fids.get("__rtThrow") else {
            // No error classes in this program: the unchecked store (out of range is dropped).
            return Stmt::StoreIndex(a.clone(), i.clone(), v);
        };
        let tmp = self.new_local(v.ty.clone());
        let tv = self.var_expr(tmp);
        let len = mk(ExprKind::Len(Box::new(a.clone())), Ty::Num);
        let cmp = |op, l: Expr, r: Expr| mk(ExprKind::Cmp(op, Box::new(l), Box::new(r)), Ty::Bool);
        let grow = Stmt::Eval(mk(
            ExprKind::Rt(RtFn::ArrPush, vec![a.clone(), tv.clone()]),
            Ty::Void,
        ));
        let inside = mk(
            ExprKind::And(
                Box::new(cmp(CmpOp::Ge, i.clone(), num(0.0))),
                Box::new(cmp(CmpOp::Lt, i.clone(), len.clone())),
            ),
            Ty::Bool,
        );
        let head = self.str_lit("array index ");
        let at = self
            .stringify(i.clone())
            .unwrap_or_else(|_| self.str_lit("?"));
        let tail = self.str_lit(
            " is beyond the end of the array: only the next index (the length) can be written",
        );
        let msg = self
            .str_concat(head, at)
            .and_then(|m| self.str_concat(m, tail))
            .unwrap_or_else(|_| self.str_lit("array index out of range"));
        let throw = Stmt::Eval(mk(ExprKind::Call(fid, vec![num(1.0), msg]), Ty::Void));
        Stmt::Block(
            self.new_label(),
            vec![
                Stmt::Assign(tmp, v),
                Stmt::If(
                    cmp(CmpOp::Eq, i.clone(), len),
                    vec![grow],
                    vec![Stmt::If(
                        inside,
                        vec![Stmt::StoreIndex(a.clone(), i.clone(), tv)],
                        vec![throw],
                    )],
                ),
            ],
        )
    }

    pub(crate) fn place_write(&mut self, p: &Place, v: Expr) -> Stmt {
        match p {
            Place::Var(var, _) => Stmt::Assign(*var, v),
            Place::Cell(cell, _) => Stmt::CellSet(cell.clone(), v),
            Place::Global(g, _) => Stmt::StoreGlobal(*g, v),
            Place::Accessor(o, _, setter, _) => {
                Stmt::Eval(Self::target_call(setter, vec![o.clone(), v], Ty::Void))
            }
            Place::Ext(o, _, setter, _) => Stmt::Eval(self.emit_extern(setter, vec![o.clone(), v])),
            Place::NsProp(_, setter, _) => Stmt::Eval(self.emit_extern(setter, vec![v])),
            Place::ArrLen(a) => Stmt::Eval(mk(
                ExprKind::Rt(RtFn::ArrSetLen, vec![a.clone(), v]),
                Ty::Void,
            )),
            Place::Field(o, s, i, _) => Stmt::StoreField(o.clone(), *s, *i, v),
            Place::Thunked(o, s, i, si, ty) => {
                // The setter closure when the object has one, the plain field otherwise.
                let tmp = self.new_local(ty.clone());
                let tv = self.var_expr(tmp);
                let sty = self.m.structs[*s].fields[*si].ty.clone();
                let thunk = mk(ExprKind::Field(Box::new(o.clone()), *s, *si), sty.clone());
                let none = mk(ExprKind::IsNull(Box::new(thunk.clone())), Ty::Bool);
                let callee = mk(ExprKind::Cast(Box::new(thunk)), sty.strip().clone());
                let call = mk(
                    ExprKind::CallClosure(Box::new(callee), vec![tv.clone()]),
                    Ty::Void,
                );
                let label = self.new_label();
                Stmt::Block(
                    label,
                    vec![
                        Stmt::Assign(tmp, v),
                        Stmt::If(
                            none,
                            vec![Stmt::StoreField(o.clone(), *s, *i, tv)],
                            vec![Stmt::Eval(call)],
                        ),
                    ],
                )
            }
            Place::MapEntry(m, k, _) => {
                // The caller coerced the value to the entry type, so this cannot fail.
                let set = self
                    .map_set_value(m.clone(), k.clone(), Some(v))
                    .expect("a Record<string, V> store of a coerced value");
                Stmt::Eval(set)
            }
            Place::Keyed(o, sid, k, ty, idxs) => {
                // The value once, then the store of the field the key names.
                let tmp = self.new_local(ty.clone());
                let tv = self.var_expr(tmp);
                let by_str = k.ty.strip() == &Ty::Str;
                let last = *idxs.last().expect("one field at least");
                let mut chain = vec![Stmt::StoreField(o.clone(), *sid, last, tv.clone())];
                for &i in idxs[..idxs.len() - 1].iter().rev() {
                    let name = self.m.structs[*sid].fields[i].name.clone();
                    let want = if by_str {
                        self.str_lit(&name)
                    } else {
                        let t = self.tag(&name);
                        mk(ExprKind::Tag(t), Ty::Lit)
                    };
                    let is = mk(
                        ExprKind::Cmp(CmpOp::Eq, Box::new(k.clone()), Box::new(want)),
                        Ty::Bool,
                    );
                    chain = vec![Stmt::If(
                        is,
                        vec![Stmt::StoreField(o.clone(), *sid, i, tv.clone())],
                        chain,
                    )];
                }
                let label = self.new_label();
                let mut body = vec![Stmt::Assign(tmp, v)];
                body.extend(chain);
                Stmt::Block(label, body)
            }
            Place::Index(a, i, _) if matches!(a.ty.strip(), Ty::Arr(_)) => {
                self.array_store(a, i, v)
            }
            Place::Index(a, i, _) => Stmt::StoreIndex(a.clone(), i.clone(), v),
        }
    }

    fn assign_target(&mut self, t: &'a AssignmentTarget<'a>, pre: &mut Vec<Stmt>) -> R<Place> {
        match t.as_simple_assignment_target() {
            Some(s) => self.place_of(s, pre),
            None => bail!("destructuring assignment is not supported"),
        }
    }

    /// `[a, b] = [b, a]`, `[xs[i], xs[j]] = [xs[j], xs[i]]`, `({ x, y } = o)`, `[n, s] = pair()`:
    /// the right side first, then each target in order. An array literal on the right with an
    /// element per target is not built: its elements are evaluated into temporaries first (so a
    /// swap reads both before writing either).
    fn lower_destructure_assign(
        &mut self,
        a: &'a AssignmentExpression<'a>,
    ) -> R<(Vec<Stmt>, Expr)> {
        let mut out = Vec::new();
        if let (AssignmentTarget::ArrayAssignmentTarget(t), Expression::ArrayExpression(lit)) =
            (&a.left, &a.right)
            && t.rest.is_none()
            && lit.elements.len() >= t.elements.len()
            && lit.elements.iter().all(|e| e.as_expression().is_some())
        {
            let mut temps = Vec::new();
            for e in &lit.elements {
                let e = e.as_expression().expect("checked");
                let v = self.lower_expr(e, None)?;
                let v = self.widen_lit(v);
                let tmp = self.new_local(v.ty.clone());
                out.push(Stmt::Assign(tmp, v));
                temps.push(self.var_expr(tmp));
            }
            for (el, v) in t.elements.iter().zip(temps) {
                if let Some(el) = el {
                    self.assign_maybe_default(el, v, &mut out)?;
                }
            }
            return Ok((out, mk(ExprKind::Null, Ty::Null)));
        }
        let v = self.lower_expr(&a.right, None)?;
        let v = self.widen_lit(v);
        let tmp = self.new_local(v.ty.clone());
        out.push(Stmt::Assign(tmp, v));
        let value = self.var_expr(tmp);
        self.assign_pattern(&a.left, value.clone(), &mut out)?;
        Ok((out, value))
    }

    /// `value` with `init` where it is null.
    fn with_default(
        &mut self,
        value: Expr,
        init: &'a Expression<'a>,
        out: &mut Vec<Stmt>,
    ) -> R<Expr> {
        if !value.ty.is_nullable() {
            return Ok(value);
        }
        let base = value.ty.strip().clone();
        let tmp = self.new_local(value.ty.clone());
        out.push(Stmt::Assign(tmp, value));
        let t = self.var_expr(tmp);
        let d = self.lower_expr(init, Some(&base))?;
        let d = self.coerce(d, &base)?;
        let have = mk(ExprKind::Cast(Box::new(t.clone())), base.clone());
        Ok(mk(
            ExprKind::Cond(
                Box::new(mk(ExprKind::IsNull(Box::new(t)), Ty::Bool)),
                Box::new(d),
                Box::new(have),
            ),
            base,
        ))
    }

    fn assign_maybe_default(
        &mut self,
        el: &'a AssignmentTargetMaybeDefault<'a>,
        value: Expr,
        out: &mut Vec<Stmt>,
    ) -> R<()> {
        match el {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(w) => {
                let v = self.with_default(value, &w.init, out)?;
                self.assign_pattern(&w.binding, v, out)
            }
            other => {
                let Some(t) = other.as_assignment_target() else {
                    bail!("unsupported destructuring assignment target")
                };
                self.assign_pattern(t, value, out)
            }
        }
    }

    /// Writes the parts of `value` to the targets of a destructuring assignment.
    fn assign_pattern(
        &mut self,
        t: &'a AssignmentTarget<'a>,
        value: Expr,
        out: &mut Vec<Stmt>,
    ) -> R<()> {
        match t {
            AssignmentTarget::ArrayAssignmentTarget(a) => {
                if let Ty::Obj(sid) = value.ty.strip().clone()
                    && self.is_tuple(sid)
                {
                    let n = self.m.structs[sid].fields.len();
                    if a.elements.len() > n {
                        bail!("a tuple pattern names at most its {n} elements")
                    }
                    let tmp = self.new_local(value.ty.clone());
                    out.push(Stmt::Assign(tmp, value));
                    for (i, el) in a.elements.iter().enumerate() {
                        let Some(el) = el else { continue };
                        let ty = self.m.structs[sid].fields[i].ty.clone();
                        let part = mk(ExprKind::Field(Box::new(self.var_expr(tmp)), sid, i), ty);
                        self.assign_maybe_default(el, part, out)?;
                    }
                    if let Some(r) = &a.rest {
                        let vals: Vec<Expr> = (a.elements.len()..n)
                            .map(|i| {
                                let ty = self.m.structs[sid].fields[i].ty.clone();
                                mk(ExprKind::Field(Box::new(self.var_expr(tmp)), sid, i), ty)
                            })
                            .collect();
                        let rest = self.tuple_or_array(vals)?;
                        self.assign_pattern(&r.target, rest, out)?;
                    }
                    return Ok(());
                }
                let value = self.match_view(value);
                let Ty::Arr(el) = value.ty.strip().clone() else {
                    bail!("cannot destructure {} as an array", value.ty)
                };
                let el = *el;
                let tmp = self.new_local(value.ty.clone());
                out.push(Stmt::Assign(tmp, value));
                for (i, elt) in a.elements.iter().enumerate() {
                    let Some(elt) = elt else { continue };
                    // Past the end an element reads as null (a zero pointer, or the null NaN).
                    let read_ty = if el.is_ptr() || el == Ty::Num {
                        el.clone().nullable()
                    } else {
                        el.clone()
                    };
                    let read = mk(
                        ExprKind::Index(Box::new(self.var_expr(tmp)), Box::new(num(i as f64))),
                        read_ty.clone(),
                    );
                    let part = if matches!(
                        elt,
                        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(_)
                    ) || read_ty == el
                    {
                        read
                    } else {
                        mk(ExprKind::Cast(Box::new(read)), el.clone())
                    };
                    self.assign_maybe_default(elt, part, out)?;
                }
                if let Some(r) = &a.rest {
                    let ty = self.var_expr(tmp).ty;
                    let rest = mk(
                        ExprKind::Rt(
                            RtFn::ArrSlice,
                            vec![
                                self.var_expr(tmp),
                                num(a.elements.len() as f64),
                                num(f64::INFINITY),
                            ],
                        ),
                        ty,
                    );
                    self.assign_pattern(&r.target, rest, out)?;
                }
                Ok(())
            }
            AssignmentTarget::ObjectAssignmentTarget(o) => {
                let tmp = self.new_local(value.ty.clone());
                out.push(Stmt::Assign(tmp, value));
                let mut taken = Vec::new();
                for p in &o.properties {
                    match p {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                            let key = p.binding.name.to_string();
                            let part = self.member_of(self.var_expr(tmp), &key)?;
                            let part = match &p.init {
                                Some(init) => self.with_default(part, init, out)?,
                                None => part,
                            };
                            let place = self.place_ident(&key)?;
                            let pty = Self::place_ty(&place).clone();
                            let v = self.coerce(part, &pty)?;
                            out.push(self.place_write(&place, v));
                            taken.push(key);
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                            if p.computed {
                                bail!("computed key in an object pattern")
                            }
                            let key = self.key_name(&p.name)?;
                            let part = self.member_of(self.var_expr(tmp), &key)?;
                            taken.push(key);
                            self.assign_maybe_default(&p.binding, part, out)?;
                        }
                    }
                }
                if let Some(r) = &o.rest {
                    let rest = self.object_rest(&self.var_expr(tmp), &taken)?;
                    self.assign_pattern(&r.target, rest, out)?;
                }
                Ok(())
            }
            other => {
                let Some(s) = other.as_simple_assignment_target() else {
                    bail!("unsupported destructuring assignment target")
                };
                let mut pre = Vec::new();
                let place = self.place_of(s, &mut pre)?;
                let pty = Self::place_ty(&place).clone();
                let v = self.coerce_ext(value, &pty)?;
                out.extend(pre);
                out.push(self.place_write(&place, v));
                Ok(())
            }
        }
    }

    /// Statements for an assignment, and an expression for its value.
    pub(crate) fn lower_assign(&mut self, a: &'a AssignmentExpression<'a>) -> R<(Vec<Stmt>, Expr)> {
        use AssignmentOperator as A;
        if a.operator == A::Assign
            && matches!(
                a.left,
                AssignmentTarget::ArrayAssignmentTarget(_)
                    | AssignmentTarget::ObjectAssignmentTarget(_)
            )
        {
            return self.lower_destructure_assign(a);
        }
        let mut pre = Vec::new();
        let mut place = self.assign_target(&a.left, &mut pre)?;
        // `ps.startLifetime = [1, 2]`: a setter declared twice (a number, a list) has a version
        // for the list.
        if a.operator == A::Assign
            && matches!(&a.right, Expression::ArrayExpression(_))
            && let Place::Ext(o, g, setter, _) = &place
            && let Some(list) = self.list_setter_of(setter)
        {
            let ty = self.ext_ir(&list.params[1].1);
            place = Place::Ext(o.clone(), g.clone(), list, ty);
        }
        let pty = Self::place_ty(&place).clone();
        // `RenderSettings.heightFog = { .. }`: an object literal for an `Option<Record>` property
        // keeps the keys it writes (see `lower_record_literal`).
        if a.operator == A::Assign
            && let Place::NsProp(_, setter, _) = &place
            && let ExtTy::Opt(inner) = &setter.params[0].1
            && let ExtTy::Record(rn) = &**inner
            && let Some(text) = self.lower_record_literal(&a.right, rn)?
        {
            let setter = setter.clone();
            pre.push(Stmt::Eval(self.emit_extern(&setter, vec![text])));
            return Ok((pre, mk(ExprKind::Null, Ty::Null)));
        }
        if matches!(a.operator, A::LogicalAnd | A::LogicalOr | A::LogicalNullish) {
            // `p ||= v` is `if (!p) p = v`: the target is evaluated once and v only when needed.
            let old = self.place_read(&place);
            let cond = match a.operator {
                A::LogicalAnd => self.bool_of(old),
                A::LogicalOr => {
                    let t = self.bool_of(old);
                    mk(ExprKind::Un(UnOp::Not, Box::new(t)), Ty::Bool)
                }
                _ if old.ty.is_nullable() => mk(ExprKind::IsNull(Box::new(old)), Ty::Bool),
                _ => mk(ExprKind::Bool(false), Ty::Bool),
            };
            let v = self.lower_expr(&a.right, Some(&pty))?;
            let v = self.coerce(v, &pty)?;
            pre.push(Stmt::If(cond, vec![self.place_write(&place, v)], vec![]));
            let value = self.place_read(&place);
            return Ok((pre, value));
        }
        let is_accessor = matches!(
            place,
            Place::Accessor(..) | Place::Ext(..) | Place::NsProp(..) | Place::Thunked(..)
        );
        let new = if a.operator == A::Assign {
            let v = self.lower_expr(&a.right, Some(&pty))?;
            self.coerce_ext(v, &pty)?
        } else if a.operator == A::Addition && Self::is_stringy(&pty) {
            let r = self.lower_expr(&a.right, None)?;
            let old = self.place_read(&place);
            let v = self.str_concat(old, r)?;
            self.coerce(v, &pty)?
        } else {
            let op = match a.operator {
                A::Addition => BinOp::Add,
                A::Subtraction => BinOp::Sub,
                A::Multiplication => BinOp::Mul,
                A::Division => BinOp::Div,
                A::Remainder => BinOp::Rem,
                A::BitwiseAnd => BinOp::BitAnd,
                A::BitwiseOR => BinOp::BitOr,
                A::BitwiseXOR => BinOp::BitXor,
                A::ShiftLeft => BinOp::Shl,
                A::ShiftRight => BinOp::Shr,
                A::ShiftRightZeroFill => BinOp::UShr,
                A::Exponential => BinOp::Pow,
                _ => bail!(
                    "{}: unsupported compound assignment",
                    self.loc(a.span.start)
                ),
            };
            let r = self.lower_expr(&a.right, Some(&Ty::Num))?;
            let old = self.place_read(&place);
            let v = self.arith(op, old, r)?;
            self.coerce(v, &pty)?
        };
        if is_accessor {
            // The value of `o.x = v` is `v`, whatever the getter says (there may be none).
            let t = self.new_local(pty.clone());
            pre.push(Stmt::Assign(t, new));
            let tv = self.var_expr(t);
            pre.push(self.place_write(&place, tv.clone()));
            return Ok((pre, tv));
        }
        pre.push(self.place_write(&place, new));
        let value = self.place_read(&place);
        Ok((pre, value))
    }

    pub(crate) fn lower_update(&mut self, u: &'a UpdateExpression<'a>) -> R<(Vec<Stmt>, Expr)> {
        let mut pre = Vec::new();
        let place = self.place_of(&u.argument, &mut pre)?;
        let pty = Self::place_ty(&place).clone();
        let old = self.place_read(&place);
        let delta = if u.operator == UpdateOperator::Increment {
            1.0
        } else {
            -1.0
        };
        let new = if pty.strip() == &Ty::BigInt {
            self.bigint_step(old.clone(), delta > 0.0)?
        } else {
            self.arith(BinOp::Add, old.clone(), num(delta))?
        };
        let new = self.coerce(new, &pty)?;
        let value = if u.prefix {
            pre.push(self.place_write(&place, new));
            self.place_read(&place)
        } else {
            let tmp = self.new_local(if pty.strip() == &Ty::BigInt {
                Ty::BigInt
            } else {
                Ty::Num
            });
            pre.push(Stmt::Assign(tmp, old));
            pre.push(self.place_write(&place, new));
            self.var_expr(tmp)
        };
        Ok((pre, value))
    }

    // ---- calls ----

    pub(crate) fn lower_args(
        &mut self,
        args: &'a [Argument<'a>],
        params: &[Ty],
        what: &str,
    ) -> R<Vec<Expr>> {
        // `f(a, ...xs)` for a function value: element `j` of the array (or tuple) feeds the
        // parameter after the plain arguments; past its end a parameter reads as null.
        if let Some(k) = args
            .iter()
            .position(|a| matches!(a, Argument::SpreadElement(_)))
            && k + 1 == args.len()
            && k <= params.len()
        {
            let mut out = Vec::new();
            for (a, p) in args[..k].iter().zip(params) {
                let Some(e) = a.as_expression() else {
                    bail!("{what}: spread argument")
                };
                let v = self.lower_expr(e, Some(p))?;
                out.push(self.coerce(v, p)?);
            }
            let Argument::SpreadElement(sp) = &args[k] else {
                unreachable!("checked")
            };
            let arr = self.lower_expr(&sp.argument, None)?;
            let n = params.len() - k;
            let parts: Vec<Expr> = match arr.ty.strip().clone() {
                Ty::Obj(sid) if self.is_tuple(sid) => {
                    let mut parts = self.tuple_elements(arr, sid);
                    parts.truncate(n);
                    parts
                }
                Ty::Arr(el) => {
                    let av = self.new_local(arr.ty.clone());
                    let raw_ty = if el.is_ptr() || *el == Ty::Num {
                        el.clone().nullable()
                    } else {
                        (*el).clone()
                    };
                    (0..n)
                        .map(|j| {
                            let read = mk(
                                ExprKind::Index(
                                    Box::new(self.var_expr(av)),
                                    Box::new(num(j as f64)),
                                ),
                                raw_ty.clone(),
                            );
                            if j == 0 {
                                mk(
                                    ExprKind::Seq(
                                        vec![Stmt::Assign(av, arr.clone())],
                                        Box::new(read),
                                    ),
                                    raw_ty.clone(),
                                )
                            } else {
                                read
                            }
                        })
                        .collect()
                }
                t => bail!("{what}: cannot spread {t} into arguments"),
            };
            for (j, part) in parts.into_iter().enumerate() {
                out.push(self.coerce(part, &params[k + j])?);
            }
            for p in &params[k + out.len() - k..] {
                out.push(mk(ExprKind::Null, p.clone()));
            }
            return Ok(out);
        }
        // A parameter that may be null (`x?: T`) can be left out.
        let optional_tail =
            params.len() > args.len() && params[args.len()..].iter().all(|p| p.is_nullable());
        if args.len() > params.len() || (args.len() < params.len() && !optional_tail) {
            bail!(
                "{what}: {} arguments, {} expected",
                args.len(),
                params.len()
            );
        }
        let mut out = Vec::new();
        for (a, p) in args.iter().zip(params) {
            let Some(e) = a.as_expression() else {
                bail!("{what}: spread argument")
            };
            let v = self.lower_expr(e, Some(p))?;
            out.push(self.coerce(v, p)?);
        }
        for p in &params[args.len()..] {
            out.push(mk(ExprKind::Null, p.clone()));
        }
        Ok(out)
    }

    /// Arguments for a call of a declared function. A missing argument takes the parameter's
    /// default (lowered here, in a scope that has only the earlier parameters) or null for `x?: T`;
    /// the arguments after the fixed ones become the rest parameter's array.
    ///
    /// Defaults are filled in by the caller, which is why such a function must be called directly:
    /// as a value (`xs.map(f)`) every parameter has to be passed.
    pub(crate) fn lower_fn_args(
        &mut self,
        name: &str,
        args: &'a [Argument<'a>],
        ptys: &[Ty],
    ) -> R<(Vec<Stmt>, Vec<Expr>)> {
        let fp: &'a FormalParameters<'a> = self.fsrc[name].params;
        let n_fixed = fp.items.len();
        let rest_ty = fp.rest.as_ref().map(|_| ptys[n_fixed].clone());
        // `f(a, ...xs)` where the spread feeds fixed parameters: elements are read by index.
        if let Some(k) = args
            .iter()
            .position(|a| matches!(a, Argument::SpreadElement(_)))
            && k < n_fixed
        {
            return self.lower_fn_args_spread(name, args, ptys, k);
        }
        let given = args.len().min(n_fixed);
        // JavaScript evaluates and drops the arguments a function does not name.
        let mut dropped = Vec::new();
        if rest_ty.is_none() && args.len() > n_fixed {
            // tsc rejects this in user code; the component hook thunks pass `dt` to every hook.
            if !matches!(self.fsrc[name].kind, FnKind::Method(_)) {
                bail!(
                    "{name}: {} arguments, at most {n_fixed} expected",
                    args.len()
                );
            }
            for a in &args[n_fixed..] {
                let Some(e) = a.as_expression() else {
                    bail!("{name}: a spread argument past the parameters")
                };
                let v = self.lower_expr(e, None)?;
                dropped.push(Stmt::Eval(v));
            }
        }
        let mut vals: Vec<Expr> = Vec::new();
        for (a, p) in args[..given].iter().zip(ptys) {
            let Some(e) = a.as_expression() else {
                bail!("{name}: a spread argument can only feed a rest parameter")
            };
            let v = self.lower_expr(e, Some(p))?;
            let v = self.temporal_arg(v, p)?;
            vals.push(self.coerce(v, p)?);
        }
        let mut pre = dropped;
        if given < n_fixed {
            // The defaults may use earlier parameters: give those names to temporaries.
            let mut names: HashMap<String, Binding> = HashMap::new();
            for i in 0..given {
                let tmp = self.new_local(ptys[i].clone());
                pre.push(Stmt::Assign(tmp, vals[i].clone()));
                vals[i] = self.var_expr(tmp);
                if let BindingPattern::BindingIdentifier(b) = &fp.items[i].pattern {
                    names.insert(b.name.to_string(), Binding::Direct(tmp));
                }
            }
            for (p, pty) in fp.items.iter().zip(ptys).skip(given) {
                let v = if let Some(init) = &p.initializer {
                    let saved = std::mem::replace(&mut self.cur.scopes, vec![names.clone()]);
                    let r = self.lower_expr(init, Some(pty));
                    self.cur.scopes = saved;
                    self.coerce(r?, pty)?
                } else if p.optional {
                    self.coerce(mk(ExprKind::Null, Ty::Null), pty)?
                } else if let Ty::Obj(sid) = pty.strip()
                    && !self.class_of_sid.contains_key(sid)
                    && self.m.structs[*sid]
                        .fields
                        .iter()
                        .all(|f| f.ty.is_nullable())
                {
                    // `<Btn />` for `props: { onPress?: () => void }`: an object with nothing set.
                    let sid = *sid;
                    let tmp = self.new_local(pty.clone());
                    let t = self.var_expr(tmp);
                    let mut stmts = vec![Stmt::Assign(tmp, mk(ExprKind::NewObj(sid), pty.clone()))];
                    for i in 0..self.m.structs[sid].fields.len() {
                        let fty = self.m.structs[sid].fields[i].ty.clone();
                        stmts.push(Stmt::StoreField(t.clone(), sid, i, mk(ExprKind::Null, fty)));
                    }
                    mk(ExprKind::Seq(stmts, Box::new(t)), pty.clone())
                } else {
                    bail!("{name}: {} arguments, {n_fixed} expected", args.len())
                };
                let tmp = self.new_local(pty.clone());
                pre.push(Stmt::Assign(tmp, v));
                vals.push(self.var_expr(tmp));
                if let BindingPattern::BindingIdentifier(b) = &p.pattern {
                    names.insert(b.name.to_string(), Binding::Direct(tmp));
                }
            }
        }
        if let Some(rt) = rest_ty.clone()
            && let Ty::Obj(tsid) = rt.strip().clone()
            && self.is_tuple(tsid)
        {
            // `...args: [A, B?]`: the arguments from here on make the tuple (or one spread tuple
            // of the same type is it).
            let rest_args = &args[given..];
            if rest_args.len() == 1
                && let Argument::SpreadElement(sp) = &rest_args[0]
            {
                let v = self.lower_expr(&sp.argument, Some(&rt))?;
                vals.push(self.coerce(v, &rt)?);
                return Ok((pre, vals));
            }
            let n = self.m.structs[tsid].fields.len();
            if rest_args.len() > n {
                bail!(
                    "{name}: {} arguments, at most {} expected",
                    args.len(),
                    given + n
                )
            }
            let mut elems = Vec::new();
            for i in 0..n {
                let fty = self.m.structs[tsid].fields[i].ty.clone();
                match rest_args.get(i) {
                    Some(a) => {
                        let Some(e) = a.as_expression() else {
                            bail!("{name}: a spread argument inside a tuple rest parameter")
                        };
                        let v = self.lower_expr(e, Some(&fty))?;
                        elems.push(self.coerce(v, &fty)?);
                    }
                    None if fty.is_nullable() => elems.push(mk(ExprKind::Null, fty)),
                    None => bail!("{name}: {} arguments, {} expected", args.len(), given + n),
                }
            }
            vals.push(self.build_tuple(tsid, elems)?);
            return Ok((pre, vals));
        }
        if let Some(rt) = rest_ty {
            let Ty::Arr(el) = &rt else {
                bail!("{name}: a rest parameter must be an array")
            };
            let el = (**el).clone();
            let mut elems = Vec::new();
            for a in &args[given..] {
                if let Argument::SpreadElement(sp) = a {
                    let v = self.lower_expr(&sp.argument, Some(&rt))?;
                    elems.push(Elem::Spread(v));
                } else if let Some(e) = a.as_expression() {
                    let v = self.lower_expr(e, Some(&el))?;
                    elems.push(Elem::One(self.coerce(v, &el)?));
                }
            }
            vals.push(self.build_array(elems, el)?);
        }
        Ok((pre, vals))
    }

    /// A call whose last argument is a spread array that feeds the fixed parameters from `k` on
    /// (`add(...xyz)`, `f(1, ...rest)`): parameter `j` is element `j - k`, its default (or null)
    /// when the array is shorter; a rest parameter takes what is left.
    #[allow(clippy::needless_range_loop)]
    fn lower_fn_args_spread(
        &mut self,
        name: &str,
        args: &'a [Argument<'a>],
        ptys: &[Ty],
        k: usize,
    ) -> R<(Vec<Stmt>, Vec<Expr>)> {
        let fp: &'a FormalParameters<'a> = self.fsrc[name].params;
        let n_fixed = fp.items.len();
        if k + 1 != args.len() {
            bail!("{name}: a spread argument that feeds fixed parameters must be the last argument")
        }
        let mut pre = Vec::new();
        let mut vals: Vec<Expr> = Vec::new();
        for (a, p) in args[..k].iter().zip(ptys) {
            let Some(e) = a.as_expression() else {
                bail!("{name}: a second spread argument")
            };
            let v = self.lower_expr(e, Some(p))?;
            let v = self.temporal_arg(v, p)?;
            vals.push(self.coerce(v, p)?);
        }
        let Argument::SpreadElement(sp) = &args[k] else {
            unreachable!()
        };
        let arr = self.lower_expr(&sp.argument, None)?;
        // `f(...tuple)`: element `j` feeds parameter `k + j`.
        if let Ty::Obj(sid) = arr.ty.strip().clone()
            && self.is_tuple(sid)
        {
            if fp.rest.is_some() {
                bail!("{name}: a tuple cannot be spread into a rest parameter")
            }
            let tv = self.new_local(arr.ty.clone());
            pre.push(Stmt::Assign(tv, arr));
            let n = self.m.structs[sid].fields.len();
            for j in k..n_fixed {
                let pty = ptys[j].clone();
                let p = &fp.items[j];
                let v = if j - k < n {
                    let fty = self.m.structs[sid].fields[j - k].ty.clone();
                    let read = mk(
                        ExprKind::Field(Box::new(self.var_expr(tv)), sid, j - k),
                        fty,
                    );
                    self.coerce(read, &pty)?
                } else if let Some(init) = &p.initializer {
                    let saved = std::mem::replace(&mut self.cur.scopes, vec![HashMap::new()]);
                    let d = self.lower_expr(init, Some(&pty));
                    self.cur.scopes = saved;
                    self.coerce(d?, &pty)?
                } else if p.optional {
                    self.coerce(mk(ExprKind::Null, Ty::Null), &pty)?
                } else {
                    bail!(
                        "{name}: the tuple has {n} elements, {} parameters need values",
                        n_fixed - k
                    )
                };
                let tmp = self.new_local(pty);
                pre.push(Stmt::Assign(tmp, v));
                vals.push(self.var_expr(tmp));
            }
            return Ok((pre, vals));
        }
        let Ty::Arr(el) = arr.ty.strip().clone() else {
            bail!("{name}: cannot spread {} into parameters", arr.ty)
        };
        let arr_ty = arr.ty.strip().clone();
        let av = self.new_local(arr_ty.clone());
        pre.push(Stmt::Assign(av, arr));
        let raw_ty = if el.is_ptr() || *el == Ty::Num {
            el.nullable()
        } else {
            (*el).clone()
        };
        for j in k..n_fixed {
            let pty = ptys[j].clone();
            let at = num((j - k) as f64);
            let read = mk(
                ExprKind::Index(Box::new(self.var_expr(av)), Box::new(at.clone())),
                raw_ty.clone(),
            );
            let elem = self.coerce(read, &pty)?;
            let p = &fp.items[j];
            let v = if let Some(init) = &p.initializer {
                let saved = std::mem::replace(&mut self.cur.scopes, vec![HashMap::new()]);
                let d = self.lower_expr(init, Some(&pty));
                self.cur.scopes = saved;
                let d = self.coerce(d?, &pty)?;
                let len = mk(ExprKind::Len(Box::new(self.var_expr(av))), Ty::Num);
                let inside = mk(
                    ExprKind::Cmp(CmpOp::Lt, Box::new(at), Box::new(len)),
                    Ty::Bool,
                );
                mk(
                    ExprKind::Cond(Box::new(inside), Box::new(elem), Box::new(d)),
                    pty.clone(),
                )
            } else {
                elem
            };
            let tmp = self.new_local(pty);
            pre.push(Stmt::Assign(tmp, v));
            vals.push(self.var_expr(tmp));
        }
        if fp.rest.is_some() {
            let rt = ptys[n_fixed].clone();
            if rt != arr_ty {
                bail!("{name}: the spread array's type {arr_ty} is not the rest parameter's {rt}")
            }
            vals.push(mk(
                ExprKind::Rt(
                    RtFn::ArrSlice,
                    vec![
                        self.var_expr(av),
                        num((n_fixed - k) as f64),
                        num(f64::INFINITY),
                    ],
                ),
                rt,
            ));
        }
        Ok((pre, vals))
    }

    fn lower_call(&mut self, c: &'a CallExpression<'a>) -> R<Expr> {
        let call_exp = self.call_exp.take();
        match &c.callee {
            // `obj[Symbol.iterator]()`.
            Expression::ComputedMemberExpression(m)
                if iter::well_known(&m.expression).is_some() =>
            {
                let name = iter::well_known(&m.expression).expect("checked");
                let recv = self.lower_expr(&m.object, None)?;
                self.call_method0(recv, name)
            }
            Expression::Identifier(id) => {
                let mut name = id.name.as_str();
                // `Symbol("x")` is the prelude's `__Symbol`.
                if name == "Symbol"
                    && self.lookup(name).is_none()
                    && self.fids.contains_key("__Symbol")
                {
                    name = "__Symbol";
                }
                let generic_name;
                if name == "__asyncG" || name == "__newPromiseG" {
                    generic_name = self.generic_promise_name(name, c)?;
                    name = &generic_name;
                }
                if name == "__await" {
                    return self.lower_await(c);
                }
                if let Some(key) = name.strip_prefix("__asyncRet$") {
                    return self.lower_async_return(key, c);
                }
                if self.lookup(name).is_some()
                    || matches!(self.consts.get(name), Some(ConstInfo::Global(..)))
                {
                    let mut callee = self.lower_ident(name)?;
                    // `f?.(x)`: nothing happens when `f` is missing (the chain's guard).
                    if c.optional {
                        return self.call_closure_value(callee, c);
                    }
                    // A function that may be missing, tested by the caller (`if (f) f()`).
                    if callee.ty.is_nullable() && matches!(callee.ty.strip(), Ty::Fn(_)) {
                        let plain = callee.ty.strip().clone();
                        callee = mk(ExprKind::Cast(Box::new(callee)), plain);
                    }
                    let Ty::Fn(sig) = callee.ty.clone() else {
                        bail!("`{name}` is not callable")
                    };
                    let args = self.lower_args(&c.arguments, &sig.params, name)?;
                    return Ok(mk(
                        ExprKind::CallClosure(Box::new(callee), args),
                        sig.ret.clone(),
                    ));
                }
                if self.lookup(name).is_none()
                    && let Some(key) = self.cur.local_generics.get(name).cloned()
                {
                    return self.lower_generic_call(&key, c, call_exp.as_ref());
                }
                if self.lookup(name).is_none() && self.generic_fns.contains_key(name) {
                    return self.lower_generic_call(name, c, call_exp.as_ref());
                }
                if !self.fids.contains_key(name)
                    && let Some(e) = self.lower_string_global(name, c)?
                {
                    return Ok(e);
                }
                let Some(&fid) = self.fids.get(name) else {
                    if name == "register" {
                        bail!(
                            "{}: `register(Class)` must be a statement at the top level of a module (not inside an `if`, a loop or a function): the host starts the registered classes before any code runs",
                            self.loc(c.span.start)
                        )
                    }
                    bail!(
                        "{}: call to unknown function `{name}`",
                        self.loc(c.span.start)
                    )
                };
                let _ = fid;
                self.call_declared(name, &c.arguments, None)
            }
            Expression::Super(_) => self.lower_super_call(c),
            Expression::PrivateFieldExpression(p) => {
                let recv = self.lower_expr(&p.object, None)?;
                self.lower_method(recv, &classes::private_name(p.field.name.as_str()), c)
            }
            Expression::StaticMemberExpression(m) => {
                let method = m.property.name.as_str();
                if let Expression::Identifier(id) = &m.object
                    && id.name == "BigInt"
                    && self.lookup("BigInt").is_none()
                {
                    return self.lower_bigint_static(method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name.starts_with("Temporal_")
                    && method == "from"
                    && self.classes.contains_key(id.name.as_str())
                {
                    return self.lower_temporal_from(id.name.as_str(), c);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "Intl"
                    && method == "getCanonicalLocales"
                    && self.fids.contains_key("__canonLocales1")
                {
                    let Some(arg) = c.arguments.first().and_then(|a| a.as_expression()) else {
                        bail!("Intl.getCanonicalLocales needs locales")
                    };
                    let v = self.lower_expr(arg, None)?;
                    let name = if matches!(v.ty.strip(), Ty::Arr(_)) {
                        "__canonLocalesN"
                    } else {
                        "__canonLocales1"
                    };
                    let v = self.widen_lit(v);
                    return self.call_helper(name, vec![v]);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "Promise"
                    && self.lookup("Promise").is_none()
                {
                    return self.lower_promise_static(method, c, call_exp.as_ref());
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "Symbol"
                    && self.lookup("Symbol").is_none()
                    && matches!(method, "for" | "keyFor")
                    && self.fids.contains_key("__symbolFor")
                {
                    let f = if method == "for" {
                        "__symbolFor"
                    } else {
                        "__symbolKeyFor"
                    };
                    return self.call_declared(f, &c.arguments, None);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "Math"
                    && self.lookup("Math").is_none()
                {
                    return self.lower_math(method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && let Some(kind) = crate::ir::TypedKind::from_name(id.name.as_str())
                    && self.lookup(id.name.as_str()).is_none()
                {
                    return self.lower_typed_static(kind, method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "Date"
                    && self.lookup("Date").is_none()
                    && self.date_sid().is_some()
                {
                    return self.lower_date_static(method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "String"
                    && self.lookup("String").is_none()
                {
                    return self.lower_string_static(method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "Number"
                    && self.lookup("Number").is_none()
                {
                    return self.lower_number_static(method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "JSON"
                    && self.lookup("JSON").is_none()
                {
                    return match method {
                        "stringify" => self.lower_json_stringify(c),
                        "parse" => self.lower_json_parse(c, None),
                        m => bail!("{}: JSON.{m} is not supported", self.loc(c.span.start)),
                    };
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "Object"
                    && self.lookup("Object").is_none()
                {
                    return self.lower_object_static(method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "console"
                    && self.lookup("console").is_none()
                {
                    return self.lower_console(method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "Array"
                    && self.lookup("Array").is_none()
                {
                    return self.lower_array_static(method, c);
                }
                if matches!(m.object, Expression::Super(_)) {
                    return self.lower_super_method(method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && self.manifest.is_host_object(id.name.as_str())
                    && self.manifest.is_host_method(method, |h| &h.find_any)
                    && self.lookup(id.name.as_str()).is_none()
                    && let Some(e) = self.lower_find_object_of_type(c)?
                {
                    return Ok(e);
                }
                if let Expression::Identifier(id) = &m.object
                    && id.name == "GameObject"
                    && matches!(method, "instantiate" | "dontDestroyOnLoad")
                    && self.lookup("GameObject").is_none()
                    && let Some(e) = self.lower_scene_call(method, c)?
                {
                    return Ok(e);
                }
                if let Expression::Identifier(id) = &m.object
                    && method == "create"
                    && self.lookup(id.name.as_str()).is_none()
                    && self.is_create_with_options(id.name.as_str(), c)
                {
                    return self.lower_new_handle_args(
                        id.name.as_str(),
                        &c.arguments,
                        c.span.start,
                    );
                }
                if let Expression::Identifier(id) = &m.object
                    && self.lookup(id.name.as_str()).is_none()
                    && let Some(e) = self.ext_static_call(id.name.as_str(), method, c)?
                {
                    return Ok(e);
                }
                if let Expression::Identifier(id) = &m.object
                    && self.lookup(id.name.as_str()).is_none()
                    && self.manifest.namespaces.contains_key(id.name.as_str())
                {
                    return self.lower_extern_call(id.name.as_str(), method, c);
                }
                if let Expression::Identifier(id) = &m.object
                    && self.lookup(id.name.as_str()).is_none()
                    && let Some(f) = self
                        .manifest
                        .classes
                        .get(id.name.as_str())
                        .and_then(|k| k.statics.get(method.strip_suffix("$n").unwrap_or(method)))
                        .cloned()
                {
                    if method == "create" && self.is_create_with_options(id.name.as_str(), c) {
                        return self.lower_new_handle_args(
                            id.name.as_str(),
                            &c.arguments,
                            c.span.start,
                        );
                    }
                    // A static declared more than once (`Sprite.create(texture, ppu)` and
                    // `Sprite.create(texture, rect, pivot, ppu)`): the version for the arguments.
                    let mut f = f;
                    if let Some(cands) = self
                        .manifest
                        .classes
                        .get(id.name.as_str())
                        .and_then(|k| k.overloads.get(&("static".to_string(), method.to_string())))
                        .cloned()
                    {
                        f = self.pick_ext_overload(&cands, 0, &c.arguments);
                    }
                    return self.call_ext_fn(f, None, &format!("{}.{method}", id.name), c);
                }
                if let Expression::Identifier(id) = &m.object
                    && self.lookup(id.name.as_str()).is_none()
                    && let Some(info) = self.classes.get(id.name.as_str())
                {
                    let Some(f) = info.statics.get(method).cloned() else {
                        bail!("class {} has no static method `{method}`", id.name)
                    };
                    return self.call_declared(&f, &c.arguments, None);
                }
                // `new Array(n).fill(x)`: the element type is the type of `x`.
                let mut recv_hint = None;
                if method == "fill"
                    && let Expression::NewExpression(ne) = &m.object
                    && matches!(&ne.callee, Expression::Identifier(i) if i.name == "Array")
                    && ne.type_arguments.is_none()
                    && let Some(a) = c.arguments.first().and_then(|a| a.as_expression())
                {
                    let v = self.lower_expr(a, None)?;
                    let el = match v.ty {
                        Ty::Lit => Ty::Str,
                        Ty::Null => Ty::Num.nullable(),
                        t => t,
                    };
                    recv_hint = Some(Ty::Arr(Box::new(el)));
                }
                // `xs.map(..).sort(..)` where an `Item[]` is expected: the array sorted is one.
                if recv_hint.is_none()
                    && matches!(
                        method,
                        "sort" | "reverse" | "slice" | "toSorted" | "toReversed"
                    )
                    && let Some(t @ Ty::Arr(_)) = call_exp.as_ref().map(|t| t.strip())
                {
                    recv_hint = Some(t.clone());
                }
                let mut recv = self.lower_expr(&m.object, recv_hint.as_ref())?;
                if m.optional {
                    recv = self.chain_guard(recv)?;
                }
                // A literal (or a constant of one) as the receiver of a string method.
                let recv = self.widen_lit(recv);
                // `shape.area()` on an interface some class implements: through its table.
                if let Ty::Obj(sid) = recv.ty.strip()
                    && self.is_dyn(*sid)
                    && let Some(e) = self.iface_call(&recv, *sid, method, c)?
                {
                    return Ok(e);
                }
                // A function-typed field (`handlers.onHit(x)`) is called, not looked up as a method.
                if let Ty::Obj(sid) = recv.ty.strip()
                    && let Some(i) = self.m.structs[*sid].field(method)
                    && matches!(self.m.structs[*sid].fields[i].ty.strip(), Ty::Fn(_))
                {
                    let callee = self.member_of(recv, method)?;
                    return self.call_closure_value(callee, c);
                }
                self.method_exp = call_exp;
                self.lower_method(recv, method, c)
            }
            // Anything else that is a function value: `f()()`, `(() => 1)()`, `fs[0](x)`.
            callee => {
                let callee = self.lower_expr(callee, None)?;
                self.call_closure_value(callee, c)
            }
        }
    }

    /// Calls a function-typed value with the call's arguments.
    pub(crate) fn call_closure_value(
        &mut self,
        callee: Expr,
        c: &'a CallExpression<'a>,
    ) -> R<Expr> {
        let Ty::Fn(sig) = callee.ty.strip().clone() else {
            bail!(
                "{}: call of a value that is not a function",
                self.loc(c.span.start)
            )
        };
        // `o.cb?.(x)`: nothing happens when the function is missing.
        let callee = if c.optional && callee.ty.is_nullable() {
            let guarded = self.chain_guard(callee)?;
            let plain = guarded.ty.strip().clone();
            mk(ExprKind::Cast(Box::new(guarded)), plain)
        } else {
            callee
        };
        let args = self.lower_args(&c.arguments, &sig.params, "closure")?;
        Ok(mk(
            ExprKind::CallClosure(Box::new(callee), args),
            sig.ret.clone(),
        ))
    }

    /// `Math.min(a, ...xs)` / `Math.max(...xs)`: a fold over the plain arguments and the elements
    /// of each spread array, starting from the identity (`Infinity` for min, `-Infinity` for max).
    fn lower_math_spread(&mut self, f: &str, c: &'a CallExpression<'a>) -> R<Expr> {
        let (op, start) = match f {
            "min" => (BinOp::Min, f64::INFINITY),
            "max" => (BinOp::Max, f64::NEG_INFINITY),
            "hypot" => (BinOp::Hypot, 0.0),
            _ => bail!(
                "{}: Math.{f} does not take spread arguments",
                self.loc(c.span.start)
            ),
        };
        let acc = self.new_local(Ty::Num);
        let mut stmts = vec![Stmt::Assign(acc, num(start))];
        for a in &c.arguments {
            let fold = |this: &mut Self, x: Expr| {
                let cur = this.var_expr(acc);
                Stmt::Assign(
                    acc,
                    mk(ExprKind::Bin(op, Box::new(cur), Box::new(x)), Ty::Num),
                )
            };
            if let Argument::SpreadElement(sp) = a {
                let arr = self.lower_expr(&sp.argument, None)?;
                if !matches!(arr.ty.strip(), Ty::Arr(e) if matches!(e.strip(), Ty::Num))
                    && !matches!(arr.ty.strip(), Ty::Typed(_))
                {
                    bail!(
                        "{}: Math.{f}(...xs) needs numbers, not {}",
                        self.loc(sp.span.start),
                        arr.ty
                    );
                }
                let it = self.begin_iter(arr)?;
                stmts.extend(it.pre.iter().cloned());
                let mut body = it.head.clone();
                let el = self.var_expr(it.elem);
                body.push(fold(self, el));
                stmts.push(Stmt::Loop(it.label, body));
            } else if let Some(e) = a.as_expression() {
                let v = self.lower_expr(e, Some(&Ty::Num))?;
                let v = self.as_num(v)?;
                stmts.push(fold(self, v));
            }
        }
        Ok(mk(
            ExprKind::Seq(stmts, Box::new(self.var_expr(acc))),
            Ty::Num,
        ))
    }

    fn lower_math(&mut self, f: &str, c: &'a CallExpression<'a>) -> R<Expr> {
        if c.arguments
            .iter()
            .any(|a| matches!(a, Argument::SpreadElement(_)))
        {
            return self.lower_math_spread(f, c);
        }
        let mut args = Vec::new();
        for a in &c.arguments {
            let Some(e) = a.as_expression() else {
                bail!("Math.{f}: spread")
            };
            let v = self.lower_expr(e, Some(&Ty::Num))?;
            args.push(self.as_num(v)?);
        }
        let n = args.len();
        match Self::math_op(f, args) {
            Some(e) => Ok(e),
            None => bail!(
                "{}: Math.{f} with {n} arguments is not supported",
                self.loc(c.span.start)
            ),
        }
    }

    /// `Math.f(args)` over lowered arguments, or `None` for a name or arity it does not know.
    pub(crate) fn math_op(f: &str, args: Vec<Expr>) -> Option<Expr> {
        let un = |op, a: Expr| mk(ExprKind::Un(op, Box::new(a)), Ty::Num);
        let bin = |op, a: Expr, b: Expr| mk(ExprKind::Bin(op, Box::new(a), Box::new(b)), Ty::Num);
        let n = args.len();
        let mut it = args.into_iter();
        Some(match (f, n) {
            ("sqrt", 1) => un(UnOp::Sqrt, it.next().unwrap()),
            ("abs", 1) => un(UnOp::Abs, it.next().unwrap()),
            ("floor", 1) => un(UnOp::Floor, it.next().unwrap()),
            ("sin", 1) => un(UnOp::Sin, it.next().unwrap()),
            ("cos", 1) => un(UnOp::Cos, it.next().unwrap()),
            ("sign", 1) => un(UnOp::Sign, it.next().unwrap()),
            ("ceil", 1) => un(UnOp::Ceil, it.next().unwrap()),
            ("trunc", 1) => un(UnOp::Trunc, it.next().unwrap()),
            ("round", 1) => un(UnOp::Round, it.next().unwrap()),
            ("fround", 1) => un(UnOp::Fround, it.next().unwrap()),
            ("clz32", 1) => un(UnOp::Clz32, it.next().unwrap()),
            ("tan", 1) => un(UnOp::Libm("ts_rt_tan"), it.next().unwrap()),
            ("asin", 1) => un(UnOp::Libm("ts_rt_asin"), it.next().unwrap()),
            ("acos", 1) => un(UnOp::Libm("ts_rt_acos"), it.next().unwrap()),
            ("atan", 1) => un(UnOp::Libm("ts_rt_atan"), it.next().unwrap()),
            ("exp", 1) => un(UnOp::Libm("ts_rt_exp"), it.next().unwrap()),
            ("log", 1) => un(UnOp::Libm("ts_rt_log"), it.next().unwrap()),
            ("log2", 1) => un(UnOp::Libm("ts_rt_log2"), it.next().unwrap()),
            ("log10", 1) => un(UnOp::Libm("ts_rt_log10"), it.next().unwrap()),
            ("cbrt", 1) => un(UnOp::Libm("ts_rt_cbrt"), it.next().unwrap()),
            ("sinh", 1) => un(UnOp::Libm("ts_rt_sinh"), it.next().unwrap()),
            ("cosh", 1) => un(UnOp::Libm("ts_rt_cosh"), it.next().unwrap()),
            ("tanh", 1) => un(UnOp::Libm("ts_rt_tanh"), it.next().unwrap()),
            ("asinh", 1) => un(UnOp::Libm("ts_rt_asinh"), it.next().unwrap()),
            ("acosh", 1) => un(UnOp::Libm("ts_rt_acosh"), it.next().unwrap()),
            ("atanh", 1) => un(UnOp::Libm("ts_rt_atanh"), it.next().unwrap()),
            ("expm1", 1) => un(UnOp::Libm("ts_rt_expm1"), it.next().unwrap()),
            ("log1p", 1) => un(UnOp::Libm("ts_rt_log1p"), it.next().unwrap()),
            ("pow", 2) => bin(BinOp::Pow, it.next().unwrap(), it.next().unwrap()),
            ("random", 0) => mk(ExprKind::Rt(RtFn::Random, vec![]), Ty::Num),
            ("hypot", 2) => bin(BinOp::Hypot, it.next().unwrap(), it.next().unwrap()),
            ("atan2", 2) => bin(BinOp::Atan2, it.next().unwrap(), it.next().unwrap()),
            ("imul", 2) => bin(BinOp::Imul, it.next().unwrap(), it.next().unwrap()),
            ("min" | "max", n) if n >= 2 => {
                let op = if f == "min" { BinOp::Min } else { BinOp::Max };
                let mut acc = it.next().unwrap();
                for x in it {
                    acc = bin(op, acc, x);
                }
                acc
            }
            ("hypot", 0) => mk(ExprKind::Num(0.0), Ty::Num),
            ("hypot", 1) => un(UnOp::Abs, it.next().unwrap()),
            ("hypot", _) => {
                let mut acc = it.next().unwrap();
                for x in it {
                    acc = bin(BinOp::Hypot, acc, x);
                }
                acc
            }
            _ => return None,
        })
    }
}
