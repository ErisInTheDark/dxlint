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
fn annotated_bindings_share_their_type_query_owner() {
    let mut failures = Vec::new();
    for annotation in ["{ m(): number }", "() => number", "{ m(): void }", "{ value: number }", "Alias"] {
        let source = format!("type Alias = {{ m(): number }}; declare const value: {annotation}; type Query = /*@*/typeof value; void /*@*/value;");
        crate::engine::tests::with_native_file(&source, |file, nodes| {
            let query = file.type_of(nodes[0]);
            let value = file.type_of(nodes[1]);
            if query.is_opaque() || !query.same_identity(&value) { failures.push((annotation, query, value)); }
        });
    }
    assert!(failures.is_empty(), "binding/query owners differ: {failures:?}");
}

#[test]
fn binding_patterns_project_values_without_inventing_rest_or_computed_keys() {
    let mut failures = Vec::new();
    for (source, expected) in [
        ("declare const input: { value: number }; const { value } = input; void /*@*/value;", Some("number")),
        ("declare const input: { outer: { value: string } }; const { outer: { value: renamed } } = input; void /*@*/renamed;", Some("string")),
        ("declare const input: [number, string]; const [, second] = input; void /*@*/second;", Some("string")),
        ("declare const input: { value?: number }; const { value = 0 } = input; void /*@*/value;", Some("number")),
        ("declare const input: { value?: number }; const { value: renamed = 0 } = input; void /*@*/renamed;", Some("number")),
        ("declare const input: { value: string }; const { value = 0 } = input; void /*@*/value;", Some("string")),
        ("declare const input: { value: number }; const { ['value']: renamed } = input; void /*@*/renamed;", Some("number")),
        ("function f({ value }: { value: string }) { void /*@*/value; }", Some("string")),
        ("const f: (input: { value: number }) => void = ({ value }) => { void /*@*/value; };", Some("number")),
        ("declare const input: { value: number }; const { ...rest } = input; void /*@*/rest;", None),
        ("declare const input: { value: number }; declare const key: string; const { [key]: value } = input; void /*@*/value;", None),
        ("declare const input: unknown; const { value = 0 } = input; void /*@*/value;", None),
        ("const input = []; void /*@*/input;", None),
    ] {
        crate::engine::tests::with_native_file(source, |file, nodes| {
            let ty = file.type_of(nodes[0]);
            let correct = expected.map_or_else(|| ty.is_opaque(), |kind| file.typing().checker.equivalent(&ty, &Ty::Prim(kind)) == Some(true));
            if !correct { failures.push((source, ty)); }
        });
    }
    assert!(failures.is_empty(), "binding projections lost: {failures:?}");
    crate::engine::tests::with_native_file("declare const input: { value: null | undefined }; const { value = 0 } = input; void /*@*/value;", |file, nodes| {
        let ty = file.type_of(nodes[0]);
        assert_eq!(file.typing().checker.equivalent(&ty, &Ty::union(vec![Ty::Null, Ty::Lit("0".into(), "number")])), Some(true), "{ty:?}");
    });
}

