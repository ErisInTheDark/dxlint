//! Rule conditions compiled to native code with Cranelift.
//!
//! A condition (a rule's `when`) only needs a truth value. Its boolean
//! skeleton (`!`, `&&`, `||`, `if`) becomes native control flow, and leaves
//! whose type is known at config load run unboxed: numbers as floats, text
//! as borrowed bytes compared in place, constants never cloned. Everything
//! else is evaluated through [`Ctx::eval`], so expression semantics keep one
//! owner; `ZLINT_JIT=verify` checks every compiled condition against it.

use cranelift_codegen::ir::{self, condcodes::FloatCC, condcodes::IntCC, types, AbiParam, BlockArg, InstBuilder, StackSlotData, StackSlotKind, UserFuncName};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};

use crate::config::ast::BinOp;
use crate::expr::value::Text;
use crate::expr::{CExpr, Ctx, Value, BUILTIN_VARS};

/// Status codes compiled conditions and helpers return.
const FALSE: u8 = 0;
const TRUE: u8 = 1;
const FAILED: u8 = 2;

type Entry = unsafe extern "C" fn(*const Ctx<'static>, *mut Option<String>) -> u8;

/// One compiled condition. Valid while the [`Code`] it came from lives, and
/// reads the expression it was compiled from, which must stay in place.
#[derive(Clone, Copy)]
pub struct Condition(Entry);

impl Condition {
    pub fn test(self, ctx: &Ctx) -> Result<bool, String> {
        let mut error = None;
        // SAFETY: the code reads `ctx` only during this call, through the
        // helpers below; the expression pointers it embeds belong to the
        // rule that owns this condition and outlives it.
        let status = unsafe { (self.0)(std::ptr::from_ref(ctx).cast::<Ctx<'static>>(), &mut error) };
        match status {
            FALSE => Ok(false),
            TRUE => Ok(true),
            _ => Err(error.unwrap_or_default()),
        }
    }
}

/// The executable memory of compiled conditions.
pub struct Code(Option<JITModule>);

// SAFETY: finished code is immutable and only executed; the module is
// touched again only to free it, which `Drop` does with exclusive access.
unsafe impl Send for Code {}
unsafe impl Sync for Code {}

impl Drop for Code {
    fn drop(&mut self) {
        if let Some(module) = self.0.take() {
            // SAFETY: conditions are only called through the rules owning
            // them, which never outlive the linter that owns this.
            unsafe { module.free_memory() };
        }
    }
}

/// Whether compiling `e` does more than call the interpreter once.
pub fn worth_compiling(e: &CExpr) -> bool {
    match e {
        CExpr::Not(_) | CExpr::If(..) | CExpr::Binary(BinOp::And | BinOp::Or, ..) => true,
        CExpr::Binary(BinOp::Eq | BinOp::Ne, _, right) => matches!(**right, CExpr::Const(_)),
        CExpr::Binary(op, left, right) => comparison(*op).is_some() && is_num(left) && is_num(right),
        _ => shape(e) != Shape::Boxed,
    }
}

/// What a subexpression always evaluates to, when known at config load.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Bool,
    Num,
    /// Text borrowed from the file or a constant.
    Text,
    Null,
    /// Anything else: evaluated by the interpreter.
    Boxed,
}

fn builtin_shape(name: &str) -> Shape {
    match name {
        // Always text borrowed from the file or its path, so the bytes stay
        // valid after the helper's value is dropped.
        "text" | "before" | "after" | "file" | "text_before" | "text_after" | "source" => Shape::Text,
        "line" | "column" | "offset" | "end_offset" => Shape::Num,
        _ => Shape::Boxed,
    }
}

fn shape(e: &CExpr) -> Shape {
    match e {
        CExpr::Const(Value::Bool(_)) => Shape::Bool,
        CExpr::Const(Value::Num(_)) => Shape::Num,
        CExpr::Const(Value::Str(_)) => Shape::Text,
        CExpr::Const(Value::Null) => Shape::Null,
        CExpr::Builtin(i) => builtin_shape(BUILTIN_VARS[*i].0),
        _ => Shape::Boxed,
    }
}

fn is_num(e: &CExpr) -> bool {
    shape(e) == Shape::Num
}

/// The float comparison for an ordering operator, with the interpreter's
/// NaN rule (unordered compares as equal): `<=` is `!(a > b)`.
fn comparison(op: BinOp) -> Option<(FloatCC, bool)> {
    Some(match op {
        BinOp::Lt => (FloatCC::LessThan, false),
        BinOp::Gt => (FloatCC::GreaterThan, false),
        BinOp::Le => (FloatCC::GreaterThan, true),
        BinOp::Ge => (FloatCC::LessThan, true),
        _ => return None,
    })
}

/// Compiles `exprs` together, or `None` where the host has no backend (the
/// interpreter then runs them).
pub fn compile(exprs: &[&CExpr]) -> Option<(Code, Vec<Condition>)> {
    let mut builder = JITBuilder::new(default_libcall_names()).ok()?;
    builder.symbol("zl_truth", truth as *const u8);
    builder.symbol("zl_equal", equal as *const u8);
    builder.symbol("zl_text", text as *const u8);
    builder.symbol("zl_number", number as *const u8);
    builder.symbol("zl_bytes_equal", bytes_equal as *const u8);
    let mut module = JITModule::new(builder);
    let pointer = module.target_config().pointer_type();
    let declare = |module: &mut JITModule, name: &str, params: &[ir::Type], ret: ir::Type| {
        let mut call = module.make_signature();
        call.params.extend(params.iter().map(|&t| AbiParam::new(t)));
        call.returns.push(AbiParam::new(ret));
        module.declare_function(name, Linkage::Import, &call).ok()
    };
    let p = pointer;
    let helpers = Helpers {
        truth: declare(&mut module, "zl_truth", &[p, p, p], types::I8)?,
        equal: declare(&mut module, "zl_equal", &[p, p, p, p], types::I8)?,
        text: declare(&mut module, "zl_text", &[p, p, p, p], types::I8)?,
        number: declare(&mut module, "zl_number", &[p, p, p, p], types::I8)?,
        bytes_equal: declare(&mut module, "zl_bytes_equal", &[p, p, p, p], types::I8)?,
    };
    let mut entry = module.make_signature();
    entry.params.extend([AbiParam::new(pointer), AbiParam::new(pointer)]);
    entry.returns.push(AbiParam::new(types::I8));

    let mut context = module.make_context();
    let mut functions = FunctionBuilderContext::new();
    let mut ids: Vec<FuncId> = Vec::with_capacity(exprs.len());
    for (i, e) in exprs.iter().enumerate() {
        let id = module.declare_function(&format!("when{i}"), Linkage::Local, &entry).ok()?;
        context.func.signature = entry.clone();
        context.func.name = UserFuncName::user(0, id.as_u32());
        lower_function(&mut module, &mut context, &mut functions, helpers, pointer, e);
        module.define_function(id, &mut context).ok()?;
        module.clear_context(&mut context);
        ids.push(id);
    }
    module.finalize_definitions().ok()?;
    let conditions = ids
        .into_iter()
        .map(|id| {
            // SAFETY: `id` was defined above with the `Entry` signature.
            Condition(unsafe { std::mem::transmute::<*const u8, Entry>(module.get_finalized_function(id)) })
        })
        .collect();
    Some((Code(Some(module)), conditions))
}

/// The interpreter entry points compiled code calls.
#[derive(Clone, Copy)]
struct Helpers {
    truth: FuncId,
    equal: FuncId,
    text: FuncId,
    number: FuncId,
    bytes_equal: FuncId,
}

struct Refs {
    truth: ir::FuncRef,
    equal: ir::FuncRef,
    text: ir::FuncRef,
    number: ir::FuncRef,
    bytes_equal: ir::FuncRef,
}

fn lower_function(module: &mut JITModule, context: &mut Context, functions: &mut FunctionBuilderContext, helpers: Helpers, pointer: ir::Type, e: &CExpr) {
    let mut b = FunctionBuilder::new(&mut context.func, functions);
    let refs = Refs {
        truth: module.declare_func_in_func(helpers.truth, b.func),
        equal: module.declare_func_in_func(helpers.equal, b.func),
        text: module.declare_func_in_func(helpers.text, b.func),
        number: module.declare_func_in_func(helpers.number, b.func),
        bytes_equal: module.declare_func_in_func(helpers.bytes_equal, b.func),
    };
    let start = b.create_block();
    b.append_block_params_for_function_params(start);
    b.switch_to_block(start);
    let ctx = b.block_params(start)[0];
    let error = b.block_params(start)[1];
    let failed = b.create_block();
    // Out-parameters for helpers that return text or a number.
    let scratch = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3));
    let mut lower = Lower { b, refs, pointer, ctx, error, failed, scratch };
    let result = lower.truth(e);
    lower.b.ins().return_(&[result]);
    lower.b.switch_to_block(lower.failed);
    let status = lower.b.ins().iconst(types::I8, i64::from(FAILED));
    lower.b.ins().return_(&[status]);
    lower.b.seal_all_blocks();
    lower.b.finalize(module.target_config());
}

