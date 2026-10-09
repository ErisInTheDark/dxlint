use crate::engine::tests::with_native_file;
use crate::expr::Value;

fn metadata(file: &crate::native::ts::facts::NativeFile<'_>, node: u32) -> Value<'static> {
    let fields = ["kind", "parts", "declarations", "callable", "constructor", "symbol_constructor", "class_value", "property_names", "iterable", "property_writes"].into_iter().map(Value::from).collect();
    crate::expr::facts::with_args(vec![Value::list(Vec::new()), Value::list(fields)], || super::structure(file, node, &[]))
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
