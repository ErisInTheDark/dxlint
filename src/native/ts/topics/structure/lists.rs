//! Syntax-owned list boundaries; expression interiors remain parser-owned.

use crate::expr::Value;
use crate::native::ts::{comments::Comment, facts::NativeFile, kinds::{Field as F, Kind as K}, lexer::Token, tokens::{lex, Lexed}};

#[derive(Clone, Copy)]
struct Edge<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

#[derive(Clone, Copy)]
struct Pair<'a> {
    before: Edge<'a>,
    after: Edge<'a>,
    spaced: bool,
    broken: bool,
    comments: bool,
}

struct Edges<'a> {
    source: &'a str,
    tokens: &'a Lexed,
    comments: &'a [Comment],
}

impl<'a> Edges<'a> {
    fn code(&self, start: usize, end: usize) -> &[Token] {
        self.tokens.range(start as u32, end as u32)
    }

    fn comments(&self, start: usize, end: usize) -> &[Comment] {
        let from = self.comments.partition_point(|comment| (comment.start as usize) < start);
        let to = self.comments.partition_point(|comment| comment.end as usize <= end);
        &self.comments[from..to.max(from)]
    }

    fn token(&self, token: &Token) -> Edge<'a> {
        Edge { start: token.start as usize, end: token.end as usize, text: &self.source[token.start as usize..token.end as usize] }
    }

    fn comment(&self, comment: &Comment) -> Edge<'a> {
        Edge { start: comment.start as usize, end: comment.end as usize, text: comment.body(self.source) }
    }

    fn first(&self, start: usize, end: usize, comments: bool) -> Option<Edge<'a>> {
        let code = self.code(start, end).first().map(|token| self.token(token));
        let comment = comments.then(|| self.comments(start, end).first().map(|comment| self.comment(comment))).flatten();
        match (code, comment) {
            (Some(code), Some(comment)) => Some(if code.start < comment.start { code } else { comment }),
            (code, comment) => code.or(comment),
        }
    }

    fn last(&self, start: usize, end: usize, comments: bool) -> Option<Edge<'a>> {
        let code = self.code(start, end).last().map(|token| self.token(token));
        let comment = comments.then(|| self.comments(start, end).last().map(|comment| self.comment(comment))).flatten();
        match (code, comment) {
            (Some(code), Some(comment)) => Some(if code.start > comment.start { code } else { comment }),
            (code, comment) => code.or(comment),
        }
    }

    fn pair(&self, before: &Edge<'a>, after: &Edge<'a>) -> Pair<'a> {
        let mut at = before.end;
        let mut spaced = false;
        let comments = self.comments(before.end, after.start);
        for comment in comments {
            spaced |= self.source[at..comment.start as usize].chars().any(char::is_whitespace);
            at = comment.end as usize;
        }
        spaced |= self.source[at..after.start].chars().any(char::is_whitespace);
        Pair {
            before: *before,
            after: *after,
            spaced,
            broken: broken(&self.source[before.end..after.start]),
            comments: !comments.is_empty(),
        }
    }
}

