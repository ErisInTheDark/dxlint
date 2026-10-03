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
    ("indented", "the node's line starts inside the `until` of a layer declared with `in layer indent`"),
    ("indent_anchor", "leading whitespace of the line that innermost `indent` layer opened on (empty when not indented); lines above the node read as their indentation fixes leave them"),
    ("indent_anchor_line", "that whole opener line (empty when not indented)"),
    ("indent_closes", "this line starts with that layer's end (what follows its `*`), so it sits at the anchor"),
    ("indent_hangs", "that layer's inside starts on its opener line (`f(a,⏎ b)`)"),
    ("line", "1-based line of the node's start"),
    ("column", "1-based column of the node's start"),
    ("offset", "byte offset of the node's start in the file (for `report at`)"),
    ("end_offset", "byte offset of the node's end"),
    ("file", "the file's path, relative to the project root"),
    ("lang", "the file's language"),
    ("before", "text on the node's first line before the node"),
    ("after", "text on the node's last line after the node"),
    ("previous_line", "the whole line before the node's first line (null on the first line)"),
    ("next_line", "the whole line after the node's last line (null on the last line)"),
    ("parent", "name of the layer the node sits in (`.lang` for the file root)"),
    ("parent_text", "text of the layer the node sits in"),
    ("parent_before", "text of the layer the node sits in, up to the node"),
    ("text_before", "all of the file before the node"),
    ("text_after", "all of the file after the node"),
    ("previous_sibling", "text of the previous node of the same layer in the same parent (null if none)"),
    ("previous_gap", "text between that previous sibling and this node (null if none)"),
    ("layer", "name of the node's own layer"),
    ("enclosing", "innermost layer containing the node's start, looking past overlays (a comment, a string, ...)"),
    ("source", "the whole file's text"),
    ("index", "the node's position among its parent's children in the same field, from 0 (`ts_call.arg` 2nd argument: 1); null for the root"),
    ("field", "the field the node fills in its parent (`callee`, `argument`, `left`); null when it has none"),
    ("gap_before", "the text between the previous child of the node's parent (of any layer or field) and the node: blank lines, comments, separators; null for the first child"),
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
    /// `value is a`: does the layer named by value specialise one of these
    /// matchers?
    Is(Box<CExpr>, Vec<u32>),
    /// A rule's own `let` value, by slot.
    Local(u16),
    /// `{ key: value }`
    Record(Vec<(Box<str>, CExpr)>),
    /// A lambda parameter, by depth (0 = outermost).
    Param(u16),
    /// `value.key` on a record.
    Key(Box<CExpr>, Box<str>),
    /// `map(list, x => body)` and friends; the body reads the item as the
    /// innermost `Param`.
    Each(Each, Box<CExpr>, Box<CExpr>),
}

