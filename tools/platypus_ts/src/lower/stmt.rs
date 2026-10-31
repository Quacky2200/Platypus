// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Statement lowering.

use super::expr::mk;
use super::*;

impl<'a> Lowerer<'a> {
    /// Does the statement always leave its block (`return`, `throw`, `break`, `continue`)?
    fn always_exits(s: &Statement<'_>) -> bool {
        match s {
            Statement::ReturnStatement(_)
            | Statement::ThrowStatement(_)
            | Statement::BreakStatement(_)
            | Statement::ContinueStatement(_) => true,
            Statement::BlockStatement(b) => b.body.last().is_some_and(Self::always_exits),
            _ => false,
        }
    }

    pub(crate) fn lower_stmts(&mut self, stmts: &'a [Statement<'a>], out: &mut Vec<Stmt>) -> R<()> {
        // Narrowings a block's early exits leave end with the block.
        let narrowed_mark = self.cur.narrowed.len();
        let r = self.lower_stmts_in(stmts, out);
        self.cur.narrowed.truncate(narrowed_mark);
        r
    }

    fn lower_stmts_in(&mut self, stmts: &'a [Statement<'a>], out: &mut Vec<Stmt>) -> R<()> {
        // Function declarations inside a body are hoisted, as in JavaScript: each becomes a closure
        // made as soon as the variables it uses from this block are declared (at the top when it
        // uses none), so a call above the declaration finds it.
        let mut hoisted: Vec<(usize, &'a Function<'a>)> = Vec::new();
        for s in stmts {
            if let Statement::FunctionDeclaration(f) = s
                && f.id.is_some()
                && f.body.is_some()
            {
                let free = self
                    .closure_free
                    .get(&f.span.start)
                    .cloned()
                    .unwrap_or_default();
                let at = stmts
                    .iter()
                    .rposition(|d| declares_any(d, &free))
                    .map_or(0, |j| j + 1);
                hoisted.push((at, f));
            }
        }
        self.prebind_arrows(stmts, out)?;
        for (j, s) in stmts.iter().enumerate() {
            for (_, f) in hoisted.iter().filter(|(at, _)| *at == j) {
                self.lower_fn_decl_here(f, out)?;
            }
            if matches!(s, Statement::FunctionDeclaration(f) if hoisted.iter().any(|(_, h)| std::ptr::eq(*h, &**f)))
            {
                continue;
            }
            if !self.in_prelude(oxc_span::GetSpan::span(s).start) {
                out.push(Stmt::Line(
                    self.line(oxc_span::GetSpan::span(s).start) as u32
                ));
            }
            // A message without a line gets the statement's (the innermost one wins).
            self.lower_stmt(s, out).map_err(|e| {
                if e.contains("line ") {
                    e
                } else {
                    format!("{}: {e}", self.loc(oxc_span::GetSpan::span(s).start))
                }
            })?;
        }
        for (_, f) in hoisted.iter().filter(|(at, _)| *at == stmts.len()) {
            self.lower_fn_decl_here(f, out)?;
        }
        Ok(())
    }

    /// `const a = (n: Node): T => b(n); const b = (n: Node): T => a(n)`: closures of a block that a
    /// closure uses (so one may call another declared after it) get their cells at the top of the
    /// block when their types are written; the declarations fill them.
    fn prebind_arrows(&mut self, stmts: &'a [Statement<'a>], out: &mut Vec<Stmt>) -> R<()> {
        for s in stmts {
            let Statement::VariableDeclaration(v) = s else {
                continue;
            };
            if v.kind != VariableDeclarationKind::Const {
                continue;
            }
            for d in &v.declarations {
                let (
                    BindingPattern::BindingIdentifier(id),
                    None,
                    Some(Expression::ArrowFunctionExpression(a)),
                ) = (&d.id, &d.type_annotation, &d.init)
                else {
                    continue;
                };
                if !self.captured.contains(id.name.as_str()) || a.type_parameters.is_some() {
                    continue;
                }
                let Some(t) = self.self_ref_arrow_ty(a)? else {
                    continue;
                };
                let cell = self.new_local(Ty::Cell(Box::new(t.clone())));
                let cell_ty = self.cur.locals[cell as usize].clone();
                let zero = self.default_value(&t);
                out.push(Stmt::Assign(
                    cell,
                    Expr {
                        kind: ExprKind::NewCell(Box::new(zero)),
                        ty: cell_ty,
                    },
                ));
                let scope = self.cur.scopes.len() - 1;
                self.cur.scopes[scope].insert(id.name.to_string(), Binding::Boxed(cell, t.clone()));
                self.cur.prebound.insert(id.name.to_string(), (cell, t));
            }
        }
        Ok(())
    }

    /// `function name(..) { .. }` inside a body: a closure bound to `name` in this scope.
    fn lower_fn_decl_here(&mut self, f: &'a Function<'a>, out: &mut Vec<Stmt>) -> R<()> {
        let Some(id) = &f.id else {
            bail!("a function declaration without a name")
        };
        let v = self.lower_closure(super::closure::ClosureNode::Func(f), None)?;
        let tmp = self.new_local(v.ty.clone());
        out.push(Stmt::Assign(tmp, v));
        self.bind_var(id.name.as_str(), tmp, out);
        Ok(())
    }

    pub(crate) fn scoped(&mut self, s: &'a Statement<'a>) -> R<Vec<Stmt>> {
        self.cur.scopes.push(HashMap::new());
        let mut out = Vec::new();
        let r = self.lower_stmt(s, &mut out);
        self.cur.scopes.pop();
        r?;
        Ok(out)
    }

    fn lower_stmt(&mut self, s: &'a Statement<'a>, out: &mut Vec<Stmt>) -> R<()> {
        match s {
            Statement::EmptyStatement(_) => {}
            Statement::BlockStatement(b) => {
                self.cur.scopes.push(HashMap::new());
                let r = self.lower_stmts(&b.body, out);
                self.cur.scopes.pop();
                r?;
            }
            Statement::ExpressionStatement(e) => self.lower_effect(&e.expression, out)?,
            Statement::VariableDeclaration(v) => self.lower_var_decl(v, out)?,
            Statement::IfStatement(i) => {
                let c = self.lower_cond(&i.test)?;
                let (yes, no) = if self.unions.is_empty() {
                    (vec![], vec![])
                } else {
                    (
                        self.narrowings(&i.test, true),
                        self.narrowings(&i.test, false),
                    )
                };
                let then = self.with_narrowed(yes, |s| s.scoped(&i.consequent))?;
                let els = match &i.alternate {
                    Some(a) => self.with_narrowed(no.clone(), |s| s.scoped(a))?,
                    None => vec![],
                };
                out.push(Stmt::If(c, then, els));
                // `if (typeof x === "string") return ..;`: the rest of the block is the other case.
                if i.alternate.is_none() && Self::always_exits(&i.consequent) {
                    self.cur.narrowed.extend(no);
                }
            }
            Statement::WhileStatement(w) => {
                let label = self.new_label();
                let c = self.lower_cond(&w.test)?;
                let not = mk(ExprKind::Un(UnOp::Not, Box::new(c)), Ty::Bool);
                let entry = self.loop_entry(label, Some(label));
                self.cur.loops.push(entry);
                let body = self.scoped(&w.body);
                self.cur.loops.pop();
                let mut b = vec![Stmt::If(not, vec![Stmt::Break(label)], vec![])];
                b.extend(body?);
                out.push(Stmt::Loop(label, b));
            }
            Statement::ForOfStatement(f) => {
                let ForStatementLeft::VariableDeclaration(d) = &f.left else {
                    bail!("for-of over an existing variable")
                };
                let pat = &d.declarations[0].id;
                // `xs.entries()`, `xs.keys()`, `xs.values()` iterate the array directly.
                let mut mode = "";
                let mut source = &f.right;
                if let Expression::CallExpression(call) = &f.right
                    && call.arguments.is_empty()
                    && let Expression::StaticMemberExpression(m) = &call.callee
                    && matches!(m.property.name.as_str(), "entries" | "keys" | "values")
                {
                    mode = m.property.name.as_str();
                    source = &m.object;
                }
                // `Object.entries(o)`, `Object.keys(o)`, `Object.values(o)`: the object's fields.
                if let Expression::CallExpression(call) = &f.right
                    && call.arguments.len() == 1
                    && let Expression::StaticMemberExpression(m) = &call.callee
                    && matches!(&m.object, Expression::Identifier(id) if id.name == "Object")
                    && self.lookup("Object").is_none()
                    && m.property.name == "entries"
                {
                    return self.lower_for_object_entries(f, call, pat, out);
                }
                if mode.is_empty()
                    && let Expression::ArrayExpression(lit) = source
                    && lit.elements.len() >= 2
                    && lit.elements.iter().all(|e| e.as_expression().is_some())
                    && self.lower_for_unrolled(f, pat, lit, out)?
                {
                    return Ok(());
                }
                let arr = self.lower_expr(source, None)?;
                let arr = self.match_view(arr);
                if matches!(arr.ty.strip(), Ty::Map(..) | Ty::Set(_)) {
                    return self.lower_for_of_collection(f, pat, arr, mode, out);
                }
                if mode.is_empty() && self.is_iterable(&arr.ty) {
                    return self.lower_for_of_iterable(f, pat, arr, out);
                }
                let arr = if matches!(arr.ty.strip(), Ty::Arr(_) | Ty::Typed(_)) || mode.is_empty()
                {
                    arr
                } else {
                    // Not an array (say, a user class with an `entries` method): iterate as written.
                    mode = "";
                    self.lower_expr(&f.right, None)?
                };
                let it = self.begin_iter(arr)?;
                self.cur.scopes.push(HashMap::new());
                // A captured loop variable gets a fresh cell every iteration.
                let mut bind = Vec::new();
                match (mode, pat) {
                    ("entries", BindingPattern::ArrayPattern(ap)) if ap.elements.len() == 2 => {
                        if let Some(p) = &ap.elements[0] {
                            self.bind_param(p, it.idx, &mut bind)?;
                        }
                        if let Some(p) = &ap.elements[1] {
                            self.bind_param(p, it.elem, &mut bind)?;
                        }
                    }
                    ("entries", _) => bail!(
                        "{}: `for (const [i, x] of xs.entries())` is the supported form",
                        self.loc(f.span.start)
                    ),
                    ("keys", p) => self.bind_param(p, it.idx, &mut bind)?,
                    (_, p) => self.bind_param(p, it.elem, &mut bind)?,
                }
                let entry = self.loop_entry(it.label, Some(it.label));
                self.cur.loops.push(entry);
                let body = self.scoped(&f.body);
                self.cur.loops.pop();
                self.cur.scopes.pop();
                out.extend(it.pre);
                let mut b = it.head;
                b.extend(bind);
                b.extend(body?);
                out.push(Stmt::Loop(it.label, b));
            }
            Statement::ForInStatement(f) => self.lower_for_in(f, out)?,
            Statement::ReturnStatement(r) => {
                let v = match &r.argument {
                    Some(e) => {
                        // Inside an inlined callback, its result's type (`(c): Align => ..`), not
                        // the enclosing function's.
                        let exp = match self.cur.rets.last() {
                            Some(rt) => rt.var.and_then(|v| {
                                let t = self.cur.locals[v as usize].clone();
                                (t != Ty::Void && matches!(rt.kind, RetKind::Val)).then_some(t)
                            }),
                            None => self.cur.ret.clone(),
                        };
                        Some(self.lower_expr(e, exp.as_ref())?)
                    }
                    None => None,
                };
                self.emit_return(v, out)?;
            }
            Statement::BreakStatement(b) => {
                let l = self.jump_target(b.label.as_ref().map(|l| l.name.as_str()))?;
                self.emit_jump(Exit::Break(l), out)?;
            }
            Statement::ContinueStatement(c) => {
                let name = c.label.as_ref().map(|l| l.name.as_str());
                let (l, is_loop) = self.continue_target(name)?;
                self.emit_jump(Exit::Continue(l, is_loop), out)?;
            }
            Statement::DoWhileStatement(d) => {
                // Loop { Block(cont) { body }; if !test break }: `continue` leaves the block and
                // reaches the test.
                let (top, cont) = (self.new_label(), self.new_label());
                let entry = self.loop_entry(top, Some(cont));
                self.cur.loops.push(entry);
                let body = self.scoped(&d.body);
                self.cur.loops.pop();
                let c = self.lower_cond(&d.test)?;
                let not = mk(ExprKind::Un(UnOp::Not, Box::new(c)), Ty::Bool);
                let b = vec![
                    Stmt::Block(cont, body?),
                    Stmt::If(not, vec![Stmt::Break(top)], vec![]),
                ];
                out.push(Stmt::Loop(top, b));
            }
            Statement::ForStatement(f) => self.lower_for(f, out)?,
            Statement::SwitchStatement(sw) => self.lower_switch(sw, out)?,
            Statement::LabeledStatement(l) => {
                let name = l.label.name.to_string();
                if matches!(
                    l.body,
                    Statement::ForStatement(_)
                        | Statement::WhileStatement(_)
                        | Statement::DoWhileStatement(_)
                        | Statement::ForOfStatement(_)
                ) {
                    self.cur.pending_label = Some(name);
                    self.lower_stmt(&l.body, out)?;
                } else {
                    // `name: { ... break name; ... }`
                    let b = self.new_label();
                    self.cur.loops.push(LoopLabels {
                        name: Some(name),
                        brk: b,
                        cont: None,
                        unlabeled_break: false,
                    });
                    let body = self.scoped(&l.body);
                    self.cur.loops.pop();
                    out.push(Stmt::Block(b, body?));
                }
            }
            Statement::ThrowStatement(t) => {
                let v = self.lower_expr(&t.argument, None)?;
                let at = self.loc(t.span.start);
                let Ty::Obj(sid) = v.ty.strip().clone() else {
                    bail!("{at}: `throw` supports Error objects only, found {}", v.ty)
                };
                let Some(error) = self
                    .classes
                    .contains_key("Error")
                    .then(|| self.classes_sid("Error"))
                else {
                    bail!("{at}: `throw` needs the Error classes")
                };
                if !self.is_subclass(sid, error) {
                    bail!("{at}: `throw` supports Error objects only (a class extending Error)")
                }
                out.push(Stmt::Throw(v));
            }
            Statement::TryStatement(t) => self.lower_try(t, out)?,
            // Alone (`if (x) function f() {}`): bound where it stands.
            Statement::FunctionDeclaration(f) if f.body.is_some() => {
                self.lower_fn_decl_here(f, out)?
            }
            other => bail!(
                "{}: unsupported statement",
                self.loc(oxc_span::GetSpan::span(other).start)
            ),
        }
        Ok(())
    }

    /// `try { .. } catch (e) { .. }`. The catch variable is an `Error`; `e instanceof RangeError`
    /// narrows it as for any class.
    fn lower_try(&mut self, t: &'a TryStatement<'a>, out: &mut Vec<Stmt>) -> R<()> {
        let Some(fin) = &t.finalizer else {
            return self.lower_try_catch(t, out);
        };
        // try { B } catch (e) { H } finally { F }
        //   kind = 0
        //   Block done { try { <B and H; a return/break/continue sets kind and leaves to done>
        //                      break done } catch (x) { F; throw x } }
        //   F
        //   if kind == 1 { <the first recorded exit> } if kind == 2 { ... }
        let done = self.new_label();
        let kind = self.new_local(Ty::Num);
        out.push(Stmt::Assign(kind, super::expr::num(0.0)));
        self.cur.fins.push(FinCtx {
            done,
            kind,
            loops_depth: self.cur.loops.len(),
            rets_depth: self.cur.rets.len(),
            ret_var: None,
            exits: Vec::new(),
        });
        let mut inner = Vec::new();
        let r = self.lower_try_catch(t, &mut inner);
        let fc = self.cur.fins.pop().expect("pushed above");
        r?;
        inner.push(Stmt::Break(done));
        let error = self.classes_sid("Error");
        let x = self.new_local(Ty::Obj(error));
        let mut on_throw = Vec::new();
        self.lower_block(&fin.body, &mut on_throw)?;
        on_throw.push(Stmt::Throw(self.var_expr(x)));
        out.push(Stmt::Block(done, vec![Stmt::Try(inner, Some(x), on_throw)]));
        self.lower_block(&fin.body, out)?;
        for (i, exit) in fc.exits.iter().enumerate() {
            let is_it = mk(
                ExprKind::Cmp(
                    CmpOp::Eq,
                    Box::new(self.var_expr(kind)),
                    Box::new(super::expr::num((i + 1) as f64)),
                ),
                Ty::Bool,
            );
            let mut take = Vec::new();
            match exit {
                Exit::Return => {
                    let v = fc.ret_var.map(|v| self.var_expr(v));
                    self.emit_return(v, &mut take)?;
                }
                other => self.emit_jump(other.clone(), &mut take)?,
            }
            out.push(Stmt::If(is_it, take, vec![]));
        }
        Ok(())
    }

    /// `for (const t of [a, b, c]) body` where the elements have unrelated types (`Tween<number>`,
    /// `Tween<Color>`): the body once per element, each with its own type. `Ok(false)` when the
    /// elements share a type (the plain loop handles them).
    fn lower_for_unrolled(
        &mut self,
        f: &'a ForOfStatement<'a>,
        pat: &'a BindingPattern<'a>,
        lit: &'a ArrayExpression<'a>,
        out: &mut Vec<Stmt>,
    ) -> R<bool> {
        let mut vals = Vec::new();
        for item in &lit.elements {
            let Some(e) = item.as_expression() else {
                return Ok(false);
            };
            match self.lower_expr(e, None) {
                Ok(v) => vals.push(v),
                Err(_) => return Ok(false),
            }
        }
        let mixed = vals
            .iter()
            .skip(1)
            .any(|v| v.ty.strip() != vals[0].ty.strip() && self.unify(&vals[0].ty, &v.ty).is_err());
        if !mixed {
            return Ok(false);
        }
        let name = self.cur.pending_label.take();
        let brk = self.new_label();
        let mut tmps = Vec::new();
        let mut pre = Vec::new();
        for v in vals {
            let t = self.new_local(v.ty.clone());
            pre.push(Stmt::Assign(t, v));
            tmps.push(t);
        }
        let mut blocks = Vec::new();
        for t in tmps {
            let cont = self.new_label();
            self.cur.scopes.push(HashMap::new());
            let mut bind = Vec::new();
            self.bind_param(pat, t, &mut bind)?;
            self.cur.loops.push(LoopLabels {
                name: name.clone(),
                brk,
                cont: Some(cont),
                unlabeled_break: true,
            });
            let body = self.scoped(&f.body);
            self.cur.loops.pop();
            self.cur.scopes.pop();
            bind.extend(body?);
            blocks.push(Stmt::Block(cont, bind));
        }
        pre.extend(blocks);
        out.push(Stmt::Block(brk, pre));
        Ok(true)
    }

    /// `for (const [k, v] of Object.entries(o)) body`: a loop over the keys with the value read
    /// from the array of values.
    fn lower_for_object_entries(
        &mut self,
        f: &'a ForOfStatement<'a>,
        call: &'a CallExpression<'a>,
        pat: &'a BindingPattern<'a>,
        out: &mut Vec<Stmt>,
    ) -> R<()> {
        let BindingPattern::ArrayPattern(ap) = pat else {
            bail!(
                "{}: `for (const [k, v] of Object.entries(o))` is the supported form",
                self.loc(f.span.start)
            )
        };
        let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) else {
            bail!("Object.entries: missing argument")
        };
        let obj = self.lower_expr(arg, None)?;
        let (keys, vals, pre) = self.object_parts(obj, "Object.entries", true)?;
        let vty = match vals.ty.strip() {
            Ty::Arr(e) => (**e).clone(),
            _ => Ty::Num,
        };
        let vals_var = self.new_local(vals.ty.clone());
        let it = self.begin_iter(keys)?;
        self.cur.scopes.push(HashMap::new());
        let mut bind = Vec::new();
        if ap.elements.len() > 2 || ap.rest.is_some() {
            bail!("an Object.entries pattern has two parts: [key, value]")
        }
        if let Some(Some(p)) = ap.elements.first() {
            self.bind_param(p, it.elem, &mut bind)?;
        }
        if let Some(Some(p)) = ap.elements.get(1) {
            let raw_ty = if vty.is_ptr() || vty == Ty::Num {
                vty.clone().nullable()
            } else {
                vty.clone()
            };
            let raw = mk(
                ExprKind::Index(
                    Box::new(self.var_expr(vals_var)),
                    Box::new(self.var_expr(it.idx)),
                ),
                raw_ty.clone(),
            );
            let got = if raw_ty == vty {
                raw
            } else {
                mk(ExprKind::Cast(Box::new(raw)), vty.clone())
            };
            let v = self.new_local(vty.clone());
            bind.push(Stmt::Assign(v, got));
            self.bind_param(p, v, &mut bind)?;
        }
        let entry = self.loop_entry(it.label, Some(it.label));
        self.cur.loops.push(entry);
        let body = self.scoped(&f.body);
        self.cur.loops.pop();
        self.cur.scopes.pop();
        out.extend(pre);
        out.push(Stmt::Assign(vals_var, vals));
        out.extend(it.pre);
        let mut b = it.head;
        b.extend(bind);
        b.extend(body?);
        out.push(Stmt::Loop(it.label, b));
        Ok(())
    }

    /// A block's statements in their own scope.
    fn lower_block(&mut self, b: &'a [Statement<'a>], out: &mut Vec<Stmt>) -> R<()> {
        self.cur.scopes.push(HashMap::new());
        let r = self.lower_stmts(b, out);
        self.cur.scopes.pop();
        r
    }

    /// Does leaving by `exit` pass through the innermost open `finally`?
    pub(crate) fn fin_applies(&self, exit: &Exit) -> bool {
        let Some(fin) = self.cur.fins.last() else {
            return false;
        };
        match exit {
            Exit::Return => fin.rets_depth == self.cur.rets.len(),
            Exit::Break(l) | Exit::Continue(l, _) => {
                let target = self
                    .cur
                    .loops
                    .iter()
                    .rposition(|e| e.brk == *l || e.cont == Some(*l));
                matches!(target, Some(ti) if fin.loops_depth > ti)
            }
        }
    }

    /// Records the exit and leaves the `try` for its `finally`.
    pub(crate) fn leave_through_finally(
        &mut self,
        exit: Exit,
        val: Option<Expr>,
        out: &mut Vec<Stmt>,
    ) -> R<()> {
        let fi = self.cur.fins.len() - 1;
        if let (Exit::Return, Some(v)) = (&exit, val) {
            let v = match (self.cur.fins[fi].ret_var, self.cur.ret.clone()) {
                (Some(rv), _) => {
                    let t = self.cur.locals[rv as usize].clone();
                    self.coerce(v, &t)?
                }
                (None, Some(t)) => self.coerce(v, &t)?,
                (None, None) => v,
            };
            let rv = match self.cur.fins[fi].ret_var {
                Some(rv) => rv,
                None => {
                    let rv = self.new_local(v.ty.clone());
                    self.cur.fins[fi].ret_var = Some(rv);
                    rv
                }
            };
            out.push(Stmt::Assign(rv, v));
        }
        let fin = &mut self.cur.fins[fi];
        let code = match fin.exits.iter().position(|e| *e == exit) {
            Some(i) => i + 1,
            None => {
                fin.exits.push(exit);
                fin.exits.len()
            }
        };
        let (kind, done) = (fin.kind, fin.done);
        out.push(Stmt::Assign(kind, super::expr::num(code as f64)));
        out.push(Stmt::Break(done));
        Ok(())
    }

    /// `break` / `continue`, through the `finally`s it leaves.
    pub(crate) fn emit_jump(&mut self, exit: Exit, out: &mut Vec<Stmt>) -> R<()> {
        if self.fin_applies(&exit) {
            return self.leave_through_finally(exit, None, out);
        }
        out.push(match exit {
            Exit::Break(l) => Stmt::Break(l),
            Exit::Continue(l, true) => Stmt::Continue(l),
            Exit::Continue(l, false) => Stmt::Break(l),
            Exit::Return => unreachable!("returns go through emit_return"),
        });
        Ok(())
    }

    /// `try { .. } catch (e) { .. }` (or a plain `try { .. }` block when only a `finally` follows).
    fn lower_try_catch(&mut self, t: &'a TryStatement<'a>, out: &mut Vec<Stmt>) -> R<()> {
        let Some(h) = &t.handler else {
            self.cur.scopes.push(HashMap::new());
            let r = self.lower_stmts(&t.block.body, out);
            self.cur.scopes.pop();
            return r;
        };
        self.cur.scopes.push(HashMap::new());
        let mut body = Vec::new();
        let r = self.lower_stmts(&t.block.body, &mut body);
        self.cur.scopes.pop();
        r?;
        let error = self.classes_sid("Error");
        self.cur.scopes.push(HashMap::new());
        let mut handler = Vec::new();
        let r = (|| -> R<Option<VarId>> {
            let var = match &h.param {
                Some(p) => match &p.pattern {
                    BindingPattern::BindingIdentifier(id) => {
                        let v = self.new_local(Ty::Obj(error));
                        self.bind_var(id.name.as_str(), v, &mut handler);
                        Some(v)
                    }
                    _ => bail!("{}: destructuring in `catch`", self.loc(h.span.start)),
                },
                None => None,
            };
            self.lower_stmts(&h.body.body, &mut handler)?;
            Ok(var)
        })();
        self.cur.scopes.pop();
        out.push(Stmt::Try(body, r?, handler));
        Ok(())
    }

    /// `switch`: find the first matching clause (or `default`), then run the clause bodies from
    /// there on, in source order, until a `break` leaves the block (that is JS's fall-through).
    ///
    /// ```text
    /// tmp = discriminant; idx = (index of default, or the clause count)
    /// Block sel { if tmp === c0 { idx = 0; break sel }  if tmp === c1 { idx = 1; break sel } ... }
    /// Block brk { if idx <= 0 { body0 }  if idx <= 1 { body1 } ... }
    /// ```
    fn lower_switch(&mut self, sw: &'a SwitchStatement<'a>, out: &mut Vec<Stmt>) -> R<()> {
        let d = self.lower_expr(&sw.discriminant, None)?;
        let tmp = self.new_local(d.ty.clone());
        out.push(Stmt::Assign(tmp, d.clone()));
        let n = sw.cases.len();
        let default = sw.cases.iter().position(|c| c.test.is_none()).unwrap_or(n);
        let idx = self.new_local(Ty::Num);
        out.push(Stmt::Assign(idx, super::expr::num(default as f64)));
        let (sel, brk) = (self.new_label(), self.new_label());
        let mut select = Vec::new();
        for (k, c) in sw.cases.iter().enumerate() {
            let Some(test) = &c.test else { continue };
            let want = d.ty.strip().clone();
            let t = self.lower_expr(test, Some(&want))?;
            let eq = self.lower_compare(CmpOp::Eq, self.var_expr(tmp), t, c.span.start)?;
            select.push(Stmt::If(
                eq,
                vec![
                    Stmt::Assign(idx, super::expr::num(k as f64)),
                    Stmt::Break(sel),
                ],
                vec![],
            ));
        }
        out.push(Stmt::Block(sel, select));
        self.cur.scopes.push(HashMap::new());
        self.cur.loops.push(LoopLabels {
            name: self.cur.pending_label.take(),
            brk,
            cont: None,
            unlabeled_break: true,
        });
        let mut bodies = Vec::new();
        let mut failed = None;
        for (k, c) in sw.cases.iter().enumerate() {
            let mut body = Vec::new();
            // `switch (b.kind)` over a discriminated union: a case's body sees the variants of
            // its tag and of the empty cases right before it (`case "a": case "b":`).
            let narrowed = self.case_narrowings(sw, k);
            let r = self.with_narrowed(narrowed, |s| s.lower_stmts(&c.consequent, &mut body));
            if let Err(e) = r {
                failed = Some(e);
                break;
            }
            let le = mk(
                ExprKind::Cmp(
                    CmpOp::Le,
                    Box::new(self.var_expr(idx)),
                    Box::new(super::expr::num(k as f64)),
                ),
                Ty::Bool,
            );
            bodies.push(Stmt::If(le, body, vec![]));
        }
        self.cur.loops.pop();
        self.cur.scopes.pop();
        if let Some(e) = failed {
            return Err(e);
        }
        out.push(Stmt::Block(brk, bodies));
        Ok(())
    }

    /// What reaching case `k` of `sw` says about a discriminated union `sw` tests the tag of.
    fn case_narrowings(
        &mut self,
        sw: &'a SwitchStatement<'a>,
        k: usize,
    ) -> Vec<(String, Vec<usize>)> {
        let Expression::StaticMemberExpression(m) = sw.discriminant.get_inner_expression() else {
            return vec![];
        };
        let mut lits = Vec::new();
        let mut j = k;
        loop {
            let c = &sw.cases[j];
            match &c.test {
                Some(Expression::StringLiteral(s)) => lits.push(s.value.to_string()),
                // The default case, or a test that is not a literal: no narrowing.
                _ => return vec![],
            }
            if j == 0 {
                break;
            }
            let prev = &sw.cases[j - 1];
            if prev.consequent.is_empty() {
                j -= 1;
                continue;
            }
            // A body before that can fall through lets its variants in too.
            let ends = matches!(
                prev.consequent.last(),
                Some(
                    Statement::BreakStatement(_)
                        | Statement::ReturnStatement(_)
                        | Statement::ThrowStatement(_)
                        | Statement::ContinueStatement(_)
                )
            );
            if !ends {
                return vec![];
            }
            break;
        }
        self.disc_narrowings(&m.object, m.property.name.as_str(), &lits, true)
    }

    /// The entry for a loop being lowered; it takes a pending source label.
    pub(crate) fn loop_entry(&mut self, brk: LabelId, cont: Option<LabelId>) -> LoopLabels {
        LoopLabels {
            name: self.cur.pending_label.take(),
            brk,
            cont,
            unlabeled_break: true,
        }
    }

    /// Where `break` (or `break name`) goes.
    fn jump_target(&self, name: Option<&str>) -> R<LabelId> {
        for e in self.cur.loops.iter().rev() {
            match name {
                Some(n) if e.name.as_deref() == Some(n) => return Ok(e.brk),
                None if e.unlabeled_break => return Ok(e.brk),
                _ => {}
            }
        }
        match name {
            Some(n) => bail!("break to unknown label `{n}`"),
            None => bail!("break outside a loop or switch"),
        }
    }

    /// Where `continue` (or `continue name`) goes: the label, and whether it is a loop top
    /// (`Continue`) or the end of a body block (`Break`).
    fn continue_target(&self, name: Option<&str>) -> R<(LabelId, bool)> {
        for e in self.cur.loops.iter().rev() {
            let Some(c) = e.cont else { continue };
            if name.is_none() || e.name.as_deref() == name {
                return Ok((c, c == e.brk));
            }
        }
        match name {
            Some(n) => bail!("continue to `{n}`, which is not a loop"),
            None => bail!("continue outside a loop"),
        }
    }

    /// `for (init; test; update) body`:
    /// `init; Loop { if !test break; Block(cont) { body }; update }`, `continue` leaving the block.
    fn lower_for(&mut self, f: &'a ForStatement<'a>, out: &mut Vec<Stmt>) -> R<()> {
        let name = self.cur.pending_label.take();
        self.cur.scopes.push(HashMap::new());
        let r = self.lower_for_in_scope(f, name, out);
        self.cur.scopes.pop();
        r
    }

    fn lower_for_in_scope(
        &mut self,
        f: &'a ForStatement<'a>,
        name: Option<String>,
        out: &mut Vec<Stmt>,
    ) -> R<()> {
        match &f.init {
            Some(ForStatementInit::VariableDeclaration(v)) => self.lower_var_decl(v, out)?,
            Some(init) => {
                let e = init.as_expression().ok_or("for initialiser")?;
                self.lower_effect(e, out)?;
            }
            None => {}
        }
        // Captured `let` variables of the head are copied into a fresh cell every iteration.
        let scope = self.cur.scopes.len() - 1;
        let fresh: Vec<(VarId, Ty)> = self.cur.scopes[scope]
            .values()
            .filter_map(|b| match b {
                Binding::Boxed(cell, ty) => Some((*cell, ty.clone())),
                _ => None,
            })
            .collect();
        let (top, cont) = (self.new_label(), self.new_label());
        let mut b = Vec::new();
        if let Some(t) = &f.test {
            let c = self.lower_cond(t)?;
            let not = mk(ExprKind::Un(UnOp::Not, Box::new(c)), Ty::Bool);
            b.push(Stmt::If(not, vec![Stmt::Break(top)], vec![]));
        }
        self.cur.pending_label = name;
        let entry = self.loop_entry(top, Some(cont));
        self.cur.loops.push(entry);
        let body = self.scoped(&f.body);
        self.cur.loops.pop();
        b.push(Stmt::Block(cont, body?));
        for (cell, ty) in fresh {
            let cell_ty = self.cur.locals[cell as usize].clone();
            let old = self.var_expr(cell);
            let value = mk(ExprKind::CellGet(Box::new(old)), ty);
            b.push(Stmt::Assign(
                cell,
                mk(ExprKind::NewCell(Box::new(value)), cell_ty),
            ));
        }
        if let Some(u) = &f.update {
            self.lower_effect(u, &mut b)?;
        }
        out.push(Stmt::Loop(top, b));
        Ok(())
    }

    /// `const { a, b: [c] } = value;`
    fn lower_destructuring(&mut self, d: &'a VariableDeclarator<'a>, out: &mut Vec<Stmt>) -> R<()> {
        let Some(init) = &d.init else {
            bail!("{}: destructuring needs a value", self.loc(d.span.start))
        };
        let ann = match &d.type_annotation {
            Some(a) => Some(self.ty(&a.type_annotation, "pattern")?),
            None => None,
        };
        let mut v = self.lower_expr(init, ann.as_ref())?;
        if let Some(t) = &ann {
            v = self.coerce(v, t)?;
        }
        self.bind_pattern(&d.id, v, out)
    }

    /// Names a parameter (or loop variable) that is already in local `var`: a plain name, or a
    /// pattern taken apart in `out`.
    pub(crate) fn bind_param(
        &mut self,
        pat: &'a BindingPattern<'a>,
        var: VarId,
        out: &mut Vec<Stmt>,
    ) -> R<()> {
        match pat {
            BindingPattern::BindingIdentifier(b) => {
                self.bind_var(b.name.as_str(), var, out);
                Ok(())
            }
            p => {
                let v = self.var_expr(var);
                self.bind_pattern(p, v, out)
            }
        }
    }

    /// Binds the names of a pattern to the parts of `value`: `{ a, b: { c } }`, `[x, y]`, and
    /// `name = default` anywhere (the default replaces a null).
    pub(crate) fn bind_pattern(
        &mut self,
        pat: &'a BindingPattern<'a>,
        value: Expr,
        out: &mut Vec<Stmt>,
    ) -> R<()> {
        match pat {
            BindingPattern::BindingIdentifier(id) => {
                let var = self.new_local(value.ty.clone());
                out.push(Stmt::Assign(var, value));
                self.bind_var(id.name.as_str(), var, out);
            }
            BindingPattern::AssignmentPattern(a) => {
                if !value.ty.is_nullable() {
                    // Never null: the default cannot be used.
                    return self.bind_pattern(&a.left, value, out);
                }
                // `anchor = null`: a missing value is null either way; the binding stays nullable.
                if matches!(&a.right, Expression::NullLiteral(_))
                    || matches!(&a.right, Expression::Identifier(i) if i.name == "undefined")
                {
                    return self.bind_pattern(&a.left, value, out);
                }
                let base = value.ty.strip().clone();
                let tmp = self.new_local(value.ty.clone());
                out.push(Stmt::Assign(tmp, value));
                let t = self.var_expr(tmp);
                let d = self.lower_expr(&a.right, Some(&base))?;
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
                self.bind_pattern(&a.left, picked, out)?;
            }
            BindingPattern::ObjectPattern(o) => {
                let tmp = self.new_local(value.ty.clone());
                out.push(Stmt::Assign(tmp, value));
                let mut taken = Vec::new();
                for p in &o.properties {
                    if p.computed {
                        bail!("computed key in an object pattern")
                    }
                    let key = self.key_name(&p.key)?;
                    let part = self.member_of(self.var_expr(tmp), &key)?;
                    taken.push(key);
                    self.bind_pattern(&p.value, part, out)?;
                }
                // `{ a, ...rest }`: the other fields, as a new object.
                if let Some(r) = &o.rest {
                    let rest = self.object_rest(&self.var_expr(tmp), &taken)?;
                    self.bind_pattern(&r.argument, rest, out)?;
                }
            }
            BindingPattern::ArrayPattern(a) if matches!(value.ty.strip(), Ty::Obj(sid) if self.is_tuple(*sid)) =>
            {
                let Ty::Obj(sid) = value.ty.strip().clone() else {
                    unreachable!("checked")
                };
                let n = self.m.structs[sid].fields.len();
                if a.elements.len() > n || (a.rest.is_none() && a.elements.len() > n) {
                    bail!("a tuple pattern names at most its {n} elements")
                }
                let tmp = self.new_local(value.ty.clone());
                out.push(Stmt::Assign(tmp, value));
                for (i, p) in a.elements.iter().enumerate() {
                    let Some(p) = p else { continue };
                    let ty = self.m.structs[sid].fields[i].ty.clone();
                    let part = mk(ExprKind::Field(Box::new(self.var_expr(tmp)), sid, i), ty);
                    self.bind_pattern(p, part, out)?;
                }
                // `[first, ...rest]`: the other elements, as a tuple (or an array when they agree).
                if let Some(r) = &a.rest {
                    let vals: Vec<Expr> = (a.elements.len()..n)
                        .map(|i| {
                            let ty = self.m.structs[sid].fields[i].ty.clone();
                            mk(ExprKind::Field(Box::new(self.var_expr(tmp)), sid, i), ty)
                        })
                        .collect();
                    let rest = self.tuple_or_array(vals)?;
                    self.bind_pattern(&r.argument, rest, out)?;
                }
            }
            BindingPattern::ArrayPattern(a) => {
                let value = self.match_view(value);
                let Ty::Arr(el) = value.ty.strip().clone() else {
                    bail!("cannot destructure {} as an array", value.ty)
                };
                let el = *el;
                let tmp = self.new_local(value.ty.clone());
                out.push(Stmt::Assign(tmp, value));
                for (i, p) in a.elements.iter().enumerate() {
                    let Some(p) = p else { continue };
                    // Past the end an element reads as null (a zero pointer, or the null NaN).
                    let read_ty = if el.is_ptr() || el == Ty::Num {
                        el.clone().nullable()
                    } else {
                        el.clone()
                    };
                    let read = mk(
                        ExprKind::Index(
                            Box::new(self.var_expr(tmp)),
                            Box::new(super::expr::num(i as f64)),
                        ),
                        read_ty.clone(),
                    );
                    let part = if matches!(p, BindingPattern::AssignmentPattern(_)) || read_ty == el
                    {
                        read
                    } else {
                        mk(ExprKind::Cast(Box::new(read)), el.clone())
                    };
                    self.bind_pattern(p, part, out)?;
                }
                // `[a, ...rest]`: the others, as a new array.
                if let Some(r) = &a.rest {
                    let rest = mk(
                        ExprKind::Rt(
                            RtFn::ArrSlice,
                            vec![
                                self.var_expr(tmp),
                                super::expr::num(a.elements.len() as f64),
                                super::expr::num(f64::INFINITY),
                            ],
                        ),
                        self.var_expr(tmp).ty,
                    );
                    self.bind_pattern(&r.argument, rest, out)?;
                }
            }
        }
        Ok(())
    }

    fn lower_var_decl(&mut self, v: &'a VariableDeclaration<'a>, out: &mut Vec<Stmt>) -> R<()> {
        let v_kind = v.kind;
        for d in &v.declarations {
            let BindingPattern::BindingIdentifier(id) = &d.id else {
                self.lower_destructuring(d, out)?;
                continue;
            };
            // `const more = <T extends Float32Array>(a: T): T => ..` inside a function: a generic
            // function of its own (it cannot use the locals around it).
            if let (Some(Expression::ArrowFunctionExpression(a)), None, true) = (
                &d.init,
                &d.type_annotation,
                v_kind == VariableDeclarationKind::Const,
            ) && a.type_parameters.is_some()
            {
                let body = match &a.body {
                    ArrowFunctionBody::FunctionBody(b) => FnBody::Block(b),
                    other => FnBody::Expr(other.as_expression().ok_or("arrow body")?),
                };
                let key = format!("{}@{}", id.name, a.span.start);
                self.generic_fns.insert(
                    key.clone(),
                    generics::GenericFn {
                        tparams: generics::tparam_list(a.type_parameters.as_deref()),
                        params: &a.params,
                        ret: a.return_type.as_ref().map(|r| &r.type_annotation),
                        body,
                    },
                );
                self.cur.local_generics.insert(id.name.to_string(), key);
                continue;
            }
            let ann = match &d.type_annotation {
                Some(a) => Some(self.ty(&a.type_annotation, id.name.as_str())?),
                None => None,
            };
            // `const loop = () => { ..; setTimeout(loop, 16) }`: no annotation, but the arrow's
            // parameters are annotated and it returns nothing, so its type is known up front.
            let ann = match (&ann, &d.init) {
                (None, Some(Expression::ArrowFunctionExpression(a)))
                    if self.captured.contains(id.name.as_str()) =>
                {
                    self.self_ref_arrow_ty(a)?
                }
                _ => ann,
            };
            // A closure that refers to its own name: the cell exists before the closure (made at
            // the top of the block by `prebind_arrows`, or here).
            let mut prebound = None;
            let (ann, early) = match self.cur.prebound.remove(id.name.as_str()) {
                Some((cell, t)) => (Some(t), Some(cell)),
                None => (ann, None),
            };
            if let Some(cell) = early {
                prebound = Some(cell);
            } else if self.captured.contains(id.name.as_str())
                && let Some(t @ Ty::Fn(_)) = &ann
            {
                let cell = self.new_local(Ty::Cell(Box::new(t.clone())));
                let cell_ty = self.cur.locals[cell as usize].clone();
                let zero = self.default_value(t);
                let new = Expr {
                    kind: ExprKind::NewCell(Box::new(zero)),
                    ty: cell_ty,
                };
                out.push(Stmt::Assign(cell, new));
                let scope = self.cur.scopes.len() - 1;
                self.cur.scopes[scope].insert(id.name.to_string(), Binding::Boxed(cell, t.clone()));
                prebound = Some(cell);
            }
            let init = match &d.init {
                Some(e) => {
                    if let Some(a) = &d.type_annotation {
                        self.check_lit_member(&a.type_annotation, e)?;
                    }
                    let v = self.lower_expr(e, ann.as_ref())?;
                    Some(match &ann {
                        Some(t) => self.coerce(v, t)?,
                        // `let s = "x"` is a `string` (a `const` keeps its literal type).
                        None if v.ty == Ty::Lit && v_kind != VariableDeclarationKind::Const => {
                            self.widen_lit(v)
                        }
                        None => v,
                    })
                }
                None => None,
            };
            let ty = match (&ann, &init) {
                (Some(t), _) => t.clone(),
                (None, Some(v)) if v.ty != Ty::Null => v.ty.clone(),
                _ => bail!(
                    "{}: cannot infer the type of `{}`",
                    self.loc(d.span.start),
                    id.name
                ),
            };
            let v = match init {
                Some(v) => v,
                None => self.default_value(&ty),
            };
            if let Some(cell) = prebound {
                let c = self.var_expr(cell);
                out.push(Stmt::CellSet(c, v));
                continue;
            }
            // `const level = e.level === "debug" ? "info" : e.level`: the names it can hold, so
            // `filter[level]` reads only the fields named so.
            if ty == Ty::Lit
                && ann.is_none()
                && let Some(init) = &d.init
                && let Some(ms) = self.expr_lits(init)
            {
                self.cur.lit_sets.insert(id.name.to_string(), ms);
            }
            let var = self.new_local(ty.clone());
            out.push(Stmt::Assign(var, v));
            self.note_regex(id.name.as_str(), d.init.as_ref());
            if v_kind == VariableDeclarationKind::Const
                && !self.captured.contains(id.name.as_str())
                && let Some(Expression::NumericLiteral(n)) = &d.init
            {
                self.cur.const_nums.insert(var, n.value);
            }
            self.bind_var(id.name.as_str(), var, out);
        }
        Ok(())
    }

    /// The string literals an expression of a literal-union type can be: a literal, either arm of
    /// a conditional, a name with a known set, or a field an interface declares as a union.
    pub(crate) fn expr_lits(&mut self, e: &'a Expression<'a>) -> Option<Vec<String>> {
        match e.get_inner_expression() {
            Expression::StringLiteral(s) => Some(vec![s.value.to_string()]),
            Expression::ConditionalExpression(c) => {
                let mut a = self.expr_lits(&c.consequent)?;
                for m in self.expr_lits(&c.alternate)? {
                    if !a.contains(&m) {
                        a.push(m);
                    }
                }
                Some(a)
            }
            Expression::Identifier(id) => self.cur.lit_sets.get(id.name.as_str()).cloned(),
            Expression::StaticMemberExpression(m) => {
                let mark = self.cur.locals.len();
                let obj = self.lower_expr(&m.object, None);
                self.cur.locals.truncate(mark);
                let Ty::Obj(sid) = obj.ok()?.ty.strip().clone() else {
                    return None;
                };
                let iface = self
                    .ifaces
                    .get(self.m.structs[sid].name.as_str())
                    .copied()?;
                iface.body.body.iter().find_map(|s| match s {
                    TSSignature::TSPropertySignature(p)
                        if p.key
                            .static_name()
                            .is_some_and(|n| n == m.property.name.as_str()) =>
                    {
                        self.lit_members_opt(&p.type_annotation.as_ref()?.type_annotation)
                    }
                    _ => None,
                })
            }
            _ => None,
        }
    }

    /// The type of an arrow function a program names and calls from inside itself, when its
    /// parameters are annotated and it returns no value (or says what it returns).
    fn self_ref_arrow_ty(&mut self, a: &'a ArrowFunctionExpression<'a>) -> R<Option<Ty>> {
        let mut params = Vec::new();
        for p in &a.params.items {
            let Some(ann) = &p.type_annotation else {
                return Ok(None);
            };
            let ty = self.ty(&ann.type_annotation, "closure")?;
            params.push(if p.optional { ty.nullable() } else { ty });
        }
        if a.params.rest.is_some() {
            return Ok(None);
        }
        let ret = match &a.return_type {
            Some(r) => self.ty(&r.type_annotation, "closure")?,
            None => match &a.body {
                ArrowFunctionBody::FunctionBody(b) => {
                    let mut scan = ValueReturns(false);
                    oxc_ast_visit::Visit::visit_function_body(&mut scan, b);
                    if scan.0 {
                        return Ok(None);
                    }
                    Ty::Void
                }
                _ => return Ok(None),
            },
        };
        Ok(Some(Ty::Fn(Box::new(FnTy { params, ret }))))
    }

    /// An expression statement: assignments and updates need no value.
    fn lower_effect(&mut self, e: &'a Expression<'a>, out: &mut Vec<Stmt>) -> R<()> {
        match e {
            Expression::AssignmentExpression(a) => {
                let (stmts, _) = self.lower_assign(a)?;
                out.extend(stmts);
            }
            Expression::UpdateExpression(u) => {
                let (stmts, _) = self.lower_update(u)?;
                out.extend(stmts);
            }
            Expression::ParenthesizedExpression(p) => self.lower_effect(&p.expression, out)?,
            _ => {
                let v = self.lower_expr(e, None)?;
                out.push(Stmt::Eval(v));
            }
        }
        Ok(())
    }
}

