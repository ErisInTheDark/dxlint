//! Statement structure preferences.

use std::collections::HashSet;

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::lexer::{Lexer, Tok};
use super::super::super::paths::{self, Domain};
use super::super::super::syntax::Syntax;
use crate::expr::Value;

struct SingleIteration(HashSet<u32>);

#[derive(Default)]
struct LoopPaths {
    entered: HashSet<u32>,
    repeated: HashSet<u32>,
}

impl Domain for LoopPaths {
    type S = ();
    fn join(&self, _: &(), _: &()) {}
    fn loop_back(&mut self, _: &Syntax, node: u32, entered: bool, state: Option<&()>) {
        if entered { self.entered.insert(node); }
        if state.is_some() { self.repeated.insert(node); }
    }
}

fn single_iterations(file: &NativeFile) -> SingleIteration {
    let s = &file.syntax;
    let mut domain = LoopPaths::default();
    for (_, body) in paths::bodies(s) { paths::walk(s, &mut domain, body, ()); }
    SingleIteration(domain.entered.difference(&domain.repeated).copied().collect())
}

pub fn single_iteration(file: &NativeFile, node: u32) -> Value<'static> {
    Value::Bool(file.cached(single_iterations).0.contains(&node))
}

pub fn label_binding(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let collision = s.get(node, F::Label).is_some_and(|label| {
        let name = s.text(label);
        file.bindings().visible(s, node, name, true).is_some()
            || matches!(file.globals().setting(name), Some("readonly" | "writable"))
    });
    Value::Bool(collision)
}

pub fn rethrow(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let pair = s.get(node, F::Param).zip(s.get(node, F::Handler)
        .and_then(|block| s.all(block, F::Body).next()).filter(|&n| s.is(n, K::Throw))
        .and_then(|throw| s.get(throw, F::Arg)).map(|n| s.unparen(n)));
    Value::Bool(pair.is_some_and(|(param, arg)| s.is(param, K::Ident) && s.is(arg, K::Ident) && s.text(param) == s.text(arg)))
}

pub fn empty_block(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let switch = s.is(node, K::Switch);
    if s.all(node, if switch { F::Case } else { F::Body }).next().is_some() { return Value::Null }
    let parent = s.parent(node);
    if parent.is_some_and(|p| matches!(s.kind(p), Some(K::Function | K::Method | K::Arrow | K::StaticBlock))) { return Value::Null }
    let span = s.tree.node(node);
    let start = if switch {
        let after = s.get(node, F::Discriminant).map_or(span.start, |n| s.tree.node(n).end);
        s.src[after as usize..span.end as usize].find('{').map_or(span.start, |offset| after + offset as u32)
    } else { span.start };
    if file.comments().iter().any(|c| c.start > start && c.end < span.end) { return Value::Null }
    Value::record(vec![
        ("switch".into(), Value::Bool(switch)),
        ("catch".into(), Value::Bool(parent.is_some_and(|p| s.is(p, K::Try)) && s.field(node) == Some(F::Handler))),
        ("offset".into(), Value::Num(f64::from(start))),
    ])
}

pub(super) fn strict(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    if file.source_type() == "module" { return true }
    let mut current = Some(node);
    while let Some(n) = current {
        if s.is(n, K::Class) { return true }
        let scope = if s.is(n, K::Program) { Some(n) }
            else if matches!(s.kind(n), Some(K::Function | K::Method | K::Arrow)) { s.get(n, F::Body).filter(|&b| s.is(b, K::Block)) }
            else { None };
        if scope.is_some_and(|body| s.all(body, F::Body)
            .map_while(|b| s.is(b, K::ExprStmt).then(|| s.get(b, F::Expr)).flatten().filter(|&e| s.is(e, K::String)))
            .any(|e| matches!(s.text(e), "'use strict'" | "\"use strict\""))) { return true }
        current = s.parent(n);
    }
    false
}

