//! Sibling spacing and expression-per-line groups use the native JSX body.

use std::collections::BTreeMap;
use super::metadata::{bounds, line, line_starts, space};
use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use crate::expr::Value;

const INLINE: &[&str] = &[
    "a", "abbr", "acronym", "b", "bdo", "big", "button", "cite", "code", "dfn", "em", "i", "img", "input", "kbd",
    "label", "map", "object", "q", "samp", "script", "select", "small", "span", "strong", "sub", "sup", "textarea", "tt", "var",
];

fn descriptor(file: &NativeFile, id: u32) -> String {
    let s = &file.syntax;
    if s.is(id, K::JsxElement) {
        if let Some(n) = s.get(id, F::Name) {
            let name = s.text(n);
            if !name.contains(['.', ':']) { return name.to_string(); }
        }
    }
    s.text(id).replace('\n', "")
}

fn report(file: &NativeFile, id: u32) -> Value<'static> {
    Value::record(vec![
        ("offset".into(), Value::from(file.syntax.tree.node(id).start as usize)),
        ("descriptor".into(), Value::from(descriptor(file, id))),
    ])
}

fn block_comment(file: &NativeFile, id: u32) -> bool {
    file.syntax.text(id).trim_start_matches(space).starts_with("{/*")
}

fn blank_line(text: &str) -> bool {
    let mut after_newline = false;
    for ch in text.chars() {
        if ch == '\n' {
            if after_newline { return true; }
            after_newline = true;
        } else if !space(ch) { after_newline = false; }
    }
    false
}

fn inline_name<'s>(file: &NativeFile<'s>, id: u32) -> Option<&'s str> {
    let s = &file.syntax;
    s.is(id, K::JsxElement).then(|| s.get(id, F::Name)).flatten()
        .map(|n| s.text(n)).filter(|name| INLINE.contains(name))
}

