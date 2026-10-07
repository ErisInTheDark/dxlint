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
