//! One expression lowered to Cranelift IR.
//!
//! Control flow (`&&`, `||`, `!`, `if`, lambdas over lists) is native code;
//! bools, numbers and borrowed text stay unboxed in registers; everything
//! else is a [`Value`] in a frame slot, handed to [`super::helpers`] by
//! pointer. Owned slots are tracked while live, so a failing helper drops
//! exactly what was built so far before the function reports the error.

use cranelift_codegen::ir::{self, condcodes::FloatCC, condcodes::IntCC, types, BlockArg, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind};
use cranelift_frontend::FunctionBuilder;
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use crate::config::ast::BinOp;
use crate::expr::{CExpr, Each, Value};

use super::helpers::{Helper, VALUE_SIZE};
use super::shape::{self, Shape};
use super::{FAILED, TRUE};

/// A value slot in the frame (or an element of a slot array).
#[derive(Clone, Copy, PartialEq)]
struct Slot(ir::StackSlot, i32);

/// Something to undo when a helper fails.
#[derive(Clone, Copy, PartialEq)]
enum Live {
    /// An owned value to drop.
    Value(Slot),
    /// A list being built (`*mut Vec<Value>`) to free.
    List(ir::Value),
    /// A lambda parameter pushed for the interpreter, to pop.
    Param,
}

/// A lowered value.
enum Lowered {
    Bool(ir::Value),
    Num(ir::Value),
    /// Borrowed text: pointer and length.
    Text(ir::Value, ir::Value),
    /// A value owned elsewhere (a constant, a list item), valid throughout.
    Ref(ir::Value),
    /// A value this code owns and must drop or move.
    Owned(Slot),
}

pub(super) struct Lower<'f, 'm> {
    pub(super) b: FunctionBuilder<'f>,
    module: &'m mut JITModule,
    helpers: &'m [FuncId],
    refs: Vec<Option<ir::FuncRef>>,
    pointer: ir::Type,
    ctx: ir::Value,
    /// Where a value root writes its result.
    out: ir::Value,
    error: ir::Value,
    failed: ir::Block,
    /// Out-parameters for helpers returning text or a number.
    scratch: ir::StackSlot,
    live: Vec<Live>,
    /// Lambda parameters bound by compiled loops: depth and item pointer.
    loops: Vec<(u16, ir::Value)>,
    /// Whether compiled loops also push their items, for interpreted code
    /// inside them that reads one.
    push_params: bool,
    /// Set when interpreted code reads a parameter only a compiled loop
    /// binds while `push_params` is off: the function must be lowered again
    /// with it on.
    pub(super) needs_params: bool,
}

