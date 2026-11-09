//! Lowered rule programs (`VExpr`) and their values.

mod handle;
mod records;

pub use handle::{Handle, Region, CELL_OFFSET_MASK, SCRATCH_TEXT};
pub use records::{decode, encode, stored, Cell, BUSY, GAVE_UP, UNSET};

use crate::builtins::Builtin;
use crate::expr::{CExpr, FactOf, HandleId, IsTest, NodeSel, QueryId, TokenRead};
use crate::functions::Function;
use crate::ops::{BinOp, Quantifier};
use crate::semantic::Value as SemanticValue;
use crate::value::Value;
use dx_core::fx_hash::FxHashMap;
use dx_tree::Tree;
use std::sync::OnceLock;

/// A VM value. Config text is interned; native text belongs to the file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum V {
    Null,
    Bool(bool),
    Num(f64),
    /// A node (`node`, a field, an enclosing layer): `.x` reads on it
    /// wherever it was passed; two nodes are equal when they are the same
    /// node. Joined, templated or compared with text, it reads as its source.
    Node(u32),
    /// A node's own source (`text`): plain text, nothing further to read.
    Source(u32),
    /// A config text constant.
    Str(ConstText),
    /// Text in a VM text pool: the file's, or evaluation scratch
    /// ([`SCRATCH_TEXT`]). Source substrings stay borrows.
    Text(u32),
    List(Handle),
    Record(u16, Handle),
}

// Every evaluation step returns one: past two words it goes through memory.
const _: () = assert!(std::mem::size_of::<Option<V>>() == 16);

/// [`V::fingerprint`] of null and of empty text.
const EMPTY_FINGERPRINT: u64 = 0xE;

fn number_fingerprint(number: f64) -> u64 {
    // `0.0 == -0.0`: one fingerprint for both.
    let bits = if number == 0.0 { 0 } else { number.to_bits() };
    bits.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x4E
}

pub fn text_fingerprint(text: &str) -> u64 {
    use std::hash::BuildHasher;
    if let Ok(number) = text.trim().parse::<f64>() { return number_fingerprint(number) }
    if text.is_empty() { return EMPTY_FINGERPRINT }
    dx_core::fx_hash::FxBuild::default().hash_one(text)
}

/// Text constants from configs, interned for the process: configs repeat
/// the same few strings, so the pool stays small across reloads.
type Pool = (FxHashMap<&'static str, ConstText>, Vec<ConstText>);

/// A pooled text constant: one word (a [`V`] stays small enough to return in
/// registers), one per distinct text, so equal constants are the same pointer.
#[derive(Clone, Copy)]
pub struct ConstText(&'static PooledText);

struct PooledText {
    text: &'static str,
    index: u32,
}

impl ConstText {
    pub fn text(self) -> &'static str { self.0.text }
    fn index(self) -> u32 { self.0.index }
}

impl PartialEq for ConstText {
    fn eq(&self, other: &Self) -> bool { std::ptr::eq(self.0, other.0) }
}

impl std::fmt::Debug for ConstText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.text.fmt(f) }
}

/// [`intern`]'s constants by index, read without a lock: every record read of
/// a text value asks. Past its end, the pool answers.
const INDEXED_TEXTS: usize = 1 << 16;
static TEXT_INDEX: [std::sync::atomic::AtomicPtr<PooledText>; INDEXED_TEXTS] = [const { std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()) }; INDEXED_TEXTS];

fn pool() -> &'static std::sync::RwLock<Pool> {
    static POOL: OnceLock<std::sync::RwLock<Pool>> = OnceLock::new();
    POOL.get_or_init(Default::default)
}

/// The pooled copy of a text constant.
pub fn intern(text: &str) -> ConstText {
    if let Some(&found) = pool().read().expect("pool lock").0.get(text) {
        return found;
    }
    let mut pool = pool().write().expect("pool lock");
    if let Some(&found) = pool.0.get(text) {
        return found;
    }
    let text: &'static str = Box::leak(text.into());
    let index = pool.1.len() as u32;
    let pooled: &'static PooledText = Box::leak(Box::new(PooledText { text, index }));
    if let Some(slot) = TEXT_INDEX.get(index as usize) {
        slot.store(std::ptr::from_ref(pooled).cast_mut(), std::sync::atomic::Ordering::Release);
    }
    let found = ConstText(pooled);
    pool.0.insert(text, found);
    pool.1.push(found);
    found
}

/// A text constant by its [`ConstText::index`].
pub(super) fn interned(index: u32) -> ConstText {
    if let Some(slot) = TEXT_INDEX.get(index as usize) {
        // SAFETY: set once to a leaked constant, before its index can reach
        // a record.
        return ConstText(unsafe { &*slot.load(std::sync::atomic::Ordering::Acquire) });
    }
    pool().read().expect("pool lock").1[index as usize]
}

