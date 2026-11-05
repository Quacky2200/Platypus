// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Platypus IR -> Cranelift -> an object file for the host target.
//!
//! Machine types: `Num` f64, `Bool` i8, `Lit` i32, pointers i64. Object and array slots are
//! 8 bytes wide whatever the value (small values are zero-extended on store).

use crate::frames;
use crate::ir::*;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, Signature, SourceLoc, StackSlot, StackSlotData,
    StackSlotKind, TrapCode, Value, types,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::Module as ClifModule;
use cranelift_module::{
    DataDescription, DataId, FuncId as CFuncId, Linkage, default_libcall_names,
};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;

type R<T> = Result<T, String>;

pub fn clif_ty(t: &Ty) -> ir::Type {
    match t {
        Ty::Num => types::F64,
        Ty::Nullable(i) if **i == Ty::Num => types::F64,
        Ty::Bool => types::I8,
        // A union member is a tag id; 0 is no tag, which is how `Nullable` holds undefined.
        Ty::Lit => types::I32,
        Ty::Nullable(i) if **i == Ty::Lit => types::I32,
        _ => types::I64,
    }
}

/// A debug build spills a function's locals at each statement only up to this many.
pub(crate) const DEBUG_SPILL_MAX: usize = 256;

/// The machine type of a value at the engine boundary.
fn ext_clif(t: &ExtTy) -> ir::Type {
    match t {
        ExtTy::F64 => types::F64,
        ExtTy::F32 => types::F32,
        ExtTy::I32 | ExtTy::U32 | ExtTy::Enum(_) => types::I32,
        ExtTy::Bool => types::I8,
        ExtTy::Handle(_)
        | ExtTy::Value(_)
        | ExtTy::Record(_)
        | ExtTy::ValueList(_)
        | ExtTy::RecordList(_)
        | ExtTy::StrList
        | ExtTy::EnumList(_)
        | ExtTy::F64List
        | ExtTy::RecordsJson(_)
        | ExtTy::HandleList(_)
        | ExtTy::Opt(_)
        | ExtTy::Void
        | ExtTy::Str
        | ExtTy::Fn(..)
        | ExtTy::Ptr
        | ExtTy::HandleArr(_)
        | ExtTy::F64Arr
        | ExtTy::StrArr
        | ExtTy::ValueArr(_) => types::I64,
    }
}

/// Defines a static string object: header (a string, no heap), lengths, flags, cursor, bytes.
fn define_static_str(module: &mut impl ClifModule, name: &str, bytes: &[u8]) -> R<DataId> {
    let id = module
        .declare_data(name, Linkage::Local, true, false)
        .map_err(|e| e.to_string())?;
    let ascii = bytes.is_ascii();
    // UTF-16 units: a continuation byte counts none, a four-byte lead two.
    let units: usize = bytes
        .iter()
        .map(|&x| match x {
            0x80..=0xBF => 0,
            0xF0.. => 2,
            _ => 1,
        })
        .sum();
    let lone_surrogate = bytes.windows(2).any(|w| w[0] == 0xED && w[1] >= 0xA0);
    let flags: u64 = if ascii {
        3
    } else if lone_surrogate {
        0
    } else {
        2
    };
    let mut b: Vec<u8> = Vec::with_capacity(48 + bytes.len());
    b.extend_from_slice(&0xA11A_0003u64.to_le_bytes());
    b.extend_from_slice(&(bytes.len() as i64).to_le_bytes());
    b.extend_from_slice(&(units as i64).to_le_bytes());
    b.extend_from_slice(&flags.to_le_bytes());
    b.extend_from_slice(&[0u8; 16]);
    b.extend_from_slice(bytes);
    let mut d = DataDescription::new();
    d.define(b.into_boxed_slice());
    d.set_align(8);
    module.define_data(id, &d).map_err(|e| e.to_string())?;
    Ok(id)
}

fn sig_of(call_conv: CallConv, params: &[Ty], ret: &Ty) -> Signature {
    let mut s = Signature::new(call_conv);
    for p in params {
        s.params.push(AbiParam::new(clif_ty(p)));
    }
    if *ret != Ty::Void {
        s.returns.push(AbiParam::new(clif_ty(ret)));
    }
    s
}

pub struct Compiled {
    pub object: Vec<u8>,
}

/// What an object file is built for (the JIT always runs on the host CPU).
#[derive(Clone, Debug, Default)]
pub struct ObjectTarget {
    /// A target triple (`x86_64-unknown-linux-musl`); `None` is the host's. The build machine can
    /// write objects for another target: Cranelift needs no toolchain for it.
    pub triple: Option<String>,
    /// Position-independent code (calls through the PLT, data through the GOT): what linking into
    /// a position-independent executable (Rust's default on Linux) or a shared library needs.
    pub pic: bool,
}

/// The host's instruction set with the settings both back ends share. `native` asks the CPU for
/// its features (the JIT runs here; an object file must run on any machine of the triple).
pub(crate) fn host_isa(
    opt: &str,
    native: bool,
) -> R<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    isa_for(opt, native, &ObjectTarget::default())
}

fn isa_for(
    opt: &str,
    native: bool,
    target: &ObjectTarget,
) -> R<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    let mut flags = settings::builder();
    flags.set("opt_level", opt).map_err(|e| e.to_string())?;
    flags
        .set("is_pic", if target.pic { "true" } else { "false" })
        .map_err(|e| e.to_string())?;
    // Only the JIT registers unwind info (`unwind.rs`); an object file has no use for it here.
    flags
        .set("unwind_info", if native { "true" } else { "false" })
        .map_err(|e| e.to_string())?;
    // A frame bigger than a page (a big `init` with thousands of mirrored locals) must touch the
    // pages it passes: Windows grows the stack one guard page at a time, so a frame that jumps
    // past the guard page crashes (a stack dispatch with no stack left: no handler runs, no log).
    // Off by default in Cranelift. Inline probes need no libcall in the JIT.
    flags
        .set("enable_probestack", "true")
        .map_err(|e| e.to_string())?;
    flags
        .set("probestack_strategy", "inline")
        .map_err(|e| e.to_string())?;
    let flags = settings::Flags::new(flags);
    if native {
        let b = cranelift_native::builder().map_err(|e| e.to_string())?;
        return b.finish(flags).map_err(|e| e.to_string());
    }
    let triple = match &target.triple {
        Some(t) => t
            .parse::<target_lexicon::Triple>()
            .map_err(|e| format!("target {t}: {e}"))?,
        None => target_lexicon::Triple::host(),
    };
    cranelift_codegen::isa::lookup(triple)
        .map_err(|e| e.to_string())?
        .finish(flags)
        .map_err(|e| e.to_string())
}

/// Compiles a module. `opt` is Cranelift's `opt_level`: "none", "speed" or "speed_and_size".
pub fn compile(m: &Module, opt: &str) -> R<Compiled> {
    compile_for(m, opt, &ObjectTarget::default())
}

/// Like `compile`, for `target`.
pub fn compile_for(m: &Module, opt: &str, target: &ObjectTarget) -> R<Compiled> {
    let isa = isa_for(opt, false, target)?;
    let mut module = ObjectModule::new(
        ObjectBuilder::new(isa, "platypus_ts", default_libcall_names())
            .map_err(|e| e.to_string())?,
    );
    let _ = generate(&mut module, m, false)?;
    let object = module.finish().emit().map_err(|e| e.to_string())?;
    Ok(Compiled { object })
}

/// What `generate` returns: every function by symbol, and per function (same order) the code
/// ranges `(start, end, line)` relative to its start, `line` a line of the bundled source.
pub(crate) struct Generated {
    pub names: Vec<(String, CFuncId)>,
    pub lines: Vec<Vec<(u32, u32, u32)>>,
    /// Per function: its code size and Windows x64 unwind info, when the ISA makes it (the JIT).
    pub unwind: Vec<Option<(u32, Vec<u8>)>>,
}

