//! Comment positions and grouped multiline styles.

use crate::expr::Value;
use crate::native::ts::{comments::Comment, facts::NativeFile, kinds::{Field, Kind}, lexer::{Lexer, Tok}};

fn directive(body: &str) -> bool {
    let text = body.trim_start();
    if text.starts_with("eslint") || text.starts_with("jscs") || text.starts_with("/ <reference") || text.starts_with("/ <amd-") {
        return true;
    }
    for prefix in ["@ts-expect-error", "@ts-ignore", "@ts-nocheck", "@ts-check", "prettier-ignore"] {
        if text.strip_prefix(prefix).is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '-')) {
            return true;
        }
    }
    ["jshint", "jslint", "istanbul", "global", "globals", "exported"].into_iter().any(|prefix| {
        text.strip_prefix(prefix).is_some_and(|tail| tail.starts_with(char::is_whitespace))
    }) || ["v8", "c8"].into_iter().any(|prefix| text.strip_prefix(prefix)
        .filter(|tail| tail.starts_with(char::is_whitespace))
        .is_some_and(|tail| boundary(tail.trim_start(), "ignore")))
        || text.strip_prefix("node:coverage").filter(|tail| tail.starts_with(char::is_whitespace)).is_some_and(|tail| {
            let tail = tail.trim_start();
            boundary(tail, "disable") || boundary(tail, "enable") || tail.strip_prefix("ignore")
                .filter(|tail| tail.starts_with(char::is_whitespace)).is_some_and(|tail| boundary(tail.trim_start(), "next"))
        })
        || ["ChunkName", "FetchPriority", "Mode", "Exports", "Include", "Exclude", "Prefetch", "Preload", "Ignore"]
            .into_iter().any(|suffix| text.strip_prefix("webpack").and_then(|tail| tail.strip_prefix(suffix)).is_some_and(|tail| tail.trim_start().starts_with(':')))
}

fn boundary(text: &str, prefix: &str) -> bool {
    text.strip_prefix(prefix).is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '-'))
}

fn line_start(src: &str, offset: usize) -> usize {
    src[..offset].rfind(['\n', '\r', '\u{2028}', '\u{2029}']).map_or(0, |at| at + src[at..].chars().next().expect("matched line terminator").len_utf8())
}

fn beside(src: &str, comment: &Comment) -> bool {
    !src[line_start(src, comment.start as usize)..comment.start as usize].trim().is_empty()
}

struct CommentLayout(Vec<Value<'static>>);

pub(super) fn layout(file: &NativeFile, node: u32) -> Value<'static> {
    if !file.syntax.is(node, Kind::Program) {
        return Value::Null;
    }
    let layout = file.cached(|file| {
        let src = file.syntax.src;
        CommentLayout(file.comments().iter().filter(|comment| !comment.block).map(|comment| {
            let body = comment.body(src);
            Value::Record(vec![
                ("offset".into(), Value::from(comment.start as usize)),
                ("body".into(), Value::from(body.to_owned())),
                ("beside".into(), Value::Bool(beside(src, comment))),
                ("directive".into(), Value::Bool(directive(body))),
            ])
        }).collect())
    });
    Value::List(layout.0.clone())
}

