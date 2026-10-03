//! Per-line measurements, retaining tab stops and comment boundaries.

use crate::expr::Value;
use crate::native::ts::{facts::NativeFile, kinds::{Field, Kind}};

struct LineLengths(Vec<Value<'static>>);

fn segments(text: &str) -> Value<'static> {
    Value::List(text.split('\t').map(|part| Value::from(part.encode_utf16().count())).collect())
}

fn unicode_extra(text: &str) -> usize {
    text.encode_utf16().count() - text.chars().count()
}

pub(super) fn lengths(file: &NativeFile, node: u32) -> Value<'static> {
    if !file.syntax.is(node, Kind::Program) {
        return Value::Null;
    }
    Value::List(file.cached(scan).0.clone())
}

fn scan(file: &NativeFile) -> LineLengths {
    let s = &file.syntax;
    let containers: Vec<_> = (0..s.tree.nodes.len() as u32)
        .filter(|&node| s.is(node, Kind::JsxExpression) && s.get(node, Field::Expr).is_none()
            && !s.text(node).contains(['\n', '\r', '\u{2028}', '\u{2029}']))
        .map(|node| s.tree.node(node))
        .collect();
    let mut comments = file.comments().to_vec();
    for comment in &mut comments {
        if let Some(container) = containers.iter().find(|container| container.start <= comment.start && comment.end <= container.end) {
            comment.start = container.start;
            comment.end = container.end;
        }
    }
    comments.dedup_by_key(|comment| comment.start);
    let mut literals = Vec::new();
    for id in 0..s.tree.nodes.len() as u32 {
        match s.kind(id) {
            Some(kind @ (Kind::String | Kind::Regex)) => {
                let node = s.tree.node(id);
                literals.push((node.start as usize, node.end as usize, kind));
            }
            Some(Kind::Template | Kind::TemplateType) => {
                for quasi in s.all(id, Field::Quasi) {
                    let node = s.tree.node(quasi);
                    literals.push((node.start as usize, node.end as usize, Kind::Template));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let mut start = 0;
    for line in s.src.split_inclusive('\n') {
        let text = line.trim_end_matches(['\n', '\r']);
        let end = start + text.len();
        let comment = comments.iter().rev().find(|comment| comment.start as usize <= end && comment.end as usize >= start);
        let full_comment = comment.is_some_and(|comment| comment.end as usize >= end
            && (comment.start as usize <= start || s.src[start..comment.start as usize].trim().is_empty()));
        let mut stripped_end = end;
        if !full_comment {
            for comment in comments.iter().rev().filter(|comment| comment.start as usize >= start) {
                if comment.start as usize <= stripped_end && comment.end as usize >= stripped_end {
                    stripped_end = comment.start as usize;
                    stripped_end = start + s.src[start..stripped_end].trim_end().len();
                }
            }
        }
        let stripped = &s.src[start..stripped_end];
        let has = |kind| literals.iter().any(|&(from, to, k)| k == kind && from <= end && to >= start);
        out.push(Value::Record(vec![
            ("offset".into(), Value::from(start)),
            ("text".into(), Value::from(text.to_owned())),
            ("stripped".into(), Value::from(stripped.to_owned())),
            ("segments".into(), segments(text)),
            ("stripped_segments".into(), segments(stripped)),
            ("unicode_extra".into(), Value::from(unicode_extra(text))),
            ("stripped_unicode_extra".into(), Value::from(unicode_extra(stripped))),
            ("comment".into(), Value::Bool(full_comment)),
            ("string".into(), Value::Bool(has(Kind::String))),
            ("template".into(), Value::Bool(has(Kind::Template))),
            ("regex".into(), Value::Bool(has(Kind::Regex))),
        ]));
        start += line.len();
    }
    LineLengths(out)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn length_uses_tab_stops_and_unicode_characters() {
        for (source, width, length) in [("a\t+b;", 4, 7), ("ab\t+c\t+de;", 4, 12), ("'\u{1f600}'\t+x;", 4, 10), ("a\t+b;", 0, 4)] {
            for (limit, expected) in [(length, 0), (length - 1, 1)] {
                let lint = native_linter(&format!("rule max-len {{ set options [{limit}, {width}] }}"));
                assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), expected, "{source}: {limit}");
            }
        }
    }
}
