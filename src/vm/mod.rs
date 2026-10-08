//! Typed VM, milestone 1: attribute definitions whose bodies use a small,
//! infallible subset of the rule language are lowered at config load into
//! typed expressions over null / bool / number, then evaluated lazily per
//! node into a per-file record buffer: each node kind has its own record
//! layout of its lowered attributes, one buffer per file. Anything outside
//! the subset (or depending on something outside it) stays interpreted.
//!
//! `ZLINT_VM=0` disables it; `ZLINT_VM=verify` evaluates both ways and
//! prints mismatches to stderr.

use crate::config::ast::BinOp;
use crate::expr::{Builtin, CExpr, IsTest, Relative, Value};
use crate::grammar::{Grammar, Tree, UserFn, ROOT, TEXT};
use std::cell::{OnceCell, RefCell};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    On,
    Off,
    Verify,
}

#[cfg(test)]
thread_local! {
    /// Tests compare modes within one process.
    static FORCED: std::cell::Cell<Option<Mode>> = const { std::cell::Cell::new(None) };
}

pub fn mode() -> Mode {
    #[cfg(test)]
    if let Some(mode) = FORCED.get() {
        return mode;
    }
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("ZLINT_VM").as_deref() {
        Ok("0") => Mode::Off,
        Ok("verify") => Mode::Verify,
        _ => Mode::On,
    })
}

/// A VM value: what the subset can produce.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum V {
    Null,
    Bool(bool),
    Num(f64),
}

impl V {
    pub fn truthy(self) -> bool {
        match self {
            V::Null => false,
            V::Bool(b) => b,
            V::Num(n) => n != 0.0,
        }
    }

    fn num(self) -> f64 {
        match self {
            V::Null => 0.0,
            V::Bool(b) => f64::from(u8::from(b)),
            V::Num(n) => n,
        }
    }

    /// [`Value::loose_eq`] restricted to these kinds.
    fn loose_eq(self, other: V) -> bool {
        match (self, other) {
            (V::Null, V::Null) => true,
            (V::Bool(a), V::Bool(b)) => a == b,
            (V::Num(a), V::Num(b)) => a == b,
            _ => false,
        }
    }

    pub fn value(self) -> Value<'static> {
        match self {
            V::Null => Value::Null,
            V::Bool(b) => Value::Bool(b),
            V::Num(n) => Value::Num(n),
        }
    }

    pub fn same(self, value: &Value) -> bool {
        match (self, value) {
            (V::Null, Value::Null) => true,
            (V::Bool(a), Value::Bool(b)) => a == *b,
            (V::Num(a), Value::Num(b)) => a == *b || a.is_nan() && b.is_nan(),
            _ => false,
        }
    }
}

/// A lowered expression, evaluated on one node.
#[derive(Clone, Debug)]
pub enum VExpr {
    Const(V),
    /// The node, or its parent layer node, is one of these matchers.
    Is { parent: bool, kinds: Box<[u32]> },
    /// The node fills this field of one of these matchers.
    InField(Box<[u32]>, u16),
    FieldPresent(u16),
    FieldCount(u16),
    /// The field holds exactly one node whose text is one of these.
    FieldTextIn(u16, Box<[Box<str>]>),
    Attr(u16),
    ParentAttr(u16),
    /// Sum of an attribute over all direct children (null counts as 0).
    SumChildren(u16),
    Default(Box<VExpr>, Box<VExpr>),
    Not(Box<VExpr>),
    Neg(Box<VExpr>),
    If(Box<VExpr>, Box<VExpr>, Box<VExpr>),
    Binary(BinOp, Box<VExpr>, Box<VExpr>),
}

#[derive(Clone)]
enum Lowered {
    V(VExpr),
    /// `children.attr`: only usable inside `sum`.
    Children(u16),
}

/// How deep `fn` calls are inlined while lowering.
const INLINE_DEPTH: u32 = 8;

/// What lowering can inline: the config's `fn`s, and for rule conditions
/// the rule's `let`s.
struct Scope<'a> {
    functions: &'a [UserFn],
    locals: &'a [CExpr],
}

/// The typed form of an attribute definition, if its body is in the subset.
pub fn lower(e: &CExpr, functions: &[UserFn]) -> Option<VExpr> {
    scalar(e, &Scope { functions, locals: &[] }, &[], 0)
}