/// Expression forms that are not plain functions, for `zlint functions`.
pub const SPECIAL_FORMS: &[(&str, &str)] = &[
    ("dbg(value, label?)", "the value unchanged; prints `file:line:col layer label = value` to stderr for each node it is evaluated on"),
    ("map(list, x => value)", "the list of `value` for each item `x`"),
    ("filter(list, x => test)", "the items for which `test` is truthy"),
    ("find(list, x => test)", "the first item for which `test` is truthy, or null"),
    ("find_last(list, x => test)", "the last item for which `test` is truthy, or null"),
    ("all(list, x => test)", "whether `test` is truthy for every item"),
    ("any(list, x => test)", "whether `test` is truthy for some item (`any(list)`: some item is truthy)"),
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
    /// Values of the lambda parameters being evaluated, outermost first.
    /// Nothing memoised above can read them: attributes and `let`s are
    /// written outside every lambda.
    params: RefCell<Vec<Value<'a>>>,
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
    /// Where tooling collects how each name was read.
    pub uses: Option<&'a RefCell<Vec<(crate::config::source::Span, Resolved)>>>,
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
            uses: None,
        }
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

    /// What `x` means on a relative node: an attribute, a fact, a builtin or
    /// a field label (any layer's).
    fn relative_part(&self, part: &crate::config::ast::Name, errors: &mut Vec<ConfigError>) -> Option<CExpr> {
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
        errors.push(
            ConfigError::new(part.span, format!("no attribute, fact, builtin or field `{}`", part.text))
                .suggest(&part.text, self.attrs.iter().map(String::as_str).chain(facts::FACTS.iter().map(|f| f.name))),
        );
        None
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
            ExprKind::List(items) => {
                CExpr::List(items.iter().map(|i| self.compile(i, errors)).collect::<Option<Vec<_>>>()?)
            }
            ExprKind::Record(entries) => CExpr::Record(
                entries.iter().map(|(k, v)| Some((k.text.as_str().into(), self.compile(v, errors)?))).collect::<Option<Vec<_>>>()?,
            ),
            ExprKind::Key(base, key) => CExpr::Key(Box::new(self.compile(base, errors)?), key.text.as_str().into()),
            ExprKind::Lambda(param, _) => {
                errors.push(
                    ConfigError::new(param.span, "a lambda (`x => ...`) only goes to `map`, `filter`, `find`, `find_last`, `all` or `any`"),
                );
                return None;
            }
            ExprKind::Call(name, args) if args.len() == 2 && matches!(args[1].kind, ExprKind::Lambda(..)) => {
                let Some((fname, each)) = Each::named(&name.text) else {
                    errors.push(
                        ConfigError::new(name.span, format!("`{}` does not take a lambda", name.text))
                            .note("lambdas go to `map`, `filter`, `find`, `find_last`, `all` and `any`"),
                    );
                    return None;
                };
                self.note(name.span, Resolved::Function(fname));
                let ExprKind::Lambda(param, body) = &args[1].kind else { unreachable!("matched") };
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
                let value = self.compile(value, errors)?;
                let mut ids = Vec::new();
                for name in names {
                    if let Some(m) = self.grammar.matcher(&name.text) {
                        self.note(name.span, Resolved::Layer(name.text.clone()));
                        ids.push(m);
                    } else if let Some(members) = self.grammar.group(&name.text) {
                        self.note(name.span, Resolved::Group(name.text.clone()));
                        for member in members {
                            if let crate::config::ast::Parent::Matcher(n) = member {
                                ids.extend(self.grammar.matcher(&n.text));
                            }
                        }
                    } else if let Some(d) = Dropped::find(&self.grammar.dropped, &name.text) {
                        errors.push(d.still_used(name.span));
                        return None;
                    } else {
                        errors.push(
                            ConfigError::new(name.span, format!("unknown layer or group `{}`", name.text))
                                .suggest(&name.text, self.grammar.matchers.iter().map(|m| m.name.as_str())),
                        );
                        return None;
                    }
                }
                CExpr::Is(Box::new(value), ids)
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
        if let Some(mut value) = value {
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
        } else {
            None
        };
        if let Some(mut value) = own {
            for key in &path[1..] {
                value = CExpr::Key(Box::new(value), key.text.as_str().into());
            }
            return Some(value);
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
                if !self.fields.is_empty() {
                    let fields: Vec<&str> = self.fields.iter().map(|(f, _)| f.as_str()).collect();
                    err = err.note(format!("fields here: {}", fields.join(", ")));
                }
                errors.push(err);
                None
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
                        return Some(CExpr::Fact(fact, FactOf::Parent));
                    }
                    if let Some(label) = self.grammar.labels.get(&field.text) {
                        return Some(CExpr::ParentField(label));
                    }
                    errors.push(ConfigError::new(field.span, format!("no field or attribute `{}`", field.text)));
                    return None;
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
                if !fields.contains(&field.text) {
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
            _ => {
                errors.push(ConfigError::new(e.span, "paths have at most two parts: `matcher.field`"));
                None
            }
        }
    }
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
    /// The rule being applied (its index) and its `let` expressions.
    pub locals: Option<(u32, &'a [CExpr])>,
    /// Facts of the native front end, when `tree` is a native tree.
    pub native: Option<&'a dyn crate::native::ts::facts::Facts>,
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
                _ => Value::List(values),
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
            _ => Value::List(items),
        }
    }

    /// Leading whitespace of the line holding byte `at`.
    fn indent_at(&self, at: usize) -> &'a str {
        let text = self.lines.line_text(self.source, self.lines.line_of(at));
        &text[..text.len() - text.trim_start_matches([' ', '\t']).len()]
    }

    /// The innermost ancestor declared `indent` whose inside (its `*`, or its
    /// whole text without one) opened on an earlier line and still holds the
    /// start of this node's line; with that inside's span.
    fn indent_block(&self) -> Option<(u32, (usize, usize))> {
        let start = self.tree.node(self.node).start as usize;
        let line = self.lines.line_of(start);
        let line_start = self.lines.line_start(line);
        let mut p = self.tree.parent(self.node);
        while let Some(id) = p {
            let n = self.tree.node(id);
            let label = self.tree.label(id);
            let owner = label.and_then(|_| self.tree.owner(id)).map(|o| self.tree.node(o).matcher);
            if !self.tree.is_root(id) && self.grammar.indents(n.matcher, label, owner) {
                let (s, e) = self.tree.hole(id).map_or((n.start as usize, n.end as usize), |(s, e)| (s as usize, e as usize));
                if self.lines.line_of(s) < line && line_start <= e {
                    return Some((id, (s, e)));
                }
            }
            p = self.tree.parent(id);
        }
        None
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
            "indented" => Value::Bool(self.indent_block().is_some()),
            "indent_anchor" => match self.indent_block() {
                Some((id, _)) => self.line_indent(self.lines.line_of(self.tree.node(id).start as usize)),
                None => Value::from(""),
            },
            "indent_anchor_line" => match self.indent_block() {
                Some((id, _)) => self.whole_line(self.lines.line_of(self.tree.node(id).start as usize)),
                None => Value::from(""),
            },
            "indent_closes" => Value::Bool(self.indent_block().is_some_and(|(_, (_, end))| {
                let line_start = self.lines.line_start(self.lines.line_of(start));
                self.lines.line_of(end) == self.lines.line_of(start) && self.source[line_start..end].trim().is_empty()
            })),
            "indent_hangs" => Value::Bool(self.indent_block().is_some_and(|(_, (inside, _))| {
                let line_end = self.lines.line_end(self.source, self.lines.line_of(inside));
                !self.source[inside..line_end.max(inside)].trim().is_empty()
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
            "field" => match self.tree.label(self.node) {
                Some(l) => Value::from(self.grammar.labels.name(l)),
                None => Value::Null,
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
            "lang" => Value::from(self.grammar.langs[self.lang].name.as_str()),
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
                Some(id) if !self.tree.is_root(id) => Value::from(self.grammar.matchers[self.tree.node(id).matcher as usize].name.as_str()),
                _ => Value::from(format!(".{}", self.grammar.langs[self.lang].name)),
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
            "layer" => {
                if self.tree.is_root(self.node) {
                    Value::from(format!(".{}", self.grammar.langs[self.lang].name))
                } else if self.tree.is_text(self.node) {
                    // Labelled text is a field, not a layer.
                    Value::Null
                } else {
                    Value::from(self.grammar.matchers[self.tree.node(self.node).matcher as usize].name.as_str())
                }
            }
            "source" => Value::from(self.source),
            "enclosing" => match self.tree.innermost_at(self.grammar, start as u32, self.node) {
                Some(id) if !self.tree.is_root(id) => Value::from(self.grammar.matchers[self.tree.node(id).matcher as usize].name.as_str()),
                _ => Value::from(format!(".{}", self.grammar.langs[self.lang].name)),
            },
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
                let get = facts::FACTS[*i].get;
                match of {
                    FactOf::Node => get(native, self.node),
                    FactOf::Parent => self.parent_layer().map_or(Value::Null, |p| get(native, p)),
                    FactOf::Field(label) => {
                        let mut values: Vec<Value> = self.tree.fields(self.node, *label).into_iter().map(|id| get(native, id)).collect();
                        match values.len() {
                            0 => Value::Null,
                            1 => values.pop().expect("one"),
                            _ => Value::List(values),
                        }
                    }
                }
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
            CExpr::FieldAttr(label, a) => {
                let mut values = Vec::new();
                for id in self.tree.fields(self.node, *label) {
                    values.push(self.attr(id, *a)?);
                }
                match values.len() {
                    0 => Value::Null,
                    1 => values.pop().expect("one"),
                    _ => Value::List(values),
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
            CExpr::Is(value, ids) => {
                let name = self.eval(value)?;
                let m = match &name {
                    Value::Null => None,
                    v => self.grammar.matcher(&v.text()),
                };
                Value::Bool(m.is_some_and(|m| ids.iter().any(|&t| self.grammar.is_a(m, t))))
            }
            CExpr::Local(i) => {
                let Some((rule, exprs)) = self.locals else { return Err("a rule's `let` outside the rule".into()) };
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
            CExpr::Relative(rel, inner) => match rel {
                Relative::Children => {
                    let kids: Vec<u32> = self.tree.node(self.node).children.iter().map(|e| e.node).collect();
                    let mut values = Vec::with_capacity(kids.len());
                    for id in kids {
                        values.push(Ctx { node: id, ..*self }.eval(inner)?);
                    }
                    Value::List(values)
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
            Value::List(out)
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