impl<'f, 'm> Lower<'f, 'm> {
    /// Starts a function `(ctx, out, error) -> status` (`out` unused by
    /// conditions).
    pub(super) fn new(mut b: FunctionBuilder<'f>, module: &'m mut JITModule, helpers: &'m [FuncId], push_params: bool) -> Self {
        let pointer = module.target_config().pointer_type();
        let start = b.create_block();
        b.append_block_params_for_function_params(start);
        b.switch_to_block(start);
        let ctx = b.block_params(start)[0];
        let out = b.block_params(start)[1];
        let error = b.block_params(start)[2];
        let failed = b.create_block();
        let scratch = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3));
        Lower {
            b,
            module,
            helpers,
            refs: vec![None; helpers.len()],
            pointer,
            ctx,
            out,
            error,
            failed,
            scratch,
            live: Vec::new(),
            loops: Vec::new(),
            push_params,
            needs_params: false,
        }
    }

    /// Lowers a condition's body: returns `e`'s truth.
    pub(super) fn condition(&mut self, e: &CExpr) {
        let t = self.truth(e);
        self.b.ins().return_(&[t]);
        self.finish();
    }

    /// Lowers a value's body: writes `e` to the `out` parameter.
    pub(super) fn value_root(&mut self, e: &CExpr) {
        self.value_into(e, self.out);
        let ok = self.b.ins().iconst(types::I8, i64::from(TRUE));
        self.b.ins().return_(&[ok]);
        self.finish();
    }

    fn finish(&mut self) {
        debug_assert!(self.live.is_empty(), "every owned value is consumed");
        self.b.switch_to_block(self.failed);
        let status = self.b.ins().iconst(types::I8, i64::from(FAILED));
        self.b.ins().return_(&[status]);
        self.b.seal_all_blocks();
    }

    // ---- helpers ----------------------------------------------------------

    fn helper(&mut self, h: Helper) -> ir::FuncRef {
        let i = h as usize;
        match self.refs[i] {
            Some(r) => r,
            None => {
                let r = self.module.declare_func_in_func(self.helpers[i], self.b.func);
                self.refs[i] = Some(r);
                r
            }
        }
    }

    fn call(&mut self, h: Helper, args: &[ir::Value]) -> Option<ir::Value> {
        let f = self.helper(h);
        let call = self.b.ins().call(f, args);
        self.b.inst_results(call).first().copied()
    }

    /// Calls a helper returning a status, leaving through the failure path
    /// (after undoing everything live) when it fails. Returns the status.
    fn checked(&mut self, h: Helper, args: &[ir::Value]) -> ir::Value {
        let status = self.call(h, args).expect("a status");
        let failure = self.b.ins().icmp_imm_u(IntCC::Equal, status, i64::from(FAILED));
        let ok = self.b.create_block();
        if self.live.is_empty() {
            self.b.ins().brif(failure, self.failed, &[], ok, &[]);
        } else {
            let undo = self.b.create_block();
            self.b.ins().brif(failure, undo, &[], ok, &[]);
            self.b.switch_to_block(undo);
            for live in self.live.clone().into_iter().rev() {
                match live {
                    Live::Value(slot) => {
                        let a = self.addr(slot);
                        self.call(Helper::DropValue, &[a]);
                    }
                    Live::List(list) => {
                        self.call(Helper::ListFree, &[list]);
                    }
                    Live::Param => {
                        self.call(Helper::PopParam, &[self.ctx]);
                    }
                }
            }
            self.b.ins().jump(self.failed, &[]);
        }
        self.b.switch_to_block(ok);
        status
    }

    fn constant(&mut self, p: *const u8) -> ir::Value {
        self.b.ins().iconst(self.pointer, p as i64)
    }

    fn index(&mut self, i: usize) -> ir::Value {
        self.b.ins().iconst(self.pointer, i as i64)
    }

    fn expr(&mut self, e: &CExpr) -> ir::Value {
        self.constant(std::ptr::from_ref(e).cast())
    }

    fn slot(&mut self) -> Slot {
        Slot(self.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, VALUE_SIZE as u32, 3)), 0)
    }

    fn addr(&mut self, slot: Slot) -> ir::Value {
        self.b.ins().stack_addr(self.pointer, slot.0, slot.1)
    }

    fn own(&mut self, slot: Slot) {
        self.live.push(Live::Value(slot));
    }

    fn release(&mut self, slot: Slot) {
        let at = self.live.iter().rposition(|l| *l == Live::Value(slot)).expect("a live slot");
        self.live.remove(at);
    }

    fn scratch_addr(&mut self) -> ir::Value {
        self.b.ins().stack_addr(self.pointer, self.scratch, 0)
    }

    /// The two pointer-sized words a helper wrote to the scratch slot.
    fn scratch_pair(&mut self) -> (ir::Value, ir::Value) {
        let first = self.b.ins().stack_load(self.pointer, self.pointer, self.scratch, 0);
        let second = self.b.ins().stack_load(self.pointer, self.pointer, self.scratch, 8);
        (first, second)
    }

    // ---- values -----------------------------------------------------------

    /// `l` as a pointer to a value, boxing unboxed values; the slot to drop
    /// afterwards (with [`Lower::done`]) when owned.
    fn boxed(&mut self, l: Lowered) -> (ir::Value, Option<Slot>) {
        match l {
            Lowered::Ref(p) => (p, None),
            Lowered::Owned(slot) => (self.addr(slot), Some(slot)),
            other => {
                let slot = self.slot();
                let a = self.addr(slot);
                self.write(other, a);
                self.own(slot);
                (a, Some(slot))
            }
        }
    }

    fn done(&mut self, owned: Option<Slot>) {
        if let Some(slot) = owned {
            self.release(slot);
            let a = self.addr(slot);
            self.call(Helper::DropValue, &[a]);
        }
    }

    /// Writes `l` as an owned value to the uninitialised `dst`.
    fn write(&mut self, l: Lowered, dst: ir::Value) {
        match l {
            Lowered::Bool(b) => {
                self.call(Helper::MakeBool, &[dst, b]);
            }
            Lowered::Num(n) => {
                self.call(Helper::MakeNum, &[dst, n]);
            }
            Lowered::Text(p, len) => {
                self.call(Helper::MakeText, &[dst, p, len]);
            }
            Lowered::Ref(p) => {
                self.call(Helper::CloneValue, &[p, dst]);
            }
            Lowered::Owned(slot) => {
                self.release(slot);
                let src = self.addr(slot);
                for offset in (0..VALUE_SIZE as i32).step_by(8) {
                    let word = self.b.ins().load(types::I64, MemFlagsData::trusted(), src, offset);
                    self.b.ins().store(MemFlagsData::trusted(), word, dst, offset);
                }
            }
        }
    }

    fn value_into(&mut self, e: &CExpr, dst: ir::Value) {
        let l = self.value(e);
        self.write(l, dst);
    }

    /// An owned slot written by a helper call that may fail.
    fn produced(&mut self, h: Helper, args: impl FnOnce(&mut Self, ir::Value) -> Vec<ir::Value>) -> Lowered {
        let slot = self.slot();
        let a = self.addr(slot);
        let args = args(self, a);
        self.checked(h, &args);
        self.own(slot);
        Lowered::Owned(slot)
    }

    fn value(&mut self, e: &CExpr) -> Lowered {
        match e {
            CExpr::Const(Value::Bool(b)) => Lowered::Bool(self.b.ins().iconst(types::I8, i64::from(*b))),
            CExpr::Const(Value::Num(n)) => Lowered::Num(self.b.ins().f64const(*n)),
            CExpr::Const(v) => Lowered::Ref(self.constant(std::ptr::from_ref(v).cast())),
            CExpr::Hole(inner, _) => self.value(inner),
            CExpr::Builtin(b) => {
                let i = self.index(*b as usize);
                match shape::builtin(*b) {
                    Shape::Text => {
                        let out = self.scratch_addr();
                        self.checked(Helper::BuiltinText, &[self.ctx, i, out, self.error]);
                        let (p, len) = self.scratch_pair();
                        Lowered::Text(p, len)
                    }
                    Shape::Num => {
                        let out = self.scratch_addr();
                        self.checked(Helper::BuiltinNum, &[self.ctx, i, out, self.error]);
                        Lowered::Num(self.b.ins().stack_load(self.pointer, types::F64, self.scratch, 0))
                    }
                    _ => self.produced(Helper::Builtin, |l, out| vec![l.ctx, i, out, l.error]),
                }
            }
            CExpr::Param(depth) => match self.loops.iter().rev().find(|(d, _)| d == depth) {
                Some(&(_, item)) => Lowered::Ref(item),
                None => {
                    let slot = self.slot();
                    let a = self.addr(slot);
                    let d = self.index(*depth as usize);
                    self.call(Helper::Param, &[self.ctx, d, a]);
                    self.own(slot);
                    Lowered::Owned(slot)
                }
            },
            CExpr::Local(i) => {
                let i = self.index(*i as usize);
                self.produced(Helper::Local, |l, out| vec![l.ctx, i, out, l.error])
            }
            CExpr::Not(_) | CExpr::Is(..) | CExpr::Test(..) | CExpr::TestDfa(..) | CExpr::Each(Each::Any | Each::All, ..) => {
                Lowered::Bool(self.truth(e))
            }
            CExpr::Binary(op, ..) if shape::is_boolean(*op) => Lowered::Bool(self.truth(e)),
            CExpr::If(c, yes, no) => self.choice(e, c, yes, no),
            CExpr::Neg(inner) if shape::of(inner) == Shape::Num => {
                let n = self.num(inner);
                Lowered::Num(self.b.ins().fneg(n))
            }
            CExpr::Key(base, key) => {
                let base = self.value(base);
                let (p, owned) = self.boxed(base);
                let slot = self.slot();
                let a = self.addr(slot);
                let k = self.constant(key.as_ptr());
                let len = self.index(key.len());
                self.call(Helper::Key, &[p, k, len, a]);
                self.done(owned);
                self.own(slot);
                Lowered::Owned(slot)
            }
            CExpr::Member(base, _, part) => {
                if let Some(part) = part {
                    self.interpreted(part);
                }
                let base = self.value(base);
                let (p, owned) = self.boxed(base);
                let me = self.expr(e);
                let slot = self.slot();
                let a = self.addr(slot);
                self.checked(Helper::Member, &[self.ctx, p, me, a, self.error]);
                self.done(owned);
                self.own(slot);
                Lowered::Owned(slot)
            }
            CExpr::Binary(_, left, right) => {
                let left = self.value(left);
                let (a, a_owned) = self.boxed(left);
                let right = self.value(right);
                let (b, b_owned) = self.boxed(right);
                let me = self.expr(e);
                let slot = self.slot();
                let out = self.addr(slot);
                self.checked(Helper::Binary, &[me, a, b, out, self.error]);
                self.done(b_owned);
                self.done(a_owned);
                self.own(slot);
                Lowered::Owned(slot)
            }
            CExpr::Call(f, args) => {
                let array = self.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, (VALUE_SIZE * args.len().max(1)) as u32, 3));
                for (i, arg) in args.iter().enumerate() {
                    let item = Slot(array, (i * VALUE_SIZE) as i32);
                    let l = self.value(arg);
                    let a = self.addr(item);
                    self.write(l, a);
                    self.own(item);
                }
                // The helper consumes the arguments, even when it fails.
                for i in 0..args.len() {
                    self.release(Slot(array, (i * VALUE_SIZE) as i32));
                }
                let function = self.constant(std::ptr::from_ref(*f).cast());
                let first = self.b.ins().stack_addr(self.pointer, array, 0);
                let n = self.index(args.len());
                self.produced(Helper::Call, |l, out| vec![function, first, n, out, l.error])
            }
            CExpr::Each(each, depth, list, body) => self.each(*each, *depth, list, body),
            CExpr::Coalesce(first, other) => {
                let first = self.value(first);
                let (p, owned) = self.boxed(first);
                let is_node = self.call(Helper::IsNode, &[p]).expect("a bool");
                let slot = self.slot();
                let out = self.addr(slot);
                let keep = self.b.create_block();
                let instead = self.b.create_block();
                let join = self.b.create_block();
                // Both paths consume `first`: moved or cloned when kept,
                // dropped otherwise.
                if let Some(s) = owned {
                    self.release(s);
                }
                self.b.ins().brif(is_node, keep, &[], instead, &[]);
                self.b.switch_to_block(keep);
                match owned {
                    Some(s) => {
                        self.own(s);
                        self.write(Lowered::Owned(s), out);
                    }
                    None => self.write(Lowered::Ref(p), out),
                }
                self.b.ins().jump(join, &[]);
                self.b.switch_to_block(instead);
                if owned.is_some() {
                    self.call(Helper::DropValue, &[p]);
                }
                self.value_into(other, out);
                self.b.ins().jump(join, &[]);
                self.b.switch_to_block(join);
                self.own(slot);
                Lowered::Owned(slot)
            }
            _ => {
                self.interpreted(e);
                let me = self.expr(e);
                self.produced(Helper::Eval, |l, out| vec![l.ctx, me, out, l.error])
            }
        }
    }

    /// Notes that the interpreter evaluates `e`: it must see the parameters
    /// of compiled loops it reads, and its own lambdas must bind theirs at
    /// the depths they read, past those loops' parameters.
    fn interpreted(&mut self, e: &CExpr) {
        if let Some(&(outermost, _)) = self.loops.first() {
            if !self.push_params && reads_param(e, &|d| d >= outermost) {
                self.needs_params = true;
            }
        }
    }

    fn num(&mut self, e: &CExpr) -> ir::Value {
        match self.value(e) {
            Lowered::Num(n) => n,
            _ => unreachable!("a number-shaped expression"),
        }
    }

    fn text(&mut self, e: &CExpr) -> (ir::Value, ir::Value) {
        if let CExpr::Const(Value::Str(s)) = e {
            let p = self.constant(s.as_ptr());
            let len = self.index(s.len());
            return (p, len);
        }
        match self.value(e) {
            Lowered::Text(p, len) => (p, len),
            _ => unreachable!("a text-shaped expression"),
        }
    }

    /// `if c then yes else no`.
    fn choice(&mut self, e: &CExpr, c: &CExpr, yes: &CExpr, no: &CExpr) -> Lowered {
        let shape = shape::of(e);
        if shape == Shape::Bool {
            return Lowered::Bool(self.truth(e));
        }
        let condition = self.truth(c);
        // A boxed result is written by both branches to one slot, addressed
        // before the branch so both see the address.
        let boxed = (!matches!(shape, Shape::Num | Shape::Text)).then(|| {
            let slot = self.slot();
            (slot, self.addr(slot))
        });
        let then = self.b.create_block();
        let otherwise = self.b.create_block();
        let join = self.b.create_block();
        self.b.ins().brif(condition, then, &[], otherwise, &[]);
        match shape {
            Shape::Num => {
                self.b.append_block_param(join, types::F64);
                for (block, branch) in [(then, yes), (otherwise, no)] {
                    self.b.switch_to_block(block);
                    let n = self.num(branch);
                    self.b.ins().jump(join, &[BlockArg::Value(n)]);
                }
                self.b.switch_to_block(join);
                Lowered::Num(self.b.block_params(join)[0])
            }
            Shape::Text => {
                self.b.append_block_param(join, self.pointer);
                self.b.append_block_param(join, self.pointer);
                for (block, branch) in [(then, yes), (otherwise, no)] {
                    self.b.switch_to_block(block);
                    let (p, len) = self.text(branch);
                    self.b.ins().jump(join, &[BlockArg::Value(p), BlockArg::Value(len)]);
                }
                self.b.switch_to_block(join);
                let params = self.b.block_params(join);
                Lowered::Text(params[0], params[1])
            }
            _ => {
                let (slot, out) = boxed.expect("addressed before branching");
                for (block, branch) in [(then, yes), (otherwise, no)] {
                    self.b.switch_to_block(block);
                    self.value_into(branch, out);
                    self.b.ins().jump(join, &[]);
                }
                self.b.switch_to_block(join);
                self.own(slot);
                Lowered::Owned(slot)
            }
        }
    }

    // ---- truth ------------------------------------------------------------

    /// `e`'s truth as 0 or 1, in the interpreter's evaluation order: `&&`
    /// and `||` skip their right side exactly where it does.
    fn truth(&mut self, e: &CExpr) -> ir::Value {
        match e {
            CExpr::Const(v) => self.b.ins().iconst(types::I8, i64::from(v.truthy())),
            CExpr::Hole(inner, _) => self.truth(inner),
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
            CExpr::Binary(op @ (BinOp::Eq | BinOp::Ne), left, right) => {
                let equal = self.equal(left, right);
                if matches!(op, BinOp::Eq) { equal } else { self.b.ins().bxor_imm_u(equal, 1) }
            }
            CExpr::Binary(op @ (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), left, right) => self.ordered(*op, left, right),
            CExpr::Is(inner, _) => {
                let v = self.value(inner);
                let (p, owned) = self.boxed(v);
                let me = self.expr(e);
                let r = self.call(Helper::Is, &[self.ctx, p, me]).expect("a bool");
                self.done(owned);
                r
            }
            CExpr::Test(inner, _) | CExpr::TestDfa(inner, _) => {
                let v = self.value(inner);
                let (p, owned) = self.boxed(v);
                let me = self.expr(e);
                let r = self.call(Helper::Test, &[self.ctx, me, p]).expect("a bool");
                self.done(owned);
                r
            }
            CExpr::Each(each @ (Each::Any | Each::All), depth, list, body) => match self.each(*each, *depth, list, body) {
                Lowered::Bool(b) => b,
                _ => unreachable!("any and all are bools"),
            },
            _ => match self.value(e) {
                Lowered::Bool(b) => b,
                Lowered::Num(n) => {
                    let zero = self.b.ins().f64const(0.0);
                    self.b.ins().fcmp(FloatCC::NotEqual, n, zero)
                }
                Lowered::Text(_, len) => self.b.ins().icmp_imm_u(IntCC::NotEqual, len, 0),
                other => {
                    let (p, owned) = self.boxed(other);
                    let t = self.call(Helper::Truthy, &[p]).expect("a bool");
                    self.done(owned);
                    t
                }
            },
        }
    }

    /// `left == right` as 0 or 1, by the interpreter's loose equality.
    fn equal(&mut self, left: &CExpr, right: &CExpr) -> ir::Value {
        match (shape::of(left), shape::of(right)) {
            (Shape::Text, Shape::Text) => {
                let (ap, al) = self.text(left);
                let (bp, bl) = self.text(right);
                self.call(Helper::BytesEqual, &[ap, al, bp, bl]).expect("a bool")
            }
            (Shape::Num, Shape::Num) => {
                let a = self.num(left);
                let b = self.num(right);
                self.b.ins().fcmp(FloatCC::Equal, a, b)
            }
            // Null equals only empty text.
            (Shape::Text, Shape::Null) => {
                let (_, len) = self.text(left);
                self.b.ins().icmp_imm_u(IntCC::Equal, len, 0)
            }
            (Shape::Null, Shape::Text) => {
                let (_, len) = self.text(right);
                self.b.ins().icmp_imm_u(IntCC::Equal, len, 0)
            }
            _ => {
                let l = self.value(left);
                let (a, a_owned) = self.boxed(l);
                let r = self.value(right);
                let (b, b_owned) = self.boxed(r);
                let equal = self.call(Helper::Equal, &[a, b]).expect("a bool");
                self.done(b_owned);
                self.done(a_owned);
                equal
            }
        }
    }

    /// An ordering comparison, with the interpreter's rule that unordered
    /// numbers (NaN) compare as equal.
    fn ordered(&mut self, op: BinOp, left: &CExpr, right: &CExpr) -> ir::Value {
        if shape::of(left) == Shape::Num && shape::of(right) == Shape::Num {
            let a = self.num(left);
            let b = self.num(right);
            let (cc, negate) = match op {
                BinOp::Lt => (FloatCC::LessThan, false),
                BinOp::Gt => (FloatCC::GreaterThan, false),
                BinOp::Le => (FloatCC::GreaterThan, true),
                _ => (FloatCC::LessThan, true),
            };
            let r = self.b.ins().fcmp(cc, a, b);
            return if negate { self.b.ins().bxor_imm_u(r, 1) } else { r };
        }
        let l = self.value(left);
        let (a, a_owned) = self.boxed(l);
        let r = self.value(right);
        let (b, b_owned) = self.boxed(r);
        let order = self.call(Helper::Compare, &[a, b]).expect("an ordering");
        self.done(b_owned);
        self.done(a_owned);
        // 0 less, 1 equal, 2 greater.
        match op {
            BinOp::Lt => self.b.ins().icmp_imm_u(IntCC::Equal, order, 0),
            BinOp::Le => self.b.ins().icmp_imm_u(IntCC::NotEqual, order, 2),
            BinOp::Gt => self.b.ins().icmp_imm_u(IntCC::Equal, order, 2),
            _ => self.b.ins().icmp_imm_u(IntCC::NotEqual, order, 0),
        }
    }

    // ---- lambdas ----------------------------------------------------------

    /// `map`, `filter`, `find`, `find_last`, `all` or `any` as a native loop
    /// over the list's items, the item borrowed in place.
    fn each(&mut self, each: Each, depth: u16, list: &CExpr, body: &CExpr) -> Lowered {
        let list = self.value(list);
        let (list, list_owned) = self.boxed(list);
        let out = self.scratch_addr();
        self.call(Helper::Items, &[list, out]);
        let (base, len) = self.scratch_pair();

        let building = matches!(each, Each::Map | Each::Filter).then(|| {
            let built = self.call(Helper::ListNew, &[len]).expect("a list");
            self.live.push(Live::List(built));
            built
        });
        let result = matches!(each, Each::Find | Each::FindLast | Each::Map | Each::Filter).then(|| self.slot());

        let backwards = each == Each::FindLast;
        let header = self.b.create_block();
        self.b.append_block_param(header, self.pointer);
        let step = self.b.create_block();
        let next = self.b.create_block();
        let end = self.b.create_block();
        let exit = self.b.create_block();
        self.b.append_block_param(exit, types::I8);
        let start = if backwards { len } else { self.index(0) };
        // Defined before the loop: the body may branch, so only this block
        // dominates every exit that passes them.
        let yes = self.b.ins().iconst(types::I8, 1);
        let no = self.b.ins().iconst(types::I8, 0);
        self.b.ins().jump(header, &[BlockArg::Value(start)]);

        self.b.switch_to_block(header);
        let i = self.b.block_params(header)[0];
        let more = if backwards { self.b.ins().icmp_imm_u(IntCC::NotEqual, i, 0) } else { self.b.ins().icmp(IntCC::UnsignedLessThan, i, len) };
        self.b.ins().brif(more, step, &[], end, &[]);

        self.b.switch_to_block(step);
        let at = if backwards { self.b.ins().iadd_imm_u(i, -1i64) } else { i };
        let offset = self.b.ins().imul_imm_u(at, VALUE_SIZE as i64);
        let item = self.b.ins().iadd(base, offset);
        if self.push_params {
            self.call(Helper::PushParam, &[self.ctx, item]);
            self.live.push(Live::Param);
        }
        self.loops.push((depth, item));
        let mapped = (each == Each::Map).then(|| {
            let slot = self.slot();
            let a = self.addr(slot);
            self.value_into(body, a);
            a
        });
        let hit = (each != Each::Map).then(|| self.truth(body));
        self.loops.pop();
        if self.push_params {
            self.call(Helper::PopParam, &[self.ctx]);
            let at = self.live.iter().rposition(|l| *l == Live::Param).expect("a pushed parameter");
            self.live.remove(at);
        }
        match each {
            Each::Map => {
                let owned = self.b.ins().iconst(types::I8, 1);
                self.call(Helper::ListPush, &[building.expect("a list"), mapped.expect("a value"), owned]);
                self.b.ins().jump(next, &[]);
            }
            Each::Filter => {
                let keep = self.b.create_block();
                self.b.ins().brif(hit.expect("a test"), keep, &[], next, &[]);
                self.b.switch_to_block(keep);
                let borrowed = self.b.ins().iconst(types::I8, 0);
                self.call(Helper::ListPush, &[building.expect("a list"), item, borrowed]);
                self.b.ins().jump(next, &[]);
            }
            Each::Any => {
                self.b.ins().brif(hit.expect("a test"), exit, &[BlockArg::Value(yes)], next, &[]);
            }
            Each::All => {
                self.b.ins().brif(hit.expect("a test"), next, &[], exit, &[BlockArg::Value(no)]);
            }
            Each::Find | Each::FindLast => {
                let found = self.b.create_block();
                self.b.ins().brif(hit.expect("a test"), found, &[], next, &[]);
                self.b.switch_to_block(found);
                let out = self.addr(result.expect("a slot"));
                self.call(Helper::CloneValue, &[item, out]);
                self.b.ins().jump(exit, &[BlockArg::Value(yes)]);
            }
        }

        self.b.switch_to_block(next);
        let following = if backwards { self.b.ins().iadd_imm_u(i, -1i64) } else { self.b.ins().iadd_imm_u(i, 1) };
        self.b.ins().jump(header, &[BlockArg::Value(following)]);

        self.b.switch_to_block(end);
        match each {
            Each::Any => {
                self.b.ins().jump(exit, &[BlockArg::Value(no)]);
            }
            Each::All => {
                self.b.ins().jump(exit, &[BlockArg::Value(yes)]);
            }
            Each::Find | Each::FindLast => {
                let out = self.addr(result.expect("a slot"));
                self.call(Helper::MakeNull, &[out]);
                self.b.ins().jump(exit, &[BlockArg::Value(no)]);
            }
            Each::Map | Each::Filter => {
                let built = building.expect("a list");
                let at = self.live.iter().rposition(|l| *l == Live::List(built)).expect("a list being built");
                self.live.remove(at);
                let out = self.addr(result.expect("a slot"));
                self.call(Helper::ListFinish, &[built, out]);
                self.b.ins().jump(exit, &[BlockArg::Value(yes)]);
            }
        }

        self.b.switch_to_block(exit);
        let flag = self.b.block_params(exit)[0];
        self.done(list_owned);
        match result {
            Some(slot) => {
                self.own(slot);
                Lowered::Owned(slot)
            }
            None => Lowered::Bool(flag),
        }
    }
}

/// Whether `e` reads a lambda parameter `bound` selects.
fn reads_param(e: &CExpr, bound: &dyn Fn(u16) -> bool) -> bool {
    if let CExpr::Param(d) = e {
        return bound(*d);
    }
    let mut found = false;
    e.for_each_child(&mut |child| found = found || reads_param(child, bound));
    found
}
