//! Brace removal must preserve JSX text, whitespace and expression adjacency.

use std::collections::HashMap;
use super::metadata::space;
use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics::literal_text;
use crate::expr::Value;

fn without_entities(text: &str) -> String {
    let mut out = String::new();
    let mut from = 0;
    while let Some(at) = text[from..].find('&') {
        let at = from + at;
        out.push_str(&text[from..at]);
        let mut end = at + 1;
        while text.as_bytes().get(end).is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == b'#') { end += 1; }
        if end > at + 1 && text.as_bytes().get(end) == Some(&b';') {
            from = end + 1;
        } else {
            out.push('&');
            from = at + 1;
        }
    }
    out.push_str(&text[from..]);
    out
}

fn whitespace(value: &str) -> bool {
    !value.is_empty() && value.chars().all(space)
}

fn edge_space(value: &str) -> bool {
    value.chars().next().is_some_and(space) || value.chars().last().is_some_and(space)
}

fn is_jsx(file: &NativeFile, node: u32) -> bool {
    matches!(file.syntax.kind(node), Some(K::JsxElement | K::JsxFragment))
}

struct Neighbours(HashMap<u32, (Option<u32>, Option<u32>, usize)>);

fn neighbours(file: &NativeFile) -> Neighbours {
    let s = &file.syntax;
    let mut out = HashMap::new();
    for parent in 0..s.tree.nodes.len() as u32 {
        if !is_jsx(file, parent) { continue; }
        let children: Vec<_> = s.all(parent, F::Body).collect();
        for (index, &child) in children.iter().enumerate() {
            if s.is(child, K::JsxExpression) {
                out.insert(child, (index.checked_sub(1).map(|i| children[i]), children.get(index + 1).copied(), children.len()));
            }
        }
    }
    Neighbours(out)
}

pub fn policy(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(parent) = s.parent(node) else { return Value::Null };
    let attribute = s.is(parent, K::JsxAttribute) && s.field(node) == Some(F::Value);
    let child = is_jsx(file, parent) && s.field(node) == Some(F::Body);
    if !attribute && !child { return Value::Null; }
    let expression = s.is(node, K::JsxExpression);
    let inner = s.get(node, F::Expr).map(|id| s.unparen(id));
    let element_value = attribute && (s.is(node, K::JsxElement) || inner.is_some_and(|id| s.is(id, K::JsxElement)));
    let scope = if element_value { "element" } else if attribute { "props" } else { "children" };
    let mut unnecessary = false;
    let mut missing = false;
    if expression {
        if let Some(inner) = inner {
            let raw = s.text(inner);
            let string = s.is(inner, K::String);
            let template = s.is(inner, K::Template) && s.all(inner, F::Expr).next().is_none();
            let value = literal_text(s, inner).unwrap_or_default();
            let comment = file.comments().iter().any(|c| c.start >= s.tree.node(node).start && c.end <= s.tree.node(node).end);
            let spelling = if template { raw.get(1..raw.len().saturating_sub(1)).unwrap_or("") } else { raw };
            let escape = spelling.contains('\\') || without_entities(spelling).len() != spelling.len()
                || child && spelling.contains(['{', '}', '<', '>']);
            if element_value { unnecessary = true; }
            else if !comment && string {
                unnecessary = !escape && !value.contains("/*") && (!attribute || !whitespace(&value))
                    && (attribute || !edge_space(&value)) && (child || !value.contains(['\'', '"']));
            } else if !comment && template {
                unnecessary = !escape && !spelling.contains('\n') && !edge_space(spelling) && !value.contains(['\'', '"']);
            } else if !comment && is_jsx(file, inner) { unnecessary = true; }
            if child {
                let neighbours = file.cached(neighbours);
                let &(prev, next, count) = neighbours.0.get(&node).expect("expression is a JSX body child");
                let adjacent = [prev, next];
                let space_expression = string && whitespace(&value);
                if adjacent.into_iter().flatten().any(|id| s.is(id, K::JsxExpression))
                    || space_expression && (count == 1 || adjacent.into_iter().flatten().any(|id| s.is(id, K::JsxElement))) {
                    unnecessary = false;
                }
            }
        }
    } else if element_value { missing = true; }
    else if s.is(node, K::String) || s.is(node, K::JsxText) {
        let raw = s.text(node);
        let line_break = raw.contains(['\n', '\r', '\u{2028}', '\u{2029}']) && raw.trim_matches(space).is_empty();
        missing = !line_break && !without_entities(raw).trim_matches(space).is_empty();
    } else { return Value::Null; }
    Value::Record(vec![
        ("scope".into(), Value::from(scope)),
        ("expression".into(), Value::Bool(expression)),
        ("unnecessary".into(), Value::Bool(unnecessary)),
        ("missing".into(), Value::Bool(missing)),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn raw_jsx_text_separates_expression_containers() {
        let l = native_linter("rule jsx-curly-brace-presence");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box>\n{'a'}\n{'b'}\n</Box>;").len(), 2);
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box>{'a'}{'b'}</Box>;").is_empty());
    }
}