/// The size and Windows x64 unwind info of the function last compiled in `ctx`.
fn unwind_of(
    ctx: &cranelift_codegen::Context,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Option<(u32, Vec<u8>)> {
    use cranelift_codegen::isa::unwind::UnwindInfo;
    let code = ctx.compiled_code()?;
    match code.create_unwind_info(isa).ok().flatten()? {
        UnwindInfo::WindowsX64(info) => {
            let mut bytes = vec![0u8; info.emit_size()];
            info.emit(&mut bytes);
            Some((code.code_info().total_size, bytes))
        }
        _ => None,
    }
}

/// Defines every function and datum of `m` in `module`. Returns every function by symbol name,
/// exported (`ts_<prefix>..`) and local (`platypus_ts_local_..`) alike, so the JIT can name a fault
/// in any of them. The same code drives the object writer and the JIT. `far_refs`: the module's
/// own data and function addresses are absolute (the JIT, whose code and data may be far apart).
pub(crate) fn generate<M: ClifModule>(module: &mut M, m: &Module, far_refs: bool) -> R<Generated> {
    let call_conv = module.isa().default_call_conv();
    let fcfg = module.isa().frontend_config();
    let mut names: Vec<(String, CFuncId)> = Vec::new();
    let mut lines: Vec<Vec<(u32, u32, u32)>> = Vec::new();
    let mut unwind: Vec<Option<(u32, Vec<u8>)>> = Vec::new();

    // Functions: declare all first so calls can be forward references.
    let mut cfuncs: Vec<CFuncId> = Vec::new();
    for f in &m.funcs {
        let ptys: Vec<Ty> = f
            .params
            .iter()
            .map(|p| f.locals[*p as usize].clone())
            .collect();
        let sig = sig_of(call_conv, &ptys, &f.ret);
        let (name, linkage) = if f.exported {
            (format!("ts_{}{}", m.prefix, f.name), Linkage::Export)
        } else {
            (format!("platypus_ts_local_{}", f.name), Linkage::Local)
        };
        let id = module
            .declare_function(&name, linkage, &sig)
            .map_err(|e| e.to_string())?;
        names.push((name, id));
        cfuncs.push(id);
    }
    // Engine functions the module calls: imports, with the C ABI of the manifest.
    let mut ext_funcs: Vec<CFuncId> = Vec::new();
    for d in &m.externs {
        let mut sig = Signature::new(call_conv);
        for p in &d.params {
            sig.params.push(AbiParam::new(ext_clif(p)));
        }
        if d.out.is_some() || d.ret == ExtTy::Str {
            sig.params.push(AbiParam::new(types::I64));
        }
        if d.ret != ExtTy::Void {
            sig.returns.push(AbiParam::new(ext_clif(&d.ret)));
        }
        ext_funcs.push(
            module
                .declare_function(&d.symbol, Linkage::Import, &sig)
                .map_err(|e| e.to_string())?,
        );
    }
    // Globals: one 8-byte slot each.
    let mut gdata: Vec<DataId> = Vec::new();
    for g in &m.globals {
        let id = module
            .declare_data(
                &format!("ts_g_{}{}", m.prefix, g.name),
                Linkage::Local,
                true,
                false,
            )
            .map_err(|e| e.to_string())?;
        let mut d = DataDescription::new();
        d.define_zeroinit(8);
        // The collector reads pointer slots as `*mut u8`: they must be aligned.
        d.set_align(8);
        module.define_data(id, &d).map_err(|e| e.to_string())?;
        gdata.push(id);
    }

    // Method tables: one 8-byte slot per struct id, holding the function address (or 0).
    let mut vtdata: Vec<DataId> = Vec::new();
    for (i, vt) in m.vtables.iter().enumerate() {
        let id = module
            .declare_data(
                &format!("ts_vt_{}{}", m.prefix, i),
                Linkage::Local,
                false,
                false,
            )
            .map_err(|e| e.to_string())?;
        let mut d = DataDescription::new();
        d.define(vec![0u8; m.structs.len() * 8].into_boxed_slice());
        d.set_align(8);
        for (sid, fid) in &vt.entries {
            let fr = module.declare_func_in_data(cfuncs[*fid], &mut d);
            d.write_function_addr((*sid * 8) as u32, fr);
        }
        module.define_data(id, &d).map_err(|e| e.to_string())?;
        vtdata.push(id);
    }
    // `instanceof` tables: one byte per struct id.
    let mut itdata: Vec<DataId> = Vec::new();
    for (i, members) in m.itables.iter().enumerate() {
        let id = module
            .declare_data(
                &format!("ts_it_{}{}", m.prefix, i),
                Linkage::Local,
                false,
                false,
            )
            .map_err(|e| e.to_string())?;
        let mut bytes = vec![0u8; m.structs.len()];
        for s in members {
            bytes[*s] = 1;
        }
        let mut d = DataDescription::new();
        d.define(bytes.into_boxed_slice());
        module.define_data(id, &d).map_err(|e| e.to_string())?;
        itdata.push(id);
    }

    // String literals: static `Str` objects (see platypus_ts_rt/src/string.rs for the layout). Writable,
    // because the runtime keeps a scan cursor in the string; the header has no heap id, so the
    // collector never touches them.
    let mut sdata: Vec<DataId> = Vec::new();
    for (i, text) in m.strs.iter().enumerate() {
        sdata.push(define_static_str(
            module,
            &format!("ts_s_{}{}", m.prefix, i),
            text,
        )?);
    }
    // Blobs (JSON type descriptors): read-only bytes.
    let mut bdata: Vec<DataId> = Vec::new();
    for (i, blob) in m.blobs.iter().enumerate() {
        let id = module
            .declare_data(
                &format!("ts_b_{}{}", m.prefix, i),
                Linkage::Local,
                false,
                false,
            )
            .map_err(|e| e.to_string())?;
        let mut d = DataDescription::new();
        let mut bytes = blob.bytes.clone();
        for at in &blob.headers {
            let v =
                u64::from_le_bytes(bytes[*at..*at + 8].try_into().unwrap()) + m.struct_base as u64;
            bytes[*at..*at + 8].copy_from_slice(&v.to_le_bytes());
        }
        d.define(bytes.into_boxed_slice());
        d.set_align(8);
        module.define_data(id, &d).map_err(|e| e.to_string())?;
        bdata.push(id);
    }
    // The text of every tag, for `TagStr`.
    let mut tagtab = None;
    if !m.tags.is_empty() {
        let mut tag_strs = Vec::new();
        for (i, text) in m.tags.iter().enumerate() {
            tag_strs.push(define_static_str(
                module,
                &format!("ts_t_{}{}", m.prefix, i),
                text.as_bytes(),
            )?);
        }
        let id = module
            .declare_data(&format!("ts_tt_{}", m.prefix), Linkage::Local, false, false)
            .map_err(|e| e.to_string())?;
        let mut d = DataDescription::new();
        d.define(vec![0u8; m.tags.len() * 8].into_boxed_slice());
        d.set_align(8);
        for (i, t) in tag_strs.iter().enumerate() {
            let gv = module.declare_data_in_data(*t, &mut d);
            d.write_data_addr((i * 8) as u32, gv, 0);
        }
        module.define_data(id, &d).map_err(|e| e.to_string())?;
        tagtab = Some(id);
    }

    let mut imports: HashMap<&'static str, CFuncId> = HashMap::new();
    let mut fbc = FunctionBuilderContext::new();
    for (fi, f) in m.funcs.iter().enumerate() {
        if std::env::var_os("PLATYPUS_TS_TRACE").is_some() {
            eprintln!("codegen {}", f.name);
        }
        let mut ctx = module.make_context();
        let ptys: Vec<Ty> = f
            .params
            .iter()
            .map(|p| f.locals[*p as usize].clone())
            .collect();
        ctx.func.signature = sig_of(call_conv, &ptys, &f.ret);
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbc);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            let mut vars = Vec::new();
            for (i, t) in f.locals.iter().enumerate() {
                let ct = clif_ty(t);
                let v = b.declare_var(ct);
                vars.push(v);
                if let Some(pi) = f.params.iter().position(|p| *p as usize == i) {
                    let pv = b.block_params(entry)[pi];
                    b.def_var(v, pv);
                } else {
                    let z = zero(&mut b, ct);
                    b.def_var(v, z);
                }
            }
            let (mf_t, mf_p) = (MemFlagsData::trusted(), MemFlagsData::new());
            let plan = frames::plan(f);
            let frame_slot = (plan.total() > 0).then(|| {
                b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    16 + 8 * plan.total(),
                    4,
                ))
            });
            let mut cg = Cg {
                frame_slot,
                ptr_slot: plan.ptr_slot.clone(),
                next_spill: plan.n_locals,
                n_slots: plan.total(),
                b,
                mf_t,
                mf_p,
                module: &mut *module,
                m,
                f,
                vars,
                cfuncs: &cfuncs,
                ext_funcs: &ext_funcs,
                gdata: &gdata,
                vtdata: &vtdata,
                itdata: &itdata,
                sdata: &sdata,
                bdata: &bdata,
                tagtab,
                imports: &mut imports,
                exc_stack: Vec::new(),
                exc_exit: None,
                labels: HashMap::new(),
                call_conv,
                far_refs,
                fi,
                dbg_slot: None,
                cur_line: 0,
            };
            cg.prologue(m.init == Some(fi))?;
            cg.stmts(&f.body)?;
            cg.b.ins().trap(TrapCode::unwrap_user(1));
            cg.finish_exceptions();
            cg.b.seal_all_blocks();
            cg.b.finalize(fcfg);
        }
        module
            .define_function(cfuncs[fi], &mut ctx)
            .map_err(|e| format!("{}: {e:?}", f.name))?;
        lines.push(
            ctx.compiled_code()
                .map(|c| {
                    c.buffer
                        .get_srclocs_sorted()
                        .iter()
                        .filter(|l| !l.loc.is_default())
                        .map(|l| (l.start, l.end, l.loc.bits()))
                        .collect()
                })
                .unwrap_or_default(),
        );
        unwind.push(unwind_of(&ctx, module.isa()));
        module.clear_context(&mut ctx);
    }
    Ok(Generated {
        names,
        lines,
        unwind,
    })
}

