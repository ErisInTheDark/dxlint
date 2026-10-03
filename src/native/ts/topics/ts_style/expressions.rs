//! Expression statements and directive prologues.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;

pub(super) fn expression_statement(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(expression) = s.get(node, F::Expr) else { return Value::Null };
    let directive = s.is(expression, K::String) && s.parent(node).is_some_and(|parent| {
        let body = s.is(parent, K::Program) || s.is(parent, K::Block)
            && s.parent(parent).is_some_and(|n| matches!(s.kind(n), Some(K::Function | K::Arrow | K::Method | K::Namespace)));
        body && s.all(parent, F::Body).take_while(|&n| n != node).all(|n| {
            s.is(n, K::ExprStmt) && s.get(n, F::Expr).is_some_and(|n| s.is(n, K::String))
        })
    });
    let modes = (0..16).filter(|&mode| !disallowed(s, expression, mode)).map(|n| Value::from(n as usize)).collect();
    Value::Record(vec![
        ("directive".into(), Value::Bool(directive)),
        ("valid_modes".into(), Value::List(modes)),
    ])
}

// Bits correspond to short circuit, ternary, tagged template, and JSX options.
fn disallowed(s: &Syntax, node: u32, mode: u8) -> bool {
    match s.kind(node) {
        Some(K::Paren | K::As | K::TypeAssertion | K::NonNull | K::Instantiation) => s.get(node, F::Expr).is_some_and(|n| disallowed(s, n, mode)),
        Some(K::Logical) => mode & 1 == 0 || s.get(node, F::Right).is_some_and(|n| disallowed(s, n, mode)),
        Some(K::Conditional) => mode & 2 == 0 || [F::Consequent, F::Alternate].iter().any(|&f| s.get(node, f).is_some_and(|n| disallowed(s, n, mode))),
        Some(K::TaggedTemplate) => mode & 4 == 0,
        Some(K::JsxElement | K::JsxFragment) => mode & 8 != 0,
        Some(K::Unary) => !matches!(s.word(node, F::Op), Some("delete" | "void")),
        Some(K::Array | K::Arrow | K::Binary | K::Class | K::Function | K::Ident | K::Member | K::Index | K::MetaProperty | K::Object | K::Sequence | K::Template | K::This | K::Number | K::String | K::Bool | K::BigInt | K::Null | K::Regex) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn directives_are_a_prologue_not_one_statement() {
        let linter = native_linter("rule no-unused-expressions");
        assert!(linter.lint(Path::new("/root/file.ts"), "'one'; 'two'; function f() { 'three'; 'four'; }").is_empty());
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "call(); 'late'; if (x) { 'not a directive'; }").len(), 2);
        assert!(linter.lint(Path::new("/root/file.ts"), "namespace A { 'directive'; }").is_empty());
    }

    #[test]
    fn optional_expression_forms_require_effectful_branches() {
        let linter = native_linter("rule no-unused-expressions { set allowShortCircuit true; set allowTernary true; set allowTaggedTemplates true }");
        assert!(linter.lint(Path::new("/root/file.ts"), "a && call(); a ? one() : two(); tag`value`; call() as unknown;").is_empty());
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "call() && a; a ? one() : two; value as unknown;").len(), 3);
    }

    #[test]
    fn confusing_assertions_include_composite_left_operands() {
        let linter = native_linter("rule no-confusing-non-null-assertion");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "1 + value! === 3; value! in object;").len(), 2);
        assert!(linter.lint(Path::new("/root/file.ts"), "(value!) === 3;").is_empty());
    }
}
