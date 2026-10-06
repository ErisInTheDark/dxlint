//! What every name in a configuration is, where it is defined and where it
//! is used, with a description of each: hover, go to definition, references,
//! completion and outline in the language server, and `zlint describe`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use super::Rule;
use crate::config::ast::{Config, Parent, Target, Use};
use crate::config::source::{FileId, SourceMap, Span};
use crate::expr::{facts, functions, Resolved, BUILTIN_VARS, SPECIAL_FORMS};
use crate::grammar::Grammar;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Rule,
    Layer,
    Group,
    Attr,
    Macro,
    Lang,
    /// A rule's option, named `rule.option`.
    Option,
    /// A rule's `let`, named `rule.name`.
    Local,
    Field,
    Builtin,
    Fact,
    Function,
    Relative,
    Keyword,
    /// An imported file.
    File,
    /// A named option type (`schema name T`).
    Schema,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Rule => "rule",
            Kind::Layer => "layer",
            Kind::Group => "group",
            Kind::Attr => "attribute",
            Kind::Macro => "macro",
            Kind::Lang => "language",
            Kind::Option => "option",
            Kind::Local => "rule value",
            Kind::Field => "field",
            Kind::Builtin => "builtin",
            Kind::Fact => "fact",
            Kind::Function => "function",
            Kind::Relative => "relative",
            Kind::Keyword => "keyword",
            Kind::File => "file",
            Kind::Schema => "schema",
        }
    }
}

pub struct Symbol {
    pub kind: Kind,
    pub name: String,
    /// Where it is defined (a group or attribute can have several lines;
    /// builtins have none).
    pub defs: Vec<Span>,
    /// One line: for completion lists.
    pub summary: String,
    /// Markdown: for hover and `zlint describe`.
    pub doc: String,
    /// What completes after it: a layer's fields (`layer.`), an option's
    /// literal values (`set option `).
    pub fields: Vec<String>,
}

pub struct Symbols {
    pub symbols: Vec<Symbol>,
    /// Every use: (where, symbol).
    pub uses: Vec<(Span, usize)>,
    by_key: HashMap<(Kind, String), usize>,
    pub sources: Arc<SourceMap>,
}