struct CommentStyles(Vec<Value<'static>>);

struct CommentGaps(Vec<Value<'static>>);

pub(super) fn gaps(file: &NativeFile, node: u32) -> Value<'static> {
    if !file.syntax.is(node, Kind::Program) {
        return Value::Null;
    }
    Value::List(file.cached(scan_gaps).0.clone())
}

fn scan_gaps(file: &NativeFile) -> CommentGaps {
    let s = &file.syntax;
    let src = s.src;
    let mut starts = vec![0];
    for (at, ch) in src.char_indices() {
        if ch == '\n' && at > 0 && src.as_bytes()[at - 1] == b'\r' {
            *starts.last_mut().expect("initial line exists") = at + 1;
        } else if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            starts.push(at + ch.len_utf8());
        }
    }
    let line = |at: usize| starts.partition_point(|&start| start <= at) - 1;
    let mut comments = file.comments().to_vec();
    if src.starts_with("#!") {
        comments.insert(0, Comment { start: 0, end: src.find(['\n', '\r', '\u{2028}', '\u{2029}']).unwrap_or(src.len()) as u32, block: false });
    }
    let code_between = |from: usize, to: usize| {
        let mut cursor = from;
        for comment in &comments {
            let start = (comment.start as usize).max(from);
            let end = (comment.end as usize).min(to);
            if start >= end { continue }
            if !src[cursor..start].trim().is_empty() { return true }
            cursor = end;
        }
        !src[cursor..to].trim().is_empty()
    };
    let blocked = |row: usize| src[starts[row]..starts.get(row + 1).copied().unwrap_or(src.len())].trim().is_empty()
        || comments.iter().any(|comment| line(comment.start as usize) == row || line(comment.end as usize) == row);
    CommentGaps(comments.iter().enumerate().flat_map(|(index, comment)| {
        let first = line(comment.start as usize);
        let last = line(comment.end as usize);
        let inline = code_between(starts[first], comment.start as usize)
            || code_between(comment.end as usize, starts.get(last + 1).copied().unwrap_or(src.len()));
        let parent = (0..s.tree.nodes.len() as u32).filter(|&id| {
            let node = s.tree.node(id);
            node.start <= comment.start && comment.end <= node.end
        }).min_by_key(|&id| s.tree.node(id).end - s.tree.node(id).start);
        let mut targets = Vec::new();
        let mut parent_start = None;
        let mut parent_end = None;
        if let Some(parent) = parent {
            let node = s.tree.node(parent);
            let kind = s.kind(parent);
            match kind {
                Some(Kind::Class) => targets.extend(["Block", "Class"]),
                Some(Kind::Block) if s.parent(parent).is_some_and(|p| s.is(p, Kind::Namespace)) => targets.push("Module"),
                Some(Kind::Block | Kind::StaticBlock | Kind::Switch | Kind::Case) => targets.push("Block"),
                Some(Kind::Object | Kind::ObjectPattern) => targets.push("Object"),
                Some(Kind::Array | Kind::ArrayPattern) => targets.push("Array"),
                Some(Kind::Interface) => targets.push("Interface"),
                Some(Kind::TypeLit) => targets.push("Type"),
                Some(Kind::Enum) => targets.push("Enum"),
                _ => {}
            }
            let brace_start = if matches!(kind, Some(Kind::Class | Kind::Interface | Kind::Enum | Kind::StaticBlock | Kind::Switch)) {
                let from = if kind == Some(Kind::Switch) {
                    s.get(parent, Field::Discriminant).map_or(node.start, |id| s.tree.node(id).end)
                } else { node.start };
                let mut lexer = Lexer::new(&src[from as usize..node.end as usize]);
                loop {
                    let token = lexer.next_token();
                    if token.tok == Tok::P("{") { break Some(from as usize + token.start as usize) }
                    if token.tok == Tok::Eof { break None }
                }
            } else { Some(node.start as usize) };
            if let Some(start) = brace_start.filter(|&start| start <= comment.start as usize) {
                parent_start = Some(line(start));
                parent_end = Some(line(node.end as usize));
            } else { targets.clear() }
        }
        let boundaries = |at_boundary: bool| Value::List(if at_boundary {
            targets.iter().map(|target| Value::from(*target)).collect()
        } else { Vec::new() });
        [
            ("before", !inline && first > 0 && !blocked(first - 1)
                && (index == 0 || line(comments[index - 1].end as usize) != first), parent_start.is_some_and(|row| row + 1 == first)),
            ("after", !inline && last + 1 < starts.len() && !blocked(last + 1)
                && comments.get(index + 1).is_none_or(|next| line(next.start as usize) != last), parent_end.is_some_and(|row| row == last + 1)),
        ].into_iter().filter(|&(_, missing, _)| missing).map(|(boundary, _, at_boundary)| Value::Record(vec![
            ("offset".into(), Value::from(comment.start as usize)),
            ("body".into(), Value::from(comment.body(src).to_owned())),
            ("block".into(), Value::Bool(comment.block)),
            ("hashbang".into(), Value::Bool(comment.start == 0 && src.starts_with("#!"))),
            ("directive".into(), Value::Bool(directive(comment.body(src)))),
            ("boundary".into(), Value::from(boundary)),
            ("targets".into(), boundaries(at_boundary)),
        ])).collect::<Vec<_>>()
    }).collect())
}

fn finding(mode: &'static str, offset: usize, message: &'static str, jsdoc: bool, exclamation: bool) -> Value<'static> {
    Value::Record(vec![
        ("mode".into(), Value::from(mode)),
        ("offset".into(), Value::from(offset)),
        ("message".into(), Value::from(message)),
        ("jsdoc".into(), Value::Bool(jsdoc)),
        ("exclamation".into(), Value::Bool(exclamation)),
    ])
}

