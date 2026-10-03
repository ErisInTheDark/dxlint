//! Spacing sites owned by expression operators.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::{gap_has_space, lex};

pub(super) fn infix_spacing_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let lexed = file.cached(lex);
    let mut ranges = Vec::new();
    let types = matches!(syntax.kind(node), Some(K::Union | K::Intersection));
    if let Some(op) = syntax.get(node, F::Op).filter(|_| matches!(syntax.kind(node), Some(K::Binary | K::Logical | K::Assign))) {
        let span = syntax.tree.node(op);
        ranges.push((span.start, span.end));
    } else {
        let pairs: Vec<_> = match syntax.kind(node) {
            Some(K::AssignPattern) => vec![(syntax.get(node, F::Left).or_else(|| syntax.parent(node).and_then(|parent| syntax.get(parent, F::Key))), syntax.get(node, F::Right))],
            Some(K::Conditional) => vec![(syntax.get(node, F::Test), syntax.get(node, F::Consequent)), (syntax.get(node, F::Consequent), syntax.get(node, F::Alternate))],
            Some(K::ConditionalType) => vec![(syntax.get(node, F::Extends), syntax.get(node, F::Consequent)), (syntax.get(node, F::Consequent), syntax.get(node, F::Alternate))],
            Some(K::Declarator | K::Param) => vec![(syntax.get(node, F::Type).or_else(|| syntax.get(node, F::Name)), syntax.get(node, F::Init))],
            Some(K::ClassProp) => vec![(syntax.get(node, F::Type).or_else(|| syntax.get(node, F::Optional)).or_else(|| syntax.get(node, F::Key)), syntax.get(node, F::Init))],
            Some(K::EnumMember) => vec![(syntax.get(node, F::Name), syntax.get(node, F::Init))],
            Some(K::TypeAlias) => vec![(syntax.all(node, F::TypeParam).last().or_else(|| syntax.get(node, F::Name)), syntax.get(node, F::Type))],
            Some(K::TypeParam) => vec![(syntax.get(node, F::Constraint).or_else(|| syntax.get(node, F::Name)), syntax.get(node, F::Default))],
            _ => Vec::new(),
        };
        for (left, right) in pairs {
            let (Some(left), Some(right)) = (left, right) else { continue };
            let from = syntax.tree.node(left).end;
            let end = syntax.tree.node(right).start;
            let at = lexed.0.partition_point(|token| token.start < from);
            if let Some(op) = lexed.0[at..].iter().take_while(|token| token.end <= end).find(|token| matches!(token.tok, Tok::P("=" | "?" | ":"))) {
                ranges.push((op.start, op.end));
            }
        }
        if types {
            for ty in syntax.all(node, F::Type) {
                let start = syntax.tree.node(ty).start;
                if let Some(at) = lexed.0.partition_point(|token| token.start < start).checked_sub(1) {
                    let op = lexed.0[at];
                    if matches!(op.tok, Tok::P("|" | "&")) {
                        ranges.push((op.start, op.end));
                    }
                }
            }
        }
    }
    let int32_hint = matches!(syntax.kind(node), Some(K::Binary | K::Logical | K::Assign | K::AssignPattern)) && syntax.text(node).ends_with("|0");
    Value::List(ranges.into_iter().filter_map(|(start, end)| {
        let at = lexed.0.partition_point(|token| token.start < start);
        let before = lexed.0.get(at.checked_sub(1)?);
        let after = lexed.0.get(lexed.0.partition_point(|token| token.start < end));
        let (Some(before), Some(after)) = (before, after) else { return None };
        let spaced = gap_has_space(&syntax.src[before.end as usize..start as usize]) && gap_has_space(&syntax.src[end as usize..after.start as usize]);
        Some(Value::Record(vec![
            ("operator".into(), Value::from(syntax.src[start as usize..end as usize].to_owned())),
            ("offset".into(), Value::from(start as usize)),
            ("spaced".into(), Value::Bool(spaced)),
            ("types".into(), Value::Bool(types)),
            ("int32_hint".into(), Value::Bool(int32_hint)),
        ]))
    }).collect())
}

pub(super) fn star_spacing_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let is_yield = syntax.is(node, K::Yield);
    let star = if is_yield {
        syntax.get(node, F::Op)
    } else if matches!(syntax.kind(node), Some(K::Function | K::Method)) {
        syntax.all(node, F::Modifier).find(|&modifier| syntax.text(modifier) == "*")
    } else {
        None
    };
    let Some(star) = star else { return Value::List(Vec::new()) };
    let lexed = file.cached(lex);
    let at = lexed.0.partition_point(|token| token.start < syntax.tree.node(star).start);
    let Some(token) = lexed.0.get(at) else { return Value::List(Vec::new()) };
    let method = syntax.is(node, K::Method);
    let kind = if is_yield {
        "yield"
    } else if method {
        if syntax.parent(node).is_some_and(|parent| syntax.is(parent, K::Object)) { "shorthand" } else { "method" }
    } else if syntax.get(node, F::Name).is_some() {
        "named"
    } else {
        "anonymous"
    };
    let mut sites = Vec::new();
    for (side, adjacent) in [
        ("before", at.checked_sub(1).and_then(|i| lexed.0.get(i))),
        ("after", lexed.0.get(at + 1)),
    ] {
        if method && side == "before" && token.start == syntax.tree.node(node).start {
            continue;
        }
        let Some(adjacent) = adjacent else { continue };
        let (mut start, mut end) = if side == "before" {
            (adjacent.end, token.start)
        } else {
            (token.end, adjacent.start)
        };
        if !is_yield {
            if side == "before" {
                if let Some(comment) = file.comments().iter().rev().find(|comment| start <= comment.start && comment.end <= end) {
                    start = comment.end;
                }
            } else if let Some(comment) = file.comments().iter().find(|comment| start <= comment.start && comment.end <= end) {
                end = comment.start;
            }
        }
        sites.push(Value::Record(vec![
            ("kind".into(), Value::from(kind)),
            ("side".into(), Value::from(side)),
            ("offset".into(), Value::from(token.start as usize)),
            ("start".into(), Value::from(start as usize)),
            ("end".into(), Value::from(end as usize)),
            ("separated".into(), Value::Bool(start != end)),
            ("spaced".into(), Value::Bool(gap_has_space(&syntax.src[start as usize..end as usize]))),
        ]));
    }
    Value::List(sites)
}

