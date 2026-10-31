// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Scalar replacement of value-class objects (`Vector2`, `Vector3`, `Quaternion`, `Color`, ...).
//!
//! The lowering makes every value-class result a heap object (`NewObj`, filled by the engine call
//! or the constructor) and reads it back field by field. When such an object never leaves its
//! function (it is only read with `.x`, written with `.x = ..`, built by `new`, or passed to the
//! engine as scalars), its fields can live in locals instead: no allocation, no frame slot for
//! the collector, and Cranelift keeps them in registers.
//!
//! A local is replaced when every use is one of: its own `NewObj`, a field read or write,
//! `CallExternInto` as the destination, or a copy from another replaced local or an expression
//! (copy-in: the fields are read once). Anything else (an argument, a return, a store into a
//! field or array, a closure capture) leaves it a heap object. Copies between locals are only
//! made scalar when neither is changed after it is built, because a heap copy would alias.

use crate::ir::*;
use std::collections::HashSet;

/// Replaces non-escaping value-class locals in every function of the module.
pub fn run(m: &mut Module) {
    let value: HashSet<StructId> = m.value_structs.iter().copied().collect();
    if value.is_empty() {
        return;
    }
    let counts: Vec<usize> = m.structs.iter().map(|s| s.fields.len()).collect();
    for f in &mut m.funcs {
        scalarise(f, &value, &counts);
    }
}

fn var_of(e: &Expr) -> Option<VarId> {
    match e.kind {
        ExprKind::Var(v) => Some(v),
        _ => None,
    }
}

fn num_var(v: VarId) -> Expr {
    Expr {
        kind: ExprKind::Var(v),
        ty: Ty::Num,
    }
}

/// Direct sub-expressions (not the statements of a `Seq`).
fn kids(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Bits(a)
        | ExprKind::Unbits(a)
        | ExprKind::TagStr(a)
        | ExprKind::StrTag(a)
        | ExprKind::Un(_, a)
        | ExprKind::IsNull(a)
        | ExprKind::Cast(a)
        | ExprKind::Truthy(a)
        | ExprKind::Field(a, ..)
        | ExprKind::Len(a)
        | ExprKind::NewCell(a)
        | ExprKind::CellGet(a)
        | ExprKind::InstanceOf(a, _) => vec![a],
        ExprKind::Bin(_, a, b)
        | ExprKind::Cmp(_, a, b)
        | ExprKind::And(a, b)
        | ExprKind::Or(a, b)
        | ExprKind::Index(a, b) => vec![a, b],
        ExprKind::Cond(a, b, c) => vec![a, b, c],
        ExprKind::Call(_, args)
        | ExprKind::CallVirtual(_, args)
        | ExprKind::CallExtern(_, args)
        | ExprKind::CallExternOut(_, args, _)
        | ExprKind::Rt(_, args) => args.iter().collect(),
        ExprKind::CallClosure(c, args) => std::iter::once(&**c).chain(args.iter()).collect(),
        ExprKind::CallExternInto(_, args, d) => args.iter().chain(std::iter::once(&**d)).collect(),
        ExprKind::Seq(_, v) => vec![v],
        ExprKind::MakeClosure(_, Some(env)) => vec![env],
        _ => vec![],
    }
}

fn kids_mut(e: &mut Expr) -> Vec<&mut Expr> {
    match &mut e.kind {
        ExprKind::Bits(a)
        | ExprKind::Unbits(a)
        | ExprKind::TagStr(a)
        | ExprKind::StrTag(a)
        | ExprKind::Un(_, a)
        | ExprKind::IsNull(a)
        | ExprKind::Cast(a)
        | ExprKind::Truthy(a)
        | ExprKind::Field(a, ..)
        | ExprKind::Len(a)
        | ExprKind::NewCell(a)
        | ExprKind::CellGet(a)
        | ExprKind::InstanceOf(a, _) => vec![a],
        ExprKind::Bin(_, a, b)
        | ExprKind::Cmp(_, a, b)
        | ExprKind::And(a, b)
        | ExprKind::Or(a, b)
        | ExprKind::Index(a, b) => vec![a, b],
        ExprKind::Cond(a, b, c) => vec![a, b, c],
        ExprKind::Call(_, args)
        | ExprKind::CallVirtual(_, args)
        | ExprKind::CallExtern(_, args)
        | ExprKind::CallExternOut(_, args, _)
        | ExprKind::Rt(_, args) => args.iter_mut().collect(),
        ExprKind::CallClosure(c, args) => {
            std::iter::once(&mut **c).chain(args.iter_mut()).collect()
        }
        ExprKind::CallExternInto(_, args, d) => {
            args.iter_mut().chain(std::iter::once(&mut **d)).collect()
        }
        ExprKind::Seq(_, v) => vec![v],
        ExprKind::MakeClosure(_, Some(env)) => vec![env],
        _ => vec![],
    }
}

