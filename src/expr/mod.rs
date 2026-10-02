//! Formatter expressions: compiled once against a target layer (names
//! resolved to fields, options and builtins), evaluated per node.

pub mod case;
pub mod functions;
mod re;
pub mod value;

use std::collections::HashMap;

use crate::config::ast::{BinOp, Expr, ExprKind, TemplatePart, UnOp};
use crate::config::error::{closest, ConfigError};
use crate::grammar::{full_match, Grammar, Tree};
use crate::line_index::LineIndex;
use crate::pattern::compile::{compile, Labels, Program};
use crate::pattern::dfa::{is_decidable, Dfa};
use functions::Function;
pub use value::Value;

/// Variables every formatter can use.
pub const BUILTIN_VARS: &[(&str, &str)] = &[
    ("text", "the node's text"),
    ("indent", "leading whitespace of the line the node starts on"),
    ("parent_indent", "leading whitespace of the line the node's layer starts on (empty for the file)"),
    ("parent_line", "the whole line the node's layer starts on (empty for the file)"),
    ("indented", "the node's line starts inside the `*` of a layer declared with `in layer indent`"),
    ("indent_anchor", "leading whitespace of the line that innermost `indent` layer opened on (empty when not indented)"),
    ("indent_anchor_line", "that whole opener line (empty when not indented)"),
    ("indent_closes", "this line starts with that layer's end (what follows its `*`), so it sits at the anchor"),
    ("indent_hangs", "that layer's inside starts on its opener line (`f(a,⏎ b)`)"),
    ("line", "1-based line of the node's start"),
    ("col", "1-based column of the node's start"),
    ("file", "the file's path, relative to the project root"),
    ("lang", "the file's language"),
    ("before", "text on the node's first line before the node"),
    ("after", "text on the node's last line after the node"),
    ("prev_line", "the whole line before the node's first line (null on the first line)"),
    ("next_line", "the whole line after the node's last line (null on the last line)"),
    ("parent", "name of the layer the node sits in (`.lang` for the file root)"),
    ("parent_text", "text of the layer the node sits in"),
    ("parent_before", "text of the layer the node sits in, up to the node"),
    ("text_before", "all of the file before the node"),
    ("text_after", "all of the file after the node"),
    ("prev_sibling", "text of the previous node of the same layer in the same parent (null if none)"),
    ("prev_gap", "text between that previous sibling and this node (null if none)"),
    ("layer", "name of the node's own layer"),
    ("enclosing", "innermost layer containing the node's start, looking past overlays (a comment, a string, ...)"),
    ("source", "the whole file's text"),
];

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
    Opt(usize),
    Call(&'static Function, Vec<CExpr>),
    Not(Box<CExpr>),
    Neg(Box<CExpr>),
    Binary(BinOp, Box<CExpr>, Box<CExpr>),
    If(Box<CExpr>, Box<CExpr>, Box<CExpr>),
    Test(Box<CExpr>, Box<Program>),
    TestDfa(Box<CExpr>, Box<Dfa>),
}

