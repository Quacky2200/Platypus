// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! TypeScript AST (oxc) -> Platypus IR.
//!
//! Types come from the source's own annotations (see `types.rs`); expression types are inferred
//! bottom-up from them, with a contextual "expected" type pushed down for literals. The program
//! must already pass `tsc --strict`: the lowering trusts narrowing instead of re-proving it.

macro_rules! bail {
    ($($t:tt)*) => { return Err(format!($($t)*)) };
}

mod accessors;
mod arrays;
mod asyncs;
mod bigint;
mod classes;
mod clone;
mod closure;
pub(crate) mod components;
mod date;
mod dicts;
mod engine_objs;
mod enums;
mod expr;
mod facade;
mod fnvalue;
mod generators;
mod generics;
mod ifaces;
mod intl;
mod iter;
mod json;
mod jsx;
mod keyed;
mod literals;
pub use jsx::UI_MANIFEST;
mod maps;
mod messages;
mod object;
mod prelude;
mod promise;
mod regexp;
mod scan;
mod stmt;
mod strings;
mod tagged;
mod temporal;
mod typed;
mod types;
mod types_index;
mod utility;

use crate::ir::*;
use classes::Which;
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;
use std::collections::{HashMap, HashSet};

pub type R<T> = Result<T, String>;

#[derive(Clone, Copy)]
pub(crate) enum FnBody<'a> {
    Block(&'a FunctionBody<'a>),
    Expr(&'a Expression<'a>),
}

/// What a declared function is, for the `this` parameter and the constructor's body.
#[derive(Clone, Copy)]
pub(crate) enum FnKind {
    Free,
    /// A method or accessor of the class with this struct: `this` is the first parameter.
    Method(StructId),
    /// A constructor body (`C$init`): `this` is the first parameter, the object already exists.
    /// The flag: a derived class with no constructor of its own.
    Ctor(StructId, bool),
}

pub(crate) struct FnSrc<'a> {
    pub params: &'a FormalParameters<'a>,
    pub ret: Option<&'a TSType<'a>>,
    pub body: FnBody<'a>,
    pub exported: bool,
    pub kind: FnKind,
    /// A return type that is not an annotation (a constructor returns its class).
    pub ret_ty: Option<Ty>,
    /// The type parameters in scope: set for a specialisation of a generic function or class.
    pub tenv: Option<generics::TEnv>,
}

/// What a name means inside a function.
#[derive(Clone)]
pub(crate) enum Binding {
    /// A local variable holding the value.
    Direct(VarId),
    /// A captured variable: a local holding the cell, with the value's type.
    Boxed(VarId, Ty),
}

/// A top-level declarator: (declarator, exported, declared with `const`).
enum TopItem<'a> {
    Var(&'a VariableDeclarator<'a>, bool, bool),
    Class(&'a Class<'a>),
    /// A statement like `createRoot().render(<App />);` or `console.log("x");`.
    Expr(&'a Expression<'a>),
    /// A control-flow statement (`if`, `for`, `while`, `switch`, `try`, a block, ..).
    Stmt(&'a Statement<'a>),
    /// The start of a module's text in a bundle that has a top-level `await` somewhere
    /// (`__platypus_module(..)`, written by `modules.rs`).
    Module(ModuleInfo),
}

/// A module of the bundle, as `modules.rs` described it.
#[derive(Clone)]
struct ModuleInfo {
    /// Its position in the bundle (dependencies first).
    id: usize,
    name: String,
    /// The module has a top-level `await` of its own.
    tla: bool,
    /// The modules it imports that may finish late (they have a top-level `await` or import
    /// such a module), by position.
    deps: Vec<usize>,
}

impl ModuleInfo {
    /// Does evaluating it take more than the synchronous run (it awaits, or waits for a dependency)?
    fn is_async(&self) -> bool {
        self.tla || !self.deps.is_empty()
    }
}

