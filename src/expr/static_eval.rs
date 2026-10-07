//! Value-only evaluation at compile time; unknown inputs stay runtime expressions.

use super::{eval_args, eval_binary, Builtin, CExpr, Value, MAX_CALL_DEPTH};
use crate::grammar::UserFn;
use std::cell::Cell;

const NEEDS_NODE: &str = "needs a node to evaluate";
const MAX_FN_VISITS: usize = 4096;

pub(super) struct StaticEval<'a> {
    options: &'a [Value<'static>],
    functions: &'a [UserFn],
    remaining: Cell<usize>,
}

impl<'a> StaticEval<'a> {
    pub(super) fn new(options: &'a [Value<'static>], functions: &'a [UserFn]) -> Self {
        Self { options, functions, remaining: Cell::new(MAX_FN_VISITS) }
    }

    pub(super) fn eval(&self, e: &CExpr) -> Result<Value<'static>, String> {
        Frame { owner: self, params: &[], calls: &[] }.eval(e)
    }
}

struct Frame<'a, 'b> {
    owner: &'b StaticEval<'a>,
    params: &'b [Option<Value<'static>>],
    calls: &'b [u32],
}

impl Frame<'_, '_> {
    fn eval(&self, e: &CExpr) -> Result<Value<'static>, String> {
        // Depth alone cannot bound acyclic functions that repeatedly call
        // shared bodies. All nested frames spend the same attempt's budget.
        if !self.calls.is_empty() {
            let remaining = self.owner.remaining.get();
            if remaining == 0 { return Err(NEEDS_NODE.into()); }
            self.owner.remaining.set(remaining - 1);
        }
        let eval = |e: &CExpr| self.eval(e);
        Ok(match e {
            CExpr::Const(v) => v.clone(),
            CExpr::Opt(i) => self.owner.options[*i].clone(),
            CExpr::Param(i) => self.params.get(*i as usize).and_then(Clone::clone)
                .ok_or_else(|| NEEDS_NODE.to_owned())?,
            CExpr::Hole(inner, _) => eval(inner)?,
            CExpr::Template(parts) => {
                let mut text = String::new();
                for part in parts { text.push_str(&eval(part)?.text()); }
                Value::from(text)
            }
            CExpr::List(items) => Value::List(items.iter().map(eval).collect::<Result<_, _>>()?),
            CExpr::Record(entries) => Value::Record(entries.iter()
                .map(|(key, value)| Ok((key.clone(), eval(value)?))).collect::<Result<_, String>>()?),
            CExpr::Key(base, key) => eval(base)?.key(key),
            CExpr::Member(base, key, _) => eval(base)?.key(key),
            CExpr::Path(base, keys, _) => keys.iter().fold(eval(base)?, |value, key| value.key(key)),
            CExpr::Call(function, args) => eval_args(args, eval, |args| (function.call)(args))?,
            CExpr::Not(inner) => Value::Bool(!eval(inner)?.truthy()),
            CExpr::Neg(inner) => Value::Num(-eval(inner)?.num()?),
            CExpr::If(condition, yes, no) => {
                eval(if eval(condition)?.truthy() { yes } else { no })?
            }
            CExpr::Binary(op, left, right) => eval_binary(*op, left, right, eval)?,
            CExpr::UserCall(id, args) => {
                // Do not unfold recursion or invent a compile-time failure for it.
                if self.calls.contains(id) || self.calls.len() >= MAX_CALL_DEPTH as usize {
                    return Err(NEEDS_NODE.into());
                }
                let Some(body) = self.owner.functions.get(*id as usize).and_then(|f| f.body.get()) else {
                    return Err(NEEDS_NODE.into());
                };
                let params = args.iter().map(|arg| match arg {
                    // These unknown values are safe to discard if the body never reads
                    // them. Other arguments must evaluate: calls are eager even when
                    // a parameter is unused (errors, debug output and recorded reads).
                    CExpr::Builtin(Builtin::Node) => Ok(None),
                    CExpr::Param(i) => self.params.get(*i as usize).cloned()
                        .ok_or_else(|| NEEDS_NODE.to_owned()),
                    _ => eval(arg).map(Some),
                }).collect::<Result<Vec<_>, String>>()?;
                let mut calls = self.calls.to_vec();
                calls.push(*id);
                Frame { params: &params, calls: &calls, owner: self.owner }.eval(body)?
            }
            _ => return Err(NEEDS_NODE.into()),
        })
    }
}
