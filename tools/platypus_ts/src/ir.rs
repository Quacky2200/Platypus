// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Platypus IR: a small typed, structured IR between the TypeScript AST and Cranelift.
//!
//! Structured on purpose (blocks, loops, labelled break/continue instead of a CFG): lowering
//! from TypeScript stays simple, and `cranelift-frontend` builds the CFG in one pass. Every
//! expression carries its type, so codegen never guesses a machine type.
//!
//! Value representation (all object fields live in 8-byte slots, see `docs/research/platypus-ts-spike.md`):
//! - `Num` f64, `Bool` i8, `Lit` (string-literal union member) i32, everything else a 64-bit pointer.
//! - `Nullable(ptr)` is the pointer with 0 as null; `Nullable(Num)` is an f64 with one reserved NaN
//!   payload as null (`NULL_NUM_BITS`), so arithmetic on a narrowed value needs no unboxing.

use std::fmt;

pub type StructId = usize;
pub type FuncId = usize;
pub type VarId = u32;
pub type GlobalId = usize;
pub type LabelId = u32;

/// The f64 bit pattern that means `null` / `undefined` in a `number | null` slot.
pub const NULL_NUM_BITS: u64 = 0x7FF8_DEAD_BEEF_0001;

/// The element type of a typed array (`Int8Array` .. `Float64Array`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypedKind {
    I8,
    U8,
    /// `Uint8ClampedArray`: stores clamp to 0..255 and round half to even.
    U8C,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl TypedKind {
    pub const ALL: [TypedKind; 9] = [
        TypedKind::I8,
        TypedKind::U8,
        TypedKind::U8C,
        TypedKind::I16,
        TypedKind::U16,
        TypedKind::I32,
        TypedKind::U32,
        TypedKind::F32,
        TypedKind::F64,
    ];

    /// Bytes per element.
    pub fn size(self) -> usize {
        match self {
            TypedKind::I8 | TypedKind::U8 | TypedKind::U8C => 1,
            TypedKind::I16 | TypedKind::U16 => 2,
            TypedKind::I32 | TypedKind::U32 | TypedKind::F32 => 4,
            TypedKind::F64 => 8,
        }
    }

    /// The constructor's name.
    pub fn name(self) -> &'static str {
        match self {
            TypedKind::I8 => "Int8Array",
            TypedKind::U8 => "Uint8Array",
            TypedKind::U8C => "Uint8ClampedArray",
            TypedKind::I16 => "Int16Array",
            TypedKind::U16 => "Uint16Array",
            TypedKind::I32 => "Int32Array",
            TypedKind::U32 => "Uint32Array",
            TypedKind::F32 => "Float32Array",
            TypedKind::F64 => "Float64Array",
        }
    }

    pub fn from_name(name: &str) -> Option<TypedKind> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The number the runtime and the generated code pass for this kind.
    pub fn code(self) -> u8 {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0) as u8
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Void,
    Num,
    Bool,
    /// A member of a string-literal union, as a small integer (the id is global, see `Module::tags`).
    Lit,
    Str,
    /// An arbitrary-precision integer: a leaf GC object.
    BigInt,
    /// The type of the `null` literal before it is coerced to a nullable type.
    Null,
    Obj(StructId),
    Arr(Box<Ty>),
    /// A typed array: the array header with `kind.size()` bytes per element.
    Typed(TypedKind),
    Fn(Box<FnTy>),
    /// A variable shared by closures: a heap box holding one value of the inner type.
    Cell(Box<Ty>),
    Nullable(Box<Ty>),
    /// An engine object named by a 64-bit id (`Entity`): a plain integer, never a GC pointer.
    Handle(String),
    /// A `Map<K, V>` (and `WeakMap`): a runtime hash table.
    Map(Box<Ty>, Box<Ty>),
    /// A `Set<K>` (and `WeakSet`).
    Set(Box<Ty>),
    /// Reserved: any dynamic value (not produced by the spike's lowering yet).
    JsValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnTy {
    pub params: Vec<Ty>,
    pub ret: Ty,
}

