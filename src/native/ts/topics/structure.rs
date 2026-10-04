//! @stylistic code shape across lines (`std/ts/structure`): indentation,
//! parentheses, wrapping.

use crate::expr::{facts::Fact, Value};
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field, Kind};

mod parens;
mod comments;
mod lines;
mod declarations;
mod operators;
mod calls;
mod statements;
mod lists;
mod indentation;
mod padding;

pub const FACTS: &[Fact] = &[
    Fact { name: "line_layout_findings", doc: "native ts_program: tab runs, mixed indentation and line endings", get: |f, n| f.with_file(line_layout_findings, n) },
    Fact { name: "bom", doc: "native ts_program: source starts with a Unicode BOM", get: |f, n| f.with_file(bom, n) },
    Fact { name: "operator_offset", doc: "native ts_binary: operator position", get: |f, n| f.with_file(operator_offset, n) },
    Fact { name: "for_declaration", doc: "native ts_variable_declaration: declaration is a loop header", get: |f, n| f.with_file(for_declaration, n) },
    Fact { name: "conditional_arrow_body", doc: "native ts_arrow: conditional body, direct or parenthesized", get: |f, n| f.with_file(conditional_arrow_body, n) },
    Fact { name: "equality_typeof", doc: "native ts_binary: an operand is a typeof expression", get: |f, n| f.with_file(equality_typeof, n) },
    Fact { name: "ternary_linebreaks", doc: "native ts_conditional: operand linebreaks and report positions", get: |f, n| f.with_file(ternary_linebreaks, n) },
    Fact { name: "excess_parens", doc: "native parenthesized expressions and types: redundant parentheses with option contexts", get: |f, n| f.with_file(parens::excess, n) },
    Fact { name: "comment_layout", doc: "native program: line comment bodies, positions and directive status", get: |f, n| f.with_file(comments::layout, n) },
    Fact { name: "comment_style_findings", doc: "native program: multiline comment style findings", get: |f, n| f.with_file(comments::styles, n) },
    Fact { name: "comment_gaps", doc: "native program: missing blank lines and immediate container boundaries around comments", get: |f, n| f.with_file(comments::gaps, n) },
    Fact { name: "line_lengths", doc: "native program: source lines, tab segments, literal and comment status", get: |f, n| f.with_file(lines::lengths, n) },
    Fact { name: "declaration_groups", doc: "native variable declarations: counts, previous statement and preceding groups in the owning scope", get: |f, n| f.with_file(declarations::groups, n) },
    Fact { name: "mixed_operators", doc: "native binary and logical expressions: adjacent unparenthesized operator pair and positions", get: |f, n| f.with_file(operators::mixed, n) },
    Fact { name: "operator_linebreaks", doc: "native infix and initializer owners: operator positions and neighbouring token linebreaks", get: |f, n| f.with_file(operators::linebreaks, n) },
    Fact { name: "iife_layout", doc: "native calls: immediate function and prototype invocations with wrapping status", get: |f, n| f.with_file(calls::layout, n) },
    Fact { name: "call_chain_layout", doc: "native calls: chain depth, member prefix and line layout", get: |f, n| f.with_file(calls::chain, n) },
    Fact { name: "statement_line_events", doc: "native program: counted statement enter and exit lines in traversal order", get: |f, n| f.with_file(statements::events, n) },
    Fact { name: "list_layout", doc: "native list owners: boundaries, item counts and spacing or wrapping pairs", get: |f, n| f.with_file(lists::sites, n) },
    Fact { name: "binary_operand_indents", doc: "native program: operand-line indentation constraints in traversal order", get: |f, n| f.with_file(indentation::operands, n) },
    Fact { name: "statement_padding", doc: "native program: adjacent body statements, matcher categories and intervening blank lines", get: |f, n| f.with_file(padding::sites, n) },
];

fn ternary_linebreaks(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let (Some(test), Some(consequent), Some(alternate)) = (
        s.get(node, Field::Test), s.get(node, Field::Consequent), s.get(node, Field::Alternate),
    ) else { return Value::Null };
    let test = s.tree.node(test);
    let consequent = s.tree.node(consequent);
    let alternate = s.tree.node(alternate);
    let broken = |start: u32, end: u32| s.src[start as usize..end as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']);
    let multiline = broken(test.start, alternate.end);
    let mut outer = node;
    while let Some(parent) = s.parent(outer).filter(|&parent| s.is(parent, Kind::Conditional)) {
        outer = parent;
    }
    let jsx = s.parent(outer).is_some_and(|parent| s.is(parent, Kind::JsxExpression));
    Value::List(vec![
        Value::Record(vec![
            ("offset".into(), Value::from(test.start as usize)),
            ("broken".into(), Value::Bool(broken(test.end, consequent.start))),
            ("multiline".into(), Value::Bool(multiline)),
            ("jsx".into(), Value::Bool(jsx)),
            ("operands".into(), Value::from("test and consequent")),
        ]),
        Value::Record(vec![
            ("offset".into(), Value::from(consequent.start as usize)),
            ("broken".into(), Value::Bool(broken(consequent.end, alternate.start))),
            ("multiline".into(), Value::Bool(multiline)),
            ("jsx".into(), Value::Bool(jsx)),
            ("operands".into(), Value::from("consequent and alternate")),
        ]),
    ])
}

fn equality_typeof(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    Value::Bool([Field::Left, Field::Right].into_iter().filter_map(|f| s.get(node, f)).any(|operand| {
        let operand = s.unparen(operand);
        s.is(operand, Kind::Unary) && s.word(operand, Field::Op) == Some("typeof")
    }))
}

fn conditional_arrow_body(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(body) = s.get(node, Field::Body) else { return Value::Null };
    match s.kind(body) {
        Some(Kind::Conditional) => Value::from("bare"),
        Some(Kind::Paren) if s.get(body, Field::Expr).is_some_and(|inner| s.is(inner, Kind::Conditional)) => Value::from("parenthesized"),
        _ => Value::Null,
    }
}

fn for_declaration(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    Value::Bool(s.parent(node).and_then(|p| s.kind(p)).is_some_and(|k| matches!(k, Kind::For | Kind::ForIn | Kind::ForOf)))
}

fn operator_offset(file: &NativeFile, node: u32) -> Value<'static> {
    file.syntax.get(node, Field::Op)
        .map(|op| Value::from(file.syntax.tree.node(op).start as usize))
        .unwrap_or(Value::Null)
}

