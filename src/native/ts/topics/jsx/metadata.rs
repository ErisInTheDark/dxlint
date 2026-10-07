//! JSX's native element owns tag boundaries; layout uses its parsed children.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::lexer::{Lexer, Tok};
use crate::expr::Value;

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
    use crate::engine::tests::native_front;

    #[test]
    fn syntax_spans_exclude_children_and_preserve_tag_punctuation() {
        use crate::grammar::Tree;
        use crate::native::ts::{modules::Project, syntax::Syntax};
        use super::{syntax, F, K, NativeFile};
        let front = native_front();
        for (src, opening, closing) in [
            ("<Box<Type> >text</Box>", "<Box<Type> >", Some("</Box>")),
            ("<Box<Type<Nested>>>text</Box>", "<Box<Type<Nested>>>", Some("</Box>")),
            ("<Box a=\"\\\" b={x} />", "<Box a=\"\\\" b={x} />", None),
            ("<><Child /></>", "<>", Some("</>")),
            ("<></>", "<>", Some("</>")),
            ("<Box>< /Box>", "<Box>", Some("< /Box>")),
        ] {
            let parsed = crate::native::ts::parse(src, true).expect("valid jsx");
            let tree = Tree::from_native(parsed.nodes, parsed.root);
            let project = Project::default();
            let file = NativeFile::new(Syntax::new(&tree, src, &front), Path::new("/root/a.tsx"), &project);
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