impl Ty {
    pub fn nullable(self) -> Ty {
        match self {
            Ty::Nullable(_) | Ty::Null => self,
            t => Ty::Nullable(Box::new(t)),
        }
    }
    /// Strips `Nullable` (same representation).
    pub fn strip(&self) -> &Ty {
        match self {
            Ty::Nullable(t) => t.strip(),
            t => t,
        }
    }
    pub fn is_nullable(&self) -> bool {
        matches!(self, Ty::Nullable(_) | Ty::Null)
    }
    pub fn is_ptr(&self) -> bool {
        matches!(
            self.strip(),
            Ty::Str
                | Ty::Obj(_)
                | Ty::Arr(_)
                | Ty::Typed(_)
                | Ty::BigInt
                | Ty::Fn(_)
                | Ty::Cell(_)
                | Ty::Map(..)
                | Ty::Set(_)
                | Ty::JsValue
        )
    }
    pub fn is_num(&self) -> bool {
        matches!(self.strip(), Ty::Num)
    }
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub ty: Ty,
    /// Declared with `?`: `JSON.stringify` leaves it out when it is null.
    pub optional: bool,
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Field>,
}

impl StructDef {
    pub fn field(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
}

/// Byte offset of field `idx`: one 8-byte header word (the struct id + 1), then 8-byte slots.
pub fn field_offset(idx: usize) -> i32 {
    8 + 8 * idx as i32
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    /// JS `%` (fmod).
    Rem,
    /// Bit operations on numbers: operands go through ToInt32, results come back as f64.
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    /// `>>>`: unsigned 32-bit result.
    UShr,
    /// `Math.imul`.
    Imul,
    Min,
    Max,
    Atan2,
    Hypot,
    /// `**` and `Math.pow`, with JavaScript's special cases.
    Pow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
    Sqrt,
    Abs,
    Floor,
    Sin,
    Cos,
    /// `Math.sign`.
    Sign,
    Ceil,
    Trunc,
    /// `Math.round`: halves go toward +Infinity, and the sign of zero is kept.
    Round,
    /// `Math.fround`: through a 32-bit float.
    Fround,
    /// `Math.clz32`.
    Clz32,
    /// A C-library function of one number, by runtime symbol (`ts_rt_tan`, ...).
    Libm(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Calls into `platypus_ts_rt` that are not inlined.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RtFn {
    /// A string function of the runtime by symbol (`ts_rt_str_slice_gc`): arguments and result
    /// are typed by the call. A name ending in `_gc` may collect.
    Str(&'static str),
    Alloc,
    ArrNew,
    ArrPush,
    /// `Math.random()`: no arguments, a number in [0, 1).
    Random,
    /// `(array, length)`: `xs.length = n`, `pop`.
    ArrSetLen,
    /// `(array, index, value)`: insert one element (`unshift`).
    ArrInsert,
    /// `(array, start, count)`: remove elements (`shift`).
    ArrDrop,
    /// `(array, start, count, items or null)`: returns the removed elements as a new array.
    ArrSplice,
    /// `(array, start, end)`: a new array.
    ArrSlice,
    /// `(dst, src)`: append all of src.
    ArrAppend,
    ArrReverse,
    /// `(array, comparator closure)`: stable, in place. Runs script code, so it can collect.
    ArrSort,
    /// `(array, value, start, end)`.
    ArrFill,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Ty,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Num(f64),
    Bool(bool),
    /// A string-literal union member (index into `Module::tags`).
    Tag(u32),
    /// A string literal as a `string`: the address of a static string (index into `Module::strs`).
    Str(u32),
    /// A value as the raw 64-bit word the runtime's collections store (a number's bits, a pointer, a
    /// boolean); `Unbits` takes it back, typed by the expression's type.
    Bits(Box<Expr>),
    Unbits(Box<Expr>),
    /// The address of a blob (index into `Module::blobs`).
    Blob(u32),
    /// A union member (a `Tag`) as a `string`: a lookup in the table of the tags' texts.
    TagStr(Box<Expr>),
    /// A `string` as a union member: the index of the tag with that text, 0 (no tag) when there
    /// is none. For text that comes from outside (an engine property, a map's keys).
    StrTag(Box<Expr>),
    /// `null`: a zero pointer or the null NaN, depending on `ty`.
    Null,
    Var(VarId),
    /// A module-level `const`: written once by `ts_init`.
    Global(GlobalId),
    /// A module-level `let`/`var`: any call or `StoreGlobal` may replace its value.
    GlobalVar(GlobalId),
    Un(UnOp, Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    /// Short-circuit `a && b` / `a || b` on bools.
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    /// `cond ? a : b`, evaluating one side only.
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// True when a nullable value is null.
    IsNull(Box<Expr>),
    /// Reinterpret between representation-identical types (`T | null` <-> `T`, `Null` -> nullable).
    Cast(Box<Expr>),
    /// A number as a bool (`x !== 0 && !NaN`).
    Truthy(Box<Expr>),
    Field(Box<Expr>, StructId, usize),
    /// `arr[i]`: the element, or null / NaN-null when out of range. Bytes yield a Num (0 out of range).
    Index(Box<Expr>, Box<Expr>),
    Len(Box<Expr>),
    NewObj(StructId),
    Call(FuncId, Vec<Expr>),
    CallClosure(Box<Expr>, Vec<Expr>),
    Rt(RtFn, Vec<Expr>),
    /// Statements, then a value.
    Seq(Vec<Stmt>, Box<Expr>),
    /// A new cell holding the value.
    NewCell(Box<Expr>),
    /// The value in a cell.
    CellGet(Box<Expr>),
    /// A call through a table of functions indexed by the receiver's struct id; the receiver is
    /// the first argument. See `Module::vtables`.
    CallVirtual(usize, Vec<Expr>),
    /// A call of an engine function (`Module::externs[i]`) through its `extern "C"` symbol.
    CallExtern(usize, Vec<Expr>),
    /// Like `CallExtern` for a function with a value result: the result's scalars are written
    /// into the fields of the object the last operand evaluates to (a fresh struct).
    CallExternInto(usize, Vec<Expr>, Box<Expr>),
    /// Like `CallExternInto`, with the result's scalars going into these number locals (a value
    /// that never became an object: see `sroa.rs`).
    CallExternOut(usize, Vec<Expr>, Vec<VarId>),
    /// Is the object (a class instance, or null) an instance of one of the structs in
    /// `Module::itables[i]`?
    InstanceOf(Box<Expr>, usize),
    /// A closure over function `FuncId`, whose first parameter is the environment: an array of
    /// cells (or null). The function's other parameters are the closure type's.
    MakeClosure(FuncId, Option<Box<Expr>>),
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Assign(VarId, Expr),
    StoreField(Expr, StructId, usize, Expr),
    StoreIndex(Expr, Expr, Expr),
    StoreGlobal(GlobalId, Expr),
    /// `cell = value`.
    CellSet(Expr, Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    /// `break(label)` leaves the block; falls out at its end.
    Block(LabelId, Vec<Stmt>),
    /// An infinite loop; `Continue(label)` jumps to its top, `Break(label)` leaves it.
    Loop(LabelId, Vec<Stmt>),
    Break(LabelId),
    Continue(LabelId),
    Return(Option<Expr>),
    Eval(Expr),
    /// `throw e`: records the exception and leaves for the innermost handler (or the caller).
    Throw(Expr),
    /// `try { body } catch (v) { handler }`: an exception thrown in `body`, by it or by a call it
    /// makes, runs the handler with the exception in `v`.
    Try(Vec<Stmt>, Option<VarId>, Vec<Stmt>),
    /// The following code comes from this line of the bundled source (debug info; emits no code).
    Line(u32),
}

#[derive(Clone, Debug)]
pub struct Func {
    pub name: String,
    pub exported: bool,
    pub params: Vec<VarId>,
    pub ret: Ty,
    /// The type of every local, params first. `VarId` indexes it.
    pub locals: Vec<Ty>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct Global {
    pub name: String,
    pub ty: Ty,
}

/// The implementations of one method for every concrete class below some class.
#[derive(Clone, Debug)]
pub struct VTable {
    pub entries: Vec<(StructId, FuncId)>,
}

/// A type at the engine boundary (the C ABI of a `ts_api` function).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtTy {
    Void,
    Bool,
    F64,
    F32,
    I32,
    U32,
    Handle(String),
    /// A value class (`Vector3`): passed as one scalar per field.
    Value(String),
    /// A `string`: a script string object in, and out through a buffer the call copies from.
    Str,
    /// A closure (`(a, b) => R`), passed as a pointer the engine keeps (and roots) and calls
    /// later as `func(env, a, b)`. The parameter types (scalars, strings, handles) then the result.
    Fn(Vec<ExtTy>, Box<ExtTy>),
    /// Any other script pointer (an object, array or closure), passed as it is: the engine
    /// stores it as a rooted opaque value.
    Ptr,
    /// An array of handles of this type; the engine reads the 64-bit ids.
    HandleArr(String),
    /// A `number[]`, read in place by the engine.
    F64Arr,
    /// A `string[]`: the engine copies the strings.
    StrArr,
    /// An array of a value class (`Vector3[]`): the engine reads each element's fields.
    ValueArr(String),
    /// An enum (`TangentMode`): a string to scripts, the index of the name (`i32`, -1 for none) on
    /// the wire. The lowering converts (`platypus_<Enum>_parse` / `_name`, or at compile time for a
    /// constant).
    Enum(String),
    /// A result record (`SplineHit`): a plain object of the manifest's fields. The call leaves its
    /// numbers in `ts_api`'s buffer and the lowering reads them with `ts_api_result`.
    Record(String),
    /// A result `Vec` of a value class: the call returns the count and leaves the numbers in the
    /// buffer.
    ValueList(String),
    /// A result `Vec` of records.
    RecordList(String),
    /// A result `Vec<String>`: the call returns the count and keeps the strings; the lowering
    /// reads them with `ts_api_result_str`.
    StrList,
    /// A result `Vec` of an enum: the count, and the indexes in the buffer.
    EnumList(String),
    /// A list of records that travels as JSON text (a parameter or a result): the record's name.
    RecordsJson(String),
    /// A result `Vec<f64>`: the count, and the numbers in the buffer.
    F64List,
    /// A result `Vec` of handles: the count, and the ids read with `ts_api_result_id`.
    HandleList(String),
    /// A value that may be `null`: `Option<f64>` (NaN is none), `Option<String>` (a result; the
    /// call leaves a flag read with `ts_api_result_none`) or `Option<V>` of a value class (NaN
    /// in every field is none).
    Opt(Box<ExtTy>),
}

impl ExtTy {
    /// Whether a parameter is a list a script passes as an array.
    pub fn is_list_arg(&self) -> bool {
        matches!(
            self,
            ExtTy::F64Arr
                | ExtTy::StrArr
                | ExtTy::ValueArr(_)
                | ExtTy::HandleArr(_)
                | ExtTy::RecordsJson(_)
        )
    }

    /// The IR type scripts see: every number is an `f64`.
    pub fn to_ir(&self) -> Ty {
        match self {
            ExtTy::Void => Ty::Void,
            ExtTy::Bool => Ty::Bool,
            ExtTy::F64 | ExtTy::F32 | ExtTy::I32 | ExtTy::U32 => Ty::Num,
            ExtTy::Handle(n) => Ty::Handle(n.clone()),
            ExtTy::Str => Ty::Str,
            ExtTy::Ptr => Ty::JsValue,
            ExtTy::HandleArr(n) => Ty::Arr(Box::new(Ty::Handle(n.clone()))),
            ExtTy::F64Arr => Ty::Arr(Box::new(Ty::Num)),
            ExtTy::StrArr => Ty::Arr(Box::new(Ty::Str)),
            // The element struct is made by the lowering (it knows the struct ids).
            ExtTy::ValueArr(_) => Ty::JsValue,
            ExtTy::Fn(ps, r) => Ty::Fn(Box::new(FnTy {
                params: ps.iter().map(|p| p.to_ir()).collect(),
                ret: r.to_ir(),
            })),
            ExtTy::Enum(_) => Ty::Str,
            // The structs are made by the lowering (it knows the struct ids).
            ExtTy::Value(_) | ExtTy::Record(_) => Ty::Void,
            ExtTy::ValueList(_) | ExtTy::RecordList(_) => Ty::JsValue,
            ExtTy::StrList | ExtTy::EnumList(_) => Ty::Arr(Box::new(Ty::Str)),
            ExtTy::F64List => Ty::Arr(Box::new(Ty::Num)),
            ExtTy::RecordsJson(_) => Ty::JsValue,
            ExtTy::HandleList(n) => Ty::Arr(Box::new(Ty::Handle(n.clone()))),
            ExtTy::Opt(inner) => inner.to_ir().nullable(),
        }
    }
}

/// An engine function the module calls.
#[derive(Clone, Debug)]
pub struct ExternDecl {
    pub symbol: String,
    /// The scalars of the C signature (a value class is already flattened into its fields).
    pub params: Vec<ExtTy>,
    pub ret: ExtTy,
    /// A value result comes back through a trailing pointer to this many scalars of this type.
    pub out: Option<(ExtTy, usize)>,
}

/// One `@property` field of a class.
#[derive(Clone, Debug)]
pub struct PropEntry {
    pub name: String,
    pub index: usize,
    pub ty: Ty,
}

/// The `@property` fields of a class (its base classes' first): what a host reads and writes to
/// save, load and edit a component without running any script.
#[derive(Clone, Debug)]
pub struct PropTable {
    pub class: StructId,
    pub entries: Vec<PropEntry>,
}

/// Bytes in the object file. `headers` are the offsets of 8-byte struct header words that were
/// written as struct id + 1: the module's `struct_base` is added at code generation, when it is
/// known.
#[derive(Clone, Debug, Default)]
pub struct Blob {
    pub bytes: Vec<u8>,
    pub headers: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub structs: Vec<StructDef>,
    pub globals: Vec<Global>,
    pub vtables: Vec<VTable>,
    pub externs: Vec<ExternDecl>,
    pub props: Vec<PropTable>,
    /// Per class struct: the struct and its ancestors, the class first.
    pub class_bases: Vec<(StructId, Vec<StructId>)>,
    /// For `instanceof`: the structs that count as instances.
    pub itables: Vec<Vec<StructId>>,
    pub funcs: Vec<Func>,
    /// String-literal union members; a `Tag` is an index here.
    pub tags: Vec<String>,
    /// String literals used as `string` values (static data in the object file), as WTF-8 bytes.
    pub strs: Vec<Vec<u8>>,
    /// Byte strings in the object file (JSON type descriptors).
    pub blobs: Vec<Blob>,
    /// Per function: can it throw (see `throws.rs`)? Calls to those are followed by a check.
    pub may_throw: Vec<bool>,
    /// Does any function throw? Closure and virtual calls are checked only if so.
    pub any_throw: bool,
    /// Runs the top-level initialisers; exported as `ts_init`.
    pub init: Option<FuncId>,
    /// Put between `ts_` and an exported name, so several modules can link into one program.
    pub prefix: String,
    /// Added to struct ids in object headers, so modules linked into one program (sharing one
    /// heap) never use the same header. Give each module a range of its own.
    pub struct_base: u32,
    /// The structs of the engine's value classes (`Vector3`, ...): all `number` fields.
    pub value_structs: Vec<StructId>,
    /// Engine names the compiled code uses without an extern call (the host-class base), for
    /// the renderer tier.
    pub uses: Vec<String>,
    /// The host's hook list (`hostClasses.hooks`): the order of `HostClass::hooks`.
    pub host_hooks: Vec<String>,
    /// Debug builds: the address of the byte that arms the per-statement hook (`debug.rs`); 0 =
    /// no hook code at all.
    pub debug_armed: usize,
    /// Debug builds: the first number of this module's function descriptors (`debug.rs`); a
    /// function's is this plus its index.
    pub debug_base: u32,
    /// The names of locals, per function (index in `funcs`): (local, name), for a debugger.
    pub local_names: std::collections::HashMap<usize, Vec<(VarId, String)>>,
}

impl Module {
    /// Records that the compiled code uses `name` (once).
    pub fn note_use(&mut self, name: &str) {
        if !self.uses.iter().any(|u| u == name) {
            self.uses.push(name.to_string());
        }
    }
}

// ---- A readable dump (for `platypus_ts --dump-ir` and the doc) ----

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Void => write!(f, "void"),
            Ty::Num => write!(f, "f64"),
            Ty::Bool => write!(f, "bool"),
            Ty::Lit => write!(f, "tag"),
            Ty::Str => write!(f, "str"),
            Ty::BigInt => write!(f, "bigint"),
            Ty::Null => write!(f, "null"),
            Ty::Obj(i) => write!(f, "obj#{i}"),
            Ty::Arr(t) => write!(f, "[{t}]"),
            Ty::Typed(k) => write!(f, "{}", k.name()),
            Ty::Fn(t) => write!(f, "fn({} params)->{}", t.params.len(), t.ret),
            Ty::Cell(t) => write!(f, "cell<{t}>"),
            Ty::Nullable(t) => write!(f, "{t}?"),
            Ty::Handle(n) => write!(f, "handle:{n}"),
            Ty::Map(k, v) => write!(f, "map<{k}, {v}>"),
            Ty::Set(k) => write!(f, "set<{k}>"),
            Ty::JsValue => write!(f, "js"),
        }
    }
}

pub fn dump(m: &Module) -> String {
    let mut s = String::new();
    for (i, st) in m.structs.iter().enumerate() {
        s += &format!("struct #{i} {} {{ ", st.name);
        for fl in &st.fields {
            s += &format!("{}: {}; ", fl.name, fl.ty);
        }
        s += "}\n";
    }
    for (i, g) in m.globals.iter().enumerate() {
        s += &format!("global #{i} {}: {}\n", g.name, g.ty);
    }
    for (i, func) in m.funcs.iter().enumerate() {
        s += &format!(
            "fn #{i} {}{}(",
            if func.exported { "export " } else { "" },
            func.name
        );
        for p in &func.params {
            s += &format!("v{p}: {}, ", func.locals[*p as usize]);
        }
        s += &format!(") -> {}\n", func.ret);
        dump_stmts(&mut s, &func.body, 1);
    }
    s
}

fn dump_stmts(s: &mut String, body: &[Stmt], depth: usize) {
    let pad = "  ".repeat(depth);
    for st in body {
        match st {
            Stmt::If(c, a, b) => {
                *s += &format!("{pad}if {} {{\n", ex(c));
                dump_stmts(s, a, depth + 1);
                if !b.is_empty() {
                    *s += &format!("{pad}}} else {{\n");
                    dump_stmts(s, b, depth + 1);
                }
                *s += &format!("{pad}}}\n");
            }
            Stmt::Block(l, b) => {
                *s += &format!("{pad}block L{l} {{\n");
                dump_stmts(s, b, depth + 1);
                *s += &format!("{pad}}}\n");
            }
            Stmt::Loop(l, b) => {
                *s += &format!("{pad}loop L{l} {{\n");
                dump_stmts(s, b, depth + 1);
                *s += &format!("{pad}}}\n");
            }
            Stmt::Assign(v, e) => *s += &format!("{pad}v{v} = {}\n", ex(e)),
            Stmt::StoreField(o, _, i, e) => *s += &format!("{pad}{}.#{i} = {}\n", ex(o), ex(e)),
            Stmt::StoreIndex(a, i, e) => *s += &format!("{pad}{}[{}] = {}\n", ex(a), ex(i), ex(e)),
            Stmt::StoreGlobal(g, e) => *s += &format!("{pad}g{g} = {}\n", ex(e)),
            Stmt::CellSet(c, e) => *s += &format!("{pad}{}.cell = {}\n", ex(c), ex(e)),
            Stmt::Throw(e) => *s += &format!("{pad}throw {}\n", ex(e)),
            Stmt::Try(b, v, h) => {
                *s += &format!("{pad}try {{\n");
                dump_stmts(s, b, depth + 1);
                *s += &format!(
                    "{pad}}} catch {} {{\n",
                    v.map(|v| format!("v{v}")).unwrap_or_default()
                );
                dump_stmts(s, h, depth + 1);
                *s += &format!("{pad}}}\n");
            }
            Stmt::Break(l) => *s += &format!("{pad}break L{l}\n"),
            Stmt::Continue(l) => *s += &format!("{pad}continue L{l}\n"),
            Stmt::Return(e) => {
                *s += &format!("{pad}return {}\n", e.as_ref().map(ex).unwrap_or_default())
            }
            Stmt::Eval(e) => *s += &format!("{pad}{}\n", ex(e)),
            Stmt::Line(n) => *s += &format!("{pad}// line {n}\n"),
        }
    }
}

fn ex(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Num(n) => format!("{n}"),
        ExprKind::Bool(b) => format!("{b}"),
        ExprKind::Tag(t) => format!("tag{t}"),
        ExprKind::Str(i) => format!("str#{i}"),
        ExprKind::Blob(i) => format!("blob#{i}"),
        ExprKind::Bits(a) => format!("bits({})", ex(a)),
        ExprKind::Unbits(a) => format!("unbits({})", ex(a)),
        ExprKind::TagStr(a) => format!("tagstr({})", ex(a)),
        ExprKind::StrTag(a) => format!("strtag({})", ex(a)),
        ExprKind::Null => "null".into(),
        ExprKind::Var(v) => format!("v{v}"),
        ExprKind::Global(g) => format!("g{g}"),
        ExprKind::GlobalVar(g) => format!("g{g}'"),
        ExprKind::Un(o, a) => format!("{o:?}({})", ex(a)),
        ExprKind::Bin(o, a, b) => format!("{o:?}({}, {})", ex(a), ex(b)),
        ExprKind::Cmp(o, a, b) => format!("{o:?}({}, {})", ex(a), ex(b)),
        ExprKind::And(a, b) => format!("({} && {})", ex(a), ex(b)),
        ExprKind::Or(a, b) => format!("({} || {})", ex(a), ex(b)),
        ExprKind::Cond(c, a, b) => format!("({} ? {} : {})", ex(c), ex(a), ex(b)),
        ExprKind::IsNull(a) => format!("isnull({})", ex(a)),
        ExprKind::Cast(a) => format!("cast<{}>({})", e.ty, ex(a)),
        ExprKind::Truthy(a) => format!("truthy({})", ex(a)),
        ExprKind::Field(o, _, i) => format!("{}.#{i}", ex(o)),
        ExprKind::Index(a, i) => format!("{}[{}]", ex(a), ex(i)),
        ExprKind::Len(a) => format!("len({})", ex(a)),
        ExprKind::NewObj(s) => format!("new#{s}"),
        ExprKind::Call(f, a) => format!(
            "call#{f}({})",
            a.iter().map(ex).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::CallExtern(i, a) => format!(
            "calle#{i}({})",
            a.iter().map(ex).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::CallExternInto(i, a, d) => format!(
            "calle#{i}({}) -> {}",
            a.iter().map(ex).collect::<Vec<_>>().join(", "),
            ex(d)
        ),
        ExprKind::CallExternOut(i, a, vs) => format!(
            "calle#{i}({}) -> {vs:?}",
            a.iter().map(ex).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::CallVirtual(t, a) => format!(
            "callv#{t}({})",
            a.iter().map(ex).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::InstanceOf(a, t) => format!("instanceof#{t}({})", ex(a)),
        ExprKind::CallClosure(c, a) => format!(
            "callc {}({})",
            ex(c),
            a.iter().map(ex).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::Rt(r, a) => format!(
            "rt.{r:?}({})",
            a.iter().map(ex).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::NewCell(a) => format!("newcell({})", ex(a)),
        ExprKind::CellGet(a) => format!("{}.cell", ex(a)),
        ExprKind::MakeClosure(f, env) => format!(
            "closure#{f}[{}]",
            env.as_ref().map(|e| ex(e)).unwrap_or_default()
        ),
        ExprKind::Seq(stmts, v) => {
            let mut s = String::new();
            dump_stmts(&mut s, stmts, 3);
            format!(
                "{{
{s}      => {}}}",
                ex(v)
            )
        }
    }
}
