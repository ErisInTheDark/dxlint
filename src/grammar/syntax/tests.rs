use super::*;
use crate::pattern::regex;
use crate::pattern::syntax::{ContextChange, Definition, Expr, Field, Goal, Node as NodeSchema, Operator, Precedence, Production, TokenMatch};

fn token(name: &str, pattern: &str) -> Token {
    Token { name: name.into(), pattern: regex::parse(pattern).unwrap(), visible_kind: None }
}

fn lexeme(name: &str) -> Expr {
    Expr::Token(TokenMatch { token: name.into(), text: None, emit: None })
}

fn bind(name: &str, inner: Expr) -> Expr {
    Expr::Bind { field: name.into(), inner: Box::new(inner) }
}

fn emit(kind: &str, inner: Expr) -> Expr {
    Expr::Emit { kind: kind.into(), inner: Box::new(inner) }
}

fn field(name: &str) -> Field {
    Field { name: name.into(), kinds: if name == "operator" { Vec::new() } else { vec!["binary".into()] }, text: true, optional: false, repeated: false }
}

fn grammar(body: Expr) -> Definition {
    Definition {
        name: "toy".into(),
        trivia: vec![regex::parse(r"\s+").unwrap()],
        line_break: regex::parse(r"[\r\n]").unwrap(),
        tokens: vec![token("word", "[a-z]+"), token("plus", r"\+"), token("multiply", r"\*"), token("power", r"\^"), token("semicolon", ";")],
        goals: vec![Goal { name: "code".into(), tokens: vec!["word".into(), "plus".into(), "multiply".into(), "power".into(), "semicolon".into()], trivia: true }],
        flags: Vec::new(),
        nodes: vec![
            NodeSchema { name: "root".into(), fields: vec![field("value")] },
            NodeSchema { name: "binary".into(), fields: vec![field("left"), field("operator"), field("right")] },
        ],
        productions: vec![Production { name: "root".into(), body }],
        root: "root".into(),
    }
}

fn registration() -> Registration {
    Registration { matchers: vec![10, 11], labels: vec![vec![1], vec![2, 3, 4]], text_matcher: super::super::TEXT }
}

#[test]
fn full_root_consumption_preserves_labelled_bytes_and_rejects_trailing_input() {
    let parser = CompiledParser::compile(&grammar(emit("root", bind("value", lexeme("word"))))).unwrap();
    let parsed = parser.parse_root(" hello ", Context::default(), &registration()).unwrap();
    let root = &parsed.nodes[parsed.root as usize];
    assert_eq!((root.start, root.end), (0, 7));
    let child = &parsed.nodes[root.children[0].node as usize];
    assert_eq!((child.start, child.end), (1, 6));
    assert_eq!(root.children[0].label, 1);
    assert!(parser.parse_root("hello hello", Context::default(), &registration()).is_err());
}

#[test]
fn failed_choices_publish_only_the_successful_branch_and_its_tokens() {
    let body = emit("root", Expr::Choice(vec![
        Expr::Try(Box::new(Expr::Seq(vec![bind("value", lexeme("word")), lexeme("plus")]))),
        bind("value", lexeme("word")),
    ]));
    let parser = CompiledParser::compile(&grammar(body)).unwrap();
    let parsed = parser.parse_root("hello", Context::default(), &registration()).unwrap();
    assert_eq!(parsed.nodes.len(), 2);
    assert_eq!(parsed.tokens.len(), 1);
    assert_eq!(parsed.nodes[parsed.root as usize].children.len(), 1);
}

#[test]
fn cuts_keep_a_failed_committed_branch_from_becoming_a_successful_fallback() {
    let body = emit("root", Expr::Choice(vec![
        Expr::Seq(vec![bind("value", lexeme("word")), Expr::Cut, lexeme("plus")]),
        bind("value", lexeme("word")),
    ]));
    let parser = CompiledParser::compile(&grammar(body)).unwrap();
    let error = parser.parse_root("hello", Context::default(), &registration()).err().unwrap();
    assert!(error.committed);
    assert_eq!(error.offset, 5);
}

