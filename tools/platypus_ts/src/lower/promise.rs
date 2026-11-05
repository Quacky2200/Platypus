// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Promises in the lowerer: routing `await`, `.then/.catch/.finally`, `Promise.*` and the value an
//! async function returns to the per-type helpers `asyncs.rs` writes. Only here are the types
//! known: `p.then(v => v + 1)` is `__then$number$number` because the callback returns a number.

use super::classes::Target;
use super::expr::mk;
use super::*;

impl<'a> Lowerer<'a> {
    /// The key and value type when `t` is a promise class.
    pub(crate) fn promise_of(&self, t: &Ty) -> Option<(String, Ty)> {
        let Ty::Obj(sid) = t.strip() else {
            return None;
        };
        let st = &self.m.structs[*sid];
        let key = st.name.strip_prefix("Promise$")?;
        let ix = st.field("value")?;
        Some((key.to_string(), st.fields[ix].ty.clone()))
    }

    /// The key of the promise class whose value type is `t`.
    pub(crate) fn promise_key_for(&self, t: &Ty) -> Option<String> {
        let t = if *t == Ty::Void { &Ty::Num } else { t };
        self.m.structs.iter().find_map(|st| {
            let key = st.name.strip_prefix("Promise$")?;
            let ix = st.field("value")?;
            (st.fields[ix].ty == *t).then(|| key.to_string())
        })
    }

    /// A call of a declared helper with arguments that are already lowered.
    pub fn call_helper(&mut self, name: &str, args: Vec<Expr>) -> R<Expr> {
        let Some(&fid) = self.fids.get(name) else {
            bail!(
                "`{name}` does not exist: the program has no promise type for it (write `Promise<T>` for the type somewhere), or does not use the feature it belongs to"
            )
        };
        self.ensure_lowered(name)?;
        let (params, ret) = self.fsig[fid].clone();
        let Some(ret) = ret else {
            bail!("{name}: return type unknown")
        };
        let mut vals = Vec::new();
        for (a, p) in args.into_iter().zip(&params) {
            vals.push(self.coerce(a, p)?);
        }
        Ok(Self::target_call(&Target::Direct(fid), vals, ret))
    }

    /// A callback argument lowered with its return type inferred from the body.
    fn lower_callback(&mut self, e: &'a Expression<'a>, params: Vec<Ty>) -> R<Expr> {
        let exp = Ty::Fn(Box::new(FnTy {
            params,
            ret: Ty::Void,
        }));
        self.infer_ret = true;
        let r = self.lower_expr(e, Some(&exp));
        self.infer_ret = false;
        r
    }

    fn error_ty(&self) -> Ty {
        Ty::Obj(self.classes_sid("Error"))
    }

    /// `p.then(f)`, `p.catch(f)`, `p.finally(f)`.
    pub(crate) fn lower_promise_method(
        &mut self,
        recv: Expr,
        key: &str,
        value: Ty,
        m: &str,
        c: &'a CallExpression<'a>,
    ) -> R<Expr> {
        let at = self.loc(c.span.start);
        let args: Vec<&'a Expression<'a>> = c
            .arguments
            .iter()
            .filter_map(|a| a.as_expression())
            .collect();
        if m == "then" && args.len() == 2 {
            return self.lower_then2(recv, key, value, args[0], args[1], &at);
        }
        let [cb] = args.as_slice() else {
            bail!("{at}: Promise.{m} takes one callback (then takes two)")
        };
        match m {
            "then" => {
                let f = self.lower_callback(cb, vec![value])?;
                let Ty::Fn(sig) = f.ty.strip().clone() else {
                    bail!("{at}: then needs a function")
                };
                if sig.ret == Ty::Void {
                    return self.call_helper(&format!("__thenV${key}"), vec![recv, f]);
                }
                if let Some((rk, _)) = self.promise_of(&sig.ret) {
                    return self.call_helper(&format!("__thenP${key}${rk}"), vec![recv, f]);
                }
                let Some(rk) = self.promise_key_for(&sig.ret) else {
                    bail!(
                        "{at}: then returns {} but the program has no `Promise<{}>`; write that type somewhere",
                        sig.ret,
                        sig.ret
                    )
                };
                self.call_helper(&format!("__then${key}${rk}"), vec![recv, f])
            }
            "catch" => {
                let f = self.lower_callback(cb, vec![self.error_ty()])?;
                let Ty::Fn(sig) = f.ty.strip().clone() else {
                    bail!("{at}: catch needs a function")
                };
                let name = if sig.ret == Ty::Void {
                    "__catchV"
                } else if self.promise_of(&sig.ret).is_some() {
                    "__catchP"
                } else {
                    "__catch"
                };
                self.call_helper(&format!("{name}${key}"), vec![recv, f])
            }
            "finally" => {
                let f = self.lower_callback(cb, vec![])?;
                self.call_helper(&format!("__finally${key}"), vec![recv, f])
            }
            _ => unreachable!("routed by name"),
        }
    }