/// Does a function body `return value;` (not counting the functions inside it)?
pub(crate) struct ValueReturns(pub bool);

/// Does this body `return value;` anywhere (not counting the functions inside it)?
pub(crate) fn returns_a_value(body: &FunctionBody<'_>) -> bool {
    let mut scan = ValueReturns(false);
    oxc_ast_visit::Visit::visit_function_body(&mut scan, body);
    scan.0
}

impl<'a> oxc_ast_visit::Visit<'a> for ValueReturns {
    fn visit_return_statement(&mut self, r: &ReturnStatement<'a>) {
        if r.argument.is_some() {
            self.0 = true;
        }
    }
    fn visit_function(&mut self, _: &Function<'a>, _: oxc_syntax::scope::ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

/// Does statement `s` declare (with `const`, `let`, `var`, at its own level) one of `names`?
fn declares_any(s: &Statement<'_>, names: &[String]) -> bool {
    struct Names<'n>(&'n [String], bool);
    impl<'a> oxc_ast_visit::Visit<'a> for Names<'_> {
        fn visit_binding_identifier(&mut self, b: &BindingIdentifier<'a>) {
            if self.0.iter().any(|n| n == b.name.as_str()) {
                self.1 = true;
            }
        }
        fn visit_expression(&mut self, _: &Expression<'a>) {}
    }
    let Statement::VariableDeclaration(d) = s else {
        return false;
    };
    let mut v = Names(names, false);
    for decl in &d.declarations {
        oxc_ast_visit::Visit::visit_binding_pattern(&mut v, &decl.id);
    }
    v.1
}