pub(super) fn styles(file: &NativeFile, node: u32) -> Value<'static> {
    if !file.syntax.is(node, Kind::Program) {
        return Value::Null;
    }
    Value::List(file.cached(scan_styles).0.clone())
}

fn scan_styles(file: &NativeFile) -> CommentStyles {
    let src = file.syntax.src;
    let mut groups: Vec<Vec<&Comment>> = Vec::new();
    for comment in file.comments().iter().filter(|comment| !directive(comment.body(src)) && !beside(src, comment)) {
        let previous = groups.last().and_then(|group| group.last()).copied();
        let joins = !comment.block && previous.is_some_and(|previous| {
            !previous.block && {
                let gap = &src[previous.end as usize..comment.start as usize];
                gap.trim().is_empty() && gap.chars().filter(|&ch| ch == '\n').count() == 1
            }
        });
        if joins {
            groups.last_mut().expect("joining an existing comment group").push(comment);
        } else {
            groups.push(vec![comment]);
        }
    }
    let mut findings = Vec::new();
    for group in groups {
        let first = group[0];
        if !first.block {
            if group.len() > 1 && !group.iter().any(|comment| comment.body(src).contains("*/")) {
                for mode in ["starred-block", "bare-block"] {
                    findings.push(finding(mode, first.start as usize, "Expected a block comment instead of consecutive line comments.", false, false));
                }
            }
            continue;
        }
        let lines: Vec<_> = first.body(src).split('\n').map(|line| line.trim_end_matches('\r')).collect();
        if lines.len() == 1 {
            continue;
        }
        let middle = &lines[1..lines.len() - 1];
        let last_line = lines[lines.len() - 1];
        let starred = lines[0].trim().is_empty() && last_line.trim().is_empty()
            && middle.iter().all(|line| line.trim_start().starts_with('*'));
        let padded = middle.iter().all(|line| line.starts_with(char::is_whitespace) && line.contains(' '));
        let jsdoc = lines[0].strip_prefix('*').is_some_and(|tail| tail.trim().is_empty()) && padded && last_line.trim().is_empty();
        let exclamation = lines[0].strip_prefix('!').is_some_and(|tail| tail.trim().is_empty()) && padded && last_line.trim().is_empty();
        if starred && !jsdoc && !exclamation {
            findings.push(finding("bare-block", first.start as usize, "Expected a block comment without padding stars.", false, false));
        }
        let after = &src[first.end as usize..];
        let line_tail = after.split(['\n', '\r', '\u{2028}', '\u{2029}']).next().unwrap_or("");
        if line_tail.trim().is_empty() {
            findings.push(finding("separate-lines", first.start as usize, "Expected multiple line comments instead of a block comment.", jsdoc, exclamation));
        }
        let start_content = lines[0].strip_prefix(['*', '!']).unwrap_or(lines[0]);
        if !start_content.trim().is_empty() {
            findings.push(finding("starred-block", first.start as usize, "Expected a linebreak after '/*'.", false, false));
        }
        if !last_line.trim().is_empty() {
            findings.push(finding("starred-block", first.end as usize - 2, "Expected a linebreak before '*/'.", false, false));
        }
        let prefix = format!("{} *", &src[line_start(src, first.start as usize)..first.start as usize]);
        let first_line_end = src[first.start as usize..].find('\n').expect("comment has multiple lines") + first.start as usize;
        let mut at = first_line_end + 1;
        while at < first.end as usize {
            let end = src[at..].find('\n').map_or(src.len(), |end| at + end);
            let line = &src[at..end];
            if !line.starts_with(&prefix) {
                let message = if line.trim_start().starts_with('*') {
                    "Expected this line to be aligned with the start of the comment."
                } else {
                    "Expected a '*' at the start of this line."
                };
                findings.push(finding("starred-block", at, message, false, false));
            }
            at = end + 1;
        }
    }
    CommentStyles(findings)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn adjacent_comments_share_outer_blank_line_requirements() {
        let lint = native_linter("rule lines-around-comment {\n set beforeBlockComment true\n set afterBlockComment true\n}");
        let source = "before();\n/* first */ /* second\n continued */\nafter();";
        let reports = lint.lint(Path::new("/root/a.ts"), source);
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].start, source.find("/* first").expect("first comment"));
        assert_eq!(reports[1].start, source.find("/* second").expect("second comment"));
    }
}
