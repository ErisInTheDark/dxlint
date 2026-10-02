//! Item-level parser. Pulls tokens from a [`Stream`], so imports and macro
//! expansions are just more tokens arriving in place.

mod expr;
mod pattern;

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
    "import", "macro", "lang", "group", "match", "in", "over", "rule", "indent", "like", "set", "ignore", "option", "when",
    "fix", "as", "files", "report", "until", "or", "if", "then", "else", "true", "false", "null", "off", "hint", "info",
    "warn", "error",
];

/// What the lines of a `{ ... }` block (or one `in ...` / `over ...` line)
/// are about: their parent layers, or what their rules check.
#[derive(Clone)]
struct Context {
    /// Layers, languages or groups, each maybe narrowed to a field.
    parents: Vec<(Parent, Option<Name>)>,
    /// `over`: matchers defined here are overlays.
    overlay: bool,
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

    fn push_source(&mut self, name: String, path: Option<PathBuf>, text: String) {
        let text: Arc<str> = Arc::from(text);
        let id = self.sources.add(name, path, text.clone());
        let tokens = lex(id, &text, 0, &mut self.errors);
        self.ts.push_tokens(tokens);
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

    /// Rule names may contain dashes (`no-dbg`) and, in `set`, `*` globs.
    /// Pieces must touch: `no - dbg` is not a name.
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
            (Tk::Ident, "macro") => self.macro_def(),
            (Tk::Ident, "lang") => self.lang(),
            (Tk::Ident, "group") => self.group(),
            (Tk::Ident, "in" | "over") => self.context_item(),
            (Tk::Ident, "match") => self.matcher(ctx),
            (Tk::Ident, "rule") => self.rule(ctx),
            (Tk::Ident, "indent") => self.indent(ctx),
            (Tk::Ident, "set") => self.set(),
            (Tk::Ident, "ignore") => self.ignore(),
            (Tk::Ident, _) if self.ts.expand(&mut self.errors) => return,
            _ => {
                self.errors.push(
                    ConfigError::new(t.span, format!("expected an item, found {}", t.describe()))
                        .note("items are: lang, group, match, in/over ..., rule, indent, set, macro, import, ignore"),
                );
                self.bump();
            }
        }
        if self.errors.len() > before {
            // Keep one error per broken item.
            self.recover_if_mid_line();
        }
    }

    /// `in A or B ...` / `over A ...`: a context for the rest of the line,
    /// or for every line of the `{ ... }` block that follows.
    fn context_item(&mut self) {
        let kw = self.bump().expect("peeked");
        let parents = self.parent_items();
        if parents.is_empty() {
            return;
        }
        let ctx = Context { parents, overlay: &*kw.text == "over" };
        self.skip(false, true);
        match self.ts.peek_raw(0).cloned() {
            Some(t) if t.kind == Tk::LBrace => {
                self.ts.next_raw();
                self.block(&ctx, t.span);
            }
            Some(t) if t.is_ident("match") => self.matcher(Some(&ctx)),
            Some(t) if t.is_ident("rule") => self.rule(Some(&ctx)),
            Some(t) if t.is_ident("indent") => self.indent(Some(&ctx)),
            other => {
                let span = other.map(|t| t.span).or(self.ts.last_span);
                self.errors.push(ConfigError::new(span, format!("expected `match`, `rule`, `indent` or `{{` after `{} ...`", kw.text)));
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
            Some(c) if !c.overlay => self.config.indents.push(IndentDef { targets: c.parents.clone() }),
            Some(_) => self.errors.push(ConfigError::new(kw.span, "`indent` needs `in`, not `over`")),
            None => self.errors.push(ConfigError::new(kw.span, "`indent` needs a context: `in <layer> indent`")),
        }
        self.end_of_item();
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

    fn import(&mut self) {
        let kw = self.bump().expect("peeked");
        let Some((path, span)) = self.string("an import path") else { return };
        self.end_of_item();

        if let Some(name) = path.strip_prefix("std/") {
            let key = format!("std/{name}");
            if !self.imported.insert(key.clone()) {
                return;
            }
            match std_lib::get(name) {
                Some(text) => self.push_source(key, None, text.to_string()),
                None => self.errors.push(
                    ConfigError::new(span, format!("unknown std module `{path}`")).suggest(name, std_lib::names()),
                ),
            }
            return;
        }

        let base = self.sources.get(kw.span.file).path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf));
        let Some(base) = base else {
            self.errors.push(ConfigError::new(span, "relative imports need the importing file to live on disk"));
            return;
        };
        let mut full = base.join(&path);
        if full.extension().is_none() {
            full.set_extension("zlint");
        }
        let full = normalize_path(&full);
        let key = full.to_string_lossy().into_owned();
        if !self.imported.insert(key) {
            return;
        }
        match self.provider.read(&full) {
            Ok(text) => self.push_source(full.display().to_string(), Some(full), text),
            Err(e) => self.errors.push(ConfigError::new(span, format!("cannot import `{}`: {e}", full.display()))),
        }
    }

    fn macro_def(&mut self) {
        self.bump();
        let Some(name) = self.ident("a macro name") else { return };
        if RESERVED.contains(&name.text.as_str()) {
            self.errors.push(ConfigError::new(name.span, format!("`{}` is a keyword and cannot name a macro", name.text)));
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
        let def = MacroDef { name: Rc::from(name.text.as_str()), params, body: body.into() };
        self.ts.macros.insert(def.name.clone(), Rc::new(def));
    }

    fn group(&mut self) {
        self.bump();
        let Some(name) = self.ident("a group name") else { return };
        let parents = self.parent_list();
        if parents.is_empty() {
            return;
        }
        self.end_of_item();
        self.config.groups.push(GroupDef { name, parents });
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
        self.block_or_end(Context { parents: vec![(root, None)], overlay: false });
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

    /// Eats an `or` that continues the current line.
    fn eat_or(&mut self) -> bool {
        self.skip(false, true);
        if self.ts.peek_raw(0).is_some_and(|t| t.is_ident("or")) {
            self.ts.next_raw();
            true
        } else {
            false
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
            if !self.eat_or() {
                break;
            }
        }
        parents
    }

    /// `match name [like base] [pattern] [{ block }]`, with its parents from
    /// the context. Without a pattern it attaches an existing matcher.
    fn matcher(&mut self, ctx: Option<&Context>) {
        self.bump();
        let (parents, overlay) = match ctx {
            Some(c) => (self.no_fields(c.parents.clone()), c.overlay),
            None => (Vec::new(), false),
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
        self.config.matchers.push(MatcherDef { name, parents, overlay, pattern, like });
        self.block_or_end(Context { parents: vec![(own, None)], overlay: false });
    }

    /// `rule name [severity [message]] [fix as expr] [{ clauses }]`, checking the
    /// context's layers.
    fn rule(&mut self, ctx: Option<&Context>) {
        let kw = self.bump().expect("peeked");
        let Some(name) = self.glued_name("a rule name", false) else { return };
        let targets: Vec<Target> = match ctx {
            Some(c) => c.parents.iter().map(target_of).collect(),
            None => {
                self.errors.push(ConfigError::new(kw.span, "a rule needs a context: `in <layer> rule name ...`"));
                return;
            }
        };
        let mut def = RuleDef {
            name,
            targets,
            severity: None,
            message: None,
            options: Vec::new(),
            files: Vec::new(),
            ignore: Vec::new(),
            when: Vec::new(),
            fix: None,
            report: Report::Node,
            broken: false,
        };
        let before = self.errors.len();
        self.skip(false, true);
        if let Some(severity) = self.peek_severity() {
            self.ts.next_raw();
            def.severity = Some(severity);
            self.message_after_severity(&mut def);
        }
        self.skip(false, true);
        if self.ts.peek_raw(0).is_some_and(|t| t.is_ident("fix")) {
            self.ts.next_raw();
            def.fix = self.fix_as();
        }
        if self.errors.len() == before {
            self.skip(false, true);
            if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::LBrace) {
                self.ts.next_raw();
                self.rule_body(&mut def);
            } else {
                self.end_of_item();
            }
        }
        // After a broken head, the caller skips the rest of the line, body
        // included.
        def.broken = self.errors.len() > before;
        self.config.rules.push(def);
    }

    /// After `fix`: `as expr`.
    fn fix_as(&mut self) -> Option<Expr> {
        self.skip(false, true);
        match self.ts.peek_raw(0).cloned() {
            Some(t) if t.is_ident("as") => {
                self.ts.next_raw();
                self.expr()
            }
            other => {
                let span = other.map(|t| t.span).or(self.ts.last_span);
                self.errors.push(ConfigError::new(span, "expected `as` after `fix`: `fix as <what the text should be>`"));
                None
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

    fn rule_body(&mut self, def: &mut RuleDef) {
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
                (Tk::Ident, "option") => {
                    self.bump();
                    if let Some(name) = self.ident("an option name") {
                        if let Some(default) = self.operand() {
                            self.skip(false, true);
                            let description = if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Str) {
                                self.string("an option description").map(|(s, _)| s)
                            } else {
                                None
                            };
                            def.options.push(OptionDef { name, default, description });
                        }
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
                        def.fix = self.fix_as();
                    }
                }
                (Tk::Ident, "when") => {
                    self.bump();
                    if let Some(e) = self.expr() {
                        def.when.push(e);
                    }
                }
                (Tk::Ident, "report") => {
                    self.bump();
                    if let Some(what) = self.ident("`node`, `change` or `end`") {
                        match what.text.as_str() {
                            "node" => def.report = Report::Node,
                            "change" => def.report = Report::Change,
                            "end" => def.report = Report::End,
                            other => self.errors.push(
                                ConfigError::new(what.span, format!("unknown report target `{other}`")).note(
                                    "`report node` points at the whole node, `report change` at what the fix changes, `report end` at the node's end",
                                ),
                            ),
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
                            .note("rule clauses: a severity and message (`warn \"...\"`), when, fix, option, files, ignore, report"),
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

    /// `set rule.option value`, or `set rule severity`. `rule` may glob.
    fn set(&mut self) {
        self.bump();
        let Some(target) = self.glued_name("a rule name or glob", true) else { return };
        self.skip(false, true);
        let (key, value) = if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Dot) {
            self.ts.next_raw();
            let Some(key) = self.ident("an option name") else { return };
            let Some(e) = self.expr() else { return };
            (key, SetValue::Expr(e))
        } else {
            let Some(level) = self.ident("a severity (`off`, `error`, ...) or `.option value`") else { return };
            match Severity::parse(&level.text) {
                Some(s) => (Name { text: "severity".into(), span: level.span }, SetValue::Severity(s)),
                None => {
                    self.errors.push(
                        ConfigError::new(level.span, format!("unknown severity `{}`", level.text))
                            .suggest(&level.text, Severity::NAMES.iter().copied()),
                    );
                    return;
                }
            }
        };
        self.end_of_item();
        self.config.sets.push(SetDef { target, key, value });
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
            in dstr or sstr match escape /\\(.)/
            in .rust match sstr /'/ until /'/
            match params /\(/ until /\)/
            in params match params
            in .rust match fn /fn/ ws name:/\w+/ ws? :params
            over .rust match line /^.*$/
            in .rust match either (/a/ or "b") until /c/
        "#);
        assert_eq!(c.langs.len(), 1);
        assert_eq!(c.matchers.len(), 9);
        let fun = c.matchers.iter().find(|m| m.name.text == "fn").unwrap();
        assert_eq!(fun.pattern.as_ref().unwrap().0.labels(), vec!["name", "params"]);
        assert!(c.matchers.iter().find(|m| m.name.text == "line").unwrap().overlay);
        assert!(c.matchers.iter().any(|m| m.name.text == "params" && m.pattern.is_none()));
        let either = &c.matchers.iter().find(|m| m.name.text == "either").unwrap().pattern.as_ref().unwrap().0;
        assert!(matches!(either, Pat::Seq(items) if matches!(items[0], Pat::Alt(_)) && matches!(items[1], Pat::Skip)), "{either:?}");
    }

    #[test]
    fn rules_in_one_line_or_with_a_body() {
        let c = ok(r#"
            in fn.name rule fn-snake warn "names are snake_case" fix as snake_case(text)
            in line rule line-length {
              option max 100 "max chars"
              error "lines under {max}"
              when len(text) > max
            }
            in todo or .md rule todo-note info "{text} left"
            in x rule quiet
            set line-length.max 120
            set * warn
        "#);
        assert_eq!(c.rules.len(), 4);
        assert!(matches!(c.rules[0].targets[..], [Target::Field(..)]));
        assert!(c.rules[0].fix.is_some() && c.rules[0].message.is_some());
        assert_eq!(c.rules[1].options.len(), 1);
        assert_eq!(c.rules[1].severity, Some(Severity::Error));
        assert!(matches!(c.rules[2].targets[..], [Target::Matcher(_), Target::Lang(_)]));
        assert_eq!(c.rules[2].severity, Some(Severity::Info));
        assert!(c.rules[3].severity.is_none() && c.rules[3].message.is_none());
        assert_eq!(c.sets.len(), 2);
        assert_eq!(c.sets[1].target.text, "*");
        assert!(matches!(c.sets[1].value, SetValue::Severity(Severity::Warn)));
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
            over .t or brace {
              match line /^.+/
            }
            in brace { in .t match back /y/ }
            in brace.body indent
        "#);
        let by_name = |n: &str| c.matchers.iter().find(|m| m.name.text == n).unwrap();
        assert_eq!(parents(by_name("brace")), vec![".t"]);
        assert_eq!(parents(by_name("inner")), vec!["brace"]);
        assert_eq!(parents(by_name("line")), vec![".t", "brace"]);
        assert!(by_name("line").overlay && !by_name("inner").overlay);
        // A nested `in` replaces the block's context.
        assert_eq!(parents(by_name("back")), vec![".t"]);
        assert!(matches!(&c.rules[0].targets[..], [Target::Matcher(n)] if n.text == "brace"));
        assert_eq!(c.indents.len(), 2);
        assert!(matches!(&c.indents[1].targets[..], [(Parent::Matcher(_), Some(f))] if f.text == "body"));
    }

    #[test]
    fn groups_add_on_every_line() {
        let c = ok("group code .t or paren\ngroup code brace\n");
        assert_eq!(c.groups.len(), 2);
        assert_eq!(c.groups[1].parents.len(), 1);
    }

    #[test]
    fn rules_need_a_context_and_matchers_take_no_fields() {
        let (_, errors, _) = parse_str("rule lonely warn \"x\"\nin a.b match c /x/\nin a indent extra\n");
        let all: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(errors.len(), 3, "{all:#?}");
        assert!(all[0].contains("needs a context") && all[1].contains("can use a field"), "{all:#?}");
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
              option max 100 "max chars"
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
        assert!(matches!(rule.options[0].default.kind, ExprKind::Num(_)) && rule.options[0].description.is_some());
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