struct Lower<'f> {
    b: FunctionBuilder<'f>,
    refs: Refs,
    pointer: ir::Type,
    ctx: ir::Value,
    error: ir::Value,
    failed: ir::Block,
    scratch: ir::StackSlot,
}

/// An unboxed value: text is a pointer and a length.
enum Native {
    Num(ir::Value),
    Text(ir::Value, ir::Value),
}

impl Lower<'_> {
    /// `e`'s truth as 0 or 1, in the interpreter's evaluation order: `&&`
    /// and `||` skip their right side exactly where it does.
    fn truth(&mut self, e: &CExpr) -> ir::Value {
        match e {
            CExpr::Const(v) => self.b.ins().iconst(types::I8, i64::from(v.truthy())),
            CExpr::Not(inner) => {
                let v = self.truth(inner);
                self.b.ins().bxor_imm_u(v, 1)
            }
            CExpr::Binary(op @ (BinOp::And | BinOp::Or), left, right) => {
                let l = self.truth(left);
                let rest = self.b.create_block();
                let done = self.b.create_block();
                self.b.append_block_param(done, types::I8);
                if matches!(op, BinOp::And) {
                    self.b.ins().brif(l, rest, &[], done, &[BlockArg::Value(l)]);
                } else {
                    self.b.ins().brif(l, done, &[BlockArg::Value(l)], rest, &[]);
                }
                self.b.switch_to_block(rest);
                let r = self.truth(right);
                self.b.ins().jump(done, &[BlockArg::Value(r)]);
                self.b.switch_to_block(done);
                self.b.block_params(done)[0]
            }
            CExpr::If(condition, yes, no) => {
                let c = self.truth(condition);
                let then = self.b.create_block();
                let otherwise = self.b.create_block();
                let done = self.b.create_block();
                self.b.append_block_param(done, types::I8);
                self.b.ins().brif(c, then, &[], otherwise, &[]);
                self.b.switch_to_block(then);
                let y = self.truth(yes);
                self.b.ins().jump(done, &[BlockArg::Value(y)]);
                self.b.switch_to_block(otherwise);
                let n = self.truth(no);
                self.b.ins().jump(done, &[BlockArg::Value(n)]);
                self.b.switch_to_block(done);
                self.b.block_params(done)[0]
            }
            CExpr::Binary(op @ (BinOp::Eq | BinOp::Ne), left, right) => match self.equal(left, right) {
                Some(equal) if matches!(op, BinOp::Eq) => equal,
                Some(equal) => self.b.ins().bxor_imm_u(equal, 1),
                None => self.call(self.refs.truth, &[e_ptr(e)]),
            },
            CExpr::Binary(op, left, right) if is_num(left) && is_num(right) && comparison(*op).is_some() => {
                let (cc, negate) = comparison(*op).expect("checked above");
                let (Native::Num(a), Native::Num(b)) = (self.native(left), self.native(right)) else { unreachable!("numbers") };
                let r = self.b.ins().fcmp(cc, a, b);
                if negate { self.b.ins().bxor_imm_u(r, 1) } else { r }
            }
            _ => match shape(e) {
                Shape::Num => {
                    let Native::Num(n) = self.native(e) else { unreachable!("a number") };
                    let zero = self.b.ins().f64const(0.0);
                    self.b.ins().fcmp(FloatCC::NotEqual, n, zero)
                }
                Shape::Text => {
                    let Native::Text(_, len) = self.native(e) else { unreachable!("text") };
                    self.b.ins().icmp_imm_u(IntCC::NotEqual, len, 0)
                }
                _ => self.call(self.refs.truth, &[e_ptr(e)]),
            },
        }
    }

    /// `left == right` as 0 or 1, by the interpreter's loose equality, or
    /// `None` when only the interpreter can tell.
    fn equal(&mut self, left: &CExpr, right: &CExpr) -> Option<ir::Value> {
        Some(match (shape(left), shape(right)) {
            (Shape::Text, Shape::Text) => {
                let (Native::Text(ap, al), Native::Text(bp, bl)) = (self.native(left), self.native(right)) else { unreachable!("text") };
                let call = self.b.ins().call(self.refs.bytes_equal, &[ap, al, bp, bl]);
                self.b.inst_results(call)[0]
            }
            (Shape::Num, Shape::Num) => {
                let (Native::Num(a), Native::Num(b)) = (self.native(left), self.native(right)) else { unreachable!("numbers") };
                self.b.ins().fcmp(FloatCC::Equal, a, b)
            }
            // Null equals only empty text.
            (Shape::Text, Shape::Null) | (Shape::Null, Shape::Text) => {
                let text = if shape(left) == Shape::Text { left } else { right };
                let Native::Text(_, len) = self.native(text) else { unreachable!("text") };
                self.b.ins().icmp_imm_u(IntCC::Equal, len, 0)
            }
            // A constant compared in place, without cloning it.
            (_, _) if matches!(right, CExpr::Const(_)) => {
                let CExpr::Const(constant) = right else { unreachable!("matched above") };
                self.call(self.refs.equal, &[e_ptr(left), std::ptr::from_ref(constant).cast()])
            }
            _ => return None,
        })
    }

    /// An unboxed leaf (see [`shape`]).
    fn native(&mut self, e: &CExpr) -> Native {
        match e {
            CExpr::Const(Value::Num(n)) => Native::Num(self.b.ins().f64const(*n)),
            CExpr::Const(Value::Str(s)) => {
                let p = self.b.ins().iconst(self.pointer, s.as_ptr() as i64);
                let l = self.b.ins().iconst(self.pointer, s.len() as i64);
                Native::Text(p, l)
            }
            _ if shape(e) == Shape::Num => {
                self.call(self.refs.number, &[e_ptr(e), std::ptr::null()]);
                let n = self.b.ins().stack_load(self.pointer, types::F64, self.scratch, 0);
                Native::Num(n)
            }
            _ => {
                self.call(self.refs.text, &[e_ptr(e), std::ptr::null()]);
                let p = self.b.ins().stack_load(self.pointer, self.pointer, self.scratch, 0);
                let l = self.b.ins().stack_load(self.pointer, self.pointer, self.scratch, 8);
                Native::Text(p, l)
            }
        }
    }

    /// Calls a helper with the context, `pointers` (a null one stands for
    /// the scratch slot) and the error slot, leaving through `failed` when
    /// it reports an error.
    fn call(&mut self, helper: ir::FuncRef, pointers: &[*const u8]) -> ir::Value {
        let mut args = vec![self.ctx];
        for &p in pointers {
            args.push(if p.is_null() {
                self.b.ins().stack_addr(self.pointer, self.scratch, 0)
            } else {
                self.b.ins().iconst(self.pointer, p as i64)
            });
        }
        args.push(self.error);
        let call = self.b.ins().call(helper, &args);
        let status = self.b.inst_results(call)[0];
        let ok = self.b.create_block();
        let is_failure = self.b.ins().icmp_imm_u(IntCC::Equal, status, i64::from(FAILED));
        self.b.ins().brif(is_failure, self.failed, &[], ok, &[]);
        self.b.switch_to_block(ok);
        status
    }
}

