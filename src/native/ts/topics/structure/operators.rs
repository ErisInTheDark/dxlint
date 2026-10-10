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
        Some(K::As | K::Satisfies) => 10,
        Some(K::Unary | K::TypeAssertion | K::TypeOperator) => 15,
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
    use super::precedence;
    use crate::engine::tests::with_native_file;

    #[test]
    fn assertions_bind_like_their_expression_operators() {
        with_native_file(
            "const a = /*@*/x as T; const b = /*@*/x satisfies T; \
             const c = /*@*/x < y; const d = /*@*/x + y; \
             const e = /*@*/!x; const f = /*@*/<T>x;",
            |file, nodes| {
                let ranks: Vec<_> = nodes.iter().map(|&node| precedence(&file.syntax, node)).collect();
                assert_eq!(ranks[0], ranks[2]);
                assert_eq!(ranks[1], ranks[2]);
                assert!(ranks[0] < ranks[3]);
                assert!(ranks[0] < ranks[4]);
                assert_eq!(ranks[5], ranks[4]);
            },
        );
    }
}
