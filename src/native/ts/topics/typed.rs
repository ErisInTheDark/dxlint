//! typescript-eslint's type-aware rules (`std/ts/typed`).

use crate::expr::facts::Fact;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field, Kind};
use crate::native::ts::types::{Checker, Ty};

mod template;
mod type_properties;
mod type_structure;
mod enum_types;
mod properties;
mod unsafe_types;
mod awaitability;
mod assertions;
mod accessors;
mod conversions;
mod constituents;
mod regexp;
mod redundant;
mod this_return;
mod nullish;
mod transfers;
mod assignments;
mod arguments;
mod enums;
pub(super) mod specifiers;
mod errors;
mod promises;
mod empty_types;
mod defaults;
mod return_await;

fn is_typescript(file: &NativeFile<'_>) -> bool {
    file.path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| matches!(extension, "ts" | "tsx" | "mts" | "cts"))
}

pub const FACTS: &[Fact] = &[
    Fact {
        name: "type_flow",
        doc: "native expressions: type_flow(site, source) gives source and expected type relations for declared, contextual and heritage contracts; incomplete records indicate traversal limits",
        get: |f, n| f.with_file(enum_types::flow, n),
    },
    Fact {
        name: "property_info",
        doc: "native expressions: static member or index access declaration presence and winning declaration visibility, even when its value type is unresolved",
        get: |f, n| f.with_file(properties::property_info, n),
    },
    Fact {
        name: "index_key_types",
        doc: "native values and types: constrained index-signature key domains as text and string_like records, inherited domains included",
        get: |f, n| f.with_file(properties::index_key_types, n),
    },
    Fact {
        name: "type_structure",
        doc: "native values and types: constrained recursive kind, parts, names, retained aliases, declaration/value provenance, call/construct/property/iteration metadata, binding-backed property writes and queried winning member declarations",
        get: |f, n| f.with_file(|file, node| {
            let args = crate::expr::facts::args();
            let names = args.first().map(|value| value.items().iter().map(ToString::to_string).collect::<Vec<_>>()).unwrap_or_default();
            type_structure::structure(file, node, &names)
        }, n),
    },
    Fact {
        name: "type_has_void",
        doc: "native values and types: a union or intersection constituent is void, distinct from undefined",
        get: |f, n| f.with_file(|file, node| Value::Bool(type_properties::has_void(&file.type_of(node))), n),
    },
    Fact {
        name: "type_literal_values",
        doc: "native values and types: literal union/intersection constituents as type/value records; bigint values retain literal text",
        get: |f, n| f.with_file(|file, node| type_properties::literal_values(&file.type_of(node)), n),
    },
    Fact {
        name: "type_is_string_like",
        doc: "native values and types: every constrained union arm is provably string-like, excluding any and unresolved intersections",
        get: |f, n| f.with_file(|file, node| Value::Bool(type_properties::string_like(&file.constrained_type_of(node), &file.typing().checker)), n),
    },
    Fact {
        name: "type_is_parameter",
        doc: "native values and types: the type is an open type parameter, before applying its constraint",
        get: |f, n| f.with_file(|file, node| Value::Bool(matches!(file.type_of(node), Ty::Param(_))), n),
    },
    Fact {
        name: "type_has_enum",
        doc: "native values and types: a constrained union or intersection constituent has enum identity",
        get: |f, n| f.with_file(|file, node| Value::Bool(type_properties::has_enum(&file.constrained_type_of(node), &file.typing().checker)), n),
    },
    Fact {
        name: "callable",
        doc: "native expressions: the type supplies call signatures (every union arm, or an intersection arm)",
        get: |f, n| f.with_file(|file, node| Value::Bool(file.typing().checker.callable_type(&file.type_of(node))), n),
    },
    Fact {
        name: "type_iterable",
        doc: "native expressions: constrained type supports synchronous iteration; null when type analysis is unresolved",
        get: |f, n| f.with_file(|file, node| file.typing().checker.iterable_type(&file.constrained_type_of(node)).map_or(Value::Null, Value::Bool), n),
    },
    Fact {
        name: "type_opaque",
        doc: "native expressions: type computation is unsupported or unresolved, distinct from explicit unknown and open type parameters",
        get: |f, n| f.with_file(|file, node| Value::Bool(file.type_of(node).is_opaque()), n),
    },
    Fact {
        name: "builtin_type",
        doc: "native expressions: builtin_type(name) asks whether the type is the named standard-library type or derives from it",
        get: |f, n| f.with_file(|file, node| {
            let args = crate::expr::facts::args();
            args.first().map_or(Value::Null, |name| Value::Bool(file.typing().checker.is_builtin_type_or_base(&file.type_of(node), &name.to_string())))
        }, n),
    },
    Fact {
        name: "builtin_constructor",
        doc: "native expressions: builtin_constructor(name) asks whether the value is a standard-library constructor or derives from it",
        get: |f, n| f.with_file(|file, node| {
            let args = crate::expr::facts::args();
            args.first().map_or(Value::Null, |name| Value::Bool(file.typing().checker.is_builtin_constructor_or_base(&file.type_of(node), &name.to_string())))
        }, n),
    },
    Fact {
        name: "static_string",
        doc: "native expressions: constant evaluated text, including templates, concatenation and unmodified variables; null when unknown",
        get: |f, n| f.with_file(|file, node| file.static_string(node).map_or(Value::Null, Value::from), n),
    },
    Fact {
        name: "returned_awaitables",
        doc: "native async functions: returned branch certainty and error/resource handling context",
        get: |f, n| f.with_file(return_await::findings, n),
    },
    Fact {
        name: "useless_default",
        doc: "native defaults: source property or contextual parameter cannot use the fallback",
        get: |f, n| f.with_file(defaults::finding, n),
    },
    Fact {
        name: "unsafe_assertion",
        doc: "native assertions: unsafe generic any transfers, narrowing and type parameter constraints",
        get: |f, n| f.with_file(assertions::unsafe_assertion, n),
    },
    Fact {
        name: "generated_empty_object",
        doc: "native type operations: provably resolved empty object, excluding unresolved mapped keys",
        get: |f, n| f.with_file(empty_types::generated, n),
    },
    Fact {
        name: "missing_await",
        doc: "native async functions: no own await, awaited loop/using, thenable return or async yield",
        get: |f, n| f.with_file(promises::missing_await, n),
    },
    Fact {
        name: "unsafe_catch_callbacks",
        doc: "native calls: inline rejection handlers whose first parameter is not unknown",
        get: |f, n| f.with_file(promises::unsafe_catch, n),
    },
    Fact {
        name: "promise_function",
        doc: "native functions: return-type name clauses, inferred hybrid policy and function-head offset",
        get: |f, n| f.with_file(promises::function, n),
    },
    Fact {
        name: "error_value",
        doc: "native values: error/any/unknown/undefined category and caught-binding provenance",
        get: |f, n| f.with_file(errors::value, n),
    },
    Fact {
        name: "type_specifier_groups",
        doc: "native values: allowed-type candidates per union arm, with named file/lib/package provenance",
        get: |f, n| f.with_file(specifiers::groups, n),
    },
    Fact {
        name: "mixed_enum_offset",
        doc: "native enums: first mixed string/number member, including merged local declarations",
        get: |f, n| f.with_file(enums::mixed, n),
    },
    Fact {
        name: "unsafe_arguments",
        doc: "native calls, constructions and tagged templates: unsafe argument and spread transfers",
        get: |f, n| f.with_file(arguments::findings, n),
    },
    Fact {
        name: "unsafe_assignments",
        doc: "native assignments, declarations, properties and spreads: unsafe any/generic/destructuring transfers",
        get: |f, n| f.with_file(assignments::findings, n),
    },
    Fact {
        name: "unsafe_return",
        doc: "native returns and arrow bodies: unsafe any category or generic sender/receiver types",
        get: |f, n| f.with_file(transfers::returned, n),
    },
    Fact {
        name: "this_return_type_offset",
        doc: "native class functions: class annotation offset when only this (not the class instance type) is returned",
        get: |f, n| f.with_file(this_return::offset, n),
    },
    Fact {
        name: "redundant_type_constituents",
        doc: "native unions and intersections: top/bottom overrides and primitive/literal dominance",
        get: |f, n| f.with_file(redundant::findings, n),
    },
    Fact {
        name: "regexp_exec_offset",
        doc: "native calls: match property offset when its pattern is provably nonglobal",
        get: |f, n| f.with_file(regexp::exec, n),
    },
    Fact {
        name: "duplicate_type_constituents",
        doc: "native unions and intersections: duplicates with first text and optional-parameter undefined",
        get: |f, n| f.with_file(constituents::duplicates, n),
    },
    Fact {
        name: "unnecessary_type_conversion",
        doc: "native calls and operators: primitive type, conversion idiom and report offset",
        get: |f, n| f.with_file(conversions::finding, n),
    },
    Fact {
        name: "non_null_assertion",
        doc: "native assertions: removes only null and undefined from an otherwise identical type",
        get: |f, n| f.with_file(assertions::non_null, n),
    },
    Fact {
        name: "reduce_type_assertion",
        doc: "native assertions: array reduce initialiser safely accepts a type parameter",
        get: |f, n| f.with_file(assertions::reduce, n),
    },
    Fact {
        name: "accessor_type_mismatch",
        doc: "native getter return annotations: incompatible with the paired setter parameter",
        get: |f, n| f.with_file(accessors::mismatch, n),
    },
    Fact {
        name: "nonasync_for_of",
        doc: "native for-of: awaited loop lacks an async iterator",
        get: |f, n| f.with_file(awaitability::for_of, n),
    },
    Fact {
        name: "nonasync_using",
        doc: "native initialisers: awaited using lacks an async dispose symbol",
        get: |f, n| f.with_file(awaitability::using, n),
    },
    Fact {
        name: "nonawaitable_type",
        doc: "native expressions: constrained type has no possibly awaitable union arm",
        get: |f, n| f.with_file(awaitability::nonawaitable, n),
    },
    Fact {
        name: "nonawaitable_iterable",
        doc: "native expressions: an iterable has a definitely nonawaitable element type",
        get: |f, n| f.with_file(awaitability::iterable, n),
    },
    Fact {
        name: "unsafe_type",
        doc: "native expressions: any or unresolved annotation provenance, distinct from unknown",
        get: |f, n| f.with_file(unsafe_types::kind, n),
    },
    Fact {
        name: "unsafe_call_type",
        doc: "native call targets: any, unresolved annotation or unspecialised Function",
        get: |f, n| f.with_file(unsafe_types::call, n),
    },
    Fact {
        name: "unsafe_this_type",
        doc: "native expressions: unsafe category of the root this expression",
        get: |f, n| f.with_file(unsafe_types::this_kind, n),
    },
    Fact {
        name: "unsafe_member_key",
        doc: "native computed keys: unsafe type, unparenthesised text and report offset",
        get: |f, n| f.with_file(unsafe_types::key, n),
    },
    Fact {
        name: "template_type_checks",
        doc: "native expressions: interpolation requirements, every clause needs an enabled option or allowed named type",
        get: |f, n| f.with_file(template::checks, n),
    },
    Fact {
        name: "template_type_display",
        doc: "native expressions: the constrained interpolation type for diagnostic messages",
        get: |f, n| f.with_file(template::display, n),
    },
    Fact {
        name: "array_type",
        doc: "native layers: every union arm (or an intersection arm) is an array or tuple",
        get: |f, n| f.with_file(array_type, n),
    },
    Fact {
        name: "string_array_type",
        doc: "native layers: an array or tuple whose every element is a string",
        get: |f, n| f.with_file(string_array_type, n),
    },
    Fact {
        name: "void_argument_kind",
        doc: "on a void unary expression: the effective argument is a call, assignment or other expression",
        get: |f, n| f.with_file(void_argument_kind, n),
    },
    Fact {
        name: "nullish_coalescing",
        doc: "on logical, assignment, conditional and if nodes: nullish replacement and truthiness context",
        get: |f, n| f.with_file(nullish::finding, n),
    },
];