/// What names mean inside one formatter.
pub struct Scope<'a> {
    pub grammar: &'a Grammar,
    /// Fields of the target node: label name -> label id.
    pub fields: Vec<(String, u16)>,
    pub options: Vec<String>,
    /// Option values are constants: no node names at all.
    pub options_only: bool,
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
            ExprKind::Var(path) => return self.var(path, e, errors),
            ExprKind::Call(name, args) => {
                let Some(f) = functions::lookup(&name.text) else {
                    errors.push(
                        ConfigError::new(name.span, format!("unknown function `{}`", name.text))
                            .suggest(&name.text, functions::names()),
                    );
                    return None;
                };
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
        if self.options_only {
            errors.push(
                ConfigError::new(first.span, format!("`{}` is not a constant", first.text))
                    .note("option values are fixed when the config loads, so they cannot use names"),
            );
            return None;
        }
        match path {
            [name] => {
                if let Some(i) = self.options.iter().position(|o| *o == name.text) {
                    return Some(CExpr::Opt(i));
                }
                if let Some((_, label)) = self.fields.iter().find(|(f, _)| *f == name.text) {
                    return Some(CExpr::Field(*label));
                }
                if let Some(i) = BUILTIN_VARS.iter().position(|(b, _)| *b == name.text) {
                    return Some(CExpr::Builtin(i));
                }
                if let Some(m) = self.grammar.matcher(&name.text) {
                    return Some(CExpr::Ancestor(m, None));
                }
                let candidates = self
                    .options
                    .iter()
                    .map(String::as_str)
                    .chain(self.fields.iter().map(|(f, _)| f.as_str()))
                    .chain(BUILTIN_VARS.iter().map(|(b, _)| *b));
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
                let Some(m) = self.grammar.matcher(&layer.text) else {
                    errors.push(
                        ConfigError::new(layer.span, format!("unknown matcher `{}`", layer.text))
                            .suggest(&layer.text, self.grammar.matchers.iter().map(|m| m.name.as_str())),
                    );
                    return None;
                };
                let fields = self.grammar.matchers[m as usize].pat.labels();
                if !fields.contains(&field.text) {
                    errors.push(
                        ConfigError::new(field.span, format!("matcher `{}` has no field `{}`", layer.text, field.text))
                            .suggest(&field.text, fields.iter().map(String::as_str)),
                    );
                    return None;
                }
                let label = self.grammar.labels.get(&field.text)?;
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
pub struct Ctx<'a> {
    pub grammar: &'a Grammar,
    pub lang: usize,
    pub tree: &'a Tree,
    pub node: u32,
    pub source: &'a str,
    pub lines: &'a LineIndex,
    pub file: &'a str,
    pub options: &'a [Value<'static>],
}

impl<'a> Ctx<'a> {
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
                Some(id) if !self.tree.is_root(id) => Value::from(self.indent_at(self.tree.node(id).start as usize)),
                _ => Value::from(""),
            },
            "indented" => Value::Bool(self.indent_block().is_some()),
            "indent_anchor" => match self.indent_block() {
                Some((id, _)) => Value::from(self.indent_at(self.tree.node(id).start as usize)),
                None => Value::from(""),
            },
            "indent_anchor_line" => match self.indent_block() {
                Some((id, _)) => Value::from(self.lines.line_text(self.source, self.lines.line_of(self.tree.node(id).start as usize))),
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
                Some(id) if !self.tree.is_root(id) => {
                    Value::from(self.lines.line_text(self.source, self.lines.line_of(self.tree.node(id).start as usize)))
                }
                _ => Value::from(""),
            },
            "line" => Value::from(self.lines.line_of(start) + 1),
            "col" => Value::from(self.lines.line_col(self.source, start).1 + 1),
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
            "prev_line" => match self.lines.line_of(start) {
                0 => Value::Null,
                line => Value::from(self.lines.line_text(self.source, line - 1)),
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
            "prev_sibling" => match self.prev_sibling() {
                Some(id) => Value::from(self.node_text(id)),
                None => Value::Null,
            },
            "prev_gap" => match self.prev_sibling() {
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
        })
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

pub fn binary<'s>(op: BinOp, a: Value<'s>, b: Value<'s>) -> Result<Value<'s>, String> {
    Ok(match op {
        BinOp::And | BinOp::Or => Value::Bool(b.truthy()),
        BinOp::Eq => Value::Bool(a.loose_eq(&b)),
        BinOp::Ne => Value::Bool(!a.loose_eq(&b)),
        BinOp::Lt => Value::Bool(a.compare(&b).is_lt()),
        BinOp::Le => Value::Bool(a.compare(&b).is_le()),
        BinOp::Gt => Value::Bool(a.compare(&b).is_gt()),
        BinOp::Ge => Value::Bool(a.compare(&b).is_ge()),
        BinOp::Add => match (&a, &b) {
            (Value::Num(x), Value::Num(y)) => Value::Num(x + y),
            (Value::List(x), _) => Value::List(x.iter().cloned().chain(b.items()).collect()),
            _ => Value::from(a.text().into_owned() + &b.text()),
        },
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