/// `__platypus_module(id, "path", tla, [dep, ..])`, the marker `modules.rs` writes before a module.
fn module_marker(e: &Expression<'_>) -> Option<ModuleInfo> {
    let Expression::CallExpression(c) = e else {
        return None;
    };
    let Expression::Identifier(f) = &c.callee else {
        return None;
    };
    if f.name != "__platypus_module" {
        return None;
    }
    let num = |a: Option<&Argument<'_>>| match a {
        Some(Argument::NumericLiteral(n)) => Some(n.value as usize),
        _ => None,
    };
    let name = match c.arguments.get(1) {
        Some(Argument::StringLiteral(s)) => s.value.to_string(),
        _ => return None,
    };
    let deps = match c.arguments.get(3) {
        Some(Argument::ArrayExpression(a)) => a
            .elements
            .iter()
            .filter_map(|e| match e {
                ArrayExpressionElement::NumericLiteral(n) => Some(n.value as usize),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    Some(ModuleInfo {
        id: num(c.arguments.first())?,
        name,
        tla: num(c.arguments.get(2))? != 0,
        deps,
    })
}

#[derive(Clone)]
pub(crate) enum ConstInfo {
    /// A compile-time constant, inlined at every use.
    Inline(Expr),
    /// A module-level value in a data slot; the flag says it is a `let`/`var` (assignable).
    Global(GlobalId, Ty, bool),
}

#[derive(Clone)]
pub(crate) enum RetKind {
    /// The callback's result is a truth value (filter, find, some, every).
    Pred,
    /// The callback's result is stored in the variable; its type is inferred if still `Void`.
    Val,
    /// Nothing is returned (forEach).
    Void,
}

#[derive(Clone)]
pub(crate) struct RetTarget {
    pub label: LabelId,
    pub var: Option<VarId>,
    pub kind: RetKind,
}

/// What `break` and `continue` mean at one nesting level.
#[derive(Clone)]
pub(crate) struct LoopLabels {
    /// The source label (`outer: for ...`), if any.
    pub name: Option<String>,
    /// `break` target: leaves the loop, switch or labelled block.
    pub brk: LabelId,
    /// `continue` target; none for a switch or a labelled block.
    pub cont: Option<LabelId>,
    /// Does an unlabelled `break` stop here (loops and switches, not labelled blocks)?
    pub unlabeled_break: bool,
}

/// How control leaves a `try` ... `finally` early.
#[derive(Clone, PartialEq)]
pub(crate) enum Exit {
    Return,
    Break(LabelId),
    /// `continue`: the label, and whether it is a loop top (`Continue`) or a block end (`Break`).
    Continue(LabelId, bool),
}

/// An open `try ... finally`. A `return`, `break` or `continue` inside it records which exit it
/// wanted in `kind`, stores a return value in `ret_var`, and leaves through the label `done`; the
/// finalizer runs after that label, and then the recorded exit is taken (through the finalizers
/// of the enclosing `try`s too).
pub(crate) struct FinCtx {
    pub done: LabelId,
    pub kind: VarId,
    pub loops_depth: usize,
    pub rets_depth: usize,
    pub ret_var: Option<VarId>,
    pub exits: Vec<Exit>,
}

#[derive(Default)]
pub(crate) struct FnCtx {
    /// The locals that hold a named variable (the cell, for a captured one), for a debugger.
    pub names: Vec<(VarId, String)>,
    /// Open `try ... finally`s, innermost last.
    pub fins: Vec<FinCtx>,
    pub locals: Vec<Ty>,
    pub scopes: Vec<HashMap<String, Binding>>,
    /// Declared or inferred return type.
    pub ret: Option<Ty>,
    /// The return type is inferred for a callback: a string literal is a `string`.
    pub widen_ret: bool,
    /// The function has no return annotation: `return null` and `return obj` make it nullable.
    pub ret_inferred: bool,
    /// An inferred return type met a bare `return;`: if a value is returned elsewhere, the bare
    /// ones return null.
    pub bare_returns: bool,
    pub next_label: u32,
    /// Enclosing loops, switches and labelled statements, innermost last.
    pub loops: Vec<LoopLabels>,
    /// A label just read (`outer:`) that the loop lowered next takes as its name.
    pub pending_label: Option<String>,
    pub rets: Vec<RetTarget>,
    /// Open optional chains: (exit label, result variable).
    pub chain: Vec<(LabelId, VarId)>,
    /// Lowering a method or constructor of this class.
    pub cls: Option<(String, StructId)>,
    /// Parameters written as a union of string literals (`key: "fog" | "lowRes"`): the members,
    /// so `state[key]` knows which fields the key can name.
    pub lit_sets: HashMap<String, Vec<String>>,
    /// Closures whose cells were made at the top of their block (`prebind_arrows`): the cell and
    /// the closure's type, for the declaration to fill.
    pub prebound: HashMap<String, (VarId, Ty)>,
    /// Generic arrow functions declared in this function (`const more = <T>(a: T) => ..`): the
    /// name and the key of the generic function in `generic_fns`.
    pub local_generics: HashMap<String, String>,
    /// Paths (`o.scale`) the tests around the expression narrowed to members of their union
    /// (`tagged.rs`); a function or closure body starts with none.
    pub narrowed: Vec<(String, Vec<usize>)>,
    /// Locals declared `const x = <number literal>`: their values, so `xs.flat(x)` is exact.
    pub const_nums: HashMap<VarId, f64>,
}

/// A discriminated union's struct (`type B = { kind: "a"; xs: A[] } | { kind: "b"; xs: B[] }`)
/// where a field has a different type per variant: that field is a tagged union, and a test of
/// the tag narrows it to the member of the variants the test lets through.
#[derive(Clone, Debug)]
pub(crate) struct Disc {
    /// The tag field (`kind`).
    pub tag: String,
    /// Each variant's tag values.
    pub variants: Vec<Vec<String>>,
    /// The differing fields, and the member of the field's union each variant has (`None`: the
    /// variant has no such field).
    pub fields: Vec<(String, Vec<Option<usize>>)>,
}

pub struct Lowerer<'a> {
    pub(crate) src: &'a str,
    pub(crate) m: Module,
    pub(crate) aliases: HashMap<String, &'a TSTypeAliasDeclaration<'a>>,
    pub(crate) ifaces: HashMap<String, &'a TSInterfaceDeclaration<'a>>,
    pub(crate) named: HashMap<String, Ty>,
    pub(crate) shapes: HashMap<String, StructId>,
    /// Object-shaped aliases being worked out (`types.rs` `named_type`), and those of them
    /// that named themselves meanwhile (recursive ones keep their placeholder struct).
    pub(crate) alias_resolving: HashSet<String>,
    pub(crate) alias_hits: HashSet<String>,
    pub(crate) tag_ids: HashMap<String, u32>,
    pub(crate) fsrc: HashMap<String, FnSrc<'a>>,
    pub(crate) fids: HashMap<String, FuncId>,
    pub(crate) fsig: Vec<(Vec<Ty>, Option<Ty>)>,
    /// 0 declared, 1 being lowered, 2 done.
    pub(crate) fstate: Vec<u8>,
    pub(crate) consts: HashMap<String, ConstInfo>,
    /// The engine API scripts may call (`Namespace.fn(..)`) and its handle types.
    pub(crate) manifest: crate::manifest::Manifest,
    /// The structs of the manifest's value classes, made when first named.
    pub(crate) value_structs: HashMap<String, StructId>,
    /// Class declarations by name, and what is known about each once its struct exists.
    pub(crate) class_decls: HashMap<String, &'a Class<'a>>,
    pub(crate) classes: HashMap<String, classes::ClassInfo>,
    pub(crate) class_of_sid: HashMap<StructId, String>,
    /// An empty parameter list and body for classes without a constructor.
    pub(crate) empty_fn: Option<(&'a FormalParameters<'a>, &'a FunctionBody<'a>)>,
    /// Enum members by enum name (`enums.rs`).
    pub(crate) enums: HashMap<String, HashMap<String, Expr>>,
    pub(crate) cur: FnCtx,
    /// Names that closures in the function being lowered capture (`scan.rs`).
    pub(crate) captured: HashSet<String>,
    /// The names the function being lowered (or the whole program, for top-level code) assigns
    /// after declaring them (`scan.rs`).
    pub(crate) assigned: HashSet<String>,
    pub(crate) closure_free: HashMap<u32, Vec<String>>,
    pub(crate) lambdas: usize,
    /// Wrapper functions that let a top-level function be used as a value.
    pub(crate) wrappers: HashMap<String, FuncId>,
    /// The capture groups of `const re = /literal/` names (for `replace` callbacks).
    pub(crate) regex_info: HashMap<String, Vec<Option<String>>>,
    /// Lines of prelude source in front of the program (skipped in messages).
    pub(crate) prelude_lines: usize,
    /// Lines of the program as given: what the compiler appends (hook wrappers, facade members)
    /// comes after and gets no line markers, so a debugger never stops in it.
    pub(crate) user_lines: usize,
    /// The next closure's return type comes from its body, not from the expected type.
    pub(crate) infer_ret: bool,
    /// Top-level `const NAME = init` without an annotation, for guessing field types from syntax.
    pub(crate) const_inits: HashMap<String, &'a Expression<'a>>,
    /// The expected type is an array of string-literal members: the next `.map` keeps its
    /// callback's literals instead of making strings.
    pub(crate) map_keeps_lit: bool,
    /// Lower the plain functions nothing calls too (`lower_source_opts`).
    pub(crate) check_unused: bool,
    /// The next object literal leaves out its computed properties (`lower_object` sets them after).
    pub(crate) skip_computed: bool,
    /// The type expected of the call being lowered (`lower_call`), and of a method call on an
    /// array (`xs.map(..)` where a `ReactNode[]` is expected: the callback's result type).
    pub(crate) call_exp: Option<Ty>,
    pub(crate) method_exp: Option<Ty>,
    /// `Array.from(xs, fn)` lowers as `xs.map(fn)` with the callback one argument further.
    pub(crate) cb_skip: usize,
    /// The type parameters of the specialisation being lowered (`generics.rs`).
    pub(crate) tenv: generics::TEnv,
    pub(crate) generic_fns: HashMap<String, generics::GenericFn<'a>>,
    pub(crate) generic_classes: HashMap<String, generics::GenericClass<'a>>,
    /// Generic methods by (class name, method name).
    pub(crate) generic_methods: HashMap<(String, String), generics::GenericFn<'a>>,
    /// Specialised classes by name: their type environment.
    pub(crate) class_tenv: HashMap<String, generics::TEnv>,
    /// Specialised classes, interfaces and aliases by struct: the generic name and its arguments.
    pub(crate) targs_of_struct: HashMap<StructId, (String, Vec<Ty>)>,
    pub(crate) inst_names: HashMap<String, String>,
    /// Specialised classes whose methods are not declared yet.
    pub(crate) pending_collect: Vec<String>,
    /// The program's own classes are laid out: later specialisations finish at once.
    pub(crate) classes_ready: bool,
    /// The program's top-level function declarations, known before any is collected (for
    /// `ReturnType<typeof f>` in a type met earlier).
    pub(crate) top_fn_decls: HashMap<String, &'a Function<'a>>,
    /// Names given a getter in an object literal (`accessors.rs`).
    pub(crate) setter_names: HashSet<String>,
    pub(crate) accessor_names: HashSet<String>,
    /// Unions of different types (`tagged.rs`): the struct and its member types.
    pub(crate) unions: HashMap<StructId, Vec<Ty>>,
    /// Discriminated unions whose variants give a field different types: how the tag picks them.
    pub(crate) discs: HashMap<StructId, Disc>,
    /// While a function body is lowered early (`ReturnType<typeof f>` in a field type), classes
    /// it instantiates are collected and laid out at once.
    pub(crate) eager_classes: bool,
    /// Classes whose fields are laid out.
    pub(crate) laid_out: HashSet<String>,
    /// `instanceof GenericClass` tables, filled in once every specialisation exists.
    pub(crate) generic_itables: Vec<(usize, String)>,
    /// `Partial<Class>` and friends met before the classes had fields: (struct, utility, class struct, keys).
    /// The program has a top-level `await`: its initialisers and statements run as the body of an
    /// implicit async function, started by `ts_init` (see `lower_program`).
    pub(crate) top_await: bool,
    /// Interfaces some class implements (and the ones those extend): see `ifaces.rs`.
    pub(crate) dyn_ifaces: HashSet<String>,
    /// The structs of those interfaces, by interface name.
    pub(crate) iface_sids: HashMap<StructId, String>,
    /// (interface struct, struct) pairs: values of the second were used as the first.
    pub(crate) iface_impls: std::cell::RefCell<HashSet<(StructId, StructId)>>,
    pub(crate) iface_tables: Vec<ifaces::IfaceTable>,
    pub(crate) pending_utility: Vec<(StructId, String, Ty, Option<Vec<String>>)>,
}

fn mk_expr(kind: ExprKind, ty: Ty) -> Expr {
    Expr { kind, ty }
}

/// Parses and lowers one TypeScript module.
pub fn lower_source(src: &str) -> R<Module> {
    lower_source_with(src, &[])
}

/// Like `lower_source`, with engine API manifests (JSON, one per namespace).
pub fn lower_source_with(src: &str, manifests: &[String]) -> R<Module> {
    lower_source_opts(src, manifests, false)
}

/// Like `lower_source_with`; `check_unused` also lowers the plain functions nothing calls, which
/// are otherwise skipped (`platypus-build build --check-unused`).
pub fn lower_source_opts(src: &str, manifests: &[String], check_unused: bool) -> R<Module> {
    let mut lap = crate::timing::Lap::start();
    let user_lines = src.matches('\n').count() + 1;
    let mut manifest = crate::manifest::Manifest::default();
    for m in manifests {
        manifest.add_json(m)?;
    }
    lap.mark("engine manifests");
    // Members of the TypeScript engine API that the compiler writes over the native one (`facade.rs`).
    let with_facade = facade::extend_source(src, &manifest);
    let src = with_facade.as_deref().unwrap_or(src);
    // JSX and hooks become calls of the `Ui` namespace (first: the other rewrites parse plain TS).
    // The compiler brings the `Ui` namespace itself when the caller did not pass it (the host links
    // the `ts_ui_*` symbols; `platypus_ts_ui` checks that this copy equals its own).
    let with_keyed = keyed::desugar(src)?;
    let src = with_keyed.as_deref().unwrap_or(src);
    let wants_ui = jsx::wanted(src);
    if wants_ui && !manifest.namespaces.contains_key("Ui") {
        manifest.add_json(jsx::UI_MANIFEST)?;
    }
    if wants_ui && !manifest.namespaces.contains_key("UiRoot") {
        manifest.add_json(jsx::UI_ROOT_MANIFEST)?;
    }
    let rewritten_ui = if wants_ui { jsx::desugar(src)? } else { None };
    let src = rewritten_ui.as_deref().unwrap_or(src);
    // `Task.delay(..)` is a class of TypeScript appended to the program.
    let with_task = prelude::wants_task(src).then(|| {
        format!(
            "{src}
{}",
            prelude::TASK
        )
    });
    let src = with_task.as_deref().unwrap_or(src);
    // Generators are rewritten into closures over a coroutine, with classes written per yield type.
    let (rewritten, gen_templates) = match generators::desugar(src)? {
        Some(d) => (Some(d.src), d.templates),
        None => (None, String::new()),
    };
    let src = rewritten.as_deref().unwrap_or(src);
    // Promises and async functions become closures over coroutines too.
    let (rewritten2, async_templates, top_await) = match asyncs::desugar(src)? {
        Some(d) => (Some(d.src), d.templates, d.top_await),
        None => (None, String::new(), false),
    };
    let src = rewritten2.as_deref().unwrap_or(src);
    // `Temporal.PlainDate` is the prelude class `Temporal_PlainDate`.
    let rewritten3 = temporal::rewrite(src);
    let wants_temporal = rewritten3.is_some();
    let src = rewritten3.as_deref().unwrap_or(src);
    // `register(Class)` becomes exported constructor and hook functions for the host.
    let has_go = manifests.iter().any(|m| m.contains("\"GameObject\""));
    let with_cic = components::children_helpers(src, has_go).map(|h| format!("{src}{h}"));
    let src = with_cic.as_deref().unwrap_or(src);
    let with_tree = components::tree_helpers(src, has_go).map(|h| format!("{src}{h}"));
    let src = with_tree.as_deref().unwrap_or(src);
    // The contact hooks get the `Collision` class of the physics manifest.
    let has_contacts = manifest.classes.contains_key("Collision");
    let rewritten4 = components::desugar(src, has_go, has_contacts)?;
    let wants_components = components::wanted(src);
    let src = rewritten4.as_deref().unwrap_or(src);
    // Programs that use errors get the `Error` classes compiled in front of them.
    let mut head = String::new();
    if !gen_templates.is_empty()
        || !async_templates.is_empty()
        || wants_temporal
        || prelude::wanted(src)
        || prelude::wants_timers(src)
        || prelude::wants_regexp(src)
        || prelude::wants_date(src)
        || prelude::wants_intl(src)
        || prelude::wants_symbol(src)
    {
        // Regular expressions throw `SyntaxError`s, so they bring the error classes along.
        head += prelude::ERRORS;
    }
    if prelude::wants_regexp(src) {
        head += prelude::REGEXP;
    }
    if prelude::wants_date(src) {
        head += prelude::DATE;
    }
    if src.contains("Intl.Collator") {
        head += prelude::INTL_COLLATOR;
    }
    if src.contains("Intl.PluralRules") {
        head += prelude::INTL_PLURAL;
    }
    if src.contains("Intl.ListFormat") {
        head += prelude::INTL_LIST;
    }
    if src.contains("Intl.RelativeTimeFormat") {
        head += prelude::INTL_RTF;
    }
    if src.contains("Intl.NumberFormat") || src.contains("Intl.DateTimeFormat") {
        head += prelude::INTL_PARTS;
    }
    if src.contains("Intl.NumberFormat") {
        head += prelude::INTL_NUMBER;
    }
    if src.contains("Intl.DisplayNames") {
        head += prelude::INTL_DISPLAYNAMES;
    }
    if src.contains("getCanonicalLocales") {
        head += prelude::INTL_CANON;
    }
    if src.contains("Intl.Locale") {
        head += prelude::INTL_LOCALE;
    }
    if src.contains("Intl.Segmenter") {
        head += prelude::INTL_SEGMENTER;
    }
    if src.contains("Intl.DateTimeFormat") {
        head += prelude::INTL_DATETIME;
    }
    if wants_components.any() {
        head += &components::prelude(has_go);
    }
    if wants_ui {
        head += jsx::UI_PRELUDE;
        head += crate::ui_dts::prelude();
    }
    if wants_temporal {
        head += temporal::TEMPORAL;
    }
    if prelude::wants_symbol(src) {
        head += prelude::SYMBOL;
    }
    head += &prelude::weak_refs(src);
    if prelude::wants_arraybuffer(src) {
        head += prelude::ARRAYBUFFER;
    }
    if !gen_templates.is_empty() || !async_templates.is_empty() {
        head += prelude::COROUTINE;
        head += &gen_templates;
    }
    if prelude::wants_timers(src) || !async_templates.is_empty() {
        head += prelude::TIMERS;
    }
    head += &async_templates;
    let prelude_lines = head.matches('\n').count();
    crate::timing::add("prelude lines", prelude_lines as f64);
    crate::timing::add("program lines", src.matches('\n').count() as f64);
    let full = format!("{head}{src}");
    let src = full.as_str();
    if std::env::var_os("PLATYPUS_TS_DUMP_SRC").is_some() {
        eprintln!("{full}");
    }
    lap.mark("source rewrites (JSX, async, components, preludes)");
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, src, SourceType::ts()).parse();
    lap.mark("parse (oxc)");
    if !ret.diagnostics.is_empty() {
        let d = &ret.diagnostics[0];
        let line = d.labels.first().map(|l| {
            src[..(l.offset() as usize).min(src.len())]
                .matches(char::from(10))
                .count()
                + 1
        });
        match line {
            Some(l) => bail!("parse error (line {l} with the prelude): {d}"),
            None => bail!("parse error: {d}"),
        }
    }
    let program = ret.program;
    let mut l = Lowerer {
        check_unused,
        src,
        m: Module::default(),
        aliases: HashMap::new(),
        ifaces: HashMap::new(),
        named: HashMap::new(),
        shapes: HashMap::new(),
        alias_resolving: HashSet::new(),
        alias_hits: HashSet::new(),
        tag_ids: HashMap::new(),
        fsrc: HashMap::new(),
        fids: HashMap::new(),
        fsig: Vec::new(),
        fstate: Vec::new(),
        consts: HashMap::new(),
        manifest,
        value_structs: HashMap::new(),
        class_decls: HashMap::new(),
        classes: HashMap::new(),
        class_of_sid: HashMap::new(),
        empty_fn: None,
        enums: HashMap::new(),
        cur: FnCtx::default(),
        captured: HashSet::new(),
        assigned: HashSet::new(),
        closure_free: HashMap::new(),
        lambdas: 0,
        wrappers: HashMap::new(),
        regex_info: HashMap::new(),
        prelude_lines,
        user_lines,
        infer_ret: false,
        const_inits: HashMap::new(),
        cb_skip: 0,
        map_keeps_lit: false,
        skip_computed: false,
        call_exp: None,
        method_exp: None,
        tenv: Default::default(),
        generic_fns: HashMap::new(),
        generic_classes: HashMap::new(),
        generic_methods: HashMap::new(),
        class_tenv: HashMap::new(),
        targs_of_struct: HashMap::new(),
        inst_names: HashMap::new(),
        pending_collect: Vec::new(),
        classes_ready: false,
        top_fn_decls: HashMap::new(),
        accessor_names: HashSet::new(),
        setter_names: HashSet::new(),
        unions: HashMap::new(),
        discs: HashMap::new(),
        eager_classes: false,
        laid_out: HashSet::new(),
        generic_itables: Vec::new(),
        pending_utility: Vec::new(),
        top_await,
        dyn_ifaces: HashSet::new(),
        iface_sids: HashMap::new(),
        iface_impls: Default::default(),
        iface_tables: Vec::new(),
    };
    l.lower_program(&program)?;
    lap.mark("type inference and lowering to IR");
    let mut m = l.m;
    // A component class is compiled whether or not anything registers it.
    if wants_components.extends {
        m.note_use(crate::renderer::COMPONENT);
    }
    m.value_structs = l.value_structs.values().copied().collect();
    m.value_structs.sort_unstable();
    crate::throws::analyze(&mut m);
    lap.mark("throw analysis");
    Ok(m)
}

