//! The Rust side of compiled code: every operation it does not do natively.
//! Each helper is one small, typed step of the interpreter's own semantics
//! (the same [`Ctx`] and [`Value`] methods), so the two engines cannot drift.
//!
//! Values cross as pointers to 32-byte [`Value`] slots in the compiled
//! frame. A helper writing to `out` initialises it (the caller then owns
//! it); one taking `owned` values consumes them.

use std::mem::size_of;
use std::ptr;

use crate::expr::functions::Function;
use crate::expr::{Builtin, CExpr, Ctx, Value};

use super::{FAILED, TRUE};

type V = Value<'static>;
type C = Ctx<'static>;
type E = Option<String>;

pub(super) const VALUE_SIZE: usize = size_of::<V>();

/// A helper's parameter or return type.
#[derive(Clone, Copy)]
pub(super) enum Ty {
    Ptr,
    I8,
    F64,
}

pub(super) struct Spec {
    pub name: &'static str,
    pub address: *const u8,
    pub params: &'static [Ty],
    pub ret: Option<Ty>,
}

macro_rules! helpers {
    ($($variant:ident = $name:ident($($param:ident),*) $(-> $ret:ident)?;)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub(super) enum Helper {
            $($variant,)*
        }

        impl Helper {
            pub(super) const ALL: &'static [Helper] = &[$(Helper::$variant,)*];

            pub(super) fn spec(self) -> Spec {
                match self {
                    $(Helper::$variant => Spec {
                        name: concat!("zl_", stringify!($name)),
                        address: $name as *const u8,
                        params: &[$(Ty::$param),*],
                        ret: helpers!(@ret $($ret)?),
                    },)*
                }
            }
        }
    };
    (@ret) => { None };
    (@ret $ret:ident) => { Some(Ty::$ret) };
}

helpers! {
    Eval = eval(Ptr, Ptr, Ptr, Ptr) -> I8;
    Truth = truth(Ptr, Ptr, Ptr) -> I8;
    DropValue = drop_value(Ptr);
    CloneValue = clone_value(Ptr, Ptr);
    Truthy = truthy(Ptr) -> I8;
    Equal = equal(Ptr, Ptr) -> I8;
    Compare = compare(Ptr, Ptr) -> I8;
    MakeBool = make_bool(Ptr, I8);
    MakeNum = make_num(Ptr, F64);
    MakeText = make_text(Ptr, Ptr, Ptr);
    MakeNull = make_null(Ptr);
    Local = local(Ptr, Ptr, Ptr, Ptr) -> I8;
    Param = param(Ptr, Ptr, Ptr);
    Builtin = builtin(Ptr, Ptr, Ptr, Ptr) -> I8;
    BuiltinText = builtin_text(Ptr, Ptr, Ptr, Ptr) -> I8;
    BuiltinNum = builtin_num(Ptr, Ptr, Ptr, Ptr) -> I8;
    Key = key(Ptr, Ptr, Ptr, Ptr);
    Member = member(Ptr, Ptr, Ptr, Ptr, Ptr) -> I8;
    Is = is(Ptr, Ptr, Ptr) -> I8;
    Test = test(Ptr, Ptr, Ptr) -> I8;
    Call = call(Ptr, Ptr, Ptr, Ptr, Ptr) -> I8;
    Binary = binary(Ptr, Ptr, Ptr, Ptr, Ptr) -> I8;
    Items = items(Ptr, Ptr);
    IsNode = is_node(Ptr) -> I8;
    PushParam = push_param(Ptr, Ptr);
    PopParam = pop_param(Ptr);
    ListNew = list_new(Ptr) -> Ptr;
    ListPush = list_push(Ptr, Ptr, I8);
    ListFinish = list_finish(Ptr, Ptr);
    ListFree = list_free(Ptr);
    BytesEqual = bytes_equal(Ptr, Ptr, Ptr, Ptr) -> I8;
}

// SAFETY (all helpers): compiled code passes the context of the call it
// runs under, expressions its rule owns, and pointers to its own live,
// initialised value slots (or uninitialised ones as `out`).

fn fail(error: *mut E, message: String) -> u8 {
    unsafe { *error = Some(message) };
    FAILED
}