#[test]
fn precedence_and_associativity_build_operand_edges_not_flat_operator_lists() {
    let row = |operator, binding_power, right_associative| Operator {
        operator: lexeme(operator), binding_power, right_associative, operand: true,
        kind: "binary".into(), left: "left".into(), operator_field: Some("operator".into()), right: Some("right".into()),
    };
    let body = emit("root", bind("value", Expr::Pratt(Precedence {
        atom: Box::new(lexeme("word")), prefix: Vec::new(),
        rows: vec![row("plus", 1, false), row("multiply", 2, false), row("power", 3, true)],
    })));
    let parser = CompiledParser::compile(&grammar(body)).unwrap();
    for (text, expected_right) in [("a+b*c", "b*c"), ("a^b^c", "b^c")] {
        let parsed = parser.parse_root(text, Context::default(), &registration()).unwrap();
        let root = &parsed.nodes[parsed.root as usize];
        let expression = &parsed.nodes[root.children[0].node as usize];
        let right = &parsed.nodes[expression.children.iter().find(|edge| edge.label == 4).unwrap().node as usize];
        assert_eq!(&text[right.start as usize..right.end as usize], expected_right);
    }
}

#[test]
fn line_restrictions_test_actual_trivia_without_consuming_an_operand() {
    let body = emit("root", Expr::Seq(vec![lexeme("word"), Expr::SameLine, bind("value", lexeme("word"))]));
    let parser = CompiledParser::compile(&grammar(body)).unwrap();
    assert!(parser.parse_root("a b", Context::default(), &registration()).is_ok());
    assert!(parser.parse_root("a\nb", Context::default(), &registration()).is_err());
}

#[test]
fn lookahead_context_does_not_poison_a_production_at_the_same_byte() {
    let call = Expr::Call { production: "atom".into(), min_binding_power: 0 };
    let look = Expr::Look {
        inner: Box::new(Expr::Context {
            change: ContextChange { set: vec!["extended".into()], ..ContextChange::default() },
            inner: Box::new(call.clone()),
        }),
        negative: false,
    };
    let mut definition = grammar(emit("root", Expr::Seq(vec![look, bind("value", call), lexeme("plus")])));
    definition.flags.push("extended".into());
    definition.productions.push(Production {
        name: "atom".into(),
        body: Expr::Flag {
            name: "extended".into(),
            yes: Box::new(Expr::Seq(vec![lexeme("word"), lexeme("plus")])),
            no: Box::new(lexeme("word")),
        },
    });
    let parser = CompiledParser::compile(&definition).unwrap();
    let parsed = parser.parse_root("a+", Context::default(), &registration()).unwrap();
    let root = &parsed.nodes[parsed.root as usize];
    let value = &parsed.nodes[root.children[0].node as usize];
    assert_eq!((value.start, value.end), (0, 1));
    assert_eq!(parsed.tokens.len(), 2);
}

#[test]
fn empty_files_have_a_root_but_nullable_called_productions_are_rejected() {
    let parser = CompiledParser::compile(&grammar(emit("root", Expr::End))).unwrap();
    let parsed = parser.parse_root("", Context::default(), &registration()).unwrap();
    assert_eq!(parsed.nodes.len(), 1);
    let mut definition = grammar(emit("root", Expr::Call { production: "empty".into(), min_binding_power: 0 }));
    definition.productions.push(Production { name: "empty".into(), body: Expr::End });
    assert!(CompiledParser::compile(&definition).is_err());
}

#[test]
fn lexical_goals_choose_tokens_locally_and_restore_the_callers_goal() {
    let mut definition = grammar(emit("root", Expr::Seq(vec![
        bind("value", Expr::Context {
            change: ContextChange { goal: Some("raw".into()), ..ContextChange::default() },
            inner: Box::new(lexeme("raw")),
        }),
        lexeme("plus"),
        lexeme("word"),
    ])));
    definition.tokens.push(token("raw", r"[a-z]+\+"));
    definition.goals.push(Goal { name: "raw".into(), tokens: vec!["raw".into()], trivia: false });
    let parser = CompiledParser::compile(&definition).unwrap();
    let parsed = parser.parse_root("a++b", Context::default(), &registration()).unwrap();
    assert_eq!(parsed.tokens.iter().map(|token| (token.start, token.end)).collect::<Vec<_>>(), vec![(0, 2), (2, 3), (3, 4)]);
}

#[test]
fn implicit_terminators_publish_no_fake_token_and_do_not_steal_the_next_statement() {
    let end = Expr::Terminator {
        explicit: TokenMatch { token: "semicolon".into(), text: None, emit: None },
        before: Vec::new(), newline: true, eof: true,
    };
    let parser = CompiledParser::compile(&grammar(emit("root", Expr::Seq(vec![
        bind("value", lexeme("word")), end.clone(), lexeme("word"), end,
    ])))).unwrap();
    let parsed = parser.parse_root("a\nb", Context::default(), &registration()).unwrap();
    assert_eq!(parsed.tokens.len(), 2);
    assert!(parser.parse_root("a b", Context::default(), &registration()).is_err());
    assert!(parser.parse_root("a;b;", Context::default(), &registration()).is_ok());
}
