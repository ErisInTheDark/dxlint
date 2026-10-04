//! Item-level parser. Pulls tokens from a [`Stream`], so imports and macro
//! expansions are just more tokens arriving in place.

mod expr;
mod pattern;
mod schema;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use super::ast::*;
use super::error::ConfigError;
use super::lexer::{lex, Tk, Token};
use super::source::{normalize_path, SourceMap, SourceProvider, Span};
use super::std_lib;
use super::stream::{MacroDef, Stream};
use crate::pattern::class::CharClass;
use crate::pattern::Pat;

/// Words with a meaning in items, clauses or patterns; they cannot name
/// macros or matchers.
const RESERVED: &[&str] = &[
    "import", "macro", "lang", "group", "match", "in", "layer", "rule", "indent", "like", "set", "ignore", "option", "when",
    "fix", "as", "files", "report", "until", "or", "if", "then", "else", "true", "false", "null", "off", "hint", "info",
    "warn", "error", "native", "let", "drop", "override", "amend", "forward", "is", "define", "schema", "each",
];

/// What the lines of a `{ ... }` block (or one `in ...` line) are about:
/// their parent layers, or what their rules check.
#[derive(Clone)]
struct Context {
    /// Layers, languages or groups, each maybe narrowed to a field.
    parents: Vec<(Parent, Option<Name>)>,
    /// `in A layer L`: matchers defined here form the named layer `L`.
    layer: Option<Name>,
}

/// A rule target for one context parent.
fn target_of((parent, field): &(Parent, Option<Name>)) -> Target {
    match (parent, field) {
        (Parent::Lang(name), _) => Target::Lang(name.clone()),
        (Parent::AnyLang(span), _) => Target::Lang(Name { text: "*".into(), span: *span }),
        (Parent::Matcher(name), None) => Target::Matcher(name.clone()),
        (Parent::Matcher(name), Some(f)) => Target::Field(name.clone(), f.clone()),
    }
}

pub struct Parser<'a> {
    ts: Stream,
    sources: &'a mut SourceMap,
    provider: &'a dyn SourceProvider,
    pub errors: Vec<ConfigError>,
    imported: HashSet<String>,
    pub config: Config,
    /// Bracket depth inside expressions; operators may only continue onto
    /// the next line while it is above zero.
    expr_depth: usize,
    /// Set by `override`: the next definition replaces an existing one.
    overriding: Option<Span>,
    /// Inside `define { ... }`: rules are defined but off.
    defining: usize,
}

impl<'a> Parser<'a> {
    pub fn new(sources: &'a mut SourceMap, provider: &'a dyn SourceProvider) -> Self {
        Parser {
            ts: Stream::new(),
            sources,
            provider,
            errors: Vec::new(),
            imported: HashSet::new(),
            config: Config::default(),
            expr_depth: 0,
            overriding: None,
            defining: 0,
        }
    }

    /// Parses an entry file and everything it imports.
    pub fn parse_entry(&mut self, path: &Path) {
        let path = normalize_path(path);
        match self.provider.read(&path) {
            Ok(text) => {
                self.imported.insert(path.to_string_lossy().into_owned());
                self.push_source(path.display().to_string(), Some(path), text);
                self.items();
            }
            Err(e) => self.errors.push(ConfigError::new(None, format!("cannot read `{}`: {e}", path.display()))),
        }
    }

    /// Parses in-memory text (tests, `--rules` snippets).
    pub fn parse_text(&mut self, name: &str, text: &str) {
        self.push_source(name.to_string(), None, text.to_string());
        self.items();
    }

    fn push_source(&mut self, name: String, path: Option<PathBuf>, text: String) -> super::source::FileId {
        let text: Arc<str> = Arc::from(text);
        let id = self.sources.add(name, path, text.clone());
        let tokens = lex(id, &text, 0, &mut self.errors);
        self.ts.push_tokens(tokens);
        id
    }

    /// Hands over what the token stream collected: macro calls become uses,
    /// and the fingerprint of everything parsed.
    pub fn finish(&mut self) -> u64 {
        for (name, span) in std::mem::take(&mut self.ts.calls) {
            self.config.uses.push(Use::Macro(Name { text: name.to_string(), span }));
        }
        self.ts.fingerprint()
    }

    // ---- token helpers -------------------------------------------------

    fn skip(&mut self, newlines: bool, invisible: bool) {
        while let Some(t) = self.ts.peek_raw(0) {
            let skip = match t.kind {
                Tk::Newline => newlines,
                Tk::InvOpen | Tk::InvClose => invisible,
                _ => false,
            };
            if !skip {
                break;
            }
            self.ts.next_raw();
        }
    }

    /// Next meaningful token at item/clause level (skips newlines and macro
    /// boundaries).
    fn peek(&mut self) -> Option<Token> {
        self.skip(true, true);
        self.ts.peek_raw(0).cloned()
    }

    fn bump(&mut self) -> Option<Token> {
        self.skip(true, true);
        self.ts.next_raw()
    }

    fn here(&mut self) -> Option<Span> {
        self.ts.peek_raw(0).map(|t| t.span).or(self.ts.last_span)
    }

    fn error_here(&mut self, msg: impl Into<String>) {
        let span = self.here();
        self.errors.push(ConfigError::new(span, msg));
    }

    fn expect(&mut self, kind: Tk, what: &str) -> Option<Token> {
        match self.peek() {
            Some(t) if t.kind == kind => self.bump(),
            Some(t) => {
                self.errors.push(ConfigError::new(t.span, format!("expected {what}, found {}", t.describe())));
                None
            }
            None => {
                self.error_here(format!("expected {what}, found end of file"));
                None
            }
        }
    }

    fn ident(&mut self, what: &str) -> Option<Name> {
        let t = self.expect(Tk::Ident, what)?;
        Some(Name { text: t.text.to_string(), span: t.span })
    }

    /// Rule names may name a variant (`family:variant`) and, in `set`, hold
    /// `*` globs (`style-*`). Pieces must touch: `a : b` is not a name.
    fn glued_name(&mut self, what: &str, allow_star: bool) -> Option<Name> {
        let first = self.peek()?;
        if !(first.kind == Tk::Ident || (allow_star && first.kind == Tk::Star)) {
            self.errors.push(ConfigError::new(first.span, format!("expected {what}, found {}", first.describe())));
            return None;
        }
        self.bump();
        let mut text = first.text.to_string();
        let mut span = first.span;
        let mut last = first;
        while let Some(next) = self.ts.peek_raw(0).cloned() {
            // `family:variant` names group several rules under one family
            // (`set family:* off`).
            let glues = matches!(next.kind, Tk::Ident | Tk::Minus | Tk::Num | Tk::Colon) || (allow_star && next.kind == Tk::Star);
            if !glues || !last.touches(&next) {
                break;
            }
            self.ts.next_raw();
            text.push_str(&next.text);
            span = span.to(next.span);
            last = next;
        }
        if text.ends_with('-') || text.ends_with(':') {
            self.errors.push(ConfigError::new(span, format!("{what} cannot end with `{}`", &text[text.len() - 1..])));
        }
        if text.matches(':').count() > 1 {
            self.errors.push(ConfigError::new(span, format!("{what} can have at most one `:`")));
        }
        Some(Name { text, span })
    }

    fn string(&mut self, what: &str) -> Option<(String, Span)> {
        let t = self.expect(Tk::Str, what)?;
        match super::lexer::decode_escapes(&t.text, t.span) {
            Ok(s) => Some((s, t.span)),
            Err(e) => {
                self.errors.push(e);
                None
            }
        }
    }

