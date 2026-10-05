//! Rule expressions: compiled once against a target layer (names
//! resolved to fields, options and builtins), evaluated per node.

pub mod case;
pub mod facts;
pub mod functions;
pub(crate) mod re;
pub mod value;

use std::cell::RefCell;
use std::collections::HashMap;

use crate::config::ast::{BinOp, Dropped, Expr, ExprKind, TemplatePart, UnOp};
use crate::config::error::{closest, ConfigError};
use crate::grammar::{full_match, Grammar, Tree, ROOT, TEXT};
use crate::line_index::LineIndex;
use crate::pattern::compile::{compile, Labels, Program};
use crate::pattern::dfa::{is_decidable, Dfa};
use functions::Function;
pub use value::Value;

/// Variables every rule can use.
pub const BUILTIN_VARS: &[(&str, &str)] = &[
    ("text", "the node's text"),
    ("indent", "leading whitespace of the line the node starts on"),
    ("parent_indent", "leading whitespace of the line the node's layer starts on (empty for the file)"),
    ("parent_line", "the whole line the node's layer starts on (empty for the file)"),
    ("indented", "one of the rule's `indent` lines applies to the node's line (the innermost node it names, walking out from the line's first character)"),
    ("indent_level", "levels that `indent` line gives the line past its anchor (its `by`, default 1; 0 on an `in` form's closing line or when not indented)"),
    ("indent_anchor", "leading whitespace of the line that `indent` line measures from: `indent in X` X's line, `indent X` X's owner's line (empty when not indented); lines above read as their indentation fixes leave them"),
    ("indent_anchor_line", "that whole anchor line (empty when not indented)"),
    ("indent_closes", "the line starts with the end of an `indent in X` inside (its closer), so it sits at the anchor"),
    ("indent_hangs", "that `indent in X` inside starts on its opener line (`f(a,⏎ b)`)"),
    ("line", "1-based line of the node's start"),
    ("column", "1-based column of the node's start"),
    ("offset", "byte offset of the node's start in the file (for `report at`)"),
    ("end_offset", "byte offset of the node's end"),
    ("file", "the file's path, relative to the project root"),
    ("before", "text on the node's first line before the node"),
    ("after", "text on the node's last line after the node"),
    ("previous_line", "the whole line before the node's first line (null on the first line)"),
    ("next_line", "the whole line after the node's last line (null on the last line)"),
    ("parent", "the node this one sits in, within its layer (the file for top nodes; null for the file); test it with `parent is a`"),
    ("parent_text", "text of the layer the node sits in"),
    ("parent_before", "text of the layer the node sits in, up to the node"),
    ("text_before", "all of the file before the node"),
    ("text_after", "all of the file after the node"),
    ("previous_sibling", "text of the previous node of the same layer in the same parent (null if none)"),
    ("previous_gap", "text between that previous sibling and this node (null if none)"),
    ("source", "the whole file's text"),
    ("index", "the node's position among its parent's children in the same field, from 0 (`ts_call.arg` 2nd argument: 1); null for the root"),
    ("gap_before", "the text between the previous child of the node's parent (of any layer or field) and the node: blank lines, comments, separators; null for the first child"),
    ("node", "the node itself as a node value (reads as its text): for `walk`, `report at`, and `.x` reads later (`right.node`, `parent.node`)"),
];

/// A built-in variable or native fact of that name.
pub fn is_builtin(name: &str) -> bool {
    BUILTIN_VARS.iter().any(|(b, _)| *b == name) || facts::fact(name).is_some()
}

#[derive(Debug)]
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
    Builtin(usize),
    /// A native fact ([`facts::FACTS`]) of the node, of a field (a list
    /// when it repeats), or of the parent layer.
    Fact(usize, FactOf),
    Opt(usize),
    Call(&'static Function, Vec<CExpr>),
    Not(Box<CExpr>),
    Neg(Box<CExpr>),
    Binary(BinOp, Box<CExpr>, Box<CExpr>),
    If(Box<CExpr>, Box<CExpr>, Box<CExpr>),
    Test(Box<CExpr>, Box<Program>),
    TestDfa(Box<CExpr>, Box<Dfa>),
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
    /// A rule's own `let` value, by slot.
    Local(u16),
    /// `{ key: value }`
    Record(Vec<(value::Key, CExpr)>),
    /// A lambda parameter, by depth (0 = outermost).
    Param(u16),
    /// `value.key` on a record.
    Key(Box<CExpr>, Box<str>),
    /// `map(list, x => body)` and friends; the body reads the item as the
    /// innermost `Param`.
    Each(Each, Box<CExpr>, Box<CExpr>),
    /// `fact(args)` on the node: the fact reads its arguments through
    /// [`facts::args`].
    FactCall(usize, Vec<CExpr>),
    /// A link (`declaration`, `references`): the node(s) it leads to.
    Link(usize),
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
    /// `walk(roots, n => next)`; the body reads the node as the innermost
    /// `Param`.
    Walk(Box<CExpr>, Box<CExpr>),
    /// `descendants(layer)`: selected nodes under this one, in source order.
    Descendants(NodeSel),
    /// `overlapping(layer)`: selected nodes anywhere in the file whose span
    /// overlaps this node's, in source order.
    Overlapping(NodeSel, Option<Box<(CExpr, CExpr)>>),
    /// `containing(layer)`: the innermost selected node, in any named layer,
    /// whose span holds this node's (not the node itself).
    Containing(NodeSel),
    /// `kind_name(n)`: the node's kind as text (`.lang` for the file).
    KindName(Box<CExpr>),
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
    /// A config `fn`, by index in [`Grammar::functions`], with its arguments.
    UserCall(u32, Vec<CExpr>),
    /// `reduce(list, initial, (state, item) => next)`; the body reads the
    /// state and the item as the two innermost `Param`s.
    Reduce(Box<CExpr>, Box<CExpr>, Box<CExpr>),
}

/// One alternative of an `is` test.
#[derive(Debug)]
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

/// Expression forms that are not plain functions, for `zlint functions`.
pub const SPECIAL_FORMS: &[(&str, &str)] = &[
    ("dbg(value, label?)", "the value unchanged; prints `file:line:col layer label = value` to stderr for each node it is evaluated on"),
    ("position(offset)", "`{line, column, js_line, js_column}` of a byte offset in the file, 1-based: `line`/`column` like the builtins; `js_line` also counts U+2028/U+2029 as line breaks and `js_column` counts UTF-16 units, as JavaScript tools do; `js_line_start` is the byte offset that JS line starts at"),
    ("kind_name(node)", "a node's kind as text (`ts_call`; `.lang` for the file), for messages: test kinds with `is`"),
    ("containing(layer)", "the innermost node of a layer or group, in any named layer, whose span holds the node's (null if none); `kind.field` selects what fills that field, as `descendants` and `overlapping` also do"),
    ("descendants(layer)", "the nodes of a layer or group (specialisations too) inside the node, in source order"),
    ("overlapping(layer, from?, to?)", "the nodes of a layer or group anywhere in the file whose span overlaps the node's (or the byte range `from..to`), in source order (`ts_line` and the comments or strings on it)"),
    ("walk(roots, n => next)", "the nodes reachable from `roots` (nodes) by `next` (nodes, or a list of them), each once, in visiting order, roots included"),
    ("map(list, x => value)", "the list of `value` for each item `x`"),
    ("filter(list, x => test)", "the items for which `test` is truthy"),
    ("find(list, x => test)", "the first item for which `test` is truthy, or null"),
    ("find_last(list, x => test)", "the last item for which `test` is truthy, or null"),
    ("all(list, x => test)", "whether `test` is truthy for every item"),
    ("any(list, x => test)", "whether `test` is truthy for some item (`any(list)`: some item is truthy)"),
    ("reduce(list, initial, (state, x) => next)", "folds the list in order: `state` starts as `initial` and becomes each `next`"),
];

/// A function whose last argument is a lambda run on each item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Each {
    Map,
    Filter,
    Find,
    FindLast,
    All,
    Any,
}

impl Each {
    const NAMES: &'static [(&'static str, Each)] =
        &[("map", Each::Map), ("filter", Each::Filter), ("find", Each::Find), ("find_last", Each::FindLast), ("all", Each::All), ("any", Each::Any)];

    fn named(name: &str) -> Option<(&'static str, Each)> {
        Each::NAMES.iter().find(|(n, _)| *n == name).copied()
    }
}

/// How deep config `fn`s may call each other (and themselves).
const MAX_CALL_DEPTH: u32 = 256;

/// Whether `name(...)` already means something built in: a function, a
/// special form or a fact.
pub fn is_builtin_call(name: &str) -> bool {
    functions::lookup(name).is_some() || Each::named(name).is_some() || matches!(name, "reduce" | "dbg" | "position" | "walk" | "descendants" | "overlapping" | "containing" | "kind_name") || facts::fact(name).is_some()
}

/// A value as `dbg` prints it: strings quoted, lists bracketed.
pub fn show(v: &Value) -> String {
    v.literal()
}

#[derive(Debug, Clone, Copy)]
pub enum FactOf {
    Node,
    Field(u16),
    Parent,
}

/// A `let` attribute: its expression on each layer that defines it.
pub struct Attr {
    pub name: String,
    pub defs: Vec<(u32, CExpr)>,
}

/// Attribute values computed for one file's tree, so each node's attribute
/// is evaluated once however many rules and parents read it. Rules' `let`
/// values are kept here too, per (node, rule, slot).
#[derive(Default)]
pub struct AttrMemo<'a> {
    values: RefCell<HashMap<(u32, u16), Value<'a>>>,
    busy: RefCell<Vec<(u32, u16)>>,
    locals: RefCell<HashMap<(u32, u32, u16), Value<'a>>>,
    /// Native facts read without arguments, per (node, fact): a rule reading
    /// `space_before.spaced` and `space_before.line_break` builds the gap once.
    facts: RefCell<HashMap<(u32, u16), Value<'a>>>,
    /// Values of the lambda parameters being evaluated, outermost first.
    /// Nothing memoised above can read them: attributes and `let`s are
    /// written outside every lambda.
    params: RefCell<Vec<Value<'a>>>,
    /// Config `fn` calls in progress (recursion is bounded).
    calls: std::cell::Cell<u32>,
    /// Where JavaScript lines start (also after U+2028 and U+2029), for
    /// `position(offset).js_line`.
    js_lines: std::cell::OnceCell<Vec<usize>>,
    /// For `overlapping(layer)`: the file's nodes of those matchers by
    /// start, with the running maximum end.
    spans: RefCell<HashMap<NodeSel, std::rc::Rc<LayerSpans>>>,
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
    fn matches(&self, grammar: &Grammar, tree: &Tree, id: u32) -> bool {
        let is_a = |m: u32| m != ROOT && m != TEXT && self.kinds.iter().any(|&k| grammar.is_a(m, k));
        match self.field {
            None => is_a(tree.node(id).matcher),
            Some(label) => tree.label(id) == Some(label) && tree.owner(id).is_some_and(|o| is_a(tree.node(o).matcher)),
        }
    }
}

/// Nodes of some layers sorted by start; `max_end[i]` is the furthest end
/// among the first `i + 1`, so overlap searches stop early.
struct LayerSpans {
    nodes: Vec<(u32, u32, u32)>,
    max_end: Vec<u32>,
}