impl<'a> Lowerer<'a> {
    pub(crate) fn line(&self, offset: u32) -> usize {
        (self.src[..(offset as usize).min(self.src.len())]
            .matches('\n')
            .count()
            + 1)
        .saturating_sub(self.prelude_lines)
        .max(1)
    }

    /// Whether a byte offset of the source is in the prelude written in front of the program.
    fn in_prelude(&self, offset: u32) -> bool {
        self.src[..(offset as usize).min(self.src.len())]
            .matches('\n')
            .count()
            < self.prelude_lines
    }

    /// A top-level function nothing needs unless the code refers to it.
    fn skippable(&self, name: &str) -> bool {
        let src = &self.fsrc[name];
        matches!(src.kind, FnKind::Free)
            && !src.exported
            && (!name.starts_with("__")
                || components::is_scene_helper(name)
                || asyncs::is_lazy_helper(name))
    }

    pub(crate) fn tag(&mut self, name: &str) -> u32 {
        if let Some(t) = self.tag_ids.get(name) {
            return *t;
        }
        // Tag 0 is reserved: it is the "no tag" of a `"a" | "b" | undefined` slot (zeroed memory).
        if self.m.tags.is_empty() {
            self.m.tags.push(String::new());
        }
        let id = self.m.tags.len() as u32;
        self.m.tags.push(name.to_string());
        self.tag_ids.insert(name.to_string(), id);
        id
    }