pub fn lone_block(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(parent) = s.parent(node) else { return Value::Null };
    if s.is(parent, K::StaticBlock) { return Value::Null }
    let nested = s.is(parent, K::Block);
    let eligible = nested || s.is(parent, K::Program) || s.is(parent, K::Case) && s.all(parent, F::Body).count() != 1;
    if !eligible { return Value::Null }
    let binding = s.all(node, F::Body).any(|n| s.is(n, K::Var) && s.word(n, F::Kind) != Some("var")
        || s.is(n, K::Class) || s.is(n, K::Function) && strict(file, n));
    if binding && !(nested && s.all(parent, F::Body).count() == 1) { return Value::Null }
    Value::from(if nested { "Nested block is redundant." } else { "Block is redundant." })
}

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
    loop {
        let Some(parent) = s.parent(current) else { break };
        if s.is(parent, K::If) && s.field(current) == Some(F::Consequent) && s.get(parent, F::Alternate).is_some() {
            return true
        }
        if !matches!(s.kind(parent), Some(K::If | K::For | K::ForIn | K::ForOf | K::While | K::With | K::Labeled)) { break }
        current = parent;
    }
    false
}

pub fn switch_default(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let cases: Vec<_> = s.all(node, F::Case).collect();
    let Some(&last) = cases.last() else { return Value::Null };
    if cases.iter().any(|&case| s.get(case, F::Test).is_none()) { return Value::Null }
    let end = s.tree.node(node).end;
    let after = s.tree.node(last).end;
    let comment = file.comments().iter().filter(|c| c.start >= after && c.end < end).next_back();
    Value::record(vec![
        ("has_comment".into(), Value::Bool(comment.is_some())),
        ("comment".into(), Value::from(comment.map_or("", |c| c.body(s.src).trim()).to_owned())),
    ])
}

pub fn extra_label(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(label) = s.get(node, F::Label) else { return Value::Null };
    let mut current = s.parent(node);
    while let Some(n) = current {
        if matches!(s.kind(n), Some(K::Function | K::Method | K::Arrow)) { break }
        if matches!(s.kind(n), Some(K::For | K::ForIn | K::ForOf | K::While | K::DoWhile | K::Switch)) {
            let matching = s.parent(n).filter(|&p| s.is(p, K::Labeled)).and_then(|p| s.get(p, F::Label))
                .is_some_and(|name| s.text(name) == s.text(label));
            return if matching { Value::record(vec![
                ("name".into(), Value::from(s.text(label).to_owned())),
                ("offset".into(), Value::Num(f64::from(s.tree.node(label).start))),
            ]) } else { Value::Null };
        }
        if s.is(n, K::Labeled) && s.get(n, F::Label).is_some_and(|name| s.text(name) == s.text(label)) { break }
        current = s.parent(n);
    }
    Value::Null
}

pub fn await_loop(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.is(node, K::ForOf) && !s.has_word(node, F::Modifier, "await")
        || s.is(node, K::Var) && s.word(node, F::Kind) != Some("await using") { return Value::Bool(false) }
    let mut current = node;
    while let Some(parent) = s.parent(current) {
        if matches!(s.kind(parent), Some(K::Function | K::Method | K::Arrow))
            || s.is(parent, K::ForOf) && s.has_word(parent, F::Modifier, "await") { break }
        let field = s.field(current);
        let looped = match s.kind(parent) {
            Some(K::For) => matches!(field, Some(F::Body | F::Test | F::Update)),
            Some(K::ForIn | K::ForOf) => field == Some(F::Body) || field == Some(F::Left) && s.word(current, F::Kind) == Some("await using"),
            Some(K::While | K::DoWhile) => matches!(field, Some(F::Body | F::Test)),
            _ => false,
        };
        if looped { return Value::Bool(true) }
        current = parent;
    }
    Value::Bool(false)
}

pub fn returning_assignment(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let parenthesized = s.parent(node).is_some_and(|p| s.is(p, K::Paren));
    let mut current = node;
    while let Some(parent) = s.parent(current) {
        let arrow = s.is(parent, K::Arrow);
        if s.is(parent, K::Return) || arrow && s.field(current) == Some(F::Body) {
            return Value::record(vec![
                ("arrow".into(), Value::Bool(arrow)),
                ("parenthesized".into(), Value::Bool(parenthesized)),
                ("offset".into(), Value::Num(f64::from(s.tree.node(parent).start))),
            ]);
        }
        if matches!(s.kind(parent), Some(K::Function | K::Method | K::Arrow | K::Class | K::Block | K::StaticBlock
            | K::ExprStmt | K::Var | K::If | K::For | K::ForIn | K::ForOf | K::While | K::DoWhile | K::Switch
            | K::Throw | K::With | K::Labeled | K::Try | K::Export)) { break }
        current = parent;
    }
    Value::Null
}

