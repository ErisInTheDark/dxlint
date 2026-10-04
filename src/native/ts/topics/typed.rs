//! typescript-eslint's type-aware rules (`std/ts/typed`).

use crate::expr::facts::Fact;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field, Kind};
use crate::native::ts::types::{Checker, Ty};

mod template;
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
mod implied_eval;
mod specifiers;
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
        name: "throw_policy",
        doc: "native thrown values: error/any/unknown/undefined category and caught-binding provenance",
        get: |f, n| f.with_file(errors::thrown, n),
    },
    Fact {
        name: "type_specifier_groups",
        doc: "native values: allowed-type candidates per union arm, with named file/lib/package provenance",
        get: |f, n| f.with_file(specifiers::groups, n),
    },
    Fact {
        name: "implied_eval",
        doc: "native calls and constructions: global eval-like handler or Function constructor",
        get: |f, n| f.with_file(implied_eval::finding, n),
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
        name: "promise_aggregator",
        doc: "native calls: a built-in Promise aggregation method",
        get: |f, n| f.with_file(awaitability::aggregator, n),
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
        name: "array_delete",
        doc: "on a delete unary expression: the target is a member of an array or tuple",
        get: |f, n| f.with_file(array_delete, n),
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

fn array_delete(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !s.is(node, Kind::Unary) || s.word(node, Field::Op) != Some("delete") {
        return Value::Bool(false);
    }
    let Some(target) = s.get(node, Field::Arg).map(|id| s.unparen(id)) else {
        return Value::Bool(false);
    };
    if !matches!(s.kind(target), Some(Kind::Member | Kind::Index)) {
        return Value::Bool(false);
    }
    let Some(object) = s.get(target, Field::Object) else {
        return Value::Bool(false);
    };
    Value::Bool(s.is(object, Kind::Array) || is_array(&file.constrained_type_of(object), &file.typing().checker))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

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
                when array_delete == true
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
