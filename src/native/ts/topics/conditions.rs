//! Runtime value domains and resolved call contracts.

use crate::expr::facts::Fact;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::types::{LiteralValue, Ty, ValueDomains};

pub const FACTS: &[Fact] = &[
    Fact {
        name: "access_resolution",
        doc: "native members and indexes: index-signature origin, optional declaration and evaluated value domains",
        get: |f, n| f.with_file(access_resolution, n),
    },
    Fact {
        name: "type_domains",
        doc: "native expressions: constrained value domains, type kind and union/intersection constituent domains",
        get: |f, n| f.with_file(domains, n),
    },
    Fact {
        name: "evaluated_type_domains",
        doc: "native expressions: value domains before optional-chain short-circuiting, retaining intrinsic member and return nullability",
        get: |f, n| f.with_file(evaluated_domains, n),
    },
    Fact {
        name: "predicate_call",
        doc: "native calls: asserted argument, assertion kind, target union and bidirectional assignability of a resolved predicate",
        get: |f, n| f.with_file(predicate, n),
    },
    Fact {
        name: "callable_return_domains",
        doc: "native expressions: constrained domains of every callable return, including locally inferred function bodies",
        get: |f, n| f.with_file(callable_returns, n),
    },
];

fn access_resolution(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let Some(resolution) = file.access_resolution(node) else { return Value::Null };
    Value::record(vec![
        ("from_index".into(), Value::Bool(resolution.from_index)),
        ("optional".into(), Value::Bool(resolution.declarations.iter().any(|declaration| declaration.member.optional))),
        ("evaluated".into(), domain_record(file, &resolution.ty)),
    ])
}

fn domains(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    domain_record(file, &file.constrained_type_of(node))
}