pub fn sequence_context(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut outer = node;
    let mut parens = 0;
    while let Some(parent) = s.parent(outer).filter(|&n| s.is(n, K::Paren)) { outer = parent; parens += 1; }
    let parent = s.parent(outer);
    if parent.is_some_and(|n| s.is(n, K::For)) && matches!(s.field(outer), Some(F::Init | F::Update)) { return Value::Null }
    let intentional = parens > 0 && !(parens == 1 && parent.is_some_and(|n| s.is(n, K::Arrow)) && s.field(outer) == Some(F::Body));
    let Some(first) = s.all(node, F::Expr).next() else { return Value::Null };
    let from = s.tree.node(first).end;
    let mut lexer = Lexer::new(&s.src[from as usize..s.tree.node(node).end as usize]);
    let offset = loop {
        let token = lexer.next_token();
        if token.tok == Tok::Eof { return Value::Null }
        if token.tok == Tok::P(",") { break from + token.start }
    };
    Value::record(vec![("intentional".into(), Value::Bool(intentional)), ("offset".into(), Value::Num(f64::from(offset)))])
}

fn brace_expectations(file: &NativeFile, owner: u32, body: u32) -> [i32; 3] {
    let s = &file.syntax;
    let block = s.is(body, K::Block);
    let statements: Vec<_> = s.all(body, F::Body).collect();
    if block && (statements.len() != 1 || braces_necessary(file, body)) { return [1; 3] }
    let span = s.tree.node(body);
    let mut before = s.tree.node(owner).start;
    let mut lexer = Lexer::new(&s.src[before as usize..span.start as usize]);
    let from = before;
    loop {
        let token = lexer.next_token();
        if token.tok == Tok::Eof { break }
        before = from + token.start;
    }
    let one_line = |node| {
        if s.is(node, K::Empty) { return true }
        let span = s.tree.node(node);
        let mut end = span.start;
        let mut lexer = Lexer::new(s.text(node));
        loop {
            let token = lexer.next_token();
            if token.tok == Tok::Eof { break }
            if token.tok != Tok::P(";") { end = span.start + token.end; }
        }
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
        !one_line(statement) || file.comments().iter().any(|c| c.start > span.start && c.end <= s.tree.node(statement).start)
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn looping_paths_respect_reachability_jump_targets_and_finally() {
        let lint = native_linter("rule no-unreachable-loop");
        for (source, expected) in [
            ("function f() { while (a) { if (b) continue; break; } }", 0),
            ("function f() { while (a) { if (b) return; throw e; } }", 1),
            ("function f() { return; while (a) { break; } }", 0),
            ("outer: while (a) { while (b) { continue outer; } break; }", 1),
            ("function f() { while (a) { try { return; } finally { continue; } } }", 0),
            ("function f() { while (a) { try { continue; } finally { break; } } }", 1),
        ] {
            assert_eq!(lint.lint(Path::new("/root/a.js"), source).len(), expected, "{source}");
        }
    }

    #[test]
    fn lone_blocks_preserve_only_real_strict_scope_bindings() {
        let lint = native_linter("rule no-lone-blocks");
        for (source, expected) in [
            ("'use strict'; { function f() {} }", 0),
            ("foo(); 'use strict'; { function f() {} }", 1),
            ("if (x) { 'use strict'; { function f() {} } other(); }", 1),
            ("function outer() { 'use strict'; { function f() {} } other(); }", 0),
        ] {
            assert_eq!(lint.lint(Path::new("/root/a.js"), source).len(), expected, "{source}");
        }
    }

    #[test]
    fn empty_blocks_distinguish_static_bodies_from_explicit_inner_blocks() {
        let lint = native_linter("rule no-empty");
        assert_eq!(lint.lint(Path::new("/root/a.ts"), "class C { static {} }").len(), 0);
        assert_eq!(lint.lint(Path::new("/root/a.ts"), "class C { static { {} } }").len(), 1);
    }
}
