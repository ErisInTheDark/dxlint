//! Facts native front ends answer about a node (`uses`, `type`, ...): one
//! registry entry each, read like built-in variables. On layers without a
//! native front end they are null.
//!
//! Each front end declares its own ([`Frontend::facts`]); the registry
//! numbers them all, and a read asks the file's [`Native`] by the front
//! end's own number.

use std::sync::LazyLock;

use crate::engine::frontend::Native;

use super::Value;

/// What a front end declares about one of its facts.
pub struct FactInfo {
    pub name: &'static str,
    pub doc: &'static str,
    pub shape: Shape,
}

#[derive(Clone, Copy)]
pub enum Shape {
    Scalar,
    Record(&'static [(&'static str, Shape)]),
    List(&'static Shape),
}

/// A registered fact: its declaration, and who answers it.
pub struct Fact {
    pub name: &'static str,
    pub doc: &'static str,
    pub shape: Shape,
    front: &'static str,
    local: usize,
}

/// Every fact of every front end, numbered in [`crate::FRONTENDS`] order.
pub static FACTS: LazyLock<Vec<Fact>> = LazyLock::new(|| {
    crate::engine::frontend::all()
        .iter()
        .flat_map(|&front| front.facts().into_iter().enumerate()
            .map(move |(local, f)| Fact { name: f.name, doc: f.doc, shape: f.shape, front: front.name(), local }))
        .collect()
});

/// A fact's index in [`FACTS`].
pub fn fact(name: &str) -> Option<usize> {
    FACTS.iter().position(|f| f.name == name)
}

/// Fact `fact` of `node`; null when another front end answers it.
pub fn read(native: &dyn Native, fact: usize, node: u32) -> Value<'static> {
    let fact = &FACTS[fact];
    if fact.front == native.frontend() { native.fact(fact.local, node) } else { Value::Null }
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