/// What names mean inside one rule.
#[derive(Clone)]
pub struct Scope<'a> {
    pub grammar: &'a Grammar,
    /// Fields of the target node: label name -> label id.
    pub fields: Vec<(String, u16)>,
    pub options: Vec<String>,
    /// Option values are constants: no node names at all.
    pub options_only: bool,
    /// Names of every `let` attribute, by id.
    pub attrs: &'a [String],
    /// The rule's own `let` values visible here, by slot.
    pub locals: Vec<String>,
    /// Lambda parameters in scope, outermost first.
    pub params: Vec<String>,
    /// The rule compiled (for its options and `let`s in `uses`).
    pub rule: Option<&'a str>,
    /// The `fn` whose body this is: it sees its parameters and nothing of
    /// any node (with `options_only`).
    pub function: Option<&'a str>,
    /// The rule `let` being compiled: readable on other nodes only.
    pub defining: Option<String>,
    /// Where tooling collects how each name was read.
    pub uses: Option<&'a RefCell<Vec<(crate::config::source::Span, Resolved)>>>,
    /// Where layers read by name are collected (`opener.x`, `closer.x`,
    /// `descendants(x)`): token and trivia nodes exist only when needed.
    pub layer_reads: Option<&'a RefCell<Vec<u32>>>,
}

