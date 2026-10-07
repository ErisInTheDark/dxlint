//! What an expression always evaluates to, when that is known at config
//! load, so compiled code can keep it unboxed.

use crate::config::ast::BinOp;
use crate::expr::{Builtin, CExpr, Each, Value};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Shape {
    Bool,
    Num,
    /// Text borrowed from the file, its path or a constant: its bytes stay
    /// valid for the whole evaluation.
    Text,
    Null,
    /// Anything else: a [`Value`].
    Boxed,
}

/// Builtins whose values are always borrowed text or numbers.
pub(super) fn builtin(b: Builtin) -> Shape {
    match b {
        Builtin::Text | Builtin::Before | Builtin::After | Builtin::File | Builtin::TextBefore | Builtin::TextAfter | Builtin::Source => Shape::Text,
        Builtin::Line | Builtin::Column | Builtin::Offset | Builtin::EndOffset => Shape::Num,
        _ => Shape::Boxed,
    }
}

pub(super) fn of(e: &CExpr) -> Shape {
    match e {
        CExpr::Const(Value::Bool(_)) => Shape::Bool,
        CExpr::Const(Value::Num(_)) => Shape::Num,
        CExpr::Const(Value::Str(_)) => Shape::Text,
        CExpr::Const(Value::Null) => Shape::Null,
        CExpr::Builtin(b) => builtin(*b),
        CExpr::Not(_) | CExpr::Is(..) | CExpr::Test(..) | CExpr::TestDfa(..) | CExpr::Each(Each::Any | Each::All, ..) => Shape::Bool,
        CExpr::Binary(op, ..) if is_boolean(*op) => Shape::Bool,
        CExpr::Neg(inner) if of(inner) == Shape::Num => Shape::Num,
        CExpr::If(_, yes, no) => match (of(yes), of(no)) {
            (a, b) if a == b && matches!(a, Shape::Bool | Shape::Num | Shape::Text) => a,
            _ => Shape::Boxed,
        },
        _ => Shape::Boxed,
    }
}

/// Operators whose result is always a bool.
pub(super) fn is_boolean(op: BinOp) -> bool {
    matches!(op, BinOp::And | BinOp::Or | BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
}