fn put(out: *mut V, result: Result<V, String>, error: *mut E) -> u8 {
    match result {
        Ok(v) => {
            unsafe { ptr::write(out, v) };
            TRUE
        }
        Err(message) => fail(error, message),
    }
}

unsafe extern "C" fn eval(ctx: *const C, e: *const CExpr, out: *mut V, error: *mut E) -> u8 {
    let (ctx, e) = unsafe { (&*ctx, &*e) };
    put(out, ctx.eval(e), error)
}

unsafe extern "C" fn truth(ctx: *const C, e: *const CExpr, error: *mut E) -> u8 {
    let (ctx, e) = unsafe { (&*ctx, &*e) };
    match ctx.eval(e) {
        Ok(v) => u8::from(v.truthy()),
        Err(message) => fail(error, message),
    }
}

unsafe extern "C" fn drop_value(v: *mut V) {
    unsafe { ptr::drop_in_place(v) };
}

unsafe extern "C" fn clone_value(v: *const V, out: *mut V) {
    unsafe { ptr::write(out, (*v).clone()) };
}

unsafe extern "C" fn truthy(v: *const V) -> u8 {
    u8::from(unsafe { (*v).truthy() })
}

unsafe extern "C" fn equal(a: *const V, b: *const V) -> u8 {
    u8::from(unsafe { (*a).loose_eq(&*b) })
}

/// `a`'s order against `b`: 0 less, 1 equal, 2 greater.
unsafe extern "C" fn compare(a: *const V, b: *const V) -> u8 {
    match unsafe { (*a).compare(&*b) } {
        std::cmp::Ordering::Less => 0,
        std::cmp::Ordering::Equal => 1,
        std::cmp::Ordering::Greater => 2,
    }
}

unsafe extern "C" fn make_bool(out: *mut V, b: u8) {
    unsafe { ptr::write(out, Value::Bool(b != 0)) };
}

unsafe extern "C" fn make_num(out: *mut V, n: f64) {
    unsafe { ptr::write(out, Value::Num(n)) };
}

/// Text borrowed from the file or a constant (see [`super::shape::Shape::Text`]).
unsafe extern "C" fn make_text(out: *mut V, bytes: *const u8, len: usize) {
    let text = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(bytes, len)) };
    unsafe { ptr::write(out, Value::Str(crate::expr::value::Text::Borrowed(text))) };
}

unsafe extern "C" fn make_null(out: *mut V) {
    unsafe { ptr::write(out, Value::Null) };
}

unsafe extern "C" fn local(ctx: *const C, slot: usize, out: *mut V, error: *mut E) -> u8 {
    let ctx = unsafe { &*ctx };
    put(out, ctx.local(slot as u16), error)
}

unsafe extern "C" fn param(ctx: *const C, depth: usize, out: *mut V) {
    let ctx = unsafe { &*ctx };
    unsafe { ptr::write(out, ctx.param(depth as u16)) };
}

unsafe extern "C" fn builtin(ctx: *const C, b: usize, out: *mut V, error: *mut E) -> u8 {
    let ctx = unsafe { &*ctx };
    put(out, ctx.builtin(Builtin::ALL[b]), error)
}

/// A text-shaped builtin's bytes, as (pointer, length) into `out`.
unsafe extern "C" fn builtin_text(ctx: *const C, b: usize, out: *mut [usize; 2], error: *mut E) -> u8 {
    let ctx = unsafe { &*ctx };
    match ctx.builtin(Builtin::ALL[b]) {
        // Borrowed text lives as long as the file; shared text would be
        // freed here, so text-shaped builtins must never produce it.
        Ok(Value::Str(crate::expr::value::Text::Borrowed(text))) => {
            unsafe { *out = [text.as_ptr() as usize, text.len()] };
            TRUE
        }
        Ok(other) => fail(error, format!("compiled code expected borrowed text, got {}", other.type_name())),
        Err(message) => fail(error, message),
    }
}

unsafe extern "C" fn builtin_num(ctx: *const C, b: usize, out: *mut f64, error: *mut E) -> u8 {
    let ctx = unsafe { &*ctx };
    match ctx.builtin(Builtin::ALL[b]) {
        Ok(Value::Num(n)) => {
            unsafe { *out = n };
            TRUE
        }
        Ok(other) => fail(error, format!("compiled code expected a number, got {}", other.type_name())),
        Err(message) => fail(error, message),
    }
}