#[test]
fn dynamic_import_values_and_imported_initializers_share_module_types() {
    use crate::native::ts::{facts::NativeFile, kinds::Kind, syntax::Syntax};
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/dynamic-import-owner");
    let source = "import { Loader } from './loader'; const direct = await import('./value'); const template = await import(`./value`); const escaped = await import('./\\x76alue'); const parenthesised = await import(('./value')); const { value: renamed } = await import('./value'); const indirect = new Loader().pending; const unsupported = import(name); void direct; void template; void escaped; void parenthesised; void renamed; void indirect; void unsupported;";
    let inputs = [
        ("tsconfig.json", r#"{"compilerOptions":{"lib":["fixture"],"types":[]}}"#),
        ("node_modules/typescript/lib/lib.fixture.d.ts", "interface Promise<T> { then(callback: (value: T) => unknown): unknown }"),
        ("main.ts", source),
        ("value.ts", "export const value: number = 1;"),
        ("loader.ts", "export class Loader { pending = import('./value'); }"),
    ];
    let buffers: Vec<_> = inputs.iter().map(|(name, text)| {
        let path = dir.join(name);
        reads::set_buffer(&path, text);
        Buffer(path)
    }).collect();
    let front = crate::engine::tests::native_front();
    let parsed = crate::native::ts::parse(source, false).unwrap();
    let tree = crate::grammar::Tree::from_native(parsed.nodes, parsed.root);
    let project = Project::default();
    let path = dir.join("main.ts");
    let file = NativeFile::new(Syntax::new(&tree, source, &front), &path, &project);
    let checker = &file.typing().checker;
    let mut failures = Vec::new();
    for node in tree.order.iter().copied().filter(|&node| file.syntax.is(node, Kind::Ident) && file.syntax.parent(node).is_some_and(|owner| file.syntax.is(owner, Kind::Unary))) {
        let text = file.syntax.text(node);
        let value = file.type_of(node);
        let ty = match text {
            "renamed" => value,
            "indirect" => checker.member(&checker.awaited(&value), "value"),
            "unsupported" => { assert!(value.is_opaque(), "{value:?}"); continue; }
            _ => checker.member(&value, "value"),
        };
        if checker.equivalent(&ty, &Ty::Prim("number")) != Some(true) { failures.push((text, ty)); }
    }
    assert!(failures.is_empty(), "dynamic module domains lost: {failures:?}");
    drop(buffers);
}

#[test]
fn ambient_dynamic_imports_respect_synthetic_default_policy_and_real_exports() {
    let mut failures = Vec::new();
    for (policy, enabled) in [("", true), (",\"allowSyntheticDefaultImports\":true", true), (",\"allowSyntheticDefaultImports\":false", false)] {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("target/dynamic-import-interop-{}", policy.len()));
        let config = format!("{{\"compilerOptions\":{{\"lib\":[\"fixture\"],\"types\":[]{policy}}}}}");
        let inputs = [
            ("tsconfig.json", config.as_str()),
            ("node_modules/typescript/lib/lib.fixture.d.ts", "interface Promise<T> { then(callback: (value: T) => unknown): unknown } declare module 'ambient' { export const value: number; } declare module 'actual' { const actual: { value: string }; export default actual; } declare module 'marked' { export const __esModule: true; export const value: number; } declare module 'assigned' { const assigned: { value: boolean }; export=assigned; }"),
            ("value.ts", "export const value: number = 1;"),
        ];
        let _buffers: Vec<_> = inputs.iter().map(|(name, text)| {
            let path = dir.join(name);
            reads::set_buffer(&path, text);
            Buffer(path)
        }).collect();
        let source = "export {};";
        let project = Project::default();
        let parsed = crate::native::ts::parse(source, false).unwrap();
        let (id, summary) = project.summary_for(&dir.join("main.ts"), source, &Raw { src: source, nodes: &parsed.nodes }, parsed.root);
        let checker = Checker::new(&project, id, summary);
        let env = Env::file(id);
        for (specifier, expected) in [("ambient", enabled.then_some("number")), ("actual", Some("string")), ("assigned", Some("boolean")), ("marked", None), ("./value", None)] {
            let module = checker.awaited(&checker.import_type(specifier, &env));
            if specifier == "ambient" {
                assert_eq!(checker.equivalent(&checker.member(&module, "value"), &Ty::Prim("number")), Some(true));
            }
            let default = checker.member(&module, "default");
            let correct = expected.map_or_else(|| default.is_opaque(), |kind| checker.equivalent(&checker.member(&default, "value"), &Ty::Prim(kind)) == Some(true));
            if !correct { failures.push((policy, specifier, default)); }
        }
    }
    assert!(failures.is_empty(), "dynamic default export policy lost: {failures:?}");
}

#[test]
fn import_aliases_preserve_namespace_and_generic_class_types() {
    let source = "namespace N { export class Box<T> { value: T; } } import Space = N; import Box = N.Box; type Direct = /*@*/N.Box<number>; type Through = /*@*/Space.Box<number>; type Alias = /*@*/Box<number>;";
    crate::engine::tests::with_native_file(source, |file, nodes| {
        let types: Vec<_> = nodes.iter().map(|&node| file.type_of(node)).collect();
        for ty in &types[1..] {
            assert!(!ty.is_opaque() && ty.same_identity(&types[0]), "import alias lost its class type: {ty:?}");
            assert!(file.typing().checker.member(ty, "value").same_identity(&Ty::Prim("number")), "import alias lost type arguments");
        }
    });
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
