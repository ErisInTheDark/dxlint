use crate::engine::tests::with_native_file;
use crate::native::ts::summary::Visibility;
use super::{Checker, Env, Ty, TypeSyn};
use crate::native::ts::{modules::{DeclRef, Globals, Project}, view::Raw};
use std::{path::Path, sync::Arc};

fn with_library(src: &str, run: impl FnOnce(&Checker<'_>)) {
    let project = Project::default();
    let parsed = crate::native::ts::parse(src, false).expect("library fixture");
    let (file, summary) = project.summary_for(Path::new("/root/lib.fixture.d.ts"), src, &Raw { src, nodes: &parsed.nodes }, parsed.root);
    let names = summary.globals.names.iter().map(|(name, declarations)| {
        (name.clone(), declarations.iter().map(|&decl| DeclRef { file, decl }).collect())
    }).collect();
    let mut checker = Checker::new(&project, file, summary);
    checker.globals = Arc::new(Globals { names, ..Globals::default() });
    run(&checker);
}

#[test]
fn property_mutability_follows_declarations_and_readonly_utilities() {
    for (shape, expected) in [
        ("interface Target { readonly value: number; }", Some(true)),
        ("class Target { readonly value: number; }", Some(true)),
        ("type Target = { readonly value: number };", Some(true)),
        ("interface Target { value: number; }", Some(false)),
        ("class Base { readonly value: number; } class Target extends Base {}", Some(true)),
        ("class Base { readonly value: number; } class Target extends Base { value: number; }", Some(false)),
        ("type Target = Readonly<{ value: number }>;", Some(true)),
    ] {
        let source = format!("type Readonly<T> = {{ readonly [P in keyof T]: T[P] }}; {shape}");
        with_library(&source, |checker| {
            let ty = checker.eval(&TypeSyn::Ref(vec!["Target".into()].into(), Vec::new()), &Env::file(checker.file));
            assert_eq!(checker.property_readonly(&ty, "value"), expected, "{shape}");
            assert_eq!(checker.property_readonly(&ty, "missing"), None, "{shape}");
        });
    }
}

#[test]
fn object_prototype_lookup_follows_explicit_properties_before_index_signatures() {
    for (shape, owner) in [
        ("class Target {}", Some("Object")),
        ("interface Target { value: number; }", Some("Object")),
        ("type Target = { value: number };", Some("Object")),
        ("type Target = object;", Some("Object")),
        ("type Target = Record<string, unknown>;", Some("Object")),
        ("interface Empty {} interface Custom { toString(): number; } interface Target extends Empty, Custom {}", Some("Custom")),
        ("class Target { toString: Missing; }", Some("Target")),
        ("type Target = Record<'toString', unknown>;", None),
    ] {
        let source = format!("interface Object {{ toString(): string; }} type Record<K extends keyof any, T> = {{ [P in K]: T }}; {shape}");
        with_library(&source, |checker| {
            let ty = checker.eval(&TypeSyn::Ref(vec!["Target".into()].into(), Vec::new()), &Env::file(checker.file));
            let resolution = checker.member_resolution(&ty, "toString");
            assert!(resolution.declared, "{source}: {resolution:?}");
            assert_eq!(resolution.declarations.len(), 1, "{source}: {resolution:?}");
            let actual = resolution.declarations[0].owner.map(|origin| checker.summary_of(origin.file).unwrap().decls[origin.decl as usize].name.clone());
            assert_eq!(actual.as_deref(), owner, "{source}: {resolution:?}");
            assert_eq!(checker.print(&resolution.ty), checker.print(&checker.member(&ty, "toString")));
        });
    }
    with_library("interface Object { toString(): string; } class Target {}", |checker| {
        let ty = checker.eval(&TypeSyn::Ref(vec!["Target".into()].into(), Vec::new()), &Env::file(checker.file));
        assert!(matches!(checker.member(&ty, "missing"), Ty::Unknown));
        assert_eq!(checker.depth.get(), 0);
    });
}

#[test]
fn member_resolution_keeps_winning_declarations_even_when_their_type_is_unknown() {
    for (declarations, expected, count) in [
        ("class Base { protected value: Missing; } class Child extends Base {}", Visibility::Protected, 1),
        ("class Base { private value: number; } class Child extends Base { public value: Missing; }", Visibility::Public, 1),
        ("class Child { constructor(private value: number) {} }", Visibility::Private, 1),
        ("interface Child { value(): number; } interface Child { value(arg: string): number; }", Visibility::Public, 2),
    ] {
        let source = format!("{declarations} declare const object: Child; const result = /*@*/object;");
        with_native_file(&source, |file, nodes| {
            let checker = &file.typing().checker;
            let ty = file.type_of(nodes[0]);
            let resolved = checker.member_resolution(&ty, "value");
            assert_eq!(resolved.declarations.len(), count, "{source}: {resolved:?}");
            assert!(!resolved.from_index);
            assert!(resolved.declarations.iter().all(|declaration| declaration.member.visibility == expected));
            let direct = checker.member(&ty, "value");
            assert_eq!(checker.print(&resolved.ty), checker.print(&direct), "{source}");
        });
    }
}

#[test]
fn computed_symbol_origins_keep_signature_shape_and_declaration_scope() {
    with_library("interface SymbolConstructor { toPrimitive: symbol; } declare const Symbol: SymbolConstructor; interface Target { [Symbol.toPrimitive](): string; } namespace Shadow { export const Symbol: SymbolConstructor; export interface Target { [Symbol.toPrimitive](): string; } }", |checker| {
        for (path, symbol) in [
            (vec!["Target".into()], Some("toPrimitive")),
            (vec!["Shadow".into(), "Target".into()], None),
        ] {
            let ty = checker.eval(&TypeSyn::Ref(path.into(), Vec::new()), &Env::file(checker.file));
            let resolved = checker.member_resolution(&ty, "[Symbol.toPrimitive]");
            assert_eq!(resolved.declarations.len(), 1, "{resolved:?}");
            let declaration = &resolved.declarations[0];
            assert!(declaration.member.signature);
            assert_eq!(checker.declaration_key_symbol(declaration), symbol);
        }
    });
}

#[test]
fn unresolved_explicit_heritage_does_not_claim_implicit_object_members() {
    with_library("interface Object { toString(): string; } type Constructable<T> = abstract new (...args: any[]) => T; interface Custom { toString(): string; } declare const base: Constructable<Custom>; class Target extends base {}", |checker| {
        let ty = checker.eval(&TypeSyn::Ref(vec!["Target".into()].into(), Vec::new()), &Env::file(checker.file));
        let resolution = checker.member_resolution(&ty, "toString");
        assert!(!resolution.declared, "{resolution:?}");
        assert!(resolution.declarations.is_empty(), "{resolution:?}");
        assert!(resolution.ty.is_opaque(), "{resolution:?}");
        assert!(checker.member(&ty, "toString").is_opaque());
    });
}

#[test]
fn record_fallback_provenance_survives_nullable_and_composite_receivers() {
    for (shape, from_index, declared) in [
        ("Record<string, number>", true, false),
        ("Record<'value', number>", false, true),
        ("Record<string, number> | null", true, false),
        ("Record<string, number> | { value: number }", true, false),
        ("Record<string, number> & { other: string }", true, false),
        ("Record<string, number> & { value: number }", true, true),
    ] {
        let source = format!("type Record<K extends keyof any, T> = {{ [P in K]: T }}; type Target = {shape};");
        with_library(&source, |checker| {
            let ty = checker.eval(&TypeSyn::Ref(vec!["Target".into()].into(), Vec::new()), &Env::file(checker.file));
            let resolution = checker.member_resolution(&ty, "value");
            assert_eq!((resolution.from_index, resolution.declared), (from_index, declared), "{shape}: {resolution:?}");
            assert_eq!(resolution.signature_keys.iter().any(|key| matches!(key, Ty::Prim("string"))), from_index);
            assert_eq!(checker.print(&resolution.ty), checker.print(&checker.member(&ty, "value")), "{shape}");
        });
    }
}