/// The typed form of a rule condition, reading the rule's `let`s inline
/// (they are pure here, so inlining keeps their values).
pub fn lower_condition(e: &CExpr, functions: &[UserFn], locals: &[CExpr]) -> Option<VExpr> {
    scalar(e, &Scope { functions, locals }, &[], 0)
}

fn scalar(e: &CExpr, g: &Scope, args: &[Lowered], depth: u32) -> Option<VExpr> {
    match lower_in(e, g, args, depth)? {
        Lowered::V(v) => Some(v),
        Lowered::Children(_) => None,
    }
}

fn constant(v: &Value) -> Option<V> {
    Some(match v {
        Value::Null => V::Null,
        Value::Bool(b) => V::Bool(*b),
        Value::Num(n) => V::Num(*n),
        _ => return None,
    })
}

fn lower_in(e: &CExpr, g: &Scope, args: &[Lowered], depth: u32) -> Option<Lowered> {
    let boxed = |e: &CExpr| scalar(e, g, args, depth).map(Box::new);
    Some(Lowered::V(match e {
        CExpr::Const(v) => VExpr::Const(constant(v)?),
        CExpr::Hole(inner, _) => return lower_in(inner, g, args, depth),
        CExpr::Param(d) => return args.get(*d as usize).cloned(),
        CExpr::Attr(a) => VExpr::Attr(*a),
        CExpr::ParentAttr(a) => VExpr::ParentAttr(*a),
        CExpr::Relative(Relative::Children, inner) => match &**inner {
            CExpr::Attr(a) => return Some(Lowered::Children(*a)),
            _ => return None,
        },
        CExpr::Is(subject, tests) => {
            let parent = match &**subject {
                CExpr::Builtin(Builtin::Node) => false,
                CExpr::Builtin(Builtin::Parent) => true,
                _ => return None,
            };
            let mut parts = Vec::new();
            for test in tests {
                parts.push(match test {
                    IsTest::Kind(ids) => VExpr::Is { parent, kinds: ids.clone().into() },
                    IsTest::Field(ids, label) if !parent => VExpr::InField(ids.clone().into(), *label),
                    _ => return None,
                });
            }
            parts.into_iter().reduce(|a, b| VExpr::Binary(BinOp::Or, Box::new(a), Box::new(b)))?
        }
        CExpr::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b) if matches!((&**a, &**b), (CExpr::Field(_), CExpr::Const(Value::Null)) | (CExpr::Const(Value::Null), CExpr::Field(_))) => {
            let ((CExpr::Field(label), _) | (_, CExpr::Field(label))) = (&**a, &**b) else { return None };
            let present = VExpr::FieldPresent(*label);
            if *op == BinOp::Ne { present } else { VExpr::Not(Box::new(present)) }
        }
        CExpr::Binary(op, a, b) => match op {
            BinOp::And | BinOp::Or | BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
            | BinOp::Add | BinOp::Sub | BinOp::Mul => VExpr::Binary(*op, boxed(a)?, boxed(b)?),
            _ => return None,
        },
        CExpr::Not(inner) => VExpr::Not(boxed(inner)?),
        CExpr::Neg(inner) => VExpr::Neg(boxed(inner)?),
        CExpr::If(c, a, b) => VExpr::If(boxed(c)?, boxed(a)?, boxed(b)?),
        CExpr::Call(f, call_args) => match (f.name, call_args.as_slice()) {
            ("count", [CExpr::Field(label)]) => VExpr::FieldCount(*label),
            ("default", [a, b]) => VExpr::Default(boxed(a)?, boxed(b)?),
            ("sum", [list]) => match lower_in(list, g, args, depth)? {
                Lowered::Children(a) => VExpr::SumChildren(a),
                Lowered::V(_) => return None,
            },
            ("contains", [CExpr::Const(Value::List(items)), CExpr::Field(label)]) => {
                let texts = items.iter().map(|item| match item {
                    Value::Str(s) => Some(Box::from(&**s)),
                    _ => None,
                }).collect::<Option<Box<[Box<str>]>>>()?;
                VExpr::FieldTextIn(*label, texts)
            }
            _ => return None,
        },
        // A rule's `let` on its own node, outside every lambda.
        CExpr::Local(i) if args.is_empty() && depth < INLINE_DEPTH => {
            return lower_in(g.locals.get(*i as usize)?, g, &[], depth + 1);
        }
        CExpr::UserCall(i, call_args) if depth < INLINE_DEPTH => {
            let body = g.functions.get(*i as usize)?.body.get()?;
            let lowered = call_args.iter().map(|a| lower_in(a, g, args, depth)).collect::<Option<Vec<_>>>()?;
            return lower_in(body, g, &lowered, depth + 1);
        }
        _ => return None,
    }))
}