    fn lower_program(&mut self, program: &'a Program<'a>) -> R<()> {
        self.scan_accessors(program);
        // Top-level code: a variable assigned anywhere in the program may change under a closure.
        self.assigned = scan::Scan::of_statements(&program.body).assigned;
        self.register_top_functions(program);
        // Top-level constants whose type comes from their initialiser.
        for stmt in &program.body {
            let decl = match stmt {
                Statement::VariableDeclaration(d) => Some(&**d),
                Statement::ExportDeclaration(x) => match &x.declaration {
                    Declaration::VariableDeclaration(d) => Some(&**d),
                    _ => None,
                },
                _ => None,
            };
            if let Some(d) = decl
                && d.kind == VariableDeclarationKind::Const
            {
                for v in &d.declarations {
                    if let (BindingPattern::BindingIdentifier(id), Some(init), None) =
                        (&v.id, &v.init, &v.type_annotation)
                    {
                        self.const_inits.insert(id.name.to_string(), init);
                    }
                }
            }
        }
        // 1. Types and function signatures, in any order.
        // 0. Enums, interfaces, aliases and classes first: their names are types that signatures
        // below mention.
        for stmt in &program.body {
            let decl = match stmt {
                Statement::ExportDeclaration(x) => Some(&x.declaration),
                _ => None,
            };
            match (stmt, decl) {
                (Statement::TSEnumDeclaration(e), _) => self.declare_enum(e)?,
                (Statement::ClassDeclaration(c), _) => self.declare_class_name(c)?,
                (_, Some(Declaration::TSEnumDeclaration(e))) => self.declare_enum(e)?,
                (_, Some(Declaration::ClassDeclaration(c))) => self.declare_class_name(c)?,
                // Interfaces and aliases are known before any class field is typed: the classes
                // written per promise type sit in front of the program and name them.
                (Statement::TSInterfaceDeclaration(d), _) => {
                    self.ifaces.insert(d.id.name.to_string(), d);
                }
                (Statement::TSTypeAliasDeclaration(d), _) => {
                    self.aliases.insert(d.id.name.to_string(), d);
                }
                (_, Some(Declaration::TSInterfaceDeclaration(d))) => {
                    self.ifaces.insert(d.id.name.to_string(), d);
                }
                (_, Some(Declaration::TSTypeAliasDeclaration(d))) => {
                    self.aliases.insert(d.id.name.to_string(), d);
                }
                _ => {}
            }
        }
        // Interfaces that classes implement are dispatched on at run time (`ifaces.rs`).
        let mut class_nodes: Vec<&'a Class<'a>> = Vec::new();
        for stmt in &program.body {
            match stmt {
                Statement::ClassDeclaration(c) => class_nodes.push(c),
                Statement::ExportDeclaration(x) => {
                    if let Declaration::ClassDeclaration(c) = &x.declaration {
                        class_nodes.push(c);
                    }
                }
                _ => {}
            }
        }
        self.mark_dynamic_ifaces(&class_nodes);
        let mut vars: Vec<TopItem<'a>> = Vec::new();
        for stmt in &program.body {
            match stmt {
                Statement::ExportDeclaration(e) => self.collect(&e.declaration, true, &mut vars)?,
                Statement::VariableDeclaration(d) => self.collect_vars(d, false, &mut vars)?,
                Statement::FunctionDeclaration(f) => self.collect_fn(f, false)?,
                Statement::ExpressionStatement(es) => match module_marker(&es.expression) {
                    Some(info) => vars.push(TopItem::Module(info)),
                    None => vars.push(TopItem::Expr(&es.expression)),
                },
                Statement::ClassDeclaration(c) if c.type_parameters.is_some() => {}
                Statement::ClassDeclaration(c) => {
                    self.collect_class(c)?;
                    vars.push(TopItem::Class(c));
                }
                Statement::TSInterfaceDeclaration(d) => {
                    self.ifaces.insert(d.id.name.to_string(), d);
                }
                Statement::TSTypeAliasDeclaration(d) => {
                    self.aliases.insert(d.id.name.to_string(), d);
                }
                Statement::IfStatement(_)
                | Statement::ForStatement(_)
                | Statement::ForInStatement(_)
                | Statement::ForOfStatement(_)
                | Statement::WhileStatement(_)
                | Statement::DoWhileStatement(_)
                | Statement::SwitchStatement(_)
                | Statement::TryStatement(_)
                | Statement::LabeledStatement(_)
                | Statement::ThrowStatement(_)
                | Statement::BlockStatement(_) => vars.push(TopItem::Stmt(stmt)),
                // Handled by the module graph, or erased types.
                Statement::EmptyStatement(_)
                | Statement::ImportDeclaration(_)
                | Statement::ExportNamedDeclaration(_)
                | Statement::ExportDefaultDeclaration(_)
                | Statement::ExportFromDeclaration(_)
                | Statement::ExportAllDeclaration(_)
                | Statement::TSEnumDeclaration(_)
                | Statement::DebuggerStatement(_) => {}
                other => {
                    let kind = match other {
                        Statement::ReturnStatement(_) => "return",
                        Statement::BreakStatement(_) => "break",
                        Statement::ContinueStatement(_) => "continue",
                        Statement::WithStatement(_) => "with",
                        _ => "statement",
                    };
                    bail!(
                        "{}: unsupported top-level statement: {kind}",
                        self.loc(oxc_span::GetSpan::span(other).start)
                    )
                }
            }
        }
        self.flush_pending_classes()?;
        self.finalize_classes()?;
        self.classes_ready = true;
        self.finish_utility_types()?;
        // 2. Top-level values, in source order, into the init function.
        let init_id = self.m.funcs.len();
        self.m.funcs.push(Func {
            name: "init".into(),
            exported: true,
            params: vec![],
            ret: Ty::Void,
            locals: vec![],
            body: vec![],
        });
        self.fsig.push((vec![], Some(Ty::Void)));
        self.fstate.push(2);
        self.cur = FnCtx::default();
        self.cur.scopes.push(HashMap::new());
        // The top-level code in segments, one per module of a bundle that has a top-level
        // `await` (modules.rs marks them). A module that awaits, or imports one that does, runs as
        // an async closure of its own, started where the module's text begins; the modules that
        // do not wait for anything run in place, so the order is ECMAScript's: a module runs after
        // the ones it imports have finished, and a sibling that does not depend on a pending
        // module runs while it waits.
        let mut segments: Vec<(Option<ModuleInfo>, Vec<TopItem<'a>>)> = vec![(None, Vec::new())];
        for item in vars {
            match item {
                TopItem::Module(info) => segments.push((Some(info), Vec::new())),
                other => segments.last_mut().expect("a segment").1.push(other),
            }
        }
        // Without markers a top-level `await` makes the whole program one module.
        if segments.len() == 1 && self.top_await {
            let items = std::mem::take(&mut segments[0].1);
            segments[0] = (
                Some(ModuleInfo {
                    id: 0,
                    name: "main".to_string(),
                    tla: true,
                    deps: Vec::new(),
                }),
                items,
            );
        }
        let mut init_body = Vec::new();
        let mut module_promise: HashMap<usize, GlobalId> = HashMap::new();
        for (info, items) in segments {
            match info {
                Some(info) if info.is_async() => {
                    self.lower_async_module(&info, items, &mut module_promise, &mut init_body)?
                }
                _ => {
                    for item in items {
                        self.lower_top_item(item, &mut init_body)?;
                    }
                }
            }
        }
        init_body.push(Stmt::Return(None));
        let ctx = std::mem::take(&mut self.cur);
        self.m.local_names.insert(init_id, ctx.names);
        self.m.funcs[init_id].locals = ctx.locals;
        self.m.funcs[init_id].body = init_body;
        self.m.init = Some(init_id);
        // 3. Every function body. Lowering can declare more functions (specialisations of
        // generics): go on until none is left. A plain top-level function that nothing lowered so
        // far refers to is left for last and only lowered once something does (an unused one with
        // a construct the compiler lacks must not fail the build). Methods, constructors,
        // exported functions and the prelude's helpers are always lowered: they can be reached
        // through classes, tables and the host.
        let mut scanned = vec![false; self.fstate.len()];
        let mut referenced: HashSet<FuncId> = HashSet::new();
        loop {
            scanned.resize(self.fstate.len(), false);
            let mut names: Vec<String> = self
                .fids
                .iter()
                .filter(|(n, id)| self.fstate[**id] == 0 && !self.skippable(n))
                .map(|(n, _)| n.clone())
                .collect();
            if names.is_empty() {
                // Nothing else to lower: see what the lowered code refers to.
                for (i, f) in self.m.funcs.iter().enumerate() {
                    if self.fstate[i] == 2 && !scanned[i] {
                        scanned[i] = true;
                        collect_callees(f, &mut referenced);
                    }
                }
                names = self
                    .fids
                    .iter()
                    .filter(|(n, id)| {
                        self.fstate[**id] == 0 && self.skippable(n) && referenced.contains(*id)
                    })
                    .map(|(n, _)| n.clone())
                    .collect();
                if names.is_empty() {
                    // Interface tables take what the code lowered so far assigned to them; that can
                    // lower and declare more.
                    if self.finish_ifaces()? {
                        continue;
                    }
                    // `--check-unused`: the program's own functions nothing calls, so their errors
                    // show now (the prelude's helpers stay skipped).
                    if self.check_unused {
                        names = self
                            .fids
                            .iter()
                            .filter(|(n, id)| {
                                self.fstate[**id] == 0
                                    && self.skippable(n)
                                    && !self.in_prelude(self.fsrc[n.as_str()].params.span.start)
                            })
                            .map(|(n, _)| n.clone())
                            .collect();
                    }
                    if names.is_empty() {
                        break;
                    }
                }
            }
            names.sort_by_key(|n| self.fids[n]);
            for n in names {
                self.ensure_lowered(&n)?;
            }
        }
        // The functions left over are never called: an empty body keeps the module well formed.
        for (f, state) in self.m.funcs.iter_mut().zip(&self.fstate) {
            if *state == 0 {
                f.body = vec![Stmt::Return(None)];
            }
        }
        self.fixup_generic_itables();
        self.fill_empty_tables()?;
        self.check_iface_structs()?;
        Ok(())
    }

    fn collect(
        &mut self,
        d: &'a Declaration<'a>,
        exported: bool,
        vars: &mut Vec<TopItem<'a>>,
    ) -> R<()> {
        match d {
            Declaration::VariableDeclaration(v) => self.collect_vars(v, exported, vars)?,
            Declaration::FunctionDeclaration(f) => self.collect_fn(f, exported)?,
            Declaration::ClassDeclaration(c) if c.type_parameters.is_some() => {}
            Declaration::ClassDeclaration(c) => {
                self.collect_class(c)?;
                vars.push(TopItem::Class(c));
            }
            Declaration::TSInterfaceDeclaration(d) => {
                self.ifaces.insert(d.id.name.to_string(), d);
            }
            Declaration::TSTypeAliasDeclaration(d) => {
                self.aliases.insert(d.id.name.to_string(), d);
            }
            _ => {}
        }
        Ok(())
    }

    fn collect_fn(&mut self, f: &'a Function<'a>, exported: bool) -> R<()> {
        let Some(id) = &f.id else {
            bail!("anonymous function")
        };
        let Some(body) = &f.body else { return Ok(()) };
        if f.type_parameters.is_some() {
            if exported {
                bail!(
                    "{}: a generic function cannot be exported; export a specialisation",
                    id.name
                )
            }
            self.generic_fns.insert(
                id.name.to_string(),
                generics::GenericFn {
                    tparams: generics::tparam_list(f.type_parameters.as_deref()),
                    params: &f.params,
                    ret: f.return_type.as_ref().map(|r| &r.type_annotation),
                    body: FnBody::Block(body),
                },
            );
            return Ok(());
        }
        let src = FnSrc {
            params: &f.params,
            ret: f.return_type.as_ref().map(|r| &r.type_annotation),
            body: FnBody::Block(body),
            exported,
            kind: FnKind::Free,
            ret_ty: None,
            tenv: None,
        };
        self.declare_fn(id.name.as_str(), src)
    }

    fn collect_vars(
        &mut self,
        v: &'a VariableDeclaration<'a>,
        exported: bool,
        vars: &mut Vec<TopItem<'a>>,
    ) -> R<()> {
        let is_const = v.kind == VariableDeclarationKind::Const;
        for d in &v.declarations {
            // `const f = (..) => ..` is a function declaration in disguise.
            if let (
                BindingPattern::BindingIdentifier(id),
                Some(Expression::ArrowFunctionExpression(a)),
                None,
            ) = (&d.id, &d.init, &d.type_annotation)
                // A `let` may be assigned another function later: it is a variable.
                && is_const
            {
                let body = match &a.body {
                    ArrowFunctionBody::FunctionBody(b) => FnBody::Block(b),
                    other => FnBody::Expr(other.as_expression().ok_or("arrow body")?),
                };
                if a.type_parameters.is_some() {
                    self.generic_fns.insert(
                        id.name.to_string(),
                        generics::GenericFn {
                            tparams: generics::tparam_list(a.type_parameters.as_deref()),
                            params: &a.params,
                            ret: a.return_type.as_ref().map(|r| &r.type_annotation),
                            body,
                        },
                    );
                    continue;
                }
                let src = FnSrc {
                    params: &a.params,
                    ret: a.return_type.as_ref().map(|r| &r.type_annotation),
                    body,
                    exported,
                    kind: FnKind::Free,
                    ret_ty: None,
                    tenv: None,
                };
                self.declare_fn(id.name.as_str(), src)?;
                continue;
            }
            vars.push(TopItem::Var(d, exported, is_const));
        }
        Ok(())
    }

    pub(crate) fn declare_fn(&mut self, name: &str, src: FnSrc<'a>) -> R<()> {
        let env = src.tenv.clone().unwrap_or_default();
        let saved = std::mem::replace(&mut self.tenv, env);
        let r = self.declare_fn_in_env(name, src);
        self.tenv = saved;
        r
    }

    fn declare_fn_in_env(&mut self, name: &str, src: FnSrc<'a>) -> R<()> {
        let mut ptys = Vec::new();
        if let FnKind::Method(sid) | FnKind::Ctor(sid, _) = src.kind {
            ptys.push(Ty::Obj(sid));
        }
        for p in &src.params.items {
            // `phase = 0`, `color = Color.white`: the type of the default value.
            let ty = match (&p.type_annotation, &p.initializer) {
                (Some(ann), _) => self.ty(&ann.type_annotation, name)?,
                (None, Some(init)) => {
                    self.field_ty(None, false, Some(init), name).map_err(|_| {
                        format!(
                            "{name}: parameter without a type (its default value is not a literal)"
                        )
                    })?
                }
                (None, None) => bail!("{name}: parameter without a type"),
            };
            // `x?: T` without a default is `T | undefined` inside.
            ptys.push(if p.optional && p.initializer.is_none() {
                ty.nullable()
            } else {
                ty
            });
        }
        if let Some(r) = &src.params.rest {
            let Some(ann) = &r.type_annotation else {
                bail!("{name}: rest parameter without a type")
            };
            let ty = self.ty(&ann.type_annotation, name)?;
            // An array, or a tuple (`...args: [string, number]`: the arguments make the tuple).
            if !matches!(ty, Ty::Arr(_)) && !matches!(&ty, Ty::Obj(s) if self.is_tuple(*s)) {
                bail!("{name}: a rest parameter must be an array type")
            }
            ptys.push(ty);
        }
        let ret = match (&src.ret_ty, src.ret) {
            (Some(t), _) => Some(t.clone()),
            (None, Some(r)) => Some(self.ty(r, name)?),
            (None, None) => None,
        };
        let id = self.m.funcs.len();
        self.m.funcs.push(Func {
            name: name.to_string(),
            exported: src.exported,
            params: vec![],
            ret: Ty::Void,
            locals: vec![],
            body: vec![],
        });
        self.fsig.push((ptys, ret));
        self.fstate.push(0);
        self.fids.insert(name.to_string(), id);
        self.fsrc.insert(name.to_string(), src);
        Ok(())
    }

    /// Lowers a function body now if it has not been yet (call sites need an inferred return type).
    pub(crate) fn ensure_lowered(&mut self, name: &str) -> R<()> {
        let id = self.fids[name];
        match self.fstate[id] {
            2 => return Ok(()),
            1 => {
                if self.fsig[id].1.is_none() {
                    // `const sort = (f) => { ..; f.kids.forEach(sort); }`: no `return value`, so
                    // it returns nothing, as TypeScript infers.
                    if let FnBody::Block(b) = self.fsrc[name].body
                        && !stmt::returns_a_value(b)
                    {
                        self.fsig[id].1 = Some(Ty::Void);
                        return Ok(());
                    }
                    // A component that renders itself (`<FileTree>` in `FileTree`): what a JSX
                    // element is, `UiElement` (TypeScript's `JSX.Element`).
                    if name.starts_with(|c: char| c.is_ascii_uppercase())
                        && self.class_decls.contains_key("StyleEntry")
                    {
                        self.fsig[id].1 = Some(Ty::Handle("UiElement".into()));
                        return Ok(());
                    }
                    bail!("{name}: recursive function needs a return type annotation");
                }
                return Ok(());
            }
            _ => {}
        }
        self.fstate[id] = 1;
        let saved = std::mem::take(&mut self.cur);
        // Closures inside this function decide which of its variables live in cells.
        let mut scan = match self.fsrc[name].body {
            FnBody::Block(b) => scan::Scan::of_statements(&b.statements),
            FnBody::Expr(e) => scan::Scan::of_expression(e),
        };
        // A constructor also runs the field initialisers (`handler = () => this.n`).
        if let FnKind::Ctor(sid, _) = self.fsrc[name].kind {
            let class = self.class_decls[&self.class_of_sid[&sid]];
            for el in &class.body.body {
                if let ClassElement::PropertyDefinition(p) = el
                    && !p.r#static
                    && let Some(v) = &p.value
                {
                    let extra = scan::Scan::of_expression(v);
                    scan.captured.extend(extra.captured);
                    scan.free.extend(extra.free);
                }
            }
        }
        let saved_captured = std::mem::replace(&mut self.captured, scan.captured);
        let saved_assigned = std::mem::replace(&mut self.assigned, scan.assigned);
        self.closure_free.extend(scan.free);
        let env = self.fsrc[name].tenv.clone().unwrap_or_default();
        let saved_env = std::mem::replace(&mut self.tenv, env);
        let r = self.lower_fn(name, id);
        self.tenv = saved_env;
        self.captured = saved_captured;
        self.assigned = saved_assigned;
        self.cur = saved;
        r.map_err(|e| {
            if std::env::var_os("PLATYPUS_TS_TRACE_FN").is_some() {
                format!("{e} [in {name}]")
            } else {
                e
            }
        })?;
        self.fstate[id] = 2;
        Ok(())
    }

    fn lower_fn(&mut self, name: &str, id: FuncId) -> R<()> {
        let (params, body, kind) = {
            let s = &self.fsrc[name];
            (s.params, s.body, s.kind)
        };
        let exp_ret = self.fsig[id].1.clone();
        self.cur = FnCtx {
            ret_inferred: exp_ret.is_none(),
            ret: exp_ret,
            cls: match kind {
                FnKind::Method(s) | FnKind::Ctor(s, _) => Some((self.class_of_sid[&s].clone(), s)),
                FnKind::Free => None,
            },
            ..FnCtx::default()
        };
        self.cur.scopes.push(HashMap::new());
        let ptys = self.fsig[id].0.clone();
        let mut pvars = Vec::new();
        let mut pro = Vec::new();
        let mut ptys = ptys.into_iter();
        if matches!(kind, FnKind::Method(_) | FnKind::Ctor(..)) {
            let ty = ptys.next().expect("a method has a this type");
            let v = self.new_local(ty);
            self.bind_var("this", v, &mut pro);
            pvars.push(v);
        }
        for (p, ty) in params.items.iter().zip(ptys.by_ref()) {
            if let BindingPattern::BindingIdentifier(id) = &p.pattern
                && ty.strip() == &Ty::Lit
                && let Some(a) = &p.type_annotation
                && let Some(ms) = self.lit_members_opt(&a.type_annotation)
            {
                self.cur.lit_sets.insert(id.name.to_string(), ms);
            }
            let v = self.new_local(ty);
            self.bind_param(&p.pattern, v, &mut pro)?;
            pvars.push(v);
        }
        if let (Some(r), Some(ty)) = (&params.rest, ptys.next()) {
            let v = self.new_local(ty);
            self.bind_param(&r.rest.argument, v, &mut pro)?;
            pvars.push(v);
        }
        if let FnKind::Ctor(sid, implicit) = kind {
            let class = self.class_of_sid[&sid].clone();
            self.ctor_prologue(&class, implicit, &pvars, &mut pro)?;
        }
        let (stmts, ret) = self.lower_body_into(body, pro)?;
        self.fsig[id].1 = Some(ret.clone());
        let ctx = std::mem::take(&mut self.cur);
        self.m.local_names.insert(id, ctx.names);
        let f = &mut self.m.funcs[id];
        f.params = pvars;
        f.ret = ret;
        f.locals = ctx.locals;
        f.body = stmts;
        Ok(())
    }

    // ---- locals and scopes ----

    pub(crate) fn new_local(&mut self, ty: Ty) -> VarId {
        self.cur.locals.push(ty);
        (self.cur.locals.len() - 1) as VarId
    }

    pub(crate) fn new_label(&mut self) -> LabelId {
        self.cur.next_label += 1;
        self.cur.next_label
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<Binding> {
        self.cur
            .scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name).cloned())
    }

