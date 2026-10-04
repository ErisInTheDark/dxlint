//! Operator precedence and constructor argument shape.

use crate::native::ts::{kinds::{Field as F, Kind as K}, syntax::Syntax};

pub(super) fn new_args(s: &Syntax, node: u32) -> bool {
    s.get(node, F::Callee).is_some_and(|callee| s.tree.node(callee).end < s.tree.node(node).end)
}

pub(super) fn precedence(s: &Syntax, node: u32) -> u8 {
    match s.kind(node) {
        Some(K::Sequence) => 0,
        Some(K::Assign | K::Arrow) => 1,
        Some(K::Yield) => 2,
        Some(K::Conditional | K::ConditionalType | K::FnType) => 3,
        Some(K::Union) => 4,
        Some(K::Intersection) => 5,
        Some(K::Binary | K::Logical) => match s.word(node, F::Op).unwrap_or("") {
            "||" | "??" => 4, "&&" => 5, "|" => 6, "^" => 7, "&" => 8,
            "==" | "!=" | "===" | "!==" => 9,
            "<" | ">" | "<=" | ">=" | "in" | "instanceof" => 10,
            "<<" | ">>" | ">>>" => 11, "+" | "-" => 12,
            "*" | "/" | "%" => 13, "**" => 14, _ => 21,
        },
        Some(K::Unary | K::As | K::Satisfies | K::TypeAssertion | K::TypeOperator) => 15,
        Some(K::Await) => 16,
        Some(K::Update) => 17,
        Some(K::New) if !new_args(s, node) => 18,
        Some(K::Call | K::New | K::TaggedTemplate | K::ImportCall) => 19,
        Some(K::Member | K::Index | K::NonNull | K::ArrayType) => 20,
        _ => 21,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn initializer_linebreaks_respect_unicode_code_positions() {
        let lint = native_linter("rule operator-linebreak { set mode \"before\" }");
        for source in ["type \u{03b1} =\n  | A\n  | B;", "type A = \u{03b2};"] {
            let reports = lint.lint(Path::new("/root/a.ts"), source);
            assert!(reports.is_empty(), "{source}: {reports:?}");
        }
    }
}
