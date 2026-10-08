//! Relationships between an expression's operand, annotation and context.

use crate::expr::{facts::Fact, Value};
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::types::{Checker, Ty};

pub const FACTS: &[Fact] = &[
    Fact {
        name: "type_relations",
        doc: "native expressions: operand, annotation and contextual type identity, shape and assignability; null when unresolved",
        get: |f, n| f.with_file(relations, n),
    },
];

fn relations(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let node = s.unparen(node);
    let operand_node = s.get(node, F::Expr).unwrap_or(node);
    let mut original_node = operand_node;
    while matches!(s.kind(s.unparen(original_node)), Some(K::As | K::TypeAssertion)) {
        let Some(expression) = s.get(s.unparen(original_node), F::Expr) else { break };
        original_node = expression;
    }
    let operand = file.type_of(operand_node);
    let original = file.type_of(original_node);
    let widened_operand = if matches!(operand, Ty::Struct(..)) { operand.clone().widen() } else { Ty::Unknown };
    let widened_original = if matches!(original, Ty::Struct(..)) { original.clone().widen() } else { Ty::Unknown };
    let annotation = if matches!(s.kind(node), Some(K::As | K::TypeAssertion | K::NonNull)) {
        file.type_of(node)
    } else {
        let bindings = file.bindings();
        let declaration = bindings.refs.get(&node).copied().unwrap_or(node);
        s.parent(declaration).and_then(|owner| s.get(owner, F::Type)).map_or(Ty::Unknown, |ty| file.type_of(ty))
    };
    let context = file.contextual_type_of(node);
    let constrained = file.constrained_type_of(operand_node);
    let ck = &file.typing().checker;
    Value::record(vec![
        ("operand".into(), profile(&operand, ck)),
        ("original".into(), profile(&original, ck)),
        ("widened_operand".into(), profile(&widened_operand, ck)),
        ("widened_original".into(), profile(&widened_original, ck)),
        ("annotation".into(), profile(&annotation, ck)),
        ("context".into(), profile(&context, ck)),
        ("constrained".into(), profile(&constrained, ck)),
        ("operand_annotation".into(), pair(&operand, &annotation, ck)),
        ("widened_operand_annotation".into(), pair(&widened_operand, &annotation, ck)),
        ("widened_original_annotation".into(), pair(&widened_original, &annotation, ck)),
        ("operand_context".into(), pair(&operand, &context, ck)),
        ("original_annotation".into(), pair(&original, &annotation, ck)),
        ("original_context".into(), pair(&original, &context, ck)),
        ("actual_constrained".into(), pair(&operand, &constrained, ck)),
    ])
}

fn pair(a: &Ty, b: &Ty, ck: &Checker<'_>) -> Value<'static> {
    if a.is_opaque() || b.is_opaque() { return Value::Null }
    Value::record(vec![
        ("identity".into(), Value::Bool(a.same_identity(b))),
        ("assignable".into(), optional(ck.assignable(a, b))),
        ("reverse_assignable".into(), optional(ck.assignable(b, a))),
        ("properties_equal".into(), optional(ck.property_shapes_equal(a, b))),
        ("type_arguments_equal".into(), optional(ck.type_arguments_equal(a, b))),
    ])
}

fn optional(value: Option<bool>) -> Value<'static> {
    value.map_or(Value::Null, Value::Bool)
}

