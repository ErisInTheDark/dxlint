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
use crate::expr::{facts, Builtin, CExpr, Each, FactOf, IsTest, Relative, Value};
use crate::grammar::{Grammar, Tree, UserFn, ROOT, TEXT};
use std::sync::OnceLock;

mod records;
pub use records::{Begin, FileVm};

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
    Nullish(Box<VExpr>, Box<VExpr>),
    /// `contains(haystack, needle)` on text, or membership in constants.
    Contains(Box<VExpr>, Box<VExpr>),
    InList(Box<[V]>, Box<VExpr>),
    StartsWith(Box<VExpr>, Box<VExpr>),
    EndsWith(Box<VExpr>, Box<VExpr>),
    /// A lambda parameter or pipeline value: frame slot `n`.
    Bound(u16),
    /// A pipeline value, evaluated once and bound at the slot for the body.
    Let(Box<VExpr>, u16, Box<VExpr>),
    /// `type_of(x) == "null"`: the null-only test behind `??`.
    IsNull(Box<VExpr>),
    /// `nonempty(x)`: null for empty text, otherwise the value.
    NonEmpty(Box<VExpr>),
    /// A native fact read (whole, or one registered projection).
    Fact { fact: usize, projection: Option<usize>, of: FactOf },
    /// `re_test(text, "pattern")` with a constant, valid pattern.
    ReTest(Box<VExpr>, &'static str),
    /// How many items a sequence has, their sum, its first or last item.
    Count(Box<Seq>),
    Sum(Box<Seq>),
    First(Box<Seq>),
    Last(Box<Seq>),
    /// `find`/`find_last`/`all`/`any` over a sequence.
    Each(EachKind, Box<Seq>, Box<VExpr>, Binder),
    /// `fold` (from `init`) or `reduce` (from the first item): the state at
    /// `slot`, the item after it.
    Fold { of: Box<Seq>, init: Option<Box<VExpr>>, body: Box<VExpr>, slot: u16 },
    /// `min_by`/`max_by` over numeric keys; the first item wins ties.
    Pick { max: bool, of: Box<Seq>, key: Box<VExpr>, bind: Binder },
}

