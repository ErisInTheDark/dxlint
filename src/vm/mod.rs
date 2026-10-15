//! Typed expressions lowered at config load, with packed per-file
//! attribute cells. Records and lists use config constants or evaluation
//! scratch; only attribute escapes become persistent file payloads.

use crate::config::ast::BinOp;
use crate::expr::functions::Function;
use crate::expr::{Builtin, CExpr, Each, FactOf, IsTest, NodeSel, Position, Relative, Value, MAX_CALL_DEPTH};
use crate::grammar::{FieldOne, Grammar, Tree, UserFn, ROOT, TEXT};
use std::sync::OnceLock;

mod records;
mod values;
pub use records::{Begin, FileVm};
pub use values::Config as Values;

/// A VM value. Config text is interned; native text belongs to the file.
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
    /// Text in a VM text pool: the file's, or evaluation scratch
    /// ([`values::SCRATCH_TEXT`]). Source substrings stay borrows.
    Text(u32),
    List(values::Handle),
    Record(u16, values::Handle),
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
    pub fn to_value<'s>(self, h: &impl Host, source: &'s str) -> Value<'s> {
        match self {
            V::List(_) | V::Record(..) | V::Text(_) => with_view(h, &values::Arena::default(), |view| view.value(self, h.tree(), source)),
            scalar => scalar.value(h.tree(), source),
        }
    }

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
            V::List(_) | V::Record(..) | V::Text(_) => unreachable!("file values need their arena"),
        }
    }

    /// [`Value::num`] for a scalar.
    fn num(self, tree: &Tree, source: &str) -> Option<f64> {
        match self {
            V::Null => Some(0.0),
            V::Bool(b) => Some(f64::from(u8::from(b))),
            V::Num(n) => Some(n),
            V::List(_) | V::Record(..) | V::Text(_) => None,
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
            V::List(_) | V::Record(..) | V::Text(_) => unreachable!("file values need their arena"),
        }
    }

}