struct Analysis<'a> {
    f: &'a Func,
    cand: Vec<bool>,
    ok: Vec<bool>,
    /// Changed after it was built (a field store or a second write).
    mutated: Vec<bool>,
    edges: Vec<(usize, usize)>,
}

impl Analysis<'_> {
    fn is_cand(&self, v: VarId) -> bool {
        self.cand[v as usize]
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Var(v) => {
                if self.is_cand(*v) {
                    self.ok[*v as usize] = false;
                }
            }
            ExprKind::Field(base, ..) => self.base(base),
            ExprKind::CallExternInto(_, args, dest) => {
                for a in args {
                    self.expr(a);
                }
                match var_of(dest) {
                    Some(d) if self.is_cand(d) => self.mutated[d as usize] = true,
                    _ => self.expr(dest),
                }
            }
            ExprKind::Seq(stmts, v) => {
                self.stmts(stmts);
                self.expr(v);
            }
            _ => {
                for k in kids(e) {
                    self.expr(k);
                }
            }
        }
    }

    /// The object of a field read: a replaced local (or a `Seq` that ends in one) is fine.
    fn base(&mut self, b: &Expr) {
        match &b.kind {
            ExprKind::Var(v) if self.is_cand(*v) => {}
            ExprKind::Seq(ss, last) if var_of(last).is_some_and(|d| self.is_cand(d)) => {
                self.stmts(ss)
            }
            _ => self.expr(b),
        }
    }

    fn stmts(&mut self, list: &[Stmt]) {
        // The local whose `NewObj` came last with only its own initialisation since.
        let mut fresh: Option<VarId> = None;
        for s in list {
            match s {
                Stmt::Assign(v, e) if self.is_cand(*v) => {
                    match &e.kind {
                        ExprKind::NewObj(sid) if self.f.locals[*v as usize] == Ty::Obj(*sid) => {
                            // A second definition is a change as far as copies go.
                            fresh = Some(*v);
                            continue;
                        }
                        ExprKind::Var(d) if self.is_cand(*d) => {
                            self.edges.push((*v as usize, *d as usize))
                        }
                        ExprKind::Seq(pre, last)
                            if var_of(last).is_some_and(|d| self.is_cand(d)) =>
                        {
                            self.stmts(pre);
                            self.edges
                                .push((*v as usize, var_of(last).unwrap() as usize));
                        }
                        _ => {
                            self.expr(e);
                            // Copy-in: modelled as an edge to nothing that is a candidate.
                            self.edges.push((*v as usize, usize::MAX));
                        }
                    }
                    fresh = None;
                }
                Stmt::StoreField(o, _, _, val) => {
                    self.expr(val);
                    match var_of(o) {
                        Some(d) if self.is_cand(d) => {
                            if fresh != Some(d) {
                                self.mutated[d as usize] = true;
                            }
                            continue;
                        }
                        _ => self.expr(o),
                    }
                    fresh = None;
                }
                Stmt::Eval(Expr {
                    kind: ExprKind::CallExternInto(_, args, dest),
                    ..
                }) if var_of(dest).is_some_and(|d| self.is_cand(d)) => {
                    for a in args {
                        self.expr(a);
                    }
                    let d = var_of(dest).unwrap();
                    if fresh != Some(d) {
                        self.mutated[d as usize] = true;
                    }
                }
                other => {
                    self.stmt(other);
                    fresh = None;
                }
            }
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Assign(_, e) | Stmt::StoreGlobal(_, e) | Stmt::Eval(e) | Stmt::Throw(e) => {
                self.expr(e)
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e)
                }
            }
            Stmt::StoreField(o, _, _, v) | Stmt::CellSet(o, v) => {
                self.expr(o);
                self.expr(v);
            }
            Stmt::StoreIndex(a, i, v) => {
                self.expr(a);
                self.expr(i);
                self.expr(v);
            }
            Stmt::If(c, a, b) => {
                self.expr(c);
                self.stmts(a);
                self.stmts(b);
            }
            Stmt::Block(_, b) | Stmt::Loop(_, b) => self.stmts(b),
            Stmt::Try(b, _, h) => {
                self.stmts(b);
                self.stmts(h);
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Line(_) => {}
        }
    }
}

