//! Compiled expressions (`CExpr`) and the small values they carry.

use crate::builtins::Builtin;
use crate::functions::Function;
use crate::ops::Quantifier;
use crate::pipeline;
use crate::semantic::Value as SemanticValue;
use crate::value::{self, Value};
use dx_pattern::compile::Program;
use dx_pattern::dfa::Dfa;
use dx_tree::{Tree, ROOT};
use std::sync::Arc;

/// A built-in query function, by position in the semantic query table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QueryId(pub u16);

/// A query function's handle form, by position in the semantic handle table:
/// rules that carry a value instead of a node read through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HandleId(pub u16);

/// Whether a query function's result propagates unknowns or represents them
/// in its schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uncertainty {
    Strict,
    Explicit,
}

/// What the runtime needs to know about a query function the semantic layer
/// answers: its name (for displays and census labels), its arity, and the
/// handle form it reads a value subject through.
#[derive(Debug)]
pub struct QuerySpec {
    pub name: &'static str,
    pub arity: usize,
    pub uncertainty: Uncertainty,
    pub handle: Option<HandleId>,
}

#[derive(Clone, Debug)]
pub enum CExpr {
    Const(Value<'static>),
    Template(Vec<CExpr>),
    /// A template hole, with its source for display without a node.
    Hole(Box<CExpr>, String),
    List(Vec<CExpr>),
    /// A field of the node itself.
    Field(u16),
    /// The closest ancestor of a matcher, or one of its fields.
    Ancestor(u32, Option<u16>),
    Builtin(Builtin),
    /// A native fact of the node, of a field (a list when it repeats), or of
    /// the parent layer.
    Fact(usize, FactOf),
    Opt(usize),
    Call(&'static Function, Vec<CExpr>),
    QueryCall(QueryId, Option<HandleId>, Vec<CExpr>),
    Not(Box<CExpr>),
    Neg(Box<CExpr>),
    Binary(crate::ops::BinOp, Box<CExpr>, Box<CExpr>),
    If(Box<CExpr>, Box<CExpr>, Box<CExpr>),
    /// Shared across cloned expression trees.
    Test(Box<CExpr>, Arc<Program>),
    TestDfa(Box<CExpr>, Arc<Dfa>),
    /// A `let` attribute of the node itself.
    Attr(u16),
    /// A `let` attribute of the node's field (a list when it repeats).
    FieldAttr(u16, u16),
    /// `parent.field`
    ParentField(u16),
    /// `parent.attr`
    ParentAttr(u16),
    /// `dbg(value, "label")`
    Dbg(Box<CExpr>, String),
    /// `children.x`, `previous.x`, `next.x`: `x` evaluated on those nodes.
    Relative(Relative, Box<CExpr>),
    /// `value is a, b.field, layer l, .lang`: does the node pass one?
    Is(Box<CExpr>, Vec<IsTest>),
    Trait(Box<CExpr>, Quantifier, u64),
    Semantic(Box<CExpr>, SemanticValue),
    /// A rule's own `let` value, by slot.
    Local(u16),
    /// `{ key: value }`
    Record(Vec<(value::Key, CExpr)>),
    /// A lambda parameter, by depth (0 = outermost).
    Param(u16),
    /// `value.key` on a record.
    Key(Box<CExpr>, Box<str>),
    /// `map(list, x => body)` and friends; the body reads the item as the
    /// innermost `Param`, at this depth.
    Each(Box<pipeline::Stage>),
    /// A lexical pipeline value, evaluated once at this parameter depth.
    Bind(Box<CExpr>, u16, Box<CExpr>),
    /// Lazy null-only fallback; the left value is evaluated once.
    Nullish(Box<CExpr>, Box<CExpr>),
    NonEmpty(Box<CExpr>),
    /// `fact(args)` on the node, or on the node the third part gives
    /// (`a.b.fact(args)`): the fact reads its arguments through `facts::args`.
    FactCall(usize, Vec<CExpr>, Option<Box<CExpr>>),
    /// `base.key`: on a record its key; on a node (or nodes) the compiled
    /// read, when `key` names one.
    Member(Box<CExpr>, value::Key, Option<Box<CExpr>>),
    /// `field.builtin`: evaluated on the node(s) in a field of this one
    /// (null without any, a list for several).
    OnField(u16, Box<CExpr>),
    /// `opener.x` / `closer.x`: evaluated on the node's first (`true`: last)
    /// child whose layer is one of these.
    OwnChild(bool, Vec<u32>, Box<CExpr>),
    /// `parent.x`: evaluated on the parent layer.
    OnParent(Box<CExpr>),
    /// `position(offset)`: `{line, column, js_line, js_column}`, 1-based, of
    /// a byte offset.
    Position(Box<CExpr>),
    /// `walk(roots, n => next)`; the body reads the node as the `Param` at
    /// this depth.
    Walk(Box<CExpr>, u16, Box<CExpr>),
    /// `descendants(layer)`: selected nodes under this one, in source order.
    Descendants(NodeSel),
    /// `n.descendants(layer)`: selected nodes under node `n` (none when `n`
    /// is not a node), in source order.
    DescendantsOf(NodeSel, Box<CExpr>),
    /// `overlapping(layer)`: selected nodes anywhere in the file whose span
    /// overlaps this node's, in source order.
    Overlapping(NodeSel, Option<Box<(CExpr, CExpr)>>),
    /// `containing(layer)`: the innermost selected node, in any named layer,
    /// whose span holds this node's (not the node itself).
    Containing(NodeSel),
    /// `kind_name(n)`: the node's kind as text (`.lang` for the file).
    KindName(Box<CExpr>),
    /// `node_at(offset)`: the innermost default-layer node holding it.
    NodeAt(Box<CExpr>),
    /// `token(n, edge)`, `token_list(n)`, `token_pair(n, pairs)`: the
    /// front end's code tokens around a node (pairs: the second expression).
    Token(TokenRead, Box<CExpr>, Option<Box<CExpr>>),
    /// The first value when it is a node, else the second (an `is`
    /// subject: `next_token` is a node read, or the fact's tag).
    Coalesce(Box<CExpr>, Box<CExpr>),
    /// `base.a.b`: records give their keys; a node, at the start or reached
    /// part way, reads the rest of the path (`parts[i]` reads `keys[i..]`).
    Path(Box<CExpr>, Vec<value::Key>, Vec<Option<CExpr>>),
    /// A field when the node has one by that label, else this (`parent.x`
    /// where `x` names both a field and a fact).
    FieldElse(u16, Box<CExpr>),
    /// `matcher.x`: evaluated on the closest ancestor of that matcher.
    OnAncestor(u32, Box<CExpr>),
    /// A config `fn`, by index in the grammar's functions, with its arguments.
    UserCall(u32, Vec<CExpr>),
}

/// `for_each_child` and its `mut` twin: one list of which forms hold which
/// subexpressions.
macro_rules! child_visitor {
    ($name:ident $(, $m:ident)?) => {
        /// Calls `f` on each direct subexpression, in evaluation order.
        pub fn $name<'e>(&'e $($m)? self, f: &mut impl FnMut(&'e $($m)? CExpr)) {
            match self {
                CExpr::Template(items) | CExpr::List(items) | CExpr::Call(_, items)
                | CExpr::UserCall(_, items) => {
                    for item in items { f(item) }
                }
                CExpr::QueryCall(_, _, items) => {
                    for item in items { f(item) }
                }
                CExpr::FactCall(_, items, receiver) => {
                    for item in items { f(item) }
                    if let Some(receiver) = receiver { f(receiver) }
                }
                CExpr::Hole(inner, _) | CExpr::Not(inner) | CExpr::Neg(inner) | CExpr::NonEmpty(inner)
                | CExpr::Test(inner, _) | CExpr::TestDfa(inner, _) | CExpr::Dbg(inner, _)
                | CExpr::Relative(_, inner) | CExpr::Is(inner, _) | CExpr::Trait(inner, _, _) | CExpr::Semantic(inner, _) | CExpr::Key(inner, _)
                | CExpr::OnField(_, inner) | CExpr::OwnChild(_, _, inner) | CExpr::OnParent(inner)
                | CExpr::Position(inner) | CExpr::KindName(inner) | CExpr::NodeAt(inner)
                | CExpr::FieldElse(_, inner) | CExpr::OnAncestor(_, inner) | CExpr::Token(_, inner, None)
                | CExpr::DescendantsOf(_, inner) => f(inner),
                CExpr::Token(_, inner, Some(pairs)) => {
                    f(inner);
                    f(pairs);
                }
                CExpr::Binary(_, a, b) | CExpr::Bind(a, _, b) | CExpr::Nullish(a, b) | CExpr::Walk(a, _, b) | CExpr::Coalesce(a, b) => {
                    f(a);
                    f(b);
                }
                CExpr::Each(stage) => {
                    f(& $($m)? stage.input);
                    for arg in & $($m)? stage.args { f(arg); }
                    f(& $($m)? stage.body);
                }
                CExpr::If(a, b, c) => {
                    f(a);
                    f(b);
                    f(c);
                }
                CExpr::Record(entries) => {
                    for (_, value) in entries { f(value) }
                }
                CExpr::Member(base, _, part) => {
                    f(base);
                    if let Some(part) = part { f(part) }
                }
                CExpr::Path(base, _, parts) => {
                    f(base);
                    for part in parts { if let Some(part) = part { f(part) } }
                }
                CExpr::Overlapping(_, range) => {
                    if let Some(range) = range {
                        let (from, to) = & $($m)? **range;
                        f(from);
                        f(to);
                    }
                }
                CExpr::Const(_) | CExpr::Opt(_) | CExpr::Field(_) | CExpr::Ancestor(_, _)
                | CExpr::Builtin(_) | CExpr::Fact(_, _) | CExpr::Attr(_)
                | CExpr::FieldAttr(_, _) | CExpr::ParentField(_) | CExpr::ParentAttr(_)
                | CExpr::Local(_) | CExpr::Param(_)
                | CExpr::Descendants(_) | CExpr::Containing(_) => {}
            }
        }
    };
}

impl CExpr {
    child_visitor!(for_each_child);
    child_visitor!(for_each_child_mut, mut);
}

/// One alternative of an `is` test.
#[derive(Clone, Debug)]
pub enum IsTest {
    /// Of one of these matchers (or a specialisation).
    Kind(Vec<u32>),
    /// In this field of a node of one of these matchers.
    Field(Vec<u32>, u16),
    /// In this named layer.
    Layer(u16),
    /// In a file of this language.
    Lang(usize),
}

/// What [`CExpr::Token`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenRead {
    First,
    Last,
    Before,
    After,
    List,
    Pair,
}

/// A function whose last argument is a lambda run on each item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Each {
    Map,
    Filter,
    Find,
    FindLast,
    All,
    Any,
    FlatMap,
    TakeWhile,
    SkipWhile,
    MinBy,
    MaxBy,
    SortBy,
    GroupBy,
    Partition,
    UniqueBy,
    Zip,
    Fold,
    Reduce,
}

