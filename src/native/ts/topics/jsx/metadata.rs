//! JSX's native element owns tag boundaries; layout uses its parsed children.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::lexer::{Lexer, Tok};
use crate::expr::Value;

pub(super) struct LineStarts(Vec<usize>);

// JavaScript whitespace includes BOM, not Unicode NEL.
pub(super) fn space(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

impl LineStarts {
    pub(super) fn start(&self, at: usize) -> usize {
        self.0[line(self, at) - 1]
    }

    pub(super) fn column(&self, src: &str, at: usize) -> usize {
        src[self.start(at)..at].encode_utf16().count()
    }

    pub(super) fn indentation(&self, src: &str, at: usize) -> usize {
        src[self.start(at)..].chars().take_while(|&ch| space(ch) && !matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
            .map(char::len_utf16).sum()
    }

    pub(super) fn starts_tab(&self, src: &str, at: usize) -> bool {
        src[self.start(at)..].starts_with('\t')
    }
}

pub(super) fn line_starts(file: &NativeFile) -> LineStarts {
    let src = file.syntax.src;
    let mut starts = vec![0];
    for (at, ch) in src.char_indices() {
        if ch == '\n' || matches!(ch, '\u{2028}' | '\u{2029}') || ch == '\r' && !src[at + 1..].starts_with('\n') {
            starts.push(at + ch.len_utf8());
        }
    }
    LineStarts(starts)
}

pub(super) fn line(starts: &LineStarts, offset: usize) -> usize {
    starts.0.partition_point(|&start| start <= offset)
}

fn canonical_name(text: &str) -> String {
    text.chars().filter(|&ch| !space(ch)).collect()
}

fn pascal(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(char::is_uppercase)
        && chars.clone().all(|ch| ch.is_lowercase() || ch.is_uppercase() || ch.is_ascii_digit())
        && chars.any(|ch| ch.is_lowercase() || ch.is_ascii_digit())
}

fn all_caps(name: &str) -> bool {
    let valid_end = |ch: char| ch.is_uppercase() || ch.is_ascii_digit();
    name.chars().next().is_some_and(valid_end) && name.chars().last().is_some_and(valid_end)
        && name.chars().all(|ch| valid_end(ch) || ch == '_')
}

pub(super) fn bounds(file: &NativeFile, node: u32) -> (usize, Option<usize>) {
    let s = &file.syntax;
    let own = s.tree.node(node);
    let closing_end = s.get(node, F::Closing).map(|n| s.tree.node(n).start as usize)
        .or_else(|| s.is(node, K::JsxFragment).then_some(own.end as usize));
    let close = closing_end.and_then(|end| {
        let previous = s.all(node, F::Body).last().or_else(|| s.all(node, F::Property).last())
            .or_else(|| s.all(node, F::TypeArg).last()).or_else(|| s.get(node, F::Name));
        let from = previous.map_or(own.start as usize, |n| s.tree.node(n).end as usize);
        let mut lexer = Lexer::new(&s.src[from..end]);
        let mut closing = None;
        loop {
            let token = lexer.next_token();
            if token.tok == Tok::Eof { break; }
            if token.tok == Tok::P("<") { closing = Some(from + token.start as usize); }
        }
        closing
    });
    let open_end = s.all(node, F::Body).next().map_or_else(
        || close.unwrap_or(own.end as usize), |n| s.tree.node(n).start as usize);
    (open_end, close)
}

pub fn tag(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let own = s.tree.node(node);
    let Some(name_node) = s.get(node, F::Name) else { return Value::Null };
    let name = canonical_name(s.text(name_node));
    let dom = name.starts_with(|ch: char| ch.is_ascii_lowercase());
    let (open_end, close) = bounds(file, node);
    let children: Vec<_> = s.all(node, F::Body).collect();
    let empty = children.is_empty() || children.len() == 1 && s.is(children[0], K::JsxText)
        && s.text(children[0]).contains('\n') && s.text(children[0]).chars().all(|ch| ch != '\u{a0}' && space(ch));
    let lines = file.cached(line_starts);
    let start_line = line(&lines, own.start as usize);
    let single = start_line == line(&lines, open_end);
    let mut previous_end = s.all(node, F::TypeArg).last().map_or(s.tree.node(name_node).end as usize, |n| {
        let end = s.tree.node(n).end as usize;
        s.src[end..open_end].find('>').map_or(end, |at| end + at + 1)
    });
    let mut run = 0usize;
    let mut group_index = 0usize;
    let attributes = s.all(node, F::Property).enumerate().map(|(i, n)| {
        let attr = s.tree.node(n);
        let attr_line = line(&lines, attr.start as usize);
        let same = line(&lines, previous_end) == attr_line;
        if !same { run = 0; group_index = 0; }
        if i > 0 && same { group_index += 1; }
        run += 1;
        previous_end = attr.end as usize;
        let prop = s.get(n, F::Name).map(|n| canonical_name(s.text(n)))
            .or_else(|| s.get(n, F::Arg).map(|n| s.text(s.unparen(n)).to_string())).unwrap_or_default();
        Value::record(vec![
            ("name".into(), Value::from(prop)),
            ("offset".into(), Value::from(attr.start as usize)),
            ("same".into(), Value::Bool(same)),
            ("head_line".into(), Value::Bool(attr_line == start_line)),
            ("first".into(), Value::Bool(i == 0)),
            ("run".into(), Value::from(run)),
            ("group_index".into(), Value::from(group_index)),
        ])
    }).collect();
    let names = name.split(['.', ':']).map(|name| {
        let unprefixed = name.strip_prefix('_').unwrap_or(name);
        Value::record(vec![
            ("name".into(), Value::from(name.to_string())),
            ("length".into(), Value::from(name.encode_utf16().count())),
            ("pascal".into(), Value::Bool(pascal(name))),
            ("caps".into(), Value::Bool(all_caps(name))),
            ("underscore_pascal".into(), Value::Bool(pascal(unprefixed))),
            ("underscore_caps".into(), Value::Bool(all_caps(unprefixed))),
        ])
    }).collect();
    Value::record(vec![
        ("name".into(), Value::from(name.clone())),
        ("names".into(), Value::list(names)),
        ("dom".into(), Value::Bool(dom)),
        ("component".into(), Value::Bool(!dom && !name.contains(':'))),
        ("self_closing".into(), Value::Bool(close.is_none())),
        ("empty".into(), Value::Bool(empty)),
        ("open_end".into(), Value::from(open_end)),
        ("closing".into(), close.map_or(Value::Null, Value::from)),
        ("single".into(), Value::Bool(single)),
        ("attributes".into(), Value::list(attributes)),
    ])
}

fn contains_quote(text: &str, quote: char) -> bool {
    if text.contains(quote) { return true; }
    text.split('&').skip(1).filter_map(|part| part.split_once(';').map(|(entity, _)| entity)).any(|entity| {
        let value = match entity {
            "quot" => Some(34),
            "apos" => Some(39),
            _ => entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")).and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse::<u32>().ok())),
        };
        value == Some(quote as u32)
    })
}

pub fn attribute(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(name) = s.get(node, F::Name) else { return Value::Null };
    let Some(value) = s.get(node, F::Value) else { return Value::Null };
    let start = s.tree.node(name).end as usize;
    let end = s.tree.node(value).start as usize;
    let mut lexer = Lexer::new(&s.src[start..end]);
    let token = lexer.next_token();
    if token.tok != Tok::P("=") { return Value::Null; }
    let equal = start + token.start as usize;
    let text = s.text(value);
    let string = s.is(value, K::String);
    let body = if string { &text[1..text.len() - 1] } else { "" };
    Value::record(vec![
        ("offset".into(), Value::from(equal)),
        ("before".into(), Value::Bool(s.src[start..equal].chars().any(space))),
        ("after".into(), Value::Bool(s.src[equal + 1..end].chars().any(space))),
        ("string".into(), Value::Bool(string)),
        ("boolean".into(), Value::Bool(s.is(value, K::JsxExpression) && s.get(value, F::Expr).is_some_and(|n| {
            let n = s.unparen(n);
            s.is(n, K::Bool) && s.text(n) == "true"
        }))),
        ("value_offset".into(), Value::from(end)),
        ("quote".into(), Value::from(text.chars().next().unwrap_or_default().to_string())),
        ("double".into(), Value::Bool(contains_quote(body, '"'))),
        ("single".into(), Value::Bool(contains_quote(body, '\''))),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn prop_limits_use_opening_tag_and_consecutive_attribute_lines() {
        let l = native_linter("rule jsx-max-props-per-line { set maximum {single: 2, multi: 1} }");
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a b><Child\n /></Box>;").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a={(\nfoo\n)} b c />;").len(), 1);
    }

    #[test]
    fn jsx_quotes_decode_entities_but_ignore_expression_strings() {
        let l = native_linter("rule jsx-quotes { set mode 'prefer-single' }");
        assert!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a=\"&#39;\" b={\"text\"} />;").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box a=\"text\" />;").len(), 1);
    }

    #[test]
    fn spaced_closing_slash_still_has_an_explicit_closing_tag() {
        let l = native_linter("rule jsx-self-closing-comp");
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const x = <Box>< /Box>;").len(), 1);
    }
}
