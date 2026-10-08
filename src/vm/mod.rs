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
use std::cell::RefCell;
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

/// A VM value: what the subset can produce. Text is never built: it is a
/// node's source text or an interned config constant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum V {
    Null,
    Bool(bool),
    Num(f64),
    /// A node as a value (reads as its text).
    Node(u32),
    /// The source text of a node, as plain text.
    NodeText(u32),
    /// A config text constant.
    Str(&'static str),
}

/// Text constants from configs, interned for the process: configs repeat
/// the same few strings, so the pool stays small across reloads.
type Pool = (crate::fx_hash::FxHashMap<&'static str, u32>, Vec<&'static str>);

fn pool() -> &'static std::sync::RwLock<Pool> {
    static POOL: OnceLock<std::sync::RwLock<Pool>> = OnceLock::new();
    POOL.get_or_init(Default::default)
}

/// The pooled copy of a text constant, and its index (stored in records).
fn intern(text: &str) -> (&'static str, u32) {
    if let Some((&s, &i)) = pool().read().expect("pool lock").0.get_key_value(text) {
        return (s, i);
    }
    let mut pool = pool().write().expect("pool lock");
    if let Some((&s, &i)) = pool.0.get_key_value(text) {
        return (s, i);
    }
    let s: &'static str = Box::leak(text.into());
    let i = pool.1.len() as u32;
    pool.0.insert(s, i);
    pool.1.push(s);
    (s, i)
}

/// A text constant by the index [`intern`] gave it.
fn interned(i: u32) -> &'static str {
    pool().read().expect("pool lock").1[i as usize]
}

impl V {
    /// The value's text, when it is text or a node.
    fn text<'s>(self, tree: &Tree, source: &'s str) -> Option<&'s str> {
        match self {
            V::Node(id) | V::NodeText(id) => {
                let n = tree.node(id);
                Some(&source[n.start as usize..n.end as usize])
            }
            V::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn truthy(self, tree: &Tree) -> bool {
        match self {
            V::Null => false,
            V::Bool(b) => b,
            V::Num(n) => n != 0.0,
            V::Node(_) => true,
            V::NodeText(id) => {
                let n = tree.node(id);
                n.end > n.start
            }
            V::Str(s) => !s.is_empty(),
        }
    }

    /// [`Value::num`]: `None` where the interpreter would fail.
    fn num(self, tree: &Tree, source: &str) -> Option<f64> {
        match self {
            V::Null => Some(0.0),
            V::Bool(b) => Some(f64::from(u8::from(b))),
            V::Num(n) => Some(n),
            text => text.text(tree, source)?.trim().parse().ok(),
        }
    }

    /// [`Value::loose_eq`].
    fn loose_eq(self, other: V, tree: &Tree, source: &str) -> bool {
        match (self, other) {
            (V::Node(a), V::Node(b)) => a == b,
            (V::Null, V::Null) => true,
            (V::Bool(a), V::Bool(b)) => a == b,
            (V::Num(a), V::Num(b)) => a == b,
            (V::Num(a), text) | (text, V::Num(a)) => text.text(tree, source).is_some_and(|s| s.trim().parse::<f64>().is_ok_and(|b| b == a)),
            (V::Null, text) | (text, V::Null) => text.text(tree, source).is_some_and(str::is_empty),
            (a, b) => match (a.text(tree, source), b.text(tree, source)) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }

    pub fn value<'s>(self, tree: &Tree, source: &'s str) -> Value<'s> {
        match self {
            V::Null => Value::Null,
            V::Bool(b) => Value::Bool(b),
            V::Num(n) => Value::Num(n),
            V::Node(id) => {
                let n = tree.node(id);
                Value::Node(id, crate::expr::value::Text::Borrowed(&source[n.start as usize..n.end as usize]))
            }
            V::NodeText(id) => {
                let n = tree.node(id);
                Value::from(&source[n.start as usize..n.end as usize])
            }
            V::Str(s) => Value::from(s),
        }
    }

    pub fn same(self, value: &Value, tree: &Tree, source: &str) -> bool {
        match (self, value) {
            (V::Null, Value::Null) => true,
            (V::Bool(a), Value::Bool(b)) => a == *b,
            (V::Num(a), Value::Num(b)) => a == *b || a.is_nan() && b.is_nan(),
            (V::Node(a), Value::Node(b, _)) => a == *b,
            (V::NodeText(_) | V::Str(_), Value::Str(s)) => self.text(tree, source) == Some(&**s),
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
    Attr(u16),
    ParentAttr(u16),
    /// Sum of an attribute over all direct children (null counts as 0).
    SumChildren(u16),
    Default(Box<VExpr>, Box<VExpr>),
    Not(Box<VExpr>),
    Neg(Box<VExpr>),
    If(Box<VExpr>, Box<VExpr>, Box<VExpr>),
    Binary(BinOp, Box<VExpr>, Box<VExpr>),
    /// The node itself, its text, its parent layer node, its span.
    Node,
    Text,
    Parent,
    Offset,
    EndOffset,
    /// A field's text: null, or one node's text (several: give up).
    Field(u16),
    /// `inner` evaluated on the node `base` gives (null stays null).
    On(Box<VExpr>, Box<VExpr>),
    /// `inner` on the single node of a field (none: null).
    OnField(u16, Box<VExpr>),
    OnParent(Box<VExpr>),
    /// A value's kind: its node's matcher, or a kind named by its text.
    IsValue(Box<VExpr>, Box<[u32]>),
    /// The first value when it is a node, else the second.
    Coalesce(Box<VExpr>, Box<VExpr>),
    /// `contains(haystack, needle)` on text, or membership in constants.
    Contains(Box<VExpr>, Box<VExpr>),
    InList(Box<[V]>, Box<VExpr>),
    StartsWith(Box<VExpr>, Box<VExpr>),
    EndsWith(Box<VExpr>, Box<VExpr>),
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

/// The typed form of an attribute definition, or the subexpression that
/// kept it out of the subset.
pub fn lower<'a>(e: &'a CExpr, functions: &'a [UserFn]) -> Result<VExpr, &'a CExpr> {
    scalar(e, &Scope { functions, locals: &[] }, &[], 0)
}

/// The typed form of a rule condition, reading the rule's `let`s inline
/// (they are pure here, so inlining keeps their values).
pub fn lower_condition<'a>(e: &'a CExpr, functions: &'a [UserFn], locals: &'a [CExpr]) -> Result<VExpr, &'a CExpr> {
    scalar(e, &Scope { functions, locals }, &[], 0)
}