/// What evaluation needs from its surroundings.
pub trait Host {
    fn tree(&self) -> &Tree;
    fn grammar(&self) -> &Grammar;
    fn source(&self) -> &str;
    /// An attribute on a node through the VM; `None` gives up on the VM.
    fn attr(&self, node: u32, attr: u16) -> Option<V>;
}

fn parent_layer(tree: &Tree, node: u32) -> Option<u32> {
    let mut p = tree.parent(node);
    while let Some(id) = p.filter(|&id| tree.is_text(id)) {
        p = tree.parent(id);
    }
    p
}

fn is_a(g: &Grammar, m: u32, kinds: &[u32]) -> bool {
    m != ROOT && m != TEXT && kinds.iter().any(|&k| g.is_a(m, k))
}

/// Evaluates `e` on `node`; `None` means the VM gives up (the caller falls
/// back to the interpreter).
pub fn eval(h: &impl Host, node: u32, e: &VExpr) -> Option<V> {
    let tree = h.tree();
    Some(match e {
        VExpr::Const(v) => *v,
        VExpr::Is { parent, kinds } => {
            let subject = if *parent { parent_layer(tree, node) } else { Some(node) };
            V::Bool(subject.is_some_and(|n| is_a(h.grammar(), tree.node(n).matcher, kinds)))
        }
        VExpr::InField(kinds, label) => V::Bool(
            tree.label(node) == Some(*label) && tree.owner(node).is_some_and(|o| is_a(h.grammar(), tree.node(o).matcher, kinds)),
        ),
        VExpr::FieldPresent(label) => V::Bool(!tree.fields(node, *label).is_empty()),
        VExpr::FieldCount(label) => V::Num(tree.fields(node, *label).len() as f64),
        VExpr::FieldTextIn(label, texts) => V::Bool(match tree.fields(node, *label).as_slice() {
            [one] => {
                let n = tree.node(*one);
                let text = &h.source()[n.start as usize..n.end as usize];
                texts.iter().any(|t| &**t == text)
            }
            _ => false,
        }),
        VExpr::Attr(a) => h.attr(node, *a)?,
        VExpr::ParentAttr(a) => match parent_layer(tree, node) {
            Some(p) => h.attr(p, *a)?,
            None => V::Null,
        },
        VExpr::SumChildren(a) => {
            let mut total = 0.0;
            for edge in &tree.node(node).children {
                total += h.attr(edge.node, *a)?.num();
            }
            V::Num(total)
        }
        VExpr::Default(a, b) => match eval(h, node, a)? {
            V::Null => eval(h, node, b)?,
            v => v,
        },
        VExpr::Not(inner) => V::Bool(!eval(h, node, inner)?.truthy()),
        VExpr::Neg(inner) => V::Num(-eval(h, node, inner)?.num()),
        VExpr::If(c, a, b) => if eval(h, node, c)?.truthy() { eval(h, node, a)? } else { eval(h, node, b)? },
        VExpr::Binary(op, a, b) => {
            let a = eval(h, node, a)?;
            match op {
                BinOp::And if !a.truthy() => return Some(V::Bool(false)),
                BinOp::Or if a.truthy() => return Some(V::Bool(true)),
                _ => {}
            }
            let b = eval(h, node, b)?;
            match op {
                BinOp::And | BinOp::Or => V::Bool(b.truthy()),
                BinOp::Eq => V::Bool(a.loose_eq(b)),
                BinOp::Ne => V::Bool(!a.loose_eq(b)),
                BinOp::Lt => V::Bool(a.num().partial_cmp(&b.num()).is_some_and(|o| o.is_lt())),
                BinOp::Le => V::Bool(a.num().partial_cmp(&b.num()).is_none_or(|o| o.is_le())),
                BinOp::Gt => V::Bool(a.num().partial_cmp(&b.num()).is_some_and(|o| o.is_gt())),
                BinOp::Ge => V::Bool(a.num().partial_cmp(&b.num()).is_none_or(|o| o.is_ge())),
                BinOp::Add => V::Num(a.num() + b.num()),
                BinOp::Sub => V::Num(a.num() - b.num()),
                BinOp::Mul => V::Num(a.num() * b.num()),
                _ => return None,
            }
        }
    })
}