    /// Names the local `var` (which already holds the value). A variable that closures capture
    /// moves into a new cell first, in `out`.
    pub(crate) fn bind_var(&mut self, name: &str, var: VarId, out: &mut Vec<Stmt>) {
        let scope = self.cur.scopes.len() - 1;
        if self.captured.contains(name) {
            let ty = self.cur.locals[var as usize].clone();
            let cell = self.new_local(Ty::Cell(Box::new(ty.clone())));
            self.cur.names.push((cell, name.to_string()));
            let cell_ty = self.cur.locals[cell as usize].clone();
            let init = Expr {
                kind: ExprKind::NewCell(Box::new(self.var_expr(var))),
                ty: cell_ty,
            };
            out.push(Stmt::Assign(cell, init));
            self.cur.scopes[scope].insert(name.to_string(), Binding::Boxed(cell, ty));
        } else {
            self.cur.names.push((var, name.to_string()));
            self.cur.scopes[scope].insert(name.to_string(), Binding::Direct(var));
        }
    }

    /// The current value of a name.
    pub(crate) fn read_binding(&self, b: &Binding) -> Expr {
        match b {
            Binding::Direct(v) => self.var_expr(*v),
            Binding::Boxed(cell, ty) => Expr {
                kind: ExprKind::CellGet(Box::new(self.var_expr(*cell))),
                ty: ty.clone(),
            },
        }
    }