/// How the compiler read a name: what hover, go to definition and
/// references show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// A rule's `let` (rule, name).
    Local(String, String),
    /// A rule's option (rule, name).
    Option(String, String),
    /// A field of the node being checked.
    Field(String),
    Attr(String),
    Builtin(&'static str),
    Fact(&'static str),
    /// A layer: an ancestor, `layer.field`'s layer, or in `is`.
    Layer(String),
    Group(String),
    Relative(&'static str),
    Function(&'static str),
    /// A config `fn`.
    UserFunction(String),
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

    fn named(name: &str) -> Option<Relative> {
        Relative::NAMES.iter().find(|(n, _)| *n == name).map(|(_, r)| *r)
    }
}

impl<'a> Scope<'a> {
    /// Nothing but the grammar: no fields, options, attributes or locals.
    pub fn new(grammar: &'a Grammar) -> Self {
        Scope {
            grammar,
            fields: Vec::new(),
            options: Vec::new(),
            options_only: false,
            attrs: &[],
            locals: Vec::new(),
            params: Vec::new(),
            rule: None,
            function: None,
            defining: None,
            uses: None,
            layer_reads: None,
        }
    }

    /// For a `fn`'s body: its parameters and constants only.
    pub fn function_body(grammar: &'a Grammar, name: &'a str, params: Vec<String>) -> Self {
        Scope { params, function: Some(name), ..Scope::constants(grammar) }
    }

    fn note(&self, span: crate::config::source::Span, what: Resolved) {
        if let Some(uses) = self.uses {
            uses.borrow_mut().push((span, what));
        }
    }

    /// For option values and defaults: constants only.
    pub fn constants(grammar: &'a Grammar) -> Self {
        Scope { options_only: true, ..Scope::new(grammar) }
    }

    /// What `x` means on a relative node: the rule's own `let` (also the one
    /// being defined: `let depth previous.depth + 1`), an attribute, a fact,
    /// a link, a builtin or a field label (any layer's).
    fn relative_part(&self, part: &crate::config::ast::Name, errors: &mut Vec<ConfigError>) -> Option<CExpr> {
        let found = self.node_part(part);
        if found.is_none() {
            errors.push(
                ConfigError::new(part.span, format!("no let, attribute, fact, builtin or field `{}`", part.text))
                    .suggest(&part.text, self.attrs.iter().map(String::as_str).chain(facts::FACTS.iter().map(|f| f.name))),
            );
        }
        found
    }

    /// `x` read on some node, when it names something nodes have.
    fn node_part(&self, part: &crate::config::ast::Name) -> Option<CExpr> {
        let rule = || self.rule.unwrap_or_default().to_string();
        if let Some(i) = self.locals.iter().position(|l| *l == part.text) {
            self.note(part.span, Resolved::Local(rule(), part.text.clone()));
            return Some(CExpr::Local(i as u16));
        }
        if self.defining.as_deref() == Some(part.text.as_str()) {
            self.note(part.span, Resolved::Local(rule(), part.text.clone()));
            return Some(CExpr::Local(self.locals.len() as u16));
        }
        if let Some(i) = facts::link(&part.text) {
            self.note(part.span, Resolved::Fact(facts::LINKS[i].name));
            return Some(CExpr::Link(i));
        }
        if let Some(i) = self.attrs.iter().position(|a| *a == part.text) {
            self.note(part.span, Resolved::Attr(part.text.clone()));
            return Some(CExpr::Attr(i as u16));
        }
        if let Some(i) = facts::fact(&part.text) {
            self.note(part.span, Resolved::Fact(facts::FACTS[i].name));
            return Some(CExpr::Fact(i, FactOf::Node));
        }
        if let Some(i) = BUILTIN_VARS.iter().position(|(b, _)| *b == part.text) {
            self.note(part.span, Resolved::Builtin(BUILTIN_VARS[i].0));
            return Some(CExpr::Builtin(i));
        }
        if let Some(label) = self.grammar.labels.get(&part.text) {
            return Some(CExpr::Field(label));
        }
        None
    }

    /// `base.key`: a record's key, or `key` read on a node.
    fn member(&self, base: CExpr, key: &crate::config::ast::Name) -> CExpr {
        CExpr::Member(Box::new(base), key.text.clone().into(), self.node_part(key).map(Box::new))
    }

    /// `base.a.b...`: on records the keys; on node values the whole rest
    /// read there (`n.callee.object.layer`).
    fn member_path(&self, base: CExpr, keys: &[crate::config::ast::Name]) -> CExpr {
        match keys {
            [] => base,
            [key] => self.member(base, key),
            _ => {
                // The read of `keys[i..]` on a node, for each `i`: a node
                // reached part way through a record (`rec.owner.line`) reads
                // the rest there.
                let parts = (0..keys.len())
                    .map(|i| {
                        let mut quiet = Vec::new();
                        let rest = &keys[i..];
                        let part = if rest.len() == 1 { self.node_part(&rest[0]) } else { self.relative_path(rest, &mut quiet) };
                        part.filter(|_| quiet.is_empty())
                    })
                    .collect();
                CExpr::Path(Box::new(base), keys.iter().map(|k| k.text.clone().into()).collect(), parts)
            }
        }
    }
}

impl Scope<'_> {
    pub fn compile(&self, e: &Expr, errors: &mut Vec<ConfigError>) -> Option<CExpr> {
        Some(match &e.kind {
            ExprKind::Str(s) => CExpr::Const(Value::from(s.clone())),
            ExprKind::Num(n) => CExpr::Const(Value::Num(*n)),
            ExprKind::Bool(b) => CExpr::Const(Value::Bool(*b)),
            ExprKind::Null => CExpr::Const(Value::Null),
            ExprKind::Template(parts) => {
                let mut out = Vec::new();
                for p in parts {
                    out.push(match p {
                        TemplatePart::Lit(s) => CExpr::Const(Value::from(s.clone())),
                        TemplatePart::Expr(e, src) => CExpr::Hole(Box::new(self.compile(e, errors)?), src.clone()),
                    });
                }
                CExpr::Template(out)
            }
            // Lists and records of constants are built once, here, not per
            // node (`contains(["a", "b", ...], text)` in hot rules).
            ExprKind::List(items) => {
                let items = items.iter().map(|i| self.compile(i, errors)).collect::<Option<Vec<_>>>()?;
                match items.iter().map(|i| match i { CExpr::Const(v) => Some(v.clone()), _ => None }).collect::<Option<Vec<_>>>() {
                    Some(values) => CExpr::Const(Value::list(values)),
                    None => CExpr::List(items),
                }
            }
            ExprKind::Record(entries) => {
                let entries = entries.iter().map(|(k, v)| Some((k.text.clone().into(), self.compile(v, errors)?))).collect::<Option<Vec<(value::Key, CExpr)>>>()?;
                match entries.iter().map(|(k, v)| match v { CExpr::Const(v) => Some((k.clone(), v.clone())), _ => None }).collect::<Option<Vec<_>>>() {
                    Some(values) => CExpr::Const(Value::record(values)),
                    None => CExpr::Record(entries),
                }
            }
            // `first(declarations).offset`: a record's key, or a read on nodes.
            // `(x).callee.node`: the whole key chain is one path read, as
            // `x.callee.node` would be (`.callee` alone reads the field's
            // value, which has no `.node`).
            ExprKind::Key(..) => {
                let mut keys = Vec::new();
                let mut at = e;
                while let ExprKind::Key(base, key) = &at.kind {
                    keys.push(key.clone());
                    at = base;
                }
                keys.reverse();
                self.member_path(self.compile(at, errors)?, &keys)
            }
            ExprKind::Lambda(..) => {
                errors.push(ConfigError::new(
                    e.span,
                    "a lambda only goes to `map`, `filter`, `find`, `find_last`, `all`, `any` (`x => ...`) or `reduce` (`(state, x) => ...`)",
                ));
                return None;
            }
            // `reduce(list, initial, (state, item) => next)`.
            ExprKind::Call(name, args) if name.text == "reduce" => {
                let lambda = match args.as_slice() {
                    [_, _, Expr { kind: ExprKind::Lambda(params, body), .. }] if params.len() == 2 => Some((params, body)),
                    _ => None,
                };
                let Some((params, body)) = lambda else {
                    errors.push(ConfigError::new(e.span, "`reduce` takes a list, an initial value and a lambda").note("usage: reduce(list, initial, (state, x) => next)"));
                    return None;
                };
                self.note(name.span, Resolved::Function("reduce"));
                let list = self.compile(&args[0], errors)?;
                let initial = self.compile(&args[1], errors)?;
                let mut inner = self.clone();
                inner.params.extend(params.iter().map(|p| p.text.clone()));
                let body = inner.compile(body, errors)?;
                CExpr::Reduce(Box::new(list), Box::new(initial), Box::new(body))
            }
            // `descendants(layer)`: nodes of a layer or group under this one.
            // `overlapping(layer)`: nodes of a layer or group sharing some of
            // this node's span (anywhere in the file).
            // `containing(layer)`: the innermost node of a layer or group, in
            // any named layer, whose span holds this node's.
            ExprKind::Call(name, args) if matches!(name.text.as_str(), "descendants" | "overlapping" | "containing") && self.grammar.function(&name.text).is_none() => {
                // `overlapping(layer, from, to)`: overlapping a byte range instead.
                let (layer_arg, range) = match args.as_slice() {
                    [l] => (Some(l), None),
                    [l, from, to] if name.text == "overlapping" => (Some(l), Some((from, to))),
                    _ => (None, None),
                };
                // `kind` or `kind.field`.
                let (layer, field) = match layer_arg {
                    Some(Expr { kind: ExprKind::Var(path), .. }) if path.len() <= 2 => (path.first(), path.get(1)),
                    _ => (None, None),
                };
                let label = match field {
                    Some(f) => match self.grammar.labels.get(&f.text) {
                        Some(l) => Some(l),
                        None => {
                            errors.push(ConfigError::new(f.span, format!("no layer has a field `{}`", f.text)));
                            return None;
                        }
                    },
                    None => None,
                };
                let ids = layer.and_then(|l| match self.grammar.matcher(&l.text) {
                    Some(m) => Some(vec![m]),
                    None => Some(
                        self.grammar.group(&l.text)?.iter().filter_map(|p| match p {
                            crate::config::ast::Parent::Matcher(n) => self.grammar.matcher(&n.text),
                            _ => None,
                        }).collect(),
                    ),
                });
                let overlapping = name.text == "overlapping";
                let Some(ids) = ids else {
                    errors.push(
                        ConfigError::new(e.span, format!("`{}` takes a layer or group name, or `name.field`", name.text))
                            .note(format!("usage: {}(ts_comment), {}(ts_template.quasi)", name.text, name.text)),
                    );
                    return None;
                };
                let containing = name.text == "containing";
                self.note(name.span, Resolved::Function(if overlapping { "overlapping" } else if containing { "containing" } else { "descendants" }));
                if let Some(reads) = self.layer_reads {
                    reads.borrow_mut().extend(&ids);
                }
                if let Some(l) = layer {
                    self.note(l.span, if ids.len() == 1 && self.grammar.matcher(&l.text).is_some() { Resolved::Layer(l.text.clone()) } else { Resolved::Group(l.text.clone()) });
                }
                let range = match range {
                    Some((from, to)) => Some(Box::new((self.compile(from, errors)?, self.compile(to, errors)?))),
                    None => None,
                };
                let sel = NodeSel { kinds: ids, field: label };
                if containing {
                    CExpr::Containing(sel)
                } else if overlapping {
                    CExpr::Overlapping(sel, range)
                } else {
                    CExpr::Descendants(sel)
                }
            }
            // `kind_name(n)`: a node's kind as text, for messages.
            ExprKind::Call(name, args) if name.text == "kind_name" && self.grammar.function("kind_name").is_none() => {
                let [arg] = args.as_slice() else {
                    errors.push(ConfigError::new(e.span, "`kind_name` takes one node").note("usage: kind_name(parent)"));
                    return None;
                };
                self.note(name.span, Resolved::Function("kind_name"));
                CExpr::KindName(Box::new(self.compile(arg, errors)?))
            }
            // `walk(roots, n => next)`: every node reachable from the roots.
            ExprKind::Call(name, args) if name.text == "walk" && self.grammar.function("walk").is_none() => {
                let lambda = match args.as_slice() {
                    [_, Expr { kind: ExprKind::Lambda(params, body), .. }] if params.len() == 1 => Some((&params[0], body)),
                    _ => None,
                };
                let Some((param, body)) = lambda else {
                    errors.push(ConfigError::new(e.span, "`walk` takes the start nodes and a lambda giving each node's next nodes").note("usage: walk([] references, n => n.writer)"));
                    return None;
                };
                self.note(name.span, Resolved::Function("walk"));
                let roots = self.compile(&args[0], errors)?;
                let mut inner = self.clone();
                inner.params.push(param.text.clone());
                let body = inner.compile(body, errors)?;
                CExpr::Walk(Box::new(roots), Box::new(body))
            }
            ExprKind::Call(name, args) if args.len() == 2 && matches!(args[1].kind, ExprKind::Lambda(..)) => {
                let Some((fname, each)) = Each::named(&name.text) else {
                    errors.push(
                        ConfigError::new(name.span, format!("`{}` does not take a lambda", name.text))
                            .note("lambdas go to `map`, `filter`, `find`, `find_last`, `all`, `any` and `reduce`"),
                    );
                    return None;
                };
                self.note(name.span, Resolved::Function(fname));
                let ExprKind::Lambda(params, body) = &args[1].kind else { unreachable!("matched") };
                let [param] = params.as_slice() else {
                    errors.push(ConfigError::new(args[1].span, format!("`{fname}` takes a one-parameter lambda: `x => ...`")));
                    return None;
                };
                let list = self.compile(&args[0], errors)?;
                let mut inner = self.clone();
                inner.params.push(param.text.clone());
                let body = inner.compile(body, errors)?;
                CExpr::Each(each, Box::new(list), Box::new(body))
            }
            ExprKind::Call(name, _) if Each::named(&name.text).is_some_and(|(_, e)| e != Each::Any) => {
                errors.push(
                    ConfigError::new(e.span, format!("`{}` takes a list and a lambda", name.text)).note(format!("usage: {}(list, x => ...)", name.text)),
                );
                return None;
            }
            ExprKind::Var(path) => return self.var(path, e, errors),
            // `dbg(value, "label")`: the value, printed per node to stderr.
            ExprKind::Call(name, args) if name.text == "dbg" => {
                if args.is_empty() || args.len() > 2 {
                    errors.push(ConfigError::new(e.span, "`dbg` takes the value to show and an optional label").note("usage: dbg(value, \"label\")"));
                    return None;
                }
                let label = match args.get(1).map(|a| self.compile(a, errors)) {
                    Some(Some(CExpr::Const(Value::Str(s)))) => Some(s.to_string()),
                    Some(Some(_)) => {
                        errors.push(ConfigError::new(args[1].span, "the label of `dbg` must be a plain string"));
                        return None;
                    }
                    Some(None) => return None,
                    None => None,
                };
                self.note(name.span, Resolved::Function("dbg"));
                CExpr::Dbg(Box::new(self.compile(&args[0], errors)?), label.unwrap_or_else(|| "dbg".into()))
            }
            // `position(offset)`: `{line, column}` of a byte offset in the file.
            ExprKind::Call(name, args) if name.text == "position" && self.grammar.function("position").is_none() => {
                let [offset] = args.as_slice() else {
                    errors.push(ConfigError::new(e.span, "`position` takes one byte offset").note("usage: position(end_offset).line"));
                    return None;
                };
                self.note(name.span, Resolved::Function("position"));
                CExpr::Position(Box::new(self.compile(offset, errors)?))
            }
            // A config `fn`.
            ExprKind::Call(name, args) if self.grammar.function(&name.text).is_some() => {
                let i = self.grammar.function(&name.text).expect("checked");
                let arity = self.grammar.functions[i].arity;
                if args.len() != arity {
                    errors.push(ConfigError::new(e.span, format!("`{}` takes {arity} argument(s), got {}", name.text, args.len())));
                    return None;
                }
                self.note(name.span, Resolved::UserFunction(name.text.clone()));
                let args = args.iter().map(|a| self.compile(a, errors)).collect::<Option<Vec<_>>>()?;
                CExpr::UserCall(i as u32, args)
            }
            // `fact(args)`: a native analysis that takes the rule's values.
            ExprKind::Call(name, args) if !self.options_only && functions::lookup(&name.text).is_none() && facts::fact(&name.text).is_some() => {
                let i = facts::fact(&name.text).expect("checked");
                self.note(name.span, Resolved::Fact(facts::FACTS[i].name));
                let args = args.iter().map(|a| self.compile(a, errors)).collect::<Option<Vec<_>>>()?;
                CExpr::FactCall(i, args)
            }
            ExprKind::Call(name, args) => {
                let Some(f) = functions::lookup(&name.text) else {
                    errors.push(
                        ConfigError::new(name.span, format!("unknown function `{}`", name.text))
                            .suggest(&name.text, functions::names()),
                    );
                    return None;
                };
                self.note(name.span, Resolved::Function(f.name));
                if args.len() < f.min || args.len() > f.max {
                    errors.push(
                        ConfigError::new(e.span, format!("`{}` takes {} argument(s), got {}", f.name, arity(f), args.len()))
                            .note(format!("usage: {}", f.signature)),
                    );
                    return None;
                }
                let args = args.iter().map(|a| self.compile(a, errors)).collect::<Option<Vec<_>>>()?;
                // Literal regex patterns are checked when the config loads.
                if f.name.starts_with("re_") {
                    if let Some(CExpr::Const(Value::Str(pattern))) = args.get(1) {
                        if let Err(err) = re::compile(pattern) {
                            errors.push(ConfigError::new(e.span, err));
                            return None;
                        }
                    }
                }
                CExpr::Call(f, args)
            }
            ExprKind::Unary(op, inner) => {
                let inner = Box::new(self.compile(inner, errors)?);
                match op {
                    UnOp::Not => CExpr::Not(inner),
                    UnOp::Neg => CExpr::Neg(inner),
                }
            }
            ExprKind::Binary(op, a, b) => {
                let a = self.compile(a, errors);
                let b = self.compile(b, errors);
                CExpr::Binary(*op, Box::new(a?), Box::new(b?))
            }
            ExprKind::If(c, a, b) => {
                let c = self.compile(c, errors);
                let a = self.compile(a, errors);
                let b = self.compile(b, errors);
                CExpr::If(Box::new(c?), Box::new(a?), Box::new(b?))
            }
            ExprKind::Is(value, names) => {
                // The subject as a node: a path reads its node (`alternate`,
                // `parent`, `previous`, `n`); anything else is a value.
                // Not a node there (a fact's kind tag): the value itself.
                let plain = self.compile(value, errors)?;
                let value = match &value.kind {
                    ExprKind::Var(path) if path.last().is_some_and(|n| n.text != "node") => {
                        let mut with_node = path.clone();
                        with_node.push(crate::config::ast::Name { text: "node".into(), span: value.span });
                        let mut quiet = Vec::new();
                        let as_node = Expr { kind: ExprKind::Var(with_node), span: value.span };
                        match self.compile(&as_node, &mut quiet).filter(|_| quiet.is_empty()) {
                            Some(c) => CExpr::Coalesce(Box::new(c), Box::new(plain)),
                            None => plain,
                        }
                    }
                    _ => plain,
                };
                let mut tests = Vec::new();
                for name in names {
                    use crate::config::ast::IsName;
                    let test = match name {
                        IsName::Kind(n) => IsTest::Kind(self.kinds(n, errors)?),
                        IsName::Field(n, f) => {
                            let Some(label) = self.grammar.labels.get(&f.text) else {
                                errors.push(ConfigError::new(f.span, format!("no layer has a field `{}`", f.text)));
                                return None;
                            };
                            IsTest::Field(self.kinds(n, errors)?, label)
                        }
                        IsName::Layer(l) => match self.grammar.layers.iter().position(|x| *x == l.text) {
                            Some(at) => IsTest::Layer(at as u16),
                            None => {
                                errors.push(
                                    ConfigError::new(l.span, format!("no layer `{}`", l.text)).suggest(&l.text, self.grammar.layers.iter().map(String::as_str)),
                                );
                                return None;
                            }
                        },
                        IsName::Lang(l) => match self.grammar.lang(&l.text) {
                            Some(at) => IsTest::Lang(at),
                            None => {
                                errors.push(ConfigError::new(l.span, format!("unknown language `.{}`", l.text)));
                                return None;
                            }
                        },
                    };
                    tests.push(test);
                }
                CExpr::Is(Box::new(value), tests)
            }
            ExprKind::Test(value, pat) => {
                let value = self.compile(value, errors)?;
                let resolve = |name: &str| self.grammar.pure_pat(name);
                if is_decidable(pat, &resolve) {
                    match Dfa::whole(pat, &resolve) {
                        Ok(dfa) => return Some(CExpr::TestDfa(Box::new(value), Box::new(dfa))),
                        Err(err) => {
                            errors.push(ConfigError::new(e.span, err));
                            return None;
                        }
                    }
                }
                let names: HashMap<String, u32> =
                    self.grammar.matchers.iter().enumerate().map(|(i, m)| (m.name.clone(), i as u32)).collect();
                // Labels inside a test pattern are never read; intern them privately.
                match compile(pat, &names, &mut Labels::default()) {
                    Ok(prog) => CExpr::Test(Box::new(value), Box::new(prog)),
                    Err(err) => {
                        errors.push(ConfigError::new(e.span, err));
                        return None;
                    }
                }
            }
        })
    }

    /// The matchers a kind or group name stands for.
    fn kinds(&self, name: &crate::config::ast::Name, errors: &mut Vec<ConfigError>) -> Option<Vec<u32>> {
        if let Some(m) = self.grammar.matcher(&name.text) {
            self.note(name.span, Resolved::Layer(name.text.clone()));
            return Some(vec![m]);
        }
        if let Some(members) = self.grammar.group(&name.text) {
            self.note(name.span, Resolved::Group(name.text.clone()));
            return Some(
                members
                    .iter()
                    .filter_map(|m| match m {
                        crate::config::ast::Parent::Matcher(n) => self.grammar.matcher(&n.text),
                        _ => None,
                    })
                    .collect(),
            );
        }
        errors.push(match Dropped::find(&self.grammar.dropped, &name.text) {
            Some(d) => d.still_used(name.span),
            None => ConfigError::new(name.span, format!("unknown layer or group `{}`", name.text))
                .suggest(&name.text, self.grammar.matchers.iter().map(|m| m.name.as_str())),
        });
        None
    }

    /// `opener` / `closer` as the head of a path: the group of that name,
    /// when nothing nearer (a field, a layer) has the name.
    fn own_bracket(&self, head: &crate::config::ast::Name) -> bool {
        matches!(head.text.as_str(), "opener" | "closer")
            && !self.fields.iter().any(|(f, _)| *f == head.text)
            && self.grammar.matcher(&head.text).is_none()
            && self.grammar.group(&head.text).is_some()
    }

    fn var(&self, path: &[crate::config::ast::Name], e: &Expr, errors: &mut Vec<ConfigError>) -> Option<CExpr> {
        let first = &path[0];
        // Values holding records: lambda parameters, options and `let`s;
        // `.key` reads into them.
        let rule = || self.rule.unwrap_or_default().to_string();
        let value = if let Some(i) = self.params.iter().rposition(|p| *p == first.text) {
            Some(CExpr::Param(i as u16))
        } else if path.len() > 1 && !self.options_only {
            if let Some(i) = self.locals.iter().position(|l| *l == first.text) {
                self.note(first.span, Resolved::Local(rule(), first.text.clone()));
                Some(CExpr::Local(i as u16))
            } else if let Some(i) = self.options.iter().position(|o| *o == first.text) {
                self.note(first.span, Resolved::Option(rule(), first.text.clone()));
                Some(CExpr::Opt(i))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(value) = value {
            return Some(self.member_path(value, &path[1..]));
        }
        if self.defining.as_deref() == Some(first.text.as_str()) {
            errors.push(
                ConfigError::new(first.span, format!("`{}` refers to itself", first.text))
                    .note(format!("read it on another node: `previous.{0}`, `parent.{0}`", first.text)),
            );
            return None;
        }
        if let Some(function) = self.function {
            errors.push(
                ConfigError::new(first.span, format!("`{}` is not a parameter of `{function}`", first.text))
                    .note("a function computes from its arguments alone: pass in what it needs"),
            );
            return None;
        }
        // Load-time expressions (`@each`) read the rule's options.
        if let (true, Some(i)) = (self.options_only, self.options.iter().position(|o| *o == first.text)) {
            let mut value = CExpr::Opt(i);
            for key in &path[1..] {
                value = CExpr::Key(Box::new(value), key.text.as_str().into());
            }
            return Some(value);
        }
        if self.options_only {
            errors.push(
                ConfigError::new(first.span, format!("`{}` is not a constant", first.text))
                    .note("option values are fixed when the config loads, so they cannot use names"),
            );
            return None;
        }
        // `attr.key`, `fact.key`: a record attribute or fact of the node
        // itself (when the name is not a field, layer or relative).
        let plain = path.len() > 1
            && first.text != "parent"
            && !self.fields.iter().any(|(f, _)| *f == first.text)
            && self.grammar.matcher(&first.text).is_none()
            && Relative::named(&first.text).is_none();
        let own = if !plain {
            None
        } else if let Some(i) = self.attrs.iter().position(|a| *a == first.text) {
            self.note(first.span, Resolved::Attr(first.text.clone()));
            Some(CExpr::Attr(i as u16))
        } else if let Some(i) = facts::fact(&first.text) {
            self.note(first.span, Resolved::Fact(facts::FACTS[i].name));
            Some(CExpr::Fact(i, FactOf::Node))
        } else if let Some(i) = facts::link(&first.text) {
            self.note(first.span, Resolved::Fact(facts::LINKS[i].name));
            Some(CExpr::Link(i))
        } else {
            None
        };
        if let Some(value) = own {
            return Some(self.member_path(value, &path[1..]));
        }
        match path {
            [name] => {
                let rule = || self.rule.unwrap_or_default().to_string();
                if let Some(i) = self.locals.iter().position(|l| *l == name.text) {
                    self.note(name.span, Resolved::Local(rule(), name.text.clone()));
                    return Some(CExpr::Local(i as u16));
                }
                if let Some(i) = self.options.iter().position(|o| *o == name.text) {
                    self.note(name.span, Resolved::Option(rule(), name.text.clone()));
                    return Some(CExpr::Opt(i));
                }
                if let Some((_, label)) = self.fields.iter().find(|(f, _)| *f == name.text) {
                    self.note(name.span, Resolved::Field(name.text.clone()));
                    return Some(CExpr::Field(*label));
                }
                if let Some(i) = self.attrs.iter().position(|a| *a == name.text) {
                    self.note(name.span, Resolved::Attr(name.text.clone()));
                    return Some(CExpr::Attr(i as u16));
                }
                if let Some(i) = BUILTIN_VARS.iter().position(|(b, _)| *b == name.text) {
                    self.note(name.span, Resolved::Builtin(BUILTIN_VARS[i].0));
                    return Some(CExpr::Builtin(i));
                }
                if let Some(i) = facts::fact(&name.text) {
                    self.note(name.span, Resolved::Fact(facts::FACTS[i].name));
                    return Some(CExpr::Fact(i, FactOf::Node));
                }
                if let Some(i) = facts::link(&name.text) {
                    self.note(name.span, Resolved::Fact(facts::LINKS[i].name));
                    return Some(CExpr::Link(i));
                }
                if let Some(m) = self.grammar.matcher(&name.text) {
                    self.note(name.span, Resolved::Layer(name.text.clone()));
                    return Some(CExpr::Ancestor(m, None));
                }
                if let Some(rel) = Relative::named(&name.text) {
                    self.note(name.span, Resolved::Relative(Relative::NAMES.iter().find(|(_, r)| *r == rel).expect("named").0));
                    let text = BUILTIN_VARS.iter().position(|(b, _)| *b == "text").expect("text is a builtin");
                    return Some(CExpr::Relative(rel, Box::new(CExpr::Builtin(text))));
                }
                if let Some(d) = Dropped::find(&self.grammar.dropped, &name.text) {
                    errors.push(d.still_used(name.span));
                    return None;
                }
                let candidates = self
                    .options
                    .iter()
                    .map(String::as_str)
                    .chain(self.fields.iter().map(|(f, _)| f.as_str()))
                    .chain(BUILTIN_VARS.iter().map(|(b, _)| *b))
                    .chain(facts::FACTS.iter().map(|f| f.name));
                let mut err = ConfigError::new(name.span, format!("unknown name `{}`", name.text));
                if let Some(best) = closest(&name.text, candidates) {
                    err = err.note(format!("did you mean `{best}`?"));
                }
                if name.text.contains('-') {
                    err = err.note(format!("`-` inside a name is part of it; subtract with spaces: `{}`", name.text.replacen('-', " - ", 1)));
                }
                if !self.fields.is_empty() {
                    let fields: Vec<&str> = self.fields.iter().map(|(f, _)| f.as_str()).collect();
                    err = err.note(format!("fields here: {}", fields.join(", ")));
                }
                errors.push(err);
                None
            }
            [layer, second, ..] if self.own_bracket(layer) => {
                // `opener.x`, `closer.x` (then `.key`s): on the node's own
                // first child in group `opener` (last in `closer`);
                // `opener.ts_open_brace.x`: only brackets of that layer.
                let members = |name: &str| -> Option<Vec<u32>> {
                    if let Some(m) = self.grammar.matcher(name) {
                        return Some(vec![m]);
                    }
                    Some(self.grammar.group(name)?.iter().filter_map(|m| match m {
                        crate::config::ast::Parent::Matcher(n) => self.grammar.matcher(&n.text),
                        _ => None,
                    }).collect())
                };
                self.note(layer.span, Resolved::Group(layer.text.clone()));
                let (ids, field, keys) = match (members(&second.text), path.get(2)) {
                    (Some(ids), Some(field)) => {
                        self.note(second.span, Resolved::Layer(second.text.clone()));
                        (ids, field, &path[3..])
                    }
                    _ => (members(&layer.text).expect("checked"), second, &path[2..]),
                };
                let inner = self.relative_part(field, errors)?;
                if let Some(reads) = self.layer_reads {
                    reads.borrow_mut().extend(&ids);
                }
                let mut value = CExpr::OwnChild(layer.text == "closer", ids, Box::new(inner));
                for key in keys {
                    value = CExpr::Key(Box::new(value), key.text.as_str().into());
                }
                Some(value)
            }
            [layer, field] => {
                // `field.attr`: an attribute of one of the node's fields.
                let own_field = self.fields.iter().find(|(f, _)| *f == layer.text);
                // `children.x`, `previous.x`, `next.x`.
                if let (Some(rel), None, None) = (Relative::named(&layer.text), own_field, self.grammar.matcher(&layer.text)) {
                    self.note(layer.span, Resolved::Relative(Relative::NAMES.iter().find(|(_, r)| *r == rel).expect("named").0));
                    return Some(CExpr::Relative(rel, Box::new(self.relative_part(field, errors)?)));
                }
                let attr = self.attrs.iter().position(|a| *a == field.text);
                let ancestor_field = self.grammar.matcher(&layer.text).is_some_and(|m| self.grammar.matchers[m as usize].labels().contains(&field.text));
                if let (Some((_, label)), Some(attr), false) = (own_field, attr, ancestor_field) {
                    self.note(layer.span, Resolved::Field(layer.text.clone()));
                    self.note(field.span, Resolved::Attr(field.text.clone()));
                    return Some(CExpr::FieldAttr(*label, attr as u16));
                }
                // `field.fact`: a native fact of one of the node's fields.
                let fact = facts::fact(&field.text);
                if let (Some((_, label)), Some(fact), None) = (own_field, fact, attr) {
                    self.note(layer.span, Resolved::Field(layer.text.clone()));
                    self.note(field.span, Resolved::Fact(facts::FACTS[fact].name));
                    return Some(CExpr::Fact(fact, FactOf::Field(*label)));
                }
                // `field.layer`, `field.offset`: a builtin read on the field's
                // node(s).
                let builtin = BUILTIN_VARS.iter().position(|(b, _)| *b == field.text);
                if let (Some((_, label)), Some(b), None, None, false) = (own_field, builtin, attr, fact, ancestor_field) {
                    self.note(layer.span, Resolved::Field(layer.text.clone()));
                    self.note(field.span, Resolved::Builtin(BUILTIN_VARS[b].0));
                    return Some(CExpr::OnField(*label, Box::new(CExpr::Builtin(b))));
                }
                // `field.link`, `field.its_field`: anything else read there.
                if let (Some((_, label)), false) = (own_field, ancestor_field) {
                    if let Some(part) = self.node_part(field) {
                        self.note(layer.span, Resolved::Field(layer.text.clone()));
                        return Some(CExpr::OnField(*label, Box::new(part)));
                    }
                }
                // `parent.x`: a field, attribute or fact of the node's
                // parent layer.
                if layer.text == "parent" && self.grammar.matcher("parent").is_none() {
                    self.note(layer.span, Resolved::Builtin("parent"));
                    if let Some(a) = attr {
                        self.note(field.span, Resolved::Attr(field.text.clone()));
                        return Some(CExpr::ParentAttr(a as u16));
                    }
                    if let Some(fact) = fact {
                        self.note(field.span, Resolved::Fact(facts::FACTS[fact].name));
                        // A field of that name wins where the parent has one.
                        if let Some(label) = self.grammar.labels.get(&field.text) {
                            return Some(CExpr::OnParent(Box::new(CExpr::FieldElse(label, Box::new(CExpr::Fact(fact, FactOf::Node))))));
                        }
                        return Some(CExpr::Fact(fact, FactOf::Parent));
                    }
                    if let Some(label) = self.grammar.labels.get(&field.text) {
                        return Some(CExpr::ParentField(label));
                    }
                    // Builtins and links (`parent.offset`, `parent.scope`).
                    if let Some(part) = self.node_part(field) {
                        return Some(CExpr::OnParent(Box::new(part)));
                    }
                    errors.push(ConfigError::new(field.span, format!("no field or attribute `{}`", field.text)));
                    return None;
                }
                // `node.parent`, `declaration.x`: read on that value.
                if self.grammar.matcher(&layer.text).is_none() && own_field.is_none() {
                    if let Some(value) = self.node_part(layer) {
                        return Some(self.member(value, field));
                    }
                }
                let Some(m) = self.grammar.matcher(&layer.text) else {
                    let dropped = Dropped::find(&self.grammar.dropped, &layer.text).or_else(|| Dropped::find(&self.grammar.dropped, &field.text));
                    errors.push(match dropped {
                        Some(d) => d.still_used(e.span),
                        None => ConfigError::new(layer.span, format!("unknown matcher `{}`", layer.text))
                            .suggest(&layer.text, self.grammar.matchers.iter().map(|m| m.name.as_str())),
                    });
                    return None;
                };
                let fields = self.grammar.matchers[m as usize].labels();
                // `ts_function.my_attr`, `ts_class.offset`: anything else a
                // node has, read on the closest ancestor.
                if !fields.contains(&field.text) {
                    if let Some(part) = self.node_part(field).filter(|p| !matches!(p, CExpr::Field(_))) {
                        self.note(layer.span, Resolved::Layer(layer.text.clone()));
                        return Some(CExpr::OnAncestor(m, Box::new(part)));
                    }
                    errors.push(
                        ConfigError::new(field.span, format!("matcher `{}` has no field `{}`", layer.text, field.text))
                            .suggest(&field.text, fields.iter().map(String::as_str)),
                    );
                    return None;
                }
                let label = self.grammar.labels.get(&field.text)?;
                self.note(layer.span, Resolved::Layer(layer.text.clone()));
                Some(CExpr::Ancestor(m, Some(label)))
            }
            [first, rest @ ..] => {
                // Longer paths walk: `callee.object.x` (own field, then the
                // rest on its node), `parent.callee.x`, `previous.left.x`,
                // `ts_class.body.x`.
                let inner = self.relative_path(rest, errors)?;
                if let Some((_, label)) = self.fields.iter().find(|(f, _)| *f == first.text) {
                    self.note(first.span, Resolved::Field(first.text.clone()));
                    return Some(CExpr::OnField(*label, Box::new(inner)));
                }
                if first.text == "parent" && self.grammar.matcher("parent").is_none() {
                    self.note(first.span, Resolved::Builtin("parent"));
                    return Some(CExpr::OnParent(Box::new(inner)));
                }
                if let Some(m) = self.grammar.matcher(&first.text) {
                    self.note(first.span, Resolved::Layer(first.text.clone()));
                    return Some(CExpr::OnAncestor(m, Box::new(inner)));
                }
                if let Some(rel) = Relative::named(&first.text) {
                    self.note(first.span, Resolved::Relative(Relative::NAMES.iter().find(|(_, r)| *r == rel).expect("named").0));
                    return Some(CExpr::Relative(rel, Box::new(inner)));
                }
                // `node.callee.x`, `declaration.parent.x`: read on that value.
                if let Some(value) = self.node_part(first) {
                    return Some(self.member_path(value, rest));
                }
                errors.push(ConfigError::new(first.span, format!("unknown name `{}`", first.text)));
                None
            }
            [] => None,
        }
    }

    /// The rest of a path, read on some node: field labels walk into their
    /// nodes; anything else is read there, then `.key`s.
    fn relative_path(&self, parts: &[crate::config::ast::Name], errors: &mut Vec<ConfigError>) -> Option<CExpr> {
        let [first, rest @ ..] = parts else { return None };
        if rest.is_empty() {
            return self.relative_part(first, errors);
        }
        // `n.parent.x`, `n.previous.x`: step to that node, read the rest there.
        if first.text == "parent" && self.grammar.matcher("parent").is_none() {
            return Some(CExpr::OnParent(Box::new(self.relative_path(rest, errors)?)));
        }
        if let Some(rel) = Relative::named(&first.text) {
            return Some(CExpr::Relative(rel, Box::new(self.relative_path(rest, errors)?)));
        }
        match self.node_part(first) {
            Some(CExpr::Field(label)) => Some(CExpr::OnField(label, Box::new(self.relative_path(rest, errors)?))),
            Some(value) => Some(self.member_path(value, rest)),
            None => self.relative_part(first, errors),
        }
    }
}

/// Byte offsets where JavaScript lines start: after `\n`, `\r` (not before
/// `\n`), U+2028 and U+2029.
fn js_line_starts(src: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let b = src.as_bytes();
    for (i, c) in src.char_indices() {
        let end = i + c.len_utf8();
        match c {
            '\n' | '\u{2028}' | '\u{2029}' => starts.push(end),
            '\r' if b.get(end) != Some(&b'\n') => starts.push(end),
            _ => {}
        }
    }
    starts
}

fn arity(f: &Function) -> String {
    if f.min == f.max {
        f.min.to_string()
    } else if f.max > 8 {
        format!("at least {}", f.min)
    } else {
        format!("{} to {}", f.min, f.max)
    }
}

/// Everything an expression can look at while evaluating for one node.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub grammar: &'a Grammar,
    pub lang: usize,
    pub tree: &'a Tree,
    pub node: u32,
    pub source: &'a str,
    pub lines: &'a LineIndex,
    pub file: &'a str,
    pub options: &'a [Value<'static>],
    /// Lines whose indentation a fix already changes: line -> its leading
    /// whitespace after the fix. Builtins reading lines above the node see
    /// these, so lines inside a mis-indented block are measured from where
    /// the block will be, and one mistake is one report.
    pub reindented: &'a RefCell<HashMap<usize, String>>,
    pub attrs: &'a [Attr],
    /// `in a forward x from field`: (matcher, attribute, label).
    pub forwards: &'a [(u32, u16, u16)],
    pub memo: &'a AttrMemo<'a>,
    /// The rule being applied (its index), its `let` expressions and the
    /// layers it targets (empty: not only whole layers).
    pub locals: Option<(u32, &'a [CExpr], &'a [u32])>,
    /// Facts of the native front end, when `tree` is a native tree.
    pub native: Option<&'a dyn crate::native::ts::facts::Facts>,
    /// The applied rule's `indent` lines (`indented`, `indent_level`, ...).
    pub indents: &'a [IndentRule],
}

/// A compiled `indent X by n` / `indent in X by n` rule line.
pub struct IndentRule {
    pub inside: bool,
    /// (matcher, field) pairs; a field means "what fills that field".
    pub targets: Vec<(u32, Option<u16>)>,
    /// Levels, read on the indented node; 1 when absent.
    pub by: Option<CExpr>,
}

/// Where a line's indentation comes from: the node an `indent` line matched,
/// the line it is measured from, and for `in` forms the inside's span.
struct IndentHit {
    node: u32,
    rule: usize,
    anchor_line: usize,
    inside: Option<(usize, usize)>,
}

impl<'a> Ctx<'a> {
    /// Binds the next outermost parameter (a rule's `each` item).
    pub fn push_param(&self, v: Value<'a>) {
        self.memo.params.borrow_mut().push(v);
    }

    pub fn pop_param(&self) {
        self.memo.params.borrow_mut().pop();
    }

    /// `f` without the parameters bound around it: attributes and `let`s
    /// are compiled outside every lambda and `each`, so their own lambdas
    /// count from the bottom of the stack.
    fn unbound<T>(&self, f: impl FnOnce() -> T) -> T {
        let saved = std::mem::take(&mut *self.memo.params.borrow_mut());
        let out = f();
        *self.memo.params.borrow_mut() = saved;
        out
    }

    /// The value of attribute `attr` on `node` (null where no layer of the
    /// node defines it).
    fn attr(&self, node: u32, attr: u16) -> Result<Value<'a>, String> {
        if self.memo.params.borrow().is_empty() {
            self.attr_unbound(node, attr)
        } else {
            self.unbound(|| self.attr_unbound(node, attr))
        }
    }

    fn attr_unbound(&self, node: u32, attr: u16) -> Result<Value<'a>, String> {
        if let Some(v) = self.memo.values.borrow().get(&(node, attr)) {
            return Ok(v.clone());
        }
        let m = self.tree.node(node).matcher;
        let def = &self.attrs[attr as usize];
        let Some((_, expr)) = def.defs.iter().find(|(d, _)| m != ROOT && m != TEXT && self.grammar.is_a(m, *d)) else {
            // A transparent wrapper: the forwarded field's value.
            let forward = self.forwards.iter().find(|&&(f, a, _)| a == attr && m != ROOT && m != TEXT && self.grammar.is_a(m, f));
            let Some(&(_, _, label)) = forward else { return Ok(Value::Null) };
            let mut values = Vec::new();
            for id in self.tree.fields(node, label) {
                values.push(self.attr(id, attr)?);
            }
            return Ok(match values.len() {
                0 => Value::Null,
                1 => values.pop().expect("one"),
                _ => Value::list(values),
            });
        };
        if self.memo.busy.borrow().contains(&(node, attr)) {
            return Err(format!("attribute `{}` depends on itself", def.name));
        }
        self.memo.busy.borrow_mut().push((node, attr));
        let ctx = Ctx { node, options: &[], ..*self };
        let value = ctx.eval(expr);
        self.memo.busy.borrow_mut().pop();
        let value = value?;
        self.memo.values.borrow_mut().insert((node, attr), value.clone());
        Ok(value)
    }

    /// Is `a` an ancestor of `b`?
    fn is_ancestor(&self, a: u32, mut b: u32) -> bool {
        while let Some(p) = self.tree.parent(b) {
            if p == a {
                return true;
            }
            b = p;
        }
        false
    }

    /// The file's nodes of these matchers, indexed once per file.
    fn layer_spans(&self, sel: &NodeSel) -> std::rc::Rc<LayerSpans> {
        if let Some(s) = self.memo.spans.borrow().get(sel) {
            return s.clone();
        }
        let mut nodes = Vec::new();
        let mut stack = vec![self.tree.root];
        while let Some(id) = stack.pop() {
            let n = self.tree.node(id);
            if sel.matches(self.grammar, self.tree, id) {
                nodes.push((n.start, n.end, id));
            }
            stack.extend(n.children.iter().map(|e| e.node));
        }
        nodes.sort_unstable();
        let mut max_end = Vec::with_capacity(nodes.len());
        let mut far = 0;
        for &(s, e, _) in &nodes {
            far = far.max(e.max(s + 1));
            max_end.push(far);
        }
        let spans = std::rc::Rc::new(LayerSpans { nodes, max_end });
        self.memo.spans.borrow_mut().insert(sel.clone(), spans.clone());
        spans
    }

    /// Leading whitespace of `line`, as fixed when it lies above the node.
    fn line_indent(&self, line: usize) -> Value<'a> {
        match self.fixed_indent(line) {
            Some(ws) => Value::from(ws),
            None => Value::from(self.indent_at(self.lines.line_start(line))),
        }
    }

    /// The whole of `line`, with its fixed indentation when it lies above
    /// the node.
    fn whole_line(&self, line: usize) -> Value<'a> {
        let text = self.lines.line_text(self.source, line);
        match self.fixed_indent(line) {
            Some(ws) => Value::from(format!("{ws}{}", text.trim_start_matches([' ', '\t']))),
            None => Value::from(text),
        }
    }

    fn fixed_indent(&self, line: usize) -> Option<String> {
        let own = self.lines.line_of(self.tree.node(self.node).start as usize);
        if line >= own {
            return None;
        }
        self.reindented.borrow().get(&line).cloned()
    }

    fn node_text(&self, id: u32) -> &'a str {
        let n = self.tree.node(id);
        &self.source[n.start as usize..n.end as usize]
    }

    fn field(&self, node: u32, label: u16) -> Value<'a> {
        let mut items: Vec<Value<'a>> = self.tree.fields(node, label).into_iter().map(|id| Value::from(self.node_text(id))).collect();
        match items.len() {
            0 => Value::Null,
            1 => items.pop().expect("one"),
            _ => Value::list(items),
        }
    }

    /// Leading whitespace of the line holding byte `at`.
    fn indent_at(&self, at: usize) -> &'a str {
        let text = self.lines.line_text(self.source, self.lines.line_of(at));
        &text[..text.len() - text.trim_start_matches([' ', '\t']).len()]
    }

    /// The `indent` line deciding this node's line: walking out from the
    /// line's first character, the innermost node one of the rule's `indent`
    /// lines applies to. `indent X` applies when X starts its own line, below
    /// its owner's line; `indent in X` when X's inside (its `*`, or its text
    /// after its first line) opened on an earlier line and holds this one.
    /// On one node a field-qualified line beats a plain one, then the last.
    fn indent_hit(&self) -> Option<IndentHit> {
        if self.indents.is_empty() {
            return None;
        }
        let start = self.tree.node(self.node).start as usize;
        let line = self.lines.line_of(start);
        let line_start = self.lines.line_start(line);
        let first = line_start + self.indent_at(line_start).len();
        let own_line = |at: usize| self.lines.line_start(self.lines.line_of(at)) + self.indent_at(at).len() == at;
        // Start from the node itself unless it is a layer's root.
        let mut p = Some(self.node);
        while let Some(id) = p {
            // From a named layer, indentation is the default layer's: carry
            // on from the node holding the line's first character there.
            if self.tree.layer_roots.iter().any(|&(r, _)| r == id) || self.tree.layer_of(id).is_some() {
                p = self.tree.innermost_at(self.grammar, first as u32, self.node);
                if p == Some(id) {
                    break;
                }
                continue;
            }
            if self.tree.is_root(id) {
                break;
            }
            let n = self.tree.node(id);
            let label = self.tree.label(id);
            let owner = label.and_then(|_| self.tree.owner(id));
            let matches = |&(m, field): &(u32, Option<u16>)| match field {
                None => n.matcher != ROOT && n.matcher != TEXT && self.grammar.is_a(n.matcher, m),
                Some(f) => label == Some(f) && owner.is_some_and(|o| self.grammar.is_a(self.tree.node(o).matcher, m)),
            };
            let best = self
                .indents
                .iter()
                .enumerate()
                .filter_map(|(i, r)| r.targets.iter().filter(|t| matches(t)).map(|t| (t.1.is_some(), i)).max())
                .max();
            if let Some((_, i)) = best {
                let rule = &self.indents[i];
                let (s, e) = (n.start as usize, n.end as usize);
                if rule.inside {
                    let (is, ie) = self.tree.hole(id).map_or((s, e), |(a, b)| (a as usize, b as usize));
                    if self.lines.line_of(is) < line && line_start <= ie {
                        return Some(IndentHit { node: id, rule: i, anchor_line: self.lines.line_of(s), inside: Some((is, ie)) });
                    }
                } else {
                    let anchor = owner.or_else(|| self.tree.parent(id)).filter(|&o| !self.tree.is_root(o)).map(|o| self.tree.node(o).start as usize);
                    if let Some(anchor) = anchor {
                        let anchor_line = self.lines.line_of(anchor);
                        if own_line(s) && anchor_line < self.lines.line_of(s) && self.lines.line_of(s) <= line && line_start <= e {
                            return Some(IndentHit { node: id, rule: i, anchor_line, inside: None });
                        }
                    }
                }
            }
            p = self.tree.parent(id);
        }
        None
    }

    /// This line starts with the end of the `in` form's inside (its closer),
    /// so it sits at the anchor.
    fn indent_closes(&self, hit: &IndentHit) -> bool {
        let Some((_, end)) = hit.inside else { return false };
        let start = self.tree.node(self.node).start as usize;
        let line = self.lines.line_of(start);
        let line_start = self.lines.line_start(line);
        if self.lines.line_of(end) != line {
            return false;
        }
        let first = line_start + self.indent_at(line_start).len();
        // An `until` ends where its closer starts; a node without one ends
        // after its closing brackets.
        match self.tree.hole(hit.node) {
            Some(_) => self.source[line_start..end].trim().is_empty(),
            None => first <= end && self.source[first..end].chars().all(|c| matches!(c, ')' | ']' | '}' | '>')) && first < end,
        }
    }

    /// The layer node this node sits in, looking through labelled text.
    fn parent_layer(&self) -> Option<u32> {
        let mut p = self.tree.parent(self.node);
        while let Some(id) = p.filter(|&id| self.tree.is_text(id)) {
            p = self.tree.parent(id);
        }
        p
    }

    /// The closest earlier node of the same layer under the same parent.
    fn prev_sibling(&self) -> Option<u32> {
        let parent = self.tree.parent(self.node)?;
        let me = self.tree.node(self.node);
        self.tree
            .node(parent)
            .children
            .iter()
            .map(|e| e.node)
            .filter(|&c| c != self.node)
            .filter(|&c| {
                let n = self.tree.node(c);
                n.matcher == me.matcher && n.end <= me.start
            })
            .max_by_key(|&c| self.tree.node(c).end)
    }

    fn builtin(&self, i: usize) -> Value<'a> {
        let start = self.tree.node(self.node).start as usize;
        match BUILTIN_VARS[i].0 {
            "text" => Value::from(self.node_text(self.node)),
            "indent" => Value::from(self.indent_at(start)),
            "parent_indent" => match self.parent_layer() {
                Some(id) if !self.tree.is_root(id) => self.line_indent(self.lines.line_of(self.tree.node(id).start as usize)),
                _ => Value::from(""),
            },
            "indented" => Value::Bool(self.indent_hit().is_some()),
            "indent_level" => match self.indent_hit() {
                Some(hit) if self.indent_closes(&hit) => Value::from(0usize),
                Some(hit) => match &self.indents[hit.rule].by {
                    Some(by) => Ctx { node: hit.node, ..*self }.eval(by).unwrap_or(Value::Null),
                    None => Value::from(1usize),
                },
                None => Value::from(0usize),
            },
            "indent_anchor" => match self.indent_hit() {
                Some(hit) => self.line_indent(hit.anchor_line),
                None => Value::from(""),
            },
            "indent_anchor_line" => match self.indent_hit() {
                Some(hit) => self.whole_line(hit.anchor_line),
                None => Value::from(""),
            },
            "indent_closes" => Value::Bool(self.indent_hit().is_some_and(|hit| self.indent_closes(&hit))),
            "indent_hangs" => Value::Bool(self.indent_hit().is_some_and(|hit| {
                hit.inside.is_some_and(|(inside, _)| {
                    let line_end = self.lines.line_end(self.source, self.lines.line_of(inside));
                    !self.source[inside..line_end.max(inside)].trim().is_empty()
                })
            })),
            "parent_line" => match self.parent_layer() {
                Some(id) if !self.tree.is_root(id) => self.whole_line(self.lines.line_of(self.tree.node(id).start as usize)),
                _ => Value::from(""),
            },
            "index" => match (self.tree.parent(self.node), self.tree.label(self.node)) {
                (Some(p), label) => Value::from(
                    self.tree.node(p).children.iter().take_while(|e| e.node != self.node).filter(|e| Some(e.label) == label).count(),
                ),
                (None, _) => Value::Null,
            },
            "gap_before" => {
                let prev = self.tree.parent(self.node).and_then(|p| {
                    self.tree.node(p).children.iter().map(|e| e.node).filter(|&c| self.tree.node(c).end as usize <= start && c != self.node).max_by_key(|&c| self.tree.node(c).end)
                });
                match prev {
                    Some(c) => Value::from(&self.source[self.tree.node(c).end as usize..start]),
                    None => Value::Null,
                }
            }
            "line" => Value::from(self.lines.line_of(start) + 1),
            "column" => Value::from(self.lines.line_col(self.source, start).1 + 1),
            "offset" => Value::from(start),
            "end_offset" => Value::from(self.tree.node(self.node).end as usize),
            "file" => Value::from(self.file),
            "before" => {
                let line_start = self.lines.line_start(self.lines.line_of(start));
                Value::from(&self.source[line_start..start])
            }
            "after" => {
                let end = self.tree.node(self.node).end as usize;
                // A node ending in (or inside) a line break has nothing
                // after it on its line.
                let line_end = self.lines.line_end(self.source, self.lines.line_of(end));
                if end > start && self.source.as_bytes()[end - 1] == b'\n' || end > line_end {
                    Value::from("")
                } else {
                    Value::from(&self.source[end..line_end])
                }
            }
            "previous_line" => match self.lines.line_of(start) {
                0 => Value::Null,
                line => self.whole_line(line - 1),
            },
            "next_line" => {
                let end = self.tree.node(self.node).end as usize;
                let last = self.lines.line_of(end.saturating_sub(1).max(start));
                if last + 1 < self.lines.line_count() {
                    Value::from(self.lines.line_text(self.source, last + 1))
                } else {
                    Value::Null
                }
            }
            "parent" => match self.parent_layer() {
                Some(id) => Value::Node(id, std::borrow::Cow::Borrowed(self.node_text(id))),
                None => Value::Null,
            },
            "text_before" => Value::from(&self.source[..start]),
            "text_after" => Value::from(&self.source[self.tree.node(self.node).end as usize..]),
            "previous_sibling" => match self.prev_sibling() {
                Some(id) => Value::from(self.node_text(id)),
                None => Value::Null,
            },
            "previous_gap" => match self.prev_sibling() {
                Some(id) => Value::from(&self.source[self.tree.node(id).end as usize..start]),
                None => Value::Null,
            },
            "parent_text" => match self.parent_layer() {
                Some(id) => Value::from(self.node_text(id)),
                None => Value::Null,
            },
            "parent_before" => match self.parent_layer() {
                Some(id) => Value::from(&self.source[self.tree.node(id).start as usize..start]),
                None => Value::Null,
            },
            "source" => Value::from(self.source),
            "node" => Value::Node(self.node, std::borrow::Cow::Borrowed(self.node_text(self.node))),
            other => unreachable!("builtin {other}"),
        }
    }

    pub fn eval(&self, e: &CExpr) -> Result<Value<'a>, String> {
        Ok(match e {
            CExpr::Const(v) => v.clone(),
            CExpr::Hole(inner, _) => self.eval(inner)?,
            CExpr::Template(parts) => {
                let mut s = String::new();
                for p in parts {
                    s.push_str(&self.eval(p)?.text());
                }
                Value::from(s)
            }
            CExpr::List(items) => Value::List(items.iter().map(|i| self.eval(i)).collect::<Result<_, _>>()?),
            CExpr::Field(label) => self.field(self.node, *label),
            CExpr::Ancestor(m, field) => match self.tree.ancestor(self.grammar, self.node, *m) {
                None => Value::Null,
                Some(a) => match field {
                    None => Value::from(self.node_text(a)),
                    Some(label) => self.field(a, *label),
                },
            },
            CExpr::Builtin(i) => self.builtin(*i),
            CExpr::Dbg(inner, label) => {
                let v = self.eval(inner)?;
                let start = self.tree.node(self.node).start as usize;
                let (line, col) = self.lines.line_col(self.source, start);
                let layer = match self.tree.node(self.node).matcher {
                    m if m == crate::grammar::ROOT || m == crate::grammar::TEXT => "",
                    m => self.grammar.matchers[m as usize].name.as_str(),
                };
                eprintln!("{}:{}:{} {layer} {label} = {}", self.file, line + 1, col + 1, show(&v));
                v
            }
            CExpr::Fact(i, of) => {
                let Some(native) = self.native else { return Ok(Value::Null) };
                let get = |native: &dyn crate::native::ts::facts::Facts, id: u32| -> Value<'a> {
                    let key = (id, *i as u16);
                    if let Some(v) = self.memo.facts.borrow().get(&key) {
                        return v.clone();
                    }
                    let v: Value<'a> = (facts::FACTS[*i].get)(native, id);
                    self.memo.facts.borrow_mut().insert(key, v.clone());
                    v
                };
                match of {
                    FactOf::Node => get(native, self.node),
                    FactOf::Parent => self.parent_layer().map_or(Value::Null, |p| get(native, p)),
                    FactOf::Field(label) => {
                        let mut values: Vec<Value> = self.tree.fields(self.node, *label).into_iter().map(|id| get(native, id)).collect();
                        match values.len() {
                            0 => Value::Null,
                            1 => values.pop().expect("one"),
                            _ => Value::list(values),
                        }
                    }
                }
            }
            CExpr::Link(i) => {
                let Some(native) = self.native else { return Ok(Value::Null) };
                let link = facts::LINKS[*i];
                let nodes = (link.get)(native, self.node);
                let node = |id: u32| {
                    let n = self.tree.node(id);
                    Value::Node(id, std::borrow::Cow::Borrowed(&self.source[n.start as usize..n.end as usize]))
                };
                if link.many {
                    Value::list(nodes.into_iter().map(node).collect())
                } else {
                    nodes.first().map_or(Value::Null, |&id| node(id))
                }
            }
            CExpr::Member(base, key, part) => {
                let base = self.eval(base)?;
                let on = |v: &Value<'a>| -> Result<Value<'a>, String> {
                    match (v, part) {
                        (Value::Node(id, _), Some(part)) => Ctx { node: *id, ..*self }.eval(part),
                        (Value::Record(_), _) => Ok(v.key(key)),
                        _ => Ok(Value::Null),
                    }
                };
                match &base {
                    // Nodes in a list (`references.line`): each one's.
                    Value::List(items) if items.iter().any(|i| matches!(i, Value::Node(..))) => {
                        Value::list(items.iter().map(on).collect::<Result<_, _>>()?)
                    }
                    v => on(v)?,
                }
            }
            CExpr::OwnChild(last, ids, inner) => {
                let fits = |e: &&crate::grammar::Edge| ids.iter().any(|&t| self.grammar.is_a(self.tree.node(e.node).matcher, t));
                let children = &self.tree.node(self.node).children;
                let child = if *last { children.iter().rev().find(fits) } else { children.iter().find(fits) };
                match child {
                    Some(e) => Ctx { node: e.node, ..*self }.eval(inner)?,
                    None => Value::Null,
                }
            }
            CExpr::OnField(label, inner) => {
                let mut values = Vec::new();
                for id in self.tree.fields(self.node, *label) {
                    values.push(Ctx { node: id, ..*self }.eval(inner)?);
                }
                match values.len() {
                    0 => Value::Null,
                    1 => values.pop().expect("one"),
                    _ => Value::list(values),
                }
            }
            CExpr::UserCall(i, args) => {
                let f = &self.grammar.functions[*i as usize];
                let Some(body) = f.body.get() else { return Ok(Value::Null) };
                let args = args.iter().map(|a| self.eval(a)).collect::<Result<Vec<_>, _>>()?;
                if self.memo.calls.get() >= MAX_CALL_DEPTH {
                    return Err(format!("`{}` calls itself too deeply", f.name));
                }
                // The body sees its arguments as its only parameters.
                let saved = std::mem::replace(&mut *self.memo.params.borrow_mut(), args);
                self.memo.calls.set(self.memo.calls.get() + 1);
                let v = self.eval(body);
                self.memo.calls.set(self.memo.calls.get() - 1);
                *self.memo.params.borrow_mut() = saved;
                v?
            }
            CExpr::FactCall(i, args) => {
                let Some(native) = self.native else { return Ok(Value::Null) };
                let args = args.iter().map(|a| self.eval(a).map(Value::into_static)).collect::<Result<Vec<_>, _>>()?;
                facts::with_args(args, || (facts::FACTS[*i].get)(native, self.node))
            }
            CExpr::Attr(a) => self.attr(self.node, *a)?,
            CExpr::ParentField(label) => match self.parent_layer() {
                Some(p) => self.field(p, *label),
                None => Value::Null,
            },
            CExpr::ParentAttr(a) => match self.parent_layer() {
                Some(p) => self.attr(p, *a)?,
                None => Value::Null,
            },
            CExpr::Position(offset) => {
                let mut at = (self.eval(offset)?.num()?.max(0.0) as usize).min(self.source.len());
                while !self.source.is_char_boundary(at) {
                    at -= 1;
                }
                let (line, column) = self.lines.line_col(self.source, at);
                let starts = self.memo.js_lines.get_or_init(|| js_line_starts(self.source));
                let js_line = starts.partition_point(|&s| s <= at);
                let js_column = self.source[starts[js_line - 1]..at].encode_utf16().count();
                Value::record(vec![
                    ("line".into(), Value::from(line + 1)),
                    ("column".into(), Value::from(column + 1)),
                    ("js_line".into(), Value::from(js_line)),
                    ("js_column".into(), Value::from(js_column + 1)),
                    ("js_line_start".into(), Value::from(starts[js_line - 1])),
                ])
            }
            CExpr::Descendants(sel) => {
                let mut found = Vec::new();
                let mut stack: Vec<u32> = self.tree.node(self.node).children.iter().map(|e| e.node).collect();
                while let Some(id) = stack.pop() {
                    let n = self.tree.node(id);
                    if n.matcher == ROOT {
                        continue;
                    }
                    if sel.matches(self.grammar, self.tree, id) {
                        found.push((n.start, std::cmp::Reverse(n.end), id));
                    }
                    stack.extend(n.children.iter().map(|e| e.node));
                }
                found.sort_unstable();
                Value::list(found.into_iter().map(|(_, _, id)| Value::Node(id, std::borrow::Cow::Borrowed(self.node_text(id)))).collect())
            }
            CExpr::Coalesce(a, b) => match self.eval(a)? {
                v @ Value::Node(..) => v,
                _ => self.eval(b)?,
            },
            CExpr::KindName(n) => match self.eval(n)? {
                Value::Node(id, _) => match self.tree.node(id).matcher {
                    ROOT => Value::from(format!(".{}", self.grammar.langs[self.lang].name)),
                    TEXT => Value::Null,
                    m => Value::from(self.grammar.matchers[m as usize].name.as_str()),
                },
                _ => Value::Null,
            },
            CExpr::Containing(sel) => {
                let spans = self.layer_spans(sel);
                let me = self.tree.node(self.node);
                let mut i = spans.nodes.partition_point(|&(s, _, _)| s <= me.start);
                let mut best: Option<(u32, u32, u32)> = None;
                while i > 0 && spans.max_end[i - 1] >= me.end {
                    i -= 1;
                    let (s, e, id) = spans.nodes[i];
                    // Smallest span; on a tie the deeper node (a program and
                    // its only class can share one).
                    let better = best.is_none_or(|(bs, be, b)| e - s < be - bs || e - s == be - bs && self.is_ancestor(b, id));
                    if e >= me.end && id != self.node && better {
                        best = Some((s, e, id));
                    }
                }
                match best {
                    Some((_, _, id)) => Value::Node(id, std::borrow::Cow::Borrowed(self.node_text(id))),
                    None => Value::Null,
                }
            }
            CExpr::Overlapping(sel, range) => {
                let spans = self.layer_spans(sel);
                let (start, end) = match range {
                    Some(r) => {
                        // Clamped to the file: out-of-range bounds overlap nothing past it.
                        let len = self.source.len() as f64;
                        let at = |e: &CExpr| self.eval(e).and_then(|v| v.num()).map(|n| n.clamp(0.0, len) as u32);
                        (at(&r.0)?, at(&r.1)?)
                    }
                    None => {
                        let me = self.tree.node(self.node);
                        (me.start, me.end)
                    }
                };
                let end = end.max(start.saturating_add(1));
                // Candidates start before our end; scan back while some
                // earlier node can still reach our start.
                let mut i = spans.nodes.partition_point(|&(s, _, _)| s < end);
                let mut found = Vec::new();
                while i > 0 && spans.max_end[i - 1] > start {
                    i -= 1;
                    let (s, e, id) = spans.nodes[i];
                    if e.max(s + 1) > start && id != self.node {
                        found.push(id);
                    }
                }
                found.reverse();
                Value::list(found.into_iter().map(|id| Value::Node(id, std::borrow::Cow::Borrowed(self.node_text(id)))).collect())
            }
            CExpr::Walk(roots, next) => {
                let mut seen = std::collections::HashSet::new();
                let mut queue: std::collections::VecDeque<Value<'a>> = self.eval(roots)?.items().into();
                let mut out = Vec::new();
                while let Some(v) = queue.pop_front() {
                    let Value::Node(id, _) = &v else { continue };
                    if !seen.insert(*id) {
                        continue;
                    }
                    self.memo.params.borrow_mut().push(v.clone());
                    let more = self.eval(next);
                    self.memo.params.borrow_mut().pop();
                    queue.extend(more?.items());
                    out.push(v);
                }
                Value::list(out)
            }
            CExpr::Path(base, keys, parts) => {
                let base = self.eval(base)?;
                // From key `i` on: a node reads the rest of the path there
                // (`.node` on a node is itself); a record its key.
                let on = |v: &Value<'a>| -> Result<Value<'a>, String> {
                    let mut v = v.clone();
                    for (i, k) in keys.iter().enumerate() {
                        v = match v {
                            Value::Node(..) if &**k == "node" => v,
                            Value::Node(id, _) => {
                                return match &parts[i] {
                                    Some(part) => Ctx { node: id, ..*self }.eval(part),
                                    None => Ok(Value::Null),
                                };
                            }
                            Value::Record(_) => v.key(k),
                            _ => return Ok(Value::Null),
                        };
                    }
                    Ok(v)
                };
                match &base {
                    Value::List(items) if items.iter().any(|i| matches!(i, Value::Node(..))) => {
                        Value::list(items.iter().map(on).collect::<Result<_, _>>()?)
                    }
                    v => on(v)?,
                }
            }
            CExpr::FieldElse(label, other) => match self.tree.fields(self.node, *label).is_empty() {
                false => self.field(self.node, *label),
                true => self.eval(other)?,
            },
            CExpr::OnParent(inner) => match self.parent_layer() {
                Some(p) => Ctx { node: p, ..*self }.eval(inner)?,
                None => Value::Null,
            },
            CExpr::OnAncestor(m, inner) => match self.tree.ancestor(self.grammar, self.node, *m) {
                Some(a) => Ctx { node: a, ..*self }.eval(inner)?,
                None => Value::Null,
            },
            CExpr::FieldAttr(label, a) => {
                let mut values = Vec::new();
                for id in self.tree.fields(self.node, *label) {
                    values.push(self.attr(id, *a)?);
                }
                match values.len() {
                    0 => Value::Null,
                    1 => values.pop().expect("one"),
                    _ => Value::list(values),
                }
            }
            CExpr::Opt(i) => self.options[*i].clone(),
            CExpr::Call(f, args) => {
                // Most calls take few arguments: keep them off the heap.
                let result = if args.len() <= 4 {
                    let mut buf = [Value::Null, Value::Null, Value::Null, Value::Null];
                    for (slot, a) in buf.iter_mut().zip(args) {
                        *slot = self.eval(a)?;
                    }
                    (f.call)(&buf[..args.len()])
                } else {
                    let args = args.iter().map(|a| self.eval(a)).collect::<Result<Vec<_>, _>>()?;
                    (f.call)(&args)
                };
                result.map_err(|e| format!("{}: {e}", f.name))?
            }
            CExpr::Not(inner) => Value::Bool(!self.eval(inner)?.truthy()),
            CExpr::Neg(inner) => Value::Num(-self.eval(inner)?.num()?),
            CExpr::If(c, a, b) => {
                if self.eval(c)?.truthy() {
                    self.eval(a)?
                } else {
                    self.eval(b)?
                }
            }
            CExpr::Binary(op, a, b) => {
                let a = self.eval(a)?;
                // Short-circuit before evaluating the right side.
                match op {
                    BinOp::And if !a.truthy() => return Ok(Value::Bool(false)),
                    BinOp::Or if a.truthy() => return Ok(Value::Bool(true)),
                    _ => {}
                }
                let b = self.eval(b)?;
                binary(*op, a, b)?
            }
            CExpr::Test(value, prog) => Value::Bool(full_match(self.grammar, self.lang, prog, &self.eval(value)?.text())),
            CExpr::TestDfa(value, dfa) => Value::Bool(dfa.is_match(self.eval(value)?.text().as_bytes())),
            CExpr::Is(value, tests) => {
                let v = self.eval(value)?;
                let node = match &v {
                    Value::Node(id, _) => Some(*id),
                    _ => None,
                };
                let is_a = |m: u32, ids: &[u32]| m != ROOT && m != TEXT && ids.iter().any(|&t| self.grammar.is_a(m, t));
                // Facts about things that are not nodes (tokens) tag them with
                // a kind: a kind text, or a record's `kind`.
                let tagged = match &v {
                    Value::Str(_) => self.grammar.matcher(&v.text()),
                    Value::Record(_) => self.grammar.matcher(&v.key("kind").text()),
                    _ => None,
                };
                Value::Bool(tests.iter().any(|t| match (t, node) {
                    (IsTest::Lang(l), _) => *l == self.lang,
                    (IsTest::Kind(ids), None) => tagged.is_some_and(|m| is_a(m, ids)),
                    (_, None) => false,
                    (IsTest::Kind(ids), Some(id)) => is_a(self.tree.node(id).matcher, ids),
                    (IsTest::Field(ids, label), Some(id)) => {
                        self.tree.label(id) == Some(*label) && self.tree.owner(id).is_some_and(|o| is_a(self.tree.node(o).matcher, ids))
                    }
                    (IsTest::Layer(l), Some(id)) => self.tree.layer_of(id) == Some(*l),
                }))
            }
            CExpr::Local(i) => {
                let Some((rule, exprs, targets)) = self.locals else { return Err("a rule's `let` outside the rule".into()) };
                // Read on another node (`previous.rank`): only where the rule
                // could run, so the `let`'s fields exist there.
                let matcher = self.tree.node(self.node).matcher;
                if !targets.is_empty() && !targets.iter().any(|&t| self.grammar.is_a(matcher, t)) {
                    return Ok(Value::Null);
                }
                let key = (self.node, rule, *i);
                if let Some(v) = self.memo.locals.borrow().get(&key) {
                    return Ok(v.clone());
                }
                let v = self.unbound(|| self.eval(&exprs[*i as usize]))?;
                self.memo.locals.borrow_mut().insert(key, v.clone());
                v
            }
            CExpr::Record(entries) => {
                Value::Record(entries.iter().map(|(k, v)| Ok((k.clone(), self.eval(v)?))).collect::<Result<_, String>>()?)
            }
            CExpr::Param(depth) => self.memo.params.borrow().get(*depth as usize).cloned().unwrap_or(Value::Null),
            CExpr::Key(base, key) => self.eval(base)?.key(key),
            CExpr::Each(each, list, body) => {
                let items = self.eval(list)?.items();
                let run = |item: &Value<'a>| -> Result<Value<'a>, String> {
                    self.memo.params.borrow_mut().push(item.clone());
                    let v = self.eval(body);
                    self.memo.params.borrow_mut().pop();
                    v
                };
                each_over(*each, items, run)?
            }
            CExpr::Reduce(list, initial, body) => {
                let mut state = self.eval(initial)?;
                for item in self.eval(list)?.items() {
                    self.memo.params.borrow_mut().extend([state, item]);
                    let next = self.eval(body);
                    {
                        let mut params = self.memo.params.borrow_mut();
                        let bound = params.len() - 2;
                        params.truncate(bound);
                    }
                    state = next?;
                }
                state
            }
            CExpr::Relative(rel, inner) => match rel {
                Relative::Children => {
                    let kids: Vec<u32> = self.tree.node(self.node).children.iter().map(|e| e.node).collect();
                    let mut values = Vec::with_capacity(kids.len());
                    for id in kids {
                        values.push(Ctx { node: id, ..*self }.eval(inner)?);
                    }
                    Value::list(values)
                }
                Relative::Previous | Relative::Next => match self.sibling(*rel == Relative::Next) {
                    Some(id) => Ctx { node: id, ..*self }.eval(inner)?,
                    None => Value::Null,
                },
            },
        })
    }

    /// The child of this node's parent just before (or after) it, among
    /// those with the same field.
    fn sibling(&self, after: bool) -> Option<u32> {
        let parent = self.tree.parent(self.node)?;
        let label = self.tree.label(self.node);
        let same: Vec<u32> = self.tree.node(parent).children.iter().filter(|e| Some(e.label) == label).map(|e| e.node).collect();
        let at = same.iter().position(|&n| n == self.node)?;
        if after {
            same.get(at + 1).copied()
        } else {
            at.checked_sub(1).map(|i| same[i])
        }
    }

    /// Evaluates without a node: constants, options and functions only
    /// (option values, descriptions shown by `zlint rules`).
    pub fn eval_static(e: &CExpr, options: &[Value<'static>]) -> Result<Value<'static>, String> {
        let eval = |e: &CExpr| Ctx::eval_static(e, options);
        Ok(match e {
            CExpr::Const(v) => v.clone(),
            CExpr::Opt(i) => options[*i].clone(),
            CExpr::Hole(inner, _) => eval(inner)?,
            CExpr::Template(parts) => {
                let mut s = String::new();
                for p in parts {
                    s.push_str(&eval(p)?.text());
                }
                Value::from(s)
            }
            CExpr::List(items) => Value::List(items.iter().map(eval).collect::<Result<_, _>>()?),
            CExpr::Record(entries) => Value::Record(entries.iter().map(|(k, v)| Ok((k.clone(), eval(v)?))).collect::<Result<_, String>>()?),
            CExpr::Key(base, key) => eval(base)?.key(key),
            CExpr::Member(base, key, _) => eval(base)?.key(key),
            CExpr::Path(base, keys, _) => keys.iter().fold(eval(base)?, |v, k| v.key(k)),
            CExpr::Call(f, args) => {
                let args = args.iter().map(eval).collect::<Result<Vec<_>, _>>()?;
                (f.call)(&args)?
            }
            CExpr::Not(inner) => Value::Bool(!eval(inner)?.truthy()),
            CExpr::Neg(inner) => Value::Num(-eval(inner)?.num()?),
            CExpr::If(c, a, b) => {
                if eval(c)?.truthy() {
                    eval(a)?
                } else {
                    eval(b)?
                }
            }
            CExpr::Binary(op, a, b) => binary(*op, eval(a)?, eval(b)?)?,
            _ => return Err("needs a node to evaluate".into()),
        })
    }

    /// Text of a template without a node: holes that need one stay as
    /// `{source}`, so `"{name} is {max}"` shows as `{name} is 100`.
    pub fn display(e: &CExpr, options: &[Value<'static>]) -> String {
        match e {
            CExpr::Template(parts) => parts.iter().map(|p| Ctx::display(p, options)).collect(),
            CExpr::Hole(inner, src) => Ctx::eval_static(inner, options).map(|v| v.text().into_owned()).unwrap_or_else(|_| format!("{{{src}}}")),
            other => Ctx::eval_static(other, options).map(|v| v.text().into_owned()).unwrap_or_else(|_| "<dynamic>".into()),
        }
    }
}

/// Runs a lambda (`run`) over `items` for `map` and friends.
fn each_over<'s>(each: Each, items: Vec<Value<'s>>, mut run: impl FnMut(&Value<'s>) -> Result<Value<'s>, String>) -> Result<Value<'s>, String> {
    Ok(match each {
        Each::Map => Value::List(items.iter().map(&mut run).collect::<Result<_, _>>()?),
        Each::Filter => {
            let mut out = Vec::new();
            for item in items {
                if run(&item)?.truthy() {
                    out.push(item);
                }
            }
            Value::list(out)
        }
        Each::Find => {
            for item in items {
                if run(&item)?.truthy() {
                    return Ok(item);
                }
            }
            Value::Null
        }
        Each::FindLast => {
            for item in items.into_iter().rev() {
                if run(&item)?.truthy() {
                    return Ok(item);
                }
            }
            Value::Null
        }
        Each::All => {
            for item in &items {
                if !run(item)?.truthy() {
                    return Ok(Value::Bool(false));
                }
            }
            Value::Bool(true)
        }
        Each::Any => {
            for item in &items {
                if run(item)?.truthy() {
                    return Ok(Value::Bool(true));
                }
            }
            Value::Bool(false)
        }
    })
}

pub fn binary<'s>(op: BinOp, a: Value<'s>, b: Value<'s>) -> Result<Value<'s>, String> {
    Ok(match op {
        BinOp::And | BinOp::Or => Value::Bool(b.truthy()),
        BinOp::Eq => Value::Bool(a.loose_eq(&b)),
        BinOp::Ne => Value::Bool(!a.loose_eq(&b)),
        BinOp::Lt => Value::Bool(a.compare(&b).is_lt()),
        BinOp::Le => Value::Bool(a.compare(&b).is_le()),
        BinOp::Gt => Value::Bool(a.compare(&b).is_gt()),
        BinOp::Ge => Value::Bool(a.compare(&b).is_ge()),
        BinOp::Join => match (&a, &b) {
            (Value::List(_), _) | (_, Value::List(_)) => Value::List(a.items().into_iter().chain(b.items()).collect()),
            _ => Value::from(a.text().into_owned() + &b.text()),
        },
        BinOp::Add => Value::Num(a.num()? + b.num()?),
        BinOp::Sub => Value::Num(a.num()? - b.num()?),
        BinOp::Mul => match (&a, &b) {
            (Value::Str(s), n @ Value::Num(_)) | (n @ Value::Num(_), Value::Str(s)) => {
                let n = n.num()?;
                if !(0.0..=1e6).contains(&n) {
                    return Err("cannot repeat text that many times".into());
                }
                Value::from(s.repeat(n as usize))
            }
            _ => Value::Num(a.num()? * b.num()?),
        },
        BinOp::Rem => {
            let d = b.num()?;
            if d == 0.0 {
                return Err("remainder by zero".into());
            }
            Value::Num(a.num()? % d)
        }
    })
}
