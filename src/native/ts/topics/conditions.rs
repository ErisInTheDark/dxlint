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
        doc: "native expressions: possible truthy, falsy and nullish values of the constrained type",
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
        doc: "native expressions: value domains of every callable return, including locally inferred function bodies",
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
    let checker = &file.typing().checker;
    let ValueDomains { truthy, falsy, nullish, non_nullish, opaque } = checker.value_domains(ty);
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
    Value::list(file.callable_return_types(&file.constrained_type_of(node)).iter().map(|ty| domain_record(file, ty)).collect())
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{native_linter, with_native_file};
    use std::path::Path;

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
        let linter = native_linter("rule no-unnecessary-condition { set allowConstantLoopConditions true\nset allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "declare const object: { item: {} }; if (object) {} if (!object) {} object?.item; [1].filter(() => true); while (true) {}";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 4);
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_none()));
    }

    #[test]
    fn condition_policy_distinguishes_index_fallback_from_declared_properties() {
        let linter = native_linter("rule no-unnecessary-condition { set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let source = "declare const bag: { value: string; [key: string]: string } | null; declare const key: string; bag?.[key]?.trim(); bag?.value?.trim();";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn condition_policy_respects_unchecked_fallback_and_chain_prefix() {
        let linter = native_linter("rule no-unnecessary-condition { set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true }");
        let fallback = "declare const foo: { key?: { [key: string]: { key: string } } }; foo.key?.someKey?.key;";
        assert!(linter.lint(Path::new("/root/a.ts"), fallback).is_empty());
        let dictionary = "function get(dict: { [key: string]: { foo: string } }, key: string) { if (dict[key]) { return dict[key].foo; } }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), dictionary).len(), 1);
        let array = "declare const array: { x: { y: { z: object } } }[]; array[42]?.x?.y?.z;";
        assert!(linter.lint(Path::new("/root/a.ts"), array).is_empty());
    }
}
