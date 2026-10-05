//! Recursive type properties exposed to DSL policies.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::summary::{DeclKind, MemberKind};
use crate::native::ts::topics::typed::specifiers;
use crate::native::ts::types::{Checker, Ty};

pub(super) fn structure(file: &NativeFile<'_>, node: u32, properties: &[String]) -> Value<'static> {
    let ck = &file.typing().checker;
    let aliases = specifiers::annotated_aliases(file, node).into_iter().filter_map(|r| {
        let retained = specifiers::retained_alias(ck, r, &mut Vec::new())?;
        Some(Value::from(ck.summary_of(retained.file)?.decls[retained.decl as usize].name.to_string()))
    }).collect();
    describe(&file.constrained_type_of(node), ck, properties, aliases, 0)
}

fn describe(ty: &Ty, ck: &Checker<'_>, properties: &[String], aliases: Vec<Value<'static>>, depth: usize) -> Value<'static> {
    if depth >= 64 { return Value::record(vec![("kind".into(), Value::from("recursive"))]) }
    if let Some(source) = ck.readonly_source(ty) { return describe(source, ck, properties, aliases, depth + 1) }
    let parts = match ty {
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().map(|part| describe(part, ck, properties, Vec::new(), depth + 1)).collect(),
        Ty::Array(array) => vec![describe(&array.element, ck, properties, Vec::new(), depth + 1)],
        Ty::Tuple(tuple) => tuple.elements.iter().map(|element| {
            if element.optional { describe(&element.ty.clone().or_undefined(), ck, properties, Vec::new(), depth + 1) }
            else { describe(&element.ty, ck, properties, Vec::new(), depth + 1) }
        }).collect(),
        _ => Vec::new(),
    };
    let named = specifiers::named_types(ty, ck, &mut Vec::new());
    let names = std::iter::once(Value::from(ck.print(ty))).chain(named.iter().map(|(_, ty)| Value::from(ck.print(ty)))).collect();
    let generic_names = named.first().and_then(|(r, _)| {
        let summary = ck.summary_of(r.file)?;
        let decl = &summary.decls[r.decl as usize];
        (!decl.type_params.is_empty()).then(|| Value::from(decl.name.to_string()))
    }).into_iter().collect();
    let members = properties.iter().map(|name| {
        let resolution = ck.member_resolution(ty, name);
        let declarations = resolution.declarations.iter().map(|declaration| {
            let owner = declaration.owner.and_then(|r| ck.summary_of(r.file).map(|summary| {
                let decl = &summary.decls[r.decl as usize];
                Value::record(vec![
                    ("name".into(), Value::from(decl.name.to_string())),
                    ("kind".into(), Value::from(match decl.kind {
                        DeclKind::Interface => "interface", DeclKind::Class => "class", DeclKind::Alias => "alias", _ => "other",
                    })),
                    ("from".into(), Value::from(if ck.is_lib_decl(r) { "lib" } else { "file" })),
                ])
            })).unwrap_or(Value::Null);
            Value::record(vec![
                ("owner".into(), owner),
                ("kind".into(), Value::from(match declaration.member.kind {
                    MemberKind::Prop => "property", MemberKind::Method => "method", MemberKind::Call => "call",
                    MemberKind::Construct => "construct", MemberKind::Index => "index",
                })),
                ("signature".into(), Value::Bool(declaration.member.signature)),
                ("key_symbol".into(), ck.declaration_key_symbol(declaration).map_or(Value::Null, |name| Value::from(name.to_string()))),
            ])
        }).collect();
        Value::record(vec![("name".into(), Value::from(name.clone())), ("declarations".into(), Value::list(declarations))])
    }).collect();
    Value::record(vec![
        ("kind".into(), Value::from(ty.kind())),
        ("parts".into(), Value::list(parts)),
        ("names".into(), Value::list(names)),
        ("generic_names".into(), Value::list(generic_names)),
        ("aliases".into(), Value::list(aliases)),
        ("members".into(), Value::list(members)),
    ])
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{native_linter, with_native_file};
    use crate::expr::Value;
    use std::path::Path;

    const OBJECT: &str = "interface Object { toString(): string; toLocaleString(): string; valueOf(): Object; }";

    #[test]
    fn optional_tuple_elements_include_their_missing_value() {
        with_native_file("interface Plain {} declare const tuple: [Plain?]; const result = /*@*/tuple;", |file, nodes| {
            let parts = super::structure(file, nodes[0], &[]).key("parts").items();
            let alternatives = parts[0].key("parts").items();
            assert!(alternatives.iter().any(|part| part.key("kind").loose_eq(&Value::from("undefined"))));
            assert!(alternatives.iter().any(|part| part.key("kind").loose_eq(&Value::from("object"))));
        });
    }

    #[test]
    fn coercion_routes_respect_string_shadowing_and_tagged_templates() {
        let linter = native_linter("rule no-base-to-string");
        let source = format!("{OBJECT} class Plain extends Object {{}} declare const plain: Plain; declare const list: Plain[]; let text = ''; String(plain); '' + plain; text += plain; `${{plain}}`; plain.toString(); plain.toLocaleString(); list.join(); (plain).toString(); tag`${{plain}}`; function f(String: (value: Plain) => string) {{ String(plain); }}");
        let diagnostics = linter.lint(Path::new("/root/lib.fixture.d.ts"), &source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["plain", "plain", "plain", "plain", "plain", "plain", "list", "plain"]);
    }

    #[test]
    fn object_coercion_follows_winning_overrides() {
        let linter = native_linter("rule no-base-to-string");
        let source = format!("{OBJECT} class Plain extends Object {{}} class Useful extends Object {{ toString(): string {{ return 'useful'; }} }} declare const plain: Plain; declare const useful: Useful; '' + plain; '' + useful;");
        let diagnostics = linter.lint(Path::new("/root/lib.fixture.d.ts"), &source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["plain"]);
    }

    #[test]
    fn collection_and_compound_types_preserve_possible_object_coercion() {
        let linter = native_linter("rule no-base-to-string");
        let source = format!("{OBJECT} interface Plain extends Object {{}} interface Useful {{ toString(): string; }} declare const mixed: Plain | string; declare const safe: Plain & Useful; declare const tuple: [string, Plain]; declare const array: Plain[]; '' + mixed; '' + safe; tuple.join(); array.join();");
        let diagnostics = linter.lint(Path::new("/root/lib.fixture.d.ts"), &source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["mixed", "tuple", "array"]);
    }

    #[test]
    fn ignored_structural_aliases_do_not_ignore_expanded_class_aliases() {
        let linter = native_linter("rule no-base-to-string { set ignoredTypeNames ['Structural', 'Generic', 'ClassAlias'] }");
        let source = format!("{OBJECT} class Plain extends Object {{}} class Other extends Object {{}} class Wrapper<T> extends Object {{}} type Structural = Plain | Other; type Generic<T> = Plain | Wrapper<T>; type ClassAlias = Plain; declare const structural: Structural; declare const generic: Generic<number>; declare const expanded: ClassAlias; '' + structural; '' + generic; '' + expanded;");
        let unfiltered = native_linter("rule no-base-to-string").lint(Path::new("/root/lib.fixture.d.ts"), &source);
        assert_eq!(unfiltered.len(), 3);
        let diagnostics = linter.lint(Path::new("/root/lib.fixture.d.ts"), &source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["expanded"]);
    }

    #[test]
    fn unknown_option_does_not_treat_unresolved_types_as_explicit_unknown() {
        let source = "declare const unknownValue: unknown; declare const unresolved: Missing; '' + unknownValue; '' + unresolved;";
        let linter = native_linter("rule no-base-to-string");
        assert!(linter.lint(Path::new("/root/a.ts"), source).is_empty());
        let linter = native_linter("rule no-base-to-string { set checkUnknown true }");
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["unknownValue"]);
    }
}
