//! Facts a front end answers about a node, read like built-in variables.
//! No front end declares any since every language moved to `.dx`; the
//! registry and its readers go next.

use std::sync::LazyLock;

use crate::grammar::frontend::Native;

use super::Value;

#[derive(Clone, Copy)]
pub enum Shape {
    Scalar,
    Record(&'static [(&'static str, Shape)]),
    List(&'static Shape),
}

pub struct Fact {
    pub name: &'static str,
    pub doc: &'static str,
    pub shape: Shape,
}

pub static FACTS: LazyLock<Vec<Fact>> = LazyLock::new(Vec::new);

/// A fact's index in [`FACTS`].
pub fn fact(name: &str) -> Option<usize> {
    FACTS.iter().position(|f| f.name == name)
}

pub fn read(_native: &dyn Native, _fact: usize, _node: u32) -> Value<'static> {
    Value::Null
}

thread_local! {
    static ARGS: std::cell::RefCell<Vec<Value<'static>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Runs `get` with `args` as its arguments: a fact written as a call
/// (`member_order(groups, order)`) reads them with [`args`]. For analyses
/// whose whole-file state depends on a rule's options.
pub fn with_args(args: Vec<Value<'static>>, get: impl FnOnce() -> Value<'static>) -> Value<'static> {
    let saved = ARGS.with(|a| std::mem::replace(&mut *a.borrow_mut(), args));
    let out = get();
    ARGS.with(|a| *a.borrow_mut() = saved);
    out
}

/// The arguments of the fact call being evaluated (empty when the fact is
/// read as a name).
pub fn args() -> Vec<Value<'static>> {
    ARGS.with(|a| a.borrow().clone())
}

/// `v`'s value, or null.
pub fn opt<T>(v: Option<T>, f: impl FnOnce(T) -> Value<'static>) -> Value<'static> {
    v.map_or(Value::Null, f)
}

/// A list of words.
pub fn words(list: Vec<&'static str>) -> Value<'static> {
    Value::List(list.into_iter().map(Value::from).collect())
}