const KEYWORDS: &[(&str, &str)] = &[
    ("in", "`in a, b <item>`: the layers (or `layer.field`s, groups, `.lang`s) the item is about; `in a, b { ... }` for a block"),
    ("layer", "`in a layer name match x pattern`: `x` goes in the named layer, a tree of its own over the same text, searched across `a`'s text without consuming it"),
    ("match", "`match name pattern`: a layer; `in parent match name` attaches an existing one"),
    ("rule", "`in a rule name [severity [message]] [fix as expr] [{ clauses }]` defines a rule; `rule name [severity [message]] [{ set option value; clauses }]` (no `in`) turns an existing one on and configures it; names may glob (`semi:*`)"),
    ("let", "`in a let name expr`: an attribute of `a`'s nodes; inside a rule body, a value of the rule"),
    ("set", "in `rule name { ... }`: `set option value` sets one of the rule's options"),
    ("define", "`define { ... }`: rules inside are defined but off until a `rule name` line turns them on"),
    ("group", "`group name a, b`: adds layers to an open, named list; everything aimed at the group reaches members added later, so libraries can be extended"),
    ("macro", "`macro name tokens`, `macro name(args) { lines }`: pasted where used"),
    ("import", "`import .ts` (a built-in language, its rules defined but off), `import rules:recommended` (a built-in ruleset), `import some/path` (a file, relative to this one): pasted here, once"),
    ("ignore", "`ignore \"glob\" ...`: never lint these files"),
    ("native", "`native ts`: the TypeScript parser's node kinds as `ts_*` layers"),
    ("lang", "`lang name \"*.ext\" ...`: a language, whose root layer is `.name`"),
    ("indent", "rule clause: `indent in a by n` indents what is inside `a` (n levels, default 1) past `a`'s line; `indent a.field by n` indents those nodes themselves when they start their own line; `from x` measures from x's line; an anchor in the middle of a line sits where the indent lines opened earlier on that line put it: levels opened on one line collapse to one, unless their indent lines are in different `class name`es; the rule's `indented`, `indent_level` and `indent_anchor` read these"),
    ("like", "`match name like base pattern`: a specialisation of `base`"),
    ("until", "inside a pattern: lazily skip (letting children eat whole chunks) until the rest matches"),
    ("when", "rule clause: `when expr`; every `when` must hold"),
    ("fix", "rule clause: `fix as expr`, what the node's text should be"),
    ("as", "`fix as expr`"),
    ("option", "rule clause: `option name <type> = <default> \"description\"`, set with `rule name { set option value }`. Types: `string`, `number`, `int`, `boolean`, `any`, `null`, `regex`, literals, `T[]`, `{ key: T }`, `a | b`, `int(0..)`, `T[1..]`, schema names"),
    ("schema", "`schema name <type> \"description\"`: a named option type rules can share"),
    ("files", "rule clause: `files \"glob\" ...`, only these files"),
    ("report", "rule clause: `report node | change | end`, where diagnostics point"),
    ("drop", "`drop rule name`, `drop match x`, `drop in a match x`, `drop let x`, `drop in a let x`, `drop macro m`, `drop group g [members]`, `drop ignore \"glob\"`: remove a definition; whatever still uses it is an error at the `drop`"),
    ("override", "`override <definition>`: replace the existing definition of that name, in its place"),
    ("forward", "`in a forward x, y from field`: where `a` lacks attribute `x`, read it from `field`'s node"),
    ("from", "`forward x from field`"),
    ("is", "`value is a` / `value is (a, b)`: is the layer named by `value` one of these (or a specialisation, or in a group)?"),
    ("not", "`value not a` / `value not (a, b)`: is the layer named by `value` none of these?"),
    ("if", "`if c then a else b`"),
    ("then", "`if c then a else b`"),
    ("else", "`if c then a else b`"),
    ("or", "inside a pattern: alternatives, `(a or b)`"),
    ("off", "severity: the rule does not run"),
    ("hint", "severity"),
    ("info", "severity"),
    ("warn", "severity (the default)"),
    ("error", "severity: fails the run"),
    ("true", "boolean"),
    ("false", "boolean"),
    ("null", "no value: unmatched fields, missing facts"),
];

const RELATIVES: &[(&str, &str)] = &[
    ("children", "every direct child node, in order; `children.x` is the list of `x` over them (nulls kept, so `last(children.x)` is the last child's)"),
    ("previous", "the sibling before this node in the same field (null for the first); `previous.x` reads `x` there, bare `previous` is its text"),
    ("next", "the sibling after this node in the same field (null for the last); `next.x` reads `x` there, bare `next` is its text"),
];

impl Symbols {
    pub fn build(
        config: &Config,
        grammar: &Grammar,
        rules: &[Rule],
        inert: &[super::InertRule],
        resolved: Vec<(Span, Resolved)>,
        sources: Arc<SourceMap>,
    ) -> Symbols {
        let mut s = Symbols { symbols: Vec::new(), uses: Vec::new(), by_key: HashMap::new(), sources };
        s.registry();
        s.layers(config, grammar);
        s.rules(config, rules);
        for r in inert {
            let doc = format!("checks `{}`, **off** (`rule {}` turns it on at {})", r.target_text, r.name, r.default_severity.as_str());
            s.add(Kind::Rule, &r.name, r.target_text.clone(), doc);
            s.def(Kind::Rule, &r.name, r.span);
        }
        s.attrs_and_macros(config);
        for f in &config.functions {
            s.def(Kind::Function, &f.name.text, f.name.span);
        }
        s.parser_uses(config, grammar);
        s.what_reads_layers(config, rules);
        for (span, r) in resolved {
            let key = match r {
                Resolved::Local(rule, name) => (Kind::Local, format!("{rule}.{name}")),
                Resolved::Option(rule, name) => (Kind::Option, format!("{rule}.{name}")),
                Resolved::Field(name) => (Kind::Field, name),
                Resolved::Attr(name) => (Kind::Attr, name),
                Resolved::Builtin(name) => (Kind::Builtin, name.to_string()),
                Resolved::Fact(name) => (Kind::Fact, name.to_string()),
                Resolved::Layer(name) => (Kind::Layer, name),
                Resolved::Group(name) => (Kind::Group, name),
                Resolved::Relative(name) => (Kind::Relative, name.to_string()),
                Resolved::Function(name) => (Kind::Function, name.to_string()),
                Resolved::UserFunction(name) => (Kind::Function, name),
            };
            s.use_of(span, key.0, &key.1);
        }
        // A use can be recorded once per target layer a rule or attribute
        // compiles for.
        s.uses.sort_by_key(|(span, id)| (span.file.0, span.start, span.end, *id));
        s.uses.dedup();
        s
    }

