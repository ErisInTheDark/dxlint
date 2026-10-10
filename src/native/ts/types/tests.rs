use super::*;
use crate::native::ts::{reads, view::Raw};
use std::path::PathBuf;

struct Buffer(PathBuf);
impl Drop for Buffer {
    fn drop(&mut self) { reads::drop_buffer(&self.0); }
}

fn with_import(src: &str, base: &str, run: impl FnOnce(&Checker<'_>, &std::path::Path)) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::current_dir().unwrap().join(format!("target/binding-test-{id}"));
    let path = dir.join("main.ts");
    let base_path = dir.join("base.ts");
    reads::set_buffer(&base_path, base);
    let _buffer = Buffer(base_path.clone());
    let project = Project::default();
    let parsed = crate::native::ts::parse(src, false).unwrap();
    let (file, summary) = project.summary_for(&path, src, &Raw { src, nodes: &parsed.nodes }, parsed.root);
    run(&Checker::new(&project, file, summary), &base_path);
}

fn reference(name: &str, args: Vec<Ty>, checker: &Checker<'_>) -> Ty {
    checker.named_type(&[name.into()], args, &Env::file(checker.file))
}

#[test]
fn imported_class_initializers_preserve_their_value_domains() {
    let mut failures = Vec::new();
    for (name, source, path) in [
        ("object", "export class C { options = { delay: 13 }; }", vec!["options", "delay"]),
        ("static object", "export class C { static defaults = { delay: 13 }; options = C.defaults; }", vec!["options", "delay"]),
        ("generic call", "declare function clone<T>(value: T): T; declare const input: { delay: number }; export class C { options = clone(input); }", vec!["options", "delay"]),
        ("generic constructor", "declare const input: { delay: number }; class Box<T> { value: T; constructor(value: T) { this.value = value; } } export class C { options = new Box(input); }", vec!["options", "value", "delay"]),
        ("arithmetic", "export class C { delay = 200 / 4; }", vec!["delay"]),
        ("combined", "const divisor = 4; const defaultDelay = 200 / divisor; declare function clone<T>(value: T): T; export class C { static defaults = { delay: defaultDelay }; options = clone(C.defaults); }", vec!["options", "delay"]),
        ("annotated control", "export class C { options: { delay: number } = { delay: 13 }; }", vec!["options", "delay"]),
        ("primitive control", "export class C { delay = 13; }", vec!["delay"]),
    ] {
        with_import("import { C } from './base';", source, |checker, _| {
            let value = path.iter().fold(reference("C", Vec::new(), checker), |ty, name| checker.member(&ty, name));
            if checker.equivalent(&value.clone().widen(), &Ty::Prim("number")) != Some(true) {
                failures.push((name, value));
            }
        });
    }
    assert!(failures.is_empty(), "initializer domains lost: {failures:?}");
}

#[test]
fn mutable_initializer_locations_widen_fresh_literals_not_contracts() {
    let source = "const ready = true; const chained = ready; const fixed: true = true; declare function returnsTrue(): true; namespace N { export const ready = true; } export class C { options = { ready, chained, fixed, call: returnsTrue(), asserted: ready as true, namespaced: N.ready }; mutable = ready; readonly immutable = ready; fixedAlias = fixed; }";
    let mut failures = Vec::new();
    with_import("import { C } from './base';", source, |checker, _| {
        let object = checker.member(&reference("C", Vec::new(), checker), "options");
        for (field, expected) in [
            ("ready", Ty::Prim("boolean")), ("chained", Ty::Prim("boolean")), ("namespaced", Ty::Prim("boolean")),
            ("fixed", Ty::Lit("true".into(), "boolean")), ("call", Ty::Lit("true".into(), "boolean")), ("asserted", Ty::Lit("true".into(), "boolean")),
        ] {
            let value = checker.member(&object, field);
            if checker.equivalent(&value, &expected) != Some(true) { failures.push((field, value)); }
        }
        for (field, expected) in [
            ("mutable", Ty::Prim("boolean")), ("immutable", Ty::Lit("true".into(), "boolean")), ("fixedAlias", Ty::Lit("true".into(), "boolean")),
        ] {
            let value = checker.member(&reference("C", Vec::new(), checker), field);
            if checker.equivalent(&value, &expected) != Some(true) { failures.push((field, value)); }
        }
    });
    assert!(failures.is_empty(), "literal contracts changed: {failures:?}");
}

#[test]
fn inherited_member_queries_share_bindings_without_sharing_evaluation() {
    let count = 64;
    let fields = (0..count).map(|i| format!("p{i}: T;")).collect::<String>();
    let base = format!("export class Base<T> {{ {fields} self: this }}");
    with_import("import { Base } from './base'; class Child<T> extends Base<T> {}", &base, |checker, _| {
        let number = reference("Child", vec![Ty::Prim("number")], checker);
        let string = reference("Child", vec![Ty::Prim("string")], checker);
        BINDING_WORK.with(|work| work.set(0));
        for _ in 0..4 {
            for i in 0..count {
                let name = format!("p{i}");
                assert!(checker.member(&number, &name).same_identity(&Ty::Prim("number")));
                assert!(checker.member(&string, &name).same_identity(&Ty::Prim("string")));
            }
            assert!(checker.member(&number, "self").same_representation(&number));
            assert!(checker.member(&string, "self").same_representation(&string));
        }
        let work = BINDING_WORK.with(|work| work.get());
        assert!(work <= 8, "repeated inherited bindings traversed {work} times");
    });
}

#[test]
fn lexical_bindings_keep_file_namespace_module_and_depth_separate() {
    let src = "import { Base } from './base'; class X {} namespace N { export class X {} } declare module 'one' { class X {} } declare module 'two' { class X {} }";
    with_import(src, "export class Base {}", |checker, _| {
        let path = ["X".into()];
        let root = Env::file(checker.file);
        let top = checker.type_decls(&path, &root);
        let namespace = checker.summary.decls.iter().position(|decl| &*decl.name == "N").unwrap() as u32;
        let envs = [
            Env { namespaces: vec![namespace], ..root.clone() },
            Env { module: ModuleAt::Ambient(0), ..root.clone() },
            Env { module: ModuleAt::Ambient(1), ..root.clone() },
        ];
        let answers: Vec<_> = envs.iter().map(|env| checker.type_decls(&path, env)).collect();
        assert_eq!(top.len(), 1);
        for (i, answer) in answers.iter().enumerate() {
            assert_eq!(answer.len(), 1);
            assert_ne!(answer, &top);
            for previous in &answers[..i] { assert_ne!(answer, previous); }
        }
        let other = checker.project.file_id(std::path::Path::new("different-binding-file.ts"));
        assert!(checker.type_decls(&path, &Env::file(other)).is_empty());
        let imported = ["Base".into()];
        checker.depth.set(MAX_DEPTH);
        assert!(checker.type_decls(&imported, &root).is_empty());
        checker.depth.set(0);
        assert_eq!(checker.type_decls(&imported, &root).len(), 1);
        checker.depth.set(MAX_DEPTH);
        assert!(checker.type_decls(&imported, &root).is_empty());
        checker.depth.set(0);
    });
}

#[test]
fn repeated_bindings_replay_success_and_missing_resolution_reads() {
    with_import("import { Base } from './base'; import { Missing } from './missing';", "export class Base {}", |checker, base_path| {
        let env = Env::file(checker.file);
        for (name, present) in [("Base", true), ("Missing", false)] {
            let path = [name.into()];
            let (first, _) = reads::track(|| checker.type_decls(&path, &env));
            let (second, recorded) = reads::track(|| checker.type_decls(&path, &env));
            assert_eq!(first, second);
            assert_eq!(!second.is_empty(), present);
            if present {
                assert!(recorded.any(&mut |dep| matches!(dep, reads::Dep::Summary(path, ..) if path == base_path)));
            } else {
                assert!(recorded.any(&mut |dep| matches!(dep, reads::Dep::IsFile(path, false) if path.ends_with("missing.ts"))));
            }
        }
    });
}