fn broken(text: &str) -> bool {
    text.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

pub(super) fn sites(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut groups = Vec::new();
    let main = match s.kind(node) {
        Some(K::Array) => Some(("ArrayExpression", "[]", F::Element)),
        Some(K::ArrayPattern) => Some(("ArrayPattern", "[]", F::Element)),
        Some(K::Object) => Some(("ObjectExpression", "{}", F::Property)),
        Some(K::ObjectPattern) => Some(("ObjectPattern", "{}", F::Property)),
        Some(K::Function) => Some((if s.parent(node).is_some_and(|parent| matches!(s.kind(parent), Some(K::Program | K::Block | K::Export))) { "FunctionDeclaration" } else { "FunctionExpression" }, "()", F::Param)),
        Some(K::Arrow) => Some(("ArrowFunctionExpression", "()", F::Param)),
        Some(K::Call) => Some(("CallExpression", "()", F::Arg)),
        Some(K::New) => Some(("NewExpression", "()", F::Arg)),
        Some(K::If) => Some(("IfStatement", "()", F::Test)),
        Some(K::Import) => Some(("ImportDeclaration", "{}", F::Specifier)),
        Some(K::Export) if s.get(node, F::Decl).is_none() => Some(("ExportNamedDeclaration", "{}", F::Specifier)),
        Some(K::Enum) => Some(("TSEnumBody", "{}", F::Member)),
        Some(K::FnType) => Some(("TSFunctionType", "()", F::Param)),
        Some(K::Interface) => Some(("TSInterfaceBody", "{}", F::Member)),
        Some(K::TypeLit) => Some(("TSTypeLiteral", "{}", F::Member)),
        Some(K::TupleType) => Some(("TSTupleType", "[]", F::Element)),
        _ => None,
    };
    if let Some((kind, brackets, field)) = main {
        let mut items: Vec<_> = s.all(node, field).collect();
        if s.is(node, K::ObjectPattern) { items.extend(s.all(node, F::Rest)); }
        groups.push((kind, brackets, items));
    }
    for (kind, field) in [("TSTypeParameterDeclaration", F::TypeParam), ("TSTypeParameterInstantiation", F::TypeArg)] {
        let items: Vec<_> = s.all(node, field).collect();
        if !items.is_empty() { groups.push((kind, "<>", items)); }
    }
    if groups.is_empty() { return Value::list(Vec::new()) }
    let tokens = file.cached(lex);
    let edges = Edges { source: s.src, tokens: &tokens, comments: file.comments() };
    let mut out = Vec::new();
    let bounds = s.tree.node(node);
    for (kind, brackets, mut items) in groups {
        items.sort_unstable_by_key(|&item| s.tree.node(item).start);
        let open = &brackets[..1];
        let close = &brackets[1..];
        let first = items.first().map(|&item| s.tree.node(s.unparen(item)).start as usize);
        let last = items.last().map(|&item| s.tree.node(item).end as usize);
        let prefix_end = first.unwrap_or_else(|| s.get(node, F::Body).map_or(bounds.end as usize, |body| s.tree.node(body).start as usize));
        let prefix = edges.code(bounds.start as usize, prefix_end);
        let left = if brackets == "()" && matches!(s.kind(node), Some(K::Call | K::New)) {
            let after = s.all(node, F::TypeArg).last().or_else(|| s.get(node, F::Callee))
                .map_or(bounds.start, |before| s.tree.node(before).end) as usize;
            prefix.iter().find(|token| token.start as usize >= after && edges.token(token).text == open).map(|token| edges.token(token))
        } else if first.is_some() {
            prefix.last().map(|token| edges.token(token)).filter(|token| token.text == open)
        } else {
            prefix.windows(2).find(|pair| edges.token(&pair[0]).text == open && edges.token(&pair[1]).text == close).map(|pair| edges.token(&pair[0]))
        };
        let Some(left) = left else { continue };
        let after_last = edges.code(last.unwrap_or(left.end), bounds.end as usize);
        let Some(right) = after_last.iter().map(|token| edges.token(token)).find(|token| token.text != ",").filter(|token| token.text == close) else { continue };
        let first_non_hole = items.iter().find(|&&item| !s.is(item, K::Hole)).copied();
        let first_item_start = first_non_hole.map_or(right.start, |item| s.tree.node(s.unparen(item)).start as usize);
        let first_item = first_non_hole.and_then(|item| edges.first(s.tree.node(s.unparen(item)).start as usize, s.tree.node(item).end as usize, false));
        let first_edge = edges.first(left.end, first_item_start, true).or(first_item).unwrap_or(right);
        let last_edge = edges.last(last.unwrap_or(left.end), right.start, true).or_else(|| {
            items.last().and_then(|&item| edges.last(s.tree.node(item).start as usize, s.tree.node(item).end as usize, false))
        }).unwrap_or(left);
        let mut spacing = vec![edges.pair(&left, &first_edge)];
        if first_edge.start != right.start { spacing.push(edges.pair(&last_edge, &right)); }
        let mut wrapping = Vec::new();
        if items.is_empty() {
            wrapping.extend_from_slice(&spacing);
        } else {
            let after_open = edges.first(left.end, first_item_start, false).or_else(|| {
                items.first().and_then(|&item| edges.first(s.tree.node(s.unparen(item)).start as usize, s.tree.node(item).end as usize, false))
            }).unwrap_or(right);
            wrapping.push(edges.pair(&left, &after_open));
            let mut previous = left.end;
            let mut previous_edge = left;
            for (index, &item) in items.iter().enumerate() {
                let inner = s.unparen(item);
                let item_node = s.tree.node(inner);
                let Some(first_token) = edges.first(item_node.start as usize, item_node.end as usize, false) else { previous = s.tree.node(item).end as usize; continue };
                if index != 0 || after_open.start != first_token.start {
                    let before = edges.code(previous, first_token.start).iter().rev().map(|token| edges.token(token)).find(|token| token.text != "(").unwrap_or(previous_edge);
                    wrapping.push(edges.pair(&before, &first_token));
                }
                previous = s.tree.node(item).end as usize;
                if let Some(edge) = edges.last(s.tree.node(item).start as usize, previous, false) { previous_edge = edge; }
            }
            let last_plain = edges.last(last.unwrap_or(left.end), right.start, false).or_else(|| {
                items.last().and_then(|&item| edges.last(s.tree.node(item).start as usize, s.tree.node(item).end as usize, false))
            }).unwrap_or(left);
            wrapping.push(edges.pair(&last_plain, &right));
        }
        let common = [
            ("kind".into(), Value::from(kind)),
            ("brackets".into(), Value::from(brackets)),
            ("count".into(), Value::from(items.len())),
            ("single".into(), Value::Bool(!broken(&s.src[left.start..right.end]))),
            ("node_single".into(), Value::Bool(!broken(s.text(node)))),
            ("first_broken".into(), Value::Bool(first.is_some_and(|at| broken(&s.src[left.end..at])))),
            ("empty_comments".into(), Value::Bool(first_edge.start != right.start)),
        ];
        for (mode, pairs) in [("spacing", spacing), (if items.is_empty() { "empty_wrapping" } else { "wrapping" }, wrapping)] {
            for pair in pairs {
                let mut fields = Vec::with_capacity(common.len() + 8);
                fields.extend([
                    ("offset".into(), Value::from(pair.before.end)),
                    ("end".into(), Value::from(pair.after.start)),
                    ("comments".into(), Value::Bool(pair.comments)),
                    ("prev".into(), Value::text_copy(pair.before.text)),
                    ("next".into(), Value::text_copy(pair.after.text)),
                    ("spaced".into(), Value::Bool(pair.spaced)),
                    ("broken".into(), Value::Bool(pair.broken)),
                ]);
                fields.extend(common.iter().cloned());
                fields.push(("mode".into(), Value::from(mode)));
                out.push(Value::record(fields));
            }
        }
    }
    Value::list(out)
}

#[cfg(test)]
mod tests {
    use super::sites;
    use crate::engine::tests::with_native_file;

    #[test]
    fn parser_aware_regex_boundaries_keep_the_literal_whole() {
        let source = r"const a = [/\]/];";
        with_native_file(source, |file, _| {
            let node = (0..file.syntax.tree.nodes.len() as u32).find(|&node| file.syntax.is(node, super::K::Array)).unwrap();
            let regex = file.syntax.get(node, super::F::Element).unwrap();
            let super::Value::List(rows) = sites(file, node) else { panic!("list rows") };
            let opening = rows.iter().find(|row| row.key("mode") == super::Value::from("spacing") && row.key("prev") == super::Value::from("[")).unwrap();
            let closing = rows.iter().find(|row| row.key("mode") == super::Value::from("spacing") && row.key("next") == super::Value::from("]")).unwrap();
            let literal = super::Value::text_copy(file.syntax.text(regex));
            assert_eq!(opening.key("next"), literal);
            assert_eq!(closing.key("prev"), literal);
            assert_eq!(closing.key("offset"), super::Value::from(file.syntax.tree.node(regex).end as usize));
        });
    }

    #[test]
    fn parser_aware_template_boundaries_keep_the_closing_quasi() {
        with_native_file("const a = [`head${f(x)}tail`];", |file, _| {
            let node = (0..file.syntax.tree.nodes.len() as u32).find(|&node| file.syntax.is(node, super::K::Array)).unwrap();
            let template = file.syntax.get(node, super::F::Element).unwrap();
            let tail = file.syntax.all(template, super::F::Quasi).last().unwrap();
            let super::Value::List(rows) = sites(file, node) else { panic!("list rows") };
            let closing = rows.iter().find(|row| row.key("mode") == super::Value::from("spacing") && row.key("next") == super::Value::from("]")).unwrap();
            assert_eq!(closing.key("prev"), super::Value::text_copy(file.syntax.text(tail)));
            assert_eq!(closing.key("offset"), super::Value::from(file.syntax.tree.node(tail).end as usize));
        });
    }

    #[test]
    fn comment_edges_exclude_internal_whitespace_and_keep_line_breaks() {
        for (source, spaced, broken) in [
            ("const a = [/* inner */x/* tail */];", false, false),
            ("const a = [ /* inner */x/* tail */ ];", true, false),
            ("const a = [\r\n/* inner */x/* tail */\r\n];", true, true),
            ("const a = [\u{2028}/* inner */x/* tail */\u{2029}];", true, true),
        ] {
            with_native_file(source, |file, _| {
                let node = (0..file.syntax.tree.nodes.len() as u32).find(|&node| file.syntax.is(node, super::K::Array)).unwrap();
                let super::Value::List(rows) = sites(file, node) else { panic!("list rows") };
                let rows: Vec<_> = rows.iter().filter(|row| row.key("mode") == super::Value::from("spacing")).collect();
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].key("next"), super::Value::from(" inner "));
                assert_eq!(rows[1].key("prev"), super::Value::from(" tail "));
                assert!(rows.iter().all(|row| row.key("spaced") == super::Value::Bool(spaced) && row.key("broken") == super::Value::Bool(broken)));
            });
        }
    }

}
