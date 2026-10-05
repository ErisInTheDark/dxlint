//! Structural punctuation sites, sharing the parser-aware lexical stream.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::lex;

pub(super) fn statement_semicolon(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let Some(kind) = syntax.kind(node) else { return Value::Null };
    if !matches!(kind, K::Var | K::ExprStmt | K::Return | K::Throw | K::DoWhile | K::Debugger | K::Break | K::Continue | K::Import | K::Export | K::ClassProp | K::Function | K::Method | K::ExportAssign | K::ImportEquals | K::TypeAlias) {
        return Value::Null;
    }
    let parent = syntax.parent(node);
    if kind == K::Var && parent.is_some_and(|id| match syntax.kind(id) {
        Some(K::For) => syntax.get(id, F::Init) == Some(node),
        Some(K::ForIn | K::ForOf) => syntax.get(id, F::Left) == Some(node),
        _ => false,
    }) ||
        matches!(kind, K::Function | K::Method) && syntax.get(node, F::Body).is_some() ||
        kind == K::Export && syntax.get(node, F::Decl).is_some() {
        return Value::Null;
    }
    let span = syntax.tree.node(node);
    let lexed = file.cached(lex);
    let tokens = &lexed.0;
    let Some(at) = tokens.partition_point(|token| token.end <= span.end).checked_sub(1) else { return Value::Null };
    let last = tokens[at];
    let present = last.tok == Tok::P(";");
    let before = if present { at.checked_sub(1).and_then(|i| tokens.get(i)) } else { Some(&last) };
    let next = tokens.get(at + 1);
    let redundant = next.is_none_or(|token| matches!(token.tok, Tok::P("}" | ";")));
    let same_line = before.zip(next).is_some_and(|(left, right)| !syntax.src[left.end as usize..right.start as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']));
    let hazard_before = next.is_some_and(|token| !matches!(token.tok, Tok::P("++" | "--")) && syntax.src[token.start as usize..token.end as usize].starts_with(['-', '[', '(', '/', '+', '`']));
    let class_field = kind == K::ClassProp;
    let class_hazard = class_field && (
        syntax.get(node, F::Init).is_none() && syntax.get(node, F::Key).is_some_and(|key| syntax.is(key, K::Ident) && matches!(syntax.text(key), "get" | "set" | "static") && !(syntax.text(key) == "static" && syntax.all(node, F::Modifier).any(|id| syntax.text(id) == "static"))) ||
        next.is_some_and(|token| matches!(&syntax.src[token.start as usize..token.end as usize], "*" | "in" | "instanceof"))
    );
    let hazard_after = match kind {
        K::DoWhile | K::Break | K::Continue | K::Debugger | K::Import => false,
        K::Export if !syntax.has_word(node, F::Kind, "default") => false,
        K::Return => syntax.get(node, F::Arg).is_some(),
        _ => !before.is_some_and(|token| token.tok == Tok::P("}") && terminal_arrow_block(file, node, token.end)),
    };
    let closing = next.is_some_and(|token| token.tok == Tok::P("}"));
    let one_line_block = closing && parent.is_some_and(|id| {
        let body = syntax.tree.node(id);
        match syntax.kind(id) {
            Some(K::Block) => !syntax.src[body.start as usize..body.end as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']),
            Some(K::StaticBlock) => {
                let at = tokens.partition_point(|token| token.start < body.start);
                tokens.get(at + 1).is_some_and(|open| !syntax.src[body.start as usize..open.start as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']))
            }
            _ => false,
        }
    });
    let one_line_class = class_field && closing && parent.is_some_and(|id| syntax.is(id, K::Class) && super::delimiters::brace_start(file, id).is_some_and(|start| !syntax.src[start as usize..syntax.tree.node(id).end as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}'])));
    Value::record(vec![
        ("present".into(), Value::Bool(present)),
        ("offset".into(), Value::from(if present { last.start } else { last.end } as usize)),
        ("end".into(), Value::from(last.end as usize)),
        ("redundant".into(), Value::Bool(redundant)),
        ("same_line".into(), Value::Bool(same_line)),
        ("hazard_before".into(), Value::Bool(hazard_before)),
        ("hazard_after".into(), Value::Bool(hazard_after)),
        ("class_hazard".into(), Value::Bool(class_hazard)),
        ("class_field".into(), Value::Bool(class_field)),
        ("one_line_block".into(), Value::Bool(one_line_block)),
        ("one_line_class".into(), Value::Bool(one_line_class)),
    ])
}

fn terminal_arrow_block(file: &NativeFile<'_>, mut node: u32, end: u32) -> bool {
    let syntax = &file.syntax;
    loop {
        if syntax.is(node, K::Block) {
            return syntax.parent(node).is_some_and(|parent| syntax.is(parent, K::Arrow));
        }
        let Some(child) = syntax.tree.node(node).children.iter().map(|child| child.node).find(|&child| syntax.tree.node(child).end == end) else { return false };
        node = child;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;

    #[test]
    fn loop_body_variables_still_own_their_semicolon() {
        let lint = native_linter("rule semi");
        let reports = lint.lint(Path::new("/root/file.ts"), "for (;;) var value = 1");
        assert_eq!(reports.len(), 1);
    }

    #[test]
    fn exported_expressions_keep_continuation_guards() {
        let lint = native_linter(r#"
            rule semi {
                set mode "never"
                set beforeStatementContinuationChars "never"
            }
        "#);
        let reports = lint.lint(Path::new("/root/file.ts"), "export default value;\n[0].forEach(visit)");
        assert!(reports.is_empty(), "{reports:?}");
    }

    #[test]
    fn class_field_omission_does_not_strip_method_signatures() {
        let lint = native_linter(r#"
            rule semi { set omitLastInOneLineClassBody true }
        "#);
        let reports = lint.lint(Path::new("/root/file.ts"), "abstract class C { abstract run(): void; }");
        assert!(reports.is_empty(), "{reports:?}");
    }
}
