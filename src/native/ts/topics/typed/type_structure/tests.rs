use crate::engine::tests::{native_linter, with_native_file};
use crate::expr::Value;
use std::path::Path;

fn metadata(file: &crate::native::ts::facts::NativeFile<'_>, node: u32) -> Value<'static> {
    let fields = ["kind", "parts", "declarations", "callable", "constructor", "symbol_constructor", "class_value", "property_names", "iterable", "property_writes"].into_iter().map(Value::from).collect();
    crate::expr::facts::with_args(vec![Value::list(Vec::new()), Value::list(fields)], || super::structure(file, node, &[]))
}

fn spans(source: &str, options: &str) -> Vec<String> {
    let linter = native_linter(&format!("import .ts/spreads\nrule no-misused-spread {options}"));
    linter.lint(Path::new("/root/a.ts"), source).iter().map(|d| source[d.start..d.end].to_string()).collect()
}

#[test]
fn spread_routes_do_not_confuse_collection_elements_with_constituents() {
    let source = "declare const text: string; declare const list: string[]; declare function call(...xs: string[]): void; [...text]; call(...text); new call(...text); ({...text}); [...list]; ({...list});";
    assert_eq!(spans(source, ""), ["...text", "...text", "...list"]);
}

#[test]
fn object_spreads_report_one_category_and_respect_function_properties() {
    let source = "declare const mixed: (() => void) | number[]; function bare() {} function useful() {} useful.value = 1; const arrow = () => {}; arrow['value'] = 2; ({...mixed}); ({...bare}); ({...useful}); ({...arrow}); function nested() { const useful = () => {}; ({...useful}); }";
    assert_eq!(spans(source, ""), ["...mixed", "...bare", "...useful"]);
}

#[test]
fn allowed_names_follow_values_and_retained_type_aliases() {
    let source = "function safe() {} function unsafe() {} type Branded = string & { brand: true }; declare const text: Branded; ({...safe}); ({...unsafe}); [...text];";
    assert_eq!(spans(source, "{ set allow ['safe', {from: 'file', name: 'Branded'}] }"), ["...unsafe"]);
}

#[test]
fn structural_metadata_distinguishes_calls_properties_and_class_sides() {
    with_native_file("interface Callable { (): void; value: string; } class C {} declare const callable: Callable; declare const instance: C; const a = /*@*/callable; const b = /*@*/instance; const c = /*@*/C;", |file, nodes| {
        let callable = metadata(file, nodes[0]);
        assert_eq!(callable.key("callable"), Value::Bool(true));
        assert!(callable.key("property_names").items().contains(&Value::from("value")));
        let instance = metadata(file, nodes[1]);
        assert!(instance.key("declarations").items().iter().any(|d| d.key("kind") == Value::from("class")));
        assert_eq!(instance.key("constructor"), Value::Bool(false));
        assert_eq!(metadata(file, nodes[2]).key("constructor"), Value::Bool(true));
    });
}

#[test]
fn interface_instance_metadata_retains_its_constructible_value_side() {
    with_native_file("interface Box {} interface Factory { new(): Box; } declare const Box: Factory; declare const box: Box; const a = /*@*/box;", |file, nodes| {
        let instance = metadata(file, nodes[0]);
        assert_eq!(instance.key("constructor"), Value::Bool(false));
        assert_eq!(instance.key("symbol_constructor"), Value::Bool(true));
    });
}

#[test]
fn call_signatures_are_not_properties_of_callable_interfaces() {
    let source = "interface Bare { (): void; } interface Useful { (): void; value: number; } declare const bare: Bare; declare const useful: Useful; ({...bare}); ({...useful});";
    assert_eq!(spans(source, ""), ["...bare"]);
}

#[test]
fn property_writes_do_not_expand_explicit_function_contracts() {
    let source = "declare const annotated: () => void; annotated.value = 1; const inferred = () => {}; inferred.value = 1; ({...annotated}); ({...inferred});";
    assert_eq!(spans(source, ""), ["...annotated"]);
}
