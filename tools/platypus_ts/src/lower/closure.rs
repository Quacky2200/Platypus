// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Closures: arrow and function expressions that are values.
//!
//! A closure is lifted to an ordinary IR function `lambda(env, params...)`; the value is a
//! `MakeClosure(lambda, env)` object `{func, env}`. `env` is an array of the cells of the
//! variables the closure captures (see `scan.rs`); inside the lifted function each captured
//! variable is a local bound to its cell, so reads and writes go through it and every closure
//! over the same variable sees the others' writes. Calls pass `env` first (`CallClosure`).

use super::expr::mk;
use super::*;

#[derive(Clone, Copy)]
pub(crate) enum ClosureNode<'a> {
    Arrow(&'a ArrowFunctionExpression<'a>),
    Func(&'a Function<'a>),
}

impl<'a> Lowerer<'a> {
    /// The environment parameter's type: an array of cells (the cells' own types differ, the
    /// lifted function casts each one it reads).
    pub(crate) fn env_ty() -> Ty {
        Ty::Arr(Box::new(Ty::Cell(Box::new(Ty::Void))))
    }

    pub(crate) fn lower_closure(&mut self, node: ClosureNode<'a>, exp: Option<&Ty>) -> R<Expr> {
        let (params, ret_ann, body, start): (
            &'a FormalParameters<'a>,
            Option<&'a TSType<'a>>,
            FnBody<'a>,
            u32,
        ) = match node {
            ClosureNode::Arrow(a) => {
                let body = match &a.body {
                    ArrowFunctionBody::FunctionBody(b) => FnBody::Block(b),
                    other => FnBody::Expr(other.as_expression().ok_or("arrow body")?),
                };
                (
                    &a.params,
                    a.return_type.as_ref().map(|r| &r.type_annotation),
                    body,
                    a.span.start,
                )
            }
            ClosureNode::Func(f) => {
                let Some(b) = &f.body else {
                    bail!("function expression without a body")
                };
                (
                    &f.params,
                    f.return_type.as_ref().map(|r| &r.type_annotation),
                    FnBody::Block(b),
                    f.span.start,
                )
            }
        };
        let exp_fn: Option<FnTy> = match exp.map(|t| t.strip()) {
            Some(Ty::Fn(f)) => Some((**f).clone()),
            _ => None,
        };
        let mut ptys = Vec::new();
        for (i, p) in params.items.iter().enumerate() {
            let ty = match &p.type_annotation {
                Some(a) => self.ty(&a.type_annotation, "closure")?,
                None => match exp_fn.as_ref().and_then(|f| f.params.get(i)) {
                    Some(t) => t.clone(),
                    // `(scale = 0.75) => ..`: the default's literal gives the type.
                    None => match p.initializer.as_deref() {
                        Some(Expression::NumericLiteral(_)) => Ty::Num,
                        Some(Expression::StringLiteral(_) | Expression::TemplateLiteral(_)) => {
                            Ty::Str
                        }
                        Some(Expression::BooleanLiteral(_)) => Ty::Bool,
                        // `(i = index) => ..`: the default's type, as TypeScript infers it.
                        Some(d) => {
                            let mark = self.cur.locals.len();
                            let v = self.lower_expr(d, None);
                            self.cur.locals.truncate(mark);
                            match v {
                                Ok(v) if !matches!(v.ty, Ty::Null | Ty::Void) => {
                                    let v = self.widen_lit(v);
                                    v.ty.strip().clone()
                                }
                                _ => bail!(
                                    "{}: closure parameter needs a type annotation",
                                    self.loc(start)
                                ),
                            }
                        }
                        _ => bail!(
                            "{}: closure parameter needs a type annotation",
                            self.loc(start)
                        ),
                    },
                },
            };
            // A parameter with a default may be left out: it is null, and the body replaces it.
            let defaulted = p.initializer.is_some()
                && (ty.is_ptr() || matches!(ty, Ty::Num | Ty::Bool | Ty::Lit));
            ptys.push(if (p.optional && p.initializer.is_none()) || defaulted {
                ty.nullable()
            } else {
                ty
            });
        }
        if let Some(r) = &params.rest {
            let i = params.items.len();
            let ty = match &r.type_annotation {
                Some(a) => self.ty(&a.type_annotation, "closure")?,
                None => match exp_fn.as_ref().and_then(|f| f.params.get(i)) {
                    Some(t) => t.clone(),
                    None => bail!(
                        "{}: closure rest parameter needs a type annotation",
                        self.loc(start)
                    ),
                },
            };
            ptys.push(ty);
        }
        // A callback may take fewer parameters than the type says; the rest are ignored.
        if params.rest.is_none()
            && let Some(f) = &exp_fn
        {
            for t in f.params.iter().skip(ptys.len()) {
                ptys.push(t.clone());
            }
        }
        let infer = std::mem::take(&mut self.infer_ret);
        let ret_exp = match ret_ann {
            Some(r) => Some(self.ty(r, "closure")?),
            None if infer => None,
            None => exp_fn.as_ref().map(|f| f.ret.clone()),
        };

        // The variables it shares with the code around it.
        let mut caps: Vec<(String, VarId, Ty)> = Vec::new();
        for name in self.closure_free.get(&start).cloned().unwrap_or_default() {
            match self.lookup(&name) {
                Some(Binding::Boxed(cell, ty)) => caps.push((name, cell, ty)),
                Some(Binding::Direct(_)) => {
                    bail!("internal: `{name}` is captured but was not boxed")
                }
                None => {} // a module-level name
            }
        }
        let env_expr = if caps.is_empty() {
            None
        } else {
            let ty = Self::env_ty();
            let tmp = self.new_local(ty.clone());
            let mut stmts = vec![Stmt::Assign(
                tmp,
                mk(
                    ExprKind::Rt(RtFn::ArrNew, vec![super::expr::num(caps.len() as f64)]),
                    ty.clone(),
                ),
            )];
            for (_, cell, _) in &caps {
                let c = self.var_expr(*cell);
                stmts.push(Stmt::Eval(mk(
                    ExprKind::Rt(RtFn::ArrPush, vec![self.var_expr(tmp), c]),
                    Ty::Void,
                )));
            }
            Some(Box::new(mk(
                ExprKind::Seq(stmts, Box::new(self.var_expr(tmp))),
                ty,
            )))
        };

        // Lift the body to a function of its own.
        let id = self.m.funcs.len();
        self.lambdas += 1;
        self.m.funcs.push(Func {
            name: format!("lambda{}", self.lambdas),
            exported: false,
            params: vec![],
            ret: Ty::Void,
            locals: vec![],
            body: vec![],
        });
        self.fsig.push((vec![], None));
        self.fstate.push(2);

        let saved = std::mem::take(&mut self.cur);
        self.cur.local_generics = saved.local_generics.clone();
        // TypeScript keeps the narrowing of a `const`, and of a parameter or `let` that is never
        // assigned, inside a closure: the closure's free names that are plain variables of the
        // code around it (a path through a property does not survive).
        let free = self.closure_free.get(&start).cloned().unwrap_or_default();
        self.cur.narrowed = saved
            .narrowed
            .iter()
            .filter(|(p, _)| free.contains(p) && !self.assigned.contains(p))
            .cloned()
            .collect();
        // The names a captured literal-union value can hold (`[key]: ..` of an outer parameter).
        self.cur.lit_sets = saved
            .lit_sets
            .iter()
            .filter(|(n, _)| free.contains(*n) && !self.assigned.contains(*n))
            .map(|(n, m)| (n.clone(), m.clone()))
            .collect();
        // No written or expected return type: the returns decide it (`return null` and then a
        // string is `string | null`), as for a function declaration.
        self.cur.ret_inferred = ret_exp.is_none();
        self.cur.ret = ret_exp;
        self.cur.widen_ret = infer;
        self.cur.scopes.push(HashMap::new());
        // An error in an expression body (`() => setMode(x)`) says where the closure is.
        let r = self.lift_body(params, &ptys, &caps, body).map_err(|e| {
            if e.contains("line ") {
                e
            } else {
                format!("{}: {e}", self.loc(start))
            }
        });
        let ctx = std::mem::take(&mut self.cur);
        self.cur = saved;
        let (pvars, envv, stmts, ret) = r?;

        self.m.local_names.insert(id, ctx.names);
        let f = &mut self.m.funcs[id];
        f.params = std::iter::once(envv).chain(pvars).collect();
        f.ret = ret.clone();
        f.locals = ctx.locals;
        f.body = stmts;
        self.fsig[id] = (ptys.clone(), Some(ret.clone()));
        Ok(mk(
            ExprKind::MakeClosure(id, env_expr),
            Ty::Fn(Box::new(FnTy { params: ptys, ret })),
        ))
    }