    /// Lowers a function body in the current context (parameters already bound; `pro` holds the
    /// statements that set them up). Returns the statements and the return type.
    pub(crate) fn lower_body_into(
        &mut self,
        body: FnBody<'a>,
        pro: Vec<Stmt>,
    ) -> R<(Vec<Stmt>, Ty)> {
        let mut stmts = pro;
        match body {
            FnBody::Block(b) => self.lower_stmts(&b.statements, &mut stmts)?,
            FnBody::Expr(e) => {
                let exp = self.cur.ret.clone();
                let ex = self.lower_expr(e, exp.as_ref())?;
                if exp == Some(Ty::Void) || (exp.is_none() && ex.ty == Ty::Void) {
                    // A void call as the body of an arrow whose return type is inferred.
                    self.cur.ret = Some(Ty::Void);
                    stmts.push(Stmt::Eval(ex));
                } else {
                    let ex = match exp {
                        Some(t) => self.coerce(ex, &t)?,
                        None if self.cur.widen_ret => self.widen_lit(ex),
                        None => ex,
                    };
                    self.cur.ret = Some(ex.ty.clone());
                    stmts.push(Stmt::Return(Some(ex)));
                }
            }
        }
        let ret = self.cur.ret.clone().unwrap_or(Ty::Void);
        if ret != Ty::Void && self.cur.bare_returns {
            // `if (x) return; ... return cleanup;`: the bare returns hand back null.
            fn patch(stmts: &mut [Stmt], ret: &Ty) {
                for s in stmts {
                    match s {
                        Stmt::Return(r @ None) => {
                            *r = Some(Expr {
                                kind: ExprKind::Null,
                                ty: ret.clone(),
                            });
                        }
                        Stmt::If(_, a, b) => {
                            patch(a, ret);
                            patch(b, ret);
                        }
                        Stmt::Block(_, b) | Stmt::Loop(_, b) => patch(b, ret),
                        Stmt::Try(a, _, b) => {
                            patch(a, ret);
                            patch(b, ret);
                        }
                        _ => {}
                    }
                }
            }
            patch(&mut stmts, &ret);
        }
        if ret == Ty::Void {
            stmts.push(Stmt::Return(None));
        } else {
            // Falling off the end of a non-void function: unreachable under tsc, keep codegen total.
            let z = self.default_value(&ret);
            stmts.push(Stmt::Return(Some(z)));
        }
        Ok((stmts, ret))
    }

