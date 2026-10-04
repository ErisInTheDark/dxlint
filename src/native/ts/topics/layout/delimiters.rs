//! Token boundaries of blocks and computed property brackets.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::{gap_has_space, lex};

pub(super) fn member_delimiter_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let kind = match syntax.kind(node) {
        Some(K::Interface) => "interface",
        Some(K::TypeLit) => "typeLiteral",
        _ => return Value::list(Vec::new()),
    };
    let Some(start) = brace_start(file, node) else { return Value::list(Vec::new()) };
    let span = syntax.tree.node(node);
    let lexed = file.cached(lex);
    let tokens = &lexed.0;
    let members: Vec<_> = syntax.all(node, F::Member).collect();
    let same = |from: u32, to: u32| !syntax.src[from as usize..to as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']);
    let last_token = |id| {
        let end = syntax.tree.node(id).end;
        let mut at = tokens.partition_point(|token| token.end <= end).checked_sub(1)?;
        if tokens.get(at + 1).is_some_and(|token| matches!(token.tok, Tok::P(";" | ","))) {
            at += 1;
        }
        Some(tokens[at])
    };
    let single_line = same(start, span.end);
    let last_same_line = members.last().and_then(|&id| last_token(id)).is_some_and(|token| same(token.end, span.end));
    Value::list(members.iter().enumerate().filter_map(|(i, &id)| {
        let token = last_token(id)?;
        let delimiter = match token.tok {
            Tok::P(";") => "semi",
            Tok::P(",") => "comma",
            _ => "none",
        };
        Some(Value::record(vec![
            ("kind".into(), Value::from(kind)),
            ("single_line".into(), Value::Bool(single_line)),
            ("last_same_line".into(), Value::Bool(last_same_line)),
            ("last".into(), Value::Bool(i + 1 == members.len())),
            ("delimiter".into(), Value::from(delimiter)),
            ("offset".into(), Value::from(token.end as usize)),
        ]))
    }).collect())
}

pub(super) fn brace_start(file: &NativeFile<'_>, node: u32) -> Option<u32> {
    let syntax = &file.syntax;
    let span = syntax.tree.node(node);
    let from = match syntax.kind(node) {
        Some(K::Switch) => syntax.get(node, F::Discriminant).map_or(span.start, |id| syntax.tree.node(id).end),
        Some(K::Class | K::Interface | K::Enum) => [F::Name, F::TypeParam, F::Extends, F::TypeArg, F::Implements].into_iter()
            .flat_map(|field| syntax.all(node, field)).map(|id| syntax.tree.node(id).end).max().unwrap_or(span.start),
        Some(K::Block | K::StaticBlock | K::TypeLit) => span.start,
        _ => return None,
    };
    let lexed = file.cached(lex);
    let at = lexed.0.partition_point(|token| token.start < from);
    lexed.0[at..].iter().take_while(|token| token.start < span.end).find(|token| token.tok == Tok::P("{")).map(|token| token.start)
}

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
    Value::record(vec![
        ("name".into(), Value::from(syntax.text(right).to_owned())),
        ("spaced".into(), Value::Bool(gap_has_space(&syntax.src[start as usize..end as usize]))),
    ])
}


#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;

    #[test]
    fn last_member_detection_includes_the_separator_line() {
        let lint = native_linter(r#"
            rule member-delimiter-style {
                set multilineDetection "last-member"
            }
        "#);
        let reports = lint.lint(Path::new("/root/file.ts"), "interface C {\n value: number\n; }");
        assert_eq!(reports.len(), 1);
    }

}