impl Each {
    const NAMES: &'static [(&'static str, Each)] =
        &[
            ("map", Each::Map), ("filter", Each::Filter), ("find", Each::Find), ("find_last", Each::FindLast),
            ("all", Each::All), ("any", Each::Any), ("flat_map", Each::FlatMap),
            ("take_while", Each::TakeWhile), ("skip_while", Each::SkipWhile),
            ("min_by", Each::MinBy), ("max_by", Each::MaxBy), ("sort_by", Each::SortBy),
            ("group_by", Each::GroupBy), ("partition", Each::Partition), ("unique_by", Each::UniqueBy),
            ("zip", Each::Zip), ("fold", Each::Fold), ("reduce", Each::Reduce),
        ];

    pub fn named(name: &str) -> Option<(&'static str, Each)> {
        Each::NAMES.iter().find(|(n, _)| *n == name).copied()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FactOf {
    Node,
    Field(u16),
    Parent,
}

/// Which nodes `descendants`, `overlapping` and `containing` look for:
/// `kind` (or a group), or `kind.field` (what fills that field of such a
/// node, text included: `ts_template.quasi`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeSel {
    kinds: Vec<u32>,
    field: Option<u16>,
}

impl NodeSel {
    /// The matchers selected (specialisations included).
    pub fn kinds(&self) -> &[u32] {
        &self.kinds
    }