    pub(crate) fn var_expr(&self, v: VarId) -> Expr {
        Expr {
            kind: ExprKind::Var(v),
            ty: self.cur.locals[v as usize].clone(),
        }
    }

    pub(crate) fn default_value(&self, ty: &Ty) -> Expr {
        let kind = match ty {
            Ty::Num => ExprKind::Num(0.0),
            Ty::Bool => ExprKind::Bool(false),
            Ty::Lit => ExprKind::Tag(0),
            _ => ExprKind::Null,
        };
        Expr {
            kind,
            ty: ty.clone(),
        }
    }

    /// A top-level `const` / `let`; an error that has no line gets this declaration's.
    fn lower_top_var(
        &mut self,
        d: &'a VariableDeclarator<'a>,
        is_const: bool,
        init_body: &mut Vec<Stmt>,
    ) -> R<()> {
        // Closures inside the initialiser decide which of the variables of closures around them
        // live in cells (`times(.., (c) => { const p = ..; sprites.emit({ anim: (t) => p.x }) })`).
        let saved_captured = d.init.as_ref().map(|init| {
            let sc = scan::Scan::of_expression(init);
            self.closure_free.extend(sc.free);
            std::mem::replace(&mut self.captured, sc.captured)
        });
        let r = self.lower_top_var_inner(d, is_const, init_body);
        if let Some(c) = saved_captured {
            self.captured = c;
        }
        r.map_err(|e| {
            if e.contains("line ") {
                e
            } else {
                format!("{}: {e}", self.loc(oxc_span::GetSpan::span(d).start))
            }
        })
    }

    fn lower_top_item(&mut self, item: TopItem<'a>, init_body: &mut Vec<Stmt>) -> R<()> {
        match item {
            TopItem::Var(decl, _exported, is_const) => {
                self.lower_top_var(decl, is_const, init_body)
            }
            TopItem::Class(c) => self.lower_class_statics(c, init_body),
            // Closures inside decide which variables of the closures around them live in cells
            // (`test("x", () => { let s = 1; const f = () => { s++; }; })`), as for a `const`.
            TopItem::Expr(e) => {
                let sc = scan::Scan::of_expression(e);
                self.closure_free.extend(sc.free);
                let saved = std::mem::replace(&mut self.captured, sc.captured);
                let v = self.lower_expr(e, None);
                self.captured = saved;
                init_body.push(Stmt::Eval(v?));
                Ok(())
            }
            TopItem::Stmt(st) => {
                let sc = scan::Scan::of_statements(std::slice::from_ref(st));
                self.closure_free.extend(sc.free);
                let saved = std::mem::replace(&mut self.captured, sc.captured);
                let r = self.lower_stmts(std::slice::from_ref(st), init_body);
                self.captured = saved;
                r
            }
            TopItem::Module(_) => Ok(()),
        }
    }

