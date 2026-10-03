//! Token boundaries of blocks and computed property brackets.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::{gap_has_space, lex, lex_comments};

pub(super) fn member_delimiter_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let kind = match syntax.kind(node) {
        Some(K::Interface) => "interface",
        Some(K::TypeLit) => "typeLiteral",
        _ => return Value::List(Vec::new()),
    };
    let Some(start) = brace_start(file, node) else { return Value::List(Vec::new()) };
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
    Value::List(members.iter().enumerate().filter_map(|(i, &id)| {
        let token = last_token(id)?;
        let delimiter = match token.tok {
            Tok::P(";") => "semi",
            Tok::P(",") => "comma",
            _ => "none",
        };
        Some(Value::Record(vec![
            ("kind".into(), Value::from(kind)),
            ("single_line".into(), Value::Bool(single_line)),
            ("last_same_line".into(), Value::Bool(last_same_line)),
            ("last".into(), Value::Bool(i + 1 == members.len())),
            ("delimiter".into(), Value::from(delimiter)),
            ("offset".into(), Value::from(token.end as usize)),
        ]))
    }).collect())
}

pub(super) fn brace_style_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    if !matches!(syntax.kind(node), Some(K::Block | K::StaticBlock | K::Switch | K::Class)) {
        return Value::List(Vec::new());
    }
    let parent = syntax.parent(node);
    if syntax.is(node, K::Block) && parent.is_some_and(|id| matches!(syntax.kind(id), Some(K::Program | K::Block | K::StaticBlock | K::Case))) {
        return Value::List(Vec::new());
    }
    let Some(start) = brace_start(file, node) else { return Value::List(Vec::new()) };
    let lexed = file.cached(lex);
    let tokens = &lexed.0;
    let open = tokens.partition_point(|token| token.start < start);
    let close = tokens.partition_point(|token| token.end <= syntax.tree.node(node).end).saturating_sub(1);
    if close <= open || tokens[close].tok != Tok::P("}") {
        return Value::List(Vec::new());
    }
    let same = |left: usize, right: usize| !syntax.src[tokens[left].end as usize..tokens[right].start as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']);
    let single_line = same(open, close);
    let mut sites = Vec::new();
    let mut site = |kind, offset, same_line| sites.push(Value::Record(vec![
        ("kind".into(), Value::from(kind)),
        ("offset".into(), Value::from(offset)),
        ("same_line".into(), Value::Bool(same_line)),
        ("single_line".into(), Value::Bool(single_line)),
    ]));
    if let Some(before) = open.checked_sub(1) {
        site("open", tokens[open].start as usize, same(before, open));
    }
    if close > open + 1 {
        site("body", tokens[open].start as usize, same(open, open + 1));
        site("close", tokens[close].start as usize, same(close - 1, close));
    }
    if parent.is_some_and(|id| matches!(syntax.kind(id), Some(K::If | K::Try)))
        && tokens.get(close + 1).is_some_and(|token| token.tok == Tok::Ident && matches!(&syntax.src[token.start as usize..token.end as usize], "else" | "catch" | "finally")) {
        site("keyword", tokens[close].start as usize, same(close, close + 1));
    }
    Value::List(sites)
}

pub(super) fn block_prefix_spacing(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let class = matches!(syntax.kind(node), Some(K::Class | K::Interface | K::Enum));
    if !class && !matches!(syntax.kind(node), Some(K::Block | K::Switch)) {
        return Value::Null;
    }
    let lexed = file.cached(lex);
    let Some(start) = brace_start(file, node) else { return Value::Null };
    let open = lexed.0.partition_point(|token| token.start < start);
    let Some(previous) = open.checked_sub(1) else { return Value::Null };
    let previous = lexed.0[previous];
    let keyword = previous.tok == Tok::Ident && matches!(&syntax.src[previous.start as usize..previous.end as usize], "try" | "do" | "else" | "finally" | "catch" | "static");
    let parent = syntax.parent(node);
    let function = parent.is_some_and(|parent| matches!(syntax.kind(parent), Some(K::Function | K::Arrow | K::Method)));
    if previous.tok == Tok::P("=>")
        || keyword && !class && !function
        || previous.tok == Tok::P(":") && parent.is_some_and(|parent| syntax.is(parent, K::Case)) {
        return Value::Null;
    }
    let gap = &syntax.src[previous.end as usize..lexed.0[open].start as usize];
    if gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return Value::Null;
    }
    let kind = if class { "classes" } else if function { "functions" } else if parent.is_some_and(|parent| syntax.is(parent, K::Namespace)) { "modules" } else { "keywords" };
    Value::Record(vec![
        ("kind".into(), Value::from(kind)),
        ("offset".into(), Value::from(lexed.0[open].start as usize)),
        ("spaced".into(), Value::Bool(gap_has_space(gap))),
    ])
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

pub(super) fn function_paren_spacing(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let span = syntax.tree.node(node);
    let lexed = file.cached(lex);
    let first = lexed.0.partition_point(|token| token.start < span.start);
    let named = syntax.is(node, K::Method) || syntax.get(node, F::Name).is_some();
    let generic = syntax.all(node, F::TypeParam).last();
    let (kind, open) = match syntax.kind(node) {
        Some(K::Try) => {
            let Some(param) = syntax.get(node, F::Param) else { return Value::Null };
            let at = lexed.0.partition_point(|token| token.start < syntax.tree.node(param).start);
            ("catch", at.checked_sub(1))
        }
        Some(K::Arrow) => {
            if lexed.0.get(first).is_none_or(|token| &syntax.src[token.start as usize..token.end as usize] != "async") {
                return Value::Null;
            }
            ("asyncArrow", Some(first + 1))
        }
        Some(K::Function | K::Method) => {
            if !named && syntax.all(node, F::Modifier).any(|modifier| syntax.text(modifier) == "*") {
                return Value::Null;
            }
            let from = generic.or_else(|| syntax.get(node, F::Name)).or_else(|| syntax.get(node, F::Key)).map_or(span.start, |id| syntax.tree.node(id).end);
            let at = lexed.0.partition_point(|token| token.start < from);
            let limit = syntax.get(node, F::Body).map_or(span.end, |id| syntax.tree.node(id).start);
            (if named { "named" } else { "anonymous" }, (at..lexed.0.len()).take_while(|&i| lexed.0[i].start < limit).find(|&i| lexed.0[i].tok == Tok::P("(")))
        }
        _ => return Value::Null,
    };
    let Some(open) = open.filter(|&i| lexed.0.get(i).is_some_and(|token| token.tok == Tok::P("("))) else { return Value::Null };
    let Some(previous) = open.checked_sub(1) else { return Value::Null };
    let start = lexed.0[previous].end as usize;
    let end = lexed.0[open].start as usize;
    Value::Record(vec![
        ("kind".into(), Value::from(kind)),
        ("start".into(), Value::from(start)),
        ("end".into(), Value::from(end)),
        ("spaced".into(), Value::Bool(gap_has_space(&syntax.src[start..end]))),
        ("generic_anonymous".into(), Value::Bool(generic.is_some() && !named)),
    ])
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

    #[test]
    fn class_body_spacing_does_not_target_a_type_argument_brace() {
        let lint = native_linter(r#"
            in ts_class rule block-body {
                when block_prefix_spacing != null && !block_prefix_spacing.spaced
                error "spacing"
            }
        "#);
        let reports = lint.lint(Path::new("/root/file.ts"), "class C extends Base<{ x: number }> {}");
        assert!(reports.is_empty());
    }
}