fn zero(b: &mut FunctionBuilder, t: ir::Type) -> Value {
    if t == types::F64 {
        b.ins().f64const(0.0)
    } else {
        b.ins().iconst(t, 0)
    }
}

struct Cg<'a, 'b, M: ClifModule> {
    /// This function's index in the module (a debug build names it by `debug_base + fi`).
    fi: usize,
    /// A debug build's spill area for the locals, made at the first statement.
    dbg_slot: Option<StackSlot>,
    /// The line of the last marker (a `throw` reports it).
    cur_line: u32,
    b: FunctionBuilder<'b>,
    /// The shadow-stack frame (see `frames.rs`): pointer locals are mirrored here.
    frame_slot: Option<StackSlot>,
    ptr_slot: Vec<Option<u32>>,
    next_spill: u32,
    n_slots: u32,
    mf_t: MemFlagsData,
    mf_p: MemFlagsData,
    module: &'a mut M,
    m: &'a Module,
    f: &'a Func,
    vars: Vec<Variable>,
    cfuncs: &'a [CFuncId],
    ext_funcs: &'a [CFuncId],
    gdata: &'a [DataId],
    vtdata: &'a [DataId],
    itdata: &'a [DataId],
    /// Static strings (`ExprKind::Str`) and the table of the tags' strings.
    sdata: &'a [DataId],
    bdata: &'a [DataId],
    tagtab: Option<DataId>,
    imports: &'a mut HashMap<&'static str, CFuncId>,
    /// The handlers of the `try` blocks being generated, innermost last.
    exc_stack: Vec<ir::Block>,
    /// Where an exception leaves the function (made when first needed).
    exc_exit: Option<ir::Block>,
    /// label -> (break target, continue target)
    labels: HashMap<LabelId, (ir::Block, ir::Block)>,
    call_conv: CallConv,
    /// Take the module's own data and function addresses as absolute values, not RIP-relative
    /// (see `data_addr`). The JIT sets it; an object file keeps the shorter relative form.
    far_refs: bool,
}

impl<M: ClifModule> Cg<'_, '_, M> {
    /// The address of one of the module's data objects. In the JIT, code and data are separate
    /// allocations that can lie more than 2 GB apart (likelier after a hot reload, in a grown
    /// process), out of reach of a 32-bit RIP-relative reference: cranelift-jit panics applying
    /// it. Not colocated, the address is a 64-bit immediate (`is_pic` is off), which reaches.
    fn data_addr(&mut self, id: DataId) -> Value {
        let gv = self.module.declare_data_in_func(id, self.b.func);
        if self.far_refs
            && let ir::GlobalValueData::Symbol { colocated, .. } =
                &mut self.b.func.global_values[gv]
        {
            *colocated = false;
        }
        self.b.ins().symbol_value(types::I64, gv)
    }

    /// The address of one of the module's functions, as a value (for a closure): absolute in the
    /// JIT, for the reason in `data_addr`. Calls need nothing: an out-of-range call goes through
    /// a veneer.
    fn func_addr(&mut self, id: CFuncId) -> Value {
        let fr = self.module.declare_func_in_func(id, self.b.func);
        if self.far_refs {
            self.b.func.dfg.ext_funcs[fr].colocated = false;
        }
        self.b.ins().func_addr(types::I64, fr)
    }

    fn null_num(&mut self) -> Value {
        self.b.ins().f64const(f64::from_bits(NULL_NUM_BITS))
    }

    fn null_of(&mut self, t: &Ty) -> Value {
        if clif_ty(t) == types::F64 {
            self.null_num()
        } else {
            let ct = clif_ty(t);
            self.b.ins().iconst(ct, 0)
        }
    }

    /// Calls a runtime function by symbol name.
    fn rt(
        &mut self,
        name: &'static str,
        params: &[ir::Type],
        ret: Option<ir::Type>,
        args: &[Value],
    ) -> Option<Value> {
        let id = match self.imports.get(name) {
            Some(i) => *i,
            None => {
                let mut s = Signature::new(self.call_conv);
                for p in params {
                    s.params.push(AbiParam::new(*p));
                }
                if let Some(r) = ret {
                    s.returns.push(AbiParam::new(r));
                }
                let id = self
                    .module
                    .declare_function(name, Linkage::Import, &s)
                    .unwrap();
                self.imports.insert(name, id);
                id
            }
        };
        let fr = self.module.declare_func_in_func(id, self.b.func);
        let call = self.b.ins().call(fr, args);
        self.b.inst_results(call).first().copied()
    }