    /// Skips to the end of the current line (outside brackets) after an error.
    /// A `}` that closes an enclosing block is left for the block.
    fn recover(&mut self) {
        let mut depth = 0usize;
        while let Some(t) = self.ts.peek_raw(0).cloned() {
            match t.kind {
                Tk::RBrace if depth == 0 => return,
                Tk::LParen | Tk::LBrace | Tk::LBrack => depth += 1,
                Tk::RParen | Tk::RBrack => depth = depth.saturating_sub(1),
                Tk::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        self.ts.next_raw();
                        return;
                    }
                }
                Tk::Newline if depth == 0 => {
                    self.ts.next_raw();
                    return;
                }
                _ => {}
            }
            self.ts.next_raw();
        }
    }

    /// Requires the item to end here (newline, `;`, or end of input).
    fn end_of_item(&mut self) {
        self.skip(false, true);
        match self.ts.peek_raw(0) {
            None => {}
            Some(t) if t.kind == Tk::Newline || t.kind == Tk::RBrace => {}
            Some(t) => {
                let (span, what) = (t.span, t.describe());
                self.errors.push(ConfigError::new(span, format!("unexpected {what}; expected end of line")));
                self.recover();
            }
        }
    }

    // ---- items ---------------------------------------------------------

    fn items(&mut self) {
        while self.peek().is_some() {
            self.item(None);
        }
    }

    /// One item. Inside a `{ ... }` block, `ctx` is the block's context:
    /// `match`, `rule` and `indent` lines take their layers from it.
    fn item(&mut self, ctx: Option<&Context>) {
        let Some(t) = self.peek() else { return };
        let before = self.errors.len();
        match (t.kind, &*t.text) {
            (Tk::Ident, "import") => self.import(),
            (Tk::Ident, "macro" | "@macro") => self.macro_def(),
            (Tk::Ident, "fn") => self.fn_def(),
            (Tk::Ident, "lang") => self.lang(),
            (Tk::Ident, "group") => self.group(),
            (Tk::Ident, "in") => self.context_item(),
            (Tk::Ident, "match") => self.matcher(ctx),
            (Tk::Ident, "rule") => self.rule(ctx),
            (Tk::Ident, "indent") => self.indent(ctx),
            (Tk::Ident, "let") => self.attr(ctx),
            (Tk::Ident, "forward") => self.forward(ctx),
            (Tk::Ident, "native") => self.native(),
            (Tk::Ident, "set") => self.old_set(),
            (Tk::Ident, "ignore") => self.ignore(),
            (Tk::Ident, "drop") => self.drop_item(),
            (Tk::Ident, "override") => self.override_item(ctx),
            (Tk::Ident, "define") => self.define_block(),
            (Tk::Ident, "schema") => self.schema_item(),
            (Tk::Ident, "amend") => {
                self.errors.push(
                    ConfigError::new(t.span, "`amend` is now part of `rule`").note("write `rule name { ... }`: its clauses add to the rule, and `set option value` sets options"),
                );
                self.bump();
            }
            (Tk::Ident, _) if self.ts.expand(&mut self.errors) => return,
            (Tk::Ident, name) if Dropped::find(&self.config.dropped, name).is_some() => {
                let d = Dropped::find(&self.config.dropped, name).expect("found").clone();
                self.errors.push(d.still_used(t.span));
                self.bump();
            }
            _ => {
                self.errors.push(
                    ConfigError::new(t.span, format!("expected an item, found {}", t.describe())).note(
                        "items are: lang, group, match, in ..., rule, indent, let, fn, macro, schema, import, ignore, native, define, drop, override",
                    ),
                );
                self.bump();
            }
        }
        if self.errors.len() > before {
            // Keep one error per broken item.
            self.recover_if_mid_line();
        }
    }

    /// `in A or B [layer L] ...`: a context for the rest of the line, or for
    /// every line of the `{ ... }` block that follows.
    fn context_item(&mut self) {
        let kw = self.bump().expect("peeked");
        let parents = self.parent_items();
        if parents.is_empty() {
            return;
        }
        self.skip(false, true);
        let layer = if self.ts.peek_raw(0).is_some_and(|t| t.is_ident("layer")) {
            self.ts.next_raw();
            match self.ident("a layer name") {
                Some(l) => Some(l),
                None => return,
            }
        } else {
            None
        };
        let ctx = Context { parents, layer };
        self.skip(false, true);
        match self.ts.peek_raw(0).cloned() {
            Some(t) if t.kind == Tk::LBrace => {
                self.ts.next_raw();
                self.block(&ctx, t.span);
            }
            Some(t) if t.is_ident("match") => self.matcher(Some(&ctx)),
            Some(t) if t.is_ident("rule") => self.rule(Some(&ctx)),
            Some(t) if t.is_ident("indent") => self.indent(Some(&ctx)),
            Some(t) if t.is_ident("let") => self.attr(Some(&ctx)),
            Some(t) if t.is_ident("forward") => self.forward(Some(&ctx)),
            other => {
                let span = other.map(|t| t.span).or(self.ts.last_span);
                self.errors.push(ConfigError::new(
                    span,
                    format!("expected `match`, `rule`, `indent`, `let`, `forward` or `{{` after `{} ...`", kw.text),
                ));
            }
        }
    }

    /// The lines of a `{ ... }` block, each an item in `ctx`.
    fn block(&mut self, ctx: &Context, opened: Span) {
        loop {
            match self.peek() {
                None => {
                    self.errors.push(ConfigError::new(opened, "unclosed `{`"));
                    return;
                }
                Some(t) if t.kind == Tk::RBrace => {
                    self.bump();
                    return;
                }
                Some(_) => self.item(Some(ctx)),
            }
        }
    }

    /// `in A indent`: lines inside A are indented.
    fn indent(&mut self, ctx: Option<&Context>) {
        let kw = self.bump().expect("peeked");
        match ctx {
            Some(c) if c.layer.is_none() => self.config.indents.push(IndentDef { targets: c.parents.clone(), library: self.defining > 0 }),
            Some(_) => self.errors.push(ConfigError::new(kw.span, "`layer` names where matchers go, not `indent`")),
            None => self.errors.push(ConfigError::new(kw.span, "`indent` needs a context: `in <layer> indent`")),
        }
        self.end_of_item();
    }

    /// `in a or b let name expr`: a computed attribute of those layers.
    fn attr(&mut self, ctx: Option<&Context>) {
        let overriding = self.overriding.take();
        let kw = self.bump().expect("peeked");
        let Some(name) = self.ident("an attribute name") else { return };
        if RESERVED.contains(&name.text.as_str()) {
            self.errors.push(ConfigError::new(name.span, format!("`{}` is a keyword and cannot name an attribute", name.text)));
            return;
        }
        let targets = match ctx {
            Some(c) if c.layer.is_none() => {
                let mut out = Vec::new();
                for (parent, field) in &c.parents {
                    match (parent, field) {
                        (Parent::Matcher(m), None) => out.push(m.clone()),
                        (Parent::Matcher(_), Some(f)) => {
                            self.errors.push(ConfigError::new(f.span, "attributes belong to layers, not fields"))
                        }
                        (Parent::Lang(n), _) => self.errors.push(ConfigError::new(n.span, "attributes belong to layers, not languages")),
                        (Parent::AnyLang(s), _) => self.errors.push(ConfigError::new(*s, "attributes belong to layers, not languages")),
                    }
                }
                out
            }
            _ => {
                self.errors.push(ConfigError::new(kw.span, "`let` needs a layer: `in <layer> let name expr`"));
                return;
            }
        };
        let Some(expr) = self.expr() else { return };
        self.end_of_item();
        if targets.is_empty() {
            return;
        }
        if overriding.is_some() && self.remove_attr_targets(&name.text, &targets) == 0 {
            let on: Vec<&str> = targets.iter().map(|t| t.text.as_str()).collect();
            self.errors.push(ConfigError::new(
                name.span,
                format!("nothing to override: `{}` is not defined on `{}`", name.text, on.join("`, `")),
            ));
        }
        self.config.attrs.push(AttrDef { name, targets, expr, library: self.defining > 0 });
    }

    /// Removes these layers from every definition of attribute `name`;
    /// returns how many it removed.
    fn remove_attr_targets(&mut self, name: &str, targets: &[Name]) -> usize {
        let mut removed = 0;
        for def in self.config.attrs.iter_mut().filter(|d| d.name.text == name) {
            let before = def.targets.len();
            def.targets.retain(|t| !targets.iter().any(|x| x.text == t.text));
            removed += before - def.targets.len();
        }
        self.config.attrs.retain(|d| !d.targets.is_empty());
        removed
    }

    /// `in a, b forward x, y from field`: those attributes, where these
    /// layers lack them, come from that field's node.
    fn forward(&mut self, ctx: Option<&Context>) {
        let kw = self.bump().expect("peeked");
        let mut attrs = Vec::new();
        loop {
            // A macro may name the attributes.
            self.skip(false, true);
            if self.ts.expand(&mut self.errors) {
                continue;
            }
            let Some(attr) = self.ident("an attribute to forward") else { return };
            attrs.push(attr);
            self.skip(false, true);
            if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Comma) {
                self.ts.next_raw();
                continue;
            }
            break;
        }
        if !self.ts.peek_raw(0).is_some_and(|t| t.is_ident("from")) {
            self.error_here("expected `from <field>`: `forward attribute, ... from field`");
            return;
        }
        self.ts.next_raw();
        let Some(field) = self.ident("the field to forward from") else { return };
        self.end_of_item();
        let targets: Vec<Name> = match ctx {
            Some(c) if c.layer.is_none() => c
                .parents
                .iter()
                .filter_map(|(p, f)| match (p, f) {
                    (Parent::Matcher(n), None) => Some(n.clone()),
                    _ => {
                        self.errors.push(ConfigError::new(p.span(), "`forward` belongs to layers, not fields or languages"));
                        None
                    }
                })
                .collect(),
            _ => {
                self.errors.push(ConfigError::new(kw.span, "`forward` needs a layer: `in <layer> forward attribute from field`"));
                return;
            }
        };
        if !targets.is_empty() {
            self.config.forwards.push(ForwardDef { targets, attrs, field, library: self.defining > 0 });
        }
    }

    /// `native ts`
    fn native(&mut self) {
        self.bump();
        let Some(name) = self.ident("a native front end name (`ts`)") else { return };
        self.end_of_item();
        // `ts_node`: every syntax layer of the front end (tokens aside);
        // `ts_code`: those whose text is code (literal text aside).
        if name.text == "ts" && !self.config.natives.iter().any(|n| n.text == "ts") {
            use crate::native::ts::kinds::{Kind, ALL_KINDS, KINDS};
            let group = |text: &str, keep: &dyn Fn(Kind) -> bool| GroupDef {
                name: Name { text: text.into(), span: name.span },
                parents: ALL_KINDS
                    .iter()
                    .filter(|&&k| !k.is_token() && keep(k))
                    .map(|&k| Parent::Matcher(Name { text: KINDS[k as usize].0.to_string(), span: name.span }))
                    .collect(),
                library: true,
            };
            self.config.groups.push(group("ts_node", &|_| true));
            self.config.groups.push(group("ts_code", &|k| !k.is_literal_text()));
        }
        self.config.natives.push(name);
    }

    /// After an item's head: its `{ ... }` block in `ctx`, or the end of the line.
    fn block_or_end(&mut self, ctx: Context) {
        self.skip(false, true);
        match self.ts.peek_raw(0).cloned() {
            Some(t) if t.kind == Tk::LBrace => {
                self.ts.next_raw();
                self.block(&ctx, t.span);
            }
            _ => self.end_of_item(),
        }
    }

    fn recover_if_mid_line(&mut self) {
        if self.ts.peek_raw(0).is_some_and(|t| t.kind != Tk::Newline) {
            self.recover();
        }
    }

    /// `import .ts` (a built-in language), `import rules:recommended` (a
    /// built-in ruleset), or `import some/path` (a file, relative to this
    /// one; quotes optional).
    fn import(&mut self) {
        let kw = self.bump().expect("peeked");
        self.skip(false, true);
        let (path, span) = match self.ts.peek_raw(0).cloned() {
            Some(t) if t.kind == Tk::Path => {
                self.ts.next_raw();
                (t.text.to_string(), t.span)
            }
            _ => match self.string("what to import: `.ts`, `rules:recommended` or a relative path") {
                Some(p) => p,
                None => return,
            },
        };
        self.end_of_item();

        if let Some(name) = path.strip_prefix("std/") {
            self.errors.push(
                ConfigError::new(span, format!("built-in languages are imported as `.{name}` now"))
                    .note(format!("write `import .{}`", std_lib::short_name(name))),
            );
            return;
        }
        let builtin = match path.strip_prefix('.') {
            Some(name) if name.starts_with(|c: char| c.is_ascii_alphabetic()) => Some(("std", std_lib::long_name(name))),
            _ => path.split_once(':').map(|(ns, name)| (ns, name.to_string())),
        };
        if let Some((namespace, name)) = builtin {
            let (key, text) = match namespace {
                "std" => (format!("std/{name}"), std_lib::get(&name)),
                "rules" => (format!("rules/{name}"), std_lib::ruleset(&name)),
                other => {
                    self.errors.push(ConfigError::new(span, format!("unknown import namespace `{other}:`")).note("built-in rulesets are `rules:<name>`"));
                    return;
                }
            };
            if !self.imported.insert(key.clone()) {
                self.record_import(span, |f| f.name == key);
                return;
            }
            match text {
                Some(text) => {
                    let id = self.push_source(key, None, text.to_string());
                    self.config.uses.push(Use::Import(span, id));
                }
                None if namespace == "std" => self.errors.push(
                    ConfigError::new(span, format!("unknown built-in language `{path}`"))
                        .suggest(&name, std_lib::names().filter(|n| !n.contains('/'))),
                ),
                None => self.errors.push(ConfigError::new(span, format!("unknown ruleset `{path}`")).suggest(&name, std_lib::rulesets())),
            }
            return;
        }

        let base = self.sources.get(kw.span.file).path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf));
        let Some(base) = base else {
            self.errors.push(ConfigError::new(span, "relative imports need the importing file to live on disk"));
            return;
        };
        let mut full = base.join(&path);
        // `.zlint` (a config file) is a whole name, not an extension.
        if full.extension().is_none() && full.file_name().is_some_and(|n| n != super::CONFIG_NAME) {
            full.set_extension("zlint");
        }
        let full = normalize_path(&full);
        let key = full.to_string_lossy().into_owned();
        if !self.imported.insert(key) {
            self.record_import(span, |f| f.path.as_deref() == Some(full.as_path()));
            return;
        }
        match self.provider.read(&full) {
            Ok(text) => {
                let id = self.push_source(full.display().to_string(), Some(full), text);
                self.config.uses.push(Use::Import(span, id));
            }
            Err(e) => self.errors.push(ConfigError::new(span, format!("cannot import `{}`: {e}", full.display()))),
        }
    }

    /// An import of a file already loaded: still a link to it.
    fn record_import(&mut self, span: Span, is: impl Fn(&super::source::SourceFile) -> bool) {
        if let Some((id, _)) = self.sources.files().find(|(_, f)| is(f)) {
            self.config.uses.push(Use::Import(span, id));
        }
    }

    /// `fn name(a, b) = body`. The parameters touch the name, as a call's
    /// arguments do.
    fn fn_def(&mut self) {
        self.bump();
        let Some(name) = self.ident("a function name") else { return };
        let mut params = Vec::new();
        if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::LParen && t.span.start == name.span.end) {
            self.ts.next_raw();
            loop {
                match self.ts.next_raw() {
                    Some(t) if t.kind == Tk::RParen => break,
                    Some(t) if t.kind == Tk::Comma || t.kind == Tk::Newline => {}
                    Some(t) if t.kind == Tk::Ident => params.push(Name { text: t.text.to_string(), span: t.span }),
                    other => {
                        let span = other.map(|t| t.span).or(self.ts.last_span);
                        self.errors.push(ConfigError::new(span, "expected parameter names in `fn name(a, b) = ...`"));
                        return;
                    }
                }
            }
        }
        if self.expect(Tk::Eq, "`=` before the function's body").is_none() {
            return;
        }
        let Some(body) = self.expr() else { return };
        self.config.functions.push(FnDef { name, params, body });
        self.end_of_item();
    }

    fn macro_def(&mut self) {
        let overriding = self.overriding.take();
        let kw = self.bump().expect("peeked");
        let Some(name) = self.ident("a macro name") else { return };
        if RESERVED.contains(&name.text.as_str()) {
            self.errors.push(ConfigError::new(name.span, format!("`{}` is a keyword and cannot name a macro", name.text)));
            return;
        }
        // Macros replace every token of their name, everywhere after them.
        if crate::expr::facts::fact(&name.text).is_some() {
            self.errors.push(ConfigError::new(name.span, format!("`{}` is a fact; a macro of that name would hide it in every rule", name.text)));
            return;
        }
        let mut params: Vec<Rc<str>> = Vec::new();
        // Parameters are glued to the name: `macro f(a, b) ...`. A spaced
        // `(` starts a one-line body: `macro nl (if ... )`.
        if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::LParen && t.span.start == name.span.end) {
            self.ts.next_raw();
            loop {
                match self.ts.next_raw() {
                    Some(t) if t.kind == Tk::RParen => break,
                    Some(t) if t.kind == Tk::Comma || t.kind == Tk::Newline => {}
                    Some(t) if t.kind == Tk::Ident => params.push(t.text.clone()),
                    Some(t) => {
                        self.errors.push(ConfigError::new(t.span, "macro parameters must be plain names"));
                        self.recover();
                        return;
                    }
                    None => {
                        self.error_here("unclosed macro parameter list");
                        return;
                    }
                }
            }
        }

        let mut body = Vec::new();
        let mut depth = 0usize;
        match self.ts.peek_raw(0).map(|t| t.kind) {
            // A block of lines.
            Some(Tk::LBrace) => {
                self.ts.next_raw();
                loop {
                    let Some(t) = self.ts.next_raw() else {
                        self.errors.push(ConfigError::new(name.span, format!("unclosed body of macro `{}`", name.text)));
                        return;
                    };
                    match t.kind {
                        Tk::LParen | Tk::LBrace | Tk::LBrack | Tk::InvOpen => depth += 1,
                        Tk::RBrace if depth == 0 => break,
                        Tk::RParen | Tk::RBrace | Tk::RBrack | Tk::InvClose => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    body.push(t);
                }
            }
            None | Some(Tk::Newline) => {
                self.errors.push(ConfigError::new(name.span, format!("macro `{}` needs a body after its name", name.text)));
                return;
            }
            // The rest of the line.
            _ => loop {
                let Some(t) = self.ts.peek_raw(0).cloned() else { break };
                if t.kind == Tk::Newline && depth == 0 {
                    break;
                }
                match t.kind {
                    Tk::LParen | Tk::LBrace | Tk::LBrack | Tk::InvOpen => depth += 1,
                    Tk::RParen | Tk::RBrace | Tk::RBrack | Tk::InvClose => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    _ => {}
                }
                self.ts.next_raw();
                body.push(t);
            },
        }
        let def = MacroDef { name: Rc::from(name.text.as_str()), span: name.span, params, body: body.into() };
        self.config.macros.push(MacroInfo {
            name: name.clone(),
            params: def.params.iter().map(|p| p.to_string()).collect(),
            span: kw.span.to(self.ts.last_span.unwrap_or(name.span)),
        });
        let first = self.ts.macros.get(&def.name).map(|m| m.span);
        match (first, overriding) {
            (Some(first), None) => {
                self.errors.push(
                    ConfigError::new(name.span, format!("macro `{}` is defined twice", name.text))
                        .related(first, "first defined here")
                        .note(format!("`override macro {} ...` replaces it", name.text)),
                );
                return;
            }
            (None, Some(_)) => {
                self.errors.push(
                    ConfigError::new(name.span, format!("nothing to override: no macro `{}`", name.text))
                        .suggest(&name.text, self.ts.macros.keys().map(|k| &**k)),
                );
            }
            _ => {}
        }
        self.ts.macros.insert(def.name.clone(), Rc::new(def));
    }

    fn group(&mut self) {
        let overriding = self.overriding.take();
        self.bump();
        let Some(name) = self.ident("a group name") else { return };
        let parents = self.parent_list();
        if parents.is_empty() {
            return;
        }
        self.end_of_item();
        if overriding.is_some() {
            let before = self.config.groups.len();
            self.config.groups.retain(|g| g.name.text != name.text);
            if self.config.groups.len() == before {
                self.errors.push(ConfigError::new(name.span, format!("nothing to override: no group `{}`", name.text)));
            }
        }
        self.config.groups.push(GroupDef { name, parents, library: self.defining > 0 });
    }

    fn lang(&mut self) {
        self.bump();
        let Some(name) = self.ident("a language name") else { return };
        let globs = self.globs("a file glob like \"*.rs\"");
        if globs.is_empty() {
            return;
        }
        let root = Parent::Lang(name.clone());
        self.config.langs.push(LangDef { name, globs });
        self.block_or_end(Context { parents: vec![(root, None)], layer: None });
    }

    /// One or more globs, side by side on the line: `"*.rs" "*.rs.in"`.
    fn globs(&mut self, what: &str) -> Vec<(String, Span)> {
        let mut globs = Vec::new();
        loop {
            match self.string(what) {
                Some(glob) => globs.push(glob),
                None => return Vec::new(),
            }
            self.skip(false, true);
            if !self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Str) {
                return globs;
            }
        }
    }

    /// Eats the `,` between two layers of a list; the list may continue on
    /// the next line after it. An `or` there is the old spelling.
    fn eat_comma(&mut self) -> bool {
        self.skip(false, true);
        match self.ts.peek_raw(0).cloned() {
            Some(t) if t.kind == Tk::Comma => {
                self.ts.next_raw();
                self.skip(true, true);
                true
            }
            Some(t) if t.is_ident("or") => {
                self.errors.push(
                    ConfigError::new(t.span, "layer lists are separated by `,`").note("write `in a, b` instead of `in a or b`"),
                );
                self.ts.next_raw();
                true
            }
            _ => false,
        }
    }

    /// A parent list where fields are an error (only `indent` takes them).
    fn parent_list(&mut self) -> Vec<Parent> {
        let items = self.parent_items();
        self.no_fields(items)
    }

    fn no_fields(&mut self, items: Vec<(Parent, Option<Name>)>) -> Vec<Parent> {
        items
            .into_iter()
            .map(|(p, field)| {
                if let Some(f) = field {
                    self.errors.push(ConfigError::new(f.span, "only rules and `indent` can use a field (`in layer.field rule ...`)"));
                }
                p
            })
            .collect()
    }

    /// `a or .lang or b.field`: parents, each maybe narrowed to a field.
    fn parent_items(&mut self) -> Vec<(Parent, Option<Name>)> {
        let mut parents: Vec<(Parent, Option<Name>)> = Vec::new();
        loop {
            self.skip(false, true);
            let Some(t) = self.ts.peek_raw(0).cloned() else { break };
            if t.kind == Tk::Dot {
                self.ts.next_raw();
                let next = self.ts.peek_raw(0).cloned();
                match next {
                    Some(n) if n.kind == Tk::Ident && t.touches(&n) => {
                        self.ts.next_raw();
                        parents.push((Parent::Lang(Name { text: n.text.to_string(), span: t.span.to(n.span) }), None));
                    }
                    Some(n) if n.kind == Tk::Star && t.touches(&n) => {
                        self.ts.next_raw();
                        parents.push((Parent::AnyLang(t.span.to(n.span)), None));
                    }
                    _ => {
                        self.errors.push(ConfigError::new(
                            t.span,
                            "expected a language name right after `.`, like `.rust` (or `.*` for every language)",
                        ));
                        return parents;
                    }
                }
            } else if t.kind == Tk::Ident {
                if self.ts.expand(&mut self.errors) {
                    continue;
                }
                self.ts.next_raw();
                let layer = Name { text: t.text.to_string(), span: t.span };
                let dot_glued = self.ts.peek_raw(0).is_some_and(|d| d.kind == Tk::Dot && d.span.start == t.span.end);
                let field = if dot_glued {
                    self.ts.next_raw();
                    match self.ident("a field name after `.`") {
                        Some(f) => Some(f),
                        None => return parents,
                    }
                } else {
                    None
                };
                parents.push((Parent::Matcher(layer), field));
            } else {
                self.errors.push(ConfigError::new(t.span, format!("expected a parent layer, found {}", t.describe())));
                return parents;
            }
            if !self.eat_comma() {
                break;
            }
        }
        parents
    }

    /// `match name [like base] [pattern] [{ block }]`, with its parents from
    /// the context. Without a pattern it attaches an existing matcher.
    fn matcher(&mut self, ctx: Option<&Context>) {
        let overriding = self.overriding.take();
        self.bump();
        let (parents, layer) = match ctx {
            Some(c) => (self.no_fields(c.parents.clone()), c.layer.clone()),
            None => (Vec::new(), None),
        };
        let Some(name) = self.ident("a matcher name") else { return };
        if RESERVED.contains(&name.text.as_str()) {
            self.errors.push(ConfigError::new(name.span, format!("`{}` is a keyword and cannot name a matcher", name.text)));
        }
        self.skip(false, true);
        let like = if self.ts.peek_raw(0).is_some_and(|t| t.is_ident("like")) {
            self.ts.next_raw();
            match self.ident("the matcher this one specialises") {
                Some(base) => Some(base),
                None => return,
            }
        } else {
            None
        };
        self.skip(false, true);
        let has_pattern = !matches!(self.ts.peek_raw(0).map(|t| t.kind), None | Some(Tk::Newline | Tk::LBrace | Tk::RBrace));
        let pattern = if has_pattern {
            match self.pattern_line() {
                Some(p) => Some(p),
                // Still declare the name (matching nothing), so a typo in one
                // pattern is one error, not one per use of the matcher.
                None => Some((Pat::Class(CharClass::new(Vec::new(), Vec::new(), false)), name.span)),
            }
        } else {
            if parents.is_empty() {
                self.errors.push(
                    ConfigError::new(name.span, format!("matcher `{}` needs a pattern", name.text))
                        .note("write `match name /regex/ ...`, or `in parent match name` to attach an existing matcher"),
                );
                return;
            }
            None
        };
        if like.is_some() && pattern.is_none() {
            self.errors.push(ConfigError::new(name.span, "`like` belongs on the definition (`match name like base /regex/ ...`)"));
            return;
        }
        let own = Parent::Matcher(name.clone());
        let def = MatcherDef { name, parents, layer, pattern, like, library: self.defining > 0 };
        match overriding {
            Some(kw) if def.pattern.is_none() => {
                self.errors.push(ConfigError::new(kw, "`override match` needs the new pattern: `override match name pattern`"));
            }
            // In place, so ties between matchers resolve as before.
            Some(_) => match self.config.matchers.iter().position(|m| m.name.text == def.name.text && m.pattern.is_some()) {
                Some(i) => self.config.matchers[i] = def,
                None => {
                    self.errors.push(ConfigError::new(def.name.span, format!("nothing to override: no matcher `{}`", def.name.text)));
                    self.config.matchers.push(def);
                }
            },
            None => self.config.matchers.push(def),
        }
        self.block_or_end(Context { parents: vec![(own, None)], layer: None });
    }

    /// `rule name [severity [message]] [fix as expr] [{ clauses }]`, checking the
    /// context's layers.
    fn rule(&mut self, ctx: Option<&Context>) {
        let overriding = self.overriding.take();
        let kw = self.bump().expect("peeked");
        let Some(ctx) = ctx else {
            if let Some(o) = overriding {
                self.errors.push(ConfigError::new(o, "`override` replaces definitions: `override in <layer> rule name ...`"));
            }
            return self.configure(kw);
        };
        let Some(name) = self.glued_name("a rule name", false) else { return };
        let targets: Vec<Target> = ctx.parents.iter().map(target_of).collect();
        let mut def = RuleDef::new(name, targets);
        def.defined_only = self.defining > 0;
        self.rule_tail(&mut def, None);
        let existing = self.config.rules.iter().position(|r| r.name.text == def.name.text);
        match (existing, overriding) {
            (Some(i), Some(_)) => {
                // Configuration above went to the old rule.
                self.forget_configs(&def.name.text);
                self.config.rules[i] = def;
            }
            (Some(i), None) => {
                let first = self.config.rules[i].name.span;
                self.errors.push(
                    ConfigError::new(def.name.span, format!("rule `{}` is defined twice", def.name.text))
                        .related(first, "first defined here")
                        .note(format!(
                            "`override in <layer> rule {0} ...` replaces it; `rule {0} {{ ... }}` (no `in`) configures it",
                            def.name.text
                        )),
                );
            }
            (None, Some(_)) => {
                self.errors.push(
                    ConfigError::new(def.name.span, format!("nothing to override: no rule `{}`", def.name.text))
                        .suggest(&def.name.text, self.config.rules.iter().map(|r| r.name.text.as_str())),
                );
                self.config.rules.push(def);
            }
            (None, None) => self.config.rules.push(def),
        }
    }

    /// `rule name [severity [message]] [{ ... }]` without a context: turns
    /// existing rules on and configures them.
    fn configure(&mut self, kw: Token) {
        let Some(name) = self.glued_name("a rule name or glob", true) else { return };
        let mut patch = RuleDef::new(name, Vec::new());
        let mut options = Vec::new();
        self.rule_tail(&mut patch, Some(&mut options));
        if !patch.options.is_empty() {
            self.errors.push(
                ConfigError::new(patch.options[0].name.span, "`option` declares options in a definition")
                    .note("in `rule name { ... }`, `set option value` sets them"),
            );
        }
        // Built-in rulesets name rules of every language; only the imported
        // ones exist.
        let soft = self.sources.get(kw.span.file).name.starts_with("rules/");
        self.config.configs.push(RuleConfig { patch, options, soft });
    }

    /// `define { ... }`: rules inside are defined but stay off until a
    /// `rule name` line turns them on.
    fn define_block(&mut self) {
        let kw = self.bump().expect("peeked");
        self.skip(false, true);
        if !self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::LBrace) {
            self.error_here("expected `{` after `define`: `define { ... }`");
            return;
        }
        self.ts.next_raw();
        self.defining += 1;
        loop {
            match self.peek() {
                None => {
                    self.errors.push(ConfigError::new(kw.span, "unclosed `define {`"));
                    break;
                }
                Some(t) if t.kind == Tk::RBrace => {
                    self.bump();
                    break;
                }
                Some(_) => self.item(None),
            }
        }
        self.defining -= 1;
    }

    /// `set` outside a rule block: the old spelling of `rule`.
    fn old_set(&mut self) {
        let kw = self.bump().expect("peeked");
        let target = self.glued_name("a rule name or glob", true);
        let hint = match target {
            Some(t) if self.ts.peek_raw(0).is_some_and(|d| d.kind == Tk::Dot) => {
                format!("write `rule {} {{ set option value }}`", t.text)
            }
            Some(t) => format!("write `rule {} <severity>`", t.text),
            None => "write `rule name <severity>` or `rule name { set option value }`".into(),
        };
        // The item loop skips the rest of the line.
        self.errors.push(ConfigError::new(kw.span, "`set` only sets options inside `rule name { ... }`").note(hint));
    }

    /// Everything after a rule's name: `[severity [message]] [fix as expr]
    /// [{ clauses }]`. Marks the rule broken when any of it fails to parse.
    /// `options` takes `set name value` lines (configuring, not defining).
    fn rule_tail(&mut self, def: &mut RuleDef, options: Option<&mut Vec<(Name, Expr)>>) {
        let before = self.errors.len();
        self.skip(false, true);
        if let Some(severity) = self.peek_severity() {
            self.ts.next_raw();
            def.severity = Some(severity);
            self.message_after_severity(def);
        }
        self.skip(false, true);
        if self.ts.peek_raw(0).is_some_and(|t| t.is_ident("fix")) {
            self.ts.next_raw();
            (def.fix_target, def.fix) = self.fix_as();
        }
        if self.errors.len() == before {
            self.skip(false, true);
            if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::LBrace) {
                self.ts.next_raw();
                self.rule_body(def, options);
            } else {
                self.end_of_item();
            }
        }
        // After a broken head, the caller skips the rest of the line, body
        // included.
        def.broken |= self.errors.len() > before;
    }

    /// A rule was replaced or dropped: configuration naming it exactly went
    /// to the old one and goes; globs stay, and may now match nothing.
    fn forget_configs(&mut self, rule: &str) {
        self.config.configs.retain(|c| c.patch.name.text != rule);
        for c in &mut self.config.configs {
            if crate::path_globs::wildcard(&c.patch.name.text, rule) {
                c.soft = true;
            }
        }
    }

    /// `override <definition>`: the definition replaces the existing one of
    /// its name, in its place.
    fn override_item(&mut self, ctx: Option<&Context>) {
        let kw = self.bump().expect("peeked");
        self.overriding = Some(kw.span);
        self.item(ctx);
        if self.overriding.take().is_some() {
            self.errors.push(ConfigError::new(kw.span, "`override` goes before a definition: `rule`, `match`, `let`, `macro`, `group` or `schema`"));
        }
    }

    /// `drop rule name`, `drop match x`, `drop in a, b match x`, `drop let x`,
    /// `drop in a let x`, `drop macro m`, `drop group g [members]`,
    /// `drop ignore "glob"`.
    fn drop_item(&mut self) {
        let kw = self.bump().expect("peeked");
        self.skip(false, true);
        let Some(what) = self.ts.peek_raw(0).cloned().filter(|t| t.kind == Tk::Ident) else {
            self.error_here("expected what to drop: `rule`, `match`, `let`, `macro`, `group`, `ignore` or `in <layer> match/let`");
            return;
        };
        self.ts.next_raw();
        match &*what.text {
            "rule" => {
                let Some(name) = self.glued_name("a rule name or glob", true) else { return };
                let span = kw.span.to(name.span);
                let gone: Vec<String> = self
                    .config
                    .rules
                    .iter()
                    .filter(|r| crate::path_globs::wildcard(&name.text, &r.name.text))
                    .map(|r| r.name.text.clone())
                    .collect();
                if gone.is_empty() {
                    self.errors.push(
                        ConfigError::new(name.span, format!("nothing to drop: no rule matches `{}`", name.text))
                            .suggest(&name.text, self.config.rules.iter().map(|r| r.name.text.as_str())),
                    );
                }
                self.config.rules.retain(|r| !gone.contains(&r.name.text));
                for rule in gone {
                    self.forget_configs(&rule);
                    self.config.dropped.push(Dropped { name: rule, span });
                }
            }
            "match" => {
                let Some(name) = self.ident("a matcher name") else { return };
                let before = self.config.matchers.len();
                // The definition and every attachment of it.
                self.config.matchers.retain(|m| m.name.text != name.text);
                if self.config.matchers.len() == before {
                    self.errors.push(
                        ConfigError::new(name.span, format!("nothing to drop: no matcher `{}`", name.text))
                            .note("native layers cannot be dropped"),
                    );
                } else {
                    self.config.dropped.push(Dropped { name: name.text, span: kw.span.to(name.span) });
                }
            }
            "let" => {
                let Some(name) = self.ident("an attribute name") else { return };
                let before = self.config.attrs.len();
                self.config.attrs.retain(|a| a.name.text != name.text);
                if self.config.attrs.len() == before {
                    self.errors.push(ConfigError::new(name.span, format!("nothing to drop: no attribute `{}`", name.text)));
                } else {
                    self.config.dropped.push(Dropped { name: name.text, span: kw.span.to(name.span) });
                }
            }
            "macro" => {
                let Some(name) = self.ident("a macro name") else { return };
                if self.ts.macros.remove(name.text.as_str()).is_none() {
                    self.errors.push(ConfigError::new(name.span, format!("nothing to drop: no macro `{}`", name.text)));
                } else {
                    self.config.dropped.push(Dropped { name: name.text, span: kw.span.to(name.span) });
                }
            }
            "group" => {
                let Some(name) = self.ident("a group name") else { return };
                if !self.config.groups.iter().any(|g| g.name.text == name.text) {
                    self.errors.push(ConfigError::new(name.span, format!("nothing to drop: no group `{}`", name.text)));
                    self.recover_if_mid_line();
                    return;
                }
                self.skip(false, true);
                let members_follow = self.ts.peek_raw(0).is_some_and(|t| !matches!(t.kind, Tk::Newline | Tk::RBrace));
                if !members_follow {
                    self.config.groups.retain(|g| g.name.text != name.text);
                    self.config.dropped.push(Dropped { name: name.text, span: kw.span.to(name.span) });
                } else {
                    for member in self.parent_list() {
                        let text = member.text();
                        let mut found = false;
                        for g in self.config.groups.iter_mut().filter(|g| g.name.text == name.text) {
                            let before = g.parents.len();
                            g.parents.retain(|p| p.text() != text);
                            found |= g.parents.len() != before;
                        }
                        if !found {
                            self.errors.push(ConfigError::new(member.span(), format!("`{text}` is not in group `{}`", name.text)));
                        }
                    }
                }
            }
            "ignore" => {
                for (glob, span) in self.globs("a path glob") {
                    let before = self.config.ignores.len();
                    self.config.ignores.retain(|(g, _)| *g != glob);
                    if self.config.ignores.len() == before {
                        self.errors.push(ConfigError::new(span, format!("nothing to drop: no `ignore \"{glob}\"`")));
                    }
                }
            }
            "in" => {
                let parents = self.parent_items();
                let layers = self.no_fields(parents);
                self.skip(false, true);
                let kind = self.ts.next_raw().filter(|t| t.is_ident("match") || t.is_ident("let"));
                let Some(kind) = kind else {
                    self.error_here("expected `match name` or `let name` after `drop in <layers>`");
                    return;
                };
                let Some(name) = self.ident("a name") else { return };
                let wanted: Vec<String> = layers.iter().map(Parent::text).collect();
                let removed = if kind.is_ident("match") {
                    let mut removed = 0;
                    for m in self.config.matchers.iter_mut().filter(|m| m.name.text == name.text) {
                        let before = m.parents.len();
                        m.parents.retain(|p| !wanted.contains(&p.text()));
                        removed += before - m.parents.len();
                    }
                    // Attachments with no parents left attach nothing.
                    self.config.matchers.retain(|m| m.pattern.is_some() || !m.parents.is_empty());
                    removed
                } else {
                    let targets: Vec<Name> = layers
                        .iter()
                        .filter_map(|p| match p {
                            Parent::Matcher(n) => Some(n.clone()),
                            _ => None,
                        })
                        .collect();
                    self.remove_attr_targets(&name.text, &targets)
                };
                if removed == 0 {
                    self.errors.push(ConfigError::new(
                        name.span,
                        format!("nothing to drop: `{}` is not {} `{}`", name.text, if kind.is_ident("match") { "inside" } else { "defined on" }, wanted.join("`, `")),
                    ));
                }
            }
            other => {
                self.errors.push(
                    ConfigError::new(what.span, format!("cannot drop `{other}`"))
                        .note("`drop` takes `rule`, `match`, `let`, `macro`, `group`, `ignore` or `in <layer> match/let`"),
                );
                return;
            }
        }
        self.end_of_item();
    }

    /// After `fix`: `as expr`, `space_before as expr` or `space_after as expr`.
    fn fix_as(&mut self) -> (FixTarget, Option<Expr>) {
        self.skip(false, true);
        let target = match self.ts.peek_raw(0) {
            Some(t) if t.is_ident("space_before") => FixTarget::SpaceBefore,
            Some(t) if t.is_ident("space_after") => FixTarget::SpaceAfter,
            _ => FixTarget::Node,
        };
        if target != FixTarget::Node {
            self.ts.next_raw();
        }
        match self.ts.peek_raw(0).cloned() {
            Some(t) if t.is_ident("as") => {
                self.ts.next_raw();
                (target, self.expr())
            }
            other => {
                let span = other.map(|t| t.span).or(self.ts.last_span);
                self.errors.push(
                    ConfigError::new(span, "expected `as` after `fix`: `fix as <what the text should be>`")
                        .note("`fix space_before as \" \"` / `fix space_after as \"\"` rewrite the whitespace next to the node"),
                );
                (target, None)
            }
        }
    }

    /// A severity keyword (`off hint info warn error`) next on the line.
    fn peek_severity(&mut self) -> Option<Severity> {
        let t = self.ts.peek_raw(0).filter(|t| t.kind == Tk::Ident)?;
        Severity::NAMES.contains(&&*t.text).then(|| Severity::parse(&t.text)).flatten()
    }

    /// The optional message after a severity keyword: the rest of the line,
    /// up to a `fix` or a `{`.
    fn message_after_severity(&mut self, def: &mut RuleDef) {
        self.skip(false, true);
        let ends = match self.ts.peek_raw(0) {
            None => true,
            Some(t) => matches!(t.kind, Tk::Newline | Tk::LBrace | Tk::RBrace) || t.is_ident("fix"),
        };
        if !ends {
            def.message = self.expr();
        }
    }

    fn rule_body(&mut self, def: &mut RuleDef, mut options: Option<&mut Vec<(Name, Expr)>>) {
        loop {
            let Some(t) = self.peek() else {
                self.errors.push(ConfigError::new(def.name.span, format!("unclosed body of rule `{}`", def.name.text)));
                return;
            };
            if t.kind == Tk::RBrace {
                self.bump();
                return;
            }
            let before = self.errors.len();
            match (t.kind, &*t.text) {
                // `set name value`: an option value (configuring only).
                (Tk::Ident, "set") => {
                    self.bump();
                    match options.as_deref_mut() {
                        Some(list) => {
                            if let Some(name) = self.ident("an option name") {
                                if let Some(value) = self.expr() {
                                    list.push((name, value));
                                }
                            }
                        }
                        None => self.errors.push(
                            ConfigError::new(t.span, "`set` sets options of an existing rule: `rule name { set option value }`")
                                .note("a definition declares them with `option name <schema> = <default> \"description\"`"),
                        ),
                    }
                }
                (Tk::Ident, "option") => {
                    self.bump();
                    if let Some(opt) = self.option_line() {
                        def.options.push(opt);
                    }
                }
                (Tk::Ident, word) if Severity::NAMES.contains(&word) => {
                    self.bump();
                    if def.severity.is_some() {
                        self.errors.push(ConfigError::new(t.span, format!("rule `{}` already has a severity", def.name.text)));
                    } else {
                        def.severity = Severity::parse(word);
                        self.message_after_severity(def);
                    }
                }
                (Tk::Ident, "fix") => {
                    self.bump();
                    if def.fix.is_some() {
                        self.errors.push(
                            ConfigError::new(t.span, format!("rule `{}` already has a fix", def.name.text))
                                .note("a rule has at most one fix: the expression saying what the text should be"),
                        );
                    } else {
                        (def.fix_target, def.fix) = self.fix_as();
                    }
                }
                (Tk::Ident, "when") => {
                    self.bump();
                    if let Some(e) = self.expr() {
                        def.when.push(e);
                    }
                }
                // A rule-local value: computed once per node.
                (Tk::Ident, "let") => {
                    self.bump();
                    if let Some(name) = self.ident("a name for the value") {
                        if def.locals.iter().any(|(n, _)| n.text == name.text) {
                            self.errors.push(ConfigError::new(name.span, format!("`{}` is already a `let` of this rule", name.text)));
                        } else if let Some(e) = self.expr() {
                            def.locals.push((name, e));
                        }
                    }
                }
                (Tk::Ident, "report") => {
                    self.bump();
                    if let Some(what) = self.ident("`node`, `change`, `end` or `at`") {
                        match what.text.as_str() {
                            "node" => def.report = Some(Report::Node),
                            "change" => def.report = Some(Report::Change),
                            "end" => def.report = Some(Report::End),
                            "at" => def.report_at = self.expr(),
                            other => self.errors.push(
                                ConfigError::new(what.span, format!("unknown report target `{other}`")).note(
                                    "`report node` points at the whole node, `report change` at what the fix changes, `report end` at the node's end, `report at <offset>` at a byte offset",
                                ),
                            ),
                        }
                    }
                }
                // `each name in list`: once per item. `@each`: the list is
                // the rule's options, known when the config loads.
                (Tk::Ident, at @ ("each" | "@each")) => {
                    def.each_options = at == "@each";
                    self.bump();
                    if let Some(name) = self.ident("a name for each item") {
                        self.skip(false, true);
                        if !self.ts.peek_raw(0).is_some_and(|t| t.is_ident("in")) {
                            self.error_here(format!("expected `in`: `each {} in <list>`", name.text));
                        } else {
                            self.ts.next_raw();
                            if def.each.is_some() {
                                self.errors.push(ConfigError::new(name.span, format!("rule `{}` already has an `each`", def.name.text)));
                            } else if let Some(list) = self.expr() {
                                def.each = Some((name, list));
                            }
                        }
                    }
                }
                (Tk::Ident, kw @ ("files" | "ignore")) => {
                    let kw = kw.to_string();
                    self.bump();
                    let globs = self.globs("a path glob");
                    if kw == "files" {
                        def.files.extend(globs);
                    } else {
                        def.ignore.extend(globs);
                    }
                }
                // A macro of clauses: its lines are read as clauses.
                (Tk::Ident, _) if self.ts.expand(&mut self.errors) => continue,
                _ => {
                    self.errors.push(
                        ConfigError::new(t.span, format!("expected a rule clause, found {}", t.describe()))
                            .note("rule clauses: a severity and message (`warn \"...\"`), when, let, each, fix, option (or set, when configuring), files, ignore, report"),
                    );
                    self.bump();
                }
            }
            if self.errors.len() > before {
                self.recover_if_mid_line();
            } else {
                self.end_of_item();
            }
        }
    }

    fn ignore(&mut self) {
        self.bump();
        let globs = self.globs("a path glob");
        if globs.is_empty() {
            return;
        }
        self.config.ignores.extend(globs);
        self.end_of_item();
    }
}

