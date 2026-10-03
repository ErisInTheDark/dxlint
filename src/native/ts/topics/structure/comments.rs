//! Comment positions and grouped multiline styles.

use crate::expr::Value;
use crate::native::ts::{comments::Comment, facts::NativeFile, kinds::Kind};

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