fn e_ptr(e: &CExpr) -> *const u8 {
    std::ptr::from_ref(e).cast()
}

fn fail(error: *mut Option<String>, message: String) -> u8 {
    // SAFETY: compiled code passes the error slot of the `Condition::test`
    // call it runs under.
    unsafe { *error = Some(message) };
    FAILED
}

/// The interpreter, for one subexpression's truth.
unsafe extern "C" fn truth(ctx: *const Ctx<'static>, e: *const CExpr, error: *mut Option<String>) -> u8 {
    // SAFETY: compiled code passes the context of the `Condition::test` call
    // it runs under, and an expression its rule owns.
    let (ctx, e) = unsafe { (&*ctx, &*e) };
    match ctx.eval(e) {
        Ok(v) => u8::from(v.truthy()),
        Err(message) => fail(error, message),
    }
}

/// The interpreter, for whether `e`'s value equals a constant (`==`).
unsafe extern "C" fn equal(ctx: *const Ctx<'static>, e: *const CExpr, constant: *const Value<'static>, error: *mut Option<String>) -> u8 {
    // SAFETY: as for `truth`; the constant belongs to the same rule.
    let (ctx, e, constant) = unsafe { (&*ctx, &*e, &*constant) };
    match ctx.eval(e) {
        Ok(v) => u8::from(v.loose_eq(constant)),
        Err(message) => fail(error, message),
    }
}

