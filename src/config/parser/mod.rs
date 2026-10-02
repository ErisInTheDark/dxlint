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

/// Words that start formatter body clauses.
const FORMAT_CLAUSES: &[&str] = &["option", "severity", "when", "message", "files", "ignore", "report"];

/// Words that start items or clauses and therefore cannot name macros.
const RESERVED: &[&str] = &[
    "import", "macro", "lang", "group", "match", "in", "over", "format", "on", "set", "ignore", "option", "severity", "when",
    "message", "files", "report", "if", "then", "else", "true", "false", "null",
];

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

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.peek().is_some_and(|t| t.is_ident(kw)) {
            self.bump();
            true
        } else {
            false
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
            // `family:variant` names group several formatters under one
            // rule family (`set family:*.severity = off`).
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
    fn recover(&mut self) {
        let mut depth = 0usize;
        while let Some(t) = self.ts.next_raw() {
            match t.kind {
                Tk::LParen | Tk::LBrace | Tk::LBrack => depth += 1,
                Tk::RParen | Tk::RBrace | Tk::RBrack => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && t.kind == Tk::RBrace {
                        return;
                    }
                }
                Tk::Newline if depth == 0 => return,
                _ => {}
            }
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
        while let Some(t) = self.peek() {
            let before = self.errors.len();
            match (t.kind, &*t.text) {
                (Tk::Ident, "import") => self.import(),
                (Tk::Ident, "macro") => self.macro_def(),
                (Tk::Ident, "lang") => self.lang(),
                (Tk::Ident, "group") => self.group(),
                (Tk::Ident, "match" | "in" | "over") => self.matcher(),
                (Tk::Ident, "format") => self.format(),
                (Tk::Ident, "set") => self.set(),
                (Tk::Ident, "ignore") => self.ignore(),
                (Tk::Ident, _) if self.ts.expand(&mut self.errors) => continue,
                _ => {
                    self.errors.push(
                        ConfigError::new(t.span, format!("expected an item, found {}", t.describe()))
                            .note("items are: lang, group, match, in/over ... match, format, set, macro, import, ignore"),
                    );
                    self.bump();
                    self.recover();
                }
            }
            if self.errors.len() > before {
                // Keep one error per broken item.
                self.recover_if_mid_line();
            }
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
        if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::LParen) {
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
        match self.ts.next_raw() {
            Some(t) if t.kind == Tk::Eq => loop {
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
            Some(t) if t.kind == Tk::LBrace => loop {
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
            },
            other => {
                let span = other.map(|t| t.span).unwrap_or(name.span);
                self.errors.push(ConfigError::new(span, "expected `=` or `{` after the macro name"));
                return;
            }
        }
        let def = MacroDef { name: Rc::from(name.text.as_str()), params, body: body.into() };
        self.ts.macros.insert(def.name.clone(), Rc::new(def));
    }

    fn group(&mut self) {
        self.bump();
        let Some(name) = self.ident("a group name") else { return };
        self.skip(false, true);
        let extend = self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Plus);
        if extend {
            self.ts.next_raw();
        }
        if self.expect(Tk::Eq, if extend { "`=` after `+`" } else { "`=` or `+=`" }).is_none() {
            return;
        }
        let parents = self.parent_list();
        if parents.is_empty() {
            return;
        }
        self.end_of_item();
        self.config.groups.push(GroupDef { name, extend, parents });
    }

    fn lang(&mut self) {
        self.bump();
        let Some(name) = self.ident("a language name") else { return };
        if self.expect(Tk::Eq, "`=`").is_none() {
            return;
        }
        let mut globs = Vec::new();
        loop {
            let Some(glob) = self.string("a file glob like \"*.rs\"") else { return };
            globs.push(glob);
            if !self.eat_on_line(Tk::Pipe) {
                break;
            }
        }
        self.end_of_item();
        self.config.langs.push(LangDef { name, globs });
    }

    /// Eats `kind` only if it continues the current line.
    fn eat_on_line(&mut self, kind: Tk) -> bool {
        self.skip(false, true);
        if self.ts.peek_raw(0).is_some_and(|t| t.kind == kind) {
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
                    self.errors.push(ConfigError::new(f.span, "only `in layer.field indent` can name a field here"));
                }
                p
            })
            .collect()
    }

    /// `a | .lang | b.field`: parents, each maybe narrowed to a field.
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
            if !self.eat_on_line(Tk::Pipe) {
                break;
            }
        }
        parents
    }

    fn matcher(&mut self) {
        let kw = self.bump().expect("peeked");
        let (parents, overlay) = match &*kw.text {
            "in" => {
                let items = self.parent_items();
                if self.eat_kw("indent") {
                    self.end_of_item();
                    self.config.indents.push(IndentDef { targets: items });
                    return;
                }
                (self.no_fields(items), false)
            }
            "over" => (self.parent_list(), true),
            _ => (Vec::new(), false),
        };
        if &*kw.text != "match" && !self.eat_kw("match") {
            let also = if &*kw.text == "in" { " or `indent`" } else { "" };
            self.error_here(format!("expected `match`{also} after `{} ...`", kw.text));
            return;
        }
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
        let pattern = if self.eat_on_line(Tk::Eq) {
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
                        .note("write `match name = /regex/ ...`, or `in parent match name` to attach an existing matcher"),
                );
                return;
            }
            None
        };
        self.end_of_item();
        if like.is_some() && pattern.is_none() {
            self.errors.push(ConfigError::new(name.span, "`like` belongs on the definition (`match name like base = ...`)"));
            return;
        }
        self.config.matchers.push(MatcherDef { name, parents, overlay, pattern, like });
    }

    fn target(&mut self) -> Option<Target> {
        let t = self.peek()?;
        if t.kind == Tk::Dot {
            self.bump();
            let n = self.ts.peek_raw(0).cloned();
            return match n {
                Some(n) if (n.kind == Tk::Ident || n.kind == Tk::Star) && t.touches(&n) => {
                    self.ts.next_raw();
                    Some(Target::Lang(Name { text: n.text.to_string(), span: t.span.to(n.span) }))
                }
                _ => {
                    self.errors.push(ConfigError::new(t.span, "expected a language name right after `.`"));
                    None
                }
            };
        }
        let layer = self.ident("a matcher to format, like `fn` or `fn.name`")?;
        let dot_glued = self.ts.peek_raw(0).is_some_and(|d| d.kind == Tk::Dot && d.span.start == layer.span.end);
        if dot_glued {
            self.ts.next_raw();
            let field = self.ident("a field name after `.`")?;
            return Some(Target::Field(layer, field));
        }
        Some(Target::Matcher(layer))
    }

    fn format(&mut self) {
        self.bump();
        let Some(name) = self.glued_name("a formatter name", false) else { return };
        if !self.eat_kw("on") {
            self.error_here(format!("expected `on <matcher>` after `format {}`", name.text));
            return;
        }
        let mut targets = Vec::new();
        loop {
            let Some(target) = self.target() else { return };
            targets.push(target);
            if !self.eat_on_line(Tk::Pipe) {
                break;
            }
        }
        let mut def = FormatDef {
            name,
            targets,
            description: None,
            severity: None,
            options: Vec::new(),
            files: Vec::new(),
            ignore: Vec::new(),
            when: Vec::new(),
            message: None,
            shape: None,
            report: Report::Node,
        };
        self.skip(false, true);
        // A description is a string, or a macro that expands to one.
        let described = match self.ts.peek_raw(0) {
            Some(t) if t.kind == Tk::Str => true,
            Some(t) if t.kind == Tk::Ident => {
                let name = t.text.clone();
                self.ts.is_macro(&name)
            }
            _ => false,
        };
        if described {
            def.description = self.expr();
        }
        self.skip(false, true);
        match self.ts.peek_raw(0).map(|t| t.kind) {
            Some(Tk::Eq) => {
                self.ts.next_raw();
                def.shape = self.expr();
                self.end_of_item();
            }
            Some(Tk::LBrace) => {
                self.ts.next_raw();
                self.format_body(&mut def);
            }
            _ => self.end_of_item(),
        }
        self.config.formats.push(def);
    }

    fn format_body(&mut self, def: &mut FormatDef) {
        loop {
            let Some(t) = self.peek() else {
                self.errors.push(ConfigError::new(def.name.span, format!("unclosed body of formatter `{}`", def.name.text)));
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
                        if self.expect(Tk::Eq, "`=` and a default value").is_some() {
                            if let Some(default) = self.expr() {
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
                }
                (Tk::Ident, "severity") => {
                    self.bump();
                    if let Some(level) = self.ident("a severity") {
                        match Severity::parse(&level.text) {
                            Some(s) => def.severity = Some(s),
                            None => self.errors.push(
                                ConfigError::new(level.span, format!("unknown severity `{}`", level.text))
                                    .suggest(&level.text, Severity::NAMES.iter().copied()),
                            ),
                        }
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
                                    "`report node` points at the whole node, `report change` at what the shape changes, `report end` at the node's end",
                                ),
                            ),
                        }
                    }
                }
                (Tk::Ident, "message") => {
                    self.bump();
                    def.message = self.expr();
                }
                (Tk::Ident, kw @ ("files" | "ignore")) => {
                    let kw = kw.to_string();
                    self.bump();
                    let mut globs = Vec::new();
                    while let Some(g) = self.string("a path glob") {
                        globs.push(g);
                        if !self.eat_on_line(Tk::Pipe) {
                            break;
                        }
                    }
                    if kw == "files" {
                        def.files.extend(globs);
                    } else {
                        def.ignore.extend(globs);
                    }
                }
                (Tk::Ident, _) if self.ts.is_macro(&t.text) => {
                    self.ts.expand(&mut self.errors);
                    // A macro of clauses is read as clauses; anything else is
                    // the shape, parsed with its invisible group intact so
                    // `macro + "x"` stays one expression.
                    let first = self.ts.peek_raw(1).cloned();
                    if first.is_some_and(|f| f.kind == Tk::Ident && FORMAT_CLAUSES.contains(&&*f.text)) {
                        continue;
                    }
                    self.shape(def)
                }
                _ => self.shape(def),
            }
            if self.errors.len() > before {
                self.recover_if_mid_line();
            } else {
                self.end_of_item();
            }
        }
    }

    fn shape(&mut self, def: &mut FormatDef) {
        let span = self.here();
        let Some(e) = self.expr() else { return };
        if def.shape.is_some() {
            self.errors.push(
                ConfigError::new(span, format!("formatter `{}` already has a shape", def.name.text))
                    .note("a formatter has at most one shape: the expression saying what the text should be"),
            );
        }
        def.shape = Some(e);
    }

    fn set(&mut self) {
        self.bump();
        let Some(target) = self.glued_name("a formatter name or glob", true) else { return };
        if self.expect(Tk::Dot, "`.` and an option name, like `rule.max`").is_none() {
            return;
        }
        let Some(key) = self.ident("an option name") else { return };
        if self.expect(Tk::Eq, "`=`").is_none() {
            return;
        }
        let value = if key.text == "severity" {
            let Some(level) = self.ident("a severity") else { return };
            match Severity::parse(&level.text) {
                Some(s) => SetValue::Severity(s),
                None => {
                    self.errors.push(
                        ConfigError::new(level.span, format!("unknown severity `{}`", level.text))
                            .suggest(&level.text, Severity::NAMES.iter().copied()),
                    );
                    return;
                }
            }
        } else {
            let Some(e) = self.expr() else { return };
            SetValue::Expr(e)
        };
        self.end_of_item();
        self.config.sets.push(SetDef { target, key, value });
    }

    fn ignore(&mut self) {
        self.bump();
        loop {
            let Some(g) = self.string("a path glob") else { return };
            self.config.ignores.push(g);
            if !self.eat_on_line(Tk::Pipe) {
                break;
            }
        }
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

    #[test]
    fn parses_the_design_sketch() {
        let c = ok(r#"
            lang rust = "*.rs"
            match ws = /\s+/
            in .rust match dstr = /"/ * /"/
            in dstr | sstr match escape = /\\(.)/
            in .rust match sstr = /'/ * /'/
            match params = /\(/ * /\)/
            in params match params
            in .rust match fn = /fn/ ws name:/\w+/ ws? :params
            over .rust match line = /^.*$/
        "#);
        assert_eq!(c.langs.len(), 1);
        assert_eq!(c.matchers.len(), 8);
        let fun = c.matchers.iter().find(|m| m.name.text == "fn").unwrap();
        assert_eq!(fun.pattern.as_ref().unwrap().0.labels(), vec!["name", "params"]);
        assert!(c.matchers.iter().find(|m| m.name.text == "line").unwrap().overlay);
        assert!(c.matchers.iter().any(|m| m.name.text == "params" && m.pattern.is_none()));
    }

    #[test]
    fn formatters_with_body_shorthand_and_sets() {
        let c = ok(r#"
            format fn-snake on fn.name "names are snake_case" = snake_case(text)
            format line-length on line "lines under {max}" {
              option max = 100 "max chars"
              severity error
              when len(text) > max
            }
            set line-length.max = 120
            set *.severity = warn
        "#);
        assert_eq!(c.formats.len(), 2);
        assert!(matches!(c.formats[0].targets[..], [Target::Field(..)]));
        assert!(c.formats[0].shape.is_some());
        assert_eq!(c.formats[1].options.len(), 1);
        assert_eq!(c.formats[1].severity, Some(Severity::Error));
        assert_eq!(c.sets.len(), 2);
        assert_eq!(c.sets[1].target.text, "*");
    }

    #[test]
    fn macros_generate_items() {
        let c = ok(r#"
            macro forbid(name, pat, why) {
              in .rust match name = pat
              format name on name why { severity error }
            }
            forbid(dbg, "dbg!" /\(/, "no dbg")
        "#);
        assert_eq!(c.matchers[0].name.text, "dbg");
        assert_eq!(c.formats[0].name.text, "dbg");
        assert_eq!(c.formats[0].severity, Some(Severity::Error));
    }

    #[test]
    fn macros_start_shapes_or_clauses_in_formatter_bodies() {
        let c = ok(r#"
            macro nl = (if true then "a" else "b")
            macro strict = severity error
            format f on x {
              strict
              nl + "!" + nl
            }
        "#);
        assert_eq!(c.formats[0].severity, Some(Severity::Error));
        assert!(matches!(c.formats[0].shape.as_ref().map(|e| &e.kind), Some(ExprKind::Binary(..))));
    }

    #[test]
    fn macro_expansions_act_as_groups() {
        let c = ok("macro digits = /\\d/+\nmatch x = \"a\" digits?\n");
        let Some((Pat::Seq(items), _)) = &c.matchers[0].pattern else { panic!("{:?}", c.matchers[0].pattern) };
        assert!(
            matches!(&items[1], Pat::Repeat { min: 0, max: Some(1), inner, .. } if matches!(**inner, Pat::Repeat { min: 1, .. })),
            "{items:?}"
        );
    }

    #[test]
    fn errors_point_at_the_problem_and_recover() {
        let (c, errors, _) = parse_str("match = /x/\nmatch ok = /y/\nmatch bad = /(/\n");
        assert_eq!(errors.len(), 2, "{errors:#?}");
        // `bad` stays declared (matching nothing) so its uses don't cascade.
        let names: Vec<&str> = c.matchers.iter().map(|m| m.name.text.as_str()).collect();
        assert_eq!(names, vec!["ok", "bad"]);
    }
}