fn bom(file: &NativeFile, node: u32) -> Value<'static> {
    if file.syntax.is(node, Kind::Program) {
        Value::Bool(file.syntax.src.starts_with('\u{feff}'))
    } else {
        Value::Null
    }
}

fn entry(kind: &'static str, offset: usize, indentation: bool) -> Value<'static> {
    Value::Record(vec![
        ("kind".into(), Value::from(kind)),
        ("offset".into(), Value::from(offset)),
        ("indentation".into(), Value::Bool(indentation)),
    ])
}

fn line_layout_findings(file: &NativeFile, node: u32) -> Value<'static> {
    if !file.syntax.is(node, Kind::Program) {
        return Value::Null;
    }
    let findings = file.cached(scan_lines);
    Value::List(findings.as_ref().clone())
}

fn scan_lines(file: &NativeFile) -> Vec<Value<'static>> {
    let src = file.syntax.src;
    let mut excluded = Vec::new();
    for id in 0..file.syntax.tree.nodes.len() as u32 {
        match file.syntax.kind(id) {
            Some(Kind::String | Kind::Regex) => {
                let n = file.syntax.tree.node(id);
                excluded.push((n.start as usize, n.end as usize));
            }
            Some(Kind::Template | Kind::TemplateType) => {
                for quasi in file.syntax.all(id, Field::Quasi) {
                    let n = file.syntax.tree.node(quasi);
                    excluded.push((n.start as usize, n.end as usize));
                }
            }
            _ => {}
        }
    }
    let comments = file.comments();
    let mut out = Vec::new();
    let mut line_start = 0;
    for line in src.split_inclusive('\n') {
        let bytes = line.as_bytes();
        let indent_end = bytes.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'\t' {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && bytes[i] == b'\t' {
                i += 1;
            }
            out.push(entry("tab", line_start + start, start < indent_end));
        }
        if indent_end > 1 {
            let indent = &bytes[..indent_end];
            if let Some(boundary) = indent.windows(2).position(|w| w[0] != w[1]) {
                let offset = line_start + boundary;
                let in_multiline_comment = comments.iter().any(|c| (c.start as usize) < line_start && line_start < c.end as usize);
                let in_literal = excluded.iter().any(|&(start, end)| start < line_start && offset < end);
                if !in_multiline_comment && !in_literal {
                    let smart_boundary = indent.windows(2).position(|w| w == b" \t");
                    out.push(entry("mixed", offset, smart_boundary.is_some()));
                    if let Some(boundary) = smart_boundary {
                        out.push(entry("mixed-smart", line_start + boundary, false));
                    }
                }
            }
        }
        if line.ends_with("\r\n") {
            out.push(entry("crlf", line_start + line.len() - 2, false));
        } else if line.ends_with('\n') {
            out.push(entry("lf", line_start + line.len() - 1, false));
        }
        line_start += line.len();
    }
    for (at, ch) in src.char_indices() {
        if ch == '\u{2028}' || ch == '\u{2029}' {
            out.push(entry("other-linebreak", at, false));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;

    #[test]
    fn equality_modes_report_only_the_operators_they_forbid() {
        let cases = [
            ("rule eqeqeq", "a == b;", true),
            ("rule eqeqeq", "a === b;", false),
            ("rule eqeqeq", "a == null;", true),
            ("rule eqeqeq { set options [\"smart\"] }", "a == null;", false),
            ("rule eqeqeq { set options [\"always\", { null: \"never\" }] }", "a === null;", true),
        ];
        for (rules, source, expected) in cases {
            let lint = native_linter(rules);
            let reports = lint.lint(Path::new("/root/file.ts"), source);
            assert_eq!(!reports.is_empty(), expected, "{rules}: {source}");
            if expected {
                assert_eq!(reports.len(), 1, "{rules}: {source}");
                assert_eq!(reports[0].start, source.find("==").unwrap(), "{rules}: {source}");
            }
        }
    }
}