/// Where an item stage's lambda binds its item (and index, if it takes one).
/// Binding saves and restores those frame slots, so a lowered sequence can
/// be walked at any frame depth (a `fn` argument or pipeline value used
/// inside a deeper lambda).
#[derive(Clone, Copy, Debug)]
pub struct Binder {
    slot: u16,
    indexed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EachKind {
    Find,
    FindLast,
    All,
    Any,
}

/// A list the VM walks without building it.
#[derive(Clone, Debug)]
pub enum Seq {
    /// `item` on each node of a field (as a field read: one node gives its
    /// value, so a null there is no item), or of every child.
    Nodes { children: bool, label: u16, item: Box<VExpr> },
    Const(Box<[V]>),
    /// An item stage, its lambda run on each item.
    Stage(SeqStage, Box<Seq>, Box<VExpr>, Binder),
    /// `flat_map`: each item's body is itself a sequence.
    FlatMap(Box<Seq>, Box<Seq>, Binder),
    /// `zip other (a, b) body`, as long as the shorter; the pair binds at
    /// the slot.
    Zip(Box<Seq>, Box<Seq>, Box<VExpr>, u16),
    Take(Box<Seq>, Box<VExpr>),
    Drop(Box<Seq>, Box<VExpr>),
    Concat(Box<Seq>, Box<Seq>),
    Reverse(Box<Seq>),
    Unique(Box<Seq>),
    /// A pipeline value bound at the slot while the sequence is walked.
    Let(Box<VExpr>, u16, Box<Seq>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqStage {
    Filter,
    Map,
    TakeWhile,
    SkipWhile,
    UniqueBy,
}

#[derive(Clone)]
enum Lowered {
    V(VExpr),
    Seq(Seq),
    /// A parameter the VM has no value for (a rule's `each` item).
    Unbound,
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
    scalar(e, &Scope { functions, locals: &[] }, &[], 0, 0)
}

/// The typed form of a rule condition, reading the rule's `let`s inline
/// (they are pure here, so inlining keeps their values).
pub fn lower_condition<'a>(e: &'a CExpr, functions: &'a [UserFn], locals: &'a [CExpr]) -> Result<VExpr, &'a CExpr> {
    scalar(e, &Scope { functions, locals }, &[], 0, 0)
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

fn scalar<'a>(e: &'a CExpr, g: &Scope<'a>, args: &[Lowered], depth: u32, bound: u16) -> Result<VExpr, &'a CExpr> {
    match lower_in(e, g, args, depth, bound)? {
        Lowered::V(v) => Ok(v),
        _ => Err(e),
    }
}

/// `e` as a list the VM can walk.
fn seq<'a>(e: &'a CExpr, g: &Scope<'a>, args: &[Lowered], depth: u32, bound: u16) -> Result<Seq, &'a CExpr> {
    match lower_in(e, g, args, depth, bound)? {
        Lowered::Seq(s) => Ok(s),
        Lowered::V(VExpr::Field(label)) => Ok(Seq::Nodes { children: false, label, item: Box::new(VExpr::Text) }),
        Lowered::V(VExpr::OnField(label, item)) => Ok(Seq::Nodes { children: false, label, item }),
        _ => Err(e),
    }
}

/// `args` with `arity` parameters at `param` bound from frame slot `bound`
/// (the rule's `each` item and other unknown parameters below stay unbound).
fn bind_params(args: &[Lowered], param: u16, arity: u16, bound: u16) -> Vec<Lowered> {
    let mut inner = args.to_vec();
    inner.resize(param as usize, Lowered::Unbound);
    inner.extend((0..arity).map(|i| Lowered::V(VExpr::Bound(bound + i))));
    inner
}

/// The `x` of `type_of(x) == "null"`, either way round.
fn null_test<'a>(a: &'a CExpr, b: &'a CExpr) -> Option<&'a CExpr> {
    match (a, b) {
        (CExpr::Call(f, x), CExpr::Const(Value::Str(s))) | (CExpr::Const(Value::Str(s)), CExpr::Call(f, x))
            if f.name == "type_of" && &**s == "null" && x.len() == 1 => Some(&x[0]),
        _ => None,
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

fn lower_in<'a>(e: &'a CExpr, g: &Scope<'a>, args: &[Lowered], depth: u32, bound: u16) -> Result<Lowered, &'a CExpr> {
    let boxed = |e: &'a CExpr| scalar(e, g, args, depth, bound).map(Box::new);
    let boxed_seq = |e: &'a CExpr| seq(e, g, args, depth, bound).map(Box::new);
    Ok(Lowered::V(match e {
        CExpr::Const(Value::List(items)) => {
            return Ok(Lowered::Seq(Seq::Const(items.iter().map(constant).collect::<Option<Box<[V]>>>().ok_or(e)?)));
        }
        CExpr::Const(v) => VExpr::Const(constant(v).ok_or(e)?),
        CExpr::Hole(inner, _) => return lower_in(inner, g, args, depth, bound),
        CExpr::Param(d) => match args.get(*d as usize) {
            Some(Lowered::Unbound) | None => return Err(e),
            Some(lowered) => return Ok(lowered.clone()),
        },
        CExpr::Attr(a) => VExpr::Attr(*a),
        CExpr::ParentAttr(a) => VExpr::ParentAttr(*a),
        CExpr::Fact(fact, of) => VExpr::Fact { fact: *fact, projection: None, of: *of },
        CExpr::FactField(read, of) => VExpr::Fact { fact: facts::FIELD_READS[*read].fact, projection: Some(*read), of: *of },
        CExpr::Relative(Relative::Children, inner) => {
            return Ok(Lowered::Seq(Seq::Nodes { children: true, label: 0, item: boxed(inner)? }));
        }
        CExpr::Each(stage) => {
            let arity = u16::from(stage.arity);
            let inner = bind_params(args, stage.depth, arity, bound);
            let body = || scalar(&stage.body, g, &inner, depth, bound + arity).map(Box::new);
            let bind = Binder { slot: bound, indexed: stage.arity == 2 };
            let of = boxed_seq(&stage.input)?;
            return Ok(match (stage.kind, stage.args.as_slice()) {
                (Each::Fold, [init]) => Lowered::V(VExpr::Fold { of, init: Some(boxed(init)?), body: body()?, slot: bound }),
                (Each::Reduce, []) => Lowered::V(VExpr::Fold { of, init: None, body: body()?, slot: bound }),
                (Each::Zip, [other]) => Lowered::Seq(Seq::Zip(of, boxed_seq(other)?, body()?, bound)),
                (Each::FlatMap, []) => Lowered::Seq(Seq::FlatMap(of, Box::new(seq(&stage.body, g, &inner, depth, bound + arity)?), bind)),
                (kind, []) => {
                    let body = body()?;
                    let stage = match kind {
                        Each::Filter => Some(SeqStage::Filter),
                        Each::Map => Some(SeqStage::Map),
                        Each::TakeWhile => Some(SeqStage::TakeWhile),
                        Each::SkipWhile => Some(SeqStage::SkipWhile),
                        Each::UniqueBy => Some(SeqStage::UniqueBy),
                        _ => None,
                    };
                    if let Some(stage) = stage {
                        Lowered::Seq(Seq::Stage(stage, of, body, bind))
                    } else {
                        Lowered::V(match kind {
                            Each::Find => VExpr::Each(EachKind::Find, of, body, bind),
                            Each::FindLast => VExpr::Each(EachKind::FindLast, of, body, bind),
                            Each::All => VExpr::Each(EachKind::All, of, body, bind),
                            Each::Any => VExpr::Each(EachKind::Any, of, body, bind),
                            Each::MinBy | Each::MaxBy => VExpr::Pick { max: kind == Each::MaxBy, of, key: body, bind },
                            // These build lists or records.
                            _ => return Err(e),
                        })
                    }
                }
                _ => return Err(e),
            });
        }
        CExpr::Bind(value, param, body) => {
            let mut inner = bind_params(args, *param, 0, bound);
            return Ok(match lower_in(value, g, args, depth, bound)? {
                // A sequence stays a view: walked again where it is used.
                Lowered::Seq(s) => {
                    inner.push(Lowered::Seq(s));
                    lower_in(body, g, &inner, depth, bound)?
                }
                Lowered::V(v) => {
                    inner.push(Lowered::V(VExpr::Bound(bound)));
                    match lower_in(body, g, &inner, depth, bound + 1)? {
                        Lowered::V(b) => Lowered::V(VExpr::Let(Box::new(v), bound, Box::new(b))),
                        Lowered::Seq(s) => Lowered::Seq(Seq::Let(Box::new(v), bound, Box::new(s))),
                        Lowered::Unbound => return Err(e),
                    }
                }
                Lowered::Unbound => return Err(e),
            });
        }
        // `[] x` and `.nodes` remain list views, including pipeline seeds.
        CExpr::Binary(BinOp::Join, a, b) if matches!(&**a, CExpr::Const(Value::List(items)) if items.is_empty()) => {
            return Ok(Lowered::Seq(seq(b, g, args, depth, bound)?));
        }
        CExpr::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b) if null_test(a, b).is_some() => {
            let test = VExpr::IsNull(boxed(null_test(a, b).expect("matched"))?);
            if *op == BinOp::Eq { test } else { VExpr::Not(Box::new(test)) }
        }
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
            ("count", [list]) => VExpr::Count(boxed_seq(list)?),
            ("first", [list]) => VExpr::First(boxed_seq(list)?),
            ("last", [list]) => VExpr::Last(boxed_seq(list)?),
            ("sum", [list]) => VExpr::Sum(boxed_seq(list)?),
            ("nonempty", [value]) => VExpr::NonEmpty(boxed(value)?),
            ("take", [list, n]) => return Ok(Lowered::Seq(Seq::Take(boxed_seq(list)?, boxed(n)?))),
            ("drop", [list, n]) => return Ok(Lowered::Seq(Seq::Drop(boxed_seq(list)?, boxed(n)?))),
            ("concat", [a, b]) => return Ok(Lowered::Seq(Seq::Concat(boxed_seq(a)?, boxed_seq(b)?))),
            ("unique", [list]) => return Ok(Lowered::Seq(Seq::Unique(boxed_seq(list)?))),
            // Reversing a text reverses its characters: lists only.
            ("reverse", [list]) => match lower_in(list, g, args, depth, bound)? {
                Lowered::Seq(s) => return Ok(Lowered::Seq(Seq::Reverse(Box::new(s)))),
                _ => return Err(e),
            },
            ("contains", [CExpr::Const(Value::List(items)), needle]) => {
                let items = items.iter().map(constant).collect::<Option<Box<[V]>>>().ok_or(e)?;
                VExpr::InList(items, boxed(needle)?)
            }
            ("contains", [haystack, needle]) => VExpr::Contains(boxed(haystack)?, boxed(needle)?),
            ("re_test", [text, CExpr::Const(Value::Str(pattern))]) if crate::expr::re::compile(pattern).is_ok() => {
                VExpr::ReTest(boxed(text)?, intern(pattern).0)
            }
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
        CExpr::Nullish(a, b) => VExpr::Nullish(boxed(a)?, boxed(b)?),
        CExpr::NonEmpty(value) => VExpr::NonEmpty(boxed(value)?),
        // A rule's `let` on its own node; lambdas don't change the node.
        CExpr::Local(i) if depth < INLINE_DEPTH => {
            return lower_in(g.locals.get(*i as usize).ok_or(e)?, g, &[], depth + 1, bound);
        }
        CExpr::UserCall(i, call_args) if depth < INLINE_DEPTH => {
            let body = g.functions.get(*i as usize).and_then(|f| f.body.get()).ok_or(e)?;
            let lowered = call_args.iter().map(|a| lower_in(a, g, args, depth, bound)).collect::<Result<Vec<_>, _>>()?;
            return lower_in(body, g, &lowered, depth + 1, bound);
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
    /// A native fact read on a node, when its value is one the VM holds.
    fn fact(&self, node: u32, fact: usize, projection: Option<usize>, of: FactOf) -> Option<V>;
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
    ev(h, node, e, &mut Vec::new())
}

/// Calls `each` with the items of `s` until it returns `false`; whether
/// it ran to the end. `None`: the VM gives up.
fn walk_seq(h: &impl Host, node: u32, s: &Seq, f: &mut Vec<V>, each: &mut dyn FnMut(V, &mut Vec<V>) -> Option<bool>) -> Option<bool> {
    let tree = h.tree();
    match s {
        Seq::Const(items) => {
            for &v in items.iter() {
                if !each(v, f)? {
                    return Some(false);
                }
            }
        }
        Seq::Nodes { children: true, item, .. } => {
            for edge in &tree.node(node).children {
                let v = ev(h, edge.node, item, f)?;
                if !each(v, f)? {
                    return Some(false);
                }
            }
        }
        Seq::Nodes { label, item, .. } => {
            let nodes = tree.fields(node, *label);
            // One node reads as its value: a null there is no item.
            if let [one] = nodes.as_slice() {
                let v = ev(h, *one, item, f)?;
                return if v == V::Null { Some(true) } else { each(v, f) };
            }
            for n in nodes {
                let v = ev(h, n, item, f)?;
                if !each(v, f)? {
                    return Some(false);
                }
            }
        }
        Seq::Stage(stage, of, body, bind) => {
            let source = h.source();
            let mut index = 0.0;
            // `take_while`: still taking; `skip_while`: still skipping.
            let mut open = true;
            let mut keys: Vec<V> = Vec::new();
            return walk_seq(h, node, of, f, &mut |v, f| {
                let at = index;
                index += 1.0;
                match stage {
                    SeqStage::Map => {
                        let mapped = call_bound(h, node, body, f, *bind, v, at)?;
                        each(mapped, f)
                    }
                    SeqStage::Filter => if call_bound(h, node, body, f, *bind, v, at)?.truthy(tree) { each(v, f) } else { Some(true) },
                    SeqStage::TakeWhile => {
                        open = open && call_bound(h, node, body, f, *bind, v, at)?.truthy(tree);
                        if open { each(v, f) } else { Some(true) }
                    }
                    SeqStage::SkipWhile => {
                        open = open && call_bound(h, node, body, f, *bind, v, at)?.truthy(tree);
                        if open { Some(true) } else { each(v, f) }
                    }
                    SeqStage::UniqueBy => {
                        let key = call_bound(h, node, body, f, *bind, v, at)?;
                        if keys.iter().any(|k| k.loose_eq(key, tree, source)) { return Some(true); }
                        keys.push(key);
                        each(v, f)
                    }
                }
            });
        }
        Seq::FlatMap(of, body, bind) => {
            let mut index = 0.0;
            return walk_seq(h, node, of, f, &mut |v, f| {
                let values = [v, V::Num(index)];
                index += 1.0;
                with_bound(f, bind.slot, &values[..bind.width()], |f| walk_seq(h, node, body, f, &mut *each))
            });
        }
        Seq::Zip(of, other, body, slot) => {
            let mut others = Vec::new();
            walk_seq(h, node, other, f, &mut |v, _| { others.push(v); Some(true) })?;
            let mut index = 0;
            return walk_seq(h, node, of, f, &mut |v, f| {
                // Past the shorter list, items are still evaluated (as the
                // interpreter does) but not paired.
                let Some(&b) = others.get(index) else { return Some(true) };
                index += 1;
                let paired = with_bound(f, *slot, &[v, b], |f| ev(h, node, body, f))?;
                each(paired, f)
            });
        }
        Seq::Take(of, n) | Seq::Drop(of, n) => {
            let n = whole(ev(h, node, n, f)?.num(tree, h.source())?)?;
            let take = matches!(s, Seq::Take(..));
            let mut seen = 0;
            return walk_seq(h, node, of, f, &mut |v, f| {
                seen += 1;
                if (seen <= n) == take { each(v, f) } else { Some(true) }
            });
        }
        Seq::Concat(a, b) => {
            if !walk_seq(h, node, a, f, &mut *each)? {
                return Some(false);
            }
            return walk_seq(h, node, b, f, each);
        }
        Seq::Reverse(of) => {
            let mut items = Vec::new();
            walk_seq(h, node, of, f, &mut |v, _| { items.push(v); Some(true) })?;
            for v in items.into_iter().rev() {
                if !each(v, f)? {
                    return Some(false);
                }
            }
        }
        Seq::Unique(of) => {
            let source = h.source();
            let mut seen: Vec<V> = Vec::new();
            return walk_seq(h, node, of, f, &mut |v, f| {
                if seen.iter().any(|s| s.loose_eq(v, tree, source)) { return Some(true); }
                seen.push(v);
                each(v, f)
            });
        }
        Seq::Let(value, slot, body) => {
            let v = ev(h, node, value, f)?;
            return with_bound(f, *slot, &[v], |f| walk_seq(h, node, body, f, each));
        }
    }
    Some(true)
}

impl Binder {
    fn width(self) -> usize {
        1 + usize::from(self.indexed)
    }
}

/// Runs `run` with `values` in the frame from `slot`, restoring what those
/// slots held before.
fn with_bound<R>(f: &mut Vec<V>, slot: u16, values: &[V], run: impl FnOnce(&mut Vec<V>) -> R) -> R {
    let at = slot as usize;
    let end = at + values.len();
    if f.len() < end {
        f.resize(end, V::Null);
    }
    let mut saved = [V::Null; 2];
    saved[..values.len()].copy_from_slice(&f[at..end]);
    f[at..end].copy_from_slice(values);
    let result = run(f);
    f[at..end].copy_from_slice(&saved[..values.len()]);
    result
}

/// A stage lambda on one item (and its index, when the lambda takes one).
fn call_bound(h: &impl Host, node: u32, body: &VExpr, f: &mut Vec<V>, bind: Binder, item: V, index: f64) -> Option<V> {
    let values = [item, V::Num(index)];
    with_bound(f, bind.slot, &values[..bind.width()], |f| ev(h, node, body, f))
}

/// A count argument (`take`, `drop`): a whole non-negative number, else the
/// interpreter reports the error.
fn whole(n: f64) -> Option<usize> {
    (n.is_finite() && n >= 0.0 && n.fract() == 0.0).then_some(n as usize)
}

fn ev(h: &impl Host, node: u32, e: &VExpr, f: &mut Vec<V>) -> Option<V> {
    let tree = h.tree();
    let source = h.source();
    let text_of = |v: V| v.text(tree, source);
    macro_rules! eval {
        ($node:expr, $e:expr) => {
            ev(h, $node, $e, f)
        };
    }
    Some(match e {
        VExpr::Bound(slot) => *f.get(*slot as usize)?,
        VExpr::Fact { fact, projection, of } => h.fact(node, *fact, *projection, *of)?,
        VExpr::ReTest(text, pattern) => {
            let hay = match eval!(node, text)? {
                V::Null => "",
                v => text_of(v)?,
            };
            let found = crate::expr::re::test(&[Value::from(hay), Value::from(*pattern)]).ok()?;
            V::Bool(found.truthy())
        }
        VExpr::Count(s) => {
            let mut n = 0.0;
            walk_seq(h, node, s, f, &mut |_, _| { n += 1.0; Some(true) })?;
            V::Num(n)
        }
        VExpr::Sum(s) => {
            let mut total = 0.0;
            walk_seq(h, node, s, f, &mut |v, _| { total += v.num(tree, source)?; Some(true) })?;
            V::Num(total)
        }
        // The interpreter builds the whole list first, so every item is
        // still evaluated (an item the VM gives up on must reach it).
        VExpr::First(s) => {
            let mut first = None;
            walk_seq(h, node, s, f, &mut |v, _| { first.get_or_insert(v); Some(true) })?;
            first.unwrap_or(V::Null)
        }
        VExpr::Last(s) => {
            let mut last = V::Null;
            walk_seq(h, node, s, f, &mut |v, _| { last = v; Some(true) })?;
            last
        }
        // Like the interpreter, the whole list is evaluated before any test.
        VExpr::Each(kind, s, test, bind) => {
            let mut items = Vec::new();
            walk_seq(h, node, s, f, &mut |v, _| { items.push(v); Some(true) })?;
            let holds = |at: usize, f: &mut Vec<V>| -> Option<bool> {
                Some(call_bound(h, node, test, f, *bind, items[at], at as f64)?.truthy(tree))
            };
            match kind {
                EachKind::Find | EachKind::FindLast => {
                    let mut found = V::Null;
                    let order: Box<dyn Iterator<Item = usize>> =
                        if *kind == EachKind::Find { Box::new(0..items.len()) } else { Box::new((0..items.len()).rev()) };
                    for at in order {
                        if holds(at, f)? { found = items[at]; break; }
                    }
                    found
                }
                EachKind::All => {
                    let mut all = true;
                    for at in 0..items.len() {
                        if !holds(at, f)? { all = false; break; }
                    }
                    V::Bool(all)
                }
                EachKind::Any => {
                    let mut any = false;
                    for at in 0..items.len() {
                        if holds(at, f)? { any = true; break; }
                    }
                    V::Bool(any)
                }
            }
        }
        VExpr::Fold { of, init, body, slot } => {
            let mut state = match init {
                Some(init) => Some(eval!(node, init)?),
                None => None,
            };
            walk_seq(h, node, of, f, &mut |v, f| {
                state = Some(match state {
                    None => v,
                    Some(acc) => with_bound(f, *slot, &[acc, v], |f| ev(h, node, body, f))?,
                });
                Some(true)
            })?;
            state.unwrap_or(V::Null)
        }
        VExpr::Pick { max, of, key, bind } => {
            let mut best: Option<(f64, V)> = None;
            let mut index = 0.0;
            walk_seq(h, node, of, f, &mut |v, f| {
                // Only numeric keys: other values (and NaN) use the
                // interpreter's general ordering.
                let V::Num(k) = call_bound(h, node, key, f, *bind, v, index)? else { return None };
                index += 1.0;
                if k.is_nan() {
                    return None;
                }
                if best.is_none_or(|(b, _)| if *max { k > b } else { k < b }) {
                    best = Some((k, v));
                }
                Some(true)
            })?;
            best.map_or(V::Null, |(_, v)| v)
        }
        VExpr::Let(value, slot, body) => {
            let v = eval!(node, value)?;
            with_bound(f, *slot, &[v], |f| ev(h, node, body, f))?
        }
        VExpr::IsNull(value) => V::Bool(eval!(node, value)? == V::Null),
        VExpr::NonEmpty(value) => match eval!(node, value)? {
            V::Str("") => V::Null,
            V::NodeText(id) if tree.node(id).end == tree.node(id).start => V::Null,
            v => v,
        },
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
        VExpr::Not(inner) => V::Bool(!eval!(node, inner)?.truthy(tree)),
        VExpr::Neg(inner) => V::Num(-eval!(node, inner)?.num(tree, source)?),
        VExpr::If(c, a, b) => if eval!(node, c)?.truthy(tree) { eval!(node, a)? } else { eval!(node, b)? },
        VExpr::Binary(op, a, b) => {
            let a = eval!(node, a)?;
            match op {
                BinOp::And if !a.truthy(tree) => return Some(V::Bool(false)),
                BinOp::Or if a.truthy(tree) => return Some(V::Bool(true)),
                _ => {}
            }
            let b = eval!(node, b)?;
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
        VExpr::On(base, inner) => match eval!(node, base)? {
            V::Node(id) => eval!(id, inner)?,
            _ => V::Null,
        },
        VExpr::OnField(label, inner) => match tree.fields(node, *label).as_slice() {
            [] => V::Null,
            [one] => eval!(*one, inner)?,
            _ => return None,
        },
        VExpr::OnParent(inner) => match parent_layer(tree, node) {
            Some(p) => eval!(p, inner)?,
            None => V::Null,
        },
        VExpr::IsValue(subject, kinds) => {
            let matcher = match eval!(node, subject)? {
                V::Node(id) => Some(tree.node(id).matcher),
                v @ (V::NodeText(_) | V::Str(_)) => h.grammar().matcher(text_of(v)?),
                _ => None,
            };
            V::Bool(matcher.is_some_and(|m| is_a(h.grammar(), m, kinds)))
        }
        VExpr::Coalesce(a, b) => match eval!(node, a)? {
            v @ V::Node(_) => v,
            _ => eval!(node, b)?,
        },
        VExpr::Nullish(a, b) => match eval!(node, a)? {
            V::Null => eval!(node, b)?,
            value => value,
        },
        VExpr::InList(items, needle) => {
            let needle = eval!(node, needle)?;
            V::Bool(items.iter().any(|item| item.loose_eq(needle, tree, source)))
        }
        VExpr::Contains(haystack, needle) => {
            let (haystack, needle) = (eval!(node, haystack)?, eval!(node, needle)?);
            V::Bool(text_of(haystack)?.contains(text_of(needle)?))
        }
        VExpr::StartsWith(text, prefix) => {
            let (text, prefix) = (eval!(node, text)?, eval!(node, prefix)?);
            V::Bool(text_of(text)?.starts_with(text_of(prefix)?))
        }
        VExpr::EndsWith(text, suffix) => {
            let (text, suffix) = (eval!(node, text)?, eval!(node, suffix)?);
            V::Bool(text_of(text)?.ends_with(text_of(suffix)?))
        }
    })
}

/// A set of matchers, as a bitset over matcher ids (ROOT and TEXT are never
/// members: they hold no attributes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindSet(Vec<u64>);

impl KindSet {
    pub fn empty(matchers: usize) -> Self {
        KindSet(vec![0; matchers.div_ceil(64)])
    }

    pub fn all(matchers: usize) -> Self {
        let mut set = Self::empty(matchers);
        for k in 0..matchers as u32 {
            set.insert(k);
        }
        set
    }

    /// Adds `k`; whether it was new. Ids outside the set (ROOT, TEXT) are ignored.
    pub fn insert(&mut self, k: u32) -> bool {
        let Some(word) = self.0.get_mut(k as usize / 64) else { return false };
        let bit = 1u64 << (k % 64);
        let new = *word & bit == 0;
        *word |= bit;
        new
    }

    pub fn contains(&self, k: u32) -> bool {
        self.0.get(k as usize / 64).is_some_and(|word| word >> (k % 64) & 1 == 1)
    }

    pub fn extend(&mut self, other: &KindSet) {
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a |= b;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&w| w == 0)
    }

    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().enumerate().flat_map(|(i, &word)| (0..64).filter(move |b| word >> b & 1 == 1).map(move |b| (i * 64 + b) as u32))
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
            CExpr::Binary(BinOp::Ne, Box::new(CExpr::Field(3)), Box::new(CExpr::Const(Value::Null))),
            CExpr::Const(Value::from("text")),
            call("starts_with", vec![CExpr::Builtin(Builtin::Text), CExpr::Const(Value::from("a"))]),
            CExpr::OnParent(Box::new(CExpr::Field(3))),
        ] {
            assert!(lower(&supported, g).is_ok(), "{supported:?}");
        }
        let children = || CExpr::Relative(Relative::Children, Box::new(CExpr::Attr(2)));
        let stage = |kind, depth, arity, args, body| {
            CExpr::Each(Box::new(crate::expr::pipeline::Stage { kind, depth, arity, input: children(), args, body }))
        };
        let add = |a: u16, b: u16| CExpr::Binary(BinOp::Add, Box::new(CExpr::Param(a)), Box::new(CExpr::Param(b)));
        for supported in [
            call("count", vec![stage(Each::Filter, 0, 2, vec![], CExpr::Param(1))]),
            stage(Each::Fold, 0, 2, vec![CExpr::Const(Value::Num(0.0))], add(0, 1)),
            stage(Each::MaxBy, 0, 1, vec![], CExpr::Param(0)),
            call("count", vec![call("take", vec![children(), CExpr::Const(Value::Num(1.0))])]),
            CExpr::NonEmpty(Box::new(CExpr::Builtin(Builtin::Text))),
            // `x ?? 1`
            CExpr::Nullish(Box::new(CExpr::Attr(0)), num(1.0)),
            // A filtered view used inside a later lambda.
            CExpr::Bind(
                Box::new(stage(Each::Filter, 0, 1, vec![], CExpr::Param(0))),
                0,
                Box::new(stage(Each::All, 1, 1, vec![], call("count", vec![CExpr::Param(0)]))),
            ),
            CExpr::Bind(
                Box::new(CExpr::Binary(BinOp::Join, Box::new(CExpr::Const(Value::list(vec![]))), Box::new(CExpr::Field(3)))),
                0,
                Box::new(call("count", vec![CExpr::Param(0)])),
            ),
        ] {
            assert!(lower(&supported, g).is_ok(), "{supported:?}");
        }
        let local = CExpr::Bind(num(3.0), 0, Box::new(CExpr::Param(0)));
        let condition = CExpr::Bind(num(10.0), 0, Box::new(CExpr::Binary(
            BinOp::Eq,
            Box::new(CExpr::Binary(BinOp::Add, Box::new(CExpr::Param(0)), Box::new(CExpr::Local(0)))),
            num(13.0),
        )));
        assert!(lower_condition(&condition, g, &[local]).is_ok(), "a rule let remains lowerable inside a binding");
        for unsupported in [
            stage(Each::SortBy, 0, 1, vec![], CExpr::Param(0)),
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
            in ts_node let depth (parent.depth ?? 0) + 1
            in ts_block let local count(body)
            in ts_node let boundary is (ts_function_like, ts_static_block) || is ts_class_property.init
            in ts_node let total (local ?? 0) + sum(children.contribution)
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
            in ts_function_like let defaulted_params { value parameter.nodes; filter (p) p.defaulted == true; count }
            in ts_function_like let all_named { value parameter.nodes; all (p) p.name != null }
            in ts_function_like let last_default { value parameter.nodes; map (p) p.defaulted; last } ?? false
            in ts_block let child_total sum(children.total)
            in ts_function_like rule params info "{defaulted_params}:{all_named}:{last_default}"
            in ts_function_like let shared { value parameter.nodes; defaulted: filter (p) p.defaulted; value parameter.nodes; all (q) count(defaulted) == 1 }
            in ts_function_like let staged { value parameter.nodes; filter (p, i) i > 0; map (p) p.defaulted; fold false (a, b) a || b }
            in ts_function_like let picked { value parameter.nodes; max_by (p) p.offset; then (p) p.name ?? "none" }
            in ts_function_like let firsts { value parameter.nodes; take 1; concat parameter.nodes; unique; count }
            in ts_function_like let label { value parameter.nodes; first; then (p) p.name; nonempty } ?? "anon"
            in ts_function_like rule stages info "{shared}:{staged}:{picked}:{firsts}:{label}"
            in ts_block rule block_total info "{child_total}:{count(children.total)}:{first(children.depth)}"
            in ts_assignment rule left info "{left_named}:{operator == '??='}:{parent.depth}"
            in ts_node rule gated {
                let twice_total twice(total)
                when { value twice_total; then (t) t > 2 && !boundary }
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

}