    /// `p.then(f, g)`: both callbacks give the same kind of result.
    fn lower_then2(
        &mut self,
        recv: Expr,
        key: &str,
        value: Ty,
        f: &'a Expression<'a>,
        g: &'a Expression<'a>,
        at: &str,
    ) -> R<Expr> {
        let f = self.lower_callback(f, vec![value])?;
        let g = self.lower_callback(g, vec![self.error_ty()])?;
        let (Ty::Fn(fs), Ty::Fn(gs)) = (f.ty.strip().clone(), g.ty.strip().clone()) else {
            bail!("{at}: then needs functions")
        };
        if fs.ret == Ty::Void && gs.ret == Ty::Void {
            return self.call_helper(&format!("__then2V${key}"), vec![recv, f, g]);
        }
        if fs.ret == Ty::Void || gs.ret == Ty::Void {
            bail!(
                "{at}: the two callbacks of then(f, g) must both return a value or both return nothing"
            )
        }
        if let (Some((rk, _)), Some(_)) = (self.promise_of(&fs.ret), self.promise_of(&gs.ret)) {
            return self.call_helper(&format!("__then2P${key}${rk}"), vec![recv, f, g]);
        }
        let Some(rk) = self.promise_key_for(&fs.ret) else {
            bail!("{at}: no `Promise<{}>` in the program", fs.ret)
        };
        self.call_helper(&format!("__then2${key}${rk}"), vec![recv, f, g])
    }

    /// `Promise.resolve`, `Promise.reject`, `Promise.all`, `Promise.race`.
    pub(crate) fn lower_promise_static(
        &mut self,
        m: &str,
        c: &'a CallExpression<'a>,
        exp: Option<&Ty>,
    ) -> R<Expr> {
        // The promise the context expects (`const p: Promise<Listing> = Promise.resolve({ .. })`).
        let wanted = exp.and_then(|t| self.promise_of(t));
        let at = self.loc(c.span.start);
        let Some(arg) = c.arguments.first().and_then(|a| a.as_expression()) else {
            // `Promise.resolve()`: a promise of nothing.
            if m == "resolve" {
                return self
                    .call_helper("__resolved$number", vec![mk(ExprKind::Num(0.0), Ty::Num)]);
            }
            bail!("{at}: Promise.{m} needs an argument")
        };
        // `Promise.all([a, b])` over promises of different types would be a tuple of promises'
        // values; a promise class is written per type, so there is no such result type.
        if matches!(m, "all" | "race" | "any" | "allSettled")
            && let Expression::ArrayExpression(list) = arg
            && list.elements.len() > 1
        {
            let mut keys: Vec<Option<String>> = Vec::new();
            for el in &list.elements {
                let Some(e) = el.as_expression() else {
                    keys.clear();
                    break;
                };
                let mark = self.cur.locals.len();
                let t = self.lower_expr(e, None);
                self.cur.locals.truncate(mark);
                keys.push(self.promise_of(&t?.ty).map(|(k, _)| k));
            }
            if keys.iter().any(|k| k.is_some()) && keys.windows(2).any(|w| w[0] != w[1]) {
                bail!(
                    "{at}: Promise.{m} over promises of different value types is not supported unless it is awaited on the spot (`const [a, b] = await Promise.all([p, q])`, which gives a tuple): await each one, or give them one type"
                )
            }
        }
        let value_exp = match (m, &wanted) {
            ("resolve", Some((_, t))) => Some(t.clone()),
            _ => None,
        };
        let mut v = self.lower_expr(arg, value_exp.as_ref())?;
        if let Some(t) = &value_exp
            && self.promise_of(&v.ty).is_none()
        {
            v = self.coerce(v, t)?;
        }
        // `Promise.resolve("c")`: a string literal is a string.
        if v.ty == Ty::Lit {
            v = self.coerce(v, &Ty::Str)?;
        }
        match m {
            "resolve" => {
                if self.promise_of(&v.ty).is_some() {
                    return Ok(v);
                }
                let Some(k) = self.promise_key_for(&v.ty) else {
                    bail!("{at}: no `Promise<{}>` in the program", v.ty)
                };
                self.call_helper(&format!("__resolved${k}"), vec![v])
            }
            "reject" => {
                let k = match c.type_arguments.as_ref().and_then(|a| a.params.first()) {
                    Some(t) => {
                        let t = self.ty(t, "gen")?;
                        self.promise_key_for(&t)
                            .ok_or_else(|| format!("{at}: no `Promise<{t}>` in the program"))?
                    }
                    None => match &wanted {
                        Some((k, _)) => k.clone(),
                        None => "number".to_string(),
                    },
                };
                self.call_helper(&format!("__rejected${k}"), vec![v])
            }
            "all" | "race" | "any" | "allSettled" => {
                let Ty::Arr(el) = v.ty.strip().clone() else {
                    bail!("{at}: Promise.{m} takes an array of promises")
                };
                let Some((k, _)) = self.promise_of(&el) else {
                    bail!("{at}: Promise.{m} takes an array of promises")
                };
                self.call_helper(&format!("__{m}${k}"), vec![v])
            }
            _ => bail!("{at}: Promise.{m} is not supported"),
        }
    }