/// A short name for what blocked lowering, for `ZLINT_VM_STATS`.
pub fn blocker_name(e: &CExpr) -> String {
    match e {
        CExpr::Call(f, _) => format!("call {}", f.name),
        CExpr::Builtin(b) => format!("builtin {}", b.name()),
        CExpr::Const(v) => format!("const {}", match v {
            Value::Str(_) => "text",
            Value::List(_) => "list",
            Value::Record(_) => "record",
            _ => "other",
        }),
        other => {
            let debug = format!("{other:?}");
            debug.split(['(', ' ', '{']).next().unwrap_or("?").to_string()
        }
    }
}

fn scalar<'a>(e: &'a CExpr, g: &Scope<'a>, args: &[Lowered], depth: u32) -> Result<VExpr, &'a CExpr> {
    match lower_in(e, g, args, depth)? {
        Lowered::V(v) => Ok(v),
        Lowered::Children(_) => Err(e),
    }
}

fn constant(v: &Value) -> Option<V> {
    Some(match v {
        Value::Null => V::Null,
        Value::Bool(b) => V::Bool(*b),
        Value::Num(n) => V::Num(*n),
        Value::Str(s) => V::Str(intern(s).0),
        _ => return None,
    })
}

fn lower_in<'a>(e: &'a CExpr, g: &Scope<'a>, args: &[Lowered], depth: u32) -> Result<Lowered, &'a CExpr> {
    let boxed = |e: &'a CExpr| scalar(e, g, args, depth).map(Box::new);
    Ok(Lowered::V(match e {
        CExpr::Const(v) => VExpr::Const(constant(v).ok_or(e)?),
        CExpr::Hole(inner, _) => return lower_in(inner, g, args, depth),
        CExpr::Param(d) => return args.get(*d as usize).cloned().ok_or(e),
        CExpr::Attr(a) => VExpr::Attr(*a),
        CExpr::ParentAttr(a) => VExpr::ParentAttr(*a),
        CExpr::Relative(Relative::Children, inner) => match &**inner {
            CExpr::Attr(a) => return Ok(Lowered::Children(*a)),
            _ => return Err(e),
        },
        CExpr::Is(subject, tests) if !matches!(&**subject, CExpr::Builtin(Builtin::Node | Builtin::Parent)) => {
            let mut kinds = Vec::new();
            for test in tests {
                match test {
                    IsTest::Kind(ids) => kinds.extend(ids),
                    _ => return Err(e),
                }
            }
            VExpr::IsValue(boxed(subject)?, kinds.into())
        }
        CExpr::Is(subject, tests) => {
            let parent = matches!(&**subject, CExpr::Builtin(Builtin::Parent));
            let mut parts = Vec::new();
            for test in tests {
                parts.push(match test {
                    IsTest::Kind(ids) => VExpr::Is { parent, kinds: ids.clone().into() },
                    IsTest::Field(ids, label) if !parent => VExpr::InField(ids.clone().into(), *label),
                    _ => return Err(e),
                });
            }
            parts.into_iter().reduce(|a, b| VExpr::Binary(BinOp::Or, Box::new(a), Box::new(b))).ok_or(e)?
        }
        CExpr::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b) if matches!((&**a, &**b), (CExpr::Field(_), CExpr::Const(Value::Null)) | (CExpr::Const(Value::Null), CExpr::Field(_))) => {
            let ((CExpr::Field(label), _) | (_, CExpr::Field(label))) = (&**a, &**b) else { return Err(e) };
            let present = VExpr::FieldPresent(*label);
            if *op == BinOp::Ne { present } else { VExpr::Not(Box::new(present)) }
        }
        CExpr::Binary(op, a, b) => match op {
            BinOp::And | BinOp::Or | BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
            | BinOp::Add | BinOp::Sub | BinOp::Mul => VExpr::Binary(*op, boxed(a)?, boxed(b)?),
            _ => return Err(e),
        },
        CExpr::Not(inner) => VExpr::Not(boxed(inner)?),
        CExpr::Neg(inner) => VExpr::Neg(boxed(inner)?),
        CExpr::If(c, a, b) => VExpr::If(boxed(c)?, boxed(a)?, boxed(b)?),
        CExpr::Call(f, call_args) => match (f.name, call_args.as_slice()) {
            ("count", [CExpr::Field(label)]) => VExpr::FieldCount(*label),
            ("default", [a, b]) => VExpr::Default(boxed(a)?, boxed(b)?),
            ("sum", [list]) => match lower_in(list, g, args, depth)? {
                Lowered::Children(a) => VExpr::SumChildren(a),
                Lowered::V(_) => return Err(e),
            },
            ("contains", [CExpr::Const(Value::List(items)), needle]) => {
                let items = items.iter().map(constant).collect::<Option<Box<[V]>>>().ok_or(e)?;
                VExpr::InList(items, boxed(needle)?)
            }
            ("contains", [haystack, needle]) => VExpr::Contains(boxed(haystack)?, boxed(needle)?),
            ("starts_with", [text, prefix]) => VExpr::StartsWith(boxed(text)?, boxed(prefix)?),
            ("ends_with", [text, suffix]) => VExpr::EndsWith(boxed(text)?, boxed(suffix)?),
            _ => return Err(e),
        },
        CExpr::Builtin(b) => match b {
            Builtin::Node => VExpr::Node,
            Builtin::Text => VExpr::Text,
            Builtin::Parent => VExpr::Parent,
            Builtin::Offset => VExpr::Offset,
            Builtin::EndOffset => VExpr::EndOffset,
            _ => return Err(e),
        },
        CExpr::Field(label) => VExpr::Field(*label),
        CExpr::Member(base, _, Some(part)) => VExpr::On(boxed(base)?, boxed(part)?),
        CExpr::OnField(label, inner) => VExpr::OnField(*label, boxed(inner)?),
        CExpr::OnParent(inner) => VExpr::OnParent(boxed(inner)?),
        CExpr::FieldAttr(label, a) => VExpr::OnField(*label, Box::new(VExpr::Attr(*a))),
        CExpr::ParentField(label) => VExpr::OnParent(Box::new(VExpr::Field(*label))),
        CExpr::Coalesce(a, b) => VExpr::Coalesce(boxed(a)?, boxed(b)?),
        // A rule's `let` on its own node, outside every lambda.
        CExpr::Local(i) if args.is_empty() && depth < INLINE_DEPTH => {
            return lower_in(g.locals.get(*i as usize).ok_or(e)?, g, &[], depth + 1);
        }
        CExpr::UserCall(i, call_args) if depth < INLINE_DEPTH => {
            let body = g.functions.get(*i as usize).and_then(|f| f.body.get()).ok_or(e)?;
            let lowered = call_args.iter().map(|a| lower_in(a, g, args, depth)).collect::<Result<Vec<_>, _>>()?;
            return lower_in(body, g, &lowered, depth + 1);
        }
        _ => return Err(e),
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
    let source = h.source();
    let text_of = |v: V| v.text(tree, source);
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
        VExpr::Attr(a) => h.attr(node, *a)?,
        VExpr::ParentAttr(a) => match parent_layer(tree, node) {
            Some(p) => h.attr(p, *a)?,
            None => V::Null,
        },
        VExpr::SumChildren(a) => {
            let mut total = 0.0;
            for edge in &tree.node(node).children {
                total += h.attr(edge.node, *a)?.num(tree, source)?;
            }
            V::Num(total)
        }
        VExpr::Default(a, b) => match eval(h, node, a)? {
            v @ (V::Num(_) | V::Bool(_)) => v,
            v if v.truthy(tree) => v,
            _ => eval(h, node, b)?,
        },
        VExpr::Not(inner) => V::Bool(!eval(h, node, inner)?.truthy(tree)),
        VExpr::Neg(inner) => V::Num(-eval(h, node, inner)?.num(tree, source)?),
        VExpr::If(c, a, b) => if eval(h, node, c)?.truthy(tree) { eval(h, node, a)? } else { eval(h, node, b)? },
        VExpr::Binary(op, a, b) => {
            let a = eval(h, node, a)?;
            match op {
                BinOp::And if !a.truthy(tree) => return Some(V::Bool(false)),
                BinOp::Or if a.truthy(tree) => return Some(V::Bool(true)),
                _ => {}
            }
            let b = eval(h, node, b)?;
            let order = || -> Option<std::cmp::Ordering> {
                let texts = matches!(a, V::NodeText(_) | V::Str(_)) && matches!(b, V::NodeText(_) | V::Str(_));
                match (a.num(tree, source), b.num(tree, source)) {
                    (Some(x), Some(y)) if !texts => Some(x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)),
                    // Text order needs both sides as text the VM holds.
                    _ => Some(text_of(a)?.cmp(text_of(b)?)),
                }
            };
            match op {
                BinOp::And | BinOp::Or => V::Bool(b.truthy(tree)),
                BinOp::Eq => V::Bool(a.loose_eq(b, tree, source)),
                BinOp::Ne => V::Bool(!a.loose_eq(b, tree, source)),
                BinOp::Lt => V::Bool(order()?.is_lt()),
                BinOp::Le => V::Bool(order()?.is_le()),
                BinOp::Gt => V::Bool(order()?.is_gt()),
                BinOp::Ge => V::Bool(order()?.is_ge()),
                BinOp::Add => V::Num(a.num(tree, source)? + b.num(tree, source)?),
                BinOp::Sub => V::Num(a.num(tree, source)? - b.num(tree, source)?),
                BinOp::Mul if !matches!((a, b), (V::NodeText(_) | V::Str(_), V::Num(_)) | (V::Num(_), V::NodeText(_) | V::Str(_))) => {
                    V::Num(a.num(tree, source)? * b.num(tree, source)?)
                }
                _ => return None,
            }
        }
        VExpr::Node => V::Node(node),
        VExpr::Text => V::NodeText(node),
        VExpr::Parent => parent_layer(tree, node).map_or(V::Null, V::Node),
        VExpr::Offset => V::Num(f64::from(tree.node(node).start)),
        VExpr::EndOffset => V::Num(f64::from(tree.node(node).end)),
        VExpr::Field(label) => match tree.fields(node, *label).as_slice() {
            [] => V::Null,
            [one] => V::NodeText(*one),
            _ => return None,
        },
        VExpr::On(base, inner) => match eval(h, node, base)? {
            V::Node(id) => eval(h, id, inner)?,
            _ => V::Null,
        },
        VExpr::OnField(label, inner) => match tree.fields(node, *label).as_slice() {
            [] => V::Null,
            [one] => eval(h, *one, inner)?,
            _ => return None,
        },
        VExpr::OnParent(inner) => match parent_layer(tree, node) {
            Some(p) => eval(h, p, inner)?,
            None => V::Null,
        },
        VExpr::IsValue(subject, kinds) => {
            let matcher = match eval(h, node, subject)? {
                V::Node(id) => Some(tree.node(id).matcher),
                v @ (V::NodeText(_) | V::Str(_)) => h.grammar().matcher(text_of(v)?),
                _ => None,
            };
            V::Bool(matcher.is_some_and(|m| is_a(h.grammar(), m, kinds)))
        }
        VExpr::Coalesce(a, b) => match eval(h, node, a)? {
            v @ V::Node(_) => v,
            _ => eval(h, node, b)?,
        },
        VExpr::InList(items, needle) => {
            let needle = eval(h, node, needle)?;
            V::Bool(items.iter().any(|item| item.loose_eq(needle, tree, source)))
        }
        VExpr::Contains(haystack, needle) => {
            let (haystack, needle) = (eval(h, node, haystack)?, eval(h, node, needle)?);
            V::Bool(text_of(haystack)?.contains(text_of(needle)?))
        }
        VExpr::StartsWith(text, prefix) => {
            let (text, prefix) = (eval(h, node, text)?, eval(h, node, prefix)?);
            V::Bool(text_of(text)?.starts_with(text_of(prefix)?))
        }
        VExpr::EndsWith(text, suffix) => {
            let (text, suffix) = (eval(h, node, text)?, eval(h, node, suffix)?);
            V::Bool(text_of(text)?.ends_with(text_of(suffix)?))
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
/// Nodes, node texts and text constants: a tag in bits 32.. over a 32-bit
/// payload. Inverted, these codes are sign-set NaNs no number encodes to.
const NODE_TAG: u64 = 1 << 32;
const NODE_TEXT_TAG: u64 = 2 << 32;
const STR_TAG: u64 = 3 << 32;

fn encode(v: V) -> u64 {
    match v {
        V::Null => NULL,
        V::Bool(false) => FALSE,
        V::Bool(true) => TRUE,
        V::Num(n) => !(if n.is_nan() { f64::NAN } else { n }).to_bits(),
        V::Node(id) => NODE_TAG | u64::from(id),
        V::NodeText(id) => NODE_TEXT_TAG | u64::from(id),
        V::Str(s) => STR_TAG | u64::from(intern(s).1),
    }
}

fn decode(cell: u64) -> Cell {
    let payload = cell as u32;
    match cell {
        UNSET => Cell::Unset,
        BUSY => Cell::Busy,
        NULL => Cell::Ready(V::Null),
        FALSE => Cell::Ready(V::Bool(false)),
        TRUE => Cell::Ready(V::Bool(true)),
        GAVE_UP => Cell::GaveUp,
        _ if cell >> 32 == NODE_TAG >> 32 => Cell::Ready(V::Node(payload)),
        _ if cell >> 32 == NODE_TEXT_TAG >> 32 => Cell::Ready(V::NodeText(payload)),
        _ if cell >> 32 == STR_TAG >> 32 => Cell::Ready(V::Str(interned(payload))),
        bits => Cell::Ready(V::Num(f64::from_bits(!bits))),
    }
}

/// One file's VM-computed attribute values, as 8-byte cells keyed by
/// (node, attribute). Rules read only a few values on most nodes, so only
/// computed values take space.
#[derive(Default)]
pub struct FileVm {
    cells: RefCell<crate::fx_hash::FxHashMap<(u32, u16), u64>>,
}

/// What [`FileVm::begin`] found.
pub enum Begin {
    Ready(V),
    /// Busy, or given up before: the interpreter handles it.
    GiveUp,
    /// Marked busy: evaluate, then [`FileVm::finish`].
    Compute,
}

impl FileVm {
    pub fn begin(&self, node: u32, attr: u16) -> Begin {
        let mut cells = self.cells.borrow_mut();
        let cell = cells.entry((node, attr)).or_insert(UNSET);
        match decode(*cell) {
            Cell::Ready(v) => Begin::Ready(v),
            Cell::Busy | Cell::GaveUp => Begin::GiveUp,
            Cell::Unset => {
                *cell = BUSY;
                Begin::Compute
            }
        }
    }

    /// Publishes a value, or marks the cell given up so later reads go
    /// straight to the interpreter.
    pub fn finish(&self, node: u32, attr: u16, value: Option<V>) {
        self.cells.borrow_mut().insert((node, attr), value.map_or(GAVE_UP, encode));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::functions;

    #[test]
    fn only_bodies_the_vm_can_answer_without_building_values_lower() {
        let g: &[UserFn] = &[];
        let num = |n: f64| Box::new(CExpr::Const(Value::Num(n)));
        let call = |name: &str, args: Vec<CExpr>| CExpr::Call(functions::lookup(name).expect("registered"), args);
        for supported in [
            CExpr::Binary(BinOp::Add, num(1.0), num(2.0)),
            CExpr::If(Box::new(CExpr::Attr(0)), num(1.0), Box::new(CExpr::ParentAttr(1))),
            call("sum", vec![CExpr::Relative(Relative::Children, Box::new(CExpr::Attr(2)))]),
            call("default", vec![CExpr::Attr(0), CExpr::Const(Value::Num(0.0))]),
            CExpr::Binary(BinOp::Ne, Box::new(CExpr::Field(3)), Box::new(CExpr::Const(Value::Null))),
            CExpr::Const(Value::from("text")),
            call("starts_with", vec![CExpr::Builtin(Builtin::Text), CExpr::Const(Value::from("a"))]),
            CExpr::OnParent(Box::new(CExpr::Field(3))),
        ] {
            assert!(lower(&supported, g).is_ok(), "{supported:?}");
        }
        for unsupported in [
            CExpr::Binary(BinOp::Rem, num(1.0), num(0.0)),
            CExpr::Binary(BinOp::Join, num(1.0), num(2.0)),
            CExpr::Relative(Relative::Children, Box::new(CExpr::Attr(2))),
            call("len", vec![CExpr::Builtin(Builtin::Text)]),
            CExpr::Builtin(Builtin::Line),
        ] {
            assert!(lower(&unsupported, g).is_err(), "{unsupported:?}");
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
            in ts_identifier let named starts_with(text, "b") || text == "c" || parent is ts_assignment
            in ts_assignment let left_named left.named
            in ts_identifier rule named info "{named}"
            in ts_assignment rule left info "{left_named}:{operator == '??='}:{parent.depth}"
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
        for v in [
            V::Null, V::Bool(false), V::Bool(true), V::Num(0.0), V::Num(-0.0), V::Num(1.5), V::Num(f64::INFINITY),
            V::Num(f64::NEG_INFINITY), V::Num(-1e300), V::Node(0), V::Node(u32::MAX), V::NodeText(7), V::Str(intern("a").0), V::Str(intern("").0),
        ] {
            match (decode(encode(v)), v) {
                (Cell::Ready(V::Num(back)), V::Num(n)) => assert_eq!(back.to_bits(), n.to_bits(), "{v:?}"),
                (Cell::Ready(back), _) => assert_eq!(back, v),
                _ => panic!("{v:?} decoded as a state"),
            }
        }
        assert!(matches!(decode(encode(V::Num(f64::NAN))), Cell::Ready(V::Num(n)) if n.is_nan()));
        assert!(matches!(decode(UNSET), Cell::Unset));
        assert!(matches!(decode(BUSY), Cell::Busy));
        assert!(matches!(decode(GAVE_UP), Cell::GaveUp));
    }
}
