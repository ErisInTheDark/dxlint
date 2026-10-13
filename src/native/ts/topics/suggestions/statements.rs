//! Statement structure preferences.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::lexer::Tok;
use super::super::super::tokens::lex;
use crate::expr::Value;

fn braces_necessary(file: &NativeFile, block: u32) -> bool {
    let s = &file.syntax;
    let Some(mut tail) = s.all(block, F::Body).next() else { return false };
    if matches!(s.kind(tail), Some(K::Function | K::Class)) || s.is(tail, K::Var) && s.word(tail, F::Kind) != Some("var") { return true }
    let unsafe_if = loop {
        match s.kind(tail) {
            Some(K::If) => match s.get(tail, F::Alternate) {
                Some(next) => tail = next,
                None => break true,
            },
            Some(K::For | K::ForIn | K::ForOf | K::While | K::With | K::Labeled) => match s.get(tail, F::Body) {
                Some(next) => tail = next,
                None => break false,
            },
            _ => break false,
        }
    };
    if !unsafe_if { return false }
    let mut current = block;
    while let Some(parent) = s.parent(current) {
        if s.is(parent, K::If) && s.field(current) == Some(F::Consequent) && s.get(parent, F::Alternate).is_some() {
            return true
        }
        if !matches!(s.kind(parent), Some(K::If | K::For | K::ForIn | K::ForOf | K::While | K::With | K::Labeled)) { break }
        current = parent;
    }
    false
}

fn brace_expectations(file: &NativeFile, owner: u32, body: u32) -> [i32; 3] {
    let s = &file.syntax;
    let block = s.is(body, K::Block);
    let statements: Vec<_> = s.all(body, F::Body).collect();
    if block && (statements.len() != 1 || braces_necessary(file, body)) { return [1; 3] }
    let span = s.tree.node(body);
    // The file's token stream, not a re-lex: `else` chains would rescan
    // every earlier branch, and a big body all of its text.
    let lexed = file.cached(lex);
    let tokens = &lexed.0;
    let owner_start = s.tree.node(owner).start;
    let before = tokens.partition_point(|t| t.end <= span.start).checked_sub(1)
        .map(|i| tokens[i].start).filter(|&start| start >= owner_start).unwrap_or(owner_start);
    let one_line = |node| {
        if s.is(node, K::Empty) { return true }
        let span = s.tree.node(node);
        let inside = &tokens[tokens.partition_point(|t| t.start < span.start)..tokens.partition_point(|t| t.end <= span.end)];
        let end = inside.iter().rev().find(|t| t.tok != Tok::P(";")).map_or(span.start, |t| t.end);
        !s.src[span.start as usize..end as usize].contains(['\r', '\n', '\u{2028}', '\u{2029}'])
    };
    let collapsed = if block {
        !s.src[before as usize..span.end as usize].contains(['\r', '\n', '\u{2028}', '\u{2029}'])
    } else {
        let text = s.text(body).trim_end();
        let text = text.strip_suffix(';').unwrap_or(text).trim_end();
        let end = span.start as usize + text.len();
        !s.src[before as usize..end].contains(['\r', '\n', '\u{2028}', '\u{2029}'])
    };
    let nested = if block {
        let statement = statements[0];
        // Comments are sorted and disjoint: only the first after the brace
        // can end before the statement starts.
        let comments = file.comments();
        let first = comments.get(comments.partition_point(|c| c.start <= span.start));
        !one_line(statement) || first.is_some_and(|c| c.end <= s.tree.node(statement).start)
    } else { !one_line(body) };
    [0, if collapsed { -1 } else { 1 }, i32::from(nested)]
}

pub fn curly_body(file: &NativeFile, body: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(owner) = s.parent(body) else { return Value::Null };
    let alternate = s.is(owner, K::If) && s.field(body) == Some(F::Alternate);
    if alternate && s.is(body, K::If) { return Value::Null }
    let (name, condition) = match s.kind(owner) {
        Some(K::If) => (if alternate { "else" } else { "if" }, !alternate),
        Some(K::While) => ("while", true),
        Some(K::For) => ("for", true),
        Some(K::ForIn) => ("for-in", false),
        Some(K::ForOf) => ("for-of", false),
        Some(K::DoWhile) => ("do", false),
        _ => return Value::Null,
    };
    let block = s.is(body, K::Block);
    let expected = brace_expectations(file, owner, body);
    let mut consistent = [false; 3];
    if s.is(owner, K::If) {
        let mut top = owner;
        while let Some(parent) = s.parent(top).filter(|&p| s.is(p, K::If) && s.field(top) == Some(F::Alternate)) { top = parent; }
        let mut current = top;
        loop {
            let mut bodies: Vec<_> = s.get(current, F::Consequent).into_iter().collect();
            let alternate = s.get(current, F::Alternate);
            if let Some(branch) = alternate.filter(|&n| !s.is(n, K::If)) { bodies.push(branch); }
            for branch in bodies {
                let values = brace_expectations(file, current, branch);
                for (i, value) in values.into_iter().enumerate() {
                    consistent[i] |= value == 1 || value == -1 && s.is(branch, K::Block);
                }
            }
            let Some(next) = alternate.filter(|&n| s.is(n, K::If)) else { break };
            current = next;
        }
    } else {
        for (i, value) in expected.into_iter().enumerate() { consistent[i] = value == 1 || value == -1 && block; }
    }
    Value::record(vec![
        ("block".into(), Value::Bool(block)), ("name".into(), Value::from(name)), ("condition".into(), Value::Bool(condition)),
        ("multi".into(), Value::from(f64::from(expected[0]))), ("multiline".into(), Value::from(f64::from(expected[1]))), ("nested".into(), Value::from(f64::from(expected[2]))),
        ("multi_consistent".into(), Value::Bool(consistent[0])), ("multiline_consistent".into(), Value::Bool(consistent[1])), ("nested_consistent".into(), Value::Bool(consistent[2])),
    ])
}