fn scalarise(f: &mut Func, value: &HashSet<StructId>, counts: &[usize]) {
    let n = f.locals.len();
    let mut cand = vec![false; n];
    for (i, t) in f.locals.iter().enumerate() {
        if let Ty::Obj(sid) = t
            && value.contains(sid)
            && !f.params.contains(&(i as VarId))
        {
            cand[i] = true;
        }
    }
    if !cand.iter().any(|c| *c) {
        return;
    }
    let mut a = Analysis {
        f,
        ok: cand.clone(),
        cand,
        mutated: vec![false; n],
        edges: Vec::new(),
    };
    let body = f.body.clone();
    a.stmts(&body);
    // Copies must not make two names for something that changes.
    loop {
        let mut changed = false;
        for &(v, d) in &a.edges {
            if !a.ok[v] && d != usize::MAX && a.ok[d] {
                a.ok[d] = false;
                changed = true;
            }
            let both = d != usize::MAX && a.ok[v] && a.ok[d];
            let copy_in = a.ok[v] && (d == usize::MAX || !a.ok[d]);
            if (both && (a.mutated[v] || a.mutated[d])) || (copy_in && a.mutated[v]) {
                a.ok[v] = false;
                if d != usize::MAX {
                    a.ok[d] = false;
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let ok = a.ok;
    if !ok.iter().any(|o| *o) {
        return;
    }
    let mut rw = Rewrite {
        base: vec![0; n],
        ok,
        counts: Vec::new(),
        locals: Vec::new(),
        sids: vec![0; n],
    };
    let mut locals = std::mem::take(&mut f.locals);
    for v in 0..n {
        if rw.ok[v] {
            let Ty::Obj(sid) = locals[v] else {
                unreachable!()
            };
            rw.base[v] = locals.len() as VarId;
            rw.sids[v] = sid;
            for _ in 0..counts[sid] {
                locals.push(Ty::Num);
            }
            locals[v] = Ty::Num;
        }
    }
    rw.counts = counts.to_vec();
    rw.locals = locals;
    let body = std::mem::take(&mut f.body);
    f.body = rw.stmts(body);
    f.locals = rw.locals;
}

struct Rewrite {
    /// First scalar local of each replaced local.
    base: Vec<VarId>,
    ok: Vec<bool>,
    counts: Vec<usize>,
    locals: Vec<Ty>,
    sids: Vec<StructId>,
}

impl Rewrite {
    fn is_ok(&self, v: VarId) -> bool {
        self.ok.get(v as usize).copied().unwrap_or(false)
    }

    fn sc(&self, v: VarId, i: usize) -> VarId {
        self.base[v as usize] + i as VarId
    }

    fn n(&self, v: VarId) -> usize {
        self.counts[self.sids[v as usize]]
    }

    fn rw(&mut self, e: &mut Expr) {
        if let ExprKind::Field(b, _, i) = &mut e.kind {
            let i = *i;
            if let Some(v) = var_of(b)
                && self.is_ok(v)
            {
                *e = num_var(self.sc(v, i));
                return;
            }
            if let ExprKind::Seq(pre, last) = &mut b.kind
                && let Some(d) = var_of(last)
                && self.is_ok(d)
            {
                let pre = self.stmts(std::mem::take(pre));
                *e = Expr {
                    kind: ExprKind::Seq(pre, Box::new(num_var(self.sc(d, i)))),
                    ty: Ty::Num,
                };
                return;
            }
        }
        if let ExprKind::CallExternInto(idx, args, dest) = &mut e.kind
            && let Some(d) = var_of(dest)
            && self.is_ok(d)
        {
            let idx = *idx;
            let mut args = std::mem::take(args);
            for a in &mut args {
                self.rw(a);
            }
            let vars = (0..self.n(d)).map(|i| self.sc(d, i)).collect();
            e.kind = ExprKind::CallExternOut(idx, args, vars);
            return;
        }
        if let ExprKind::Seq(ss, v) = &mut e.kind {
            *ss = self.stmts(std::mem::take(ss));
            self.rw(v);
            return;
        }
        for k in kids_mut(e) {
            self.rw(k);
        }
    }

    fn zero(&self, v: VarId) -> Vec<Stmt> {
        (0..self.n(v))
            .map(|i| {
                Stmt::Assign(
                    self.sc(v, i),
                    Expr {
                        kind: ExprKind::Num(0.0),
                        ty: Ty::Num,
                    },
                )
            })
            .collect()
    }

    /// `v = e` for a replaced `v`.
    fn assign(&mut self, v: VarId, e: Expr) -> Vec<Stmt> {
        let mut e = e;
        match &mut e.kind {
            ExprKind::NewObj(_) => return self.zero(v),
            ExprKind::Var(d) if self.is_ok(*d) => {
                return (0..self.n(v))
                    .map(|i| Stmt::Assign(self.sc(v, i), num_var(self.sc(*d, i))))
                    .collect();
            }
            ExprKind::Seq(pre, last) if var_of(last).is_some_and(|d| self.is_ok(d)) => {
                let d = var_of(last).unwrap();
                let mut out = self.stmts(std::mem::take(pre));
                out.extend(
                    (0..self.n(v)).map(|i| Stmt::Assign(self.sc(v, i), num_var(self.sc(d, i)))),
                );
                return out;
            }
            _ => {}
        }
        // Copy-in: read the object once.
        let sid = self.sids[v as usize];
        let ty = Ty::Obj(sid);
        let h = self.locals.len() as VarId;
        self.locals.push(ty.clone());
        self.rw(&mut e);
        let mut out = vec![Stmt::Assign(h, e)];
        for i in 0..self.n(v) {
            out.push(Stmt::Assign(
                self.sc(v, i),
                Expr {
                    kind: ExprKind::Field(
                        Box::new(Expr {
                            kind: ExprKind::Var(h),
                            ty: ty.clone(),
                        }),
                        sid,
                        i,
                    ),
                    ty: Ty::Num,
                },
            ));
        }
        out
    }

    fn stmts(&mut self, list: Vec<Stmt>) -> Vec<Stmt> {
        let mut out = Vec::with_capacity(list.len());
        for s in list {
            match s {
                Stmt::Assign(v, e) if self.is_ok(v) => out.extend(self.assign(v, e)),
                Stmt::StoreField(o, sid, i, mut val) => {
                    if let Some(d) = var_of(&o)
                        && self.is_ok(d)
                    {
                        self.rw(&mut val);
                        out.push(Stmt::Assign(self.sc(d, i), val));
                    } else {
                        let mut o = o;
                        self.rw(&mut o);
                        self.rw(&mut val);
                        out.push(Stmt::StoreField(o, sid, i, val));
                    }
                }
                other => out.push(self.stmt(other)),
            }
        }
        out
    }

    fn stmt(&mut self, s: Stmt) -> Stmt {
        match s {
            Stmt::Assign(v, mut e) => {
                self.rw(&mut e);
                Stmt::Assign(v, e)
            }
            Stmt::StoreGlobal(g, mut e) => {
                self.rw(&mut e);
                Stmt::StoreGlobal(g, e)
            }
            Stmt::Eval(mut e) => {
                self.rw(&mut e);
                Stmt::Eval(e)
            }
            Stmt::Throw(mut e) => {
                self.rw(&mut e);
                Stmt::Throw(e)
            }
            Stmt::Return(e) => Stmt::Return(e.map(|mut e| {
                self.rw(&mut e);
                e
            })),
            Stmt::CellSet(mut a, mut b) => {
                self.rw(&mut a);
                self.rw(&mut b);
                Stmt::CellSet(a, b)
            }
            Stmt::StoreIndex(mut a, mut i, mut v) => {
                self.rw(&mut a);
                self.rw(&mut i);
                self.rw(&mut v);
                Stmt::StoreIndex(a, i, v)
            }
            Stmt::StoreField(mut o, sid, i, mut v) => {
                self.rw(&mut o);
                self.rw(&mut v);
                Stmt::StoreField(o, sid, i, v)
            }
            Stmt::If(mut c, a, b) => {
                self.rw(&mut c);
                Stmt::If(c, self.stmts(a), self.stmts(b))
            }
            Stmt::Block(l, b) => Stmt::Block(l, self.stmts(b)),
            Stmt::Loop(l, b) => Stmt::Loop(l, self.stmts(b)),
            Stmt::Try(b, v, h) => Stmt::Try(self.stmts(b), v, self.stmts(h)),
            s @ (Stmt::Break(_) | Stmt::Continue(_) | Stmt::Line(_)) => s,
        }
    }
}