unsafe extern "C" fn key(base: *const V, key: *const u8, len: usize, out: *mut V) {
    let key = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(key, len)) };
    unsafe { ptr::write(out, (*base).key(key)) };
}

/// `base.key` for a [`CExpr::Member`] `e`, `base` already evaluated.
unsafe extern "C" fn member(ctx: *const C, base: *const V, e: *const CExpr, out: *mut V, error: *mut E) -> u8 {
    let (ctx, e) = unsafe { (&*ctx, &*e) };
    let CExpr::Member(_, key, part) = e else { unreachable!("a member read") };
    put(out, ctx.member(unsafe { &*base }, key, part.as_deref()), error)
}

/// `v is ...` for a [`CExpr::Is`] `e`, `v` already evaluated.
unsafe extern "C" fn is(ctx: *const C, v: *const V, e: *const CExpr) -> u8 {
    let (ctx, e) = unsafe { (&*ctx, &*e) };
    let CExpr::Is(_, tests) = e else { unreachable!("an is test") };
    u8::from(ctx.is_value(unsafe { &*v }, tests))
}

/// A pattern test (`e`: [`CExpr::Test`] or [`CExpr::TestDfa`]) on `v`.
unsafe extern "C" fn test(ctx: *const C, e: *const CExpr, v: *const V) -> u8 {
    let (ctx, e) = unsafe { (&*ctx, &*e) };
    u8::from(ctx.test_value(e, unsafe { &*v }))
}

/// Calls `f` with `n` owned arguments, consuming them.
unsafe extern "C" fn call(f: *const Function, args: *mut V, n: usize, out: *mut V, error: *mut E) -> u8 {
    let f = unsafe { &*f };
    let args = unsafe { std::slice::from_raw_parts_mut(args, n) };
    let result = (f.call)(args).map_err(|e| format!("{}: {e}", f.name));
    unsafe { ptr::drop_in_place(args) };
    put(out, result, error)
}

/// A [`CExpr::Binary`] `e`'s operator on borrowed `a` and `b`.
unsafe extern "C" fn binary(e: *const CExpr, a: *const V, b: *const V, out: *mut V, error: *mut E) -> u8 {
    let CExpr::Binary(op, ..) = (unsafe { &*e }) else { unreachable!("a binary operator") };
    let (a, b) = unsafe { ((*a).clone(), (*b).clone()) };
    put(out, crate::expr::binary(*op, a, b), error)
}

/// A value's items (see [`Value::item_slice`]) as (pointer, length).
unsafe extern "C" fn items(v: *const V, out: *mut [usize; 2]) {
    let items = unsafe { (*v).item_slice() };
    unsafe { *out = [items.as_ptr() as usize, items.len()] };
}

unsafe extern "C" fn is_node(v: *const V) -> u8 {
    u8::from(matches!(unsafe { &*v }, Value::Node(..)))
}

unsafe extern "C" fn push_param(ctx: *const C, v: *const V) {
    let ctx = unsafe { &*ctx };
    ctx.push_param(unsafe { (*v).clone() });
}

unsafe extern "C" fn pop_param(ctx: *const C) {
    unsafe { &*ctx }.pop_param();
}

unsafe extern "C" fn list_new(capacity: usize) -> *mut Vec<V> {
    Box::into_raw(Box::new(Vec::with_capacity(capacity)))
}

/// Appends `v`, moved when `owned`, else cloned.
unsafe extern "C" fn list_push(list: *mut Vec<V>, v: *mut V, owned: u8) {
    let v = if owned != 0 { unsafe { ptr::read(v) } } else { unsafe { (*v).clone() } };
    unsafe { (*list).push(v) };
}

unsafe extern "C" fn list_finish(list: *mut Vec<V>, out: *mut V) {
    let list = unsafe { Box::from_raw(list) };
    unsafe { ptr::write(out, Value::list(*list)) };
}

unsafe extern "C" fn list_free(list: *mut Vec<V>) {
    drop(unsafe { Box::from_raw(list) });
}

unsafe extern "C" fn bytes_equal(a: *const u8, a_len: usize, b: *const u8, b_len: usize) -> u8 {
    let (a, b) = unsafe { (std::slice::from_raw_parts(a, a_len), std::slice::from_raw_parts(b, b_len)) };
    u8::from(a == b)
}