    /// A selection of already-expanded matchers.
    pub fn new(kinds: Vec<u32>, field: Option<u16>) -> Self {
        Self { kinds, field }
    }

    pub fn matches(&self, tree: &Tree, id: u32) -> bool {
        let is_a = |m: u32| self.kinds.binary_search(&m).is_ok();
        match self.field {
            None => is_a(tree.node(id).matcher),
            Some(label) => tree.label(id) == Some(label) && tree.owner(id).is_some_and(|o| is_a(tree.node(o).matcher)),
        }
    }

    pub fn descendants<'a>(&self, tree: &'a Tree, node: u32) -> impl Iterator<Item = u32> + 'a {
        let indexed = (!self.kinds.is_empty()).then(|| tree.descendant_nodes(node, &self.kinds, self.field)).flatten();
        // The walk reads what the indexed search records itself
        // ([`Tree::descendant_nodes`]), not each node it passes.
        let fallback = (indexed.is_none() && !self.kinds.is_empty()).then(|| dx_tree::footprint::quiet(|| self.walk(tree, node)));
        indexed.into_iter().flatten().chain(fallback.into_iter().flatten())
    }

    pub fn walk(&self, tree: &Tree, node: u32) -> Vec<u32> {
        let mut found = Vec::new();
        let mut stack: Vec<u32> = tree.children(node).iter().map(|edge| edge.node).collect();
        while let Some(id) = stack.pop() {
            let child = tree.node(id);
            if child.matcher == ROOT { continue; }
            if self.matches(tree, id) { found.push((child.start, std::cmp::Reverse(child.end), id)); }
            stack.extend(tree.children(id).iter().map(|edge| edge.node));
        }
        found.sort_unstable();
        found.into_iter().map(|(_, _, id)| id).collect()
    }
}