/// A lowered expression, evaluated on one node.
#[derive(Clone, Debug)]
pub enum VExpr {
    Const(V),
    List(Box<[VExpr]>),
    Record(u16, Box<[VExpr]>),
    Key(Box<VExpr>, Box<str>),
    Member(Box<VExpr>, Box<str>, Option<Box<VExpr>>),
    Path(Box<VExpr>, Box<[Box<str>]>, Box<[Option<VExpr>]>),
    Collect(Box<Seq>),
    /// The node, or its parent layer node, is one of these matchers.
    Is { parent: bool, kinds: Box<[u32]> },
    /// The node fills this field of one of these matchers.
    InField(Box<[u32]>, u16),
    /// The node fills one of these fields (each of its matchers): one
    /// label and owner read for a whole `is (a.x, b.y, ...)`.
    InFields(Box<[(u16, Box<[u32]>)]>),
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
    Index,
    Line,
    /// A field's text: null, one node's text, or a list.
    Field(u16),
    /// `inner` on the single node of a field (none: null).
    OnField(u16, Box<VExpr>),
    OnParent(Box<VExpr>),
    OnSibling(bool, Box<VExpr>),
    /// A value's kind: its node's matcher, or a kind named by its text.
    IsValue(Box<VExpr>, Box<[u32]>),
    /// The first value when it is a node, else the second.
    Coalesce(Box<VExpr>, Box<VExpr>),
    Nullish(Box<VExpr>, Box<VExpr>),
    /// `contains(haystack, needle)` on text, or membership in constants.
    Contains(Box<VExpr>, Box<VExpr>),
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
    /// A native fact read.
    Fact { fact: usize, of: FactOf },
    FactCall { fact: usize, args: Box<[VExpr]>, receiver: Option<Box<VExpr>> },
    FieldElse(u16, Box<VExpr>),
    GroupBy { of: Box<Seq>, key: Box<VExpr>, bind: Binder, shape: u16 },
    Partition { of: Box<Seq>, test: Box<VExpr>, bind: Binder, shape: u16 },
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
    /// `min_by`/`max_by`; the first item wins ties.
    Pick { max: bool, of: Box<Seq>, key: Box<VExpr>, bind: Binder },
    /// A value-only built-in function on its arguments.
    Call(&'static Function, Box<[VExpr]>),
    /// A built-in variable read by the host (`indent`, `before`, ...).
    Builtin(Builtin),
    Option(usize),
    Debug(Box<VExpr>, Box<str>),
    /// `position(offset)`: one of [`Position::KEYS`], or the whole record.
    Position(Box<VExpr>, PositionRead),
    /// `get(record, key)` with a computed key.
    Get(Box<VExpr>, Box<VExpr>),
    /// `containing(sel)`: the innermost selected node holding this one.
    Containing(NodeSel),
    /// `node_at(offset)`: the innermost default-layer node holding it.
    NodeAt(Box<VExpr>),
    Token(crate::expr::TokenRead, Box<VExpr>, Option<Box<VExpr>>),
    /// `opener.x` / `closer.x`: on the first (last) child of these layers.
    OwnChild(bool, Box<[u32]>, Box<VExpr>),
    /// `matcher.x`: on the closest ancestor of that matcher.
    OnAncestor(u32, Box<VExpr>),
    /// `kind_name(n)`.
    KindName(Box<VExpr>),
    /// A template: its parts' texts joined.
    Template(Box<[VExpr]>),
    /// A rule's own `let`, by slot: computed once per rule run
    /// ([`Session`]), on the rule's node.
    Local(u16),
    /// `value is a, b.field, layer l, .lang`, every test form.
    IsTests(Box<VExpr>, Box<[IsTest]>),
    Trait(Box<VExpr>, crate::grammar::names::Value, &'static crate::expr::queries::Query, u64),
    QueryCall(&'static crate::expr::queries::QueryFunction, Box<[VExpr]>),
    /// A pattern test (`x ~ pattern`): the [`CExpr::Test`] or
    /// [`CExpr::TestDfa`] node, its matcher shared.
    Pattern(Box<VExpr>, Box<CExpr>),
    /// The closest ancestor of a matcher: its text, or one of its fields.
    Ancestor(u32, Option<u16>),
    /// A config `fn` on its arguments: its body is lowered once per config
    /// and runs in a frame of its own.
    CallFn(u32, Box<[VExpr]>),
    /// A call's body with its arguments in place ([`inlinable`]); like
    /// [`VExpr::CallFn`] it gives up past [`MAX_CALL_DEPTH`].
    Inlined(u32, Box<VExpr>, Box<[VExpr]>),
}

/// What `position(offset)` reads.
#[derive(Clone, Copy, Debug)]
pub enum PositionRead {
    /// One key, by index in [`Position::KEYS`].
    Key(u8),
    /// The record, in this config shape.
    Whole(u16),
}

impl VExpr {
    /// Small value-only bodies cost less than a second per-node cache.
    pub(crate) fn is_direct(&self) -> bool {
        fn cost(e: &VExpr, remaining: &mut u8) -> bool {
            let Some(left) = remaining.checked_sub(1) else { return false };
            *remaining = left;
            match e {
                VExpr::Const(_) | VExpr::Node | VExpr::Text | VExpr::Parent | VExpr::Offset | VExpr::EndOffset
                | VExpr::Index | VExpr::Line | VExpr::FieldPresent(_) | VExpr::Is { .. } | VExpr::InField(..) | VExpr::InFields(_) => true,
                VExpr::Not(a) | VExpr::Neg(a) | VExpr::IsNull(a) | VExpr::NonEmpty(a) => cost(a, remaining),
                VExpr::Binary(_, a, b) | VExpr::Coalesce(a, b) => cost(a, remaining) && cost(b, remaining),
                VExpr::If(a, b, c) => cost(a, remaining) && cost(b, remaining) && cost(c, remaining),
                _ => false,
            }
        }
        cost(self, &mut 8)
    }
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
    Value(Box<VExpr>),
    /// An item stage, its lambda run on each item.
    Stage(SeqStage, Box<Seq>, Box<VExpr>, Binder),
    /// `flat_map`: each item's body is itself a sequence.
    FlatMap(Box<Seq>, Box<Seq>, Binder),
    /// `sort_by`: items ordered by their keys (stable).
    SortBy(Box<Seq>, Box<VExpr>, Binder),
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
    /// `overlapping(sel)` / `overlapping(sel, start, end)`: selected nodes
    /// overlapping this node (or the range), in source order.
    Overlapping(NodeSel, Option<Box<(VExpr, VExpr)>>),
    /// `descendants(sel)`: selected nodes under this one, in source order.
    Descendants(NodeSel),
    /// `walk(roots, n => next)`: every node reached, breadth first, once;
    /// the node binds at the slot for the body.
    Walk(Box<Seq>, Box<Seq>, u16),
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

/// What lowering reads: the config's `fn`s, and for rule conditions the
/// rule's `let`s.
struct Scope<'a, 'v> {
    functions: &'a [UserFn],
    locals: &'a [CExpr],
    values: &'v std::cell::RefCell<&'v mut Values>,
}

/// Whether `fn` `body` inlined at a call ([`VExpr::Inlined`]) evaluates just
/// what the call does: it calls no `fn` itself (so arguments and body run at
/// the call's depth either way), and reads each of its `arity` parameters
/// exactly once where it always evaluates it (not in a branch, a
/// short-circuit's right side or a lambda). Then an argument stays a view
/// (`children.x` is walked, not collected) and no frame is built.
fn inlinable(body: &CExpr, arity: u16) -> bool {
    fn count(e: &CExpr, eager: bool, uses: &mut [u8]) -> bool {
        match e {
            CExpr::UserCall(..) => return false,
            CExpr::Param(d) => {
                return match uses.get_mut(usize::from(*d)) {
                    Some(n) => {
                        *n = n.saturating_add(1);
                        eager && *n == 1
                    }
                    None => true,
                };
            }
            _ => {}
        }
        // Children from this index on are evaluated conditionally or repeatedly.
        let lazy_from = match e {
            CExpr::If(..) | CExpr::Binary(BinOp::And | BinOp::Or, ..) | CExpr::Nullish(..) | CExpr::Coalesce(..) | CExpr::Walk(..) => 1,
            CExpr::Each(stage) => 1 + stage.args.len(),
            _ => usize::MAX,
        };
        let (mut ok, mut at) = (true, 0);
        e.for_each_child(&mut |child| {
            ok &= count(child, eager && at < lazy_from, uses);
            at += 1;
        });
        ok
    }
    let mut uses = vec![0u8; usize::from(arity)];
    count(body, true, &mut uses) && uses.iter().all(|&n| n == 1)
}

/// The typed form of an attribute definition.
pub fn lower(e: &CExpr, functions: &[UserFn], values: &mut Values) -> VExpr {
    let values = std::cell::RefCell::new(values);
    scalar(e, &Scope { functions, locals: &[], values: &values }, &[], 0)
}

/// The typed form of a rule condition, `let` or clause; the rule's `let`s
/// are read from their [`Session`] slots. With `item`, the rule's `each`
/// item is frame slot 0 (its lambdas count from 1).
pub fn lower_condition(e: &CExpr, functions: &[UserFn], locals: &[CExpr], values: &mut Values, item: bool) -> VExpr {
    let values = std::cell::RefCell::new(values);
    let args: &[Lowered] = if item { &[Lowered::V(VExpr::Bound(0))] } else { &[] };
    scalar(e, &Scope { functions, locals, values: &values }, args, u16::from(item))
}

fn scalar(e: &CExpr, g: &Scope<'_, '_>, args: &[Lowered], bound: u16) -> VExpr {
    match lower_in(e, g, args, bound) {
        Ok(Lowered::V(v)) => v,
        Ok(Lowered::Seq(s)) => VExpr::Collect(Box::new(s)),
        _ => invalid(e, g),
    }
}

/// `e` as a list the VM can walk.
fn seq(e: &CExpr, g: &Scope<'_, '_>, args: &[Lowered], bound: u16) -> Seq {
    match lower_in(e, g, args, bound) {
        Ok(Lowered::Seq(s)) => s,
        Ok(Lowered::V(VExpr::Field(label))) => Seq::Nodes { children: false, label, item: Box::new(VExpr::Text) },
        Ok(Lowered::V(VExpr::OnField(label, item))) => Seq::Nodes { children: false, label, item },
        Ok(Lowered::V(v)) => Seq::Value(Box::new(v)),
        _ => Seq::Value(Box::new(invalid(e, g))),
    }
}

fn invalid(e: &CExpr, g: &Scope<'_, '_>) -> VExpr {
    g.values.borrow_mut().lowering_errors.push(format!("expression cannot lower to the VM: {e:?}"));
    VExpr::Const(V::Null)
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

/// `position(offset).key`: the offset and the key's index.
fn position_key<'a>(base: &'a CExpr, key: &str) -> Option<(&'a CExpr, u8)> {
    let CExpr::Position(offset) = base else { return None };
    Some((offset, Position::KEYS.iter().position(|k| *k == key)? as u8))
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

fn lower_in<'a>(e: &'a CExpr, g: &Scope<'a, '_>, args: &[Lowered], bound: u16) -> Result<Lowered, &'a CExpr> {
    let boxed = |e: &'a CExpr| -> Result<Box<VExpr>, &'a CExpr> { Ok(Box::new(scalar(e, g, args, bound))) };
    let boxed_seq = |e: &'a CExpr| -> Result<Box<Seq>, &'a CExpr> { Ok(Box::new(seq(e, g, args, bound))) };
    let all = |items: &'a [CExpr]| -> Box<[VExpr]> { items.iter().map(|v| scalar(v, g, args, bound)).collect() };
    Ok(Lowered::V(match e {
        CExpr::Const(v) => VExpr::Const(g.values.borrow_mut().constant(v).ok_or(e)?),
        CExpr::List(items) => VExpr::List(all(items)),
        CExpr::Record(entries) => {
            let shape = g.values.borrow_mut().shape(entries.iter().map(|(key, _)| key.clone())).ok_or(e)?;
            VExpr::Record(shape, entries.iter().map(|(_, v)| scalar(v, g, args, bound)).collect())
        }
        // `position(x).line`: only the key read, no record built.
        CExpr::Key(base, key) if position_key(base, key).is_some() => {
            let (offset, at) = position_key(base, key).expect("matched");
            VExpr::Position(boxed(offset)?, PositionRead::Key(at))
        }
        CExpr::Member(base, key, _) if position_key(base, key).is_some() => {
            let (offset, at) = position_key(base, key).expect("matched");
            VExpr::Position(boxed(offset)?, PositionRead::Key(at))
        }
        CExpr::Key(base, key) => VExpr::Key(boxed(base)?, key.clone()),
        CExpr::Member(base, key, part) => VExpr::Member(boxed(base)?, key.to_string().into(),
            part.as_deref().map(boxed).transpose()?),
        CExpr::Path(base, keys, parts) => VExpr::Path(boxed(base)?, keys.iter().map(|k| k.to_string().into()).collect(),
            parts.iter().map(|part| part.as_ref().map(|e| scalar(e, g, args, bound))).collect()),
        CExpr::Hole(inner, _) => return lower_in(inner, g, args, bound),
        CExpr::Param(d) => match args.get(*d as usize) {
            Some(Lowered::Unbound) | None => return Err(e),
            Some(lowered) => return Ok(lowered.clone()),
        },
        CExpr::Attr(a) => VExpr::Attr(*a),
        CExpr::ParentAttr(a) => VExpr::ParentAttr(*a),
        CExpr::Fact(fact, of) => {
            g.values.borrow_mut().register_fact(*fact).ok_or(e)?;
            VExpr::Fact { fact: *fact, of: *of }
        }
        CExpr::FactCall(fact, call_args, receiver) => {
            g.values.borrow_mut().register_fact(*fact).ok_or(e)?;
            VExpr::FactCall { fact: *fact, args: all(call_args), receiver: receiver.as_deref().map(boxed).transpose()? }
        }
        CExpr::FieldElse(label, inner) => VExpr::FieldElse(*label, boxed(inner)?),
        CExpr::Relative(Relative::Children, inner) => {
            return Ok(Lowered::Seq(Seq::Nodes { children: true, label: 0, item: boxed(inner)? }));
        }
        CExpr::Relative(relative, inner) => VExpr::OnSibling(*relative == Relative::Next, boxed(inner)?),
        CExpr::Each(stage) => {
            let arity = u16::from(stage.arity);
            let inner = bind_params(args, stage.depth, arity, bound);
            let body = || -> Result<Box<VExpr>, &'a CExpr> { Ok(Box::new(scalar(&stage.body, g, &inner, bound + arity))) };
            let bind = Binder { slot: bound, indexed: stage.arity == 2 };
            let of = boxed_seq(&stage.input)?;
            return Ok(match (stage.kind, stage.args.as_slice()) {
                (Each::Fold, [init]) => Lowered::V(VExpr::Fold { of, init: Some(boxed(init)?), body: body()?, slot: bound }),
                (Each::Reduce, []) => Lowered::V(VExpr::Fold { of, init: None, body: body()?, slot: bound }),
                (Each::Zip, [other]) => Lowered::Seq(Seq::Zip(of, boxed_seq(other)?, body()?, bound)),
                (Each::FlatMap, []) => Lowered::Seq(Seq::FlatMap(of, Box::new(seq(&stage.body, g, &inner, bound + arity)), bind)),
                (Each::SortBy, []) => Lowered::Seq(Seq::SortBy(of, body()?, bind)),
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
                            Each::GroupBy => {
                                let shape = g.values.borrow_mut().shape(["key".into(), "items".into()].into_iter()).ok_or(e)?;
                                VExpr::GroupBy { of, key: body, bind, shape }
                            }
                            Each::Partition => {
                                let shape = g.values.borrow_mut().shape(["yes".into(), "no".into()].into_iter()).ok_or(e)?;
                                VExpr::Partition { of, test: body, bind, shape }
                            }
                            _ => return Err(e),
                        })
                    }
                }
                _ => return Err(e),
            });
        }
        CExpr::Bind(value, param, body) => {
            let mut inner = bind_params(args, *param, 0, bound);
            let value = lower_in(value, g, args, bound)?;
            return Ok(match value {
                // A sequence stays a view: walked again where it is used.
                Lowered::Seq(s) => {
                    inner.push(Lowered::Seq(s));
                    match lower_in(body, g, &inner, bound) {
                        Ok(lowered @ (Lowered::V(_) | Lowered::Seq(_))) => lowered,
                        _ => return Err(body),
                    }
                }
                Lowered::V(v) => {
                    inner.push(Lowered::V(VExpr::Bound(bound)));
                    match lower_in(body, g, &inner, bound + 1) {
                        Ok(Lowered::Seq(s)) => Lowered::Seq(Seq::Let(Box::new(v), bound, Box::new(s))),
                        Ok(Lowered::V(b)) => Lowered::V(VExpr::Let(Box::new(v), bound, Box::new(b))),
                        _ => return Err(body),
                    }
                }
                Lowered::Unbound => return Err(e),
            });
        }
        // `[] x` and `.nodes` remain list views, including pipeline seeds.
        CExpr::Binary(BinOp::Join, a, b) if matches!(&**a, CExpr::Const(Value::List(items)) if items.is_empty()) => {
            return Ok(Lowered::Seq(seq(b, g, args, bound)));
        }
        CExpr::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b) if null_test(a, b).is_some() => {
            let test = VExpr::IsNull(boxed(null_test(a, b).expect("matched"))?);
            if *op == BinOp::Eq { test } else { VExpr::Not(Box::new(test)) }
        }
        // Kind-only tests on a value, and kind/field tests on this node or
        // its parent, have direct forms; the rest test the value generally.
        CExpr::Is(subject, tests)
            if !matches!(&**subject, CExpr::Builtin(Builtin::Node | Builtin::Parent)) && tests.iter().all(|t| matches!(t, IsTest::Kind(_))) =>
        {
            let kinds: Vec<u32> = tests.iter().flat_map(|t| match t { IsTest::Kind(ids) => ids.clone(), _ => Vec::new() }).collect();
            VExpr::IsValue(boxed(subject)?, kinds.into())
        }
        CExpr::Is(subject, tests)
            if matches!(&**subject, CExpr::Builtin(Builtin::Node | Builtin::Parent))
                && tests.iter().all(|t| matches!(t, IsTest::Kind(_)) || matches!(t, IsTest::Field(..)) && matches!(&**subject, CExpr::Builtin(Builtin::Node))) =>
        {
            let parent = matches!(&**subject, CExpr::Builtin(Builtin::Parent));
            let kinds: Vec<u32> = tests.iter().flat_map(|t| match t { IsTest::Kind(ids) => ids.clone(), _ => Vec::new() }).collect();
            let fields: Vec<(u16, Box<[u32]>)> =
                tests.iter().filter_map(|t| match t { IsTest::Field(ids, label) => Some((*label, ids.clone().into())), _ => None }).collect();
            let kinds = (!kinds.is_empty()).then(|| VExpr::Is { parent, kinds: kinds.into() });
            let fields = match fields.len() {
                0 => None,
                1 => fields.into_iter().next().map(|(label, ids)| VExpr::InField(ids, label)),
                _ => Some(VExpr::InFields(fields.into())),
            };
            match (kinds, fields) {
                (Some(a), Some(b)) => VExpr::Binary(BinOp::Or, Box::new(a), Box::new(b)),
                (Some(one), None) | (None, Some(one)) => one,
                (None, None) => return Err(e),
            }
        }
        CExpr::Is(subject, tests) => VExpr::IsTests(boxed(subject)?, tests.clone().into()),
        CExpr::Trait(subject, quantifier, mask) => {
            let (subject, path) = match subject.as_ref() {
                CExpr::Semantic(subject, path) => (subject.as_ref(), path.clone()),
                subject => (subject, crate::grammar::names::Value::Path(Vec::new())),
            };
            VExpr::Trait(boxed(subject)?, path, crate::expr::queries::query(*quantifier), *mask)
        }
        CExpr::Semantic(..) => return Err(e),
        CExpr::QueryCall(query, call_args) => VExpr::QueryCall(query, all(call_args)),
        CExpr::Test(value, _) | CExpr::TestDfa(value, _) => VExpr::Pattern(boxed(value)?, Box::new(e.clone())),
        CExpr::Ancestor(matcher, field) => VExpr::Ancestor(*matcher, *field),
        CExpr::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b) if matches!((&**a, &**b), (CExpr::Field(_), CExpr::Const(Value::Null)) | (CExpr::Const(Value::Null), CExpr::Field(_))) => {
            let ((CExpr::Field(label), _) | (_, CExpr::Field(label))) = (&**a, &**b) else { return Err(e) };
            let present = VExpr::FieldPresent(*label);
            if *op == BinOp::Ne { present } else { VExpr::Not(Box::new(present)) }
        }
        CExpr::Binary(op, a, b) => VExpr::Binary(*op, boxed(a)?, boxed(b)?),
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
            // A list view reverses in place; text goes to the function.
            ("reverse", [list]) => match lower_in(list, g, args, bound)? {
                Lowered::Seq(s) => return Ok(Lowered::Seq(Seq::Reverse(Box::new(s)))),
                Lowered::V(v) => VExpr::Call(f, Box::new([v])),
                Lowered::Unbound => return Err(e),
            },
            ("get", [record, key]) => VExpr::Get(boxed(record)?, boxed(key)?),
            ("contains", [haystack, needle]) => VExpr::Contains(boxed(haystack)?, boxed(needle)?),
            ("re_test", [text, CExpr::Const(Value::Str(pattern))]) if crate::expr::re::compile(pattern).is_ok() => {
                VExpr::ReTest(boxed(text)?, intern(pattern).0)
            }
            ("starts_with", [text, prefix]) => VExpr::StartsWith(boxed(text)?, boxed(prefix)?),
            ("ends_with", [text, suffix]) => VExpr::EndsWith(boxed(text)?, boxed(suffix)?),
            _ => VExpr::Call(f, all(call_args)),
        },
        CExpr::Builtin(b) => match b {
            Builtin::Node => VExpr::Node,
            Builtin::Text => VExpr::Text,
            Builtin::Parent => VExpr::Parent,
            Builtin::Offset => VExpr::Offset,
            Builtin::EndOffset => VExpr::EndOffset,
            Builtin::Index => VExpr::Index,
            Builtin::Line => VExpr::Line,
            other => VExpr::Builtin(*other),
        },
        CExpr::Opt(index) => VExpr::Option(*index),
        CExpr::Dbg(inner, label) => VExpr::Debug(boxed(inner)?, label.clone().into()),
        CExpr::Position(offset) => {
            let shape = g.values.borrow_mut().shape(Position::KEYS.iter().map(|&key| key.into())).ok_or(e)?;
            VExpr::Position(boxed(offset)?, PositionRead::Whole(shape))
        }
        CExpr::Containing(sel) => VExpr::Containing(sel.clone()),
        CExpr::NodeAt(offset) => VExpr::NodeAt(boxed(offset)?),
        CExpr::Token(read, inner, pairs) => VExpr::Token(*read, boxed(inner)?, match pairs {
            Some(pairs) => Some(boxed(pairs)?),
            None => None,
        }),
        CExpr::OwnChild(last, kinds, inner) => VExpr::OwnChild(*last, kinds.clone().into(), boxed(inner)?),
        CExpr::OnAncestor(matcher, inner) => VExpr::OnAncestor(*matcher, boxed(inner)?),
        CExpr::KindName(inner) => VExpr::KindName(boxed(inner)?),
        CExpr::Template(parts) => VExpr::Template(all(parts)),
        CExpr::Overlapping(sel, range) => {
            let range = range.as_deref().map(|(from, to)| Box::new((scalar(from, g, args, bound), scalar(to, g, args, bound))));
            return Ok(Lowered::Seq(Seq::Overlapping(sel.clone(), range)));
        }
        CExpr::Descendants(sel) => return Ok(Lowered::Seq(Seq::Descendants(sel.clone()))),
        CExpr::Walk(roots, param, body) => {
            let inner = bind_params(args, *param, 1, bound);
            let next = seq(body, g, &inner, bound + 1);
            return Ok(Lowered::Seq(Seq::Walk(boxed_seq(roots)?, Box::new(next), bound)));
        }
        CExpr::Field(label) => VExpr::Field(*label),
        CExpr::OnField(label, inner) => VExpr::OnField(*label, boxed(inner)?),
        CExpr::OnParent(inner) => VExpr::OnParent(boxed(inner)?),
        CExpr::FieldAttr(label, a) => VExpr::OnField(*label, Box::new(VExpr::Attr(*a))),
        CExpr::ParentField(label) => VExpr::OnParent(Box::new(VExpr::Field(*label))),
        CExpr::Coalesce(a, b) => VExpr::Coalesce(boxed(a)?, boxed(b)?),
        CExpr::Nullish(a, b) => VExpr::Nullish(boxed(a)?, boxed(b)?),
        CExpr::NonEmpty(value) => VExpr::NonEmpty(boxed(value)?),
        // A rule's `let`: its own slot in the rule's session.
        CExpr::Local(i) if (*i as usize) < g.locals.len() => VExpr::Local(*i),
        // Inlined only where that evaluates exactly what the call would
        // ([`inlinable`]): inlining a body that reads a parameter twice
        // copies the argument into every use, nested calls multiplying it.
        CExpr::UserCall(i, call_args) => {
            let f = g.functions.get(*i as usize).ok_or(e)?;
            let body = f.body.get().ok_or(e)?;
            let arity = u16::try_from(f.arity).map_err(|_| e)?;
            if call_args.len() == f.arity && inlinable(body, arity) {
                let lowered: Vec<Lowered> =
                    call_args.iter().map(|arg| lower_in(arg, g, args, bound)).collect::<Result<_, _>>()?;
                let inner = Scope { functions: g.functions, locals: &[], values: g.values };
                // A body that is a list view stays a call: inlined, it would
                // escape the depth check.
                match lower_in(body, &inner, &lowered, bound) {
                    Ok(Lowered::V(v)) => return Ok(Lowered::V(VExpr::Inlined(*i, Box::new(v), all(call_args)))),
                    Err(e) => return Err(e),
                    Ok(_) => {}
                }
            }
            let begun = g.values.borrow_mut().begin_function(*i);
            if begun {
                let start = g.values.borrow().lowering_errors.len();
                let params: Vec<Lowered> = (0..arity).map(|p| Lowered::V(VExpr::Bound(p))).collect();
                let lowered = scalar(body, &Scope { functions: g.functions, locals: &[], values: g.values }, &params, arity);
                let mut values = g.values.borrow_mut();
                if values.lowering_errors.len() > start {
                    values.reject_function(*i, &f.name, start);
                } else {
                    values.set_function(*i, lowered);
                }
            } else {
                let mut values = g.values.borrow_mut();
                if let Some(error) = values.function_error(*i).map(str::to_owned) {
                    values.lowering_errors.push(error);
                }
            }
            VExpr::CallFn(*i, all(call_args))
        }
        _ => return Err(e),
    }))
}

