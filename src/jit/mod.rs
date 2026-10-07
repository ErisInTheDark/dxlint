//! Rule expressions compiled to native code with Cranelift.
//!
//! Each compiled expression is one native function. Control flow, lambdas
//! over lists, and values whose type is known at config load (bools,
//! numbers, borrowed text) are native; other values live in frame slots and
//! go through small typed helpers that call the interpreter's own
//! operations, so expression semantics keep one owner. `ZLINT_JIT=verify`
//! checks every compiled expression against the interpreter.

mod helpers;
mod lower;
mod shape;

use std::mem::MaybeUninit;

use cranelift_codegen::ir::{types, AbiParam, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};

use crate::expr::{CExpr, Ctx, Value};

use helpers::{Helper, Ty};

/// Status codes compiled code and helpers return.
const FALSE: u8 = 0;
const TRUE: u8 = 1;
const FAILED: u8 = 2;

type Entry = unsafe extern "C" fn(*const Ctx<'static>, *mut Value<'static>, *mut Option<String>) -> u8;

/// What a compiled expression produces.
#[derive(Clone, Copy, PartialEq)]
pub enum Want {
    /// Only its truth (a condition).
    Truth,
    /// Its value.
    Value,
}

/// One compiled expression. Valid while the [`Code`] it came from lives,
/// and reads the expression it was compiled from, which must stay in place.
#[derive(Clone, Copy)]
pub struct Compiled(Entry);

impl Compiled {
    /// The truth of an expression compiled as [`Want::Truth`].
    pub fn test(self, ctx: &Ctx) -> Result<bool, String> {
        let mut error = None;
        // SAFETY: the code reads `ctx` only during this call, through the
        // helpers; conditions never write `out`.
        let status = unsafe { (self.0)(std::ptr::from_ref(ctx).cast::<Ctx<'static>>(), std::ptr::null_mut(), &mut error) };
        match status {
            FALSE => Ok(false),
            TRUE => Ok(true),
            _ => Err(error.unwrap_or_default()),
        }
    }

    /// The value of an expression compiled as [`Want::Value`].
    pub fn eval<'a>(self, ctx: &Ctx<'a>) -> Result<Value<'a>, String> {
        let mut error = None;
        let mut out = MaybeUninit::<Value<'a>>::uninit();
        // SAFETY: as for `test`; the code initialises `out` exactly when it
        // succeeds, with a value borrowing only what `ctx` does.
        let status = unsafe { (self.0)(std::ptr::from_ref(ctx).cast::<Ctx<'static>>(), out.as_mut_ptr().cast::<Value<'static>>(), &mut error) };
        match status {
            TRUE => Ok(unsafe { out.assume_init() }),
            _ => Err(error.unwrap_or_default()),
        }
    }
}

/// The executable memory of compiled expressions.
pub struct Code(Option<JITModule>);

// SAFETY: finished code is immutable and only executed; the module is
// touched again only to free it, which `Drop` does with exclusive access.
unsafe impl Send for Code {}
unsafe impl Sync for Code {}

impl Drop for Code {
    fn drop(&mut self) {
        if let Some(module) = self.0.take() {
            // SAFETY: compiled expressions are only called through the rules
            // owning them, which never outlive the linter that owns this.
            unsafe { module.free_memory() };
        }
    }
}

/// Whether compiling `e` does more than one interpreter call would.
pub fn worth_compiling(e: &CExpr) -> bool {
    match e {
        CExpr::Hole(inner, _) => worth_compiling(inner),
        CExpr::Not(_) | CExpr::If(..) | CExpr::Binary(..) | CExpr::Is(..) | CExpr::Test(..) | CExpr::TestDfa(..)
        | CExpr::Each(..) | CExpr::Key(..) | CExpr::Member(..) | CExpr::Call(..) | CExpr::Coalesce(..) | CExpr::Neg(_) => true,
        _ => false,
    }
}

/// Compiles `exprs` together, or `None` where the host has no backend (the
/// interpreter then runs them).
pub fn compile(exprs: &[(&CExpr, Want)]) -> Option<(Code, Vec<Compiled>)> {
    let mut builder = JITBuilder::new(default_libcall_names()).ok()?;
    for &h in Helper::ALL {
        let spec = h.spec();
        builder.symbol(spec.name, spec.address);
    }
    let mut module = JITModule::new(builder);
    let pointer = module.target_config().pointer_type();
    let ty = |t: Ty| match t {
        Ty::Ptr => pointer,
        Ty::I8 => types::I8,
        Ty::F64 => types::F64,
    };
    let mut ids: Vec<FuncId> = Vec::with_capacity(Helper::ALL.len());
    for &h in Helper::ALL {
        let spec = h.spec();
        let mut signature = module.make_signature();
        signature.params.extend(spec.params.iter().map(|&t| AbiParam::new(ty(t))));
        signature.returns.extend(spec.ret.map(|t| AbiParam::new(ty(t))));
        ids.push(module.declare_function(spec.name, Linkage::Import, &signature).ok()?);
    }
    let target = module.target_config();
    let mut entry = module.make_signature();
    entry.params.extend([AbiParam::new(pointer), AbiParam::new(pointer), AbiParam::new(pointer)]);
    entry.returns.push(AbiParam::new(types::I8));

    let mut context = module.make_context();
    let mut functions = FunctionBuilderContext::new();
    let mut defined: Vec<FuncId> = Vec::with_capacity(exprs.len());
    for (i, &(e, want)) in exprs.iter().enumerate() {
        let id = module.declare_function(&format!("expr{i}"), Linkage::Local, &entry).ok()?;
        // Loops push their items for the interpreter only when interpreted
        // code inside them reads one.
        for push_params in [false, true] {
            context.func.signature = entry.clone();
            context.func.name = UserFuncName::user(0, id.as_u32());
            let b = FunctionBuilder::new(&mut context.func, &mut functions);
            let mut lower = lower::Lower::new(b, &mut module, &ids, push_params);
            match want {
                Want::Truth => lower.condition(e),
                Want::Value => lower.value_root(e),
            }
            let needs_params = lower.needs_params;
            lower.b.finalize(target);
            if !needs_params {
                break;
            }
            module.clear_context(&mut context);
        }
        module.define_function(id, &mut context).ok()?;
        module.clear_context(&mut context);
        defined.push(id);
    }
    module.finalize_definitions().ok()?;
    let compiled = defined
        .into_iter()
        .map(|id| {
            // SAFETY: `id` was defined above with the `Entry` signature.
            Compiled(unsafe { std::mem::transmute::<*const u8, Entry>(module.get_finalized_function(id)) })
        })
        .collect();
    Some((Code(Some(module)), compiled))
}