fn profile(ty: &Ty, ck: &Checker<'_>) -> Value<'static> {
    if ty.is_opaque() { return Value::Null }
    fn has(ty: &Ty, predicate: fn(&Ty) -> bool) -> bool {
        match ty {
            Ty::Union(parts) => parts.iter().any(|part| has(part, predicate)),
            ty => predicate(ty),
        }
    }
    Value::record(vec![
        ("any".into(), Value::Bool(matches!(ty, Ty::Any))),
        ("union".into(), Value::Bool(matches!(ty, Ty::Union(..)))),
        ("intersection".into(), Value::Bool(matches!(ty, Ty::Inter(..)))),
        ("contains_any".into(), optional(ck.type_contains_any(ty))),
        ("contains_variable".into(), optional(ck.type_contains_variable(ty))),
        ("nonprimitive".into(), Value::Bool(matches!(ty, Ty::Prim("object")))),
        ("index_signature".into(), Value::Bool(!ck.index_signature_keys(ty).is_empty())),
        ("literal".into(), Value::Bool(matches!(ty, Ty::Lit(..)))),
        ("property_bag".into(), Value::Bool(matches!(ty, Ty::Struct(..)))),
        ("property_literals".into(), optional(ck.property_literals(ty))),
        ("generic_properties".into(), ck.generic_properties(ty).map_or(Value::Null, |names| {
            Value::List(names.into_iter().map(|name| Value::from(name.to_string())).collect())
        })),
        ("null".into(), Value::Bool(has(ty, |ty| matches!(ty, Ty::Null)))),
        ("undefined".into(), Value::Bool(has(ty, |ty| matches!(ty, Ty::Undefined)))),
        ("void".into(), Value::Bool(has(ty, |ty| matches!(ty, Ty::Void)))),
        ("unknown".into(), Value::Bool(has(ty, |ty| matches!(ty, Ty::IntrinsicUnknown)))),
        ("nullable".into(), Value::Bool(ty.may_be_nullish())),
    ])
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{buffered_project, native_linter, with_native_file};
    use std::path::Path;

    fn assert_fixes(linter: &crate::engine::Linter, path: &Path, cases: &[(&str, &str)]) {
        let checked: Vec<_> = cases.iter().map(|&(source, expected)| {
            assert!(crate::native::ts::parse(source, false).is_ok(), "invalid fixture: {source}");
            let reports = linter.lint(path, source);
            assert_eq!(reports.len(), 1, "{source}: {reports:?}");
            (source, expected, reports)
        }).collect();
        for (source, expected, reports) in checked {
            let (fixed, applied) = crate::engine::apply_fixes(source, &reports);
            assert_eq!(applied, 1, "{source}: {reports:?}");
            assert_eq!(fixed, expected);
            assert!(crate::native::ts::parse(&fixed, false).is_ok(), "{fixed}");
            assert!(linter.lint(path, &fixed).is_empty(), "{fixed}");
            assert_eq!(linter.fix(path, &fixed).text, fixed);
        }
    }

    #[test]
    fn type_relations_preserve_known_identity() {
        with_native_file("declare const value: string; const result = /*@*/(value as string);", |file, nodes| {
            let relation = super::relations(file, nodes[0]).key("operand_annotation");
            assert!(relation.key("identity").truthy());
        });
    }

    #[test]
    fn type_relations_distinguish_mutability_from_assignability() {
        with_native_file("interface A { value: number; } interface B { readonly value: number; } declare const value: A; const result = /*@*/(value as B);", |file, nodes| {
            let relation = super::relations(file, nodes[0]).key("operand_annotation");
            assert!(relation.key("assignable").truthy());
            assert!(relation.key("reverse_assignable").truthy());
            assert!(matches!(relation.key("properties_equal"), crate::expr::Value::Bool(false)));
        });
    }

    #[test]
    fn type_relations_keep_phantom_generic_arguments() {
        with_native_file("interface Tag<T> {} declare const value: Tag<string>; const result = /*@*/(value as Tag<number>);", |file, nodes| {
            let relation = super::relations(file, nodes[0]).key("operand_annotation");
            assert!(matches!(relation.key("type_arguments_equal"), crate::expr::Value::Bool(false)));
        });
    }

    #[test]
    fn assertion_policy_preserves_literal_widening_and_contextual_nullability() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        let source = "const exact = 3 as 3; let narrower = 3 as 3; declare const text: string; text!; declare const nullable: string | null; const receiving: string | null = nullable!;";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let expected: Vec<_> = ["3 as 3", "text!", "nullable!"].into_iter().map(|text| {
            let start = source.find(text).unwrap();
            (start, start + text.len())
        }).collect();
        assert_eq!(diagnostics.iter().map(|diagnostic| (diagnostic.start, diagnostic.end)).collect::<Vec<_>>(), expected);
        let (fixed, applied) = crate::engine::apply_fixes(source, &diagnostics);
        assert_eq!(applied, 3);
        assert_eq!(fixed, "const exact = 3; let narrower = 3 as 3; declare const text: string; text; declare const nullable: string | null; const receiving: string | null = nullable;");
    }

    #[test]
    fn assertion_fixes_preserve_comment_ownership_and_byte_boundaries() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        let path = Path::new("/root/a.ts");
        assert_fixes(&linter, path, &[
            ("const value = ( 3 + 5 ) /*keep*/ as //type\n(number);", "const value = ( 3 + 5 ) /*keep*/;"),
            ("declare const value: number; const result = (value //keep\n) as number;", "declare const value: number; const result = (value //keep\n);"),
            ("const value = <number> /*keep*/ (3 + 5);", "const value =  /*keep*/ (3 + 5);"),
            ("const value = <number /*type*/>(3 + 5);", "const value = (3 + 5);"),
            ("const unicode = '🌿'; declare const value: number; value /*keep*/ !;", "const unicode = '🌿'; declare const value: number; value /*keep*/ ;"),
        ]);
    }

    #[test]
    fn assertion_fixes_keep_expression_precedence_and_statement_boundaries() {
        let project = buffered_project("assertion-precedence", &[
            ("tsconfig.json", r#"{"compilerOptions":{"lib":["es5"],"types":[]}}"#),
            ("node_modules/typescript/lib/lib.es5.d.ts", "interface Function { readonly length: number; }"),
        ], "rule no-unnecessary-type-assertion");
        let linter = &project.linter;
        let path = &project.path("a.ts");
        assert_fixes(linter, path, &[
            ("<number>class Clazz {}.length;", "(class Clazz {}.length);"),
            ("<number>function Fun() {}.length;", "(function Fun() {}.length);"),
            ("<{ a: string }>{ a: 'foo' };", "({ a: 'foo' });"),
            ("const f = () => <{ a: string }>{ a: 'foo' };", "const f = () => ({ a: 'foo' });"),
            ("const value = <number>(3 + 5) * 2;", "const value = (3 + 5) * 2;"),
            ("function f() { return <number>\n(3 + 5); }", "function f() { return (\n(3 + 5)); }"),
            ("declare const n: number\n<number>(3 + 5);", "declare const n: number\n;(3 + 5);"),
            ("declare const value: number; value!instanceof Object;", "declare const value: number; value instanceof Object;"),
            ("interface Overloaded { (): void; (value: string): undefined; }\n((value => {}) as Overloaded)('') as undefined;", "interface Overloaded { (): void; (value: string): undefined; }\n((value => {}) as Overloaded)('');"),
            ("(<{ a: string }>{ a: 'foo' }, 1);", "({ a: 'foo' }, 1);"),
        ]);
    }

    #[test]
    fn assertion_fixes_remove_redundant_chains_and_contextual_casts() {
        let project = buffered_project("assertion-context", &[
            ("tsconfig.json", r#"{"compilerOptions":{"lib":["es5"],"types":[]}}"#),
            ("node_modules/typescript/lib/lib.es5.d.ts", "type Pick<T, K extends keyof T> = { [P in K]: T[P] };"),
        ], "rule no-unnecessary-type-assertion");
        let linter = &project.linter;
        let path = &project.path("a.ts");
        assert_fixes(linter, path, &[
            ("const value = 5 as any as 5;", "const value = 5;"),
            ("const value = (1 + 1) as any as number;", "const value = 1 + 1;"),
            ("const value = 2 * ((1 + 1) as any as number);", "const value = 2 * (1 + 1);"),
            ("declare const value: number; const result = <number>(<any>value);", "declare const value: number; const result = value;"),
            ("declare const value: number; const result = (value /*keep*/) as any as number;", "declare const value: number; const result = (value /*keep*/);"),
            ("declare function fn(value: number): void; fn(42 as unknown as number);", "declare function fn(value: number): void; fn(42);"),
            ("interface T { a: string } declare function fn<U extends T>(args: Pick<U, 'a'>): void; fn<T>({ a: '' as string });", "interface T { a: string } declare function fn<U extends T>(args: Pick<U, 'a'>): void; fn<T>({ a: '' });"),
        ]);
    }

    #[test]
    fn assertion_fixes_do_not_erase_generic_call_context() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        let source = "declare function get<T = number>(): T; const value = get() as number;";
        let path = Path::new("/root/a.ts");
        let reports = linter.lint(path, source);
        assert_eq!(reports.len(), 1, "{reports:?}");
        assert!(reports[0].fix.is_none());
        assert_eq!(linter.fix(path, source).text, source);
    }

    #[test]
    fn assertion_context_does_not_erase_generic_method_requirements() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        let source = "interface Generic { method<T>(value: T): T; } declare const broad: { method(value: any): any; extra: string }; const target: Generic = broad as Generic; declare function echo<T>(value: T): T; const inferred = echo(3) as number;";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let start = source.find("echo(3) as number").unwrap();
        assert_eq!(diagnostics.iter().map(|diagnostic| (diagnostic.start, diagnostic.end)).collect::<Vec<_>>(), [(start, start + "echo(3) as number".len())]);
    }

    #[test]
    fn assertion_options_control_const_checks_and_written_type_exclusions() {
        let source = "const literal = 3 as const; declare const numberValue: number; const typed = numberValue as number;";
        let linter = native_linter("rule no-unnecessary-type-assertion { set typesToIgnore [\"number\"] }\noverride in ts_as, ts_type_assertion let no_unnecessary_type_assertion_checked true");
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let start = source.find("3 as const").unwrap();
        assert_eq!(diagnostics.iter().map(|diagnostic| (diagnostic.start, diagnostic.end)).collect::<Vec<_>>(), [(start, start + "3 as const".len())]);
    }

    #[test]
    fn assertion_selection_preserves_required_casts_and_written_type_exclusions() {
        let source = "const literal = 3 as const; let kept = 3 as const; let needed = 3 as 3; declare const n: number; n as number; <number>n; n!; const template = `${n}` as const; n as string;";
        for (options, selection, expected) in [
            ("", "", vec!["n as number", "<number>n", "n!"]),
            ("", "override in ts_as, ts_type_assertion, ts_non_null let no_unnecessary_type_assertion_checked true", vec!["3 as const", "n as number", "<number>n", "n!"]),
            ("", "override in ts_as, ts_type_assertion, ts_non_null let no_unnecessary_type_assertion_checked false", vec![]),
            ("", "override in ts_as let no_unnecessary_type_assertion_checked false", vec!["<number>n", "n!"]),
            ("", "override in ts_type_assertion let no_unnecessary_type_assertion_checked false", vec!["n as number", "n!"]),
            ("", "override in ts_non_null let no_unnecessary_type_assertion_checked false", vec!["n as number", "<number>n"]),
            ("{ set typesToIgnore ['number'] }", "", vec!["n!"]),
            ("{ set typesToIgnore ['number'] }", "override in ts_as, ts_type_assertion, ts_non_null let no_unnecessary_type_assertion_checked true", vec!["3 as const", "n!"]),
        ] {
            let linter = native_linter(&format!("rule no-unnecessary-type-assertion {options}\n{selection}"));
            let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
            let positions: Vec<_> = expected.iter().map(|text| {
                let start = source.find(text).expect("expected assertion in source");
                (start, start + text.len())
            }).collect();
            assert_eq!(diagnostics.iter().map(|diagnostic| (diagnostic.start, diagnostic.end)).collect::<Vec<_>>(), positions, "{options} {selection}: {diagnostics:#?}");
        }
    }

    #[test]
    fn assertion_policy_retains_definite_assignment_and_fresh_literal_types() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        let source = "let value: number; value! + 1; type Literal = { value: 'a' }; const literal = { value: 'a' } as Literal;";
        assert!(linter.lint(Path::new("/root/a.ts"), source).is_empty());
    }

    #[test]
    fn assertion_context_retains_phantom_method_parameters() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        let source = "interface Generic { method<T>(value: string): void; } declare const broad: { method(value: string): void; extra: string }; const target: Generic = broad as Generic;";
        assert!(linter.lint(Path::new("/root/a.ts"), source).is_empty());
    }

    #[test]
    fn assertion_policy_compares_widened_mutable_record_properties() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        let source = "interface Props { value: number } const record = { value: 3 }; const result = record as Props;";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let start = source.find("record as Props").unwrap();
        assert_eq!(diagnostics.iter().map(|diagnostic| (diagnostic.start, diagnostic.end)).collect::<Vec<_>>(), [(start, start + "record as Props".len())]);
    }

    #[test]
    fn type_relations_detect_any_inside_named_properties() {
        with_native_file("interface Source { value: any } declare const value: Source; const result = /*@*/(value as { value: string });", |file, nodes| {
            assert!(matches!(super::relations(file, nodes[0]).key("operand").key("contains_any"), crate::expr::Value::Bool(true)));
        });
    }

    #[test]
    fn assertion_policy_honours_definite_assignment_declarations() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        let source = "let before: number; before!; let definite!: number; definite!;";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let start = source.rfind("definite!").unwrap();
        assert_eq!(diagnostics.iter().map(|diagnostic| (diagnostic.start, diagnostic.end)).collect::<Vec<_>>(), [(start, start + "definite!".len())]);
    }

    #[test]
    fn assertion_policy_preserves_template_const_inference() {
        let linter = native_linter("rule no-unnecessary-type-assertion\noverride in ts_as, ts_type_assertion let no_unnecessary_type_assertion_checked true");
        let source = "declare const text: 'foo'; const exact = `${text}-tail` as const; declare const name: string; const broad = `${name}` as string;";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let start = source.find("`${name}` as string").unwrap();
        assert_eq!(diagnostics.iter().map(|diagnostic| (diagnostic.start, diagnostic.end)).collect::<Vec<_>>(), [(start, start + "`${name}` as string".len())]);
    }

    #[test]
    fn assertion_context_preserves_open_parameters_and_satisfies_widening() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        for source in [
            "function cast<T extends string>(value: string): T { return value as T; }",
            "declare function foo<T extends unknown>(bar: T): T; const baz: unknown = {}; foo(baz!);",
            "interface Wide { name?: string } interface Narrow { name: string } declare const narrow: Narrow; const value = narrow as Wide satisfies Wide;",
            "interface Wide { name?: string } interface Narrow { name: string } declare const narrow: Narrow; const value = { value: narrow as Wide } satisfies Record<string, Wide>;",
            "declare function fn<T extends boolean>(options: { a: T extends true ? never : unknown } & { b: T }): void; fn({ a: true, b: true as any });",
        ] {
            assert!(linter.lint(Path::new("/root/a.ts"), source).is_empty(), "{source}");
        }
    }

    #[test]
    fn nested_assertions_report_only_the_redundant_outer_cast() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        for (source, outer) in [
            ("const x = 5 as any as 5;", "5 as any as 5"),
            ("declare function fn(value: number): void; fn(42 as unknown as number);", "42 as unknown as number"),
            ("interface Props { a: number } const fn = (): Props => ({ a: 1 }) as unknown as Props;", "({ a: 1 }) as unknown as Props"),
        ] {
            let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
            let start = source.find(outer).unwrap();
            assert_eq!(diagnostics.iter().map(|diagnostic| (diagnostic.start, diagnostic.end)).collect::<Vec<_>>(), [(start, start + outer.len())], "{source}");
        }
    }

    #[test]
    fn assertion_policy_preserves_nested_property_mutability() {
        let linter = native_linter("rule no-unnecessary-type-assertion");
        let source = "interface Mutable { nested: { value: number } } interface ReadonlyNested { nested: { readonly value: number } } declare const mutable: Mutable; const changed = mutable as ReadonlyNested;";
        assert!(linter.lint(Path::new("/root/a.ts"), source).is_empty());
    }
}