/// A [`V`] in one word (the record cell coding), for tables that keep many.
#[derive(Clone, Copy)]
pub struct Packed(u64);

impl Packed {
    pub fn new(v: V) -> Self { Packed(records::encode(v)) }
    pub fn get(self) -> V { records::unpack(self.0) }
}

impl V {
    /// The value's text, when it is text or a node.
    pub fn text<'s>(self, tree: &Tree, source: &'s str) -> Option<&'s str> {
        match self {
            V::Node(id) | V::Source(id) => Some(tree.text(source, id)),
            V::Str(s) => Some(s.text()),
            _ => None,
        }
    }

    pub fn truthy(self, tree: &Tree) -> bool {
        match self {
            V::Null => false,
            V::Bool(b) => b,
            V::Num(n) => n != 0.0,
            V::Node(_) => true,
            V::Source(id) => {
                tree.read_text(id);
                let n = tree.node(id);
                n.end > n.start
            }
            V::Str(s) => !s.text().is_empty(),
            V::List(_) | V::Record(..) | V::Text(_) => unreachable!("file values need their arena"),
        }
    }

    /// [`Value::num`] for a scalar.
    pub fn num(self, tree: &Tree, source: &str) -> Option<f64> {
        match self {
            V::Null => Some(0.0),
            V::Bool(b) => Some(f64::from(u8::from(b))),
            V::Num(n) => Some(n),
            V::List(_) | V::Record(..) | V::Text(_) => None,
            text => text.text(tree, source)?.trim().parse().ok(),
        }
    }

    /// A hash that values [`Self::loose_eq`] calls equal share (unequal ones
    /// may collide): text reads as the number it parses as, empty text as
    /// null, so `1`, `"1"` and `" 1.0 "` agree.
    pub fn fingerprint(self, tree: &Tree, source: &str) -> u64 {
        match self {
            V::Null => EMPTY_FINGERPRINT,
            V::Num(number) => number_fingerprint(number),
            V::List(_) | V::Record(..) | V::Text(_) => unreachable!("file values need their arena"),
            other => match other.text(tree, source) {
                Some(text) => text_fingerprint(text),
                None => match other { V::Bool(value) => 0xB001 + u64::from(value), _ => 0x0DD },
            },
        }
    }

    /// [`Value::loose_eq`].
    pub fn loose_eq(self, other: V, tree: &Tree, source: &str) -> bool {
        match (self, other) {
            // Nodes are equal when they are the same node; compare their
            // source with templates (`"{a}" == "{b}"`).
            (V::Node(a), V::Node(b)) => a == b,
            (V::Str(a), V::Str(b)) => a == b,
            (V::Null, V::Null) => true,
            (V::Bool(a), V::Bool(b)) => a == b,
            (V::Num(a), V::Num(b)) => a == b,
            (V::Num(a), text) | (text, V::Num(a)) => text.text(tree, source).is_some_and(|s| s.trim().parse::<f64>().is_ok_and(|b| b == a)),
            // Only null is null: an empty string or node (an array hole) is present.
            (V::Null, _) | (_, V::Null) => false,
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
                Value::Node(id, crate::value::Text::Borrowed(&source[n.start as usize..n.end as usize]))
            }
            V::Source(id) => Value::Str(crate::value::Text::Read(tree.text(source, id))),
            V::Str(s) => Value::from(s.text()),
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
    /// `position(offset)`: one of [`crate::expr::Position::KEYS`], or the whole record.
    Position(Box<VExpr>, PositionRead),
    /// `get(record, key)` with a computed key.
    Get(Box<VExpr>, Box<VExpr>),
    /// `containing(sel)`: the innermost selected node holding this one.
    Containing(NodeSel),
    /// `node_at(offset)`: the innermost default-layer node holding it.
    NodeAt(Box<VExpr>),
    /// With the shape of the token record it gives.
    Token(TokenRead, Box<VExpr>, Option<Box<VExpr>>, u16),
    /// `opener.x` / `closer.x`: on the first (last) child of these layers.
    OwnChild(bool, Box<[u32]>, Box<VExpr>),
    /// `matcher.x`: on the closest ancestor of that matcher.
    OnAncestor(u32, Box<VExpr>),
    /// `kind_name(n)`.
    KindName(Box<VExpr>),
    /// A template: its parts' texts joined.
    Template(Box<[VExpr]>),
    /// A rule's own `let`, by slot: computed once per rule run
    /// (Session), on the rule's node.
    Local(u16),
    /// `value is a, b.field, layer l, .lang`, every test form.
    IsTests(Box<VExpr>, Box<[IsTest]>),
    Trait(Box<VExpr>, SemanticValue, Quantifier, u64),
    QueryCall(QueryId, Option<HandleId>, Box<[VExpr]>),
    /// A pattern test (`x ~ pattern`): the [`CExpr::Test`] or
    /// [`CExpr::TestDfa`] node, its matcher shared.
    Pattern(Box<VExpr>, Box<CExpr>),
    /// The closest ancestor of a matcher: its text, or one of its fields.
    Ancestor(u32, Option<u16>),
    /// A config `fn` on its arguments: its body is lowered once per config
    /// and runs in a frame of its own.
    CallFn(u32, Box<[VExpr]>),
    /// A call's body with its arguments in place (inlinable); like
    /// [`VExpr::CallFn`] it gives up past the runtime's call depth.
    Inlined(u32, Box<VExpr>, Box<[VExpr]>),
    /// A rule `let` reading nothing of the node (hoist): computed once
    /// per file.
    Hoisted(Box<VExpr>),
}

