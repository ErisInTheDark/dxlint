use super::*;
use crate::engine::tests::with_native_file;

#[test]
fn immutable_binding_guards_cross_callback_frames() {
    with_native_file(r#"
        declare const value: string | null;
        if (value) {
            const arrow = () => { const unrelated = "value = null"; return /*@*/value; };
            function ordinary() { return /*@*/value; }
            const nested = () => () => /*@*/value;
            const deferred = async () => { await 0; return /*@*/value; };
            const object = { method() { return /*@*/value; } };
        }
        function create() {
            const value: string | null = nullable();
            if (!value) return;
            return () => /*@*/value;
        }
        declare function nullable(): string | null;
        function isString(value: string | null): value is string { return value !== null; }
        if (isString(value)) { void /*@*/value; const callback = () => /*@*/value; }
    "#, |file, marks| {
        let guarded = file.type_of(marks[6]);
        let captured = file.type_of(marks[7]);
        assert!(captured.same_identity(&guarded),
            "callback changed its enclosing predicate refinement: {guarded:?} -> {captured:?}");
        for &mark in &marks[..6] {
            assert!(file.type_of(mark).same_identity(&Ty::Prim("string")),
                "immutable guarded value lost its narrowing: {:?}", file.type_of(mark));
        }
    });
}

#[test]
fn captured_guard_proofs_reject_writes_mutable_paths_and_different_bindings() {
    with_native_file(r#"
        declare const source: string | null;
        let mutable: string | null = source;
        if (mutable) { const callback = () => /*@*/mutable; }
        const written: string | null = source;
        if (written) { const callback = () => /*@*/written; }
        written = null;
        declare const value: string | null;
        if (value) {
            const value: string | null = source;
            const callback = () => /*@*/value;
            void /*@*/value;
        }
        if (value) { const callback = (value: string | null) => /*@*/value; }
        function isString(value: string | null): value is string { return value !== null; }
        if (isString(value)) {
            const value: string | null = source;
            const callback = () => /*@*/value;
            void /*@*/value;
        }
        const object = {value: source};
        if (object.value) { const callback = () => /*@*/object.value; }
    "#, |file, marks| {
        let nullable = Ty::union(vec![Ty::Prim("string"), Ty::Null]);
        for (index, &mark) in marks.iter().enumerate() {
            assert!(file.type_of(mark).same_identity(&nullable),
                "unproven capture or different binding borrowed guard at mark {index}: {:?}", file.type_of(mark));
        }
    });
}

#[test]
fn known_union_arms_narrow_without_resolving_opaque_members() {
    with_native_file("", |file, _| {
        let ck = &file.typing().checker;
        let opaque = [
            Ty::Param("T".into()), Ty::Unknown, Ty::IntrinsicUnknown,
            Ty::Inter(vec![Ty::Param("T".into()), Ty::Prim("object")]),
        ];
        for member in opaque {
            let nullable = Ty::Union(vec![member.clone(), Ty::Null]);
            let known_non_null = Ty::union(vec![member.clone()]);
            let positive = truthy(nullable.clone(), true, ck);
            let negative = truthy(nullable.clone(), false, ck);
            let compared = file.keep(nullable, |part| matches!(part, Ty::Null), false);
            if known_non_null.is_unknown() {
                assert!(positive.is_unknown() && compared.is_unknown());
                assert!(!matches!(negative, Ty::Never | Ty::Null));
            } else {
                assert!(positive.same_identity(&known_non_null));
                assert!(negative.same_identity(&Ty::union(vec![member, Ty::Null])));
                assert!(compared.same_identity(&known_non_null));
            }
        }
    });
}

#[test]
fn generic_guards_refine_known_null_arms_on_each_guard_route() {
    let source = r#"
        function truth<T>(x: T | null) { if (x) { void /*@*/x; } }
        function early<T>(x: T | null) { if (!x) return; void /*@*/x; }
        function equal<T>(x: T | null) { if (x !== null) { void /*@*/x; } }
        function exit<T>(x: T | null) { if (x === null) return; void /*@*/x; }
        function falsy<T>(x: T | null) { if (!x) { void /*@*/x; } }
    "#;
    with_native_file(source, |file, marks| {
        let parameter = Ty::Param("T".into());
        for &mark in &marks[..4] {
            assert!(file.type_of(mark).same_identity(&parameter));
        }
        assert!(file.type_of(marks[4]).same_identity(&Ty::union(vec![parameter, Ty::Null])));
    });
}

#[test]
fn guard_refinement_does_not_cross_writes_functions_or_other_references() {
    let source = r#"
        function written<T>(x: T | null) { if (!x) return; x = null; void /*@*/x; }
        function nested<T>(x: T | null) { if (!x) return; const child = () => /*@*/x; }
        function other<T>(x: T | null, y: T | null) { if (!y) return; void /*@*/x; }
    "#;
    with_native_file(source, |file, marks| {
        let nullable = Ty::union(vec![Ty::Param("T".into()), Ty::Null]);
        for &mark in marks {
            assert!(file.type_of(mark).same_identity(&nullable));
        }
    });
}
