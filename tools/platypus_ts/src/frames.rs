// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Planning the shadow-stack frame of a function: which locals are mirrored, and how many
//! extra slots the code generator needs to keep pointer temporaries alive across allocations.
//!
//! The rules here and the code generator's `eval_args` must agree exactly: both use
//! `spill_flags`, so the slot count computed here is the number the generator consumes.

use crate::ir::*;

pub struct FramePlan {
    /// Frame slot of each local (only pointer-typed locals have one).
    pub ptr_slot: Vec<Option<u32>>,
    /// Slots for pointer locals; spill temporaries come after them.
    pub n_locals: u32,
    pub n_spills: u32,
}

impl FramePlan {
    pub fn total(&self) -> u32 {
        self.n_locals + self.n_spills
    }
}

pub fn plan(f: &Func) -> FramePlan {
    let mut ptr_slot = Vec::new();
    let mut n = 0;
    for t in &f.locals {
        if t.is_ptr() {
            ptr_slot.push(Some(n));
            n += 1;
        } else {
            ptr_slot.push(None);
        }
    }
    let mut spills = 0;
    count_stmts(&f.body, &mut spills);
    FramePlan {
        ptr_slot,
        n_locals: n,
        n_spills: spills,
    }
}

/// A pointer that stays reachable through a root while the `later` operands are evaluated: a
/// local whose slot no later operand overwrites, a global, null.
///
/// A field read is not stable. The object it came from is rooted, but a later operand can
/// overwrite that field and allocate, leaving the value only in the half-evaluated argument list
/// (`f(h.item, replace(h))`; see `M9-TS-27`).
fn is_stable(e: &Expr, later: &[&Expr]) -> bool {
    match &e.kind {
        ExprKind::Var(v) => !later.iter().any(|l| assigns(l, *v)),
        ExprKind::Global(_) | ExprKind::Null => true,
        // A `let` global is a root too, but replacing it drops the old value. Any call can do
        // that (the callee may assign the global), so only a call-free tail keeps it stable.
        ExprKind::GlobalVar(g) => !later.iter().any(|l| replaces_global(l, *g)),
        ExprKind::Cast(a) => is_stable(a, later),
        _ => false,
    }
}

/// Does evaluating `e` assign local `v` (so its frame slot stops holding the old value)?
fn assigns(e: &Expr, v: VarId) -> bool {
    scan_expr(
        e,
        &|_| false,
        &|s| matches!(s, Stmt::Assign(x, _) if *x == v),
    )
}

/// Can evaluating `e` store to global `g`: directly, or inside any call?
fn replaces_global(e: &Expr, g: GlobalId) -> bool {
    scan_expr(
        e,
        &|x| {
            matches!(
                x.kind,
                ExprKind::Call(..) | ExprKind::CallVirtual(..) | ExprKind::CallClosure(..)
            )
        },
        &|s| matches!(s, Stmt::StoreGlobal(x, _) if *x == g),
    )
}

/// Does `hit_e` match an expression, or `hit_s` a statement, anywhere inside `e`?
pub(crate) fn scan_expr(
    e: &Expr,
    hit_e: &dyn Fn(&Expr) -> bool,
    hit_s: &dyn Fn(&Stmt) -> bool,
) -> bool {
    if hit_e(e) {
        return true;
    }
    if let ExprKind::Seq(stmts, _) = &e.kind
        && stmts.iter().any(|s| scan_stmt(s, hit_e, hit_s))
    {
        return true;
    }
    children(e).iter().any(|c| scan_expr(c, hit_e, hit_s))
}

pub(crate) fn scan_stmt(s: &Stmt, he: &dyn Fn(&Expr) -> bool, hs: &dyn Fn(&Stmt) -> bool) -> bool {
    if hs(s) {
        return true;
    }
    let e = |x: &Expr| scan_expr(x, he, hs);
    let st = |b: &[Stmt]| b.iter().any(|s| scan_stmt(s, he, hs));
    match s {
        Stmt::Assign(_, x) | Stmt::StoreGlobal(_, x) | Stmt::Eval(x) | Stmt::Throw(x) => e(x),
        Stmt::Try(b, _, h) => st(b) || st(h),
        Stmt::Return(x) => x.as_ref().is_some_and(e),
        Stmt::StoreField(o, _, _, x) | Stmt::CellSet(o, x) => e(o) || e(x),
        Stmt::StoreIndex(a, i, x) => e(a) || e(i) || e(x),
        Stmt::If(c, a, b) => e(c) || st(a) || st(b),
        Stmt::Block(_, b) | Stmt::Loop(_, b) => st(b),
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::Line(_) => false,
    }
}

/// Which operands (evaluated in order) must be spilled to the frame because a later operand
/// can allocate and so collect.
pub fn spill_flags(args: &[&Expr]) -> Vec<bool> {
    (0..args.len())
        .map(|i| {
            args[i].ty.is_ptr()
                && !is_stable(args[i], &args[i + 1..])
                && args[i + 1..].iter().any(|a| may_alloc(a))
        })
        .collect()
}

fn n_spills(args: &[&Expr]) -> u32 {
    spill_flags(args).iter().filter(|b| **b).count() as u32
}

