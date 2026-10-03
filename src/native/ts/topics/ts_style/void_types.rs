//! Syntactic contexts where typescript-eslint permits void types.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;

pub(super) fn context(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(parent) = s.parent(node) else { return Value::from("other") };
    if s.field(node) == Some(F::ReturnType) {
        return Value::from("return");
    }
    if s.field(node) == Some(F::TypeArg) {
        return Value::from(if s.is(parent, K::TypeRef) { "generic" } else if s.is(parent, K::Call) { "other" } else { "generic-other" });
    }
    if s.is(parent, K::TypeParam) && s.field(node) == Some(F::Default) {
        return Value::from("default");
    }
    if s.is(parent, K::Param) && s.word(parent, F::Name) == Some("this") {
        return Value::from("this");
    }
    if s.is(parent, K::Union) {
        let valid_union = s.all(parent, F::Type).all(|n| {
            s.is(n, K::TypeKeyword) && matches!(s.text(n), "void" | "never")
                || s.is(n, K::TypeRef) && s.all(n, F::TypeArg).any(|n| s.is(n, K::TypeKeyword) && s.text(n) == "void")
        });
        if valid_union || in_overload_implementation(s, parent) {
            return Value::from("return");
        }
        return Value::from("union");
    }
    Value::from("other")
}

fn in_overload_implementation(s: &Syntax, node: u32) -> bool {
    let mut at = s.parent(node);
    while let Some(n) = at {
        if matches!(s.kind(n), Some(K::Function | K::Method)) {
            if s.get(n, F::Body).is_none() {
                return false;
            }
            let field = if s.is(n, K::Method) { F::Key } else { F::Name };
            let name = s.word(n, field);
            let parent = s.parent(n).and_then(|p| if s.is(p, K::Export) { s.parent(p) } else { Some(p) });
            return parent.is_some_and(|p| s.kids(p).any(|(_, sibling)| {
                let sibling = if s.is(sibling, K::Export) { s.get(sibling, F::Decl).unwrap_or(sibling) } else { sibling };
                sibling != n && s.kind(sibling) == s.kind(n) && s.word(sibling, field) == name && s.get(sibling, F::Body).is_none()
            }));
        }
        at = s.parent(n);
    }
    false
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn exported_overload_implementations_can_return_void_unions() {
        let linter = native_linter("rule no-invalid-void-type");
        for text in [
            "export function f(): void; export function f(x: string): string; export function f(x?: string): string | void {}",
            "export default function (): void; export default function (x: string): string; export default function (x?: string): string | void {}",
        ] {
            assert!(linter.lint(Path::new("/root/file.ts"), text).is_empty(), "{text}");
        }
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "export function f(): string | void {}").len(), 1);
    }
}