    /// `__await(e)`: wait for a promise (or give one tick to a plain value).
    pub(crate) fn lower_await(&mut self, c: &'a CallExpression<'a>) -> R<Expr> {
        let at = self.loc(c.span.start);
        let Some(arg) = c.arguments.first().and_then(|a| a.as_expression()) else {
            bail!("{at}: await needs an operand")
        };
        // `await Promise.all([a, b])` over promises of different value types: a tuple.
        if let Some(e) = self.await_all_tuple(arg)? {
            return Ok(e);
        }
        let mut v = self.lower_expr(arg, None)?;
        if v.ty == Ty::Lit {
            v = self.coerce(v, &Ty::Str)?;
        }
        if let Some((k, _)) = self.promise_of(&v.ty) {
            return self.call_helper(&format!("__await${k}"), vec![v]);
        }
        let Some(k) = self.promise_key_for(&v.ty) else {
            bail!(
                "{at}: awaiting a {} needs a `Promise<{}>` somewhere in the program",
                v.ty,
                v.ty
            )
        };
        self.call_helper(&format!("__awaitV${k}"), vec![v])
    }

    /// `await Promise.all([a, b, c])` where the promises have different value types: the result
    /// is a tuple, which no promise class stands for (they are written per type, before lowering),
    /// so the operand is not made into a promise: the elements are evaluated (starting every
    /// promise), then awaited one after the other into a tuple. A promise that rejects while an
    /// earlier one is still pending is reported as unhandled until its turn comes.
    fn await_all_tuple(&mut self, arg: &'a Expression<'a>) -> R<Option<Expr>> {
        let mut inner = arg;
        while let Expression::ParenthesizedExpression(p) = inner {
            inner = &p.expression;
        }
        let Expression::CallExpression(call) = inner else {
            return Ok(None);
        };
        let Expression::StaticMemberExpression(m) = &call.callee else {
            return Ok(None);
        };
        if m.property.name != "all"
            || !matches!(&m.object, Expression::Identifier(id) if id.name == "Promise")
            || self.lookup("Promise").is_some()
        {
            return Ok(None);
        }
        let Some(Expression::ArrayExpression(list)) =
            call.arguments.first().and_then(|a| a.as_expression())
        else {
            return Ok(None);
        };
        if list.elements.len() < 2 || list.elements.iter().any(|e| e.as_expression().is_none()) {
            return Ok(None);
        }
        // Do the promises have one type? Then the promise class of the array does the work.
        let mut keys: Vec<Option<String>> = Vec::new();
        for el in &list.elements {
            let e = el.as_expression().expect("checked");
            let mark = self.cur.locals.len();
            let t = self.lower_expr(e, None);
            self.cur.locals.truncate(mark);
            keys.push(self.promise_of(&t?.ty).map(|(k, _)| k));
        }
        if keys.windows(2).all(|w| w[0] == w[1]) {
            return Ok(None);
        }
        let mut stmts = Vec::new();
        let mut parts: Vec<Expr> = Vec::new();
        let mut started = Vec::new();
        for el in &list.elements {
            let e = el.as_expression().expect("checked");
            let mut v = self.lower_expr(e, None)?;
            if v.ty == Ty::Lit {
                v = self.coerce(v, &Ty::Str)?;
            }
            let tmp = self.new_local(v.ty.clone());
            stmts.push(Stmt::Assign(tmp, v));
            started.push(tmp);
        }
        for tmp in started {
            let v = self.var_expr(tmp);
            let awaited = if let Some((k, _)) = self.promise_of(&v.ty) {
                self.call_helper(&format!("__await${k}"), vec![v])?
            } else {
                let Some(k) = self.promise_key_for(&v.ty) else {
                    bail!(
                        "{}: awaiting a {} needs a `Promise<{}>` somewhere in the program",
                        self.loc(call.span.start),
                        v.ty,
                        v.ty
                    )
                };
                self.call_helper(&format!("__awaitV${k}"), vec![v])?
            };
            let slot = self.new_local(awaited.ty.clone());
            stmts.push(Stmt::Assign(slot, awaited));
            parts.push(self.var_expr(slot));
        }
        let tuple = self.tuple_or_array(parts)?;
        let ty = tuple.ty.clone();
        Ok(Some(mk(ExprKind::Seq(stmts, Box::new(tuple)), ty)))
    }

