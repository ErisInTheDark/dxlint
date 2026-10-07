//! Error-value types and caught-binding provenance.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};

fn error_like(ty: &Ty, ck: &Checker<'_>, depth: usize) -> bool {
    if depth >= 64 { return false }
    if let Some(source) = ck.readonly_source(ty) { return error_like(source, ck, depth + 1) }
    match ty {
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|part| error_like(part, ck, depth + 1)),
        Ty::Inter(parts) => parts.iter().any(|part| error_like(part, ck, depth + 1)),
        _ => ck.is_builtin_type_or_base(ty, "Error"),
    }
}

fn rethrown(file: &NativeFile<'_>, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    if !s.is(node, K::Ident) { return false }
    let Some(decl) = file.decl_of(node) else { return false };
    if file.binding(decl).is_some_and(|binding| binding.kind == "catch") { return true }
    let Some(owner) = s.parent(decl) else { return false };
    if s.is(owner, K::Try) && s.get(owner, F::Param) == Some(decl) { return true }
    if !s.is(owner, K::Param) { return false }
    let Some(arrow) = s.parent(owner).filter(|&arrow| s.is(arrow, K::Arrow) && s.all(arrow, F::Param).next() == Some(owner)) else { return false };
    let Some(call) = s.parent(arrow).filter(|&call| s.is(call, K::Call) && s.field(arrow) == Some(F::Arg)) else { return false };
    super::promises::rejection_handler(file, call).is_some_and(|(handler, _)| handler == arrow)
}

pub(super) fn value(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let ty = file.constrained_type_of(node);
    let kind = match ty {
        Ty::Any => "any",
        Ty::Unknown | Ty::IntrinsicUnknown => "unknown",
        Ty::Undefined => "undefined",
        _ if error_like(&ty, &file.typing().checker, 0) => "error",
        _ => "other",
    };
    Value::record(vec![
        ("kind".into(), Value::from(kind)),
        ("rethrown".into(), Value::Bool(rethrown(file, node))),
    ])
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn readonly_error_values_keep_union_and_intersection_semantics() {
        let linter = native_linter("rule only-throw-error { set allowThrowingUnknown false }");
        let source = "interface Error { message: string; } type Readonly<T> = { readonly [P in keyof T]: T[P] }; declare const good: Readonly<Readonly<Error>>; declare const bad: Readonly<Error | number>; declare const mixed: Readonly<Error & { tag: string }>; throw good; throw bad; throw mixed;";
        let diagnostics = linter.lint(Path::new("/root/lib.fixture.d.ts"), source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(&source[diagnostics[0].start..diagnostics[0].end], "bad");
    }

    #[test]
    fn thrown_values_highlight_the_expression_without_grouping_parentheses() {
        let linter = native_linter("rule only-throw-error");
        let source = "const value = 1; throw ((value)); throw (value + 1);";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["value", "value + 1"]);
    }

    #[test]
    fn rejection_constructors_follow_value_heritage_not_instance_names() {
        let linter = native_linter("rule prefer-promise-reject-errors");
        let source = "interface Promise<T> {} interface PromiseConstructor { new <T>(executor: (resolve: (value: T) => void, reject: (reason: any) => void) => void): Promise<T>; reject(reason?: any): Promise<never>; } declare const Promise: PromiseConstructor; class Derived extends Promise<number> {} class Further extends Derived {} class Other { static reject(reason: any) {} } Derived.reject(1); Further.reject(2); Other.reject(3); new Derived((resolve, reject) => { reject(4); });";
        let diagnostics = linter.lint(Path::new("/root/lib.fixture.d.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["Derived.reject(1)", "Further.reject(2)", "reject(4)"]);
    }

    #[test]
    fn indexed_rejection_reasons_use_the_value_type() {
        let linter = native_linter("rule prefer-promise-reject-errors");
        let source = "interface Error { message: string; } interface PromiseConstructor { reject(reason?: any): unknown; } declare const Promise: PromiseConstructor; declare const errors: Error[]; declare const numbers: number[]; Promise.reject(errors[5]); Promise.reject(numbers[5]);";
        let diagnostics = linter.lint(Path::new("/root/lib.fixture.d.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["Promise.reject(numbers[5])"]);
    }

    #[test]
    fn promise_rejections_follow_executor_bindings_across_nested_functions() {
        let linter = native_linter("rule prefer-promise-reject-errors");
        let source = "Promise.reject(5); new Promise((resolve, reject) => { reject('bad'); function later() { reject(7); } function shadow(reject) { reject(8); } resolve(9); }); function unrelated(reject) { reject(10); }";
        let diagnostics = linter.lint(Path::new("/root/a.js"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["Promise.reject(5)", "reject('bad')", "reject(7)"]);
    }

    #[test]
    fn empty_rejection_option_does_not_allow_explicit_undefined() {
        let linter = native_linter("rule prefer-promise-reject-errors { set allowEmptyReject true }");
        let source = "Promise.reject(); Promise.reject(undefined); new Promise((resolve, reject) => { reject(); reject(undefined); });";
        let diagnostics = linter.lint(Path::new("/root/a.js"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["Promise.reject(undefined)", "reject(undefined)"]);
    }

    #[test]
    fn rethrows_follow_the_caught_binding_not_other_variables_or_handlers() {
        let linter = native_linter("rule only-throw-error { set allowThrowingAny false\nset allowThrowingUnknown false }");
        let code = "interface Like { then(ok: (v: unknown) => void, bad: (e: any) => void): Like; catch(bad: (e: any) => void): Like; } declare const promise: Like; try {} catch (caught) { throw caught; const other = 1; throw other; } promise.catch(caught => { throw caught; }); promise.then(caught => { throw caught; });";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }
}