/// `position(offset)`: 1-based line and column, by line breaks and by
/// JavaScript line terminators (UTF-16 columns), of a byte offset.
#[derive(Clone, Copy, Debug)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub js_line: usize,
    pub js_column: usize,
    pub js_line_start: usize,
}

impl Position {
    pub const KEYS: [&'static str; 5] = ["line", "column", "js_line", "js_column", "js_line_start"];

    pub fn get(self, key: usize) -> usize {
        [self.line, self.column, self.js_line, self.js_column, self.js_line_start][key]
    }
}

/// Every `let` attribute's name by id, and each name's id: name resolution
/// asks for ids by name once per name read, and configs define thousands.
#[derive(Debug)]
pub struct AttrNames {
    names: Vec<String>,
    ids: dx_core::fx_hash::FxHashMap<String, u16>,
}

impl AttrNames {
    pub const EMPTY: AttrNames = AttrNames { names: Vec::new(), ids: dx_core::fx_hash::FxHashMap::with_hasher(dx_core::fx_hash::FxBuild::new()) };

    /// The names in first-definition order (a repeated name keeps its id).
    pub fn new<'n>(names: impl IntoIterator<Item = &'n str>) -> Self {
        let mut all = AttrNames::EMPTY;
        for name in names {
            if !all.ids.contains_key(name) {
                all.ids.insert(name.to_string(), all.names.len() as u16);
                all.names.push(name.to_string());
            }
        }
        all
    }

    pub fn get(&self, name: &str) -> Option<u16> {
        self.ids.get(name).copied()
    }
}

impl std::ops::Deref for AttrNames {
    type Target = [String];

    fn deref(&self) -> &[String] {
        &self.names
    }
}

/// A node next to the current one, for `children.x`, `previous.x`,
/// `next.x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relative {
    /// Every direct child, in order (a list, nulls kept).
    Children,
    /// The sibling before / after this node in the same field.
    Previous,
    Next,
}

impl Relative {
    pub const NAMES: &'static [(&'static str, Relative)] =
        &[("children", Relative::Children), ("previous", Relative::Previous), ("next", Relative::Next)];

    pub fn named(name: &str) -> Option<Relative> {
        Relative::NAMES.iter().find(|(n, _)| *n == name).map(|(_, r)| *r)
    }
}