pub(super) fn unary_spacing(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let kind = syntax.kind(node);
    if !matches!(kind, Some(K::Unary | K::Update | K::New | K::Await | K::Yield | K::NonNull)) {
        return Value::Null;
    }
    let span = syntax.tree.node(node);
    let lexed = file.cached(lex);
    let first = lexed.0.partition_point(|token| token.start < span.start);
    let last = lexed.0.partition_point(|token| token.end <= span.end);
    if last < first + 2 {
        return Value::Null;
    }
    let prefix = match kind {
        Some(K::NonNull) => false,
        Some(K::Unary | K::Update) => syntax.get(node, F::Op).is_none_or(|op| syntax.tree.node(op).start == span.start),
        _ => true,
    };
    let (left, right) = if prefix {
        (lexed.0[first], lexed.0[first + 1])
    } else {
        (lexed.0[last - 2], lexed.0[last - 1])
    };
    if kind == Some(K::Yield) && right.tok == Tok::P("*") {
        return Value::Null;
    }
    let operator_token = if prefix { left } else { right };
    let operator = &syntax.src[operator_token.start as usize..operator_token.end as usize];
    let word = operator_token.tok == Tok::Ident;
    let removable = if word {
        !matches!(right.tok, Tok::Ident | Tok::Num | Tok::BigInt)
    } else {
        !matches!((left.tok, right.tok), (Tok::P("+"), Tok::P("+" | "++")) | (Tok::P("-"), Tok::P("-" | "--")))
    };
    let first_bang = operator == "!" && prefix && syntax.get(node, F::Arg).is_some_and(|arg| syntax.is(arg, K::Unary) && syntax.get(arg, F::Op).is_some_and(|op| syntax.text(op) == "!"));
    let start = left.end as usize;
    let end = right.start as usize;
    Value::Record(vec![
        ("operator".into(), Value::from(operator.to_owned())),
        ("word".into(), Value::Bool(word)),
        ("prefix".into(), Value::Bool(prefix)),
        ("start".into(), Value::from(start)),
        ("end".into(), Value::from(end)),
        ("separated".into(), Value::Bool(start != end)),
        ("removable".into(), Value::Bool(removable)),
        ("first_bang".into(), Value::Bool(first_bang)),
        ("override_key".into(), Value::from(if kind == Some(K::NonNull) { "ts-non-null" } else { operator }.to_owned())),
    ])
}

pub(super) fn spread_spacing(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let rest = match syntax.kind(node) {
        Some(K::Rest) => true,
        Some(K::Spread) => false,
        _ => return Value::Null,
    };
    let lexed = file.cached(lex);
    let at = lexed.0.partition_point(|token| token.start < syntax.tree.node(node).start);
    let Some(operator) = lexed.0.get(at).filter(|token| token.tok == Tok::P("...")) else { return Value::Null };
    let Some(next) = lexed.0.get(at + 1) else { return Value::Null };
    let start = operator.end as usize;
    let end = file.comments().iter().find(|comment| operator.end <= comment.start && comment.end <= next.start).map_or(next.start, |comment| comment.start) as usize;
    let property = syntax.parent(node).is_some_and(|parent| matches!(syntax.kind(parent), Some(K::Object | K::ObjectPattern)));
    let kind = match (rest, property) {
        (true, true) => "rest property",
        (true, false) => "rest",
        (false, true) => "spread property",
        (false, false) => "spread",
    };
    Value::Record(vec![
        ("kind".into(), Value::from(kind)),
        ("start".into(), Value::from(start)),
        ("end".into(), Value::from(end)),
        ("spaced".into(), Value::Bool(gap_has_space(&syntax.src[start..end]))),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;

    #[test]
    fn infix_spacing_covers_default_initializers() {
        let lint = native_linter(r#"
            in ts_parameter, ts_assignment_pattern, ts_declarator rule spacing {
                each site in filter(infix_spacing_sites, s => !s.spaced)
                report at site.offset
                error "spacing"
            }
        "#);
        let path = Path::new("/root/file.ts");
        assert_eq!(lint.lint(path, "var {a=0}=bar; function foo(a: number=0) {}").len(), 3);
        assert!(lint.lint(path, "var {a = 0} = bar; function foo(a: number = 0) {}").is_empty());
    }
}