/// What evaluation needs from its surroundings.
pub trait Host {
    fn tree(&self) -> &Tree;
    fn grammar(&self) -> &Grammar;
    fn source(&self) -> &str;
    fn lines(&self) -> &crate::line_index::LineIndex;
    /// An attribute on a node through the VM; `None` gives up on the VM.
    fn attr(&self, node: u32, attr: u16) -> Option<V>;
    /// A native fact read on a node, when its value is one the VM holds.
    fn fact(&self, node: u32, fact: usize, of: FactOf) -> Option<V>;
    fn fact_call(&self, _node: u32, _fact: usize, _args: Vec<Value<'static>>) -> Option<V> { None }
    fn facts_available(&self) -> bool { true }
    fn local(&self, _node: u32, _local: u16) -> Option<V> { None }
    fn values(&self) -> &Values;
    fn file_vm(&self) -> Option<&FileVm> { None }
    /// A built-in variable on a node.
    fn builtin(&self, _node: u32, _builtin: Builtin) -> Option<Value<'_>> { None }
    fn query(&self, _node: u32, _subject: &crate::grammar::names::Value, _query: &crate::expr::queries::Query, _mask: u64) -> Option<bool> { None }
    fn query_function(&self, _node: u32, _query: &crate::expr::queries::QueryFunction, _args: &[Value<'static>]) -> Option<Value<'static>> { None }
    fn query_handle(&self, _handle: crate::grammar::names::question::Handle, _query: &crate::expr::queries::HandleFunction, _args: &[Value<'static>]) -> Option<Value<'static>> { None }
    fn option(&self, _index: usize) -> Option<Value<'_>> { None }
    fn debug(&self, _node: u32, _label: &str, _value: &Value) {}
    /// `position(offset)`.
    fn position(&self, _offset: f64) -> Option<Position> { None }
    /// `overlapping(sel)` from a node, over the node or clamped offsets.
    fn overlapping(&self, _node: u32, _sel: &NodeSel, _range: Option<(f64, f64)>) -> Option<Vec<u32>> { None }
    /// `containing(sel)` from a node.
    fn containing(&self, _node: u32, _sel: &NodeSel) -> Option<Option<u32>> { None }
    /// `descendants(sel)` under `node`, in source order.
    fn descendants<'s>(&'s self, node: u32, sel: &NodeSel) -> impl Iterator<Item = u32> + 's {
        sel.descendants(self.tree(), node)
    }
    /// The file's language, for `is .lang`.
    fn lang(&self) -> usize { 0 }
    /// The front end's code tokens for `token(...)`; none without one.
    fn tokens(&self) -> Option<std::rc::Rc<crate::engine::frontend::Tokens>> { None }
    /// Whether `text` passes a pattern test ([`CExpr::Test`]/[`CExpr::TestDfa`]).
    fn pattern(&self, _test: &CExpr, _text: &str) -> Option<bool> { None }
}

/// `ZLINT_VM_STATS`: count delegated runs per site.
pub fn stats() -> bool {
    static STATS: OnceLock<bool> = OnceLock::new();
    *STATS.get_or_init(|| std::env::var_os("ZLINT_VM_STATS").is_some())
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

/// An evaluated value and the scratch holding its temporary payloads.
pub struct Evaluated {
    value: V,
    scratch: values::Arena,
}

fn with_view<R>(h: &impl Host, scratch: &values::Arena, run: impl FnOnce(&values::View<'_>) -> R) -> R {
    let file = h.file_vm().map(|vm| vm.values.borrow());
    let empty = values::Arena::default();
    run(&values::View { config: h.values(), scratch, file: file.as_deref().unwrap_or(&empty) })
}

impl Evaluated {
    #[cfg(test)]
    pub fn truthy(&self, h: &impl Host) -> bool {
        match self.value {
            V::List(_) | V::Record(..) | V::Text(_) => with_view(h, &self.scratch, |view| view.truthy(self.value, h.tree())),
            scalar => scalar.truthy(h.tree()),
        }
    }
    pub fn value<'s>(&self, h: &impl Host, source: &'s str) -> Value<'s> {
        with_view(h, &self.scratch, |view| view.value(self.value, h.tree(), source))
    }
    pub fn persist(self, file: &FileVm) -> Option<V> {
        match self.value {
            V::List(handle) | V::Record(_, handle) if handle.is_scratch() => {
                file.packing(self.scratch.try_promote(self.value, &mut file.values.borrow_mut()))
            }
            V::Text(handle) if handle & values::SCRATCH_TEXT != 0 => file.packing(self.scratch.try_promote(self.value, &mut file.values.borrow_mut())),
            value => Some(value),
        }
    }
}

struct Runtime<'h, H> {
    host: &'h H,
    scratch: std::cell::RefCell<values::Arena>,
    /// The rule's node, where its `let`s are read.
    home: u32,
    locals: &'h [VExpr],
    local_values: std::cell::RefCell<Vec<LocalSlot>>,
    /// Frames of finished `fn` calls, reused; and how deep calls are.
    frames: std::cell::RefCell<Vec<Vec<V>>>,
    calls: std::cell::Cell<u32>,
}

#[derive(Clone)]
enum LocalSlot {
    Unset,
    Busy,
    Failed(Box<str>),
    Ready(V),
}

/// One rule run on one node: its conditions share evaluation scratch and
/// each lowered `let`, computed at most once. Its buffers come from (and
/// return to) the file's pool, so runs allocate only while they grow.
pub struct Session<'h, H: Host> {
    runtime: Runtime<'h, H>,
    /// The evaluation frame, reused across this run's clauses.
    frame: std::cell::RefCell<Vec<V>>,
}

/// A finished run's buffers, kept by the file for the next run.
#[derive(Default)]
pub(crate) struct Spare {
    scratch: values::Arena,
    local_values: Vec<LocalSlot>,
    frames: Vec<Vec<V>>,
    frame: Vec<V>,
}

impl<'h, H: Host> Session<'h, H> {
    pub fn new(host: &'h H, node: u32, locals: &'h [VExpr]) -> Self {
        let spare = host.file_vm().and_then(|vm| vm.spare.borrow_mut().pop()).unwrap_or_default();
        let mut runtime = Runtime::new(host, node, locals);
        runtime.scratch = std::cell::RefCell::new(spare.scratch);
        runtime.local_values = std::cell::RefCell::new(spare.local_values);
        runtime.frames = std::cell::RefCell::new(spare.frames);
        Session { runtime, frame: std::cell::RefCell::new(spare.frame) }
    }

    /// `e` on the rule's node with the rule's `each` item (if any) in frame
    /// slot 0.
    fn run(&self, e: &VExpr, item: Option<V>) -> Option<V> {
        if let Some(vm) = self.runtime.host.file_vm() { vm.clear_error(); }
        let mut f = self.frame.take();
        f.clear();
        f.extend(item);
        let v = ev(&self.runtime, self.runtime.home, e, &mut f);
        self.frame.replace(f);
        v
    }

    /// Whether `e` holds on the rule's node, with the rule's `each` item (if
    /// any) in frame slot 0.
    pub fn holds(&self, e: &VExpr, item: Option<V>) -> Option<bool> {
        let v = self.run(e, item)?;
        Some(self.runtime.truthy(v))
    }

    /// Whether `e` holds on `node`, for one session serving many nodes
    /// (name conditions): its scratch is cleared after each.
    pub fn holds_on(&self, node: u32, e: &VExpr) -> Option<bool> {
        if let Some(vm) = self.runtime.host.file_vm() { vm.clear_error(); }
        let mut f = self.frame.take();
        f.clear();
        let held = ev(&self.runtime, node, e, &mut f).map(|v| self.runtime.truthy(v));
        self.frame.replace(f);
        self.runtime.scratch.borrow_mut().clear();
        held
    }

    pub fn value_on(&self, node: u32, e: &VExpr, item: Option<u32>) -> Option<Value<'static>> {
        if let Some(vm) = self.runtime.host.file_vm() { vm.clear_error(); }
        let mut frame = self.frame.take();
        frame.clear();
        frame.extend(item.map(V::Node));
        let value = ev(&self.runtime, node, e, &mut frame).map(|value| self.runtime.unpack(value).into_owned());
        self.frame.replace(frame);
        self.runtime.scratch.borrow_mut().clear();
        value
    }

    /// `e`'s value on the rule's node.
    pub fn value(&self, e: &VExpr, item: Option<V>) -> Option<Value<'_>> {
        let v = self.run(e, item)?;
        Some(self.runtime.unpack(v))
    }

    fn import_item(&self, item: &Value) -> Option<V> {
        if let Some(vm) = self.runtime.host.file_vm() { vm.clear_error(); }
        self.runtime.pack(item)
    }

    pub fn holds_with(&self, e: &VExpr, item: &Value) -> Option<bool> {
        self.holds(e, Some(self.import_item(item)?))
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "consumed by module resolution modes (units layer)"))]
    pub fn value_with(&self, e: &VExpr, item: &Value) -> Option<Value<'_>> {
        self.value(e, Some(self.import_item(item)?))
    }

    /// The items of the rule's `each` list.
    pub fn items(&self, e: &VExpr) -> Option<Vec<V>> {
        let v = self.run(e, None)?;
        Some(self.runtime.items(v))
    }

    pub fn error(&self) -> String {
        self.runtime.host.file_vm().map_or_else(|| "expression evaluation failed".into(), FileVm::error)
    }

}