/// Can evaluating this expression run a collection?
pub fn may_alloc(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::NewObj(_)
        | ExprKind::Call(..)
        | ExprKind::CallVirtual(..)
        | ExprKind::CallClosure(..) => true,
        ExprKind::NewCell(_) | ExprKind::MakeClosure(..) => true,
        // An engine function returning a string copies it into a new script string.
        ExprKind::CallExtern(..) if e.ty == Ty::Str => true,
        // A sort runs the comparator, which allocates.
        ExprKind::Rt(RtFn::ArrNew | RtFn::ArrSort, _) => true,
        // The runtime's string functions that make a string can collect first.
        ExprKind::Rt(RtFn::Str(name), _) if name.ends_with("_gc") => true,
        ExprKind::Seq(stmts, v) => stmts.iter().any(stmt_may_alloc) || may_alloc(v),
        _ => children(e).iter().any(|c| may_alloc(c)),
    }
}

fn stmt_may_alloc(s: &Stmt) -> bool {
    match s {
        Stmt::Assign(_, e) | Stmt::StoreGlobal(_, e) | Stmt::Eval(e) | Stmt::Throw(e) => {
            may_alloc(e)
        }
        Stmt::Try(b, _, h) => b.iter().chain(h).any(stmt_may_alloc),
        Stmt::Return(e) => e.as_ref().is_some_and(may_alloc),
        Stmt::StoreField(o, _, _, v) | Stmt::CellSet(o, v) => may_alloc(o) || may_alloc(v),
        Stmt::StoreIndex(a, i, v) => may_alloc(a) || may_alloc(i) || may_alloc(v),
        Stmt::If(c, a, b) => {
            may_alloc(c) || a.iter().any(stmt_may_alloc) || b.iter().any(stmt_may_alloc)
        }
        Stmt::Block(_, b) | Stmt::Loop(_, b) => b.iter().any(stmt_may_alloc),
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::Line(_) => false,
    }
}

/// Direct sub-expressions (not the statements inside a `Seq`).
fn children(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Un(_, a)
        | ExprKind::IsNull(a)
        | ExprKind::Cast(a)
        | ExprKind::Truthy(a)
        | ExprKind::TagStr(a)
        | ExprKind::StrTag(a)
        | ExprKind::Bits(a)
        | ExprKind::Unbits(a)
        | ExprKind::Field(a, ..)
        | ExprKind::InstanceOf(a, _)
        | ExprKind::Len(a) => vec![a],
        ExprKind::Bin(_, a, b)
        | ExprKind::Cmp(_, a, b)
        | ExprKind::And(a, b)
        | ExprKind::Or(a, b)
        | ExprKind::Index(a, b) => vec![a, b],
        ExprKind::Cond(c, a, b) => vec![c, a, b],
        ExprKind::Call(_, args)
        | ExprKind::CallVirtual(_, args)
        | ExprKind::CallExtern(_, args)
        | ExprKind::CallExternOut(_, args, _)
        | ExprKind::Rt(_, args) => args.iter().collect(),
        ExprKind::CallClosure(c, args) => std::iter::once(&**c).chain(args.iter()).collect(),
        ExprKind::CallExternInto(_, args, dest) => {
            args.iter().chain(std::iter::once(&**dest)).collect()
        }
        ExprKind::Seq(_, v) => vec![v],
        ExprKind::NewCell(a) | ExprKind::CellGet(a) => vec![a],
        ExprKind::MakeClosure(_, Some(env)) => vec![env],
        _ => vec![],
    }
}

fn count_expr(e: &Expr, n: &mut u32) {
    match &e.kind {
        ExprKind::Call(_, args)
        | ExprKind::CallVirtual(_, args)
        | ExprKind::CallExtern(_, args)
        | ExprKind::CallExternInto(_, args, _)
        | ExprKind::CallExternOut(_, args, _)
        | ExprKind::Rt(_, args) => {
            *n += n_spills(&args.iter().collect::<Vec<_>>());
        }
        ExprKind::CallClosure(c, args) => {
            let v: Vec<&Expr> = std::iter::once(&**c).chain(args.iter()).collect();
            *n += n_spills(&v);
        }
        ExprKind::Index(a, i) | ExprKind::Cmp(_, a, i) => *n += n_spills(&[a, i]),
        ExprKind::Seq(stmts, _) => count_stmts(stmts, n),
        _ => {}
    }
    for c in children(e) {
        count_expr(c, n);
    }
}

fn count_stmts(stmts: &[Stmt], n: &mut u32) {
    for s in stmts {
        match s {
            Stmt::Assign(_, e) | Stmt::StoreGlobal(_, e) | Stmt::Eval(e) | Stmt::Throw(e) => {
                count_expr(e, n)
            }
            Stmt::Try(b, _, h) => {
                count_stmts(b, n);
                count_stmts(h, n);
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    count_expr(e, n)
                }
            }
            Stmt::StoreField(o, _, _, v) | Stmt::CellSet(o, v) => {
                *n += n_spills(&[o, v]);
                count_expr(o, n);
                count_expr(v, n);
            }
            Stmt::StoreIndex(a, i, v) => {
                *n += n_spills(&[a, i, v]);
                count_expr(a, n);
                count_expr(i, n);
                count_expr(v, n);
            }
            Stmt::If(c, a, b) => {
                count_expr(c, n);
                count_stmts(a, n);
                count_stmts(b, n);
            }
            Stmt::Block(_, b) | Stmt::Loop(_, b) => count_stmts(b, n),
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Line(_) => {}
        }
    }
}
