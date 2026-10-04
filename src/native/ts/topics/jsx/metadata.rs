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

pub fn syntax(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !matches!(s.kind(node), Some(K::JsxElement | K::JsxFragment)) { return Value::Null; }
    let own = s.tree.node(node);
    let (open_end, close) = bounds(file, node);
    let span = |start: usize, end: usize| Value::record(vec![
        ("start".into(), Value::from(start)),
        ("end".into(), Value::from(end)),
    ]);
    Value::record(vec![
        ("opening".into(), span(own.start as usize, open_end)),
        ("closing".into(), close.map_or(Value::Null, |start| span(start, own.end as usize))),
        ("self_closing".into(), Value::Bool(close.is_none())),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn syntax_spans_exclude_children_and_preserve_tag_punctuation() {
        use crate::grammar::Tree;
        use crate::native::ts::{modules::Project, syntax::Syntax};
        use super::{syntax, F, K, NativeFile};
        let linter = native_linter("");
        let front = linter.grammar.natives.first().expect("native ts");
        for (src, opening, closing) in [
            ("<Box<Type> >text</Box>", "<Box<Type> >", Some("</Box>")),
            ("<Box<Type<Nested>>>text</Box>", "<Box<Type<Nested>>>", Some("</Box>")),
            ("<Box a=\"\\\" b={x} />", "<Box a=\"\\\" b={x} />", None),
            ("<><Child /></>", "<>", Some("</>")),
            ("<></>", "<>", Some("</>")),
            ("<Box>< /Box>", "<Box>", Some("< /Box>")),
        ] {
            let parsed = crate::native::ts::parse(src, true).expect("valid jsx");
            let tree = Tree::from_native(front.remap(parsed.nodes), parsed.root);
            let project = Project::default();
            let file = NativeFile::new(Syntax::new(&tree, src, front), Path::new("/root/a.tsx"), &project);
            let element = file.syntax.all(tree.root, F::Body).next()
                .and_then(|statement| file.syntax.get(statement, F::Expr)).expect("expression");
            assert!(matches!(file.syntax.kind(element), Some(K::JsxElement | K::JsxFragment)));
            let result = syntax(&file, element);
            let text = |span: crate::expr::Value| {
                &src[span.key("start").num().unwrap() as usize..span.key("end").num().unwrap() as usize]
            };
            assert_eq!(text(result.key("opening")), opening);
            if let Some(closing) = closing {
                assert_eq!(text(result.key("closing")), closing);
            } else {
                assert_eq!(result.key("closing"), crate::expr::Value::Null);
            }
            assert_eq!(result.key("self_closing"), crate::expr::Value::Bool(closing.is_none()));
        }
    }

}