impl<H: Host> Drop for Session<'_, H> {
    fn drop(&mut self) {
        let Some(vm) = self.runtime.host.file_vm() else { return };
        let mut spare = Spare {
            scratch: self.runtime.scratch.take(),
            local_values: self.runtime.local_values.take(),
            frames: self.runtime.frames.take(),
            frame: self.frame.take(),
        };
        spare.scratch.clear();
        spare.local_values.clear();
        vm.spare.borrow_mut().push(spare);
    }
}

impl<H> std::ops::Deref for Runtime<'_, H> {
    type Target = H;
    fn deref(&self) -> &H { self.host }
}

impl<'h, H: Host> Runtime<'h, H> {
    fn fail<T>(&self, error: String) -> Option<T> {
        if let Some(vm) = self.host.file_vm() { vm.fail(error); }
        None
    }

    fn result<T>(&self, result: Result<T, String>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => self.fail(error),
        }
    }

    fn new(host: &'h H, home: u32, locals: &'h [VExpr]) -> Self {
        Runtime {
            host,
            scratch: Default::default(),
            home,
            locals,
            // Most rule runs read no lowered `let`: no allocation until one does.
            local_values: Default::default(),
            frames: Default::default(),
            calls: Default::default(),
        }
    }

    /// `fn` `i` on `args`, bounded by [`MAX_CALL_DEPTH`].
    fn call(&self, node: u32, i: u32, args: &[VExpr], f: &mut Vec<V>) -> Option<V> {
        let body = self.values().function(i)?;
        let mut frame = self.frames.borrow_mut().pop().unwrap_or_default();
        frame.clear();
        let mut result = None;
        if args.iter().try_for_each(|arg| { frame.push(ev(self, node, arg, f)?); Some(()) }).is_some() {
            if self.calls.get() >= MAX_CALL_DEPTH {
                result = self.fail(format!("`{}` calls itself too deeply", self.grammar().functions[i as usize].name));
            } else {
                self.calls.set(self.calls.get() + 1);
                result = ev(self, node, body, &mut frame);
                self.calls.set(self.calls.get() - 1);
            }
        }
        self.frames.borrow_mut().push(frame);
        result
    }

    /// A rule `let`, computed on first read.
    fn local(&self, local: u16) -> Option<V> {
        let at = local as usize;
        let body = self.locals.get(at)?;
        {
            let mut slots = self.local_values.borrow_mut();
            if slots.is_empty() {
                slots.resize(self.locals.len(), LocalSlot::Unset);
            }
            match &slots[at] {
                LocalSlot::Ready(v) => return Some(*v),
                LocalSlot::Busy => return self.fail("a rule's `let` depends on itself".into()),
                LocalSlot::Failed(error) => return self.fail(error.to_string()),
                LocalSlot::Unset => {}
            }
        }
        self.local_values.borrow_mut()[at] = LocalSlot::Busy;
        let v = ev(self, self.home, body, &mut Vec::new());
        self.local_values.borrow_mut()[at] = v.map_or_else(|| LocalSlot::Failed(
            self.host.file_vm().map_or_else(|| "expression evaluation failed".into(), FileVm::error).into()
        ), LocalSlot::Ready);
        v
    }

    fn truthy(&self, value: V) -> bool {
        match value {
            V::List(_) | V::Record(..) | V::Text(_) => with_view(self.host, &self.scratch.borrow(), |view| view.truthy(value, self.tree())),
            scalar => scalar.truthy(self.tree()),
        }
    }
    fn equal(&self, a: V, b: V) -> bool {
        match (a, b) {
            (V::List(_) | V::Record(..) | V::Text(_), _) | (_, V::List(_) | V::Record(..) | V::Text(_)) => {
                with_view(self.host, &self.scratch.borrow(), |view| view.equal(a, b, self.tree(), self.source()))
            }
            (a, b) => a.loose_eq(b, self.tree(), self.source()),
        }
    }
    fn num(&self, value: V) -> Option<f64> {
        let result = match value {
            V::Text(_) => self.with_text(value, |text| text.trim().parse().ok()),
            scalar => scalar.num(self.tree(), self.source()),
        };
        if result.is_none() {
            return self.result(self.unpack(value).num());
        }
        result
    }
    fn with_text<R>(&self, value: V, run: impl FnOnce(&str) -> Option<R>) -> Option<R> {
        match value {
            V::Text(_) => with_view(self.host, &self.scratch.borrow(), |view| run(view.text(value, self.tree(), self.source())?)),
            V::Null | V::Bool(_) | V::Num(_) => run(&scalar_text(value)),
            V::List(_) | V::Record(..) => run(&self.unpack(value).text()),
            scalar => run(scalar.text(self.tree(), self.source())?),
        }
    }
    fn text_pair<R>(&self, a: V, b: V, run: impl FnOnce(&str, &str) -> R) -> Option<R> {
        if matches!(a, V::List(_) | V::Record(..)) || matches!(b, V::List(_) | V::Record(..)) {
            return Some(run(&self.unpack(a).text(), &self.unpack(b).text()));
        }
        if matches!(a, V::Null | V::Bool(_) | V::Num(_)) {
            let a = scalar_text(a);
            return self.with_text(b, |b| Some(run(&a, b)));
        }
        if matches!(b, V::Null | V::Bool(_) | V::Num(_)) {
            let b = scalar_text(b);
            return self.with_text(a, |a| Some(run(a, &b)));
        }
        if !matches!(a, V::Text(_)) && !matches!(b, V::Text(_)) {
            return Some(run(a.text(self.tree(), self.source())?, b.text(self.tree(), self.source())?));
        }
        with_view(self.host, &self.scratch.borrow(), |view| {
            Some(run(view.text(a, self.tree(), self.source())?, view.text(b, self.tree(), self.source())?))
        })
    }
    fn key(&self, value: V, key: &str) -> V {
        match value {
            V::Record(shape, handle) => with_view(self.host, &self.scratch.borrow(), |view| view.key(shape, handle, key)),
            _ => V::Null,
        }
    }
    fn list(&self, items: &[V]) -> Option<V> {
        Some(V::List(self.allocate(items)?))
    }
    fn allocate(&self, items: &[V]) -> Option<values::Handle> {
        self.result(self.scratch.borrow_mut().try_allocate(values::Region::Scratch, items).map_err(|error| error.to_string()))
    }
    /// A value for shared value functions.
    fn unpack(&self, value: V) -> Value<'_> {
        match value {
            V::List(_) | V::Record(..) | V::Text(_) => with_view(self.host, &self.scratch.borrow(), |view| view.value(value, self.tree(), self.source())),
            scalar => scalar.value(self.tree(), self.source()),
        }
    }
    /// Packs a value into evaluation scratch.
    fn pack(&self, value: &Value) -> Option<V> {
        self.result(self.values().try_value_into(value, &mut self.scratch.borrow_mut(), values::Region::Scratch, self.source()).map_err(|error| error.to_string()))
    }
    fn items(&self, value: V) -> Vec<V> {
        match value {
            V::Null => Vec::new(),
            V::List(handle) => with_view(self.host, &self.scratch.borrow(), |view| {
                (0..view.len(handle)).map(|at| view.item(handle, at)).collect()
            }),
            value => vec![value],
        }
    }
    fn project(&self, value: V, mut on: impl FnMut(V) -> Option<V>) -> Option<V> {
        if matches!(value, V::List(_)) {
            let items = self.items(value);
            if items.iter().any(|v| matches!(v, V::Node(_))) {
                return self.list(&items.into_iter().map(on).collect::<Option<Vec<_>>>()?);
            }
        }
        on(value)
    }
}

pub fn eval(h: &impl Host, node: u32, e: &VExpr) -> Option<Evaluated> {
    let runtime = Runtime::new(h, node, &[]);
    let value = ev(&runtime, node, e, &mut Vec::new())?;
    Some(Evaluated { value, scratch: runtime.scratch.into_inner() })
}

/// Calls `each` with the items of `s` until it returns `false`; whether
/// it ran to the end. `None`: the VM gives up.
fn walk_seq<H: Host>(h: &Runtime<'_, H>, node: u32, s: &Seq, f: &mut Vec<V>, each: &mut dyn FnMut(V, &mut Vec<V>) -> Option<bool>) -> Option<bool> {
    let tree = h.tree();
    match s {
        Seq::Value(value) => return walk_value(h, ev(h, node, value, f)?, f, each),
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
                return walk_value(h, v, f, each);
            }
            for n in nodes {
                let v = ev(h, n, item, f)?;
                if !each(v, f)? {
                    return Some(false);
                }
            }
        }
        Seq::Stage(stage, of, body, bind) => {
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
                    SeqStage::Filter => if h.truthy(call_bound(h, node, body, f, *bind, v, at)?) { each(v, f) } else { Some(true) },
                    SeqStage::TakeWhile => {
                        open = open && h.truthy(call_bound(h, node, body, f, *bind, v, at)?);
                        if open { each(v, f) } else { Some(true) }
                    }
                    SeqStage::SkipWhile => {
                        open = open && h.truthy(call_bound(h, node, body, f, *bind, v, at)?);
                        if open { Some(true) } else { each(v, f) }
                    }
                    SeqStage::UniqueBy => {
                        let key = call_bound(h, node, body, f, *bind, v, at)?;
                        if keys.iter().any(|&k| h.equal(k, key)) { return Some(true); }
                        keys.push(key);
                        each(v, f)
                    }
                }
            });
        }
        // Equal keys keep their original order.
        Seq::SortBy(of, body, bind) => {
            let mut keyed: Vec<(Value<'static>, V)> = Vec::new();
            let mut index = 0.0;
            walk_seq(h, node, of, f, &mut |v, f| {
                let key = call_bound(h, node, body, f, *bind, v, index)?;
                index += 1.0;
                keyed.push((h.unpack(key).into_static(), v));
                Some(true)
            })?;
            keyed.sort_by(|(a, _), (b, _)| a.compare(b));
            for (_, v) in keyed {
                if !each(v, f)? {
                    return Some(false);
                }
            }
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
                // Unpaired items still evaluate, so their errors propagate.
                let Some(&b) = others.get(index) else { return Some(true) };
                index += 1;
                let paired = with_bound(f, *slot, &[v, b], |f| ev(h, node, body, f))?;
                each(paired, f)
            });
        }
        Seq::Take(of, n) | Seq::Drop(of, n) => {
            let n = h.num(ev(h, node, n, f)?)?;
            let name = if matches!(s, Seq::Take(..)) { "take" } else { "drop" };
            let n = h.result(whole(n).ok_or_else(|| format!("{name}: expected a whole non-negative number, got {n}")))?;
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
            let mut seen: Vec<V> = Vec::new();
            return walk_seq(h, node, of, f, &mut |v, f| {
                if seen.iter().any(|&s| h.equal(s, v)) { return Some(true); }
                seen.push(v);
                each(v, f)
            });
        }
        Seq::Let(value, slot, body) => {
            let v = ev(h, node, value, f)?;
            return with_bound(f, *slot, &[v], |f| walk_seq(h, node, body, f, each));
        }
        Seq::Overlapping(sel, range) => {
            let range = match range.as_deref() {
                Some((from, to)) => {
                    let from = h.num(ev(h, node, from, f)?)?;
                    Some((from, h.num(ev(h, node, to, f)?)?))
                }
                None => None,
            };
            for id in h.overlapping(node, sel, range)? {
                if !each(V::Node(id), f)? {
                    return Some(false);
                }
            }
        }
        Seq::Descendants(sel) => {
            for id in h.descendants(node, sel) {
                if !each(V::Node(id), f)? {
                    return Some(false);
                }
            }
        }
        // The whole walk runs before any reached item is used.
        Seq::Walk(roots, next, slot) => {
            let mut queue = std::collections::VecDeque::new();
            walk_seq(h, node, roots, f, &mut |v, _| { queue.push_back(v); Some(true) })?;
            let mut seen_nodes = crate::fx_hash::FxHashSet::default();
            let mut seen_values: Vec<V> = Vec::new();
            let mut reached = Vec::new();
            while let Some(v) = queue.pop_front() {
                let fresh = match v {
                    V::Null => false,
                    V::Node(id) => seen_nodes.insert(id),
                    _ if seen_values.iter().any(|&s| h.equal(s, v)) => false,
                    _ => { seen_values.push(v); true }
                };
                if !fresh {
                    continue;
                }
                with_bound(f, *slot, &[v], |f| walk_seq(h, node, next, f, &mut |more, _| { queue.push_back(more); Some(true) }))?;
                reached.push(v);
            }
            for v in reached {
                if !each(v, f)? {
                    return Some(false);
                }
            }
        }
    }
    Some(true)
}