fn is_array(t: &Ty, ck: &Checker<'_>) -> bool {
    match t {
        Ty::Array(_) | Ty::Tuple(_) => true,
        Ty::Named(sym, _) => {
            ck.is_builtin_symbol(sym, "Array") || ck.is_builtin_symbol(sym, "ReadonlyArray")
        }
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|t| is_array(t, ck)),
        Ty::Inter(parts) => parts.iter().any(|t| is_array(t, ck)),
        _ => false,
    }
}

fn string_only(t: &Ty) -> bool {
    match t {
        Ty::Prim("string") | Ty::Lit(_, "string") => true,
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(string_only),
        _ => false,
    }
}

fn is_string_array(t: &Ty, ck: &Checker<'_>) -> bool {
    match t {
        Ty::Array(element) => string_only(&element.element),
        Ty::Tuple(elements) => elements.iter().all(string_only),
        Ty::Named(sym, args) if ck.is_builtin_symbol(sym, "Array") || ck.is_builtin_symbol(sym, "ReadonlyArray") => {
            args.first().is_some_and(string_only)
        }
        _ => false,
    }
}

fn array_type(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    Value::Bool(is_array(&file.constrained_type_of(node), &file.typing().checker))
}

fn string_array_type(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    Value::Bool(is_string_array(&file.constrained_type_of(node), &file.typing().checker))
}

