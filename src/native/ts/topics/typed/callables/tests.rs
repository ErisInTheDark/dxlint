use std::hash::{Hash, Hasher};

use crate::engine::tests::lint_project;

#[test]
fn callable_contract_paths_ignore_unrequested_receiver_structure() {
    let mut source = String::from("interface Receiver { (): void; unrelated: Level0 } ");
    for level in 0..70 {
        source.push_str(&format!("interface Level{level} {{ next: Level{} }} ", level + 1));
    }
    source.push_str("interface Level70 { finish: () => void } declare function take(value: Receiver): void; take(() => 1);");
    let lint = crate::engine::tests::native_linter(r#"
        in ts_call.argument rule complete-root {
            each contract in callable_contracts(parent, node, [])
            when contract.complete
            warn "complete"
        }
    "#);
    let reports = lint.lint(std::path::Path::new("/root/file.ts"), &source);
    assert_eq!(reports.len(), 1, "{reports:?}");
}

#[test]
fn callable_contract_paths_select_only_the_requested_callable() {
    let source = "interface Receiver { selected: { callback: () => void }; unrelated: () => void } const value: Receiver = { selected: { callback: () => 1 }, unrelated: () => 2 };";
    for (path, expected) in [
        ("[]", 0),
        ("[\"selected\", \"callback\"]", 1),
        ("[\"missing\"]", 0),
    ] {
        let lint = crate::engine::tests::native_linter(&format!(r#"
            in ts_declarator.init rule selected-contract {{
                when {{ value callable_contracts(parent, node, {path}); count }} == {expected}
                warn "selected"
            }}
        "#));
        let reports = lint.lint(std::path::Path::new("/root/file.ts"), source);
        assert_eq!(reports.len(), 1, "{path}: {reports:?}");
    }
}

#[test]
fn callable_contract_limits_remain_visible_after_receiver_fallback() {
    let source = format!("interface Many {{ {} }} declare const take: Many; take(() => 1);",
        "(callback: () => void): void;".repeat(crate::native::ts::types::Checker::CONTRACT_WORK_LIMIT + 1));
    let lint = crate::engine::tests::native_linter(r#"
        in ts_call.argument rule partial-contract {
            each contract in callable_contracts(parent, node)
            when !contract.complete
            warn "partial"
        }
    "#);
    let reports = lint.lint(std::path::Path::new("/root/file.ts"), &source);
    assert_eq!(reports.len(), 1, "{reports:?}");
}

fn check_contract(body: &str, variant: &str, expected: usize, jsx: bool) {
    for rule in ["rule no-misused-promises".to_owned(), format!("rule no-misused-promises:{variant}")] {
        check_rules(body, &rule, expected, jsx);
    }
}

fn check_rules(body: &str, rule: &str, expected: usize, jsx: bool) {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (body, rule, jsx).hash(&mut hash);
    let main = if jsx { "main.tsx" } else { "main.ts" };
    let reports = lint_project(&format!("promise-contract-{:016x}", hash.finish()), &[
        ("tsconfig.json", r#"{"compilerOptions":{"lib":["fixture"],"types":[]}}"#),
        ("node_modules/typescript/lib/lib.fixture.d.ts", "interface Promise<T> { then(callback: (value: T) => unknown): unknown; finally(callback?: (() => void) | undefined | null): Promise<T> } type Record<K extends keyof any, T> = { [P in K]: T };"),
        (main, body),
    ], main, rule);
    assert_eq!(reports.len(), expected, "{rule}: {body}: {reports:?}");
}

#[test]
fn promise_static_members_do_not_override_instance_contracts() {
    let body = r#"
        class Base { method(): void {} field = (): void => {}; accessor callback = (): void => {} }
        class Static extends Base { static async method() {} static field = async () => {}; static accessor callback = async () => {} }
        class Instance extends Base { async method() {} field = async () => {}; accessor callback = async () => {} }
    "#;
    check_contract(body, "void-inherited-methods", 3, false);
    let explicit = "class Base { field = (): void => {} } class Child extends Base { static field: () => void = async () => {} }";
    check_rules(explicit, "rule no-misused-promises", 1, false);
    check_rules(explicit, "rule no-misused-promises:void-inherited-methods", 0, false);
}

#[test]
fn promise_finally_callbacks_allow_promise_returns() {
    check_contract(r#"
        declare const promise: Promise<void>;
        promise.finally(async () => {});
        const method = "finally";
        promise[method](async () => {});
        (promise.finally)(async () => {});
        interface Derived extends Promise<void> {}
        declare const derived: Derived;
        derived.finally(async () => {});
    "#, "void-arguments", 0, false);
    check_contract(r#"
        interface Thenable { then(callback: (value: number) => unknown): unknown; finally(callback: () => void): void }
        declare const custom: Thenable;
        custom.finally(async () => {});
        const method = "finally";
        custom[method](async () => {});
    "#, "void-arguments", 2, false);
    check_contract(r#"
        namespace custom {
            export interface Promise<T> { then(callback: (value: T) => unknown): unknown; finally(callback: () => void): void }
            declare const promise: Promise<number>;
            promise.finally(async () => {});
        }
    "#, "void-arguments", 1, false);
}

#[test]
fn promise_shorthand_properties_keep_declared_and_returned_consumers() {
    for (body, expected) in [
        ("const callback = async () => {}; const object: { callback: () => void } = { callback };", 1),
        ("const callback = async () => {}; function make(): { callback: () => void } { return { callback }; }", 1),
        ("const callback = async () => {}; const make: () => Record<string, () => void> = () => ({ callback });", 1),
        ("const callback = async () => {}; const object: { callback: () => Promise<void> } = { callback };", 0),
        ("const callback = () => 1; const object: { callback: () => void } = { callback };", 0),
    ] {
        check_contract(body, "void-properties", expected, false);
    }
}

#[test]
fn promise_nullable_callbacks_keep_known_callable_arms_at_each_transfer() {
    for (body, variant, jsx, expected) in [
        ("declare const callback: undefined | (() => Promise<void>); declare function take(cb: undefined | (() => void)): void; take(callback);", "void-arguments", false, 1),
        ("declare const callback: undefined | (() => Promise<void>); const target: undefined | (() => void) = callback;", "void-variables", false, 1),
        ("declare const callback: undefined | (() => Promise<void>); const object: { callback: undefined | (() => void) } = { callback };", "void-properties", false, 1),
        ("declare const callback: undefined | (() => Promise<void>); function make(): undefined | (() => void) { return callback; }", "void-returns", false, 1),
        ("declare const callback: undefined | (() => Promise<void>); declare function C(props: { callback: undefined | (() => void) }): unknown; const element = <C callback={callback} />;", "void-attributes", true, 1),
        ("declare const callback: undefined; declare function take(cb: undefined | (() => void)): void; take(callback);", "void-arguments", false, 0),
        ("declare const callback: undefined | (() => Promise<void>); declare function take(cb: undefined | (() => Promise<void>)): void; take(callback);", "void-arguments", false, 0),
    ] {
        check_contract(body, variant, expected, jsx);
    }
}

#[test]
fn promise_class_expression_members_keep_instance_heritage() {
    for (body, expected) in [
        ("const Base = class { method(): void {} }; class Child extends Base { async method() {} }", 1),
        ("const Base = class { method(): void {} }; type Constructor = typeof Base; interface Child extends Constructor { method(): Promise<void> }", 1),
        ("namespace N { export const Base = class { method(): void {} }; } class Child extends N.Base { async method() {} }", 1),
        ("function make() { const Base = class { method(): void {} }; class Child extends Base { async method() {} } }", 1),
        ("type T = () => void; function make<T extends () => Promise<void>>() { const Base = class { callback: T }; class Child extends Base { callback = async () => {}; } }", 0),
        ("const Base = class { method(): void {} }; class Child extends Base { static async method() {} }", 0),
        ("const Base = class { method(): Promise<void> { return null!; } }; class Child extends Base { async method() {} }", 0),
    ] {
        check_contract(body, "void-inherited-methods", expected, false);
    }
}

#[test]
fn promise_class_expression_members_keep_lexical_aliases() {
    check_contract("type Callback = () => void; function make() { type Callback = () => Promise<void>; const Base = class { callback: Callback }; class Child extends Base { callback = async () => {}; } }",
        "void-inherited-methods", 0, false);
}

#[test]
fn promise_explicit_instantiation_is_not_reinferred_from_the_callback() {
    for (body, expected) in [
        ("type Callback = () => void; declare const take: <T extends (...args: unknown[]) => unknown>(fn: T) => T; declare const specialized: typeof take<Callback>; specialized(async () => {});", 1),
        ("type Callback = () => void; declare const take: <T extends (...args: unknown[]) => unknown>(fn: T) => T; const specialized = take<Callback>; specialized(async () => {});", 1),
        ("declare const take: <T extends (...args: unknown[]) => unknown>(fn: T) => T; take(async () => {});", 0),
        ("type Callback = () => Promise<void>; declare const take: <T extends (...args: unknown[]) => unknown>(fn: T) => T; declare const specialized: typeof take<Callback>; specialized(async () => {});", 0),
    ] {
        check_contract(body, "void-arguments", expected, false);
    }
}

#[test]
fn promise_contracts_read_imported_class_expressions_and_instantiations() {
    for (rule, expected) in [
        ("no-misused-promises", 2),
        ("no-misused-promises:void-inherited-methods", 1),
        ("no-misused-promises:void-arguments", 1),
    ] {
        let reports = lint_project(&format!("promise-imported-core-{}", rule.replace(':', "-")), &[
            ("tsconfig.json", r#"{"compilerOptions":{"lib":["fixture"],"types":[]}}"#),
            ("node_modules/typescript/lib/lib.fixture.d.ts", "interface Promise<T> { then(callback: (value: T) => unknown): unknown }"),
            ("base.ts", "export const Base = class { method(): void {} }; export declare function take<T extends (...args: unknown[]) => unknown>(callback: T): T;"),
            ("main.ts", "import { Base, take } from './base'; class Child extends Base { async method() {} } declare const specialized: typeof take<() => void>; specialized(async () => {});"),
        ], "main.ts", &format!("rule {rule}"));
        assert_eq!(reports.len(), expected, "{rule}: {reports:?}");
    }
}

#[test]
fn predicate_and_void_argument_policies_can_be_switched_independently() {
    let source = "const values = [1]; declare function take(callback: () => void): void; values.filter(async () => true); take(async () => {});";
    for (rules, expected) in [
        ("rule no-misused-promises\nrule no-misused-promises:void-arguments off", 1),
        ("rule no-misused-promises", 2),
        ("rule no-misused-promises\nrule no-misused-promises:predicates off", 1),
        ("rule no-misused-promises\nrule no-misused-promises:void-arguments off\nrule no-misused-promises:predicates off", 0),
        ("rule no-misused-promises:predicates", 1),
        ("rule no-misused-promises:void-arguments", 1),
        ("rule no-misused-promises\nrule no-misused-promises:conditionals off", 2),
    ] {
        check_rules(source, rules, expected, false);
    }
}

#[test]
fn argument_contracts_include_inferred_callbacks_and_allow_promise_receivers() {
    for (body, expected) in [
        ("declare function take(cb: () => void): void; take(async () => {});", 1),
        ("declare function take(cb: () => void): void; declare const p: Promise<number>; take(() => p);", 1),
        ("declare function take(cb: () => void): void; async function callback() {} take(callback);", 1),
        ("declare function take(cb: () => void): void; take(() => 1);", 0),
        ("declare function take(cb: () => void | Promise<void>): void; take(async () => {});", 0),
        ("declare function take(cb: (() => void) | (() => Promise<void>)): void; take(async () => {});", 0),
        ("declare function take<T extends () => void>(cb: T): void; take<() => void>(async () => {});", 1),
        ("declare function take(cb: () => void): void; take(((async () => {})));", 1),
    ] {
        check_contract(body, "void-arguments", expected, false);
    }
}

#[test]
fn argument_contracts_include_overloads_merged_declarations_and_rest_positions() {
    for body in [
        "interface Take { (cb: () => number): void; (cb: () => void): void } declare const take: Take; take(async () => {});",
        "interface Take { (cb: () => number): void } interface Take { (cb: () => void): void } declare const take: Take; take(async () => {});",
        "interface Take { (cb: () => void): void } interface Take { (cb: () => number): void } declare const take: Take; take(async () => {});",
        "declare function take(...args: [number, () => void]): void; take(1, async () => {});",
        "declare function take(...callbacks: (() => void)[]): void; take(async () => {});",
        "declare function take(value: number, cb: () => void): void; take(...[1, async () => {}]);",
        "declare class Take { constructor(cb: () => void) } new Take(async () => {});",
    ] {
        check_contract(body, "void-arguments", 1, false);
    }
}

#[test]
fn variable_and_return_contracts_distinguish_declared_consumers_from_inference() {
    for (body, variant, expected) in [
        ("const callback: () => void = async () => {};", "void-variables", 1),
        ("let callback: () => void; callback = async () => {};", "void-variables", 1),
        ("function f(callback: () => void = async () => {}) {}", "void-variables", 1),
        ("function f(): () => void { return async () => {}; }", "void-returns", 1),
        ("const callback = async () => {};", "void-variables", 0),
        ("function f() { return async () => {}; }", "void-returns", 0),
    ] {
        check_contract(body, variant, expected, false);
    }
}

#[test]
fn property_contracts_include_methods_and_accessors() {
    for body in [
        "const object: { callback: () => void } = { callback: async () => {} };",
        "const object: { callback(): void } = { async callback() {} };",
        "const object: { callback: () => void } = { get callback() { return async () => {}; } };",
        "class C { callback: () => void = async () => {}; }",
        "declare const object: { callback: () => void }; object.callback = async () => {};",
    ] {
        check_contract(body, "void-properties", 1, false);
    }
}

#[test]
fn heritage_contracts_keep_each_base_and_interface_method() {
    for (body, expected) in [
        ("interface Base { method(): void } class C implements Base { async method() {} }", 1),
        ("class Base { method(): void {} } class C extends Base { async method() {} }", 1),
        ("interface A { method(): void } interface B { method(): void } class C implements A, B { async method() {} }", 2),
        ("interface Base { method(): void } interface C extends Base { method(): Promise<void> }", 1),
        ("interface Base { method(): Promise<void> } class C implements Base { async method() {} }", 0),
    ] {
        check_contract(body, "void-inherited-methods", expected, false);
    }
}

#[test]
fn jsx_callback_contracts_follow_component_props() {
    check_contract("declare function C(props: { onClick: () => void }): unknown; const element = <C onClick={async () => {}} />;", "void-attributes", 1, true);
    check_contract("declare function C(props: { onClick: () => Promise<void> }): unknown; const element = <C onClick={async () => {}} />;", "void-attributes", 0, true);
}

#[test]
fn inherited_callback_fields_include_declarations_without_initialisers() {
    for body in [
        "abstract class Base { abstract accessor callback: () => void } abstract class Derived extends Base { abstract accessor callback: () => Promise<void> }",
        "interface Base { callback: () => void } interface Derived extends Base { callback: () => Promise<void> }",
    ] {
        check_contract(body, "void-inherited-methods", 1, false);
    }
}

#[test]
fn array_predicates_consume_booleans_but_custom_filter_functions_do_not() {
    for method in ["every", "filter", "find", "findIndex", "findLast", "findLastIndex", "some"] {
        check_contract(&format!("[1].{method}(async () => true);"), "predicates", 1, false);
    }
    check_contract("const object = { filter(callback: () => Promise<boolean>) {} }; object.filter(async () => true);", "predicates", 0, false);
}