fn domain_record(file: &NativeFile<'_>, ty: &Ty) -> Value<'static> {
    describe_domain(file, ty, 0)
}

fn describe_domain(file: &NativeFile<'_>, ty: &Ty, depth: usize) -> Value<'static> {
    let checker = &file.typing().checker;
    let mut shape = ty;
    let mut depth = depth;
    while depth < 64 {
        let Some(source) = checker.readonly_source(shape) else { break };
        shape = source;
        depth += 1;
    }
    if depth >= 64 {
        return Value::record(vec![("kind".into(), Value::from("recursive")), ("opaque".into(), Value::Bool(true))]);
    }
    let ValueDomains { truthy, falsy, nullish, non_nullish, opaque } = checker.value_domains(ty);
    let parts = match shape {
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().map(|part| describe_domain(file, part, depth + 1)).collect(),
        _ => Vec::new(),
    };
    let mut flags = Vec::new();
    file.flags_of(ty, &mut flags);
    let literal = checker.literal_value(ty).map_or(Value::Null, |literal| {
        let (kind, value) = match literal {
            LiteralValue::String(value) => ("string", Value::from(value)),
            LiteralValue::Number(value) => ("number", Value::from(value)),
            LiteralValue::Boolean(value) => ("boolean", Value::Bool(value)),
            LiteralValue::BigInt(value) => ("bigint", Value::from(value.to_string())),
            LiteralValue::Null => ("null", Value::Null),
            LiteralValue::Undefined => ("undefined", Value::Null),
        };
        Value::record(vec![("type".into(), Value::from(kind)), ("value".into(), value)])
    });
    Value::record(vec![
        ("kind".into(), Value::from(shape.kind())),
        ("parts".into(), Value::list(parts)),
        ("truthy".into(), Value::Bool(truthy)),
        ("falsy".into(), Value::Bool(falsy)),
        ("nullish".into(), Value::Bool(nullish)),
        ("non_nullish".into(), Value::Bool(non_nullish)),
        ("opaque".into(), Value::Bool(opaque)),
        ("flags".into(), Value::list(flags.into_iter().map(Value::from).collect())),
        ("literal".into(), literal),
    ])
}

fn evaluated_domains(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    domain_record(file, &file.evaluated_type_of(node))
}

fn predicate(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let Some((argument, predicate)) = file.resolved_predicate(node) else { return Value::Null };
    let checker = &file.typing().checker;
    let actual = file.constrained_type_of(argument);
    let target = predicate.target;
    let relation = |reverse| target.as_ref().map_or(Value::Null, |target| {
        let answer = if reverse { checker.assignable(target, &actual) } else { checker.assignable(&actual, target) };
        answer.map_or(Value::Null, Value::Bool)
    });
    Value::record(vec![
        ("argument_index".into(), Value::from(predicate.parameter.unwrap_or_default())),
        ("asserts".into(), Value::Bool(predicate.asserts)),
        ("argument".into(), domain_record(file, &actual)),
        ("target".into(), target.as_ref().map_or(Value::Null, |target| domain_record(file, target))),
        ("to_target".into(), relation(false)),
        ("from_target".into(), relation(true)),
        ("target_union".into(), Value::Bool(matches!(target, Some(Ty::Union(_))))),
    ])
}

fn callable_returns(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    Value::list(file.callable_return_types(&file.constrained_type_of(node)).into_iter().map(|ty| {
        domain_record(file, &file.constrained_type(ty, node))
    }).collect())
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{lint_project, native_linter, with_native_file};
    use std::path::Path;

    #[test]
    fn strict_boolean_checks_logical_contexts_without_duplicate_reports() {
        let linter = native_linter("rule strict-boolean-expressions { set allowNumber false\nset allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "declare const a: number; declare const b: number; const fallback = a || b; if ((a || b)) {} !((a)); if (a ?? b) {}";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["a", "a", "b", "a", "a ?? b"]);
    }

    #[test]
    fn strict_boolean_distinguishes_truthy_literals_from_nullable_primitive_groups() {
        let linter = native_linter("rule strict-boolean-expressions { set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "declare const yes: 'yes' | null; declare const maybe: '' | 'yes' | null; declare const one: 1 | null; declare const zero: 0 | 1 | null; declare const truth: true | null; declare const bool: boolean | null; if (yes) {} if (maybe) {} if (one) {} if (zero) {} if (truth) {} if (bool) {}";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["maybe", "zero", "bool"]);
    }

    #[test]
    fn strict_boolean_preserves_brand_policy_and_explicit_unknown() {
        let linter = native_linter("rule strict-boolean-expressions { set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "declare const booleanBrand: boolean & { brand: true }; declare const numberBrand: number & { brand: true }; declare const unknownValue: unknown; declare const unresolvedValue: Missing; declare const unresolvedBrand: Missing & { brand: true }; if (booleanBrand) {} if (numberBrand) {} if (unknownValue) {} if (unresolvedValue) {} if (unresolvedBrand) {}";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["numberBrand", "unknownValue"]);
        let permissive = native_linter("rule strict-boolean-expressions { set allowAny true\nset allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let diagnostics = permissive.lint(Path::new("/root/a.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["numberBrand"]);
    }

    #[test]
    fn strict_boolean_checks_asserted_logical_arguments_but_not_arguments_after_spread() {
        let linter = native_linter("rule strict-boolean-expressions { set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "declare function assert(value: unknown): asserts value; declare function second(first: unknown, value: unknown): asserts value; declare const a: string | null; declare const b: string | null; declare const args: unknown[]; assert(a || b); second(...args, a);";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["a", "b"]);
    }

    #[test]
    fn strict_boolean_combines_callback_contracts_and_reports_whole_callbacks() {
        let linter = native_linter("rule strict-boolean-expressions { set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "declare const value: boolean | null; declare function predicate(): number; declare function predicate(): string; [1].filter(() => value); [1].some(async () => true); [1].every(predicate);";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["() => value", "async () => true", "predicate"]);
        assert!(diagnostics.iter().all(|d| d.fix.is_none()));
    }

    #[test]
    fn strict_boolean_uses_constraints_in_conditions_and_callback_returns() {
        let linter = native_linter("rule strict-boolean-expressions { set allowNumber false\nset allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "function check<T extends number>(value: T) { if (value) {} [1].filter((): T => value); } function safe<T extends boolean>(value: T) { if (value) {} [1].filter((): T => value); }";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["value", "(): T => value"]);
    }

    #[test]
    fn strict_boolean_reports_disabled_null_checks_without_stopping_expression_checks() {
        let source = "declare const value: {}; if (value) {}";
        let linter = native_linter("rule strict-boolean-expressions");
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().any(|d| d.start == 0 && d.end == 0));
        assert!(diagnostics.iter().any(|d| &source[d.start..d.end] == "value"));
        let overridden = native_linter("rule strict-boolean-expressions { set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let diagnostics = overridden.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(&source[diagnostics[0].start..diagnostics[0].end], "value");
    }

    #[test]
    fn domains_distinguish_certainty_from_open_values_and_constraints() {
        let linter = native_linter("in ts_expression_statement.expression rule certain { when type_domains.truthy && !type_domains.falsy\nwarn 'certain' }");
        let source = "declare const object: {}; declare const number: number; declare const unknown: unknown; declare const any: any; declare const zero: 0x0; declare const union: 0 | {}; declare const branded: number & { brand: true }; object; number; unknown; any; zero; union; branded; function bounded<T extends {}>(value: T) { value; } function open<T>(value: T) { value; }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), source).len(), 2);
    }

    #[test]
    fn intersection_domains_respect_primitive_and_literal_constraints() {
        for (annotation, truthy, falsy) in [
            ("string & number", false, false),
            ("(string | number) & ('foo' | 123) & { brand: true }", true, false),
            ("((string & { brandA: true }) | (number & { brandB: true })) & 'foo'", true, false),
            ("number & { brand: true }", true, true),
        ] {
            let source = format!("declare const value: {annotation}; const result = /*@*/value;");
            with_native_file(&source, |file, nodes| {
                let ty = file.type_of(nodes[0]);
                let domain = file.typing().checker.value_domains(&ty);
                assert_eq!((domain.truthy, domain.falsy), (truthy, falsy), "{annotation}: {ty:?}");
            });
        }
    }

    #[test]
    fn evaluated_domains_keep_intrinsic_nullability_not_chain_short_circuiting() {
        let linter = native_linter("in ts_expression_statement.expression rule nonnull { when !evaluated_type_domains.nullish && evaluated_type_domains.non_nullish\nwarn 'nonnull' }");
        let source = "declare const source: { item: { value: string | null }; make(): {}; nullable(): {} | null } | undefined; source?.item; source?.item.value; source?.make(); source?.nullable();";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), source).len(), 2);
    }

    #[test]
    fn predicates_resolve_named_arguments_overloads_and_generic_targets() {
        let linter = native_linter("in ts_expression_statement.expression rule equivalent { let p predicate_call\nwhen p.to_target && p.from_target\nwarn 'equivalent' }");
        let source = "declare function check(skip: unknown, value: unknown): value is string; declare function generic<T>(value: unknown): value is T; declare function overloaded(value: string): value is string; declare function overloaded(value: number): value is number; declare const text: string; declare const number: number; declare const args: unknown[]; check(1, text); generic<string>(text); overloaded(number); check(...args, text); check(1, number);";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), source).len(), 3);
    }

    #[test]
    fn callable_domains_include_inferred_returns_and_keep_unknown_branches() {
        let linter = native_linter("in ts_expression_statement.expression rule constant { let returns callable_return_domains\nwhen count(returns) > 0 && all(returns, domain => domain.truthy && !domain.falsy)\nwarn 'constant' }");
        let source = "const expression = () => ({}); function body() { return {}; } function partial(value: boolean) { if (value) { return {}; } } declare const declared: () => {}; declare const variable: () => boolean; declare const mixed: (() => {}) | (() => unknown); expression; body; declared; variable; mixed; partial;";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), source).len(), 3);
    }

    #[test]
    fn condition_policy_checks_tests_chains_callbacks_and_loop_options_without_fixes() {
        let linter = native_linter("rule no-unnecessary-condition { set allowConstantLoopConditions 'always'\nset allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "declare const object: { item: {} }; if (object) {} if (!object) {} object?.item; [1].filter(() => true); while (true) {}";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 4);
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_none()));
    }

    #[test]
    fn condition_predicate_selection_keeps_truthiness_and_type_guards_independent() {
        let source = "declare function assertTruthy(value: unknown): asserts value; declare function guard(value: unknown): value is string; declare function other(value: unknown): value is string; declare const text: string; declare const unknown: unknown; assertTruthy(true); assertTruthy(unknown); guard(text); other(text); guard(0);";
        for (selection, expected) in [
            ("", vec![]),
            ("override in ts_call let no_unnecessary_condition_truthiness_assertion_checked true", vec!["true"]),
            ("override in ts_call let no_unnecessary_condition_predicate_checked true", vec!["text", "text"]),
            ("override in ts_call let no_unnecessary_condition_truthiness_assertion_checked true\noverride in ts_call let no_unnecessary_condition_predicate_checked true", vec!["true", "text", "text"]),
            ("override in ts_call let no_unnecessary_condition_truthiness_assertion_checked false\noverride in ts_call let no_unnecessary_condition_predicate_checked false", vec![]),
            ("override in ts_call let no_unnecessary_condition_predicate_checked callee.text == 'guard'", vec!["text"]),
        ] {
            let linter = native_linter(&format!("rule no-unnecessary-condition {{ set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }}\n{selection}"));
            let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
            assert_eq!(diagnostics.iter().map(|diagnostic| &source[diagnostic.start..diagnostic.end]).collect::<Vec<_>>(), expected, "{selection}: {diagnostics:#?}");
        }
    }

    #[test]
    fn condition_loop_modes_distinguish_static_true_from_allowed_literal_syntax() {
        let source = "declare const yes: true; declare const no: false; while ((true)) {} while (false) {} while (1) {} while (0) {} while (yes) {} while (no) {} while (2) {} for (; true;) {} do {} while (false);";
        for (mode, expected) in [("never", 9), ("always", 6), ("only-allowed-literals", 3)] {
            let linter = native_linter(&format!("rule no-unnecessary-condition {{ set allowConstantLoopConditions '{mode}'\nset allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }}"));
            let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
            assert_eq!(diagnostics.len(), expected, "{mode}: {diagnostics:#?}");
        }
    }

    #[test]
    fn strict_null_override_changes_only_the_bootstrap_warning() {
        for (strict, allow, expected) in [(false, false, 2), (false, true, 1), (true, false, 1), (true, true, 1)] {
            let config = format!(r#"{{"compilerOptions":{{"strictNullChecks":{strict}}}}}"#);
            let messages = lint_project(
                "condition-strict-null-override",
                &[("tsconfig.json", &config), ("main.ts", "if (true) {}")],
                "main.ts",
                &format!("rule no-unnecessary-condition {{ set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing {allow} }}"),
            );
            assert_eq!(messages.len(), expected, "strict={strict}, allow={allow}: {messages:#?}");
        }
    }

    #[test]
    fn condition_policy_distinguishes_index_fallback_from_declared_properties() {
        let linter = native_linter("rule no-unnecessary-condition { set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "declare const bag: { value: string; [key: string]: string } | null; declare const key: string; bag?.[key]?.trim(); bag?.value?.trim();";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn condition_policy_respects_unchecked_mapped_fallback_and_chain_prefix() {
        let linter = native_linter("rule no-unnecessary-condition { set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let fallback = "declare const foo: { key?: { [key: string]: { key: string } } }; foo.key?.someKey?.key;";
        assert!(linter.lint(Path::new("/root/a.ts"), fallback).is_empty());
        let dictionary = "function get(dict: { [key: string]: { foo: string } }, key: string) { if (dict[key]) { return dict[key].foo; } }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), dictionary).len(), 1);
        let array = "declare const array: { x: { y: { z: object } } }[]; array[42]?.x?.y?.z;";
        assert!(linter.lint(Path::new("/root/a.ts"), array).is_empty());
        for (unchecked, expected) in [(false, 1), (true, 0)] {
            let config = format!(r#"{{"compilerOptions":{{"strictNullChecks":true,"noUncheckedIndexedAccess":{unchecked}}}}}"#);
            for (source, expected) in [
                ("type A = { [name in string]: { a: 1 } }; declare const a: A; a.a?.a;", expected),
                ("type A = { [name in string]?: { [name in string]: { a: 1 } } }; declare const a: A; a.a?.a?.a;", 0),
            ] {
                let diagnostics = lint_project("condition-mapped-unchecked", &[("tsconfig.json", &config), ("main.ts", source)],
                    "main.ts", "rule no-unnecessary-condition");
                assert_eq!(diagnostics.len(), expected, "unchecked={unchecked}, source={source}: {diagnostics:#?}");
            }
        }
    }
}