    /// A module that awaits or waits for one that does: its statements become the body of a
    /// closure `(env, p) => { await dependencies; .. }` that `__async$number` runs as a coroutine.
    /// The promise is kept in a module variable for the modules that import this one, and
    /// `__modWatch` reports its start and end (or failure) to the host's startup bookkeeping.
    fn lower_async_module(
        &mut self,
        info: &ModuleInfo,
        items: Vec<TopItem<'a>>,
        promises: &mut HashMap<usize, GlobalId>,
        init_body: &mut Vec<Stmt>,
    ) -> R<()> {
        let outer = std::mem::take(&mut self.cur);
        self.cur.scopes.push(HashMap::new());
        let env = self.new_local(Self::env_ty());
        let promise = self.named_type("Promise$number")?;
        let p = self.new_local(promise.clone());
        let mut body = Vec::new();
        // Wait for the dependencies that are still running, in import order.
        for dep in &info.deps {
            let Some(&g) = promises.get(dep) else {
                continue; // an import cycle: the module comes later in the bundle
            };
            let q = mk_expr(ExprKind::GlobalVar(g), promise.clone());
            let wait = self.call_helper("__await$number", vec![q])?;
            body.push(Stmt::Eval(wait));
        }
        let mut result = Ok(());
        for item in items {
            result = self.lower_top_item(item, &mut body);
            if result.is_err() {
                break;
            }
        }
        body.push(Stmt::Return(None));
        let ctx = std::mem::replace(&mut self.cur, outer);
        result?;
        let main_id = self.m.funcs.len();
        self.m.funcs.push(Func {
            name: format!("tla$module{}", info.id),
            exported: false,
            params: vec![env, p],
            ret: Ty::Void,
            locals: ctx.locals,
            body,
        });
        self.fsig.push((vec![promise.clone()], Some(Ty::Void)));
        self.fstate.push(2);
        let clo = mk_expr(
            ExprKind::MakeClosure(main_id, None),
            Ty::Fn(Box::new(FnTy {
                params: vec![promise.clone()],
                ret: Ty::Void,
            })),
        );
        let started = self.call_helper("__async$number", vec![clo])?;
        let g = self.m.globals.len();
        self.m.globals.push(Global {
            name: format!("__module{}", info.id),
            ty: promise.clone(),
        });
        promises.insert(info.id, g);
        init_body.push(Stmt::StoreGlobal(g, started));
        let name = self.str_lit(&info.name);
        let watched = mk_expr(ExprKind::GlobalVar(g), promise);
        let watch = self.call_helper("__modWatch", vec![name, watched])?;
        init_body.push(Stmt::Eval(watch));
        Ok(())
    }

    /// `const [a, b] = pair();` / `const { x, y } = o;` at the top level: every name becomes a
    /// module variable, set by the statements that take the value apart.
    fn lower_top_destructure(
        &mut self,
        d: &'a VariableDeclarator<'a>,
        is_const: bool,
        init_body: &mut Vec<Stmt>,
    ) -> R<()> {
        let Some(init) = &d.init else {
            bail!("a destructuring declaration needs a value")
        };
        let exp = match &d.type_annotation {
            Some(a) => Some(self.ty(&a.type_annotation, "destructuring")?),
            None => None,
        };
        let mut value = self.lower_expr(init, exp.as_ref())?;
        if let Some(t) = &exp {
            value = self.coerce(value, t)?;
        }
        let mut stmts = Vec::new();
        self.bind_pattern(&d.id, value, &mut stmts)?;
        let mut names = Vec::new();
        pattern_names(&d.id, &mut names);
        for name in names {
            // The pattern bound the name to a local of the init function; the module variable
            // takes its place, so functions (and the code after) see the same one.
            let Some(b) = self.cur.scopes[0].remove(&name) else {
                continue;
            };
            let read = self.read_binding(&b);
            let g = self.m.globals.len();
            self.m.globals.push(Global {
                name: name.clone(),
                ty: read.ty.clone(),
            });
            self.consts
                .insert(name, ConstInfo::Global(g, read.ty.clone(), !is_const));
            stmts.push(Stmt::StoreGlobal(g, read));
        }
        init_body.extend(stmts);
        Ok(())
    }

    fn lower_top_var_inner(
        &mut self,
        d: &'a VariableDeclarator<'a>,
        is_const: bool,
        init_body: &mut Vec<Stmt>,
    ) -> R<()> {
        let BindingPattern::BindingIdentifier(id) = &d.id else {
            return self.lower_top_destructure(d, is_const, init_body);
        };
        let name = id.name.to_string();
        let mut exp = match &d.type_annotation {
            Some(a) => Some(self.ty(&a.type_annotation, &name)?),
            None => None,
        };
        // `const sfx = { play() { sfx.preload(); }, preload(): void {..} }`: the methods name the
        // object they are in, so its type is made from their written signatures first.
        let mut reserved: Option<GlobalId> = None;
        if exp.is_none()
            && is_const
            && let Some(Expression::ObjectExpression(o)) = &d.init
            && self.refers_to_itself(o, &name)
            && let Some(sid) = self.object_shape(o)?
        {
            let ty = Ty::Obj(sid);
            let g = self.m.globals.len();
            self.m.globals.push(Global {
                name: name.clone(),
                ty: ty.clone(),
            });
            self.consts
                .insert(name.clone(), ConstInfo::Global(g, ty.clone(), false));
            reserved = Some(g);
            exp = Some(ty);
        }
        let e = match (&d.init, &exp) {
            (Some(init), _) => {
                let mut e = self.lower_expr(init, exp.as_ref())?;
                if let Some(t) = &exp {
                    e = self.coerce(e, t)?;
                } else if !is_const {
                    // `let s = "x"` is a `string`.
                    e = self.widen_lit(e);
                }
                e
            }
            // `let x: T;` starts as null / 0, like a field nobody set.
            (None, Some(t)) if !is_const => self.default_value(t),
            _ => bail!("{name}: top-level binding without a value"),
        };
        // Only a `const` may be folded into its uses; a `let` can change.
        if is_const
            && matches!(
                e.kind,
                ExprKind::Num(_) | ExprKind::Bool(_) | ExprKind::Tag(_)
            )
        {
            self.consts.insert(name, ConstInfo::Inline(e));
            return Ok(());
        }
        if matches!(e.ty, Ty::Null) {
            bail!("{name}: a variable initialised with null needs a type annotation")
        }
        if let Some(g) = reserved {
            init_body.push(Stmt::StoreGlobal(g, e));
            return Ok(());
        }
        let g = self.m.globals.len();
        self.m.globals.push(Global {
            name: name.clone(),
            ty: e.ty.clone(),
        });
        self.consts
            .insert(name, ConstInfo::Global(g, e.ty.clone(), !is_const));
        init_body.push(Stmt::StoreGlobal(g, e));
        Ok(())
    }
}

/// The names a binding pattern declares.
fn pattern_names(p: &BindingPattern<'_>, out: &mut Vec<String>) {
    match p {
        BindingPattern::BindingIdentifier(id) => out.push(id.name.to_string()),
        BindingPattern::AssignmentPattern(a) => pattern_names(&a.left, out),
        BindingPattern::ObjectPattern(o) => {
            for prop in &o.properties {
                pattern_names(&prop.value, out);
            }
            if let Some(r) = &o.rest {
                pattern_names(&r.argument, out);
            }
        }
        BindingPattern::ArrayPattern(a) => {
            for el in a.elements.iter().flatten() {
                pattern_names(el, out);
            }
            if let Some(r) = &a.rest {
                pattern_names(&r.argument, out);
            }
        }
    }
}

/// Every function `f`'s body calls or makes a closure of.
fn collect_callees(f: &Func, out: &mut HashSet<FuncId>) {
    let found = std::cell::RefCell::new(Vec::new());
    for s in &f.body {
        crate::frames::scan_stmt(
            s,
            &|e| {
                if let ExprKind::Call(id, _) | ExprKind::MakeClosure(id, _) = &e.kind {
                    found.borrow_mut().push(*id);
                }
                false
            },
            &|_| false,
        );
    }
    out.extend(found.into_inner());
}