    #[allow(clippy::type_complexity)]
    fn lift_body(
        &mut self,
        params: &'a FormalParameters<'a>,
        ptys: &[Ty],
        caps: &[(String, VarId, Ty)],
        body: FnBody<'a>,
    ) -> R<(Vec<VarId>, VarId, Vec<Stmt>, Ty)> {
        let envv = self.new_local(Self::env_ty());
        let mut pro = Vec::new();
        for (i, (name, _, ty)) in caps.iter().enumerate() {
            let cell = self.new_local(Ty::Cell(Box::new(ty.clone())));
            let slot = mk(
                ExprKind::Index(
                    Box::new(self.var_expr(envv)),
                    Box::new(super::expr::num(i as f64)),
                ),
                Ty::Cell(Box::new(Ty::Void)).nullable(),
            );
            let cast = mk(
                ExprKind::Cast(Box::new(slot)),
                Ty::Cell(Box::new(ty.clone())),
            );
            pro.push(Stmt::Assign(cell, cast));
            self.cur
                .scopes
                .last_mut()
                .unwrap()
                .insert(name.clone(), Binding::Boxed(cell, ty.clone()));
        }
        let mut pvars = Vec::new();
        let mut ptys = ptys.iter();
        for (p, ty) in params.items.iter().zip(ptys.by_ref()) {
            if let BindingPattern::BindingIdentifier(id) = &p.pattern
                && ty.strip() == &Ty::Lit
                && let Some(a) = &p.type_annotation
                && let Some(ms) = self.lit_members_opt(&a.type_annotation)
            {
                self.cur.lit_sets.insert(id.name.to_string(), ms);
            }
            let v = self.new_local(ty.clone());
            pvars.push(v);
            match &p.initializer {
                Some(init) if ty.is_nullable() && !p.optional => {
                    let base = ty.strip().clone();
                    let t = self.var_expr(v);
                    let d = self.lower_expr(init, Some(&base))?;
                    let d = self.coerce(d, &base)?;
                    let have = mk(ExprKind::Cast(Box::new(t.clone())), base.clone());
                    let picked = mk(
                        ExprKind::Cond(
                            Box::new(mk(ExprKind::IsNull(Box::new(t)), Ty::Bool)),
                            Box::new(d),
                            Box::new(have),
                        ),
                        base,
                    );
                    self.bind_pattern(&p.pattern, picked, &mut pro)?;
                }
                _ => self.bind_param(&p.pattern, v, &mut pro)?,
            }
        }
        if let (Some(r), Some(ty)) = (&params.rest, ptys.next()) {
            let v = self.new_local(ty.clone());
            pvars.push(v);
            self.bind_param(&r.rest.argument, v, &mut pro)?;
        }
        // Parameters the closure does not name (it takes fewer than its type).
        for ty in ptys {
            pvars.push(self.new_local(ty.clone()));
        }
        let (stmts, ret) = self.lower_body_into(body, pro)?;
        Ok((pvars, envv, stmts, ret))
    }

