//! Token boundaries of blocks and computed property brackets.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::{gap_has_space, lex, lex_comments};

pub(super) fn property_spacing(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let (left, right, indexed, same_line) = match syntax.kind(node) {
        Some(K::Member) => (syntax.get(node, F::Object), syntax.get(node, F::Property), false, true),
        Some(K::Index) => (syntax.get(node, F::Object), syntax.get(node, F::Index), true, true),
        Some(K::IndexedAccess) => (syntax.get(node, F::Object), syntax.get(node, F::Index), true, false),
        Some(K::QualifiedName) => (syntax.get(node, F::Left), syntax.get(node, F::Right), false, false),
        Some(K::ImportType) => (None, syntax.get(node, F::Qualifier), false, false),
        _ => return Value::Null,
    };
    let Some(right) = right else { return Value::Null };
    let right_span = syntax.tree.node(right);
    let lexed = file.cached(lex);
    let right_at = lexed.0.partition_point(|token| token.start < right_span.start);
    let start = if let Some(left) = left {
        syntax.tree.node(left).end
    } else {
        let Some(at) = right_at.checked_sub(2) else { return Value::Null };
        lexed.0[at].end
    };
    let end = if indexed {
        let from = lexed.0.partition_point(|token| token.start < start);
        let Some(open) = (from..right_at).rev().find(|&i| lexed.0[i].tok == Tok::P("[")) else { return Value::Null };
        lexed.0[open].start
    } else {
        right_span.start
    };
    if same_line && syntax.src[start as usize..right_span.start as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return Value::Null;
    }
    Value::Record(vec![
        ("name".into(), Value::from(syntax.text(right).to_owned())),
        ("spaced".into(), Value::Bool(gap_has_space(&syntax.src[start as usize..end as usize]))),
    ])
}

pub(super) fn delimiter_spacing_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let span = syntax.tree.node(node);
    let block = matches!(syntax.kind(node), Some(K::Block | K::StaticBlock | K::Switch | K::Enum));
    let from = match syntax.kind(node) {
        Some(K::Switch) => syntax.get(node, F::Discriminant).map_or(span.start, |id| syntax.tree.node(id).end),
        Some(K::Index | K::IndexedAccess) => syntax.get(node, F::Object).map_or(span.start, |id| syntax.tree.node(id).end),
        Some(K::Computed | K::Block | K::StaticBlock | K::Enum) => span.start,
        _ => return Value::List(Vec::new()),
    };
    let lexed = file.cached(lex_comments);
    let at = lexed.0.partition_point(|token| token.start < from);
    let opener = if block { "{" } else { "[" };
    let Some(open) = (at..lexed.0.len()).take_while(|&i| lexed.0[i].start < span.end).find(|&i| lexed.0[i].tok == Tok::P(opener)) else {
        return Value::List(Vec::new());
    };
    let Some(close) = lexed.0.partition_point(|token| token.end <= span.end).checked_sub(1).filter(|&i| lexed.0[i].tok == Tok::P(if block { "}" } else { "]" })) else {
        return Value::List(Vec::new());
    };
    if close <= open + 1 {
        return Value::List(Vec::new());
    }
    let class_member = syntax.is(node, K::Computed) && syntax.parent(node).is_some_and(|parent|
        syntax.is(parent, K::ClassProp) || syntax.is(parent, K::Method) && syntax.parent(parent).is_some_and(|class| syntax.is(class, K::Class)));
    let line_comment = lexed.0[open + 1].tok == Tok::P("//");
    let mut sites = Vec::new();
    for (side, delimiter, adjacent) in [("after", open, open + 1), ("before", close, close - 1)] {
        let token = lexed.0[delimiter];
        let neighbour = lexed.0[adjacent];
        let (start, end) = if side == "after" { (token.end, neighbour.start) } else { (neighbour.end, token.start) };
        let gap = &syntax.src[start as usize..end as usize];
        if gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            continue;
        }
        let spaced = gap_has_space(gap);
        sites.push(Value::Record(vec![
            ("side".into(), Value::from(side)),
            ("token".into(), Value::from(syntax.src[token.start as usize..token.end as usize].to_owned())),
            ("offset".into(), Value::from(if spaced { start as usize } else { token.start as usize })),
            ("start".into(), Value::from(start as usize)),
            ("end".into(), Value::from(end as usize)),
            ("spaced".into(), Value::Bool(spaced)),
            ("class_member".into(), Value::Bool(class_member)),
            ("line_comment".into(), Value::Bool(line_comment)),
        ]));
    }
    Value::List(sites)
}
