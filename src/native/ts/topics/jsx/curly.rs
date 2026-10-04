//! Brace policies share measurements, not comment semantics.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::metadata::space;
use crate::expr::Value;

fn newline(text: &str) -> bool {
    text.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

pub fn gaps(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.is(node, K::JsxExpression) && s.get(node, F::Expr).is_some_and(|expr| s.is(expr, K::Spread)) {
        return Value::Null;
    }
    let n = s.tree.node(node);
    let start = n.start as usize;
    let end = n.end as usize;
    let body = &s.src[start + 1..end - 1];
    let leading = body.len() - body.trim_start_matches(space).len();
    let trailing = body.trim_end_matches(space).len();
    let expression = s.get(node, F::Expr).or_else(|| s.get(node, F::Arg));
    let (first, last, single) = expression.map_or((end - 1, start + 1, !newline(body)), |id| {
        let expr = s.tree.node(id);
        (expr.start as usize, expr.end as usize, !newline(s.text(id)))
    });
    let attribute = s.is(node, K::JsxSpreadAttribute)
        || s.parent(node).is_some_and(|p| s.is(p, K::JsxAttribute));
    let sides = [
        ("after", "{", start, &body[..leading], &s.src[start + 1..first]),
        ("before", "}", end - 1, &body[trailing..], &s.src[last..end - 1]),
    ].into_iter().map(|(direction, token, offset, gap, token_gap)| Value::Record(vec![
        ("direction".into(), Value::from(direction)),
        ("token".into(), Value::from(token)),
        ("offset".into(), Value::from(offset)),
        ("space".into(), Value::Bool(!gap.is_empty())),
        ("newline".into(), Value::Bool(newline(gap))),
        ("token_newline".into(), Value::Bool(newline(token_gap))),
    ])).collect();
    Value::Record(vec![
        ("attribute".into(), Value::Bool(attribute)),
        ("object".into(), Value::Bool(body.trim_start_matches(space).starts_with('{'))),
        ("single".into(), Value::Bool(single)),
        ("left_newline".into(), Value::Bool(newline(&s.src[start + 1..first]))),
        ("sides".into(), Value::List(sides)),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn context_spacing_object_replaces_global_spacing() {
        let l = native_linter("rule jsx-curly-spacing { set options [{when: 'always', spacing: {objectLiterals: 'never'}, attributes: {spacing: {}}}] }");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a={{x: 1}} />;").len(), 2);
    }

    #[test]
    fn spread_children_are_not_expression_containers() {
        let l = native_linter("rule jsx-curly-spacing { set options [{children: true}] }");
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box>{ ...items }</Box>;").is_empty());
    }
}