fn walk_value<H: Host>(h: &Runtime<'_, H>, value: V, f: &mut Vec<V>, each: &mut dyn FnMut(V, &mut Vec<V>) -> Option<bool>) -> Option<bool> {
    match value {
        V::Null => Some(true),
        V::List(handle) => {
            let len = with_view(h.host, &h.scratch.borrow(), |view| view.len(handle));
            for at in 0..len {
                let value = with_view(h.host, &h.scratch.borrow(), |view| view.item(handle, at));
                if !each(value, f)? { return Some(false); }
            }
            Some(true)
        }
        scalar => each(scalar, f),
    }
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
fn call_bound<H: Host>(h: &Runtime<'_, H>, node: u32, body: &VExpr, f: &mut Vec<V>, bind: Binder, item: V, index: f64) -> Option<V> {
    let values = [item, V::Num(index)];
    with_bound(f, bind.slot, &values[..bind.width()], |f| ev(h, node, body, f))
}

/// A count argument (`take`, `drop`): a whole non-negative number.
fn whole(n: f64) -> Option<usize> {
    (n.is_finite() && n >= 0.0 && n.fract() == 0.0).then_some(n as usize)
}

/// A scalar's text, as [`Value`]'s `Display` writes it.
fn scalar_text(value: V) -> String {
    match value {
        V::Bool(b) => b.to_string(),
        V::Num(n) => Value::Num(n).to_string(),
        _ => String::new(),
    }
}

/// Value functions on a list argument, read in the arena instead of copied
/// out (same results as the `FUNCTIONS` entries); `None`: not handled here.
fn list_call<H: Host>(h: &Runtime<'_, H>, name: &str, args: &[V]) -> Option<Option<V>> {
    let &[V::List(handle), ..] = args else { return None };
    let len = with_view(h.host, &h.scratch.borrow(), |view| view.len(handle));
    Some(match (name, args) {
        ("len", [_]) => Some(V::Num(len as f64)),
        ("slice", [_, from, rest @ ..]) if rest.len() <= 1 => (|| {
            let resolve = |i: f64| if i < 0.0 { len.saturating_sub((-i) as usize) } else { (i as usize).min(len) };
            let from = resolve(h.num(*from)?);
            let to = match rest { [to] => resolve(h.num(*to)?), _ => len }.max(from);
            let items: Vec<V> = with_view(h.host, &h.scratch.borrow(), |view| (from..to).map(|at| view.item(handle, at)).collect());
            h.list(&items)
        })(),
        ("index_of", [_, needle]) => {
            let items: Vec<V> = with_view(h.host, &h.scratch.borrow(), |view| (0..len).map(|at| view.item(handle, at)).collect());
            Some(V::Num(items.iter().position(|&item| h.equal(item, *needle)).map_or(-1.0, |at| at as f64)))
        }
        _ => return None,
    })
}

/// One expression on one node. Every nested expression is a call here, so
/// this holds only the common scalar forms: an arm needing lists, closures
/// or text would grow every call's frame. The rest is [`ev_rare`].
fn ev<H: Host>(h: &Runtime<'_, H>, node: u32, e: &VExpr, f: &mut Vec<V>) -> Option<V> {
    let tree = h.tree();
    Some(match e {
        VExpr::Const(v) => *v,
        VExpr::Bound(slot) => *f.get(*slot as usize)?,
        VExpr::Local(local) if node == h.home => h.local(*local)?,
        VExpr::Attr(a) => h.attr(node, *a)?,
        VExpr::ParentAttr(a) => match parent_layer(tree, node) {
            Some(p) => h.attr(p, *a)?,
            None => V::Null,
        },
        VExpr::Fact { fact, of } => h.fact(node, *fact, *of)?,
        VExpr::Inlined(_, body, _) if h.calls.get() < MAX_CALL_DEPTH => ev(h, node, body, f)?,
        VExpr::Inlined(i, _, args) => {
            for arg in args.iter() { ev(h, node, arg, f)?; }
            return h.fail(format!("`{}` calls itself too deeply", h.grammar().functions[*i as usize].name));
        }
        VExpr::Is { parent, kinds } => {
            let subject = if *parent { parent_layer(tree, node) } else { Some(node) };
            V::Bool(subject.is_some_and(|n| is_a(h.grammar(), tree.node(n).matcher, kinds)))
        }
        VExpr::InField(kinds, label) => V::Bool(
            tree.label(node) == Some(*label) && tree.owner(node).is_some_and(|o| is_a(h.grammar(), tree.node(o).matcher, kinds)),
        ),
        VExpr::InFields(fields) => V::Bool(tree.label(node).is_some_and(|label| {
            let owner = tree.owner(node).map(|o| tree.node(o).matcher);
            fields.iter().any(|(l, kinds)| *l == label && owner.is_some_and(|m| is_a(h.grammar(), m, kinds)))
        })),
        VExpr::FieldPresent(label) => V::Bool(tree.field_one(node, *label) != FieldOne::None),
        VExpr::FieldCount(label) => {
            let mut count = 0u32;
            tree.visit_fields(node, *label, &mut |_| {
                count += 1;
                true
            });
            V::Num(f64::from(count))
        }
        VExpr::Field(label) => match tree.field_one(node, *label) {
            FieldOne::None => V::Null,
            FieldOne::One(one) => V::NodeText(one),
            FieldOne::Many => return ev_rare(h, node, e, f),
        },
        VExpr::OnField(label, inner) => match tree.field_one(node, *label) {
            FieldOne::None => V::Null,
            FieldOne::One(one) => ev(h, one, inner, f)?,
            FieldOne::Many => return ev_rare(h, node, e, f),
        },
        VExpr::OnParent(inner) => match parent_layer(tree, node) {
            Some(p) => ev(h, p, inner, f)?,
            None => V::Null,
        },
        VExpr::OnSibling(after, inner) => match tree.field_sibling(node, *after) {
            Some(sibling) => ev(h, sibling, inner, f)?,
            None => V::Null,
        },
        VExpr::OnAncestor(matcher, inner) => match tree.ancestor(h.grammar(), node, *matcher) {
            Some(ancestor) => ev(h, ancestor, inner, f)?,
            None => V::Null,
        },
        VExpr::Not(inner) => V::Bool(!h.truthy(ev(h, node, inner, f)?)),
        VExpr::Neg(inner) => V::Num(-h.num(ev(h, node, inner, f)?)?),
        VExpr::If(c, a, b) => if h.truthy(ev(h, node, c, f)?) { ev(h, node, a, f)? } else { ev(h, node, b, f)? },
        VExpr::Binary(op, a, b) => {
            let a = ev(h, node, a, f)?;
            match op {
                BinOp::And if !h.truthy(a) => return Some(V::Bool(false)),
                BinOp::Or if h.truthy(a) => return Some(V::Bool(true)),
                _ => {}
            }
            let b = ev(h, node, b, f)?;
            match op {
                BinOp::And | BinOp::Or => V::Bool(h.truthy(b)),
                BinOp::Eq => V::Bool(h.equal(a, b)),
                BinOp::Ne => V::Bool(!h.equal(a, b)),
                _ => binary_rest(h, *op, a, b)?,
            }
        }
        VExpr::IsNull(value) => V::Bool(ev(h, node, value, f)? == V::Null),
        VExpr::NonEmpty(value) => match ev(h, node, value, f)? {
            V::Str("") => V::Null,
            V::NodeText(id) if tree.node(id).end == tree.node(id).start => V::Null,
            v @ (V::List(_) | V::Record(..) | V::Text(_)) if !h.truthy(v) => V::Null,
            v => v,
        },
        VExpr::Coalesce(a, b) => match ev(h, node, a, f)? {
            v @ V::Node(_) => v,
            _ => ev(h, node, b, f)?,
        },
        VExpr::Nullish(a, b) => match ev(h, node, a, f)? {
            V::Null => ev(h, node, b, f)?,
            value => value,
        },
        VExpr::Node => V::Node(node),
        VExpr::Text => V::NodeText(node),
        VExpr::Parent => parent_layer(tree, node).map_or(V::Null, V::Node),
        VExpr::Offset => V::Num(f64::from(tree.node(node).start)),
        VExpr::EndOffset => V::Num(f64::from(tree.node(node).end)),
        _ => return ev_rare(h, node, e, f),
    })
}

/// [`ev`]'s `Binary` past `and`/`or`/`==`/`!=`: orderings, arithmetic, joins.
#[inline(never)]
fn binary_rest<H: Host>(h: &Runtime<'_, H>, op: BinOp, a: V, b: V) -> Option<V> {
    let order = || h.unpack(a).compare(&h.unpack(b));
    Some(match op {
        BinOp::Lt => V::Bool(order().is_lt()),
        BinOp::Le => V::Bool(order().is_le()),
        BinOp::Gt => V::Bool(order().is_gt()),
        BinOp::Ge => V::Bool(order().is_ge()),
        BinOp::Add => V::Num(h.num(a)? + h.num(b)?),
        BinOp::Sub => V::Num(h.num(a)? - h.num(b)?),
        BinOp::Rem => {
            let divisor = h.num(b)?;
            if divisor == 0.0 {
                return h.fail("remainder by zero".into());
            }
            V::Num(h.num(a)? % divisor)
        }
        BinOp::Mul if !matches!((a, b), (V::NodeText(_) | V::Str(_) | V::Text(_), V::Num(_)) | (V::Num(_), V::NodeText(_) | V::Str(_) | V::Text(_))) => {
            V::Num(h.num(a)? * h.num(b)?)
        }
        BinOp::Join if matches!(a, V::List(_)) || matches!(b, V::List(_)) => {
            let mut items = h.items(a);
            items.extend(h.items(b));
            h.list(&items)?
        }
        _ => {
            let result = h.result(crate::expr::binary(op, h.unpack(a), h.unpack(b)))?;
            h.pack(&result)?
        }
    })
}

/// The forms [`ev`] hands over: whatever builds lists, records or text,
/// walks sequences, or calls out.
#[inline(never)]
fn ev_rare<H: Host>(h: &Runtime<'_, H>, node: u32, e: &VExpr, f: &mut Vec<V>) -> Option<V> {
    let tree = h.tree();
    let source = h.source();
    macro_rules! eval {
        ($node:expr, $e:expr) => {
            ev(h, $node, $e, f)
        };
    }
    Some(match e {
        VExpr::List(items) => {
            let items = items.iter().map(|item| ev(h, node, item, f)).collect::<Option<Vec<_>>>()?;
            h.list(&items)?
        }
        VExpr::Record(shape, entries) => {
            let items = entries.iter().map(|entry| ev(h, node, entry, f)).collect::<Option<Vec<_>>>()?;
            V::Record(*shape, h.allocate(&items)?)
        }
        VExpr::Collect(sequence) => {
            let mut items = Vec::new();
            walk_seq(h, node, sequence, f, &mut |v, _| { items.push(v); Some(true) })?;
            h.list(&items)?
        }
        VExpr::FactCall { fact, args, receiver } => {
            if !h.facts_available() { return Some(V::Null); }
            let target = match receiver {
                Some(receiver) => match eval!(node, receiver)? {
                    V::Node(id) => id,
                    _ => return Some(V::Null),
                },
                None => node,
            };
            let args = args.iter().map(|arg| eval!(node, arg).map(|v| h.unpack(v).into_static())).collect::<Option<Vec<_>>>()?;
            h.fact_call(target, *fact, args)?
        }
        VExpr::FieldElse(label, inner) => {
            if tree.field_one(node, *label) == FieldOne::None {
                eval!(node, inner)?
            } else {
                ev(h, node, &VExpr::Field(*label), f)?
            }
        }
        VExpr::GroupBy { of, key, bind, shape } => {
            let mut input = Vec::new();
            walk_seq(h, node, of, f, &mut |item, _| { input.push(item); Some(true) })?;
            let mut groups: Vec<(V, Vec<V>)> = Vec::new();
            for (index, item) in input.into_iter().enumerate() {
                let value = call_bound(h, node, key, f, *bind, item, index as f64)?;
                match groups.iter_mut().find(|(key, _)| h.equal(*key, value)) {
                    Some((_, items)) => items.push(item),
                    None => groups.push((value, vec![item])),
                }
            }
            let mut records = Vec::with_capacity(groups.len());
            for (key, items) in groups {
                let items = h.list(&items)?;
                records.push(V::Record(*shape, h.allocate(&[key, items])?));
            }
            h.list(&records)?
        }
        VExpr::Partition { of, test, bind, shape } => {
            let mut input = Vec::new();
            walk_seq(h, node, of, f, &mut |item, _| { input.push(item); Some(true) })?;
            let (mut yes, mut no) = (Vec::new(), Vec::new());
            for (index, item) in input.into_iter().enumerate() {
                let value = call_bound(h, node, test, f, *bind, item, index as f64)?;
                if h.truthy(value) { yes.push(item); } else { no.push(item); }
            }
            let yes = h.list(&yes)?;
            let no = h.list(&no)?;
            V::Record(*shape, h.allocate(&[yes, no])?)
        }
        VExpr::Key(base, key) => h.key(eval!(node, base)?, key),
        VExpr::Member(base, key, part) => {
            let base = eval!(node, base)?;
            if &**key == "nodes" {
                match base {
                    V::Node(id) | V::NodeText(id) => h.list(&[V::Node(id)])?,
                    V::Null => h.list(&[])?,
                    value => h.project(value, |value| Some(match value {
                        V::Node(id) | V::NodeText(id) => V::Node(id),
                        value => h.key(value, key),
                    }))?,
                }
            } else {
                h.project(base, |value| match value {
                    V::Node(id) => match part { Some(part) => ev(h, id, part, f), None => Some(V::Null) },
                    value => Some(h.key(value, key)),
                })?
            }
        }
        VExpr::Path(base, keys, parts) => {
            let base = eval!(node, base)?;
            h.project(base, |mut value| {
                for (at, key) in keys.iter().enumerate() {
                    value = match value {
                        V::Node(_) if &**key == "node" => value,
                        V::Node(id) => return match &parts[at] { Some(part) => ev(h, id, part, f), None => Some(V::Null) },
                        value => h.key(value, key),
                    };
                }
                Some(value)
            })?
        }
        VExpr::Index => tree.field_index(node).map_or(V::Null, |index| V::Num(index as f64)),
        VExpr::Line => V::Num((h.lines().line_of(tree.node(node).start as usize) + 1) as f64),
        VExpr::Local(local) => {
            h.host.local(node, *local)?
        }
        VExpr::Trait(subject, path, query, mask) => match eval!(node, subject)? {
            V::Node(target) => V::Bool(h.query(target, path, query, *mask)?),
            _ => V::Null,
        },
        VExpr::QueryCall(query, items) => {
            let Some((subject, items)) = items.split_first() else { return Some(V::Null) };
            let subject = eval!(node, subject)?;
            let mut args = Vec::with_capacity(items.len());
            for item in items {
                args.push(h.unpack(eval!(node, item)?).into_owned());
            }
            let value = match subject {
                V::Node(target) => h.query_function(target, query, &args)?,
                other => {
                    let Some(function) = crate::expr::queries::handle_function(query.name) else { return Some(V::Null) };
                    let Some(handle) = crate::expr::queries::handle_of(&h.unpack(other)) else { return Some(V::Null) };
                    h.query_handle(handle, function, &args)?
                }
            };
            match h.file_vm() {
                Some(file) => file.shared_query(&value, h.values(), h.source())?,
                None => h.pack(&value)?,
            }
        }
        VExpr::IsTests(subject, tests) => {
            let value = eval!(node, subject)?;
            let id = match value { V::Node(id) => Some(id), _ => None };
            let g = h.grammar();
            // Non-node values (tokens) carry a kind: their text, or a
            // record's `kind`.
            let tagged = match value {
                V::NodeText(_) | V::Str(_) | V::Text(_) => h.with_text(value, |text| Some(g.matcher(text))).flatten(),
                V::Record(..) => h.with_text(h.key(value, "kind"), |text| Some(g.matcher(text))).flatten(),
                _ => None,
            };
            V::Bool(tests.iter().any(|test| match (test, id) {
                (IsTest::Lang(lang), _) => *lang == h.lang(),
                (IsTest::Kind(ids), None) => tagged.is_some_and(|m| is_a(g, m, ids)),
                (_, None) => false,
                (IsTest::Kind(ids), Some(id)) => is_a(g, tree.node(id).matcher, ids),
                (IsTest::Field(ids, label), Some(id)) => {
                    tree.label(id) == Some(*label) && tree.owner(id).is_some_and(|o| is_a(g, tree.node(o).matcher, ids))
                }
                (IsTest::Layer(layer), Some(id)) => tree.layer_of(id) == Some(*layer),
            }))
        }
        VExpr::Pattern(value, test) => {
            let value = eval!(node, value)?;
            let text = h.unpack(value).text().into_owned();
            V::Bool(h.pattern(test, &text)?)
        }
        VExpr::Ancestor(matcher, field) => match tree.ancestor(h.grammar(), node, *matcher) {
            None => V::Null,
            Some(ancestor) => match field {
                None => V::NodeText(ancestor),
                Some(label) => match tree.fields(ancestor, *label).as_slice() {
                    [] => V::Null,
                    [one] => V::NodeText(*one),
                    many => h.list(&many.iter().map(|&id| V::NodeText(id)).collect::<Vec<_>>())?,
                },
            },
        },
        VExpr::CallFn(i, args) => h.call(node, *i, args, f)?,
        VExpr::ReTest(text, pattern) => {
            let run = |hay: &str| h.result(crate::expr::re::test(&[Value::from(hay), Value::from(*pattern)])
                .map_err(|error| format!("re_test: {error}"))).map(|value| value.truthy());
            V::Bool(h.with_text(eval!(node, text)?, run)?)
        }
        VExpr::Count(s) => {
            if let Seq::Descendants(sel) = &**s {
                return Some(V::Num(h.descendants(node, sel).count() as f64));
            }
            let mut n = 0.0;
            walk_seq(h, node, s, f, &mut |_, _| { n += 1.0; Some(true) })?;
            V::Num(n)
        }
        VExpr::Sum(s) => {
            let mut total = 0.0;
            walk_seq(h, node, s, f, &mut |v, _| { total += h.num(v)?; Some(true) })?;
            V::Num(total)
        }
        VExpr::First(s) => {
            if let Seq::Descendants(sel) = &**s {
                return Some(h.descendants(node, sel).next().map_or(V::Null, V::Node));
            }
            // Other sequences drain so late errors still propagate.
            let mut first = None;
            walk_seq(h, node, s, f, &mut |v, _| { first.get_or_insert(v); Some(true) })?;
            first.unwrap_or(V::Null)
        }
        VExpr::Last(s) => {
            let mut last = V::Null;
            walk_seq(h, node, s, f, &mut |v, _| { last = v; Some(true) })?;
            last
        }
        VExpr::Each(kind, s, test, bind) => {
            if let (EachKind::Any, Seq::Descendants(sel)) = (*kind, &**s) {
                for (at, id) in h.descendants(node, sel).enumerate() {
                    if h.truthy(call_bound(h, node, test, f, *bind, V::Node(id), at as f64)?) {
                        return Some(V::Bool(true));
                    }
                }
                return Some(V::Bool(false));
            }
            // Non-raw inputs evaluate before any test.
            let mut items = Vec::new();
            walk_seq(h, node, s, f, &mut |v, _| { items.push(v); Some(true) })?;
            let holds = |at: usize, f: &mut Vec<V>| -> Option<bool> {
                Some(h.truthy(call_bound(h, node, test, f, *bind, items[at], at as f64)?))
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
            let mut best: Option<(V, V)> = None;
            let mut index = 0.0;
            walk_seq(h, node, of, f, &mut |v, f| {
                let k = call_bound(h, node, key, f, *bind, v, index)?;
                index += 1.0;
                if best.is_none_or(|(b, _)| {
                    let order = h.unpack(k).compare(&h.unpack(b));
                    if *max { order.is_gt() } else { order.is_lt() }
                }) {
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
        // A field holding several nodes (`ev` reads none or one).
        VExpr::Field(label) => h.list(&tree.fields(node, *label).into_iter().map(V::NodeText).collect::<Vec<_>>())?,
        VExpr::OnField(label, inner) => {
            let items = tree.fields(node, *label).into_iter().map(|id| ev(h, id, inner, f)).collect::<Option<Vec<_>>>()?;
            h.list(&items)?
        }
        VExpr::IsValue(subject, kinds) => {
            let matcher = match eval!(node, subject)? {
                V::Node(id) => Some(tree.node(id).matcher),
                v @ (V::NodeText(_) | V::Str(_) | V::Text(_)) => h.with_text(v, |text| Some(h.grammar().matcher(text)))?,
                value @ V::Record(..) => h.with_text(h.key(value, "kind"), |text| Some(h.grammar().matcher(text))).flatten(),
                _ => None,
            };
            V::Bool(matcher.is_some_and(|m| is_a(h.grammar(), m, kinds)))
        }
        VExpr::Contains(haystack, needle) => {
            let (haystack, needle) = (eval!(node, haystack)?, eval!(node, needle)?);
            V::Bool(match haystack {
                V::List(handle) => with_view(h.host, &h.scratch.borrow(), |view| {
                    (0..view.len(handle)).any(|at| view.equal(view.item(handle, at), needle, tree, source))
                }),
                value => h.text_pair(value, needle, |text, needle| text.contains(needle))?,
            })
        }
        VExpr::StartsWith(text, prefix) => {
            let (text, prefix) = (eval!(node, text)?, eval!(node, prefix)?);
            V::Bool(h.text_pair(text, prefix, |text, prefix| text.starts_with(prefix))?)
        }
        VExpr::EndsWith(text, suffix) => {
            let (text, suffix) = (eval!(node, text)?, eval!(node, suffix)?);
            V::Bool(h.text_pair(text, suffix, |text, suffix| text.ends_with(suffix))?)
        }
        VExpr::Call(function, items) => {
            let mut args = [V::Null; 3];
            let direct = items.len() <= args.len();
            if direct {
                for (slot, item) in args.iter_mut().zip(items.iter()) {
                    *slot = eval!(node, item)?;
                }
                // List arguments stay in the arena.
                if let Some(result) = list_call(h, function.name, &args[..items.len()]) {
                    return result;
                }
            }
            let mut values = Vec::with_capacity(items.len());
            for (at, item) in items.iter().enumerate() {
                let v = if direct { args[at] } else { eval!(node, item)? };
                values.push(h.unpack(v));
            }
            let result = h.result((function.call)(&values).map_err(|error| format!("{}: {error}", function.name)))?;
            h.pack(&result)?
        }
        VExpr::Builtin(builtin) => {
            let value = h.builtin(node, *builtin)?;
            h.pack(&value)?
        }
        VExpr::Option(index) => h.pack(&h.option(*index)?)?,
        VExpr::Debug(inner, label) => {
            let value = eval!(node, inner)?;
            h.debug(node, label, &h.unpack(value));
            value
        }
        VExpr::Position(offset, read) => {
            let at = h.position(h.num(eval!(node, offset)?)?)?;
            match read {
                PositionRead::Key(key) => V::Num(at.get(usize::from(*key)) as f64),
                PositionRead::Whole(shape) => {
                    let items: Vec<V> = (0..Position::KEYS.len()).map(|key| V::Num(at.get(key) as f64)).collect();
                    V::Record(*shape, h.allocate(&items)?)
                }
            }
        }
        VExpr::Get(record, key) => {
            let record = eval!(node, record)?;
            let key = eval!(node, key)?;
            match (record, key) {
                (V::List(handle), V::Num(at)) => with_view(h.host, &h.scratch.borrow(), |view| {
                    (at >= 0.0 && at.fract() == 0.0 && (at as usize) < view.len(handle)).then(|| view.item(handle, at as usize))
                }).unwrap_or(V::Null),
                _ => {
                    let key = h.unpack(key).text().into_owned();
                    h.key(record, &key)
                }
            }
        }
        VExpr::Containing(sel) => h.containing(node, sel)?.map_or(V::Null, V::Node),
        VExpr::NodeAt(offset) => match eval!(node, offset)? {
            V::Null => V::Null,
            at => {
                let at = (h.num(at)?.max(0.0) as usize).min(source.len()) as u32;
                tree.innermost_at(h.grammar(), at, u32::MAX).filter(|&id| !tree.is_root(id)).map_or(V::Null, V::Node)
            }
        },
        VExpr::Token(read, inner, pairs) => {
            use crate::expr::TokenRead;
            let V::Node(id) = eval!(node, inner)? else { return Some(V::Null) };
            let pairs = match pairs {
                Some(pairs) => Some(eval!(node, pairs)?),
                None => None,
            };
            let Some(tokens) = h.host.tokens() else { return Some(V::Null) };
            let (start, end) = (tree.node(id).start, tree.node(id).end);
            let text = |(from, to): (u32, u32)| source.get(from as usize..to as usize).unwrap_or("");
            let record = |index: usize| {
                let span = tokens.0[index];
                let exact = tree.innermost_at(h.grammar(), span.0, u32::MAX)
                    .filter(|&found| !tree.is_root(found) && tree.node(found).start == span.0 && tree.node(found).end == span.1);
                h.pack(&Value::record(vec![
                    ("text".into(), Value::from(text(span))),
                    ("start".into(), Value::Num(f64::from(span.0))),
                    ("end".into(), Value::Num(f64::from(span.1))),
                    ("node".into(), exact.map_or(Value::Null, |found| Value::Node(found, crate::expr::value::Text::Borrowed(text(span))))),
                ]))
            };
            let found = match read {
                TokenRead::First => tokens.after(start).filter(|&i| tokens.0[i].1 <= end),
                TokenRead::Last => tokens.before(end).filter(|&i| tokens.0[i].0 >= start),
                TokenRead::Before => tokens.before(start),
                TokenRead::After => tokens.after(end),
                TokenRead::List => {
                    let texts: Vec<Value> = tokens.within(start, end).iter().map(|&span| Value::from(text(span))).collect();
                    return h.pack(&Value::list(texts));
                }
                TokenRead::Pair => {
                    let pairs = h.unpack(pairs?);
                    let Value::List(pairs) = &pairs else { return Some(V::Null) };
                    let pairs: Vec<(String, String)> = pairs.iter().filter_map(|pair| match pair {
                        Value::List(ends) if ends.len() == 2 => Some((ends[0].text().into_owned(), ends[1].text().into_owned())),
                        _ => None,
                    }).collect();
                    let pairs: Vec<(&str, &str)> = pairs.iter().map(|(open, close)| (open.as_str(), close.as_str())).collect();
                    let opener = tokens.after(start).filter(|&i| tokens.0[i].0 == start);
                    let closer = tokens.before(end).filter(|&i| tokens.0[i].1 == end);
                    opener.and_then(|i| tokens.partner(i, source, &pairs))
                        .or_else(|| closer.and_then(|i| tokens.partner(i, source, &pairs)))
                }
            };
            match found {
                Some(index) => record(index)?,
                None => V::Null,
            }
        }
        VExpr::OwnChild(last, kinds, inner) => {
            let fits = |edge: &&crate::grammar::Edge| kinds.iter().any(|&k| h.grammar().is_a(tree.node(edge.node).matcher, k));
            let children = &tree.node(node).children;
            let child = if *last { children.iter().rev().find(fits) } else { children.iter().find(fits) };
            match child {
                Some(edge) => eval!(edge.node, inner)?,
                None => V::Null,
            }
        }
        VExpr::KindName(inner) => match eval!(node, inner)? {
            V::Node(id) => match tree.node(id).matcher {
                crate::grammar::ROOT => h.pack(&Value::from(format!(".{}", h.grammar().langs[h.host.lang()].name)))?,
                crate::grammar::TEXT => V::Null,
                m => V::Str(intern(&h.grammar().matchers[m as usize].name).0),
            },
            _ => V::Null,
        },
        VExpr::Template(parts) => {
            let mut text = String::new();
            for part in parts.iter() {
                let v = eval!(node, part)?;
                text.push_str(&h.unpack(v).text());
            }
            h.pack(&Value::from(text))?
        }
        VExpr::Const(_) | VExpr::Bound(_) | VExpr::Attr(_) | VExpr::ParentAttr(_) | VExpr::Fact { .. } | VExpr::Inlined(..) | VExpr::Is { .. }
        | VExpr::InField(..) | VExpr::InFields(_) | VExpr::FieldPresent(_) | VExpr::FieldCount(_) | VExpr::OnParent(_) | VExpr::OnSibling(..)
        | VExpr::OnAncestor(..) | VExpr::Not(_) | VExpr::Neg(_) | VExpr::If(..) | VExpr::Binary(..) | VExpr::IsNull(_)
        | VExpr::NonEmpty(_) | VExpr::Coalesce(..) | VExpr::Nullish(..) | VExpr::Node | VExpr::Text | VExpr::Parent
        | VExpr::Offset | VExpr::EndOffset => unreachable!("ev evaluates these itself"),
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

    /// The kinds in this set and not in `other`.
    pub fn without(&self, other: &KindSet) -> KindSet {
        KindSet(self.0.iter().enumerate().map(|(i, &word)| word & !other.0.get(i).copied().unwrap_or(0)).collect())
    }

    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().enumerate().flat_map(|(i, &word)| {
            let mut rest = word;
            std::iter::from_fn(move || {
                if rest == 0 { return None }
                let bit = rest.trailing_zeros();
                rest &= rest - 1;
                Some((i * 64) as u32 + bit)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::functions;

    #[test]
    fn compound_call_arguments_keep_values_and_lambda_bindings() {
        for condition in [
            r#"upper(lower(text)) == "AB""#,
            r#"twice(len(lower(text))) == 4"#,
            r#"join(map([text], item => upper(lower(item))), ",") == "AB""#,
            r#"upper(if len(text) == 2 then lower(text) else "wrong") == "AB""#,
            r#"upper("{lower(text)}") == "AB""#,
            r#"upper({ value text; lower }) == "AB""#,
            r#"join({ value [text]; concat [lower(text)] }, ",") == "Ab,ab""#,
            r#"pick({ value: upper(lower(text)) }) == "AB""#,
        ] {
            let linter = crate::engine::tests::linter(&format!(
                "fn twice(value) = value + value\nfn pick(value) = value.value\nin .t match item /[A-Za-z]+/ {{ rule result warn \"result\" {{ when {condition} }} }}",
            ));
            assert_eq!(crate::engine::tests::lint(&linter, "Ab").len(), 1, "{condition}");
            assert_eq!(crate::engine::tests::lint(&linter, "x").len(), 0, "{condition}");
        }
    }

    #[test]
    fn record_and_list_values_preserve_keys_paths_and_container_semantics() {
        use crate::grammar::{Edge, Node};
        struct TestHost {
            tree: Tree,
            lines: crate::line_index::LineIndex,
            values: Values,
        }
        impl Host for TestHost {
            fn tree(&self) -> &Tree { &self.tree }
            fn grammar(&self) -> &Grammar { panic!("no grammar queries") }
            fn source(&self) -> &str { "x" }
            fn lines(&self) -> &crate::line_index::LineIndex { &self.lines }
            fn attr(&self, _: u32, _: u16) -> Option<V> { None }
            fn fact(&self, _: u32, _: usize, _: FactOf) -> Option<V> { None }
            fn values(&self) -> &Values { &self.values }
        }
        let mut host = TestHost {
            tree: Tree::from_native(vec![
                Node { matcher: TEXT, start: 0, end: 1, children: vec![] },
                Node { matcher: ROOT, start: 0, end: 1, children: vec![Edge { node: 0, label: 0 }] },
                Node { matcher: TEXT, start: 0, end: 1, children: vec![] },
                Node { matcher: TEXT, start: 0, end: 1, children: vec![] },
                Node { matcher: ROOT, start: 0, end: 1, children: vec![Edge { node: 2, label: 0 }, Edge { node: 3, label: 0 }] },
                Node { matcher: TEXT, start: 0, end: 0, children: vec![] },
                Node { matcher: ROOT, start: 0, end: 0, children: vec![Edge { node: 5, label: 0 }] },
            ], 1),
            lines: crate::line_index::LineIndex::new("x"),
            values: Values::default(),
        };
        let num = |n| CExpr::Const(Value::Num(n));
        let record = || CExpr::Record(vec![
            ("size".into(), num(3.0)),
            ("size".into(), num(9.0)),
            ("owner".into(), CExpr::Builtin(Builtin::Node)),
        ]);
        let call = |name, value| CExpr::Call(functions::lookup(name).expect("registered"), vec![value]);
        let list = || CExpr::Const(Value::list(vec![Value::Num(2.0), Value::Num(5.0)]));
        let empty_record = || CExpr::Const(Value::record(Vec::new()));
        let regex = |value| CExpr::Call(functions::lookup("re_test").expect("registered"), vec![value, CExpr::Const(Value::from("."))]);
        for (expression, expected) in [
            (call("count", CExpr::List(vec![CExpr::Builtin(Builtin::Node), record()])), V::Num(2.0)),
            (call("sum", CExpr::Member(Box::new(CExpr::List(vec![CExpr::Builtin(Builtin::Node), record()])),
                "size".into(), Some(Box::new(CExpr::Builtin(Builtin::Line))))), V::Num(4.0)),
            (CExpr::Key(Box::new(record()), "size".into()), V::Num(3.0)),
            (CExpr::Key(Box::new(record()), "absent".into()), V::Null),
            (CExpr::Member(Box::new(record()), "size".into(), None), V::Num(3.0)),
            (CExpr::Path(Box::new(record()), vec!["owner".into(), "line".into()],
                vec![None, Some(CExpr::Builtin(Builtin::Line))]), V::Num(1.0)),
            (call("count", CExpr::If(Box::new(CExpr::Const(Value::Bool(true))), Box::new(list()), Box::new(empty_record()))), V::Num(2.0)),
            (CExpr::Member(Box::new(list()), "size".into(), None), V::Null),
            (CExpr::Not(Box::new(empty_record())), V::Bool(true)),
            (CExpr::NonEmpty(Box::new(empty_record())), V::Null),
            (regex(CExpr::Const(Value::Null)), V::Bool(false)),
            (regex(CExpr::Const(Value::Bool(true))), V::Bool(true)),
            (regex(num(4.0)), V::Bool(true)),
            (regex(list()), V::Bool(true)),
            (regex(record()), V::Bool(true)),
            (CExpr::Binary(BinOp::Eq, Box::new(list()), Box::new(CExpr::Const(Value::list(vec![
                Value::from("2"), Value::Num(5.0),
            ])))), V::Bool(true)),
            (CExpr::Binary(BinOp::Eq,
                Box::new(CExpr::Const(Value::record(vec![("a".into(), Value::Num(1.0)), ("b".into(), Value::Bool(true))]))),
                Box::new(CExpr::Const(Value::record(vec![("b".into(), Value::Bool(true)), ("a".into(), Value::from("1"))])))),
                V::Bool(true)),
        ] {
            let lowered = lower(&expression, &[], &mut host.values);
            assert_eq!(eval(&host, 0, &lowered).map(|v| v.value), Some(expected), "{expression:?}");
        }
        let projection = call("count", CExpr::OnField(0, Box::new(list())));
        let lowered = lower(&projection, &[], &mut host.values);
        assert_eq!(eval(&host, 1, &lowered).map(|v| v.value), Some(V::Num(2.0)), "one field's list is the sequence, not one nested item");
        assert_eq!(eval(&host, 4, &lowered).map(|v| v.value), Some(V::Num(2.0)), "multiple fields keep nested lists");
        let children = call("count", CExpr::Relative(Relative::Children, Box::new(list())));
        let lowered = lower(&children, &[], &mut host.values);
        assert_eq!(eval(&host, 4, &lowered).map(|v| v.value), Some(V::Num(2.0)), "children keep nested lists");
        let fallback = CExpr::Binary(BinOp::Rem, Box::new(num(1.0)), Box::new(num(0.0)));
        let invalid = lower(&fallback, &[], &mut host.values);
        let field_else = lower(&CExpr::FieldElse(0, Box::new(fallback)), &[], &mut host.values);
        for (node, expected) in [
            (1, Value::from("x")),
            (4, Value::list(vec![Value::from("x"), Value::from("x")])),
            (6, Value::from("")),
        ] {
            assert_eq!(eval(&host, node, &field_else).unwrap().value(&host, "x"), expected);
        }
        let missing = lower(&CExpr::FieldElse(99, Box::new(num(9.0))), &[], &mut host.values);
        assert_eq!(eval(&host, 1, &missing).unwrap().value(&host, "x"), Value::Num(9.0));
        let composite = lower(&record(), &[], &mut host.values);
        let file = FileVm::default();
        let escaped = eval(&host, 0, &composite).unwrap().persist(&file).unwrap();
        let file_bytes = file.values.borrow().bytes();
        for _ in 0..100 {
            let evaluated = eval(&host, 0, &composite).unwrap();
            assert!(evaluated.truthy(&host));
            assert!(evaluated.scratch.bytes() > 0);
        }
        assert_eq!(file.values.borrow().bytes(), file_bytes, "discarded evaluations must not retain payloads");
        let scratch = values::Arena::default();
        let persistent = file.values.borrow();
        let view = values::View { config: &host.values, scratch: &scratch, file: &persistent };
        let V::Record(shape, handle) = escaped else { panic!("record"); };
        assert_eq!(view.key(shape, handle, "size"), V::Num(3.0));
        let owned_list = lower(&CExpr::List(vec![CExpr::Builtin(Builtin::Text)]), &[], &mut host.values);
        let target = lower_condition(&CExpr::List(vec![CExpr::Builtin(Builtin::Text), CExpr::Param(0)]), &[], &[], &mut host.values, true);
        let session = Session::new(&host, 0, &[]);
        assert_eq!(session.value_on(0, &target, Some(5)), Some(Value::list(vec![Value::from("x"), Value::Node(5, "".into())])));
        let first = session.value_on(0, &owned_list, None).expect("owned result");
        assert_eq!(session.value_on(0, &invalid, None), None);
        assert_eq!(session.value_on(5, &owned_list, None), Some(Value::list(vec![Value::from("")])));
        assert_eq!(first, Value::list(vec![Value::from("x")]));
        let enabled = VExpr::Key(Box::new(VExpr::Bound(0)), "enabled".into());
        let context = |enabled| Value::record(vec![
            ("enabled".into(), Value::Bool(enabled)),
            ("conditions".into(), Value::list(vec![Value::from("module")])),
        ]);
        assert_eq!(session.holds_with(&enabled, &context(false)), Some(false));
        assert_eq!(session.holds_with(&enabled, &context(true)), Some(true));
        let conditions = VExpr::Key(Box::new(VExpr::Bound(0)), "conditions".into());
        assert_eq!(session.value_with(&conditions, &context(true)), Some(Value::list(vec![Value::from("module")])));
    }

    #[test]
    fn grouping_preserves_loose_keys_and_first_seen_order() {
        use crate::grammar::Node;
        struct TestHost { tree: Tree, lines: crate::line_index::LineIndex, values: Values }
        impl Host for TestHost {
            fn tree(&self) -> &Tree { &self.tree }
            fn grammar(&self) -> &Grammar { panic!("no grammar queries") }
            fn source(&self) -> &str { "" }
            fn lines(&self) -> &crate::line_index::LineIndex { &self.lines }
            fn attr(&self, _: u32, _: u16) -> Option<V> { None }
            fn fact(&self, _: u32, _: usize, _: FactOf) -> Option<V> { None }
            fn values(&self) -> &Values { &self.values }
        }
        let mut host = TestHost {
            tree: Tree::from_native(vec![Node { matcher: ROOT, start: 0, end: 0, children: vec![] }], 0),
            lines: crate::line_index::LineIndex::new(""), values: Values::default(),
        };
        let record = |value| Value::record(vec![("a".into(), value)]);
        let input = vec![Value::Num(2.0), record(Value::Num(1.0)), Value::from("2"), record(Value::from("1")), Value::Num(3.0)];
        let expression = CExpr::Each(Box::new(crate::expr::pipeline::Stage {
            kind: Each::GroupBy, depth: 0, arity: 1, input: CExpr::Const(Value::list(input.clone())),
            args: vec![], body: CExpr::Param(0),
        }));
        let expression = lower(&expression, &[], &mut host.values);
        let result = eval(&host, 0, &expression).expect("grouped").value(&host, "");
        let group = |key, items| Value::record(vec![("key".into(), key), ("items".into(), Value::list(items))]);
        assert_eq!(result, Value::list(vec![
            group(input[0].clone(), vec![input[0].clone(), input[2].clone()]),
            group(input[1].clone(), vec![input[1].clone(), input[3].clone()]),
            group(input[4].clone(), vec![input[4].clone()]),
        ]));
    }

    #[test]
    fn position_and_remainder_reads_preserve_boundaries_and_failure() {
        use crate::grammar::{Edge, Node};
        struct TestHost {
            tree: Tree,
            lines: crate::line_index::LineIndex,
        }
        impl Host for TestHost {
            fn tree(&self) -> &Tree { &self.tree }
            fn grammar(&self) -> &Grammar { panic!("no grammar queries") }
            fn source(&self) -> &str { "a\r\nβ\nc" }
            fn lines(&self) -> &crate::line_index::LineIndex { &self.lines }
            fn attr(&self, _: u32, _: u16) -> Option<V> { None }
            fn fact(&self, _: u32, _: usize, _: FactOf) -> Option<V> { None }
            fn values(&self) -> &Values {
                static VALUES: OnceLock<Values> = OnceLock::new();
                VALUES.get_or_init(Values::default)
            }
        }
        let leaf = |start, end| Node { matcher: TEXT, start, end, children: vec![] };
        let host = TestHost { lines: crate::line_index::LineIndex::new("a\r\nβ\nc"), tree: Tree::from_native(vec![
            leaf(0, 1), leaf(3, 5), leaf(6, 7), leaf(7, 7),
            Node { matcher: ROOT, start: 0, end: 7, children: vec![
                Edge { node: 0, label: 7 }, Edge { node: 1, label: 8 },
                Edge { node: 2, label: 7 }, Edge { node: 3, label: u16::MAX },
                Edge { node: 0, label: 7 },
            ] },
        ], 4) };
        let run = |node, expression: CExpr| eval(&host, node, &lower(&expression, &[], &mut Values::default())).map(|v| v.value);
        for (node, index, line) in [(0, 0.0, 1.0), (1, 0.0, 2.0), (2, 2.0, 3.0)] {
            assert_eq!(run(node, CExpr::Builtin(Builtin::Index)), Some(V::Num(index)));
            assert_eq!(run(node, CExpr::Builtin(Builtin::Line)), Some(V::Num(line)));
        }
        assert_eq!(run(4, CExpr::Builtin(Builtin::Index)), Some(V::Null));
        let sibling = |relative| CExpr::Relative(relative, Box::new(CExpr::Builtin(Builtin::Line)));
        assert_eq!(run(0, sibling(Relative::Previous)), Some(V::Null));
        assert_eq!(run(0, sibling(Relative::Next)), Some(V::Num(1.0)));
        assert_eq!(run(2, sibling(Relative::Previous)), Some(V::Num(1.0)));
        assert_eq!(run(3, sibling(Relative::Next)), Some(V::Null));
        let remainder = |a, b| CExpr::Binary(BinOp::Rem, Box::new(CExpr::Const(a)), Box::new(CExpr::Const(b)));
        assert_eq!(run(0, remainder(Value::from("-5"), Value::Num(2.0))), Some(V::Num(-1.0)));
        assert_eq!(run(0, remainder(Value::Num(1.0), Value::Num(0.0))), None);
        assert_eq!(run(0, remainder(Value::from("invalid"), Value::Num(2.0))), None);
        assert_eq!(run(0, CExpr::Binary(BinOp::And, Box::new(CExpr::Const(Value::Bool(false))),
            Box::new(remainder(Value::Num(1.0), Value::Num(0.0))))), Some(V::Bool(false)));
    }

    #[test]
    fn lowered_attributes_keep_cycles_and_eager_argument_errors_visible() {
        use crate::engine::tests::native_linter;
        use std::path::Path;
        let config = r#"
            in ts_node let cyclic cyclic
            in ts_return rule cycle info "{cyclic}"
            fn lazy_pick(flag, x) = if flag then 0 else x
            in ts_identifier let text_number num(text)
            in ts_identifier rule lazy_arg info "{lazy_pick(true, text_number)}"
            fn identity(value) = value
            fn depth(n, value) = if n == 0 then { value value; num; identity } else depth(n - 1, value)
            in ts_identifier rule depth_arg info "{depth(255, text)}"
        "#;
        let diagnostics = native_linter(config).lint(Path::new("/root/a.ts"), "function f(a) { return a; return a; }");
        assert!(diagnostics.iter().any(|d| d.rule == "cycle" && d.severity == crate::config::ast::Severity::Error));
        let failed_arguments: Vec<_> = diagnostics.iter().filter(|d| d.rule == "lazy_arg").collect();
        assert!(!failed_arguments.is_empty());
        assert!(failed_arguments.iter().all(|d| d.severity == crate::config::ast::Severity::Error));
        let depth_arguments: Vec<_> = diagnostics.iter().filter(|d| d.rule == "depth_arg").collect();
        assert_eq!(depth_arguments.len(), failed_arguments.len());
        for (deep, shallow) in depth_arguments.iter().zip(failed_arguments) {
            assert_eq!(deep.message.split_once("failed here:").map(|(_, cause)| cause), shallow.message.split_once("failed here:").map(|(_, cause)| cause));
        }
    }

    #[test]
    fn raw_descendant_reducers_stop_without_hiding_pipeline_errors() {
        use crate::grammar::{Edge, Node};
        struct CountingHost<'a> {
            grammar: &'a Grammar,
            tree: Tree,
            lines: crate::line_index::LineIndex,
            values: Values,
            file: FileVm,
            yielded: std::cell::Cell<usize>,
        }
        impl Host for CountingHost<'_> {
            fn tree(&self) -> &Tree { &self.tree }
            fn grammar(&self) -> &Grammar { self.grammar }
            fn source(&self) -> &str { "abc" }
            fn lines(&self) -> &crate::line_index::LineIndex { &self.lines }
            fn values(&self) -> &Values { &self.values }
            fn file_vm(&self) -> Option<&FileVm> { Some(&self.file) }
            fn attr(&self, _: u32, _: u16) -> Option<V> { None }
            fn fact(&self, _: u32, _: usize, _: FactOf) -> Option<V> { None }
            fn descendants<'s>(&'s self, node: u32, sel: &NodeSel) -> impl Iterator<Item = u32> + 's {
                sel.descendants(&self.tree, node).inspect(|_| self.yielded.set(self.yielded.get() + 1))
            }
        }
        let linter = crate::engine::tests::linter("match item \"i\"");
        let item = linter.grammar.matcher("item").unwrap();
        let host = CountingHost {
            grammar: &linter.grammar,
            tree: Tree::from_native(vec![
                Node { matcher: item, start: 0, end: 1, children: vec![] },
                Node { matcher: item, start: 1, end: 2, children: vec![] },
                Node { matcher: item, start: 2, end: 3, children: vec![] },
                Node { matcher: ROOT, start: 0, end: 3, children: vec![
                    Edge { node: 0, label: 1 }, Edge { node: 1, label: 1 }, Edge { node: 2, label: 1 },
                ] },
            ], 3),
            lines: crate::line_index::LineIndex::new("abc"),
            values: Values::default(),
            file: FileVm::default(),
            yielded: Default::default(),
        };
        let source = || Box::new(Seq::Descendants(NodeSel::new(host.grammar, &[item], None)));
        let bind = Binder { slot: 0, indexed: false };
        let first = VExpr::First(source());
        assert_eq!(eval(&host, 3, &first).unwrap().value, V::Node(0));
        assert_eq!(host.yielded.replace(0), 1);
        let any = |source| VExpr::Each(EachKind::Any, source, Box::new(VExpr::Const(V::Bool(true))), bind);
        assert_eq!(eval(&host, 3, &any(source())).unwrap().value, V::Bool(true));
        assert_eq!(host.yielded.replace(0), 1);
        let indexed = VExpr::Each(EachKind::Any, source(), Box::new(VExpr::Binary(
            BinOp::Eq, Box::new(VExpr::Bound(1)), Box::new(VExpr::Const(V::Num(1.0))),
        )), Binder { slot: 0, indexed: true });
        assert_eq!(eval(&host, 3, &indexed).unwrap().value, V::Bool(true));
        assert_eq!(host.yielded.replace(0), 2);
        let never = VExpr::Each(EachKind::Any, source(), Box::new(VExpr::Const(V::Bool(false))), bind);
        assert_eq!(eval(&host, 3, &never).unwrap().value, V::Bool(false));
        assert_eq!(host.yielded.replace(0), 3);
        let error = || VExpr::Call(functions::lookup("num").unwrap(), vec![VExpr::Const(V::Str("bad"))].into());
        let failed = VExpr::Each(EachKind::Any, source(), Box::new(error()), bind);
        assert!(eval(&host, 3, &failed).is_none());
        assert_eq!(host.yielded.replace(0), 1);
        assert_eq!(eval(&host, 3, &VExpr::Count(source())).unwrap().value, V::Num(3.0));
        assert_eq!(host.yielded.replace(0), 3);
        let empty = Box::new(Seq::Descendants(NodeSel::new(host.grammar, &[], None)));
        assert_eq!(eval(&host, 3, &VExpr::First(empty)).unwrap().value, V::Null);
        let mapped = || Box::new(Seq::Stage(SeqStage::Map, source(), Box::new(VExpr::If(
            Box::new(VExpr::Binary(BinOp::Eq, Box::new(VExpr::Bound(0)), Box::new(VExpr::Const(V::Node(1))))),
            Box::new(VExpr::Call(functions::lookup("num").unwrap(), vec![VExpr::Const(V::Str("bad"))].into())),
            Box::new(VExpr::Const(V::Num(1.0))),
        )), bind));
        assert!(eval(&host, 3, &VExpr::First(mapped())).is_none());
        assert!(eval(&host, 3, &any(mapped())).is_none());
        let walked = || Box::new(Seq::Walk(source(), Box::new(Seq::Value(Box::new(VExpr::If(
            Box::new(VExpr::Binary(BinOp::Eq, Box::new(VExpr::Bound(0)), Box::new(VExpr::Const(V::Node(1))))),
            Box::new(error()), Box::new(VExpr::Const(V::Null)),
        )))), 0));
        assert!(eval(&host, 3, &VExpr::First(walked())).is_none());
        assert!(eval(&host, 3, &any(walked())).is_none());
    }

}