/// A node's record cell, as stored.
enum Cell {
    Unset,
    Busy,
    /// The VM gave up here once; the interpreter owns this value.
    GaveUp,
    Ready(V),
}

const UNSET: u64 = 0;
const BUSY: u64 = 1;
const NULL: u64 = 2;
const FALSE: u64 = 3;
const TRUE: u64 = 4;
const GAVE_UP: u64 = 5;

/// Numbers are stored as their bits inverted, so a zeroed buffer reads as
/// unset; every NaN is stored as the canonical one, keeping the small codes
/// above free.
fn encode(v: V) -> u64 {
    match v {
        V::Null => NULL,
        V::Bool(false) => FALSE,
        V::Bool(true) => TRUE,
        V::Num(n) => !(if n.is_nan() { f64::NAN } else { n }).to_bits(),
    }
}

fn decode(cell: u64) -> Cell {
    match cell {
        UNSET => Cell::Unset,
        BUSY => Cell::Busy,
        NULL => Cell::Ready(V::Null),
        FALSE => Cell::Ready(V::Bool(false)),
        TRUE => Cell::Ready(V::Bool(true)),
        GAVE_UP => Cell::GaveUp,
        bits => Cell::Ready(V::Num(f64::from_bits(!bits))),
    }
}

/// One file's records: each node's lowered attributes, at the node's offset
/// in one shared buffer (record sizes per matcher: `layout`).
#[derive(Default)]
pub struct FileVm {
    offsets: OnceCell<Vec<u32>>,
    cells: RefCell<Vec<u64>>,
}

/// What [`FileVm::begin`] found.
pub enum Begin {
    Ready(V),
    /// Busy, or given up before: the interpreter handles it.
    GiveUp,
    /// Marked busy at this cell: evaluate, then [`FileVm::finish`].
    Compute(usize),
}

impl FileVm {
    fn offset(&self, tree: &Tree, layout: &[u16], node: u32) -> usize {
        let offsets = self.offsets.get_or_init(|| {
            let mut at = 0u32;
            let offsets = tree.nodes.iter().map(|n| {
                let start = at;
                if n.matcher != ROOT && n.matcher != TEXT {
                    at += u32::from(layout.get(n.matcher as usize).copied().unwrap_or(0));
                }
                start
            }).collect();
            *self.cells.borrow_mut() = vec![UNSET; at as usize];
            offsets
        });
        offsets[node as usize] as usize
    }

    pub fn begin(&self, tree: &Tree, layout: &[u16], node: u32, slot: u16) -> Begin {
        let at = self.offset(tree, layout, node) + slot as usize;
        let mut cells = self.cells.borrow_mut();
        match decode(cells[at]) {
            Cell::Ready(v) => Begin::Ready(v),
            Cell::Busy | Cell::GaveUp => Begin::GiveUp,
            Cell::Unset => {
                cells[at] = BUSY;
                Begin::Compute(at)
            }
        }
    }