    /// The helper `__asyncG<X>(..)` / `__newPromiseG<X>(..)` stands for, once `X` (written with a
    /// type parameter) is known: `__async$K` / `__newPromise$K` of the promise class of that type.
    pub(crate) fn generic_promise_name(
        &mut self,
        name: &str,
        c: &'a CallExpression<'a>,
    ) -> R<String> {
        let at = self.loc(c.span.start);
        let Some(ta) = c.type_arguments.as_ref().and_then(|t| t.params.first()) else {
            bail!("{at}: {name} needs its promise type")
        };
        let key = self.generic_promise_key(ta, &at)?;
        Ok(if name == "__asyncG" {
            format!("__async${key}")
        } else {
            format!("__newPromise${key}")
        })
    }

    /// The class key of `Promise<X>` where `X` mentions a type parameter of the specialisation
    /// being lowered. The class must exist: some `Promise<..>` of that type is written in the
    /// program (a promise class is written per value type before lowering starts).
    pub(crate) fn generic_promise_key(&mut self, x: &'a TSType<'a>, at: &str) -> R<String> {
        let t = self.ty(x, "gen")?;
        if let Some(k) = self.promise_key_for(&t) {
            return Ok(k);
        }
        let sp = oxc_span::GetSpan::span(x);
        let text = self.src[sp.start as usize..sp.end as usize]
            .trim()
            .to_string();
        let t = self.ty_key(&t);
        let t = t.split('#').next().unwrap_or(&t);
        bail!(
            "{at}: a generic function or class makes `Promise<{text}>` here with {text} = {t}, but the program never writes `Promise<{t}>`. A promise class exists per value type written in the source (before lowering), so write that type once, for example `let p: Promise<{t}>` at the top of the file"
        )
    }

    /// `__asyncRet$K(__p, e)`: what `return e` does in an async function.
    pub(crate) fn lower_async_return(&mut self, key: &str, c: &'a CallExpression<'a>) -> R<Expr> {
        let at = self.loc(c.span.start);
        let args: Vec<&'a Expression<'a>> = c
            .arguments
            .iter()
            .filter_map(|a| a.as_expression())
            .collect();
        let [p, e] = args.as_slice() else {
            bail!("{at}: __asyncRet$K(promise, value)")
        };
        let p = self.lower_expr(p, None)?;
        // A promise made in a generic function names its class by the specialisation.
        let key = &self.promise_of(&p.ty).map_or(key.to_string(), |(k, _)| k);
        // The value is what the promise holds (an object literal takes its shape from it).
        let want = self.promise_of(&p.ty).map(|(_, v)| v);
        let e = self.lower_expr(e, want.as_ref())?;
        if e.ty == Ty::Void {
            let done = self.call_helper(&format!("__asyncRetN${key}"), vec![p])?;
            return Ok(mk(
                ExprKind::Seq(vec![Stmt::Eval(e)], Box::new(done)),
                Ty::Void,
            ));
        }
        if self.promise_of(&e.ty).is_some() {
            return self.call_helper(&format!("__asyncRetP${key}"), vec![p, e]);
        }
        self.call_helper(&format!("__asyncRetV${key}"), vec![p, e])
    }
}
