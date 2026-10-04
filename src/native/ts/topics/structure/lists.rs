//! Syntax-owned list boundaries; expression interiors remain parser-owned.

use crate::expr::Value;
use crate::native::ts::{facts::NativeFile, kinds::{Field as F, Kind as K}, lexer::{Lexer, Tok}};

#[derive(Clone)]
struct Edge {
    start: usize,
    end: usize,
    text: String,
}

fn tokens(file: &NativeFile, start: usize, end: usize, comments: bool) -> Vec<Edge> {
    let src = file.syntax.src;
    let mut lexer = Lexer::new(&src[start..end]);
    let mut out = Vec::new();
    loop {
        let token = lexer.next_token();
        if token.tok == Tok::Eof { break }
        let from = start + token.start as usize;
        let to = start + token.end as usize;
        out.push(Edge { start: from, end: to, text: src[from..to].to_owned() });
    }
    if comments {
        out.extend(file.comments().iter().filter(|comment| start <= comment.start as usize && comment.end as usize <= end).map(|comment| Edge {
            start: comment.start as usize, end: comment.end as usize, text: comment.body(src).to_owned(),
        }));
        out.sort_unstable_by_key(|token| token.start);
    }
    out
}

fn broken(text: &str) -> bool {
    text.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

fn pair(file: &NativeFile, before: &Edge, after: &Edge) -> Value<'static> {
    let mut at = before.end;
    let mut spaced = false;
    for comment in file.comments().iter().filter(|comment| before.end <= comment.start as usize && comment.end as usize <= after.start) {
        spaced |= file.syntax.src[at..comment.start as usize].chars().any(char::is_whitespace);
        at = comment.end as usize;
    }
    spaced |= file.syntax.src[at..after.start].chars().any(char::is_whitespace);
    Value::Record(vec![
        ("offset".into(), Value::from(before.end)),
        ("prev".into(), Value::from(before.text.clone())),
        ("next".into(), Value::from(after.text.clone())),
        ("spaced".into(), Value::Bool(spaced)),
        ("broken".into(), Value::Bool(broken(&file.syntax.src[before.end..after.start]))),
    ])
}

