//! Facts native front ends answer about a node (`uses`, `type`, ...): one
//! registry entry each, read like built-in variables. On layers without a
//! native front end they are null.
//!
//! Each front end declares its own ([`Frontend::facts`], [`Frontend::links`],
//! [`Frontend::projections`]); the registry numbers them all, and a read asks
//! the file's [`Native`] by the front end's own number.

use std::sync::LazyLock;

use crate::engine::frontend::{Frontend, Native};

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

/// A link: a relation from a node to other nodes of the same tree
/// (`declaration`, `references`). Rules read it as a node (or a list of
/// them, with `many`), then read anything on it: `declaration.line`.
pub struct LinkInfo {
    pub name: &'static str,
    pub doc: &'static str,
    pub many: bool,
}

/// Optional projections of a record-or-null fact. Reading one field (or
/// presence) need not build the whole record; it must agree with it.
pub struct ProjectionInfo {
    /// The fact it projects.
    pub name: &'static str,
    pub fields: &'static [&'static str],
}

/// A registered fact: its declaration, and who answers it.
pub struct Fact {
    pub name: &'static str,
    pub doc: &'static str,
    pub shape: Shape,
    front: &'static str,
    local: usize,
}

pub struct Link {
    pub name: &'static str,
    pub doc: &'static str,
    pub many: bool,
    front: &'static str,
    local: usize,
}

/// One projected read: a field of a fact (`key`), or its presence (`None`).
pub struct FieldRead {
    pub fact: usize,
    pub key: Option<&'static str>,
    field: Option<usize>,
    front: &'static str,
    local: usize,
}

/// Every front end's declarations, numbered in [`crate::FRONTENDS`] order.
fn declared<T, R>(list: impl Fn(&'static dyn Frontend) -> Vec<T>, entry: impl Fn(T, &'static str, usize) -> R) -> Vec<R> {
    crate::engine::frontend::all()
        .iter()
        .flat_map(|&front| list(front).into_iter().enumerate().map(move |(local, item)| (item, front.name(), local)))
        .map(|(item, front, local)| entry(item, front, local))
        .collect()
}

/// Every fact of every front end.
pub static FACTS: LazyLock<Vec<Fact>> =
    LazyLock::new(|| declared(|front| front.facts(), |f, front, local| Fact { name: f.name, doc: f.doc, shape: f.shape, front, local }));

/// Every link of every front end.
pub static LINKS: LazyLock<Vec<Link>> =
    LazyLock::new(|| declared(|front| front.links(), |l, front, local| Link { name: l.name, doc: l.doc, many: l.many, front, local }));

/// Compile-time field identities; values live only in the expression memo.
pub static FIELD_READS: LazyLock<Vec<FieldRead>> = LazyLock::new(|| {
    declared(|front| front.projections(), |p, front, local| (p, front, local))
        .into_iter()
        .flat_map(|(projection, front, local)| {
            let fact = FACTS.iter().position(|f| f.front == front && f.name == projection.name).expect("projection belongs to a registered fact");
            std::iter::once(FieldRead { fact, key: None, field: None, front, local }).chain(
                projection.fields.iter().enumerate().map(move |(field, &key)| FieldRead { fact, key: Some(key), field: Some(field), front, local }),
            )
        })
        .collect()
});

/// A fact's index in [`FACTS`].
pub fn fact(name: &str) -> Option<usize> {
    FACTS.iter().position(|f| f.name == name)
}

/// A link's index in [`LINKS`].
pub fn link(name: &str) -> Option<usize> {
    LINKS.iter().position(|l| l.name == name)
}

pub fn field_read(fact: usize, key: Option<&str>) -> Option<usize> {
    FIELD_READS.iter().position(|read| read.fact == fact && read.key == key)
}

/// Fact `fact` of `node`; null when another front end answers it.
pub fn read(native: &dyn Native, fact: usize, node: u32) -> Value<'static> {
    let fact = &FACTS[fact];
    if fact.front == native.frontend() { native.fact(fact.local, node) } else { Value::Null }
}

/// A projected read of `node` (see [`FIELD_READS`]).
pub fn read_field(native: &dyn Native, read: usize, node: u32) -> Value<'static> {
    let read = &FIELD_READS[read];
    if read.front == native.frontend() { native.projection(read.local, node, read.field) } else { Value::Null }
}

/// The nodes link `link` leads to from `node`.
pub fn follow(native: &dyn Native, link: usize, node: u32) -> Vec<u32> {
    let link = &LINKS[link];
    if link.front == native.frontend() { native.link(link.local, node) } else { Vec::new() }
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
