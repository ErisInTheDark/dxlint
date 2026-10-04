//! Closing-tag measurements and closing-bracket placement.

use super::metadata::{bounds, line, line_starts, space};
use super::super::super::facts::NativeFile;
use super::super::super::kinds::Field as F;
use super::super::super::lexer::Lexer;
use crate::expr::Value;

pub fn closing(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let start = s.tree.node(node).start as usize;
    let (_, close) = bounds(file, node);
    let Some(close) = close else { return Value::Null };
    let lines = file.cached(line_starts);
    let prefix = &s.src[lines.start(close)..close];
    Value::record(vec![
        ("offset".into(), Value::from(close)),
        ("single".into(), Value::Bool(line(&lines, start) == line(&lines, close))),
        ("own_line".into(), Value::Bool(prefix.chars().all(space))),
        ("column".into(), Value::from(lines.column(s.src, close))),
        ("open_column".into(), Value::from(lines.column(s.src, start))),
        ("indent".into(), Value::from(lines.indentation(s.src, start))),
    ])
}

pub fn layout(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let own = s.tree.node(node);
    let Some(name) = s.get(node, F::Name) else { return Value::Null };
    let (open_end, close) = bounds(file, node);
    let lines = file.cached(line_starts);
    let self_closing = close.is_none();
    let last_prop = s.all(node, F::Property).last();
    let mut last_end = s.tree.node(name).end as usize;
    if let Some(n) = s.all(node, F::TypeArg).last() {
        let end = s.tree.node(n).end as usize;
        last_end = s.src[end..open_end].find('>').map_or(end, |at| end + at + 1);
    }
    let left_end = last_prop.map_or(last_end, |p| s.tree.node(p).end as usize);
    let mut tags = vec![(own.start as usize, open_end, name, true, left_end)];
    if let (Some(start), Some(name)) = (close, s.get(node, F::Closing)) {
        let end = s.tree.node(name).end as usize;
        tags.push((start, own.end as usize, name, false, end));
    }
    let records = tags.into_iter().map(|(start, end, name, opening, left_end)| {
        let name_start = s.tree.node(name).start as usize;
        let gt = end - 1;
        let self_close = opening && self_closing;
        let slash = if self_close {
            s.src[left_end..gt].rfind('/').map(|at| left_end + at)
        } else if !opening {
            let mut lx = Lexer::new(&s.src[start..name_start]);
            lx.next_token();
            Some(start + lx.next_token().start as usize)
        } else { None };
        let bracket = if self_close { slash.unwrap_or(gt) } else { gt };
        let last = last_prop.map(|p| s.tree.node(p));
        let trailing_gap = &s.src[left_end..bracket];
        let trailing_comment = trailing_gap.contains("/*") || trailing_gap.contains("//");
        let bracket_column = lines.column(s.src, bracket);
        let open_column = lines.column(s.src, start);
        let indent = lines.indentation(s.src, start);
        let tabs_match = lines.starts_tab(s.src, start) == lines.starts_tab(s.src, bracket);
        let last_column = last.map_or(0, |p| lines.column(s.src, p.start as usize));
        let last_on_close_line = last.is_some_and(|p| line(&lines, p.end as usize) == line(&lines, bracket));
        let locations = ["after-tag", "after-props", "props-aligned", "tag-aligned", "line-aligned"].into_iter().map(|mode| {
            let correct_column = match mode {
                "props-aligned" => Some(last_column),
                "tag-aligned" => Some(open_column),
                "line-aligned" => Some(indent),
                _ => None,
            };
            let correct = match mode {
                "after-tag" => line(&lines, name_start) == line(&lines, bracket),
                "after-props" => last_on_close_line,
                _ => correct_column == Some(bracket_column) && (mode != "tag-aligned" || tabs_match),
            };
            let fallback = !correct && trailing_comment && matches!(mode, "after-tag" | "after-props");
            Value::record(vec![
                ("mode".into(), Value::from(mode)),
                ("placement".into(), Value::from(if fallback { "line-aligned" } else { mode })),
                ("correct".into(), Value::Bool(if fallback { bracket_column == indent } else { correct })),
                ("column".into(), if fallback { Value::from(indent) } else { correct_column.map_or(Value::Null, Value::from) }),
            ])
        }).collect();
        Value::record(vec![
            ("opening".into(), Value::Bool(opening)),
            ("self_closing".into(), Value::Bool(self_close)),
            ("bracket".into(), Value::from(bracket)),
            ("has_props".into(), Value::Bool(last.is_some())),
            ("locations".into(), Value::list(locations)),
            ("last_on_open_line".into(), Value::Bool(last.is_some_and(|p| line(&lines, p.end as usize) == line(&lines, start)))),
            ("last_on_close_line".into(), Value::Bool(last_on_close_line)),
        ])
    }).collect();
    Value::record(vec![
        ("tags".into(), Value::list(records)),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn tag_spacing_uses_tokens_not_multiline_attribute_extents() {
        let l = native_linter("rule jsx-tag-spacing {\nset closingSlash 'allow'\nset beforeSelfClosing 'allow'\nset afterOpening 'allow'\nset beforeClosing 'always'\n}");
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a=\"a\nb\"></Box >;").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a={foo\n}></Box >;").len(), 1);
    }

    #[test]
    fn slash_spacing_checks_keep_independent_decisions_and_element_ranges() {
        let l = native_linter("rule jsx-tag-spacing {\nset closingSlash 'never'\nset beforeSelfClosing 'never'\nset afterOpening 'allow'\nset beforeClosing 'allow'\n}");
        let text = "const x = <Outer><Box / ></Outer>;";
        let slash = text.find("/ >").unwrap();
        let diags = l.lint(Path::new("/root/file.tsx"), text);
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().all(|diag| diag.start == slash && diag.end == slash + "/ >".len()));
    }
}
