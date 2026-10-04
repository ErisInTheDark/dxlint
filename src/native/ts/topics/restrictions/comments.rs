//! Comment and line metadata; policy stays in the rule definitions.

use super::super::super::facts::NativeFile;
use crate::expr::Value;
use super::super::super::kinds::{Field as F, Kind as K};

struct EmptyJsx(Vec<(u32, u32)>);

fn empty_jsx(file: &NativeFile) -> EmptyJsx {
    let s = &file.syntax;
    let mut spans: Vec<_> = (0..s.tree.nodes.len() as u32).filter(|&n| s.is(n, K::JsxExpression) && s.get(n, F::Expr).is_none())
        .map(|n| (s.tree.node(n).start, s.tree.node(n).end)).collect();
    spans.sort_unstable();
    EmptyJsx(spans)
}

pub fn comments(file: &NativeFile, _: u32) -> Value<'static> {
    let src = file.syntax.src;
    let jsx = file.cached(empty_jsx);
    Value::list(file.comments().iter().enumerate().map(|(i, comment)| {
        let body = comment.body(src);
        let before = &src[..comment.start as usize];
        let after = &src[comment.end as usize..];
        let prefix = before.rsplit('\n').next().unwrap_or("");
        let suffix = after.split('\n').next().unwrap_or("");
        let preamble = !prefix.trim().is_empty();
        let postamble = !suffix.trim().is_empty();
        let prior_jsx = jsx.0.partition_point(|&(start, _)| start <= comment.start);
        let jsx_empty = (!preamble || prefix.trim() == "{") && (!postamble || suffix.trim() == "}")
            && prior_jsx > 0 && comment.end <= jsx.0[prior_jsx - 1].1;
        let consecutive = i > 0 && src[file.comments()[i - 1].end as usize..comment.start as usize].trim().is_empty();
        let clean: String = body.chars().filter(|&c| c != '*').collect();
        let letter = clean.chars().find(|c| !c.is_whitespace());
        Value::record(vec![
            ("offset".into(), Value::from(comment.start as usize)),
            ("text".into(), Value::from(body.to_string())),
            ("clean".into(), Value::from(clean)),
            ("block".into(), Value::Bool(comment.block)),
            ("inline".into(), Value::Bool((preamble || postamble) && !jsx_empty)),
            ("surrounded".into(), Value::Bool(preamble && postamble)),
            ("consecutive".into(), Value::Bool(consecutive)),
            ("lowercase".into(), Value::Bool(letter.is_some_and(char::is_lowercase))),
            ("uppercase".into(), Value::Bool(letter.is_some_and(char::is_uppercase))),
        ])
    }).collect())
}

struct LineMetadata(Vec<(usize, bool, bool)>);

fn line_metadata(file: &NativeFile) -> LineMetadata {
    let src = file.syntax.src;
    let comments = file.comments();
    let mut offset = 0usize;
    let mut rows = Vec::new();
    for text in src.split('\n') {
        let end = offset + text.len();
        let blank = text.trim().is_empty();
        let enclosing_comment = comments.partition_point(|c| c.start as usize <= offset);
        let blank_comment = enclosing_comment > 0 && offset < comments[enclosing_comment - 1].end as usize;
        let comment = (!blank || blank_comment) && text.char_indices().all(|(i, ch)| {
            let at = offset + i;
            let prior = comments.partition_point(|c| c.start as usize <= at);
            ch.is_whitespace() || prior > 0 && at < comments[prior - 1].end as usize
        });
        rows.push((offset, blank, comment));
        offset = end + 1;
    }
    LineMetadata(rows)
}

pub fn lines(file: &NativeFile, node: u32) -> Value<'static> {
    let src = file.syntax.src;
    let own = file.syntax.tree.node(node);
    let start_line = src[..own.start as usize].bytes().filter(|&b| b == b'\n').count();
    let end_line = src[..own.end as usize].bytes().filter(|&b| b == b'\n').count();
    let program = file.syntax.is(node, super::super::super::kinds::Kind::Program);
    let lines = file.cached(line_metadata);
    Value::list(lines.0.iter().enumerate().filter(|(line, (offset, _, _))| {
        *line >= start_line && *line <= end_line && !(program && *offset == src.len() && *line > 0)
    }).map(|(_, &(offset, blank, comment))| {
        Value::record(vec![("offset".into(), Value::from(offset)), ("blank".into(), Value::Bool(blank)), ("comment".into(), Value::Bool(comment))])
    }).collect())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn jsx_comments_are_exempt_only_on_otherwise_empty_lines() {
        let l = native_linter("rule no-inline-comments");
        assert!(l.lint(Path::new("/root/file.tsx"), "const view = <div>\n{/* comment */}\n</div>").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.tsx"), "const view = <div>{/* comment */}</div>").len(), 1);
    }

    #[test]
    fn blank_lines_inside_block_comments_are_comment_lines() {
        let l = native_linter("rule max-lines { set max 1\nset skipComments true\n}");
        assert!(l.lint(Path::new("/root/file.ts"), "run();\n/* comment\n\nend */").is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "run();\n\n/* comment\n\nend */").len(), 1);
    }
}