pub fn layout(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let children: Vec<_> = s.all(node, F::Body).collect();
    let lines = file.cached(line_starts);
    let own = s.tree.node(node);
    let (open_end, close) = bounds(file, node);
    let opening_line = line(&lines, open_end);
    let closing_line = close.map_or(opening_line, |at| line(&lines, at));
    let mut groups: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    let mut newlines = Vec::new();
    let mut spacing = Vec::new();
    let is_element = |id| s.is(id, K::JsxElement) || s.is(id, K::JsxExpression)
        && !s.get(id, F::Expr).is_some_and(|expr| s.is(expr, K::Spread));
    let single = |id| {
        let n = s.tree.node(id);
        line(&lines, n.start as usize) == line(&lines, n.end as usize)
    };
    for (index, &child) in children.iter().enumerate() {
        let n = s.tree.node(child);
        let text = s.text(child);
        let is_text = s.is(child, K::JsxText);
        if !is_text || !text.chars().all(space) {
            let leading = is_text && text.find('\n').is_some_and(|at| text[..at].chars().all(space));
            let trailing = is_text && text.rfind('\n').is_some_and(|at| text[at + 1..].chars().all(space));
            let first = line(&lines, n.start as usize) + usize::from(leading);
            let last = line(&lines, n.end as usize) - usize::from(trailing);
            groups.entry(first).or_default().push(child);
            if last != first { groups.entry(last).or_default().push(child); }
        }
        if is_element(child) && !block_comment(file, child) && index + 2 < children.len() && s.is(children[index + 1], K::JsxText) {
            let next = children[index + 2..].iter().copied().find(|&id| is_element(id) && !block_comment(file, id));
            newlines.push(Value::record(vec![
                ("offset".into(), Value::from(s.tree.node(children[index + 2]).start as usize)),
                ("blank".into(), Value::Bool(blank_line(s.text(children[index + 1])))),
                ("multiline".into(), Value::Bool(!single(child) || next.is_some_and(|id| !single(id)))),
            ]));
        }
        if is_text {
            let prev = index.checked_sub(1).map(|i| children[i]);
            let next = children.get(index + 1).copied();
            if (prev.is_some() || next.is_some())
                && prev.is_none_or(|id| inline_name(file, id).is_some())
                && next.is_none_or(|id| inline_name(file, id).is_some()) {
                let after = text.find('\n').is_some_and(|at| text[..at].chars().all(space) && !text[at + 1..].trim_matches(space).is_empty());
                let before = !text.trim_end_matches(space).is_empty() && text[text.trim_end_matches(space).len()..].contains('\n');
                let target = if after && prev.is_some() { prev.map(|id| (id, true)) } else if before { next.map(|id| (id, false)) } else { None };
                if let Some((id, after)) = target {
                    spacing.push(Value::record(vec![
                        ("offset".into(), Value::from(if after { s.tree.node(id).end as usize } else { s.tree.node(id).start as usize })),
                        ("name".into(), Value::from(inline_name(file, id).unwrap_or("").to_string())),
                        ("after".into(), Value::Bool(after)),
                    ]));
                }
            }
        }
    }
    let mut default_reports = BTreeMap::new();
    for (&at, ids) in &groups {
        for (index, &id) in ids.iter().enumerate() {
            if index > 0 || at == opening_line || index + 1 == ids.len() && at == closing_line {
                default_reports.entry(id).or_insert_with(|| report(file, id));
            }
        }
    }
    let reports: Vec<_> = default_reports.into_values().collect();
    let single_line_reports = if groups.len() == 1 {
        let (&at, ids) = groups.first_key_value().expect("one group");
        let mut reports = Vec::new();
        if at == opening_line { reports.push(report(file, ids[0])); }
        if at == closing_line { reports.push(report(file, *ids.last().expect("nonempty group"))); }
        reports
    } else { reports.clone() };
    let first = children.first().copied();
    let last = children.last().copied();
    let single_line_allowed = first.zip(last).is_some_and(|(first, last)| {
        let first_n = s.tree.node(first);
        let last_n = s.tree.node(last);
        let delta = line(&lines, last_n.end as usize) - line(&lines, first_n.start as usize);
        let leading = s.is(first, K::JsxText) && s.text(first).find('\n').is_some_and(|at| s.text(first)[..at].chars().all(space));
        let trailing = s.is(last, K::JsxText) && s.text(last).rfind('\n').is_some_and(|at| s.text(last)[at + 1..].chars().all(space));
        delta == 0 && !leading && !trailing || delta == 2 && leading && trailing
    });
    Value::record(vec![
        ("reports".into(), Value::list(reports)),
        ("single_line_reports".into(), Value::list(single_line_reports)),
        ("single_line_allowed".into(), Value::Bool(single_line_allowed)),
        ("single_child".into(), Value::Bool(children.len() == 1 && single(children[0])
            && line(&lines, own.start as usize) == line(&lines, own.end as usize))),
        ("literal_child".into(), Value::Bool(children.len() == 1 && s.is(children[0], K::JsxText))),
        ("has_jsx".into(), Value::Bool(children.iter().any(|&id| matches!(s.kind(id), Some(K::JsxElement | K::JsxFragment))))),
        ("newlines".into(), Value::list(newlines)),
        ("spacing".into(), Value::list(spacing)),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn spread_children_do_not_enable_multiline_expression_exemptions() {
        let l = native_linter("rule jsx-newline {\nset prevent true\nset allowMultilines true\n}");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box><Item />\n\n{\n...items\n}</Box>;").len(), 1);
    }

    #[test]
    fn a_child_sharing_both_tag_lines_is_reported_once() {
        let l = native_linter("rule jsx-one-expression-per-line");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box><Child>\ntext\n</Child></Box>;").len(), 1);
    }

    #[test]
    fn javascript_whitespace_does_not_form_a_child_expression() {
        let l = native_linter("rule jsx-one-expression-per-line");
        let problems = l.lint(Path::new("/root/file.tsx"), "const x = <Box>\n\u{feff}<Child />\n</Box>;");
        assert!(problems.is_empty(), "{problems:?}");
    }
}