pub(super) fn sites(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut groups = Vec::new();
    let main = match s.kind(node) {
        Some(K::Array) => Some(("ArrayExpression", "[]", F::Element)),
        Some(K::ArrayPattern) => Some(("ArrayPattern", "[]", F::Element)),
        Some(K::Object) => Some(("ObjectExpression", "{}", F::Property)),
        Some(K::ObjectPattern) => Some(("ObjectPattern", "{}", F::Property)),
        Some(K::Function) => Some((if s.parent(node).is_some_and(|parent| matches!(s.kind(parent), Some(K::Program | K::Block | K::Export))) { "FunctionDeclaration" } else { "FunctionExpression" }, "()", F::Param)),
        Some(K::Arrow) => Some(("ArrowFunctionExpression", "()", F::Param)),
        Some(K::Call) => Some(("CallExpression", "()", F::Arg)),
        Some(K::New) => Some(("NewExpression", "()", F::Arg)),
        Some(K::If) => Some(("IfStatement", "()", F::Test)),
        Some(K::Import) => Some(("ImportDeclaration", "{}", F::Specifier)),
        Some(K::Export) if s.get(node, F::Decl).is_none() => Some(("ExportNamedDeclaration", "{}", F::Specifier)),
        Some(K::Enum) => Some(("TSEnumBody", "{}", F::Member)),
        Some(K::FnType) => Some(("TSFunctionType", "()", F::Param)),
        Some(K::Interface) => Some(("TSInterfaceBody", "{}", F::Member)),
        Some(K::TypeLit) => Some(("TSTypeLiteral", "{}", F::Member)),
        Some(K::TupleType) => Some(("TSTupleType", "[]", F::Element)),
        _ => None,
    };
    if let Some((kind, brackets, field)) = main {
        let mut items: Vec<_> = s.all(node, field).collect();
        if s.is(node, K::ObjectPattern) { items.extend(s.all(node, F::Rest)); }
        groups.push((kind, brackets, items));
    }
    for (kind, field) in [("TSTypeParameterDeclaration", F::TypeParam), ("TSTypeParameterInstantiation", F::TypeArg)] {
        let items: Vec<_> = s.all(node, field).collect();
        if !items.is_empty() { groups.push((kind, "<>", items)); }
    }
    let mut out = Vec::new();
    let bounds = s.tree.node(node);
    for (kind, brackets, mut items) in groups {
        items.sort_unstable_by_key(|&item| s.tree.node(item).start);
        let open = &brackets[..1];
        let close = &brackets[1..];
        let first = items.first().map(|&item| s.tree.node(s.unparen(item)).start as usize);
        let last = items.last().map(|&item| s.tree.node(item).end as usize);
        let prefix_end = first.unwrap_or_else(|| s.get(node, F::Body).map_or(bounds.end as usize, |body| s.tree.node(body).start as usize));
        let prefix = tokens(file, bounds.start as usize, prefix_end, false);
        let left = if brackets == "()" && matches!(s.kind(node), Some(K::Call | K::New)) {
            let after = s.all(node, F::TypeArg).last().or_else(|| s.get(node, F::Callee))
                .map_or(bounds.start, |before| s.tree.node(before).end) as usize;
            prefix.iter().find(|token| token.start >= after && token.text == open).cloned()
        } else if first.is_some() {
            prefix.last().filter(|token| token.text == open).cloned()
        } else {
            prefix.windows(2).find(|pair| pair[0].text == open && pair[1].text == close).map(|pair| pair[0].clone())
        };
        let Some(left) = left else { continue };
        let after_last = tokens(file, last.unwrap_or(left.end), bounds.end as usize, false);
        let Some(right) = after_last.iter().find(|token| token.text != ",").filter(|token| token.text == close).cloned() else { continue };
        let first_non_hole = items.iter().find(|&&item| !s.is(item, K::Hole)).copied();
        let first_item_start = first_non_hole.map_or(right.start, |item| s.tree.node(s.unparen(item)).start as usize);
        let inner_comments = tokens(file, left.end, first_item_start, true);
        let first_item = first_non_hole.and_then(|item| tokens(file, s.tree.node(s.unparen(item)).start as usize, s.tree.node(item).end as usize, false).first().cloned());
        let first_edge = inner_comments.first().cloned().or_else(|| first_item.clone()).unwrap_or_else(|| right.clone());
        let tail_comments = tokens(file, last.unwrap_or(left.end), right.start, true);
        let last_edge = tail_comments.last().cloned().or_else(|| {
            items.last().and_then(|&item| tokens(file, s.tree.node(item).start as usize, s.tree.node(item).end as usize, false).last().cloned())
        }).unwrap_or_else(|| left.clone());
        let mut spacing = vec![pair(file, &left, &first_edge)];
        if first_edge.start != right.start { spacing.push(pair(file, &last_edge, &right)); }
        let mut wrapping = Vec::new();
        let after_open = tokens(file, left.end, first_item_start, false).first().cloned().or_else(|| {
            items.first().and_then(|&item| tokens(file, s.tree.node(s.unparen(item)).start as usize, s.tree.node(item).end as usize, false).first().cloned())
        }).unwrap_or_else(|| right.clone());
        wrapping.push(pair(file, &left, &after_open));
        let mut previous = left.end;
        let mut previous_edge = left.clone();
        for (index, &item) in items.iter().enumerate() {
            let inner = s.unparen(item);
            let item_node = s.tree.node(inner);
            let Some(first_token) = tokens(file, item_node.start as usize, item_node.end as usize, false).first().cloned() else { previous = s.tree.node(item).end as usize; continue };
            if index != 0 || after_open.start != first_token.start {
                let before = tokens(file, previous, first_token.start, false).into_iter().rev().find(|token| token.text != "(").unwrap_or_else(|| previous_edge.clone());
                wrapping.push(pair(file, &before, &first_token));
            }
            previous = s.tree.node(item).end as usize;
            if let Some(edge) = tokens(file, s.tree.node(item).start as usize, previous, false).last() { previous_edge = edge.clone(); }
        }
        let last_plain = after_last.iter().take_while(|token| token.start < right.start).last().cloned().or_else(|| {
            items.last().and_then(|&item| tokens(file, s.tree.node(item).start as usize, s.tree.node(item).end as usize, false).last().cloned())
        }).unwrap_or_else(|| left.clone());
        wrapping.push(pair(file, &last_plain, &right));
        let empty_wrapping = if first_edge.start == right.start { vec![pair(file, &left, &right)] }
            else { vec![pair(file, &left, &first_edge), pair(file, &last_edge, &right)] };
        let common = vec![
            ("kind".into(), Value::from(kind)),
            ("brackets".into(), Value::from(brackets)),
            ("count".into(), Value::from(items.len())),
            ("single".into(), Value::Bool(!broken(&s.src[left.start..right.end]))),
            ("node_single".into(), Value::Bool(!broken(s.text(node)))),
            ("first_broken".into(), Value::Bool(first.is_some_and(|at| broken(&s.src[left.end..at])))),
            ("empty_comments".into(), Value::Bool(first_edge.start != right.start)),
        ];
        for (mode, pairs) in [("spacing", spacing), ("wrapping", wrapping), ("empty_wrapping", empty_wrapping)] {
            for pair in pairs {
                let Value::Record(mut fields) = pair else { continue };
                fields.extend(common.clone());
                fields.push(("mode".into(), Value::from(mode)));
                out.push(Value::Record(fields));
            }
        }
    }
    Value::List(out)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn wrapping_limit_rejects_inline_boundaries_not_wrapped_ones() {
        let lint = native_linter("rule list-style { set singleLine { maxItems: 1 } }");
        for (source, count) in [
            ("const x = [1, 2];", 3),
            ("const x = [\n 1,\n 2,\n];", 0),
        ] {
            assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{source}");
        }
    }
}
