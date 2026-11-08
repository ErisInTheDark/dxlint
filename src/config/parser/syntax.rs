//! Structured grammar declarations beside byte-pattern declarations.

mod expressions;
mod precedence;
mod resolve;

use super::Parser;
use crate::config::ast::{Name, Parent, ParserDecl};
use crate::config::error::ConfigError;
use crate::config::lexer::{decode_escapes, Tk, Token};
use crate::pattern::syntax::{Definition, Expr, Field, Goal, Node, Production};
use crate::pattern::{regex, Pat};

impl Parser<'_> {
    pub(super) fn syntax_parser(&mut self) {
        self.bump();
        let Some(name) = self.ident("a parser name") else { return; };
        if !self.word("in") {
            self.error_here("expected `in` and a language after the parser name");
            return;
        }
        let Some(language) = self.ident("a language name") else { return; };
        if self.expect(Tk::LBrace, "`{` after the parser language").is_none() { return; }
        let mut declaration = ParserDecl {
            name: name.clone(), language,
            definition: Definition {
                name: name.text.clone(), trivia: Vec::new(), line_break: Pat::Empty,
                tokens: Vec::new(), goals: Vec::new(), flags: Vec::new(), nodes: Vec::new(),
                productions: Vec::new(), root: String::new(),
            },
            option_flags: Vec::new(), name_escapes: Vec::new(), file_flags: Vec::new(),
        };
        let before = self.errors.len();
        self.syntax_items(&mut declaration, false);
        let definition = &declaration.definition;
        if definition.root.is_empty() {
            self.errors.push(ConfigError::new(name.span, "a parser needs a root production"));
        }
        if matches!(definition.line_break, Pat::Empty) {
            self.errors.push(ConfigError::new(name.span, "a parser needs a line_break pattern"));
        }
        if self.errors.len() == before {
            if let Err(message) = resolve::resolve(&mut declaration.definition) {
                self.errors.push(ConfigError::new(name.span, message));
            } else {
                self.config.parsers.push(declaration);
            }
        }
    }

    /// `extend parser name { ... }`: more items for a parser declared above,
    /// where `override production x = ...` replaces a production and
    /// `production x |= ...` adds an alternative after its own.
    pub(super) fn syntax_extension(&mut self) {
        self.bump();
        if !self.word("parser") {
            self.error_here("expected `parser` after `extend`: `extend parser name { ... }`");
            return;
        }
        let Some(name) = self.ident("a parser name") else { return; };
        if self.expect(Tk::LBrace, "`{` after the parser name").is_none() { return; }
        let Some(index) = self.config.parsers.iter().position(|parser| parser.name.text == name.text) else {
            self.errors.push(ConfigError::new(name.span, format!("no parser `{}` above to extend", name.text)));
            self.recover();
            return;
        };
        let mut declaration = self.config.parsers[index].clone();
        let known = declaration.definition.nodes.len();
        let before = self.errors.len();
        self.syntax_items(&mut declaration, true);
        if self.errors.len() != before { return; }
        if let Err(message) = resolve::resolve(&mut declaration.definition) {
            self.errors.push(ConfigError::new(name.span, message));
            return;
        }
        // New node kinds join the front end's `<name>_node`/`<name>_code` groups.
        let definition = &declaration.definition;
        for node in &definition.nodes[known..] {
            if definition.tokens.iter().any(|token| token.visible_kind.as_deref() == Some(node.name.as_str())) { continue }
            for suffix in ["node", "code"].into_iter().filter(|suffix| *suffix == "node" || !node.literal_text) {
                let group_name = format!("{}_{suffix}", name.text);
                if let Some(group) = self.config.groups.iter_mut().find(|group| group.name.text == group_name) {
                    group.parents.push(Parent::Matcher(Name { text: node.name.clone(), span: name.span }));
                }
            }
        }
        self.config.parsers[index] = declaration;
    }

    fn syntax_items(&mut self, declaration: &mut ParserDecl, extending: bool) {
        let name = declaration.name.clone();
        let definition = &mut declaration.definition;
        let option_flags = &mut declaration.option_flags;
        let name_escapes = &mut declaration.name_escapes;
        // An extension's `|=` and `override` edits, applied once its `files` are known.
        let mut edits: Vec<(Name, bool, Expr)> = Vec::new();
        let mut files = Vec::new();
        loop {
            let Some(token) = self.peek() else {
                self.errors.push(ConfigError::new(name.span, "unclosed parser declaration"));
                break;
            };
            if token.kind == Tk::RBrace {
                self.bump();
                break;
            }
            let item_errors = self.errors.len();
            match (token.kind, &*token.text) {
                (Tk::Ident, "override") if extending => {
                    self.bump();
                    if !self.word("production") {
                        self.error_here("expected `production` after `override` in a parser");
                    } else if let Some(production) = self.ident("a production name") {
                        let body = self.expect(Tk::Eq, "`=` before a production body").and_then(|_| self.syntax_expression(0));
                        match (definition.productions.iter().any(|own| own.name == production.text), body) {
                            (true, Some(body)) => edits.push((production, false, body)),
                            (false, _) => self.errors.push(ConfigError::new(production.span, format!("no production `{}` to override", production.text))),
                            (_, None) => {}
                        }
                    }
                }
                (Tk::Ident, "files") if extending => {
                    self.bump();
                    files.extend(self.globs("a path glob"));
                }
                (Tk::Ident, "token") => {
                    self.bump();
                    if let Some(token_name) = self.ident("a token name") {
                        if let Some(pattern) = self.syntax_pattern() {
                            let lines = self.syntax_peek(0).is_some_and(|token| token.is_ident("lines"));
                            if lines { self.ts.next_raw(); }
                            let visible_kind = if self.syntax_peek(0).is_some_and(|token| token.is_ident("as")) {
                                self.ts.next_raw();
                                self.ident("a public token kind").map(|name| name.text)
                            } else { None };
                            definition.tokens.push(crate::pattern::syntax::Token { name: token_name.text, pattern, visible_kind, lines });
                        }
                    }
                }
                (Tk::Ident, "trivia") => {
                    self.bump();
                    if let Some(pattern) = self.syntax_pattern() { definition.trivia.push(pattern); }
                }
                (Tk::Ident, "line_break") => {
                    self.bump();
                    if let Some(pattern) = self.syntax_pattern() { definition.line_break = pattern; }
                }
                (Tk::Ident, "name_escape") => {
                    self.bump();
                    match self.bump() {
                        Some(regex) if regex.kind == Tk::Regex => match crate::expr::re::compile(&regex.text) {
                            Ok(compiled) if compiled.group_info().group_len(regex_automata::PatternID::ZERO) > 1 => name_escapes.push(regex.text.to_string()),
                            Ok(_) => self.errors.push(ConfigError::new(regex.span, "a name escape needs a group capturing the hex code point")),
                            Err(message) => self.errors.push(ConfigError::new(regex.span, message)),
                        },
                        Some(other) => self.errors.push(ConfigError::new(other.span, "expected a regex: `name_escape /\\\\u([0-9a-fA-F]{4})/`")),
                        None => {}
                    }
                }
                (Tk::Ident, "flag") => {
                    self.bump();
                    if let Some(flag) = self.ident("a flag name") {
                        if self.syntax_peek(0).is_some_and(|token| token.is_ident("option")) {
                            self.ts.next_raw();
                            option_flags.push(flag.text.clone());
                        }
                        definition.flags.push(flag.text);
                    }
                }
                (Tk::Ident, "goal") => {
                    if let Some(goal) = self.syntax_goal() { definition.goals.push(goal); }
                }
                (Tk::Ident, "node") => {
                    if let Some(node) = self.syntax_node() { definition.nodes.push(node); }
                }
                (Tk::Ident, "production" | "root") => {
                    self.bump();
                    if let Some(production) = self.ident("a production name") {
                        if Self::syntax_constructor_name(&production.text) {
                            self.errors.push(ConfigError::new(production.span, "grammar constructor names are reserved"));
                        }
                        let own = definition.productions.iter().position(|own| own.name == production.text);
                        let alternative = extending && self.syntax_peek(1).is_some_and(|token| token.kind == Tk::Pipe);
                        let body = if alternative {
                            self.ts.next_raw();
                            self.expect(Tk::Eq, "`=` after `|`: `production x |= ...`").and_then(|_| self.syntax_expression(0))
                        } else if self.word("precedence") {
                            self.syntax_precedence()
                        } else {
                            self.expect(Tk::Eq, "`=` before a production body")
                                .and_then(|_| self.syntax_expression(0))
                        };
                        match (body, own) {
                            (Some(body), Some(_)) if alternative => edits.push((production, true, body)),
                            (Some(_), None) if alternative => self.errors.push(ConfigError::new(production.span,
                                format!("no production `{}` to add an alternative to", production.text))),
                            (Some(_), Some(_)) if extending => self.errors.push(ConfigError::new(production.span,
                                format!("production `{}` exists: `override production` replaces it, `|=` adds an alternative", production.text))),
                            (Some(body), _) => {
                                if token.is_ident("root") {
                                    if !definition.root.is_empty() {
                                        self.errors.push(ConfigError::new(production.span, "a parser has one root"));
                                    }
                                    definition.root = production.text.clone();
                                }
                                definition.productions.push(Production { name: production.text, body });
                            }
                            (None, _) => {}
                        }
                    }
                }
                (Tk::Ident, _) if self.ts.expand(&mut self.errors) => continue,
                _ => {
                    self.errors.push(ConfigError::new(token.span, "parser items are token, trivia, line_break, name_escape, flag, goal, node, production and root"));
                    self.bump();
                }
            }
            if self.errors.len() > item_errors { self.recover(); }
            else { self.end_of_item(); }
        }
        // `files`: the edits hold only in matching files, through a flag
        // the front end sets by path; names with `:` are no user flag.
        let flag = (!files.is_empty()).then(|| {
            let flag = format!("files:{}", declaration.file_flags.len());
            definition.flags.push(flag.clone());
            declaration.file_flags.push((flag.clone(), files.into_iter().map(|(glob, _)| glob).collect()));
            flag
        });
        let scoped = |yes: Expr, no: Expr| match &flag {
            Some(flag) => Expr::Flag { name: flag.clone(), yes: Box::new(yes), no: Box::new(no) },
            None => yes,
        };
        for (production, alternative, body) in edits {
            let Some(own) = definition.productions.iter_mut().find(|own| own.name == production.text) else { continue };
            // A node's production holds its own emit: edits go inside it.
            let unwrap = |body: Expr| match body {
                Expr::Emit { kind, inner } if kind == production.text => *inner,
                body => body,
            };
            let target = match &mut own.body {
                Expr::Emit { kind, inner } if *kind == production.text => &mut **inner,
                body => body,
            };
            let old = std::mem::replace(target, Expr::Seq(Vec::new()));
            *target = if alternative {
                Expr::Choice(vec![old, scoped(body, Expr::Choice(Vec::new()))])
            } else {
                scoped(unwrap(body), old)
            };
        }
    }

    fn syntax_peek(&mut self, depth: usize) -> Option<Token> {
        self.skip(depth > 0, false);
        self.ts.peek_raw(0).cloned()
    }

    fn syntax_pattern(&mut self) -> Option<Pat> {
        let token = self.bump()?;
        let pattern = match token.kind {
            Tk::Regex => regex::parse(&token.text),
            Tk::Str => match decode_escapes(&token.text, token.span) {
                Ok(text) => Ok(Pat::Lit { bytes: text.into_bytes(), fold: false }),
                Err(error) => {
                    self.errors.push(error);
                    return None;
                }
            },
            _ => Err("expected a regex or literal token pattern".into()),
        };
        match pattern {
            Ok(pattern) if !pattern.nullable() => Some(pattern),
            Ok(_) => {
                self.errors.push(ConfigError::new(token.span, "token patterns must consume text"));
                None
            }
            Err(message) => {
                self.errors.push(ConfigError::new(token.span, message));
                None
            }
        }
    }

    fn syntax_goal(&mut self) -> Option<Goal> {
        self.bump();
        let name = self.ident("a goal name")?;
        let trivia = !self.word("raw");
        self.expect(Tk::LBrace, "`{` before goal tokens")?;
        let mut tokens = Vec::new();
        while let Some(token) = self.peek() {
            if token.kind == Tk::RBrace { self.bump(); break; }
            if token.kind == Tk::Comma { self.bump(); continue; }
            tokens.push(self.ident("a goal token")?.text);
        }
        Some(Goal { name: name.text, tokens, trivia })
    }

    fn syntax_node(&mut self) -> Option<Node> {
        self.bump();
        let name = self.ident("a public node kind")?;
        let literal_text = self.word("literal");
        self.expect(Tk::LBrace, "`{` before node fields")?;
        let mut fields = Vec::new();
        while let Some(token) = self.peek() {
            if token.kind == Tk::RBrace { self.bump(); break; }
            if token.kind == Tk::Comma { self.bump(); continue; }
            let field_token = self.expect(Tk::Ident, "a field name")?;
            if !self.field_name(&field_token) { return None; }
            let mut optional = false;
            if self.syntax_peek(1).is_some_and(|token| token.kind == Tk::Question) {
                self.ts.next_raw();
                optional = true;
            }
            self.expect(Tk::Colon, "`:` before a field shape")?;
            let mut kinds = Vec::new();
            let mut text = false;
            loop {
                let kind = self.ident("a node kind, production or `text`")?;
                if kind.text == "text" { text = true; }
                else { kinds.push(kind.text); }
                if self.syntax_peek(1).is_some_and(|token| token.kind == Tk::Pipe || token.is_ident("or")) {
                    self.ts.next_raw();
                } else { break; }
            }
            let mut repeated = false;
            if let Some(suffix) = self.syntax_peek(1) {
                match suffix.kind {
                    Tk::Question => { optional = true; self.ts.next_raw(); }
                    Tk::Star => { optional = true; repeated = true; self.ts.next_raw(); }
                    Tk::Plus => { repeated = true; self.ts.next_raw(); }
                    _ => {}
                }
            }
            fields.push(Field { name: field_token.text.to_string(), kinds, text, optional, repeated });
        }
        Some(Node { name: name.text, fields, literal_text })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::source::Disk;
    use crate::grammar::syntax::{CompiledParser, Context, Registration};

    #[test]
    fn production_references_can_be_followed_by_grouped_sequences() {
        let loaded = crate::config::load_text("grammar", r#"
            parser toy in js {
                token word /[a-z]+/
                trivia /\s+/
                line_break /[\r\n]/
                goal code { word }
                node root { value: text+ }
                production part = word
                root entry = emit(root, value:part (value:part) end)
            }
        "#, &Disk);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let parser = CompiledParser::compile(&loaded.config.parsers[0].definition).unwrap();
        let parsed = parser.parse_root("a b", Context::default(), &Registration {
            matchers: vec![10], labels: vec![vec![7]], text_matcher: crate::grammar::TEXT,
        }).unwrap();
        let root = &parsed.nodes[parsed.root as usize];
        let values: Vec<_> = root.children.iter().map(|edge| {
            let child = &parsed.nodes[edge.node as usize];
            &"a b"[child.start as usize..child.end as usize]
        }).collect();
        assert_eq!(values, vec!["a", "b"]);
    }

    /// `extend parser`: `|=` adds an alternative after a production's own,
    /// `override production` replaces one; a node's production keeps
    /// building its node.
    #[test]
    fn extensions_change_what_a_declared_parser_reads() {
        let base = r#"
            parser toy in js {
                token word /[a-z]+/
                token plus /\+/
                token star /\*/
                trivia /\s+/
                line_break /[\r\n]/
                goal code { word plus star }
                node root { value: item* }
                node item { name: text }
                production item = name:"a"
                production joiner = "+"
                root entry = emit(root, value:item (joiner value:item)* end)
            }
        "#;
        // `in_files`: whether the file matches the extension's `files` globs.
        let parse_in = |extension: &str, text: &str, in_files: bool| {
            let loaded = crate::config::load_text("grammar", &format!("{base}{extension}"), &Disk);
            assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
            let declaration = &loaded.config.parsers[0];
            let parser = CompiledParser::compile(&declaration.definition).unwrap();
            let mut context = Context::default();
            for (flag, _) in declaration.file_flags.iter().filter(|_| in_files) {
                context.flags |= 1 << parser.flags().iter().position(|known| known == flag).unwrap();
            }
            parser.parse_root(text, context, &Registration {
                matchers: vec![10, 11], labels: vec![vec![1], vec![2]], text_matcher: crate::grammar::TEXT,
            }).map(|parsed| parsed.nodes[parsed.root as usize].children.len())
        };
        let parse = |extension: &str, text: &str| parse_in(extension, text, false);
        assert!(parse("", "a + b").is_err());
        assert_eq!(parse("extend parser toy {\n production item |= name:\"b\"\n}", "a + b"), Ok(2));
        assert!(parse("extend parser toy {\n override production joiner = \"*\"\n}", "a + a").is_err());
        assert_eq!(parse("extend parser toy {\n override production joiner = \"*\"\n}", "a * a"), Ok(2));
        // `files`: the edits hold only where the globs match.
        let scoped = "extend parser toy {\n files \"*.x\"\n production item |= name:\"b\"\n override production joiner = \"*\"\n}";
        assert_eq!(parse_in(scoped, "a * b", true), Ok(2));
        assert!(parse_in(scoped, "a * b", false).is_err());
        assert_eq!(parse_in(scoped, "a + a", false), Ok(2));
        assert!(parse_in(scoped, "a + a", true).is_err());
        for wrong in ["extend parser none {\n}", "extend parser toy {\n production item = \"b\"\n}",
            "extend parser toy {\n production missing |= \"b\"\n}", "extend parser toy {\n override production missing = \"b\"\n}"] {
            assert!(!crate::config::load_text("grammar", &format!("{base}{wrong}"), &Disk).errors.is_empty(), "{wrong}");
        }
    }

    #[test]
    fn constructor_names_cannot_be_declared_as_productions() {
        let loaded = crate::config::load_text("grammar", r#"
            parser toy in js {
                token word /[a-z]+/
                line_break /[\r\n]/
                goal code { word }
                node root {}
                production emit = word
                root entry = emit(root, word end)
            }
        "#, &Disk);
        assert!(!loaded.errors.is_empty());
    }

    #[test]
    fn whitespace_does_not_turn_a_production_reference_into_a_constructor() {
        let loaded = crate::config::load_text("grammar", r#"
            parser toy in js {
                token word /[a-z]+/
                line_break /[\r\n]/
                goal code { word }
                node root {}
                root entry = emit (root, word end)
            }
        "#, &Disk);
        assert!(!loaded.errors.is_empty() ||
            CompiledParser::compile(&loaded.config.parsers[0].definition).is_err());
    }

    #[test]
    fn private_headers_can_move_through_inline_nodes_in_declared_grammars() {
        let loaded = crate::config::load_text("grammar", r#"
            parser toy in js {
                token word /[a-z]+/
                token plus /\+/
                line_break /[\r\n]/
                goal code { word plus }
                node root { value: pair }
                node pair { header?: pair, value?: text }
                root entry = emit(root, scope(saved, emit(pair, word),
                    "+" value:emit(pair, header:reuse(saved) value:word)) end)
            }
        "#, &Disk);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let parser = CompiledParser::compile(&loaded.config.parsers[0].definition).unwrap();
        let parsed = parser.parse_root("a+b", Context::default(), &Registration {
            matchers: vec![10, 11], labels: vec![vec![1], vec![2, 3]], text_matcher: crate::grammar::TEXT,
        }).unwrap();
        let root = &parsed.nodes[parsed.root as usize];
        let pair = &parsed.nodes[root.children[0].node as usize];
        let header = &parsed.nodes[pair.children[0].node as usize];
        assert_eq!((pair.start, pair.end), (0, 3));
        assert_eq!((header.start, header.end), (0, 1));
        assert_eq!(parsed.tokens.len(), 3);
    }

    #[test]
    fn declared_literals_consume_punctuation_without_publishing_field_values() {
        let loaded = crate::config::load_text("grammar", r#"
            parser toy in js {
                token word /[a-z]+/
                token plus /\+/
                trivia /\s+/
                line_break /[\r\n]/
                goal code { word plus }
                node root { value: text* }
                root entry = emit(root, value:word "+" value:word end)
            }
        "#, &Disk);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let parser = CompiledParser::compile(&loaded.config.parsers[0].definition).unwrap();
        let parsed = parser.parse_root("a + b", Context::default(), &Registration {
            matchers: vec![10], labels: vec![vec![7]], text_matcher: crate::grammar::TEXT,
        }).unwrap();
        let root = &parsed.nodes[parsed.root as usize];
        assert_eq!(root.children.len(), 2);
        assert_eq!(parsed.tokens.len(), 3);
        let values: Vec<_> = root.children.iter().map(|edge| {
            let child = &parsed.nodes[edge.node as usize];
            &"a + b"[child.start as usize..child.end as usize]
        }).collect();
        assert_eq!(values, vec!["a", "b"]);
    }

    #[test]
    fn declared_precedence_uses_the_same_structured_parser_as_programmatic_grammars() {
        let loaded = crate::config::load_text("grammar", r#"
            parser toy in js {
                token word /[a-z]+/
                token plus /\+/
                token multiply /\*/
                trivia /\s+/
                line_break /[\r\n]/
                goal code { word plus multiply }
                node root { value: expression }
                node binary { left: expression, operator: text, right: expression }
                node identifier {}
                production expression precedence emit(identifier, word) {
                    left 1 operator:"+" => binary(left, operator, right)
                    left 2 operator:"*" => binary(left, operator, right)
                }
                root entry = emit(root, value:expression end)
            }
        "#, &Disk);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let parser = CompiledParser::compile(&loaded.config.parsers[0].definition).unwrap();
        let text = "a+b*c";
        let parsed = parser.parse_root(text, Context::default(), &Registration {
            matchers: vec![10, 11, 12], labels: vec![vec![1], vec![2, 3, 4], vec![]], text_matcher: crate::grammar::TEXT,
        }).unwrap();
        let expression = &parsed.nodes[parsed.nodes[parsed.root as usize].children[0].node as usize];
        let right = &parsed.nodes[expression.children.iter().find(|edge| edge.label == 4).unwrap().node as usize];
        assert_eq!(&text[right.start as usize..right.end as usize], "b*c");
    }
}