    /// A top-level function used as a value: a closure over a small wrapper that adds the
    /// environment parameter and calls it.
    pub(crate) fn function_value(&mut self, name: &str) -> R<Expr> {
        self.ensure_lowered(name)?;
        let fid = self.fids[name];
        let (ptys, ret) = self.fsig[fid].clone();
        let ret = ret.ok_or_else(|| format!("{name}: return type unknown"))?;
        let wid = match self.wrappers.get(name) {
            Some(w) => *w,
            None => {
                let mut locals = vec![Self::env_ty()];
                locals.extend(ptys.iter().cloned());
                let params: Vec<VarId> = (0..locals.len() as u32).collect();
                let args: Vec<Expr> = (1..locals.len())
                    .map(|i| mk(ExprKind::Var(i as u32), locals[i].clone()))
                    .collect();
                let call = mk(ExprKind::Call(fid, args), ret.clone());
                let body = if ret == Ty::Void {
                    vec![Stmt::Eval(call), Stmt::Return(None)]
                } else {
                    vec![Stmt::Return(Some(call))]
                };
                let wid = self.m.funcs.len();
                self.m.funcs.push(Func {
                    name: format!("{name}$value"),
                    exported: false,
                    params,
                    ret: ret.clone(),
                    locals,
                    body,
                });
                self.fsig.push((ptys.clone(), Some(ret.clone())));
                self.fstate.push(2);
                self.wrappers.insert(name.to_string(), wid);
                wid
            }
        };
        Ok(mk(
            ExprKind::MakeClosure(wid, None),
            Ty::Fn(Box::new(FnTy { params: ptys, ret })),
        ))
    }
}