    /// Publishes a value, or marks the cell given up so later reads go
    /// straight to the interpreter.
    pub fn finish(&self, at: usize, value: Option<V>) {
        self.cells.borrow_mut()[at] = value.map_or(GAVE_UP, encode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::functions;

    #[test]
    fn only_infallible_null_bool_number_bodies_lower() {
        let g: &[UserFn] = &[];
        let num = |n: f64| Box::new(CExpr::Const(Value::Num(n)));
        let call = |name: &str, args: Vec<CExpr>| CExpr::Call(functions::lookup(name).expect("registered"), args);
        for supported in [
            CExpr::Binary(BinOp::Add, num(1.0), num(2.0)),
            CExpr::If(Box::new(CExpr::Attr(0)), num(1.0), Box::new(CExpr::ParentAttr(1))),
            call("sum", vec![CExpr::Relative(Relative::Children, Box::new(CExpr::Attr(2)))]),
            call("default", vec![CExpr::Attr(0), CExpr::Const(Value::Num(0.0))]),
            CExpr::Binary(BinOp::Ne, Box::new(CExpr::Field(3)), Box::new(CExpr::Const(Value::Null))),
        ] {
            assert!(lower(&supported, g).is_some(), "{supported:?}");
        }
        for unsupported in [
            CExpr::Binary(BinOp::Rem, num(1.0), num(0.0)),
            CExpr::Const(Value::from("text")),
            CExpr::Relative(Relative::Children, Box::new(CExpr::Attr(2))),
            call("len", vec![CExpr::Builtin(Builtin::Text)]),
            CExpr::Binary(BinOp::Add, Box::new(CExpr::Field(3)), num(1.0)),
        ] {
            assert!(lower(&unsupported, g).is_none(), "{unsupported:?}");
        }
    }

    #[test]
    fn lowered_attributes_agree_with_the_interpreter() {
        use crate::engine::tests::native_linter;
        use std::path::Path;
        let config = r#"
            in ts_node let depth default(parent.depth, 0) + 1
            in ts_block let local count(body)
            in ts_node let boundary is (ts_function_like, ts_static_block) || is ts_class_property.init
            in ts_node let total default(local, 0) + sum(children.contribution)
            in ts_node let contribution if boundary then 0 else total
            in ts_assignment let logical contains(["&&=", "||=", "??="], operator)
            in ts_parameter let defaulted init != null
            fn twice(n) = n + n
            in ts_node let doubled twice(total)
            in ts_node let cyclic cyclic
            in ts_identifier let size len(text)
            in ts_function_like, ts_static_block rule probe info "{depth}:{total}:{doubled}"
            in ts_assignment rule assign info "{logical}"
            in ts_parameter rule param info "{defaulted}"
            in ts_identifier rule ident info "{size}:{depth}"
            in ts_return rule cycle info "{cyclic}"
            in ts_node rule gated {
                let twice_total twice(total)
                when twice_total > 2 && !boundary
                when depth < 9 || logical
                info "{twice_total}"
            }
        "#;
        let sources = [
            "function f(a = 1, b) { if (a) { b ??= c; } else { d = e?.f; } switch (a) { case 1: break; default: } return; }",
            "class C { x = () => { y = 1; }; static { try { g(); } catch { h(); } } }",
        ];
        let run = |mode| {
            FORCED.set(Some(mode));
            let lint = native_linter(config);
            let out: Vec<Vec<_>> = sources.iter().map(|source| {
                lint.lint(Path::new("/root/a.ts"), source).into_iter().map(|d| (d.rule, d.message, d.start, d.end)).collect()
            }).collect();
            FORCED.set(None);
            out
        };
        let interpreted = run(Mode::Off);
        assert!(interpreted.iter().all(|file| file.len() > 5), "{interpreted:#?}");
        assert!(interpreted.iter().flatten().any(|d| d.1.contains("depends on itself")), "{interpreted:#?}");
        assert_eq!(run(Mode::On), interpreted);
    }

    #[test]
    fn stored_values_round_trip_and_stay_distinct_from_states() {
        for v in [V::Null, V::Bool(false), V::Bool(true), V::Num(0.0), V::Num(-0.0), V::Num(1.5), V::Num(f64::INFINITY), V::Num(-1e300)] {
            match decode(encode(v)) {
                Cell::Ready(back) => assert!(back == v && back.num().to_bits() == v.num().to_bits(), "{v:?} -> {back:?}"),
                _ => panic!("{v:?} decoded as a state"),
            }
        }
        assert!(matches!(decode(encode(V::Num(f64::NAN))), Cell::Ready(V::Num(n)) if n.is_nan()));
        assert!(matches!(decode(UNSET), Cell::Unset));
        assert!(matches!(decode(BUSY), Cell::Busy));
        assert!(matches!(decode(GAVE_UP), Cell::GaveUp));
    }
}
