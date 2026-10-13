use super::*;
use crate::pattern::regex;
use crate::pattern::syntax::{ContextChange, Definition, Expr, Field, Goal, Node as NodeSchema, Operator, Precedence, Production, TokenMatch};

fn token(name: &str, pattern: &str) -> Token {
    Token { name: name.into(), pattern: regex::parse(pattern).unwrap(), visible_kind: None }
}

#[test]
fn lowered_choices_preserve_fallback_failure_offsets_labels_and_cuts() {
    let branches = vec![
        Expr::Seq(vec![lexeme("word"), lexeme("semicolon"), Expr::End]),
        Expr::Seq(vec![lexeme("word"), lexeme("plus"), lexeme("word"), Expr::End]),
        Expr::Seq(vec![lexeme("multiply"), Expr::Cut, lexeme("word"), Expr::End]),
    ];
    let direct = grammar(emit("root", Expr::Choice(branches.clone())));
    let fallback = grammar(emit("root", Expr::Choice(branches.into_iter().map(|branch| {
        Expr::Seq(vec![Expr::Look { inner: Box::new(Expr::Seq(Vec::new())), negative: false }, branch])
    }).collect())));
    let direct = CompiledParser::compile(&direct).unwrap();
    let fallback = CompiledParser::compile(&fallback).unwrap();
    for text in [";", "*", "*+", "a", "a*", "a+", "a+b*"] {
        let actual = direct.parse_root(text, Context::default(), &registration()).err().unwrap();
        let expected = fallback.parse_root(text, Context::default(), &registration()).err().unwrap();
        assert_eq!(actual, expected, "{text}");
    }
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
            NodeSchema { name: "root".into(), fields: vec![field("value")], literal_text: false },
            NodeSchema { name: "binary".into(), fields: vec![field("left"), field("operator"), field("right")], literal_text: false },
        ],
        productions: vec![Production { name: "root".into(), body }],
        root: "root".into(),
    }
}

fn registration() -> Registration {
    Registration { matchers: vec![10, 11], labels: vec![vec![1], vec![2, 3, 4]], text_matcher: super::super::TEXT }
}

fn scoped_header(body: Expr) -> Expr {
    Expr::Scope {
        capture: "header".into(), header: Box::new(emit("binary", lexeme("word"))),
        body: Box::new(body),
    }
}

fn captured_pair() -> Expr {
    bind("value", emit("binary", Expr::Seq(vec![
        bind("left", Expr::Reuse("header".into())),
        bind("right", lexeme("word")),
    ])))
}

#[test]
fn private_headers_move_into_one_child_without_replaying_tokens_or_failed_branches() {
    let pair = captured_pair();
    let continuation = Expr::Seq(vec![
        Expr::Discard(Box::new(lexeme("plus"))),
        Expr::Choice(vec![
            Expr::Try(Box::new(Expr::Seq(vec![pair.clone(), lexeme("multiply")]))),
            pair,
        ]),
    ]);
    let parser = CompiledParser::compile(&grammar(emit("root", scoped_header(continuation)))).unwrap();
    let parsed = parser.parse_root(" a + b ", Context::default(), &registration()).unwrap();
    let root = &parsed.nodes[parsed.root as usize];
    let child = &parsed.nodes[root.children[0].node as usize];
    let header = &parsed.nodes[child.children[0].node as usize];
    assert_eq!((child.start, child.end), (1, 6));
    assert_eq!((header.start, header.end), (1, 2));
    assert_eq!(child.children.iter().map(|edge| edge.label).collect::<Vec<_>>(), vec![2, 4]);
    assert_eq!(parsed.nodes.len(), 4);
    assert_eq!(parsed.tokens.len(), 3);
}

