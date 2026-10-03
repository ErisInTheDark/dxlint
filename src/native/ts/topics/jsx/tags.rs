//! Measurements for tag-token spacing and closing-bracket placement.

use super::metadata::{bounds, line, line_starts, LineStarts};
use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::lexer::Lexer;
use crate::expr::Value;

fn column(src: &str, lines: &LineStarts, at: usize) -> usize {
    src[lines.0[line(lines, at) - 1]..at].encode_utf16().count()
}

fn indentation(src: &str, lines: &LineStarts, at: usize) -> usize {
    src[lines.0[line(lines, at) - 1]..].chars().take_while(|ch| ch.is_whitespace() && !matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        .map(char::len_utf16).sum()
}

fn starts_tab(src: &str, lines: &LineStarts, at: usize) -> bool {
    src[lines.0[line(lines, at) - 1]..].starts_with('\t')
}

pub fn closing(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let start = s.tree.node(node).start as usize;
    let (_, close) = bounds(file, node);
    let Some(close) = close else { return Value::Null };
    let lines = file.cached(line_starts);
    let prefix = &s.src[lines.0[line(&lines, close) - 1]..close];
    Value::Record(vec![
        ("offset".into(), Value::from(close)),
        ("single".into(), Value::Bool(line(&lines, start) == line(&lines, close))),
        ("own_line".into(), Value::Bool(prefix.chars().all(char::is_whitespace))),
        ("column".into(), Value::from(column(s.src, &lines, close))),
        ("open_column".into(), Value::from(column(s.src, &lines, start))),
        ("indent".into(), Value::from(indentation(s.src, &lines, start))),
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
    let (left_start, left_end) = last_prop.map_or((last_end - 1, last_end), |p| {
        let prop = s.tree.node(p);
        let value = s.get(p, F::Value);
        let token_start = value.filter(|&v| s.is(v, K::String)).map_or(prop.end as usize - 1, |v| s.tree.node(v).start as usize);
        (token_start, prop.end as usize)
    });
    let mut tags = vec![(own.start as usize, open_end, name, true, left_start, left_end)];
    if let (Some(start), Some(name)) = (close, s.get(node, F::Closing)) {
        let end = s.tree.node(name).end as usize;
        tags.push((start, own.end as usize, name, false, end - 1, end));
    }
    let mut all_parts = Vec::new();
    let records = tags.into_iter().map(|(start, end, name, opening, left_start, left_end)| {
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
        let after_open = if opening { start } else { slash.unwrap_or(start) };
        let mut parts = Vec::new();
        let mut gap = |kind: &'static str, left_start: usize, left_end: usize, right: usize, offset: usize| {
            parts.push(Value::Record(vec![
                ("kind".into(), Value::from(kind)),
                ("offset".into(), Value::from(offset)),
                ("left_end".into(), Value::from(left_end)),
                ("space".into(), Value::Bool(s.src[left_end..right].chars().any(char::is_whitespace))),
                ("newline".into(), Value::Bool(line(&lines, left_end) != line(&lines, right))),
                ("start_newline".into(), Value::Bool(line(&lines, left_start) != line(&lines, right))),
                ("opening".into(), Value::Bool(opening)),
                ("self_closing".into(), Value::Bool(self_close)),
                ("single".into(), Value::Bool(line(&lines, start) == line(&lines, end))),
            ]));
        };
        if let Some(slash) = slash {
            if self_close { gap("closingSlash", slash, slash + 1, gt, slash); }
            else { gap("closingSlash", start, start + 1, slash, start); }
        }
        gap("afterOpening", after_open, after_open + 1, name_start, after_open);
        if self_close { gap("beforeSelfClosing", left_start, left_end, bracket, bracket); }
        else { gap("beforeClosing", left_start, left_end, gt, left_end); }
        all_parts.extend(parts);
        let last = last_prop.map(|p| s.tree.node(p));
        let trailing_gap = &s.src[left_end..bracket];
        let trailing_comment = trailing_gap.contains("/*") || trailing_gap.contains("//");
        let bracket_column = column(s.src, &lines, bracket);
        let open_column = column(s.src, &lines, start);
        let indent = indentation(s.src, &lines, start);
        let tabs_match = starts_tab(s.src, &lines, start) == starts_tab(s.src, &lines, bracket);
        let last_column = last.map_or(0, |p| column(s.src, &lines, p.start as usize));
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
            Value::Record(vec![
                ("mode".into(), Value::from(mode)),
                ("placement".into(), Value::from(if fallback { "line-aligned" } else { mode })),
                ("correct".into(), Value::Bool(if fallback { bracket_column == indent } else { correct })),
                ("column".into(), if fallback { Value::from(indent) } else { correct_column.map_or(Value::Null, Value::from) }),
            ])
        }).collect();
        Value::Record(vec![
            ("opening".into(), Value::Bool(opening)),
            ("self_closing".into(), Value::Bool(self_close)),
            ("bracket".into(), Value::from(bracket)),
            ("has_props".into(), Value::Bool(last.is_some())),
            ("locations".into(), Value::List(locations)),
            ("last_on_open_line".into(), Value::Bool(last.is_some_and(|p| line(&lines, p.end as usize) == line(&lines, start)))),
            ("last_on_close_line".into(), Value::Bool(last_on_close_line)),
        ])
    }).collect();
    Value::Record(vec![
        ("tags".into(), Value::List(records)),
        ("parts".into(), Value::List(all_parts)),
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
}
