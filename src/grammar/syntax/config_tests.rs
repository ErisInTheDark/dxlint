//! Declared parsers (`parser ... in ...`, `extend parser`): their config
//! declarations reach the compiled parser. Moved out of `dx-config` because
//! compiling them needs the grammar crate.

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