#[test]
fn reused_parsers_do_not_keep_failed_file_captures_tokens_or_context() {
    let continuation = Expr::Seq(vec![
        Expr::Discard(Box::new(Expr::Flag {
            name: "extended".into(), yes: Box::new(lexeme("multiply")), no: Box::new(lexeme("plus")),
        })),
        captured_pair(), Expr::End,
    ]);
    let mut definition = grammar(emit("root", scoped_header(continuation)));
    definition.flags.push("extended".into());
    let parser = CompiledParser::compile(&definition).unwrap();
    let signature = |parsed: &Parsed| {
        let mut result = Vec::new();
        let mut pending = vec![(parsed.root, 0, None)];
        while let Some((id, depth, field)) = pending.pop() {
            let node = &parsed.nodes[id as usize];
            result.push((depth, field, node.matcher, node.start, node.end));
            pending.extend(node.children.iter().rev().map(|edge| (edge.node, depth + 1, Some(edge.label))));
        }
        result
    };
    for (index, (text, context)) in [
        (" a + b ", Context::default()),
        (" e + ", Context::default()),
        ("\n c * d  ", Context { goal: 0, flags: 1 }),
        (" a + b ", Context::default()),
    ].into_iter().enumerate() {
        let actual = parser.parse_root(text, context, &registration());
        assert_eq!(actual.is_ok(), index != 1, "{text:?}");
        let fresh = CompiledParser::compile(&definition).unwrap();
        let expected = fresh.parse_root(text, context, &registration());
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(signature(&actual), signature(&expected));
                assert_eq!(actual.tokens.iter().map(|token| (token.token, token.start, token.end)).collect::<Vec<_>>(),
                    expected.tokens.iter().map(|token| (token.token, token.start, token.end)).collect::<Vec<_>>());
                assert_eq!(actual.trivia.iter().map(|trivia| (trivia.start, trivia.end, trivia.line_break)).collect::<Vec<_>>(),
                    expected.trivia.iter().map(|trivia| (trivia.start, trivia.end, trivia.line_break)).collect::<Vec<_>>());
            }
            (Err(actual), Err(expected)) => assert_eq!(actual, expected),
            _ => panic!("parser reuse changed whether {text:?} parses"),
        }
    }
}

#[test]
fn private_header_moves_ignore_alternatives_that_cannot_succeed() {
    let body = Expr::Seq(vec![
        Expr::Discard(Box::new(lexeme("plus"))),
        Expr::Choice(vec![Expr::Choice(Vec::new()), captured_pair()]),
    ]);
    let parser = CompiledParser::compile(&grammar(emit("root", scoped_header(body)))).unwrap();
    let parsed = parser.parse_root("a+b", Context::default(), &registration()).unwrap();
    assert_eq!(parsed.nodes.len(), 4);
    assert_eq!(parsed.tokens.len(), 3);
}

#[test]
fn repeated_scopes_move_different_headers_without_leaking_prior_captures() {
    let continuation = Expr::Seq(vec![
        Expr::Discard(Box::new(lexeme("plus"))), captured_pair(),
        Expr::Discard(Box::new(lexeme("semicolon"))),
    ]);
    let mut definition = grammar(emit("root", Expr::Repeat {
        inner: Box::new(scoped_header(continuation)), min: 0, max: None,
    }));
    definition.nodes[0].fields[0].repeated = true;
    let parser = CompiledParser::compile(&definition).unwrap();
    let text = "a+b;c+d;";
    let parsed = parser.parse_root(text, Context::default(), &registration()).unwrap();
    let root = &parsed.nodes[parsed.root as usize];
    let headers: Vec<_> = root.children.iter().map(|edge| {
        let child = &parsed.nodes[edge.node as usize];
        let header = &parsed.nodes[child.children[0].node as usize];
        &text[header.start as usize..header.end as usize]
    }).collect();
    assert_eq!(headers, vec!["a", "c"]);
    assert_eq!(parsed.nodes.len(), 7);
    assert_eq!(parsed.tokens.len(), 8);
}

#[test]
fn private_capture_moves_must_be_exactly_once_on_every_successful_path() {
    let reuse = Expr::Reuse("header".into());
    let invalid = [
        scoped_header(lexeme("plus")),
        scoped_header(Expr::Seq(vec![reuse.clone(), reuse.clone(), lexeme("plus")])),
        scoped_header(Expr::Choice(vec![
            Expr::Seq(vec![reuse.clone(), lexeme("plus")]), lexeme("multiply"),
        ])),
        scoped_header(Expr::Repeat {
            inner: Box::new(Expr::Seq(vec![reuse.clone(), lexeme("plus")])), min: 0, max: None,
        }),
        scoped_header(Expr::Seq(vec![
            Expr::Discard(Box::new(reuse.clone())), lexeme("plus"),
        ])),
        scoped_header(Expr::Seq(vec![
            Expr::Look { inner: Box::new(reuse.clone()), negative: false }, lexeme("plus"),
        ])),
        scoped_header(Expr::Pratt(Precedence {
            atom: Box::new(Expr::Seq(vec![reuse.clone(), lexeme("word")])), prefix: Vec::new(),
            rows: vec![Operator {
                operator: lexeme("plus"), binding_power: 1, right_associative: false,
                operand: true, kind: "binary".into(), left: "left".into(),
                operator_field: None, right: Some("right".into()), flatten: false,
            }],
        })),
        Expr::Seq(vec![reuse, lexeme("word")]),
    ];
    let accepted: Vec<_> = invalid.into_iter().enumerate().filter_map(|(case, body)| {
        CompiledParser::compile(&grammar(emit("root", body))).is_ok().then_some(case)
    }).collect();
    assert!(accepted.is_empty(), "invalid move cases accepted: {accepted:?}");
}