    /// A debug build's check before a statement: when the debugger's armed byte is set, call its
    /// hook with the line.
    fn debug_call(&mut self, hook_fn: &'static str, line: u32) {
        let at = self.b.ins().iconst(types::I64, self.m.debug_armed as i64);
        let armed = self.b.ins().uload8(types::I64, self.mf_t, at, 0);
        let hook = self.b.create_block();
        let go_on = self.b.create_block();
        self.b.ins().brif(armed, hook, &[], go_on, &[]);
        self.b.switch_to_block(hook);
        let l = self.b.ins().iconst(types::I32, i64::from(line));
        // The locals, raw, in a frame the debugger reads at the stop: only on this path, and not
        // for a function with so many (a big `init`) that spilling them at every statement would
        // swamp the code.
        let n = self.f.locals.len();
        let frame = if n > 0 && n <= DEBUG_SPILL_MAX {
            let slot = *self.dbg_slot.get_or_insert_with(|| {
                self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    8 * n as u32,
                    3,
                ))
            });
            for (i, v) in self.vars.clone().into_iter().enumerate() {
                let x = self.b.use_var(v);
                let raw = match self.b.func.dfg.value_type(x) {
                    types::F64 => self.b.ins().bitcast(types::I64, self.mf_p, x),
                    types::I64 => x,
                    _ => self.b.ins().uextend(types::I64, x),
                };
                self.b
                    .ins()
                    .stack_store(types::I64, raw, slot, 8 * i as i32);
            }
            self.b.ins().stack_addr(types::I64, slot, 0)
        } else {
            self.b.ins().iconst(types::I64, 0)
        };
        let id = self
            .b
            .ins()
            .iconst(types::I32, i64::from(self.m.debug_base + self.fi as u32));
        self.rt(
            hook_fn,
            &[types::I32, types::I64, types::I32],
            None,
            &[l, frame, id],
        );
        self.b.ins().jump(go_on, &[]);
        self.b.switch_to_block(go_on);
    }

    fn cvt_i64(&mut self, v: Value) -> Value {
        self.b.ins().fcvt_to_sint_sat(types::I64, v)
    }

    /// Element `idx` of a typed array's data as a double.
    fn typed_load(&mut self, kind: crate::ir::TypedKind, data: Value, idx: Value) -> Value {
        use crate::ir::TypedKind as K;
        let off = self.b.ins().imul_imm_s(idx, kind.size() as i64);
        let addr = self.b.ins().iadd(data, off);
        let f = self.mf_t;
        match kind {
            K::U8 | K::U8C => {
                let v = self.b.ins().uload8(types::I64, f, addr, 0);
                self.b.ins().fcvt_from_sint(types::F64, v)
            }
            K::I8 => {
                let v = self.b.ins().sload8(types::I64, f, addr, 0);
                self.b.ins().fcvt_from_sint(types::F64, v)
            }
            K::U16 => {
                let v = self.b.ins().uload16(types::I64, f, addr, 0);
                self.b.ins().fcvt_from_sint(types::F64, v)
            }
            K::I16 => {
                let v = self.b.ins().sload16(types::I64, f, addr, 0);
                self.b.ins().fcvt_from_sint(types::F64, v)
            }
            K::U32 => {
                let w = self.b.ins().load(types::I32, f, addr, 0);
                let v = self.b.ins().uextend(types::I64, w);
                self.b.ins().fcvt_from_sint(types::F64, v)
            }
            K::I32 => {
                let w = self.b.ins().load(types::I32, f, addr, 0);
                let v = self.b.ins().sextend(types::I64, w);
                self.b.ins().fcvt_from_sint(types::F64, v)
            }
            K::F32 => {
                let v = self.b.ins().load(types::F32, f, addr, 0);
                self.b.ins().fpromote(types::F64, v)
            }
            K::F64 => self.b.ins().load(types::F64, f, addr, 0),
        }
    }

    /// Stores a double as element `idx` of a typed array's data, converting as the kind requires.
    fn typed_store(&mut self, kind: crate::ir::TypedKind, data: Value, idx: Value, val: Value) {
        use crate::ir::TypedKind as K;
        let off = self.b.ins().imul_imm_s(idx, kind.size() as i64);
        let addr = self.b.ins().iadd(data, off);
        let f = self.mf_t;
        match kind {
            K::F64 => {
                self.b.ins().store(f, val, addr, 0);
            }
            K::F32 => {
                let v = self.b.ins().fdemote(types::F32, val);
                self.b.ins().store(f, v, addr, 0);
            }
            K::U8C => {
                // Round half to even, clamp to 0..255; NaN converts to 0.
                let r = self.b.ins().nearest(val);
                let zero = self.b.ins().f64const(0.0);
                let max = self.b.ins().f64const(255.0);
                let lo = self.b.ins().fmax(r, zero);
                let hi = self.b.ins().fmin(lo, max);
                let n = self.b.ins().fcvt_to_uint_sat(types::I64, hi);
                let byte = self.b.ins().ireduce(types::I8, n);
                self.b.ins().store(f, byte, addr, 0);
            }
            _ => {
                // NaN, infinities and values of 2^63 or more store 0 (their low bits).
                let sat = self.cvt_i64(val);
                let abs = self.b.ins().fabs(val);
                let limit = self.b.ins().f64const(9.223372036854776e18);
                let small = self.b.ins().fcmp(FloatCC::LessThan, abs, limit);
                let zero_i = self.b.ins().iconst(types::I64, 0);
                let n = self.b.ins().select(small, sat, zero_i);
                let w = match kind.size() {
                    1 => types::I8,
                    2 => types::I16,
                    _ => types::I32,
                };
                let v = self.b.ins().ireduce(w, n);
                self.b.ins().store(f, v, addr, 0);
            }
        }
    }

    /// JS ToInt32 for the values games use (|x| < 2^63).
    fn cvt_i32(&mut self, v: Value) -> Value {
        let w = self.cvt_i64(v);
        self.b.ins().ireduce(types::I32, w)
    }

    /// The raw 64-bit slot value for a typed value.
    fn slot_of(&mut self, v: Value, t: &Ty) -> Value {
        match clif_ty(t) {
            types::F64 => self.b.ins().bitcast(types::I64, self.mf_p, v),
            types::I8 | types::I32 => self.b.ins().uextend(types::I64, v),
            _ => v,
        }
    }

    /// Stores a pointer in frame slot `idx` (a GC root).
    fn root_store(&mut self, idx: u32, v: Value) {
        let slot = self.frame_slot.expect("frame");
        let base = self.b.ins().stack_addr(types::I64, slot, 0);
        self.b.ins().store(self.mf_t, v, base, 16 + 8 * idx as i32);
    }

    /// Links this function's frame onto the shadow stack and roots its pointer parameters. The
    /// init function also tells the collector which struct slots are pointers and which globals
    /// are roots.
    fn prologue(&mut self, is_init: bool) -> R<()> {
        if let Some(slot) = self.frame_slot {
            let base = self.b.ins().stack_addr(types::I64, slot, 0);
            let n = self.b.ins().iconst(types::I64, self.n_slots as i64);
            self.rt(
                "ts_rt_frame_push",
                &[types::I64, types::I64],
                None,
                &[base, n],
            );
            for p in self.f.params.clone() {
                if let Some(i) = self.ptr_slot[p as usize] {
                    let v = self.b.use_var(self.vars[p as usize]);
                    self.root_store(i, v);
                }
            }
        }
        if is_init {
            for (sid, st) in self.m.structs.iter().enumerate() {
                if st.fields.len() > 64 {
                    return Err(format!("struct {} has more than 64 fields", st.name));
                }
                let mut mask = 0u64;
                for (i, f) in st.fields.iter().enumerate() {
                    if f.ty.is_ptr() {
                        mask |= 1 << i;
                    }
                }
                let h = self
                    .b
                    .ins()
                    .iconst(types::I64, self.m.struct_base as i64 + sid as i64 + 1);
                let n = self.b.ins().iconst(types::I32, st.fields.len() as i64);
                let mk = self.b.ins().iconst(types::I64, mask as i64);
                self.rt(
                    "ts_rt_register_struct",
                    &[types::I64, types::I32, types::I64],
                    None,
                    &[h, n, mk],
                );
            }
            for (g, gl) in self.m.globals.iter().enumerate() {
                if gl.ty.is_ptr() {
                    let addr = self.data_addr(self.gdata[g]);
                    self.rt("ts_rt_add_global_root", &[types::I64], None, &[addr]);
                }
            }
        }
        Ok(())
    }

    /// Evaluates operands in order, spilling pointer values that a later operand's allocation
    /// could otherwise leave unrooted.
    fn eval_args(&mut self, args: &[(&Expr, Ty)]) -> R<Vec<Value>> {
        let refs: Vec<&Expr> = args.iter().map(|(e, _)| *e).collect();
        let flags = frames::spill_flags(&refs);
        let mut vals = Vec::new();
        for ((e, t), spill) in args.iter().zip(flags) {
            let v = self.expr_as(e, t)?;
            if spill {
                let idx = self.next_spill;
                self.next_spill += 1;
                if idx >= self.n_slots {
                    // The plan in frames.rs and this walk disagree; writing on would corrupt the stack.
                    return Err(format!(
                        "{}: frame plan too small ({} slots)",
                        self.f.name, self.n_slots
                    ));
                }
                self.root_store(idx, v);
            }
            vals.push(v);
        }
        Ok(vals)
    }

    /// The block an exception goes to from here: the innermost `try`'s handler, or the exit.
    fn exc_target(&mut self) -> ir::Block {
        if let Some(b) = self.exc_stack.last() {
            return *b;
        }
        match self.exc_exit {
            Some(b) => b,
            None => {
                let b = self.b.create_block();
                self.exc_exit = Some(b);
                b
            }
        }
    }

    /// After a call that can throw: leave for the handler if an exception is pending.
    fn check_exception(&mut self) {
        let pending = self
            .rt("ts_rt_exc_pending", &[], Some(types::I8), &[])
            .unwrap();
        let target = self.exc_target();
        let cont = self.b.create_block();
        self.b.ins().brif(pending, target, &[], cont, &[]);
        self.b.switch_to_block(cont);
    }

    /// The exit for exceptions no `try` in this function handles: pop the frame, return nothing
    /// (the caller sees the pending exception).
    fn finish_exceptions(&mut self) {
        let Some(b) = self.exc_exit else { return };
        self.b.switch_to_block(b);
        self.pop_frame();
        match self.f.ret.clone() {
            Ty::Void => {
                self.b.ins().return_(&[]);
            }
            t => {
                let z = zero(&mut self.b, clif_ty(&t));
                self.b.ins().return_(&[z]);
            }
        }
    }

    fn pop_frame(&mut self) {
        if self.frame_slot.is_some() {
            self.rt("ts_rt_frame_pop", &[], None, &[]);
        }
    }

    fn dead_block(&mut self) {
        let d = self.b.create_block();
        self.b.switch_to_block(d);
    }

    fn stmts(&mut self, body: &[Stmt]) -> R<()> {
        for s in body {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn expr_as(&mut self, e: &Expr, want: &Ty) -> R<Value> {
        if matches!(e.kind, ExprKind::Null) {
            return Ok(self.null_of(want));
        }
        self.expr(e)
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        match s {
            Stmt::Assign(v, e) => {
                let val = self.expr_as(e, &self.f.locals[*v as usize].clone())?;
                let var = self.vars[*v as usize];
                self.b.def_var(var, val);
                if let Some(i) = self.ptr_slot[*v as usize] {
                    self.root_store(i, val);
                }
            }
            Stmt::StoreField(o, _sid, i, v) => {
                let vals = self.eval_args(&[(o, o.ty.clone()), (v, v.ty.clone())])?;
                let (ptr, val) = (vals[0], vals[1]);
                let slot = self.slot_of(val, &v.ty);
                self.b.ins().store(self.mf_t, slot, ptr, field_offset(*i));
            }
            Stmt::CellSet(c, v) => {
                let vals = self.eval_args(&[(c, c.ty.clone()), (v, v.ty.clone())])?;
                let slot = self.slot_of(vals[1], &v.ty);
                self.b.ins().store(self.mf_t, slot, vals[0], 8);
            }
            Stmt::StoreIndex(a, i, v) => {
                let vals =
                    self.eval_args(&[(a, a.ty.clone()), (i, i.ty.clone()), (v, v.ty.clone())])?;
                let (arr, idx, val) = (vals[0], vals[1], vals[2]);
                let idx = self.cvt_i64(idx);
                let len = self.b.ins().load(types::I64, self.mf_t, arr, 8);
                let ok = self.b.ins().icmp(IntCC::UnsignedLessThan, idx, len);
                let (st, join) = (self.b.create_block(), self.b.create_block());
                self.b.ins().brif(ok, st, &[], join, &[]);
                self.b.switch_to_block(st);
                let data = self.b.ins().load(types::I64, self.mf_t, arr, 24);
                if let Ty::Typed(kind) = a.ty.strip() {
                    self.typed_store(*kind, data, idx, val);
                } else {
                    let slot = self.slot_of(val, &v.ty);
                    let off = self.b.ins().imul_imm_s(idx, 8);
                    let addr = self.b.ins().iadd(data, off);
                    self.b.ins().store(self.mf_t, slot, addr, 0);
                }
                self.b.ins().jump(join, &[]);
                self.b.switch_to_block(join);
            }
            Stmt::StoreGlobal(g, e) => {
                let val = self.expr(e)?;
                let slot = self.slot_of(val, &e.ty);
                let addr = self.data_addr(self.gdata[*g]);
                self.b.ins().store(self.mf_t, slot, addr, 0);
            }
            Stmt::If(c, t, e) => {
                let cv = self.expr(c)?;
                let (tb, eb, join) = (
                    self.b.create_block(),
                    self.b.create_block(),
                    self.b.create_block(),
                );
                self.b.ins().brif(cv, tb, &[], eb, &[]);
                self.b.switch_to_block(tb);
                self.stmts(t)?;
                self.b.ins().jump(join, &[]);
                self.b.switch_to_block(eb);
                self.stmts(e)?;
                self.b.ins().jump(join, &[]);
                self.b.switch_to_block(join);
            }
            Stmt::Block(l, body) => {
                let after = self.b.create_block();
                self.labels.insert(*l, (after, after));
                self.stmts(body)?;
                self.b.ins().jump(after, &[]);
                self.b.switch_to_block(after);
            }
            Stmt::Loop(l, body) => {
                let (head, after) = (self.b.create_block(), self.b.create_block());
                self.labels.insert(*l, (after, head));
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(head);
                self.stmts(body)?;
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(after);
            }
            Stmt::Break(l) => {
                let t = self.labels[l].0;
                self.b.ins().jump(t, &[]);
                self.dead_block();
            }
            Stmt::Continue(l) => {
                let t = self.labels[l].1;
                self.b.ins().jump(t, &[]);
                self.dead_block();
            }
            Stmt::Return(v) => {
                match v {
                    Some(e) => {
                        let val = self.expr_as(e, &self.f.ret.clone())?;
                        self.pop_frame();
                        self.b.ins().return_(&[val]);
                    }
                    None => {
                        self.pop_frame();
                        self.b.ins().return_(&[]);
                    }
                }
                self.dead_block();
            }
            Stmt::Eval(e) => {
                self.expr(e)?;
            }
            Stmt::Line(n) => {
                self.cur_line = *n;
                self.b.set_srcloc(SourceLoc::new(*n));
                if self.m.debug_armed != 0 {
                    self.debug_call("ts_dbg_line", *n);
                }
            }
            Stmt::Throw(e) => {
                let v = self.expr(e)?;
                if self.m.debug_armed != 0 {
                    self.debug_call("ts_dbg_throw", self.cur_line);
                }
                self.rt("ts_rt_throw", &[types::I64], None, &[v]);
                let t = self.exc_target();
                self.b.ins().jump(t, &[]);
                self.dead_block();
            }
            Stmt::Try(body, var, handler) => {
                let (hb, join) = (self.b.create_block(), self.b.create_block());
                self.exc_stack.push(hb);
                self.stmts(body)?;
                self.exc_stack.pop();
                self.b.ins().jump(join, &[]);
                self.b.switch_to_block(hb);
                let exc = self
                    .rt("ts_rt_exc_take", &[], Some(types::I64), &[])
                    .unwrap();
                if let Some(v) = var {
                    self.b.def_var(self.vars[*v as usize], exc);
                    if let Some(i) = self.ptr_slot[*v as usize] {
                        self.root_store(i, exc);
                    }
                }
                self.stmts(handler)?;
                self.b.ins().jump(join, &[]);
                self.b.switch_to_block(join);
            }
        }
        Ok(())
    }

    fn load_slot(&mut self, ptr: Value, off: i32, t: &Ty) -> Value {
        self.b.ins().load(clif_ty(t), self.mf_t, ptr, off)
    }

    fn expr(&mut self, e: &Expr) -> R<Value> {
        let b_f64 = |s: &mut Self, v: f64| s.b.ins().f64const(v);
        Ok(match &e.kind {
            ExprKind::Num(n) => b_f64(self, *n),
            ExprKind::Bool(v) => self.b.ins().iconst(types::I8, *v as i64),
            ExprKind::Tag(t) => self.b.ins().iconst(types::I32, *t as i64),
            ExprKind::Str(i) => self.data_addr(self.sdata[*i as usize]),
            ExprKind::Blob(i) => self.data_addr(self.bdata[*i as usize]),
            ExprKind::Bits(a) => {
                let v = self.expr(a)?;
                self.slot_of(v, &a.ty)
            }
            ExprKind::Unbits(a) => {
                let v = self.expr(a)?;
                match clif_ty(&e.ty) {
                    types::F64 => self.b.ins().bitcast(types::F64, self.mf_p, v),
                    t @ (types::I8 | types::I32) => self.b.ins().ireduce(t, v),
                    _ => v,
                }
            }
            ExprKind::StrTag(a) => {
                let s = self.expr(a)?;
                let table = self.tagtab.ok_or("a tag was used before any tag exists")?;
                let base = self.data_addr(table);
                let count = self.b.ins().iconst(types::I64, self.m.tags.len() as i64);
                self.rt(
                    "ts_rt_str_tag",
                    &[types::I64, types::I64, types::I64],
                    Some(types::I32),
                    &[base, count, s],
                )
                .unwrap()
            }
            ExprKind::TagStr(a) => {
                let tag = self.expr(a)?;
                let table = self.tagtab.ok_or("a tag was used before any tag exists")?;
                let base = self.data_addr(table);
                let idx = self.b.ins().uextend(types::I64, tag);
                let off = self.b.ins().ishl_imm_u(idx, 3);
                let addr = self.b.ins().iadd(base, off);
                self.b.ins().load(types::I64, self.mf_t, addr, 0)
            }
            ExprKind::Null => self.null_of(&e.ty),
            ExprKind::Var(v) => self.b.use_var(self.vars[*v as usize]),
            ExprKind::Global(g) | ExprKind::GlobalVar(g) => {
                let addr = self.data_addr(self.gdata[*g]);
                self.load_slot(addr, 0, &e.ty)
            }
            ExprKind::Un(op, a) => {
                let v = self.expr(a)?;
                match op {
                    UnOp::Neg => self.b.ins().fneg(v),
                    UnOp::Not => self.b.ins().bxor_imm_s(v, 1),
                    UnOp::Sqrt => self.b.ins().sqrt(v),
                    UnOp::Abs => self.b.ins().fabs(v),
                    UnOp::Floor => self.b.ins().floor(v),
                    UnOp::Sin => self
                        .rt("ts_rt_sin", &[types::F64], Some(types::F64), &[v])
                        .unwrap(),
                    UnOp::Cos => self
                        .rt("ts_rt_cos", &[types::F64], Some(types::F64), &[v])
                        .unwrap(),
                    UnOp::Ceil => self.b.ins().ceil(v),
                    UnOp::Trunc => self.b.ins().trunc(v),
                    // floor(x), plus one when the fraction is at least a half; copysign keeps -0.
                    UnOp::Round => {
                        let r = self.b.ins().floor(v);
                        let frac = self.b.ins().fsub(v, r);
                        let (half, one) = (b_f64(self, 0.5), b_f64(self, 1.0));
                        let up = self.b.ins().fcmp(FloatCC::GreaterThanOrEqual, frac, half);
                        let r1 = self.b.ins().fadd(r, one);
                        let pick = self.b.ins().select(up, r1, r);
                        self.b.ins().fcopysign(pick, v)
                    }
                    UnOp::Fround => {
                        let s = self.b.ins().fdemote(types::F32, v);
                        self.b.ins().fpromote(types::F64, s)
                    }
                    UnOp::Clz32 => {
                        let i = self.cvt_i32(v);
                        let c = self.b.ins().clz(i);
                        self.b.ins().fcvt_from_uint(types::F64, c)
                    }
                    UnOp::Libm(name) => self
                        .rt(name, &[types::F64], Some(types::F64), &[v])
                        .unwrap(),
                    UnOp::Sign => {
                        let z = b_f64(self, 0.0);
                        let (one, neg) = (b_f64(self, 1.0), b_f64(self, -1.0));
                        let gt = self.b.ins().fcmp(FloatCC::GreaterThan, v, z);
                        let lt = self.b.ins().fcmp(FloatCC::LessThan, v, z);
                        let r = self.b.ins().select(lt, neg, v);
                        self.b.ins().select(gt, one, r)
                    }
                }
            }
            ExprKind::Bin(op, a, c) => {
                let x = self.expr(a)?;
                let y = self.expr(c)?;
                match op {
                    BinOp::Add => self.b.ins().fadd(x, y),
                    BinOp::Sub => self.b.ins().fsub(x, y),
                    BinOp::Mul => self.b.ins().fmul(x, y),
                    BinOp::Div => self.b.ins().fdiv(x, y),
                    BinOp::Rem => self.rem(x, y),
                    BinOp::Min => self.b.ins().fmin(x, y),
                    BinOp::Max => self.b.ins().fmax(x, y),
                    BinOp::Atan2 => self
                        .rt(
                            "ts_rt_atan2",
                            &[types::F64, types::F64],
                            Some(types::F64),
                            &[x, y],
                        )
                        .unwrap(),
                    BinOp::Pow => self
                        .rt(
                            "ts_rt_pow",
                            &[types::F64, types::F64],
                            Some(types::F64),
                            &[x, y],
                        )
                        .unwrap(),
                    // sqrt(x*x + y*y): within an ulp of libm's hypot, without the call.
                    BinOp::Hypot => {
                        let (xx, yy) = (self.b.ins().fmul(x, x), self.b.ins().fmul(y, y));
                        let s = self.b.ins().fadd(xx, yy);
                        self.b.ins().sqrt(s)
                    }
                    BinOp::UShr => {
                        let (xi, yi) = (self.cvt_i32(x), self.cvt_i32(y));
                        let r = self.b.ins().ushr(xi, yi);
                        self.b.ins().fcvt_from_uint(types::F64, r)
                    }
                    BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor
                    | BinOp::Shl
                    | BinOp::Shr
                    | BinOp::Imul => {
                        let (xi, yi) = (self.cvt_i32(x), self.cvt_i32(y));
                        let r = match op {
                            BinOp::BitAnd => self.b.ins().band(xi, yi),
                            BinOp::BitOr => self.b.ins().bor(xi, yi),
                            BinOp::BitXor => self.b.ins().bxor(xi, yi),
                            BinOp::Shl => self.b.ins().ishl(xi, yi),
                            BinOp::Shr => self.b.ins().sshr(xi, yi),
                            _ => self.b.ins().imul(xi, yi),
                        };
                        self.b.ins().fcvt_from_sint(types::F64, r)
                    }
                }
            }
            ExprKind::Cmp(op, a, c) => {
                // Pointer operands wait for each other like arguments do (an address compared
                // after being freed and reused would be wrong).
                let vals = self.eval_args(&[(a, a.ty.clone()), (c, c.ty.clone())])?;
                let (x, y) = (vals[0], vals[1]);
                if matches!(a.ty.strip(), Ty::Num) {
                    let cc = match op {
                        CmpOp::Eq => FloatCC::Equal,
                        CmpOp::Ne => FloatCC::NotEqual,
                        CmpOp::Lt => FloatCC::LessThan,
                        CmpOp::Le => FloatCC::LessThanOrEqual,
                        CmpOp::Gt => FloatCC::GreaterThan,
                        CmpOp::Ge => FloatCC::GreaterThanOrEqual,
                    };
                    self.b.ins().fcmp(cc, x, y)
                } else if matches!(a.ty.strip(), Ty::Str) {
                    // Strings compare by content (a null string equals only null).
                    let r = self
                        .rt(
                            "ts_rt_str_eq",
                            &[types::I64, types::I64],
                            Some(types::I8),
                            &[x, y],
                        )
                        .unwrap();
                    if *op == CmpOp::Eq {
                        r
                    } else {
                        self.b.ins().bxor_imm_u(r, 1)
                    }
                } else {
                    let cc = if *op == CmpOp::Eq {
                        IntCC::Equal
                    } else {
                        IntCC::NotEqual
                    };
                    self.b.ins().icmp(cc, x, y)
                }
            }
            ExprKind::And(a, c) | ExprKind::Or(a, c) => {
                let is_and = matches!(e.kind, ExprKind::And(..));
                let x = self.expr(a)?;
                let (rhs, join) = (self.b.create_block(), self.b.create_block());
                self.b.append_block_param(join, types::I8);
                if is_and {
                    self.b
                        .ins()
                        .brif(x, rhs, &[], join, &[ir::BlockArg::Value(x)]);
                } else {
                    self.b
                        .ins()
                        .brif(x, join, &[ir::BlockArg::Value(x)], rhs, &[]);
                }
                self.b.switch_to_block(rhs);
                let y = self.expr(c)?;
                self.b.ins().jump(join, &[ir::BlockArg::Value(y)]);
                self.b.switch_to_block(join);
                self.b.block_params(join)[0]
            }
            ExprKind::Cond(c, a, d) => {
                let cv = self.expr(c)?;
                let ct = clif_ty(&e.ty);
                let (tb, eb, join) = (
                    self.b.create_block(),
                    self.b.create_block(),
                    self.b.create_block(),
                );
                self.b.append_block_param(join, ct);
                self.b.ins().brif(cv, tb, &[], eb, &[]);
                self.b.switch_to_block(tb);
                let x = self.expr_as(a, &e.ty)?;
                self.b.ins().jump(join, &[ir::BlockArg::Value(x)]);
                self.b.switch_to_block(eb);
                let y = self.expr_as(d, &e.ty)?;
                self.b.ins().jump(join, &[ir::BlockArg::Value(y)]);
                self.b.switch_to_block(join);
                self.b.block_params(join)[0]
            }
            ExprKind::IsNull(a) => {
                let v = self.expr(a)?;
                if clif_ty(&a.ty) == types::F64 {
                    let bits = self.b.ins().bitcast(types::I64, self.mf_p, v);
                    self.b
                        .ins()
                        .icmp_imm_s(IntCC::Equal, bits, NULL_NUM_BITS as i64)
                } else {
                    self.b.ins().icmp_imm_s(IntCC::Equal, v, 0)
                }
            }
            ExprKind::Cast(a) => {
                let v = self.expr_as(a, &e.ty)?;
                // A `boolean | undefined` is a word: 0 undefined, 1 false, 2 true.
                match (&a.ty, &e.ty) {
                    (Ty::Bool, Ty::Nullable(i)) if **i == Ty::Bool => {
                        let w = self.b.ins().uextend(types::I64, v);
                        self.b.ins().iadd_imm_s(w, 1)
                    }
                    (Ty::Nullable(i), Ty::Bool) if **i == Ty::Bool => {
                        self.b.ins().icmp_imm_s(IntCC::Equal, v, 2)
                    }
                    _ => v,
                }
            }
            ExprKind::Truthy(a) => {
                let v = self.expr(a)?;
                let z = b_f64(self, 0.0);
                self.b.ins().fcmp(FloatCC::OrderedNotEqual, v, z)
            }
            ExprKind::Field(o, _sid, i) => {
                let ptr = self.expr(o)?;
                self.load_slot(ptr, field_offset(*i), &e.ty)
            }
            ExprKind::Index(a, i) => {
                let vals = self.eval_args(&[(a, a.ty.clone()), (i, i.ty.clone())])?;
                let (arr, idx) = (vals[0], vals[1]);
                let idx = self.cvt_i64(idx);
                let len = self.b.ins().load(types::I64, self.mf_t, arr, 8);
                let ok = self.b.ins().icmp(IntCC::UnsignedLessThan, idx, len);
                let ct = clif_ty(&e.ty);
                let (inb, oob, join) = (
                    self.b.create_block(),
                    self.b.create_block(),
                    self.b.create_block(),
                );
                self.b.append_block_param(join, ct);
                self.b.ins().brif(ok, inb, &[], oob, &[]);
                self.b.switch_to_block(inb);
                let data = self.b.ins().load(types::I64, self.mf_t, arr, 24);
                let v = if let Ty::Typed(kind) = a.ty.strip() {
                    self.typed_load(*kind, data, idx)
                } else {
                    let off = self.b.ins().imul_imm_s(idx, 8);
                    let addr = self.b.ins().iadd(data, off);
                    self.load_slot(addr, 0, &e.ty)
                };
                self.b.ins().jump(join, &[ir::BlockArg::Value(v)]);
                self.b.switch_to_block(oob);
                let d = if matches!(a.ty.strip(), Ty::Typed(_)) {
                    b_f64(self, 0.0)
                } else {
                    self.null_of(&e.ty)
                };
                self.b.ins().jump(join, &[ir::BlockArg::Value(d)]);
                self.b.switch_to_block(join);
                self.b.block_params(join)[0]
            }
            ExprKind::Len(a) => {
                let arr = self.expr(a)?;
                let len = self.b.ins().load(types::I64, self.mf_t, arr, 8);
                self.b.ins().fcvt_from_sint(types::F64, len)
            }
            ExprKind::NewObj(sid) => {
                let n = self.m.structs[*sid].fields.len();
                let size = self.b.ins().iconst(types::I64, 8 + 8 * n as i64);
                let h = self
                    .b
                    .ins()
                    .iconst(types::I64, self.m.struct_base as i64 + *sid as i64 + 1);
                self.rt(
                    "ts_rt_obj_new_gc",
                    &[types::I64, types::I64],
                    Some(types::I64),
                    &[h, size],
                )
                .unwrap()
            }
            ExprKind::CallExtern(i, args) => {
                let decl = &self.m.externs[*i];
                let list: Vec<(&Expr, Ty)> = args
                    .iter()
                    .zip(&decl.params)
                    .map(|(a, p)| (a, p.to_ir()))
                    .collect();
                let vals = self.eval_args(&list)?;
                // Scripts hold every number as an f64; the engine takes its own width.
                let mut conv = Vec::new();
                for (v, p) in vals.iter().zip(&decl.params) {
                    conv.push(match p {
                        ExtTy::F32 => self.b.ins().fdemote(types::F32, *v),
                        ExtTy::I32 => self.b.ins().fcvt_to_sint_sat(types::I32, *v),
                        ExtTy::U32 => self.b.ins().fcvt_to_uint_sat(types::I32, *v),
                        _ => *v,
                    });
                }
                let fr = self
                    .module
                    .declare_func_in_func(self.ext_funcs[*i], self.b.func);
                // A string result: the engine leaves the text in a buffer of its own and writes its
                // length through a trailing pointer; copy it into a new script string.
                if decl.ret == ExtTy::Str {
                    let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        8,
                        3,
                    ));
                    let len_addr = self.b.ins().stack_addr(types::I64, slot, 0);
                    conv.push(len_addr);
                    let call = self.b.ins().call(fr, &conv);
                    let text = self.b.inst_results(call)[0];
                    let len = self.b.ins().load(types::I64, self.mf_t, len_addr, 0);
                    return Ok(self
                        .rt(
                            "ts_rt_str_new_gc",
                            &[types::I64, types::I64],
                            Some(types::I64),
                            &[text, len],
                        )
                        .unwrap());
                }
                let call = self.b.ins().call(fr, &conv);
                let ret = decl.ret.clone();
                match (self.b.inst_results(call).first().copied(), ret) {
                    (Some(r), ExtTy::F32) => self.b.ins().fpromote(types::F64, r),
                    (Some(r), ExtTy::I32) => self.b.ins().fcvt_from_sint(types::F64, r),
                    (Some(r), ExtTy::U32) => self.b.ins().fcvt_from_uint(types::F64, r),
                    (Some(r), _) => r,
                    (None, _) => self.b.ins().iconst(types::I64, 0),
                }
            }
            ExprKind::CallExternInto(i, args, dest) => {
                let decl = &self.m.externs[*i];
                let (part, n) = decl.out.clone().expect("a value result has an out pointer");
                let list: Vec<(&Expr, Ty)> = args
                    .iter()
                    .zip(&decl.params)
                    .map(|(a, p)| (a, p.to_ir()))
                    .collect();
                let vals = self.eval_args(&list)?;
                let mut conv = Vec::new();
                for (v, p) in vals.iter().zip(&decl.params) {
                    conv.push(match p {
                        ExtTy::F32 => self.b.ins().fdemote(types::F32, *v),
                        ExtTy::I32 => self.b.ins().fcvt_to_sint_sat(types::I32, *v),
                        ExtTy::U32 => self.b.ins().fcvt_to_uint_sat(types::I32, *v),
                        _ => *v,
                    });
                }
                // Room for the result's scalars on the stack; the engine writes them there.
                let width = match ext_clif(&part) {
                    types::F64 => 8,
                    _ => 4,
                };
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    (width * n) as u32,
                    3,
                ));
                let addr = self.b.ins().stack_addr(types::I64, slot, 0);
                conv.push(addr);
                let fr = self
                    .module
                    .declare_func_in_func(self.ext_funcs[*i], self.b.func);
                self.b.ins().call(fr, &conv);
                // Copy the scalars into the fields of the destination object, as f64.
                let obj = self.expr(dest)?;
                for k in 0..n {
                    let raw =
                        self.b
                            .ins()
                            .load(ext_clif(&part), self.mf_t, addr, (width * k) as i32);
                    let num = match part {
                        ExtTy::F32 => self.b.ins().fpromote(types::F64, raw),
                        ExtTy::I32 => self.b.ins().fcvt_from_sint(types::F64, raw),
                        ExtTy::U32 => self.b.ins().fcvt_from_uint(types::F64, raw),
                        _ => raw,
                    };
                    self.b.ins().store(self.mf_t, num, obj, field_offset(k));
                }
                self.b.ins().iconst(types::I64, 0)
            }
            ExprKind::CallExternOut(i, args, vars) => {
                let decl = &self.m.externs[*i];
                let (part, n) = decl.out.clone().expect("a value result has an out pointer");
                let list: Vec<(&Expr, Ty)> = args
                    .iter()
                    .zip(&decl.params)
                    .map(|(a, p)| (a, p.to_ir()))
                    .collect();
                let vals = self.eval_args(&list)?;
                let mut conv = Vec::new();
                for (v, p) in vals.iter().zip(&decl.params) {
                    conv.push(match p {
                        ExtTy::F32 => self.b.ins().fdemote(types::F32, *v),
                        ExtTy::I32 => self.b.ins().fcvt_to_sint_sat(types::I32, *v),
                        ExtTy::U32 => self.b.ins().fcvt_to_uint_sat(types::I32, *v),
                        _ => *v,
                    });
                }
                let width = match ext_clif(&part) {
                    types::F64 => 8,
                    _ => 4,
                };
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    (width * n) as u32,
                    3,
                ));
                let addr = self.b.ins().stack_addr(types::I64, slot, 0);
                conv.push(addr);
                let fr = self
                    .module
                    .declare_func_in_func(self.ext_funcs[*i], self.b.func);
                self.b.ins().call(fr, &conv);
                for (k, v) in vars.iter().enumerate() {
                    let raw =
                        self.b
                            .ins()
                            .load(ext_clif(&part), self.mf_t, addr, (width * k) as i32);
                    let num = match part {
                        ExtTy::F32 => self.b.ins().fpromote(types::F64, raw),
                        ExtTy::I32 => self.b.ins().fcvt_from_sint(types::F64, raw),
                        ExtTy::U32 => self.b.ins().fcvt_from_uint(types::F64, raw),
                        _ => raw,
                    };
                    let var = self.vars[*v as usize];
                    self.b.def_var(var, num);
                }
                self.b.ins().iconst(types::I64, 0)
            }
            ExprKind::CallVirtual(table, args) => {
                // The signature comes from any entry; they agree up to the receiver's class.
                let fid = self.m.vtables[*table].entries[0].1;
                let ptys: Vec<Ty> = self.m.funcs[fid]
                    .params
                    .iter()
                    .map(|p| self.m.funcs[fid].locals[*p as usize].clone())
                    .collect();
                let list: Vec<(&Expr, Ty)> = args.iter().zip(ptys.clone()).collect();
                let vals = self.eval_args(&list)?;
                // The object's header holds its struct id: index the table with it.
                let header = self.b.ins().load(types::I64, self.mf_t, vals[0], 0);
                let kind = self.b.ins().band_imm_s(header, 0xFFFF_FFFF);
                let idx = self
                    .b
                    .ins()
                    .iadd_imm_s(kind, -(self.m.struct_base as i64 + 1));
                let off = self.b.ins().imul_imm_s(idx, 8);
                let base = self.data_addr(self.vtdata[*table]);
                let slot = self.b.ins().iadd(base, off);
                let target = self.b.ins().load(types::I64, self.mf_t, slot, 0);
                let sig = sig_of(self.call_conv, &ptys, &self.m.funcs[fid].ret);
                let sr = self.b.import_signature(sig);
                let call = self.b.ins().call_indirect(sr, target, &vals);
                let res = match self.b.inst_results(call).first() {
                    Some(v) => *v,
                    None => self.b.ins().iconst(types::I64, 0),
                };
                if self.m.any_throw {
                    self.check_exception();
                }
                res
            }
            ExprKind::InstanceOf(a, table) => {
                let v = self.expr(a)?;
                let (yes, no, join) = (
                    self.b.create_block(),
                    self.b.create_block(),
                    self.b.create_block(),
                );
                self.b.append_block_param(join, types::I8);
                let isnull = self.b.ins().icmp_imm_s(IntCC::Equal, v, 0);
                self.b.ins().brif(isnull, no, &[], yes, &[]);
                self.b.switch_to_block(yes);
                let header = self.b.ins().load(types::I64, self.mf_t, v, 0);
                let kind = self.b.ins().band_imm_s(header, 0xFFFF_FFFF);
                let idx = self
                    .b
                    .ins()
                    .iadd_imm_s(kind, -(self.m.struct_base as i64 + 1));
                let base = self.data_addr(self.itdata[*table]);
                let addr = self.b.ins().iadd(base, idx);
                let hit = self.b.ins().load(types::I8, self.mf_t, addr, 0);
                self.b.ins().jump(join, &[ir::BlockArg::Value(hit)]);
                self.b.switch_to_block(no);
                let zero = self.b.ins().iconst(types::I8, 0);
                self.b.ins().jump(join, &[ir::BlockArg::Value(zero)]);
                self.b.switch_to_block(join);
                self.b.block_params(join)[0]
            }
            ExprKind::Call(fid, args) => {
                let ptys: Vec<Ty> = self.m.funcs[*fid]
                    .params
                    .iter()
                    .map(|p| self.m.funcs[*fid].locals[*p as usize].clone())
                    .collect();
                let list: Vec<(&Expr, Ty)> = args.iter().zip(ptys).collect();
                let vals = self.eval_args(&list)?;
                let fr = self
                    .module
                    .declare_func_in_func(self.cfuncs[*fid], self.b.func);
                let call = self.b.ins().call(fr, &vals);
                let res = match self.b.inst_results(call).first() {
                    Some(v) => *v,
                    None => self.b.ins().iconst(types::I64, 0),
                };
                if self.m.may_throw[*fid] {
                    self.check_exception();
                }
                res
            }
            ExprKind::CallClosure(c, args) => {
                let Ty::Fn(sig) = c.ty.strip() else {
                    return Err("call of a non-function".into());
                };
                let mut list: Vec<(&Expr, Ty)> = vec![(&**c, c.ty.clone())];
                list.extend(args.iter().zip(sig.params.iter().cloned()));
                let evaled = self.eval_args(&list)?;
                let clo = evaled[0];
                let fp = self.b.ins().load(types::I64, self.mf_t, clo, 8);
                let env = self.b.ins().load(types::I64, self.mf_t, clo, 16);
                let mut vals = vec![env];
                vals.extend(&evaled[1..]);
                let mut s = Signature::new(self.call_conv);
                s.params.push(AbiParam::new(types::I64));
                for p in &sig.params {
                    s.params.push(AbiParam::new(clif_ty(p)));
                }
                if sig.ret != Ty::Void {
                    s.returns.push(AbiParam::new(clif_ty(&sig.ret)));
                }
                let sr = self.b.import_signature(s);
                let call = self.b.ins().call_indirect(sr, fp, &vals);
                let res = match self.b.inst_results(call).first() {
                    Some(v) => *v,
                    None => self.b.ins().iconst(types::I64, 0),
                };
                if self.m.any_throw {
                    self.check_exception();
                }
                res
            }
            ExprKind::NewCell(v) => {
                let val = self.expr(v)?;
                let slot = self.slot_of(val, &v.ty);
                let is_ptr = self.b.ins().iconst(types::I64, v.ty.is_ptr() as i64);
                self.rt(
                    "ts_rt_cell_new_gc",
                    &[types::I64, types::I64],
                    Some(types::I64),
                    &[slot, is_ptr],
                )
                .unwrap()
            }
            ExprKind::CellGet(c) => {
                let ptr = self.expr(c)?;
                self.load_slot(ptr, 8, &e.ty)
            }
            ExprKind::MakeClosure(fid, env) => {
                let env = match env {
                    Some(env) => self.expr(env)?,
                    None => self.b.ins().iconst(types::I64, 0),
                };
                let addr = self.func_addr(self.cfuncs[*fid]);
                self.rt(
                    "ts_rt_closure_new_gc",
                    &[types::I64, types::I64],
                    Some(types::I64),
                    &[addr, env],
                )
                .unwrap()
            }
            ExprKind::Rt(r, args) => self.rt_call(*r, args, &e.ty)?,
            ExprKind::Seq(stmts, v) => {
                self.stmts(stmts)?;
                if v.ty == Ty::Void {
                    // A void call still has to run (`Seq(temporaries, call)`).
                    self.expr(v)?;
                    self.b.ins().iconst(types::I64, 0)
                } else {
                    self.expr_as(v, &e.ty)?
                }
            }
        })
    }

    /// JS `%`. Whole numbers with a positive divisor (loop indexes, wrap-around) take an
    /// integer `srem`; anything else calls libm's fmod. (A negative dividend that leaves 0 gives
    /// +0 instead of JS's -0.)
    fn rem(&mut self, x: Value, y: Value) -> Value {
        let (xi, yi) = (self.cvt_i64(x), self.cvt_i64(y));
        let (xb, yb) = (
            self.b.ins().fcvt_from_sint(types::F64, xi),
            self.b.ins().fcvt_from_sint(types::F64, yi),
        );
        let (xe, ye) = (
            self.b.ins().fcmp(FloatCC::Equal, xb, x),
            self.b.ins().fcmp(FloatCC::Equal, yb, y),
        );
        let pos = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, yi, 0);
        let both = self.b.ins().band(xe, ye);
        let ok = self.b.ins().band(both, pos);
        let (fast, slow, join) = (
            self.b.create_block(),
            self.b.create_block(),
            self.b.create_block(),
        );
        self.b.append_block_param(join, types::F64);
        self.b.ins().brif(ok, fast, &[], slow, &[]);
        self.b.switch_to_block(fast);
        let r = self.b.ins().srem(xi, yi);
        let rf = self.b.ins().fcvt_from_sint(types::F64, r);
        self.b.ins().jump(join, &[ir::BlockArg::Value(rf)]);
        self.b.switch_to_block(slow);
        let m = self
            .rt(
                "ts_rt_fmod",
                &[types::F64, types::F64],
                Some(types::F64),
                &[x, y],
            )
            .unwrap();
        self.b.ins().jump(join, &[ir::BlockArg::Value(m)]);
        self.b.switch_to_block(join);
        self.b.block_params(join)[0]
    }

    fn rt_call(&mut self, r: RtFn, args: &[Expr], ty: &Ty) -> R<Value> {
        let list: Vec<(&Expr, Ty)> = args.iter().map(|a| (a, a.ty.clone())).collect();
        let vals = self.eval_args(&list)?;
        let dummy = |s: &mut Self| s.b.ins().iconst(types::I64, 0);
        Ok(match r {
            RtFn::Str(name) => {
                let params: Vec<ir::Type> = args.iter().map(|a| clif_ty(&a.ty)).collect();
                let ret = (*ty != Ty::Void).then(|| clif_ty(ty));
                match self.rt(name, &params, ret, &vals) {
                    Some(v) => v,
                    None => dummy(self),
                }
            }
            RtFn::Alloc => return Err("RtFn::Alloc is unused".into()),
            RtFn::ArrNew => {
                let cap = self.cvt_i64(vals[0]);
                let ptrs = matches!(ty.strip(), Ty::Arr(el) if el.is_ptr()) as i64;
                let ptrs = self.b.ins().iconst(types::I64, ptrs);
                self.rt(
                    "ts_rt_arr_new_gc",
                    &[types::I64, types::I64],
                    Some(types::I64),
                    &[cap, ptrs],
                )
                .unwrap()
            }
            RtFn::ArrPush => {
                let slot = self.slot_of(vals[1], &args[1].ty);
                self.rt(
                    "ts_rt_arr_push",
                    &[types::I64, types::I64],
                    None,
                    &[vals[0], slot],
                );
                dummy(self)
            }
            RtFn::ArrSetLen => {
                self.rt(
                    "ts_rt_arr_set_len",
                    &[types::I64, types::F64],
                    None,
                    &[vals[0], vals[1]],
                );
                dummy(self)
            }
            RtFn::ArrInsert => {
                let v = self.slot_of(vals[2], &args[2].ty);
                self.rt(
                    "ts_rt_arr_insert",
                    &[types::I64, types::F64, types::I64],
                    None,
                    &[vals[0], vals[1], v],
                );
                dummy(self)
            }
            RtFn::ArrDrop => {
                self.rt(
                    "ts_rt_arr_drop",
                    &[types::I64, types::F64, types::F64],
                    None,
                    &[vals[0], vals[1], vals[2]],
                );
                dummy(self)
            }
            RtFn::ArrSplice => self
                .rt(
                    "ts_rt_arr_splice",
                    &[types::I64, types::F64, types::F64, types::I64],
                    Some(types::I64),
                    &[vals[0], vals[1], vals[2], vals[3]],
                )
                .unwrap(),
            RtFn::ArrSlice => self
                .rt(
                    "ts_rt_arr_slice",
                    &[types::I64, types::F64, types::F64],
                    Some(types::I64),
                    &[vals[0], vals[1], vals[2]],
                )
                .unwrap(),
            RtFn::ArrAppend => {
                self.rt(
                    "ts_rt_arr_append",
                    &[types::I64, types::I64],
                    None,
                    &[vals[0], vals[1]],
                );
                dummy(self)
            }
            RtFn::ArrReverse => {
                self.rt("ts_rt_arr_reverse", &[types::I64], None, &[vals[0]]);
                dummy(self)
            }
            RtFn::ArrSort => {
                let ptrs = matches!(args[0].ty.strip(), Ty::Arr(el) if el.is_ptr()) as i64;
                let mode = self.b.ins().iconst(types::I64, ptrs);
                self.rt(
                    "ts_rt_arr_sort",
                    &[types::I64, types::I64, types::I64],
                    None,
                    &[vals[0], vals[1], mode],
                );
                dummy(self)
            }
            RtFn::ArrFill => {
                let v = self.slot_of(vals[1], &args[1].ty);
                self.rt(
                    "ts_rt_arr_fill",
                    &[types::I64, types::I64, types::F64, types::F64],
                    None,
                    &[vals[0], v, vals[2], vals[3]],
                );
                dummy(self)
            }
            RtFn::Random => self.rt("ts_rt_random", &[], Some(types::F64), &[]).unwrap(),
        })
    }
}