    // ---- lookups ------------------------------------------------------------

    pub fn get(&self, kind: Kind, name: &str) -> Option<usize> {
        self.by_key.get(&(kind, name.to_string())).copied()
    }

    /// Every symbol of that name, whatever its kind.
    pub fn named(&self, name: &str) -> Vec<usize> {
        (0..self.symbols.len()).filter(|&i| self.symbols[i].name == name).collect()
    }

    /// The symbol used or defined at `offset` of `file`: the innermost use,
    /// else a definition.
    pub fn at(&self, file: FileId, offset: u32) -> Option<usize> {
        let inside = |s: &Span| s.file == file && s.start <= offset && offset <= s.end;
        let used = self.uses.iter().filter(|(s, _)| inside(s)).min_by_key(|(s, _)| s.end - s.start).map(|(_, id)| *id);
        used.or_else(|| {
            self.symbols
                .iter()
                .enumerate()
                .filter_map(|(i, sym)| sym.defs.iter().filter(|d| inside(d)).map(|d| (d.end - d.start, i)).min())
                .min()
                .map(|(_, i)| i)
        })
    }

    pub fn uses_of(&self, id: usize) -> impl Iterator<Item = Span> + '_ {
        self.uses.iter().filter(move |(_, i)| *i == id).map(|(s, _)| *s)
    }

    /// `file:line:col` of a span.
    pub fn location(&self, span: Span) -> String {
        let f = self.sources.get(span.file);
        let (line, col) = f.lines.line_col(&f.text, span.start as usize);
        format!("{}:{}:{}", f.name, line + 1, col + 1)
    }

    /// Hover text: kind and name, doc comments, the description, where
    /// it is defined.
    pub fn describe(&self, id: usize) -> String {
        let sym = &self.symbols[id];
        let mut out = format!("**{}** `{}`\n\n", sym.kind.word(), sym.name);
        for d in &sym.defs {
            if let Some(doc) = self.doc_comment(*d) {
                out.push_str(&doc);
                out.push_str("\n\n");
                break;
            }
        }
        out.push_str(&sym.doc);
        if !sym.defs.is_empty() && sym.kind != Kind::File {
            out.push_str("\n\ndefined at ");
            let at: Vec<String> = sym.defs.iter().map(|d| format!("`{}`", self.location(*d))).collect();
            out.push_str(&at.join(", "));
        }
        out
    }

    /// The `#` comment lines right above a definition's line.
    fn doc_comment(&self, span: Span) -> Option<String> {
        let f = self.sources.get(span.file);
        let mut line = f.lines.line_of(span.start as usize);
        let mut lines = Vec::new();
        while line > 0 {
            line -= 1;
            let text = f.lines.line_text(&f.text, line).trim();
            match text.strip_prefix('#') {
                Some(rest) if !rest.trim_start().starts_with("---") => lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string()),
                _ => break,
            }
        }
        if lines.is_empty() {
            return None;
        }
        lines.reverse();
        Some(lines.join("\n"))
    }

    /// The source text of a span (definitions, for hover).
    fn source(&self, span: Span) -> &str {
        let f = self.sources.get(span.file);
        &f.text[span.start as usize..(span.end as usize).min(f.text.len())]
    }

    // ---- building -------------------------------------------------------------

    fn add(&mut self, kind: Kind, name: &str, summary: String, doc: String) -> usize {
        if let Some(&id) = self.by_key.get(&(kind, name.to_string())) {
            return id;
        }
        self.symbols.push(Symbol { kind, name: name.to_string(), defs: Vec::new(), summary, doc, fields: Vec::new() });
        self.by_key.insert((kind, name.to_string()), self.symbols.len() - 1);
        self.symbols.len() - 1
    }

    fn def(&mut self, kind: Kind, name: &str, span: Span) {
        if let Some(&id) = self.by_key.get(&(kind, name.to_string())) {
            if !self.symbols[id].defs.contains(&span) {
                self.symbols[id].defs.push(span);
            }
        }
    }

    fn use_of(&mut self, span: Span, kind: Kind, name: &str) {
        if let Some(&id) = self.by_key.get(&(kind, name.to_string())) {
            self.uses.push((span, id));
        }
    }

    /// A layer list name: a layer, a group or `.lang`.
    fn use_parent(&mut self, p: &Parent) {
        match p {
            Parent::Matcher(n) => {
                let kind = if self.get(Kind::Layer, &n.text).is_some() { Kind::Layer } else { Kind::Group };
                self.use_of(n.span, kind, &n.text);
            }
            Parent::Lang(n) => self.use_of(n.span, Kind::Lang, &n.text),
            Parent::AnyLang(_) => {}
        }
    }

    fn registry(&mut self) {
        for (name, doc) in BUILTIN_VARS {
            self.add(Kind::Builtin, name, doc.to_string(), format!("{doc}."));
        }
        for f in facts::FACTS.iter() {
            self.add(Kind::Fact, f.name, f.doc.to_string(), format!("{}.", f.doc));
        }
        for f in functions::FUNCTIONS {
            self.add(Kind::Function, f.name, f.signature.to_string(), format!("`{}`\n\n{}.", f.signature, f.doc));
        }
        for (sig, doc) in SPECIAL_FORMS {
            let name = sig.split('(').next().unwrap_or(sig);
            self.add(Kind::Function, name, sig.to_string(), format!("`{sig}`\n\n{doc}."));
        }
        for (name, doc) in RELATIVES {
            self.add(Kind::Relative, name, doc.to_string(), format!("{doc}."));
        }
        for (name, doc) in KEYWORDS {
            self.add(Kind::Keyword, name, doc.to_string(), doc.to_string());
        }
    }

    fn layers(&mut self, config: &Config, grammar: &Grammar) {
        for lang in &grammar.langs {
            let doc = format!("the root layer of `{}` files: {}", lang.name, lang.globs.join(" "));
            self.add(Kind::Lang, &lang.name, doc.clone(), doc);
            self.def(Kind::Lang, &lang.name.clone(), lang.span);
        }
        for m in &grammar.matchers {
            let fields = m.fields();
            let mut doc = String::new();
            if let Some(native) = m.native {
                let _ = writeln!(doc, "a node kind of the native `{}` parser", grammar.natives[native.front as usize].name);
            } else if let Some(def) = config.matchers.iter().find(|d| d.name.text == m.name && d.pattern.is_some()) {
                if let Some((_, span)) = &def.pattern {
                    let _ = writeln!(doc, "```zlint\nmatch {} {}\n```", m.name, self.source(*span));
                }
            }
            if let Some(base) = m.like {
                let _ = writeln!(doc, "\nspecialises `{}`: inherits its children and wins ties against it", grammar.matchers[base as usize].name);
            }
            let parents: Vec<String> =
                config.matchers.iter().filter(|d| d.name.text == m.name).flat_map(|d| d.parents.iter().map(Parent::text)).collect();
            if !parents.is_empty() {
                let _ = writeln!(doc, "\nsearched inside: `{}`", parents.join("`, `"));
            }
            let children: Vec<&str> = config
                .matchers
                .iter()
                .filter(|d| d.parents.iter().any(|p| matches!(p, Parent::Matcher(n) if n.text == m.name)))
                .map(|d| d.name.text.as_str())
                .collect();
            if !children.is_empty() {
                let _ = writeln!(doc, "\nchildren: `{}`", children.join("`, `"));
            }
            if !fields.is_empty() {
                let list: Vec<String> = fields
                    .iter()
                    .map(|(f, target)| match target {
                        Some(t) => format!("`{f}` ({t})"),
                        None => format!("`{f}`"),
                    })
                    .collect();
                let _ = writeln!(doc, "\nfields: {}", list.join(", "));
            }
            let summary = if m.native.is_some() { "native layer".to_string() } else { "layer".to_string() };
            let id = self.add(Kind::Layer, &m.name, summary, doc);
            self.symbols[id].fields = fields.iter().map(|(f, _)| f.clone()).collect();
            if m.native.is_none() {
                self.def(Kind::Layer, &m.name.clone(), m.span);
            }
            for (field, target) in fields {
                let id = self.add(Kind::Field, &field, "field".into(), String::new());
                let line = match target {
                    Some(t) => format!("\n- of `{}` ({t})", m.name),
                    None => format!("\n- of `{}`", m.name),
                };
                if self.symbols[id].doc.len() < 4000 {
                    self.symbols[id].doc.push_str(&line);
                }
            }
        }
        for g in &config.groups {
            if self.get(Kind::Group, &g.name.text).is_none() {
                let members: Vec<String> = grammar.group(&g.name.text).unwrap_or_default().iter().map(Parent::text).collect();
                let doc = format!(
                    "members: `{}`\n\nGroups are open: every `group {} ...` line adds members, and everything aimed at the group (children, rules, attributes) reaches members added later. `drop group {} a` removes some.",
                    members.join("`, `"),
                    g.name.text,
                    g.name.text
                );
                self.add(Kind::Group, &g.name.text, format!("group of {}", members.len()), doc);
            }
            self.def(Kind::Group, &g.name.text, g.name.span);
        }
    }

    /// An option of `rule`: `rule.name`, and the default option also under
    /// the rule's own name (what `rule name set ` completes). The first
    /// registration wins: compiled rules carry configured values.
    fn option(&mut self, rule: &str, o: OptionView, schemas: &crate::config::schema::Schemas) {
        let key = format!("{rule}.{}", o.name);
        if self.by_key.contains_key(&(Kind::Option, key.clone())) {
            return;
        }
        let setting = if o.primary { format!("`rule {rule} set value`") } else { format!("`rule {rule} {{ set {} value }}`", o.name) };
        let doc = format!(
            "of rule `{rule}`: {}\n\n```zlint\n{}option {} {}\n```\n\nvalue {}, default {}\n\n{setting} changes it",
            o.description.unwrap_or("no description"),
            if o.primary { "default " } else { "" },
            o.name,
            o.schema,
            crate::expr::show(o.value),
            crate::expr::show(o.default),
        );
        let summary = o.description.map_or_else(|| o.schema.to_string(), String::from);
        let choices: Vec<String> = o.schema.choices(schemas).iter().map(|v| enum_or_literal(o.schema, v)).collect();
        let names = if o.primary { vec![key, rule.to_string()] } else { vec![key] };
        for name in names {
            let id = self.add(Kind::Option, &name, summary.clone(), doc.clone());
            self.symbols[id].fields = choices.clone();
        }
    }

    fn rules(&mut self, config: &Config, rules: &[Rule]) {
        let schemas: crate::config::schema::Schemas = config.schemas.iter().map(|s| (s.name.text.clone(), s.clone())).collect();
        for r in rules {
            let state = if r.severity == crate::config::ast::Severity::Off {
                format!("**off** (`rule {}` turns it on at {})", r.name, r.default_severity.as_str())
            } else {
                format!("on at **{}**", r.severity.as_str())
            };
            let mut doc = format!("checks `{}`, {state}", r.target_text);
            if let Some(m) = r.message_template() {
                let _ = write!(doc, "\n\nmessage: {m}");
            }
            if r.has_fix() {
                doc.push_str("\n\nhas a fix");
            }
            for o in &r.options {
                let changed = if o.value != o.default { format!(" (default {})", crate::expr::show(&o.default)) } else { String::new() };
                let _ = write!(doc, "\n- option `{}`: `{}` = {}{changed}", o.name, o.schema, crate::expr::show(&o.value));
                if let Some(d) = &o.description {
                    let _ = write!(doc, ", {d}");
                }
                self.option(&r.name, OptionView { name: &o.name, schema: &o.schema, value: &o.value, default: &o.default, description: o.description.as_deref(), primary: o.primary }, &schemas);
            }
            self.add(Kind::Rule, &r.name, r.target_text.clone(), doc);
            self.def(Kind::Rule, &r.name, r.span);
        }
        for def in &config.rules {
            // Rules that did not compile (a mistyped `set` line breaks its
            // rule) still complete and document their options, at defaults.
            for o in &def.options {
                let view = OptionView { name: &o.name.text, schema: &o.schema, value: &o.default, default: &o.default, description: o.description.as_deref(), primary: o.primary };
                self.option(&def.name.text, view, &schemas);
            }
            for o in &def.options {
                self.def(Kind::Option, &format!("{}.{}", def.name.text, o.name.text), o.name.span);
            }
            for (name, e) in &def.locals {
                let key = format!("{}.{}", def.name.text, name.text);
                let doc = format!("a value of rule `{}`, computed once per node:\n```zlint\nlet {} {}\n```", def.name.text, name.text, self.source(e.span));
                self.add(Kind::Local, &key, format!("value of {}", def.name.text), doc);
                self.def(Kind::Local, &key, name.span);
            }
            for t in &def.targets {
                match t {
                    Target::Matcher(n) => self.use_parent(&Parent::Matcher(n.clone())),
                    Target::Field(layer, field) => {
                        self.use_parent(&Parent::Matcher(layer.clone()));
                        self.use_of(field.span, Kind::Field, &field.text);
                    }
                    Target::Lang(n) if n.text != "*" => self.use_of(n.span, Kind::Lang, &n.text),
                    Target::Lang(_) => {}
                }
            }
        }
        for c in &config.configs {
            let name = &c.patch.name;
            if !name.text.contains('*') {
                self.use_of(name.span, Kind::Rule, &name.text);
                for (option, _) in &c.options {
                    self.use_of(option.span, Kind::Option, &format!("{}.{}", name.text, option.text));
                }
            }
        }
    }

    fn attrs_and_macros(&mut self, config: &Config) {
        for def in &config.attrs {
            let on: Vec<&str> = def.targets.iter().map(|t| t.text.as_str()).collect();
            let line = format!("\n```zlint\nin {} let {} {}\n```", on.join(", "), def.name.text, self.source(def.expr.span));
            let id = self.add(Kind::Attr, &def.name.text, "attribute".into(), String::new());
            if self.symbols[id].doc.is_empty() {
                self.symbols[id].doc = "computed once per node, on demand; readable as `name`, `field.name`, `parent.name` and through `children`/`previous`/`next`".into();
            }
            self.symbols[id].doc.push_str(&line);
            self.def(Kind::Attr, &def.name.text, def.name.span);
            for t in &def.targets {
                self.use_parent(&Parent::Matcher(t.clone()));
            }
        }
        for f in &config.forwards {
            for t in &f.targets {
                self.use_parent(&Parent::Matcher(t.clone()));
            }
            for a in &f.attrs {
                self.use_of(a.span, Kind::Attr, &a.text);
            }
            self.use_of(f.field.span, Kind::Field, &f.field.text);
        }
        for m in &config.macros {
            let doc = format!("```zlint\n{}\n```\n\npasted where it is used", self.source(m.span));
            let summary = if m.params.is_empty() { "macro".to_string() } else { format!("macro({})", m.params.join(", ")) };
            self.add(Kind::Macro, &m.name.text, summary, doc);
            self.def(Kind::Macro, &m.name.text, m.name.span);
        }
    }

    /// A layer's description lists the rules checking it and the attributes
    /// defined on it.
    fn what_reads_layers(&mut self, config: &Config, rules: &[Rule]) {
        let mut on: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
        for r in rules {
            for t in r.target_text.split(", ") {
                let layer = t.split('.').next().unwrap_or(t).to_string();
                on.entry(layer).or_default().0.push(r.name.clone());
            }
        }
        for a in &config.attrs {
            for t in &a.targets {
                let list = &mut on.entry(t.text.clone()).or_default().1;
                if !list.contains(&a.name.text) {
                    list.push(a.name.text.clone());
                }
            }
        }
        for (layer, (rules, attrs)) in on {
            let kind = if self.get(Kind::Layer, &layer).is_some() { Kind::Layer } else { Kind::Group };
            let Some(id) = self.get(kind, &layer) else { continue };
            let doc = &mut self.symbols[id].doc;
            if !rules.is_empty() {
                let _ = write!(doc, "\n\nrules: `{}`", rules.join("`, `"));
            }
            if !attrs.is_empty() {
                let _ = write!(doc, "\n\nattributes: `{}`", attrs.join("`, `"));
            }
        }
    }

    fn parser_uses(&mut self, config: &Config, grammar: &Grammar) {
        for def in &config.matchers {
            for p in &def.parents {
                self.use_parent(p);
            }
            if def.pattern.is_none() {
                self.use_of(def.name.span, Kind::Layer, &def.name.text);
            }
            if let Some(base) = &def.like {
                self.use_of(base.span, Kind::Layer, &base.text);
            }
        }
        for g in &config.groups {
            for p in &g.parents {
                self.use_parent(p);
            }
        }
        for (p, field) in config.rules.iter().flat_map(|r| &r.indents).flat_map(|i| &i.targets) {
            self.use_parent(p);
            if let Some(f) = field {
                self.use_of(f.span, Kind::Field, &f.text);
            }
        }
        for s in &config.schemas {
            let doc = format!("```zlint\nschema {} {}\n```{}", s.name.text, s.schema, s.doc.as_ref().map(|d| format!("\n\n{d}")).unwrap_or_default());
            self.add(Kind::Schema, &s.name.text, s.doc.clone().unwrap_or_else(|| s.schema.to_string()), doc);
            self.def(Kind::Schema, &s.name.text, s.name.span);
        }
        for u in &config.uses {
            match u {
                Use::Macro(n) => self.use_of(n.span, Kind::Macro, &n.text),
                Use::Schema(n) => self.use_of(n.span, Kind::Schema, &n.text),
                Use::Layer(n) => {
                    let kind = if grammar.matcher(&n.text).is_some() { Kind::Layer } else { Kind::Group };
                    self.use_of(n.span, kind, &n.text);
                }
                Use::Import(span, file) => {
                    let name = self.sources.get(*file).name.clone();
                    let id = self.add(Kind::File, &name, "imported file".into(), format!("`{name}`"));
                    let whole = Span { file: *file, start: 0, end: 0 };
                    if !self.symbols[id].defs.contains(&whole) {
                        self.symbols[id].defs.push(whole);
                    }
                    self.uses.push((*span, id));
                }
            }
        }
    }
}

/// How a value of `schema` is written: enum members bare (`never`),
/// anything else as a literal (`"never"`, `3`).
/// An option as symbols show it: compiled (configured value) or as defined.
struct OptionView<'a> {
    name: &'a str,
    schema: &'a crate::config::schema::Schema,
    value: &'a crate::expr::Value<'static>,
    default: &'a crate::expr::Value<'static>,
    description: Option<&'a str>,
    primary: bool,
}

fn enum_or_literal(schema: &crate::config::schema::Schema, v: &crate::expr::Value) -> String {
    match v {
        crate::expr::Value::Str(s) if schema.takes_word(s, &|_| None) => s.to_string(),
        other => other.literal(),
    }
}