/// A text-shaped leaf's bytes, borrowed from the file or its constant, as
/// (pointer, length) into `out`.
unsafe extern "C" fn text(ctx: *const Ctx<'static>, e: *const CExpr, out: *mut [usize; 2], error: *mut Option<String>) -> u8 {
    // SAFETY: as for `truth`; `out` is the caller's scratch slot.
    let (ctx, e) = unsafe { (&*ctx, &*e) };
    match ctx.eval(e) {
        // Borrowed text lives as long as the file; shared text would be
        // freed here, so text-shaped leaves must never produce it.
        Ok(Value::Str(Text::Borrowed(bytes))) => {
            // SAFETY: `out` is the caller's 16-byte scratch slot.
            unsafe { *out = [bytes.as_ptr() as usize, bytes.len()] };
            TRUE
        }
        Ok(other) => fail(error, format!("compiled condition expected text, got {}", other.type_name())),
        Err(message) => fail(error, message),
    }
}

/// A number-shaped leaf, as an `f64` into `out`.
unsafe extern "C" fn number(ctx: *const Ctx<'static>, e: *const CExpr, out: *mut f64, error: *mut Option<String>) -> u8 {
    // SAFETY: as for `text`.
    let (ctx, e) = unsafe { (&*ctx, &*e) };
    match ctx.eval(e) {
        Ok(Value::Num(n)) => {
            unsafe { *out = n };
            TRUE
        }
        Ok(other) => fail(error, format!("compiled condition expected a number, got {}", other.type_name())),
        Err(message) => fail(error, message),
    }
}

extern "C" fn bytes_equal(a: *const u8, a_len: usize, b: *const u8, b_len: usize) -> u8 {
    // SAFETY: both come from `text` or constants and are valid for their
    // lengths during the condition.
    let (a, b) = unsafe { (std::slice::from_raw_parts(a, a_len), std::slice::from_raw_parts(b, b_len)) };
    u8::from(a == b)
}