fn void_argument_kind(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !s.is(node, Kind::Unary) || s.word(node, Field::Op) != Some("void") {
        return Value::Null;
    }
    let Some(mut expr) = s.get(node, Field::Arg) else {
        return Value::Null;
    };
    loop {
        match s.kind(expr) {
            Some(Kind::Paren | Kind::As | Kind::Satisfies | Kind::NonNull | Kind::TypeAssertion) => {
                let Some(inner) = s.get(expr, Field::Expr) else { break };
                expr = inner;
            }
            Some(Kind::Sequence) => {
                let Some(last) = s.all(expr, Field::Expr).last() else { break };
                expr = last;
            }
            _ => break,
        }
    }
    Value::from(match s.kind(expr) {
        Some(Kind::Call | Kind::New) => "call",
        Some(Kind::Assign) => "assignment",
        _ => "other",
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn optional_chains_report_calls_and_computed_access_once() {
        let linter = native_linter("rule prefer-optional-chain");
        for (source, chain) in [
            ("declare const foo: (() => number) | null; foo && foo();", "foo && foo()"),
            ("declare const foo: any; declare const key: string; foo && foo[key] && foo[key].value;", "foo && foo[key] && foo[key].value"),
            ("declare const foo: any; foo.bar! && foo.bar.value;", "foo.bar! && foo.bar.value"),
        ] {
            let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
            let start = source.find(chain).unwrap();
            assert_eq!(diagnostics.iter().map(|d| (d.start, d.end)).collect::<Vec<_>>(), [(start, start + chain.len())], "{source}");
        }
    }

    #[test]
    fn optional_chains_preserve_guard_semantics_and_independent_segments() {
        let linter = native_linter("rule prefer-optional-chain");
        for (source, expected) in [
            ("declare const foo: false | { value: number }; foo && foo.value;", 0),
            ("declare const foo: 0 | { value: number }; foo && foo.value;", 0),
            ("declare const foo: '' | { value: number }; foo && foo.value;", 0),
            ("declare const foo: 0n | { value: number }; foo && foo.value;", 0),
            ("declare const foo: void | (() => void); foo && foo();", 0),
            ("declare const foo: { left: number; right: number } | null; foo != null && foo.left === foo.right;", 0),
            ("declare const foo: { value: number } | null; foo && foo.value === undefined;", 0),
            ("declare const foo: { value: number } | null | undefined; foo !== null && foo !== undefined && foo.value;", 1),
            ("declare const foo: any; declare const bar: any; foo && foo() || bar && bar();", 2),
            ("declare const foo: any; foo && (foo.bar && foo.bar.value);", 1),
            ("declare const foo: any; (foo?.bar).value && foo.bar.value.next;", 0),
            ("declare const foo: any; (foo ?? {}).value;", 1),
        ] {
            assert_eq!(linter.lint(Path::new("/root/file.ts"), source).len(), expected, "{source}");
        }
    }

    #[test]
    fn optional_chains_obey_primitive_and_nullish_options() {
        let source = "declare const foo: string; foo && foo.length;";
        assert_eq!(native_linter("rule prefer-optional-chain").lint(Path::new("/root/file.ts"), source).len(), 1);
        for options in ["set checkString false", "set requireNullish true"] {
            let linter = native_linter(&format!("rule prefer-optional-chain {{ {options} }}"));
            assert!(linter.lint(Path::new("/root/file.ts"), source).is_empty(), "{options}");
        }
    }

    #[test]
    fn template_interpolations_report_each_redundant_value() {
        let linter = native_linter("rule no-unnecessary-template-expression");
        let source = "const value = `before ${1} middle ${true} after`;";
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        let expected: Vec<_> = ["${1}", "${true}"].into_iter().map(|text| {
            let start = source.find(text).unwrap();
            (start, start + text.len())
        }).collect();
        assert_eq!(diagnostics.iter().map(|d| (d.start, d.end)).collect::<Vec<_>>(), expected);
    }

    #[test]
    fn template_interpolations_preserve_comments_tags_and_line_spacing() {
        let linter = native_linter("rule no-unnecessary-template-expression");
        for source in [
            "const value = `before ${/* keep */ 1} after`;",
            "const value = `before ${1 /* keep */} after`;",
            "const value = tag`${1}`;",
            "const value = `before ${'  '}\n after`;",
            "const value = `before ${''}\n after`;",
            "declare const value: string; const result = `before ${value} after`;",
        ] {
            assert!(linter.lint(Path::new("/root/file.ts"), source).is_empty(), "{source}");
        }
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "const value = `before ${'x'}\n after`;").len(), 1);
    }

    #[test]
    fn template_type_interpolations_preserve_parameter_and_enum_identity() {
        let linter = native_linter("rule no-unnecessary-template-expression");
        for (source, expected) in [
            ("type Plain = `${string}`;", 1),
            ("type Generic<T extends string> = `${T}`;", 0),
            ("enum E { A = 'a' } type Enum = `${E.A}`;", 0),
            ("type Literal = `prefix${1}suffix`;", 1),
            ("type Whitespace = `prefix${''}\n`;", 0),
            ("type Unresolved = `${Missing.A & string}`;", 0),
        ] {
            assert_eq!(linter.lint(Path::new("/root/file.ts"), source).len(), expected, "{source}");
        }
    }

    #[test]
    fn unresolved_annotations_are_distinct_from_unknown_values() {
        let linter = native_linter("rule no-unsafe-call");
        let code = r#"
            declare let missing: Missing;
            declare let uncertain: unknown;
            missing();
            uncertain();
        "#;
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn block_aliases_use_their_lexical_declarations() {
        let linter = native_linter(r#"
            in ts_call.callee rule callable {
                when contains(default(type_flags, []), "function")
                error "callable"
            }
        "#);
        let code = "{ type Alias = () => number; declare const x: Alias; x(); } declare const y: Alias; y();";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn optional_any_access_restarts_reporting_after_the_optional_member() {
        let linter = native_linter("rule no-unsafe-member-access { set allowOptionalChaining true }");
        let code = r#"
            declare const value: any;
            value?.middle.inner;
            value?.outer.middle.inner;
            value.outer?.middle.inner;
        "#;
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 4);
    }

    #[test]
    fn compound_assignment_preserves_any_for_computed_member_keys() {
        let linter = native_linter("rule no-unsafe-member-access:key");
        let code = "declare const x: { a: number }; declare let y: any; x[(y += 1)]; x[(y *= 1)];";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn optional_object_properties_do_not_make_awaitability_unknown() {
        let linter = native_linter("rule await-thenable");
        let code = "declare const value: { a?: number }; await value;";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].start, code.find("await").unwrap());
    }

    #[test]
    fn promise_aggregators_check_constrained_elements_and_skip_elisions() {
        let linter = native_linter("rule await-thenable:aggregate\nrule await-thenable:iterable");
        let code = "Promise.all([,]); function f<T extends Promise<unknown>>(x: T[]) { Promise.all(x); } declare const xs: (number | Promise<number>)[]; Promise.all(xs);";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].start, code.rfind("xs").unwrap());
    }

    #[test]
    fn thenable_protocol_requires_a_callback_parameter() {
        let linter = native_linter("rule await-thenable");
        let code = "declare const bad: { then(): void }; declare const good: { then(cb: () => void): void }; await bad; await good;";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn assertions_require_identical_nonnull_types_and_assignable_reduce_initialisers() {
        let linter = native_linter("rule non-nullable-type-assertion-style\nrule prefer-reduce-type-parameter");
        let code = "declare const x: string | null; x as string; x as number; declare const a: number[]; a.reduce((x, y) => x, [] as number[]); a.reduce((x, y) => x, [] as number);";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn nonnull_assertions_preserve_generic_object_arguments() {
        let linter = native_linter("rule non-nullable-type-assertion-style");
        let code = "type Box<T> = { value: T }; declare const x: Box<string> | null; x as Box<string>; x as Box<number>;";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn duplicate_types_keep_readonly_identity_and_collapse_nested_duplicates() {
        let linter = native_linter("rule no-duplicate-type-constituents");
        let code = "type R = readonly string[] | string[]; type A = 'A'; type T = A | (A | A); type U = string | number | string;";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn accessor_pairs_stay_within_their_owner_and_require_a_proven_mismatch() {
        let linter = native_linter("rule related-getter-setter-pairs");
        let code = "interface A { get x(): string; set x(v: number); get y(): number; set y(v: number | string); } interface B { get x(): number; set x(v: number); }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn conversions_keep_bigint_arithmetic_and_signed_integer_types() {
        let linter = native_linter("rule no-unnecessary-type-conversion");
        assert_eq!(linter.lint(Path::new("/root/a.ts"), "2n * BigInt(2n + 2n); ~~ -1;").len(), 2);
    }

    #[test]
    fn regexp_exec_recognises_branded_strings_but_rejects_invalid_patterns() {
        let linter = native_linter("rule prefer-regexp-exec");
        let code = "type Safe = string & { brand: void }; function f(s: Safe) { s.match('ok'); } 'x'.match('['); 'x'.match('g');";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn redundant_literals_report_inner_type_not_parentheses() {
        let linter = native_linter("rule no-redundant-type-constituents:*");
        let code = "type T = number | (0 | (0 | 1));";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].start, code.find("(0").unwrap() + 1);
    }

    #[test]
    fn mixed_union_arms_do_not_prove_an_intersection_primitive_redundant() {
        let linter = native_linter("rule no-redundant-type-constituents:*");
        assert!(linter.lint(Path::new("/root/a.ts"), "type T = string & ('x' | { a: number });").is_empty());
    }

    #[test]
    fn callback_parameters_read_declared_and_aliased_function_contexts() {
        let linter = native_linter("in ts_expression_statement.expression rule typed {\nwhen contains(default(type_flags, []), \"string\")\ninfo \"typed\"\n}");
        let code = "const f: (x: string) => string = x => { x; return x; }; type Fn = (x: string) => void; declare function take(f: Fn): void; take(x => { x; });";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn awaited_types_follow_nested_thenable_callback_values() {
        let linter = native_linter("in ts_expression_statement.expression rule typed {\nwhen contains(default(type_flags, []), \"string\")\ninfo \"typed\"\n}");
        let code = "interface Task<T> { then(cb: (value: T) => void): void; } declare const p: Task<Task<string>>; await p;";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn explicit_constructor_type_arguments_select_a_generic_overload() {
        let linter = native_linter("in ts_expression_statement.expression rule typed {\nwhen contains(default(type_flags, []), \"string\")\ninfo \"typed\"\n}");
        let code = "interface Box<K, V> { key: K; value: V; } interface Ctor { new(): Box<any, any>; new<K, V>(): Box<K, V>; } declare const Maker: Ctor; new Maker<string, number>().key;";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn this_parameters_override_dynamic_functions_but_arrows_keep_class_this() {
        let linter = native_linter("in ts_expression_statement.expression rule typed {\nwhen contains(default(type_flags, []), \"string\")\ninfo \"typed\"\n}");
        let code = "function f(this: { value: string }) { this.value; } class C { value: string; m() { (() => { this.value; })(); function nested() { this.value; } } }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn typeof_narrows_unknown_only_on_the_matching_branch() {
        let linter = native_linter(r#"
            in ts_identifier rule narrowed {
                when text == "x" && contains(default(type_flags, []), "string")
                error "narrowed"
            }
        "#);
        let code = "declare const x: unknown; if (typeof x === 'string') { x; } else { x; }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn constrained_arrays_cover_parameters_and_uninferred_call_results() {
        let linter = native_linter(r#"
            in ts_unary rule deletion {
                when operator == "delete" && argument.object_array_type == true
                error "deletion"
            }
        "#);
        let code = r#"
            declare function getArray<T extends number[]>(): T;
            delete getArray()[0];
            function remove<T extends number[]>(a: T) { delete a[0]; }
            function unknown<T>(a: T) { delete a[0]; }
        "#;
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn template_arrays_check_elements_and_intersections_accept_string_arms() {
        let linter = native_linter("rule restrict-template-expressions { set allowArray true }");
        let code = r#"
            declare const stringBrand: string & { brand: "string" };
            `${stringBrand}`;
            `${[{}, {}]}`;
            `${["a", "b"]}`;
        "#;
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].start, code.find("[{}, {}]").expect("array expression"));
    }

}
