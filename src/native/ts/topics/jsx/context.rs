//! Transparent parentheses preserve JSX's enclosing expression owner.

use super::metadata::{line, line_starts};
use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::lexer::{Lexer, Tok, Token};
use crate::expr::Value;

fn outside_parens(file: &NativeFile, mut node: u32) -> u32 {
    let s = &file.syntax;
    while let Some(parent) = s.parent(node).filter(|&p| s.is(p, K::Paren)) { node = parent; }
    node
}

fn last_token(src: &str, start: usize, end: usize) -> Option<Token> {
    let mut lexer = Lexer::new(&src[start..end]);
    let mut last = None;
    loop {
        let mut token = lexer.next_token();
        if token.tok == Tok::Eof { break; }
        token.start += start as u32;
        token.end += start as u32;
        last = Some(token);
    }
    last
}

pub fn owner(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let n = s.tree.node(node);
    let lines = file.cached(line_starts);
    let top = outside_parens(file, node);
    let parent = s.parent(top);
    let field = s.field(top);
    let paren = s.parent(node).filter(|&p| s.is(p, K::Paren)).map(|p| s.tree.node(p));
    let mut contexts = Vec::new();
    let mut add = |kind: &'static str, fallback: bool| contexts.push(Value::Record(vec![
        ("kind".into(), Value::from(kind)),
        ("fallback".into(), Value::Bool(fallback)),
    ]));
    if let Some(parent) = parent {
        match (s.kind(parent), field) {
            (Some(K::Declarator), Some(F::Init)) => add("declaration", false),
            (Some(K::Assign), Some(F::Right)) => add("assignment", false),
            (Some(K::Return), Some(F::Arg)) => add("return", false),
            (Some(K::Arrow), Some(F::Body)) => add("arrow", false),
            (Some(K::Logical), Some(F::Right)) => add("logical", false),
            (Some(K::Conditional), Some(F::Consequent | F::Alternate)) => {
                add("condition", false);
                let conditional = outside_parens(file, parent);
                if let Some(p) = s.parent(conditional) {
                    match (s.kind(p), s.field(conditional)) {
                        (Some(K::Declarator), Some(F::Init)) => add("declaration", true),
                        (Some(K::Assign), Some(F::Right)) => add("assignment", true),
                        _ => {}
                    }
                }
            }
            (Some(K::JsxExpression), Some(F::Expr)) if s.parent(parent).is_some_and(|p| s.is(p, K::JsxAttribute)) => add("prop", false),
            (Some(K::Prop), Some(F::Value)) if s.is(node, K::JsxElement) && s.parent(parent).is_some_and(|p| s.is(p, K::Object)) => add("propertyValue", false),
            _ => {}
        }
    }
    let call = parent.filter(|&p| matches!(s.kind(p), Some(K::Call | K::New)) && field == Some(F::Arg));
    let call_break = call.is_some_and(|p| {
        let start = n.start as usize;
        let end = n.end as usize;
        let before = if let Some(paren) = paren {
            paren.start as usize
        } else {
            let args: Vec<_> = s.all(p, F::Arg).collect();
            let index = args.iter().position(|&arg| arg == top).unwrap_or(0);
            index.checked_sub(1).map(|i| s.tree.node(args[i]).end as usize)
                .or_else(|| s.get(p, F::Callee).map(|callee| s.tree.node(callee).end as usize))
                .unwrap_or(s.tree.node(p).start as usize)
        };
        let previous = last_token(s.src, before, start);
        let after = paren.map_or(s.tree.node(p).end as usize, |paren| paren.end as usize);
        let mut next = Lexer::new(&s.src[end..after]).next_token();
        next.start += end as u32;
        next.end += end as u32;
        previous.is_some_and(|token| line(&lines, token.end as usize) == line(&lines, start))
            || next.tok != Tok::Eof && next.tok != Tok::P(",") && line(&lines, next.end as usize) == line(&lines, end)
    });
    Value::Record(vec![
        ("single".into(), Value::Bool(line(&lines, n.start as usize) == line(&lines, n.end as usize))),
        ("parenthesized".into(), Value::Bool(paren.is_some())),
        ("parens_same_line".into(), Value::Bool(paren.is_some_and(|p| {
            line(&lines, p.start as usize) == line(&lines, n.start as usize)
                || line(&lines, p.end as usize - 1) == line(&lines, n.end as usize)
        }))),
        ("contexts".into(), Value::List(contexts)),
        ("call_break".into(), Value::Bool(call_break)),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn conditional_policy_takes_ownership_from_the_declaration_fallback() {
        let text = "const x = flag ? (<Box>\ntext\n</Box>) : null;";
        let declaration = native_linter("rule jsx-wrap-multilines { set declaration 'parens-new-line' }");
        assert_eq!(declaration.lint(Path::new("/root/file.tsx"), text).len(), 1);
        let condition = native_linter("rule jsx-wrap-multilines {\nset declaration 'parens-new-line'\nset condition 'parens'\n}");
        assert!(condition.lint(Path::new("/root/file.tsx"), text).is_empty());
    }
}