#[test]
fn private_capture_output_shapes_follow_the_moved_header_not_the_consumed_continuation() {
    let mut definition = grammar(emit("root", bind("value", Expr::Call {
        production: "moved".into(), min_binding_power: 0,
    })));
    definition.nodes[0].fields[0].kinds = vec!["moved".into()];
    definition.nodes[0].fields[0].text = false;
    definition.productions.push(Production {
        name: "moved".into(),
        body: scoped_header(Expr::Seq(vec![
            Expr::Discard(Box::new(lexeme("plus"))), Expr::Reuse("header".into()),
        ])),
    });
    let parser = CompiledParser::compile(&definition).unwrap();
    assert_eq!(parser.nodes()[0].fields[0].kinds, vec![definition.nodes[1].name.clone()]);
    assert!(!parser.nodes()[0].fields[0].text);
    let parsed = parser.parse_root("a+", Context::default(), &registration()).unwrap();
    let root = &parsed.nodes[parsed.root as usize];
    assert_eq!(parsed.nodes[root.children[0].node as usize].matcher, registration().matchers[1]);
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
    let body = emit("root", Expr::Choice(vec![
        Expr::Seq(vec![Expr::Cut, lexeme("plus")]),
        bind("value", lexeme("word")),
    ]));
    let parser = CompiledParser::compile(&grammar(body)).unwrap();
    let error = parser.parse_root("hello", Context::default(), &registration()).err().unwrap();
    assert!(error.committed);
    assert_eq!(error.offset, 0);
}