#[cfg(test)]
pub(crate) fn parse_str(src: &str) -> (Config, Vec<ConfigError>, SourceMap) {
    let mut sources = SourceMap::default();
    let provider = super::source::Disk;
    let mut p = Parser::new(&mut sources, &provider);
    p.parse_text("test.zlint", src);
    let (config, errors) = (std::mem::take(&mut p.config), std::mem::take(&mut p.errors));
    (config, errors, sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::Pat;

    fn ok(src: &str) -> Config {
        let (config, errors, _) = parse_str(src);
        assert!(errors.is_empty(), "{errors:#?}");
        config
    }

    fn parents(m: &MatcherDef) -> Vec<String> {
        m.parents
            .iter()
            .map(|p| match p {
                Parent::Lang(n) => format!(".{}", n.text),
                Parent::AnyLang(_) => ".*".into(),
                Parent::Matcher(n) => n.text.clone(),
            })
            .collect()
    }

    #[test]
    fn parses_the_design_sketch() {
        let c = ok(r#"
            lang rust "*.rs"
            match ws /\s+/
            in .rust match dstr /"/ until /"/
            in dstr, sstr match escape /\\(.)/
            in .rust match sstr /'/ until /'/
            match params /\(/ until /\)/
            in params match params
            in .rust match fn /fn/ ws name:/\w+/ ws? :params
            in .rust layer lines match line /^.*$/
            in .rust match either (/a/ or "b") until /c/
        "#);
        assert_eq!(c.langs.len(), 1);
        assert_eq!(c.matchers.len(), 9);
        let fun = c.matchers.iter().find(|m| m.name.text == "fn").unwrap();
        assert_eq!(fun.pattern.as_ref().unwrap().0.labels(), vec!["name", "params"]);
        assert_eq!(c.matchers.iter().find(|m| m.name.text == "line").unwrap().layer.as_ref().map(|l| l.text.as_str()), Some("lines"));
        assert!(c.matchers.iter().any(|m| m.name.text == "params" && m.pattern.is_none()));
        let either = &c.matchers.iter().find(|m| m.name.text == "either").unwrap().pattern.as_ref().unwrap().0;
        assert!(matches!(either, Pat::Seq(items) if matches!(items[0], Pat::Alt(_)) && matches!(items[1], Pat::Skip)), "{either:?}");
    }

    #[test]
    fn rules_in_one_line_or_with_a_body() {
        let c = ok(r#"
            in fn.name rule fn-snake warn "names are snake_case" fix as snake_case(text)
            in line rule line-length {
              option max int(1..) = 100 "max chars"
              error "lines under {max}"
              when len(text) > max
            }
            in todo, .md rule todo-note info "{text} left"
            in x rule quiet
            rule line-length { set max 120 }
            rule * warn
        "#);
        assert_eq!(c.rules.len(), 4);
        assert!(matches!(c.rules[0].targets[..], [Target::Field(..)]));
        assert!(c.rules[0].fix.is_some() && c.rules[0].message.is_some());
        assert_eq!(c.rules[1].options.len(), 1);
        assert_eq!(c.rules[1].severity, Some(Severity::Error));
        assert!(matches!(c.rules[2].targets[..], [Target::Matcher(_), Target::Lang(_)]));
        assert_eq!(c.rules[2].severity, Some(Severity::Info));
        assert!(c.rules[3].severity.is_none() && c.rules[3].message.is_none());
        assert_eq!(c.configs.len(), 2);
        assert_eq!(c.configs[0].options.len(), 1);
        assert_eq!(c.configs[1].patch.name.text, "*");
        assert_eq!(c.configs[1].patch.severity, Some(Severity::Warn));
    }

    #[test]
    fn context_blocks_apply_to_every_line() {
        let c = ok(r#"
            lang t "*.t" {
              match brace /\{/ until /\}/ {
                indent
                match inner /x/
                rule braces warn "brace" { when true }
              }
            }
            in .t, brace layer lines {
              match line /^.+/
            }
            in brace { in .t match back /y/ }
            in brace.body indent
        "#);
        let by_name = |n: &str| c.matchers.iter().find(|m| m.name.text == n).unwrap();
        assert_eq!(parents(by_name("brace")), vec![".t"]);
        assert_eq!(parents(by_name("inner")), vec!["brace"]);
        assert_eq!(parents(by_name("line")), vec![".t", "brace"]);
        assert!(by_name("line").layer.is_some() && by_name("inner").layer.is_none());
        // A nested `in` replaces the block's context.
        assert_eq!(parents(by_name("back")), vec![".t"]);
        assert!(matches!(&c.rules[0].targets[..], [Target::Matcher(n)] if n.text == "brace"));
        assert_eq!(c.indents.len(), 2);
        assert!(matches!(&c.indents[1].targets[..], [(Parent::Matcher(_), Some(f))] if f.text == "body"));
    }

    #[test]
    fn groups_add_on_every_line() {
        let c = ok("group code .t, paren\ngroup code brace\n");
        assert_eq!(c.groups.len(), 2);
        assert_eq!(c.groups[1].parents.len(), 1);
    }

    #[test]
    fn rules_without_a_context_configure_and_matchers_take_no_fields() {
        let (c, errors, _) = parse_str("set x off\namend rule y {}\nrule lonely warn \"x\"\nin a.b match c /x/\nin a indent extra\n");
        let all: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        // `rule` without `in` configures an existing rule; `set` and `amend`
        // outside one say how they are written now.
        assert_eq!((c.rules.len(), c.configs.len()), (0, 1));
        assert_eq!(errors.len(), 4, "{all:#?}");
        assert!(all[0].contains("`set` only sets options") && all[1].contains("`amend` is now part of `rule`") && all[2].contains("can use a field"), "{all:#?}");
        assert!(errors[0].notes.iter().any(|n| n.contains("rule x <severity>")), "{:#?}", errors[0]);
    }

    #[test]
    fn macros_generate_items() {
        let c = ok(r#"
            macro forbid(name, pat, why) {
              in .rust match name pat {
                rule name error why
              }
            }
            forbid(dbg, "dbg!" /\(/, "no dbg")
        "#);
        assert_eq!(c.matchers[0].name.text, "dbg");
        assert_eq!(c.rules[0].name.text, "dbg");
        assert_eq!(c.rules[0].severity, Some(Severity::Error));
        assert!(c.rules[0].message.is_some());
    }

    #[test]
    fn macros_expand_to_rule_clauses_and_messages() {
        let c = ok(r#"
            macro nl (if true then "a" else "b")
            macro strict {
              when true
              fix as nl "!" nl
            }
            in x rule f warn nl {
              strict
            }
        "#);
        assert_eq!(c.rules[0].when.len(), 1);
        assert!(matches!(c.rules[0].fix.as_ref().map(|e| &e.kind), Some(ExprKind::Binary(BinOp::Join, ..))));
        assert!(c.rules[0].message.is_some());
    }

    #[test]
    fn values_side_by_side_join_until_a_keyword_or_the_line_ends() {
        let c = ok(r#"
            in x rule a warn "{text}" "!" fix as if c then text "=" else (text
              "-") {
              option max number = 100 "max chars"
              when len(text) + 1 "px" == "4px"
              files "src/**" "lib/**"
              ignore "a" "b"
            }
            lang l "*.l" "*.ll"
            ignore "x" "y"
        "#);
        let rule = &c.rules[0];
        let join = |e: &Expr| matches!(e.kind, ExprKind::Binary(BinOp::Join, ..));
        assert!(join(rule.message.as_ref().unwrap()));
        let Some(ExprKind::If(_, then, other)) = rule.fix.as_ref().map(|e| &e.kind) else { panic!("{:?}", rule.fix) };
        assert!(join(then) && join(other));
        assert!(rule.options[0].default == crate::expr::Value::Num(100.0) && rule.options[0].description.is_some());
        // `len(text) + 1` is one side of the join; the join is one side of `==`.
        let ExprKind::Binary(BinOp::Eq, lhs, _) = &rule.when[0].kind else { panic!("{:?}", rule.when[0]) };
        assert!(matches!(&lhs.kind, ExprKind::Binary(BinOp::Join, sum, _) if matches!(sum.kind, ExprKind::Binary(BinOp::Add, ..))));
        assert_eq!((rule.files.len(), rule.ignore.len(), c.langs[0].globs.len(), c.ignores.len()), (2, 2, 2, 2));
    }

    #[test]
    fn plus_adds_numbers_and_fix_needs_as() {
        let (_, errors, _) = parse_str("in x rule a fix as text + \"!\"\nin x rule b fix text\nin x rule c { fix as [a] + 1 }\n");
        let all: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(all, vec!["`+` adds numbers", "expected `as` after `fix`: `fix as <what the text should be>`", "`+` adds numbers"]);
    }

    #[test]
    fn dashed_names_work_anywhere_and_binary_operators_need_spaces() {
        let c = ok("macro m(rule-id) {\n  in x rule rule-id warn \"a\" { when -1 < len(text) - 2 * 3 && !false }\n}\nm(no-dbg)\n");
        assert_eq!(c.rules[0].name.text, "no-dbg");
        let (_, errors, _) = parse_str("in x rule a {\n  when 1+2 > 0\n  when 1 -2 == -1\n  when 1 <=2\n  when text~ /x/\n}\n");
        let all: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(all, vec!["put spaces around `+`", "put spaces around `-`", "put spaces around `<=`", "put spaces around `~`"]);
    }

    #[test]
    fn several_layers_after_is_go_in_parentheses() {
        let (_, errors, _) = parse_str("in x rule a { when layer is [a, b] }\n");
        assert_eq!(errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["several layers go in parentheses: `is (a, b)`"]);
    }

    #[test]
    fn macros_cannot_hide_facts() {
        let (_, errors, _) = parse_str("macro reference_kind \"x\"\n");
        assert_eq!(errors.len(), 1, "{errors:#?}");
    }

    #[test]
    fn macro_expansions_act_as_groups() {
        let c = ok("macro digits /\\d/+\nmatch x \"a\" digits?\n");
        let Some((Pat::Seq(items), _)) = &c.matchers[0].pattern else { panic!("{:?}", c.matchers[0].pattern) };
        assert!(
            matches!(&items[1], Pat::Repeat { min: 0, max: Some(1), inner, .. } if matches!(**inner, Pat::Repeat { min: 1, .. })),
            "{items:?}"
        );
    }

    #[test]
    fn errors_point_at_the_problem_and_recover() {
        let (c, errors, _) = parse_str("match /x/\nmatch ok /y/\nmatch bad /(/\nin ok {\n  match bad2 /(/\n  match after /z/\n}\nmatch last /w/\n");
        assert_eq!(errors.len(), 3, "{errors:#?}");
        // `bad` stays declared (matching nothing) so its uses don't cascade,
        // and an error inside a block leaves the block's `}` alone.
        let names: Vec<&str> = c.matchers.iter().map(|m| m.name.text.as_str()).collect();
        assert_eq!(names, vec!["ok", "bad", "bad2", "after", "last"]);
        assert!(c.matchers[4].parents.is_empty());
    }
}
