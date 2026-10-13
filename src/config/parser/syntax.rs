//! Structured grammar declarations beside byte-pattern declarations.

mod expressions;
mod precedence;
mod resolve;

use super::Parser;
use crate::config::ast::ParserDecl;
use crate::config::error::ConfigError;
use crate::config::lexer::{decode_escapes, Tk, Token};
use crate::pattern::syntax::{Definition, Field, Goal, Node, Production};
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
        let mut definition = Definition {
            name: name.text.clone(), trivia: Vec::new(), line_break: Pat::Empty,
            tokens: Vec::new(), goals: Vec::new(), flags: Vec::new(), nodes: Vec::new(),
            productions: Vec::new(), root: String::new(),
        };
        let mut option_flags = Vec::new();
        let before = self.errors.len();
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
                (Tk::Ident, "token") => {
                    self.bump();
                    if let Some(token_name) = self.ident("a token name") {
                        if let Some(pattern) = self.syntax_pattern() {
                            let visible_kind = if self.syntax_peek(0).is_some_and(|token| token.is_ident("as")) {
                                self.ts.next_raw();
                                self.ident("a public token kind").map(|name| name.text)
                            } else { None };
                            definition.tokens.push(crate::pattern::syntax::Token { name: token_name.text, pattern, visible_kind });
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
                        let body = if self.word("precedence") {
                            self.syntax_precedence()
                        } else {
                            self.expect(Tk::Eq, "`=` before a production body")
                                .and_then(|_| self.syntax_expression(0))
                        };
                        if let Some(body) = body {
                            if token.is_ident("root") {
                                if !definition.root.is_empty() {
                                    self.errors.push(ConfigError::new(production.span, "a parser has one root"));
                                }
                                definition.root = production.text.clone();
                            }
                            definition.productions.push(Production { name: production.text, body });
                        }
                    }
                }
                (Tk::Ident, _) if self.ts.expand(&mut self.errors) => continue,
                _ => {
                    self.errors.push(ConfigError::new(token.span, "parser items are token, trivia, line_break, flag, goal, node, production and root"));
                    self.bump();
                }
            }
            if self.errors.len() > item_errors { self.recover(); }
            else { self.end_of_item(); }
        }
        if definition.root.is_empty() {
            self.errors.push(ConfigError::new(name.span, "a parser needs a root production"));
        }
        if matches!(definition.line_break, Pat::Empty) {
            self.errors.push(ConfigError::new(name.span, "a parser needs a line_break pattern"));
        }
        if self.errors.len() == before {
            if let Err(message) = resolve::resolve(&mut definition) {
                self.errors.push(ConfigError::new(name.span, message));
            } else {
                self.config.parsers.push(ParserDecl { name, language, definition, option_flags });
            }
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