#[test]
fn precedence_and_associativity_build_operand_edges_not_flat_operator_lists() {
    let row = |operator, binding_power, right_associative| Operator {
        operator: lexeme(operator), binding_power, right_associative, operand: true,
        flatten: false,
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
fn failed_lexical_goals_rewind_token_boundaries_trivia_and_receipts() {
    let mut definition = grammar(emit("root", Expr::Choice(vec![
        Expr::Context {
            change: ContextChange { goal: Some("raw".into()), ..ContextChange::default() },
            inner: Box::new(Expr::Seq(vec![bind("value", lexeme("raw")), lexeme("multiply")])),
        },
        Expr::Seq(vec![bind("value", lexeme("word")), lexeme("plus"), lexeme("word")]),
    ])));
    definition.tokens.push(token("raw", r"[a-z]+\+"));
    definition.goals.push(Goal { name: "raw".into(), tokens: vec!["raw".into(), "multiply".into()], trivia: false });
    let parser = CompiledParser::compile(&definition).unwrap();
    for text in ["a+b", "a+ \n b", "a+b"] {
        let parsed = parser.parse_root(text, Context::default(), &registration()).unwrap();
        let root = &parsed.nodes[parsed.root as usize];
        let child = &parsed.nodes[root.children[0].node as usize];
        assert_eq!((child.start, child.end), (0, 1));
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].label, 1);
        assert_eq!(parsed.tokens.iter().map(|token| &text[token.start as usize..token.end as usize])
            .collect::<Vec<_>>(), vec!["a", "+", "b"]);
        let trivia: Vec<_> = parsed.trivia.iter().map(|item| (
            &text[item.start as usize..item.end as usize], item.line_break,
        )).collect();
        assert_eq!(trivia, if text.contains('\n') { vec![(" \n ", true)] } else { Vec::new() });
        assert_eq!(root.end as usize, text.len());
    }
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

#[test]
fn production_field_shapes_cover_every_published_child_including_recursive_operators() {
    let mut definition = grammar(emit("root", bind("value", Expr::Call { production: "expression".into(), min_binding_power: 0 })));
    for node in &mut definition.nodes {
        for field in &mut node.fields {
            if field.name != "operator" {
                field.kinds = vec!["expression".into()];
                field.text = false;
            }
        }
    }
    definition.productions.push(Production {
        name: "expression".into(),
        body: Expr::Pratt(Precedence {
            atom: Box::new(lexeme("word")), prefix: Vec::new(),
            rows: vec![Operator {
                operator: lexeme("plus"), binding_power: 1, right_associative: false, operand: true,
                flatten: false,
                kind: "binary".into(), left: "left".into(), operator_field: Some("operator".into()), right: Some("right".into()),
            }],
        }),
    });
    let parser = CompiledParser::compile(&definition).unwrap();
    let registration = registration();
    let parsed = parser.parse_root("a+b+c", Context::default(), &registration).unwrap();
    for node in &parsed.nodes {
        let Some(kind) = registration.matchers.iter().position(|matcher| *matcher == node.matcher) else { continue; };
        for edge in &node.children {
            let field = registration.labels[kind].iter().position(|label| *label == edge.label).unwrap();
            let shape = &parser.nodes()[kind].fields[field];
            let child = &parsed.nodes[edge.node as usize];
            if child.matcher == registration.text_matcher {
                assert!(shape.text);
            } else {
                let child_kind = registration.matchers.iter().position(|matcher| *matcher == child.matcher).unwrap();
                assert!(shape.kinds.contains(&parser.nodes()[child_kind].name));
            }
        }
    }
}

#[test]
fn committed_precedence_operators_keep_missing_operands_from_falling_back() {
    use crate::pattern::syntax::Prefix;
    for prefix in [false, true] {
        let committed = Expr::Seq(vec![lexeme("plus"), Expr::Cut]);
        let precedence = Precedence {
            atom: Box::new(lexeme("word")),
            prefix: if prefix { vec![Prefix {
                operator: committed.clone(), binding_power: 1, kind: "binary".into(),
                operator_field: Some("operator".into()), argument: "right".into(),
            }] } else { Vec::new() },
            rows: if prefix { Vec::new() } else { vec![Operator {
                operator: committed, binding_power: 1, right_associative: false, operand: true,
                flatten: false,
                kind: "binary".into(), left: "left".into(), operator_field: Some("operator".into()), right: Some("right".into()),
            }] },
        };
        let fallback = if prefix { lexeme("plus") } else { Expr::Seq(vec![lexeme("word"), lexeme("plus")]) };
        let parser = CompiledParser::compile(&grammar(emit("root", bind("value", Expr::Choice(vec![
            Expr::Pratt(precedence), fallback,
        ]))))).unwrap();
        let error = parser.parse_root(if prefix { "+" } else { "a+" }, Context::default(), &registration()).err().unwrap();
        assert!(error.committed);
    }
}

#[test]
fn text_comparisons_preserve_exact_mode_and_ignore_only_the_declared_pattern() {
    let nested = |name| bind(name, emit("binary", bind("left", lexeme("word"))));
    for ignore in [None, Some(regex::parse(r"[ \t]+").unwrap())] {
        let body = emit("root", bind("value", emit("binary", Expr::Seq(vec![
            nested("left"), lexeme("plus"), nested("right"),
            Expr::SameText { left: "left.left".into(), right: "right.left".into(), ignore: ignore.clone() },
        ]))));
        let mut definition = grammar(body);
        definition.tokens[0] = token("word", r"[a-z]+(?:[ \t]*\.[ \t]*[a-z]+)?");
        let parser = CompiledParser::compile(&definition).unwrap();
        assert!(parser.parse_root("a.b+a.b", Context::default(), &registration()).is_ok());
        assert_eq!(parser.parse_root("a . b+a.b", Context::default(), &registration()).is_ok(), ignore.is_some());
        assert!(parser.parse_root("a . b+a.c", Context::default(), &registration()).is_err());
    }
}

#[test]
fn nullable_text_comparison_ignore_pattern_is_rejected() {
    let mut definition = grammar(emit("root", Expr::Seq(vec![
        bind("value", lexeme("word")),
        Expr::SameText { left: "value".into(), right: "value".into(), ignore: Some(regex::parse("a*").unwrap()) },
    ])));
    assert!(CompiledParser::compile(&definition).is_err());
    definition.productions[0].body = emit("root", lexeme("word"));
    assert!(CompiledParser::compile(&definition).is_ok());
}

#[test]
fn text_captures_join_consumed_spans_without_publishing_inner_nodes_or_losing_tokens() {
    let parser = CompiledParser::compile(&grammar(emit("root", bind("value", Expr::Text(Box::new(
        Expr::Seq(vec![bind("left", emit("binary", lexeme("word"))), bind("right", lexeme("word"))]),
    )))))).unwrap();
    let parsed = parser.parse_root(" a  b ", Context::default(), &registration()).unwrap();
    let root = &parsed.nodes[parsed.root as usize];
    assert_eq!(root.children.len(), 1);
    let text = &parsed.nodes[root.children[0].node as usize];
    assert_eq!(text.matcher, registration().text_matcher);
    assert_eq!((text.start, text.end), (1, 5));
    assert!(text.children.is_empty());
    assert_eq!(parsed.nodes.len(), 2);
    assert_eq!(parsed.tokens.len(), 2);
}

#[test]
fn nullable_text_capture_is_rejected_even_inside_a_consuming_root() {
    let body = emit("root", Expr::Seq(vec![
        lexeme("word"), bind("value", Expr::Text(Box::new(Expr::Seq(Vec::new())))),
    ]));
    assert!(CompiledParser::compile(&grammar(body)).is_err());
}

#[test]
fn suffix_fields_follow_the_left_operand_in_published_child_order() {
    let parser = CompiledParser::compile(&grammar(emit("root", bind("value", Expr::Pratt(Precedence {
        atom: Box::new(lexeme("word")), prefix: Vec::new(),
        rows: vec![Operator {
            operator: Expr::Seq(vec![lexeme("plus"), bind("right", lexeme("word"))]),
            binding_power: 1, right_associative: false, operand: false,
            kind: "binary".into(), left: "left".into(), operator_field: None,
            right: None, flatten: false,
        }],
    }))))).unwrap();
    let parsed = parser.parse_root("a+b", Context::default(), &registration()).unwrap();
    let root = &parsed.nodes[parsed.root as usize];
    let expression = &parsed.nodes[root.children[0].node as usize];
    assert_eq!(expression.children.iter().map(|edge| edge.label).collect::<Vec<_>>(), vec![2, 4]);
}

#[test]
fn flattening_rows_collect_same_kind_operands_but_keep_wrappers() {
    for right_associative in [false, true] {
        let call = Expr::Call { production: "expression".into(), min_binding_power: 0 };
        let mut definition = grammar(emit("root", bind("value", call.clone())));
        let mut values = field("value");
        values.repeated = true;
        values.kinds.push("wrapped".into());
        definition.nodes[1].fields = vec![values];
        definition.nodes.push(NodeSchema {
            name: "wrapped".into(), fields: vec![field("value")], literal_text: false,
        });
        definition.tokens.extend([token("open", r"\("), token("close", r"\)")]);
        definition.goals[0].tokens.extend(["open".into(), "close".into()]);
        definition.productions.push(Production {
            name: "expression".into(),
            body: Expr::Pratt(Precedence {
                atom: Box::new(Expr::Choice(vec![
                    emit("wrapped", Expr::Seq(vec![
                        lexeme("open"), bind("value", call), lexeme("close"),
                    ])),
                    lexeme("word"),
                ])),
                prefix: Vec::new(),
                rows: vec![Operator {
                    operator: lexeme("plus"), binding_power: 1, right_associative, operand: true,
                    kind: "binary".into(), left: "value".into(), operator_field: None,
                    right: Some("value".into()), flatten: true,
                }],
            }),
        });
        let registration = Registration {
            matchers: vec![10, 11, 12], labels: vec![vec![1], vec![1], vec![1]],
            text_matcher: super::super::TEXT,
        };
        let parser = CompiledParser::compile(&definition).unwrap();
        for (text, expected) in [
            ("a+b+c", vec!["a", "b", "c"]),
            ("a+(b+c)+d", vec!["a", "(b+c)", "d"]),
        ] {
            let parsed = parser.parse_root(text, Context::default(), &registration).unwrap();
            let root = &parsed.nodes[parsed.root as usize];
            let expression = &parsed.nodes[root.children[0].node as usize];
            let values: Vec<_> = expression.children.iter().map(|edge| {
                let value = &parsed.nodes[edge.node as usize];
                &text[value.start as usize..value.end as usize]
            }).collect();
            assert_eq!(values, expected);
            assert_eq!(parsed.tokens.len(), if text.contains('(') { 9 } else { 5 });
        }
    }
}

#[test]
fn token_declaration_order_breaks_ties_independently_of_goal_membership_order() {
    let mut definition = grammar(emit("root", bind("value", lexeme("word"))));
    definition.tokens.push(token("keyword", "a"));
    definition.goals[0].tokens.insert(0, "keyword".into());
    let parser = CompiledParser::compile(&definition).unwrap();
    assert!(parser.parse_root("a", Context::default(), &registration()).is_ok());
}

#[test]
fn structural_wrappers_do_not_shrink_the_supported_production_nesting() {
    let call = Expr::Call { production: "nested".into(), min_binding_power: 0 };
    let mut definition = grammar(emit("root", bind("value", call.clone())));
    definition.productions.push(Production {
        name: "nested".into(),
        body: Expr::Choice(vec![
            emit("binary", Expr::Seq(vec![lexeme("plus"), bind("right", call)])),
            lexeme("word"),
        ]),
    });
    let parser = CompiledParser::compile(&definition).unwrap();
    let text = format!("{}a", "+".repeat(2048));
    let parsed = parser.parse_root(&text, Context::default(), &registration()).unwrap();
    assert_eq!(parsed.nodes[parsed.root as usize].end as usize, text.len());
    assert_eq!(parsed.tokens.len(), text.len());
}

#[test]
fn resource_errors_cannot_become_successful_grammar_alternatives() {
    let call = Expr::Call { production: "nested".into(), min_binding_power: 0 };
    let consume = Expr::Seq(vec![
        Expr::Repeat { inner: Box::new(lexeme("plus")), min: 0, max: None }, lexeme("word"),
    ]);
    let protected = [
        Expr::Choice(vec![Expr::Try(Box::new(call.clone())), consume.clone()]),
        Expr::Choice(vec![
            Expr::Seq(vec![Expr::Look { inner: Box::new(call.clone()), negative: true }, consume.clone()]),
            lexeme("word"),
        ]),
        Expr::Choice(vec![
            Expr::Seq(vec![
                Expr::Repeat { inner: Box::new(Expr::Try(Box::new(call.clone()))), min: 0, max: None },
                consume,
            ]),
            lexeme("word"),
        ]),
    ];
    let text = format!("{}a", "+".repeat(100_000));
    for body in protected {
        let mut definition = grammar(emit("root", Expr::Seq(vec![body, Expr::End])));
        definition.productions.push(Production {
            name: "nested".into(),
            body: Expr::Choice(vec![Expr::Seq(vec![lexeme("plus"), call.clone()]), lexeme("word")]),
        });
        let parser = CompiledParser::compile(&definition).unwrap();
        let error = parser.parse_root(&text, Context::default(), &registration()).err()
            .expect("resource failure must not select a successful fallback");
        assert!(error.committed);
        let parsed = parser.parse_root("a", Context::default(), &registration()).unwrap();
        assert_eq!(parsed.nodes[parsed.root as usize].end, 1);
        assert_eq!(parsed.tokens.len(), 1);
    }
}

#[test]
fn token_patterns_require_the_whole_goal_selected_token_not_a_prefix() {
    let mut definition = grammar(emit("root", bind("value", Expr::TokenPattern {
        pattern: regex::parse("[+-]").unwrap(), emit: None,
    })));
    definition.tokens.extend([token("increment", r"\+\+"), token("assign", r"\+=")]);
    definition.goals[0].tokens.extend(["increment".into(), "assign".into()]);
    let parser = CompiledParser::compile(&definition).unwrap();
    assert!(parser.parse_root("+", Context::default(), &registration()).is_ok());
    for text in ["++", "+="] {
        assert!(parser.parse_root(text, Context::default(), &registration()).is_err());
    }
}

#[test]
fn empty_inline_nodes_anchor_after_trivia_without_consuming_the_next_token() {
    let point = emit("binary", Expr::Look { inner: Box::new(lexeme("plus")), negative: false });
    let parser = CompiledParser::compile(&grammar(emit("root", Expr::Seq(vec![
        lexeme("word"), bind("value", point), lexeme("plus"),
    ])))).unwrap();
    let parsed = parser.parse_root("a  +", Context::default(), &registration()).unwrap();
    let root = &parsed.nodes[parsed.root as usize];
    let point = &parsed.nodes[root.children[0].node as usize];
    assert_eq!((point.start, point.end), (3, 3));
    assert_eq!(parsed.tokens.len(), 2);
}