/// What `position(offset)` reads.
#[derive(Clone, Copy, Debug)]
pub enum PositionRead {
    /// One key, by index in [`crate::expr::Position::KEYS`].
    Key(u8),
    /// The record, in this config shape.
    Whole(u16),
}

impl VExpr {
    /// The conjuncts of a condition that read only syntax (fields, kinds,
    /// texts, value-only functions on them), joined: when they are false
    /// the whole condition is, in every frame, without a type question.
    pub fn syntax_guard(&self) -> Option<VExpr> {
        if let VExpr::Binary(BinOp::And, a, b) = self {
            return match (a.syntax_guard(), b.syntax_guard()) {
                (Some(a), Some(b)) => Some(VExpr::Binary(BinOp::And, Box::new(a), Box::new(b))),
                (one, other) => one.or(other),
            };
        }
        self.is_syntactic().then(|| self.clone())
    }

    fn is_syntactic(&self) -> bool {
        match self {
            VExpr::Const(_) | VExpr::Is { .. } | VExpr::InField(..) | VExpr::InFields(_) | VExpr::FieldPresent(_)
            | VExpr::FieldCount(_) | VExpr::Node | VExpr::Text | VExpr::Parent | VExpr::Offset | VExpr::EndOffset
            | VExpr::Index | VExpr::Line | VExpr::Field(_) | VExpr::Ancestor(..) => true,
            VExpr::Not(a) | VExpr::Neg(a) | VExpr::IsNull(a) | VExpr::NonEmpty(a) | VExpr::OnField(_, a) | VExpr::OnParent(a)
            | VExpr::OnSibling(_, a) | VExpr::IsValue(a, _) | VExpr::FieldElse(_, a) | VExpr::ReTest(a, _) | VExpr::KindName(a)
            | VExpr::OwnChild(_, _, a) | VExpr::OnAncestor(_, a) | VExpr::Pattern(a, _) => a.is_syntactic(),
            VExpr::Binary(_, a, b) | VExpr::Coalesce(a, b) | VExpr::Nullish(a, b) | VExpr::Contains(a, b)
            | VExpr::StartsWith(a, b) | VExpr::EndsWith(a, b) => a.is_syntactic() && b.is_syntactic(),
            VExpr::If(a, b, c) => a.is_syntactic() && b.is_syntactic() && c.is_syntactic(),
            VExpr::List(items) | VExpr::Template(items) | VExpr::Call(_, items) => items.iter().all(VExpr::is_syntactic),
            _ => false,
        }
    }

    /// Reads only the tree, the option table or what is already bound:
    /// asks no semantic question and runs no `let`.
    pub fn is_cheap(&self) -> bool {
        match self {
            VExpr::Bound(_) | VExpr::Option(_) | VExpr::Hoisted(_) => true,
            VExpr::Not(a) | VExpr::IsNull(a) | VExpr::IsTests(a, _) | VExpr::IsValue(a, _) | VExpr::NonEmpty(a) => a.is_cheap(),
            VExpr::Binary(_, a, b) => a.is_cheap() && b.is_cheap(),
            other => other.is_syntactic(),
        }
    }

    /// Small value-only bodies cost less than a second per-node cache.
    pub fn is_direct(&self) -> bool {
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
    pub slot: u16,
    pub indexed: bool,
}

impl Binder {
    pub fn width(self) -> usize {
        1 + usize::from(self.indexed)
    }
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
    /// `n.descendants(sel)`: selected nodes under node `n`; none when `n` is
    /// not a node.
    DescendantsOf(NodeSel, Box<VExpr>),
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
