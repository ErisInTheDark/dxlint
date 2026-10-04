//! Statement traversal events for per-line limits.

use crate::expr::Value;
use crate::native::ts::{facts::NativeFile, kinds::{Field as F, Kind as K}, syntax::Syntax};

fn declaration(s: &Syntax, node: u32) -> bool {
    s.parent(node).is_some_and(|parent| matches!(s.kind(parent),
        Some(K::Program | K::Block | K::If | K::For | K::ForIn | K::ForOf | K::While | K::DoWhile | K::Labeled | K::With | K::Export)))
}

fn statement(s: &Syntax, node: u32) -> Option<&'static str> {
    Some(match s.kind(node)? {
        K::Break => "BreakStatement",
        K::Class if declaration(s, node) => "ClassDeclaration",
        K::Continue => "ContinueStatement",
        K::Debugger => "DebuggerStatement",
        K::DoWhile => "DoWhileStatement",
        K::ExprStmt => "ExpressionStatement",
        K::ForIn => "ForInStatement",
        K::ForOf => "ForOfStatement",
        K::For => "ForStatement",
        K::Function if declaration(s, node) => "FunctionDeclaration",
        K::If => "IfStatement",
        K::Import => "ImportDeclaration",
        K::Labeled => "LabeledStatement",
        K::Return => "ReturnStatement",
        K::Switch => "SwitchStatement",
        K::Throw => "ThrowStatement",
        K::Try => "TryStatement",
        K::Var => "VariableDeclaration",
        K::While => "WhileStatement",
        K::With => "WithStatement",
        K::Export if s.has_word(node, F::Kind, "default") => "ExportDefaultDeclaration",
        K::Export if s.has_word(node, F::Kind, "*") => "ExportAllDeclaration",
        K::Export => "ExportNamedDeclaration",
        _ => return None,
    })
}

struct StatementEvents(Vec<Value<'static>>);

pub(super) fn events(file: &NativeFile, node: u32) -> Value<'static> {
    if !file.syntax.is(node, K::Program) { return Value::Null }
    Value::list(file.cached(scan).0.clone())
}

fn scan(file: &NativeFile) -> StatementEvents {
    let s = &file.syntax;
    let mut starts = vec![0];
    for (at, ch) in s.src.char_indices() {
        if ch == '\n' && at > 0 && s.src.as_bytes()[at - 1] == b'\r' {
            *starts.last_mut().expect("initial line exists") = at + 1;
        } else if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            starts.push(at + ch.len_utf8());
        }
    }
    let mut out = Vec::new();
    let mut stack = vec![(s.root(), true)];
    while let Some((node, enter)) = stack.pop() {
        if enter {
            stack.push((node, false));
            let kids: Vec<_> = s.kids(node).map(|(_, child)| child).collect();
            stack.extend(kids.into_iter().rev().map(|child| (child, true)));
        }
        let Some(kind) = statement(s, node) else { continue };
        let suppressed = enter && s.parent(node).is_some_and(|parent| {
            matches!(s.kind(parent), Some(K::DoWhile | K::For | K::ForIn | K::ForOf | K::If | K::Labeled | K::While))
                && s.field(node) != Some(F::Alternate)
                || s.is(parent, K::Export) && !s.has_word(parent, F::Kind, "*")
        });
        if suppressed { continue }
        let start = s.tree.node(node).start as usize;
        let at = if enter { start } else {
            start + s.text(node).trim_end().trim_end_matches(';').trim_end().len()
        };
        out.push(Value::record(vec![
            ("index".into(), Value::from(out.len())),
            ("offset".into(), Value::from(start)),
            ("line".into(), Value::from(starts.partition_point(|&start| start <= at))),
            ("kind".into(), Value::from(kind)),
            ("enter".into(), Value::Bool(enter)),
        ]));
    }
    StatementEvents(out)
}
