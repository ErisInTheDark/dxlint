//! Recursive type properties exposed to DSL policies.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::summary::{DeclKind, MemberKind};
use crate::native::ts::topics::typed::specifiers;
use crate::native::ts::types::{Checker, Ty};
use crate::native::ts::kinds::{Field, Kind};
use std::collections::HashMap;

#[cfg(test)]
#[path = "type_structure/tests.rs"]
mod spread_tests;

pub(super) fn structure(file: &NativeFile<'_>, node: u32, properties: &[String]) -> Value<'static> {
    let args = crate::expr::facts::args();
    let fields = args.get(1).map(|value| value.items().iter().map(ToString::to_string).collect::<Vec<_>>())
        .unwrap_or_else(|| ["kind", "parts", "names", "generic_names", "aliases", "members"].into_iter().map(String::from).collect());
    let requested = |name: &str| fields.iter().any(|field| field == name);
    let ck = &file.typing().checker;
    let aliases = if requested("aliases") { specifiers::annotated_aliases(file, node).into_iter().filter_map(|r| {
        let retained = specifiers::retained_alias(ck, r, &mut Vec::new())?;
        Some(Value::from(ck.summary_of(retained.file)?.decls[retained.decl as usize].name.to_string()))
    }).collect() } else { Vec::new() };
    let constrained = !matches!(args.get(2), Some(Value::Bool(false)));
    let ty = if constrained { file.constrained_type_of(node) } else { file.type_of(node) };
    let value = describe(&ty, ck, properties, &fields, aliases, constrained, 0);
    if !requested("property_writes") && !requested("name_bindings") { return value }
    let Value::Record(entries) = value else { unreachable!() };
    let mut entries = entries.iter().cloned().collect::<Vec<_>>();
    if requested("property_writes") {
        let writes = file.decl_of(file.syntax.unparen(node)).and_then(|decl| file.cached(property_writes).0.get(&decl).cloned()).unwrap_or_default();
        entries.push(("property_writes".into(), Value::list(writes)));
    }
    if requested("name_bindings") {
        let bindings = file.bindings();
        let environment = file.env_at(node);
        let names = specifiers::named_types(&ty, ck, &mut Vec::new()).into_iter().filter_map(|(declaration, _)| {
            let summary = ck.summary_of(declaration.file)?;
            let name = &summary.decls[declaration.decl as usize].name;
            let visible = bindings.visible(&file.syntax, node, name, false);
            let kind = visible.and_then(|node| bindings.decls.get(&node)).map_or("unknown", |binding| binding.kind);
            let same_declaration = if let Some(visible) = visible.filter(|_| kind != "import") {
                declaration.file == ck.file && file.syntax.parent(visible).is_some_and(|parent|
                    file.syntax.tree.node(parent).start == summary.decls[declaration.decl as usize].start)
            } else {
                ck.type_decls(std::slice::from_ref(name), &environment).contains(&declaration)
            };
            Some(Value::record(vec![
                ("name".into(), Value::from(name.to_string())),
                ("kind".into(), Value::from(kind)),
                ("same_declaration".into(), Value::Bool(same_declaration)),
            ]))
        }).collect();
        entries.push(("name_bindings".into(), Value::list(names)));
    }
    Value::record(entries)
}

struct PropertyWrites(HashMap<u32, Vec<Value<'static>>>);

#[cfg(test)]
thread_local! {
    static PROPERTY_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn constructible(ty: &Ty, ck: &Checker<'_>) -> bool {
    match ty {
        Ty::Class(_) => true,
        Ty::Obj(members, _) => members.iter().any(|m| m.kind == MemberKind::Construct),
        Ty::Named(symbols, _) => symbols.iter().any(|r| ck.summary_of(r.file).is_some_and(|s| s.decls[r.decl as usize].members.iter().any(|m| m.kind == MemberKind::Construct))),
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|part| constructible(part, ck)),
        Ty::Inter(parts) => parts.iter().any(|part| constructible(part, ck)),
        _ => false,
    }
}

fn property_writes(file: &NativeFile<'_>) -> PropertyWrites {
    let s = &file.syntax;
    let mut writes: HashMap<u32, Vec<Value<'static>>> = HashMap::new();
    for (&reference, &decl) in &file.bindings().refs {
        let Some(access) = s.parent(reference).filter(|&n| matches!(s.kind(n), Some(Kind::Member | Kind::Index)) && s.get(n, Field::Object) == Some(reference)) else { continue };
        if !s.parent(access).is_some_and(|n| s.is(n, Kind::Assign) && s.get(n, Field::Left) == Some(access)) { continue }
        let name = if s.is(access, Kind::Member) {
            s.get(access, Field::Property).map(|n| s.text(n).to_string())
        } else {
            s.get(access, Field::Index).and_then(|n| file.static_string(n))
        };
        if let Some(name) = name {
            let names = writes.entry(decl).or_default();
            let name = Value::from(name);
            if !names.contains(&name) { names.push(name); }
        }
    }
    for names in writes.values_mut() { names.sort_by_key(ToString::to_string); }
    PropertyWrites(writes)
}

fn describe(ty: &Ty, ck: &Checker<'_>, properties: &[String], fields: &[String], aliases: Vec<Value<'static>>, constrained: bool, depth: usize) -> Value<'static> {
    if depth >= 64 { return Value::record(vec![("kind".into(), Value::from("recursive"))]) }
    if constrained {
        if let Some(source) = ck.readonly_source(ty) { return describe(source, ck, properties, fields, aliases, constrained, depth + 1) }
    }
    let requested = |name: &str| fields.iter().any(|field| field == name);
    let parts = if requested("parts") { match ty {
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().map(|part| describe(part, ck, properties, fields, Vec::new(), constrained, depth + 1)).collect(),
        Ty::Array(array) => vec![describe(&array.element, ck, properties, fields, Vec::new(), constrained, depth + 1)],
        Ty::Tuple(tuple) => tuple.elements.iter().map(|element| {
            if element.optional { describe(&element.ty.clone().or_undefined(), ck, properties, fields, Vec::new(), constrained, depth + 1) }
            else { describe(&element.ty, ck, properties, fields, Vec::new(), constrained, depth + 1) }
        }).collect(),
        _ => Vec::new(),
    } } else { Vec::new() };
    let arguments = if requested("arguments") {
        match ty {
            Ty::Named(_, arguments) => arguments.iter().map(|argument| describe(argument, ck, properties, fields, Vec::new(), constrained, depth + 1)).collect(),
            _ => Vec::new(),
        }
    } else { Vec::new() };
    let named = if requested("names") || requested("generic_names") || requested("declarations") { specifiers::named_types(ty, ck, &mut Vec::new()) } else { Vec::new() };
    let class_symbols = if let Ty::Class(symbols) = ty { &symbols[..] } else { &[] };
    let declarations = if requested("declarations") { named.iter().map(|(r, _)| r).chain(class_symbols).filter_map(|r| {
        let summary = ck.summary_of(r.file)?;
        let decl = &summary.decls[r.decl as usize];
        Some(Value::record(vec![
            ("name".into(), Value::from(decl.name.to_string())),
            ("kind".into(), Value::from(match decl.kind { DeclKind::Class => "class", DeclKind::Interface => "interface", _ => "other" })),
            ("from".into(), Value::from(if ck.is_lib_decl(*r) { "lib" } else { "file" })),
            ("inherited".into(), Value::Bool(!matches!(ty, Ty::Named(symbols, _) | Ty::Class(symbols) if symbols.contains(r)))),
        ]))
    }).collect() } else { Vec::new() };
    let mut property_names = Vec::new();
    if requested("property_names") {
        match ty {
            Ty::Fn(_) => {}
            Ty::Obj(members, _) => property_names.extend(members.iter().filter(|m| matches!(m.kind, MemberKind::Prop | MemberKind::Method)).map(|m| m.name.clone())),
            Ty::Class(symbols) => {
                for r in symbols.iter() {
                    if let Some(summary) = ck.summary_of(r.file) {
                        property_names.extend(summary.decls[r.decl as usize].members.iter().filter(|m| m.is_static && matches!(m.kind, MemberKind::Prop | MemberKind::Method)).map(|m| m.name.clone()));
                    }
                }
            }
            _ => ck.member_names(ty, &mut property_names),
        }
        if matches!(ty, Ty::Named(..)) {
            property_names.retain(|name| {
                #[cfg(test)]
                PROPERTY_LOOKUPS.with(|count| count.set(count.get() + 1));
                ck.member_resolution(ty, name).declarations.iter().any(|d| matches!(d.member.kind, MemberKind::Prop | MemberKind::Method))
            });
        }
    }
    let symbol_constructor = match ty {
        Ty::Named(symbols, _) if requested("symbol_constructor") => symbols.iter().any(|&r| ck.summary_of(r.file).is_some_and(|summary| {
            let decl = &summary.decls[r.decl as usize];
            constructible(&ck.value_of(&ck.lookup(&ck.decl_env(r), &decl.name)), ck)
        })),
        _ => false,
    };
    let names = if requested("names") { std::iter::once(Value::from(ck.print(ty))).chain(named.iter().map(|(_, ty)| Value::from(ck.print(ty)))).collect() } else { Vec::new() };
    let generic_names = if requested("generic_names") { named.first().and_then(|(r, _)| {
        let summary = ck.summary_of(r.file)?;
        let decl = &summary.decls[r.decl as usize];
        (!decl.type_params.is_empty()).then(|| Value::from(decl.name.to_string()))
    }).into_iter().collect() } else { Vec::new() };
    let members = if requested("members") { properties.iter().map(|name| {
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
                    MemberKind::Construct => "construct", MemberKind::Index => "index", MemberKind::Mapped => "mapped",
                })),
                ("signature".into(), Value::Bool(declaration.member.signature)),
                ("key_symbol".into(), ck.declaration_key_symbol(declaration).map_or(Value::Null, |name| Value::from(name.to_string()))),
            ])
        }).collect();
        Value::record(vec![("name".into(), Value::from(name.clone())), ("declarations".into(), Value::list(declarations))])
    }).collect() } else { Vec::new() };
    let member_types = if requested("member_types") { properties.iter().map(|name| {
        let member = ck.member(ty, name);
        Value::record(vec![
            ("name".into(), Value::from(name.clone())),
            ("type".into(), describe(&member, ck, &[], fields, Vec::new(), constrained, depth + 1)),
        ])
    }).collect() } else { Vec::new() };
    let call_signatures = if requested("call_signatures") {
        ck.call_signature_parameters(ty, constrained).map_or(Value::Null, |signatures| {
            // Parameter types retain selected scalar and collection fields, not callback graphs.
            let parameter_fields: Vec<_> = fields.iter().filter(|field|
                !matches!(field.as_str(), "members" | "member_types" | "call_signatures")).cloned().collect();
            Value::list(signatures.into_iter().map(|(parameters, rest)| Value::record(vec![
                ("parameters".into(), Value::list(parameters.iter().map(|parameter|
                    describe(parameter, ck, &[], &parameter_fields, Vec::new(), constrained, depth + 1)).collect())),
                ("rest".into(), Value::Bool(rest)),
            ])).collect())
        })
    } else { Value::Null };
    let entries: Vec<(crate::expr::value::Key, Value<'static>)> = vec![
        ("kind".into(), Value::from(ty.kind())),
        ("parts".into(), Value::list(parts)),
        ("arguments".into(), Value::list(arguments)),
        ("names".into(), Value::list(names)),
        ("generic_names".into(), Value::list(generic_names)),
        ("aliases".into(), Value::list(aliases)),
        ("members".into(), Value::list(members)),
        ("member_types".into(), Value::list(member_types)),
        ("call_signatures".into(), call_signatures),
        ("declarations".into(), Value::list(declarations)),
        ("callable".into(), Value::Bool(requested("callable") && ck.callable_type(ty))),
        ("constructor".into(), Value::Bool(requested("constructor") && constructible(ty, ck))),
        ("symbol_constructor".into(), Value::Bool(symbol_constructor)),
        ("class_value".into(), Value::Bool(matches!(ty, Ty::Class(_)))),
        ("property_names".into(), Value::list(property_names.into_iter().map(|n| Value::from(n.to_string())).collect())),
        ("iterable".into(), if requested("iterable") { ck.iterable_type(ty).map_or(Value::Null, Value::Bool) } else { Value::Null }),
    ];
    Value::record(entries.into_iter().filter(|(name, _)| requested(name)).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;
    use crate::expr::Value;

    fn callable_structure(file: &crate::native::ts::facts::NativeFile<'_>, node: u32, names: &[&str]) -> Value<'static> {
        let fields = ["kind", "parts", "member_types", "call_signatures", "callable"];
        crate::expr::facts::with_args(vec![
            Value::list(names.iter().map(|name| Value::from(name.to_string())).collect()),
            Value::list(fields.into_iter().map(Value::from).collect()),
        ], || super::structure(file, node, &names.iter().map(|name| name.to_string()).collect::<Vec<_>>()))
    }

    #[test]
    fn callable_structure_keeps_inherited_overloads_and_callback_union_types() {
        with_native_file(r#"
            type Callback = () => void;
            interface Base<T> {
                then(first: T, second?: T | undefined): unknown;
                then(value: string): unknown;
            }
            interface Derived extends Base<Callback> {}
            declare const value: Derived;
            const reflected = /*@*/value;
        "#, |file, nodes| {
            let projected = callable_structure(file, nodes[0], &["then"]);
            let members = projected.key("member_types").items();
            assert_eq!(members.len(), 1, "requested inherited member was not projected");
            let signatures = members[0].key("type").key("call_signatures").items();
            let parameters = signatures.iter().find(|signature| signature.key("parameters").items().len() == 2)
                .expect("inherited two-callback overload missing").key("parameters").items();
            assert_eq!(parameters[0].key("callable"), Value::Bool(true), "generic callback did not bind");
            let optional = parameters[1].key("parts").items();
            assert!(optional.iter().any(|part| part.key("callable") == Value::Bool(true)),
                "optional callback lost its callable arm");
            assert!(optional.iter().any(|part| part.key("kind") == Value::from("undefined")),
                "optional callback lost its missing-value arm");
            assert!(signatures.iter().any(|signature| {
                let parameters = signature.key("parameters").items();
                parameters.len() == 1 && parameters[0].key("kind") == Value::from("string")
            }), "other overload was discarded");
        });
    }

    #[test]
    fn callable_structure_distinguishes_recursive_callable_plain_and_unknown_types() {
        with_native_file(r#"
            interface Recursive { (value: Recursive): void; }
            declare const recursive: Recursive;
            declare const plain: number;
            declare const missing: unknown;
            const first = /*@*/recursive;
            const second = /*@*/plain;
            const third = /*@*/missing;
        "#, |file, nodes| {
            let signatures = callable_structure(file, nodes[0], &[]).key("call_signatures").items();
            assert_eq!(signatures.len(), 1, "recursive callable contract missing");
            let parameters = signatures[0].key("parameters").items();
            assert_eq!(parameters.len(), 1);
            assert_eq!(parameters[0].key("callable"), Value::Bool(true), "recursive parameter became opaque");
            assert_eq!(callable_structure(file, nodes[1], &[]).key("call_signatures"), Value::list(Vec::new()),
                "known non-callable acquired signatures");
            assert_eq!(callable_structure(file, nodes[2], &[]).key("call_signatures"), Value::Null,
                "unknown was presented as known non-callable");
        });
    }

    #[test]
    fn callable_structure_raw_parameters_stay_open() {
        with_native_file("declare function generic<T extends () => void>(callback: T): void; const value = /*@*/generic;", |file, nodes| {
            for (constrained, expected) in [(true, "function"), (false, "parameter")] {
                let fields = ["kind", "parts", "call_signatures", "callable"].into_iter().map(Value::from).collect();
                let projected = crate::expr::facts::with_args(vec![
                    Value::list(Vec::new()), Value::list(fields), Value::Bool(constrained),
                ], || super::structure(file, nodes[0], &[]));
                let signatures = projected.key("call_signatures").items();
                assert_eq!(signatures.len(), 1, "generic callable contract missing");
                let parameters = signatures[0].key("parameters").items();
                assert_eq!(parameters.len(), 1);
                assert_eq!(parameters[0].key("kind"), Value::from(expected), "wrong constraint mode");
            }
        });
    }

    #[test]
    fn resolved_generic_arguments_keep_union_and_intersection_structure() {
        for (annotation, kind, argument_kind) in [
            ("V", "void", None),
            ("V | number", "union", None),
            ("V & number", "intersection", None),
            ("Promise<V>", "object", Some("void")),
            ("PromiseLike<V | number>", "object", Some("union")),
            ("Promise<PromiseLike<V>>", "object", Some("object")),
        ] {
            let source = format!("interface Promise<T> {{}} interface PromiseLike<T> {{}} type V = void; function f(): {annotation} {{ return; }}");
            with_native_file(&source, |file, _| {
                use crate::native::ts::kinds::{Field as F, Kind as K};
                let function = (0..file.syntax.tree.nodes.len() as u32).find(|&node| file.syntax.is(node, K::Function)).unwrap();
                let annotation = file.syntax.get(function, F::ReturnType).unwrap();
                let fields = ["kind", "parts", "declarations", "arguments"].into_iter().map(Value::from).collect();
                let result = crate::expr::facts::with_args(vec![Value::list(Vec::new()), Value::list(fields), Value::Bool(false)], || super::structure(file, annotation, &[]));
                assert_eq!(result.key("kind"), Value::from(kind), "{source}");
                let arguments = result.key("arguments").items();
                if let Some(kind) = argument_kind {
                    assert_eq!(arguments.len(), 1, "{source}");
                    assert_eq!(arguments[0].key("kind"), Value::from(kind), "{source}");
                } else {
                    assert!(arguments.is_empty(), "{source}");
                }
                if matches!(kind, "union" | "intersection") {
                    let parts = result.key("parts").items();
                    assert!(parts.iter().any(|part| part.key("kind") == Value::from("void")), "{source}");
                    assert!(parts.iter().any(|part| part.key("kind") == Value::from("number")), "{source}");
                }
            });
        }
    }

    #[test]
    fn raw_structure_retains_open_parameters_inside_generic_arguments() {
        with_native_file("interface Box<T> {} function f<T extends void>(box: Box<T>) { const result = /*@*/box; }", |file, nodes| {
            let fields = ["kind", "arguments"].into_iter().map(Value::from).collect();
            let raw = crate::expr::facts::with_args(vec![Value::list(Vec::new()), Value::list(fields), Value::Bool(false)], || super::structure(file, nodes[0], &[]));
            assert_eq!(raw.key("arguments").items()[0].key("kind"), Value::from("parameter"));
            let fields = ["kind", "arguments"].into_iter().map(Value::from).collect();
            let bounded = crate::expr::facts::with_args(vec![Value::list(Vec::new()), Value::list(fields)], || super::structure(file, nodes[0], &[]));
            assert_eq!(bounded.key("arguments").items()[0].key("kind"), Value::from("void"));
            let default = super::structure(file, nodes[0], &[]);
            assert_eq!(default.key("arguments"), Value::Null);
        });
    }

    #[test]
    fn default_structure_does_not_resolve_unrequested_properties() {
        with_native_file("interface Item { value: string; method(): void; } declare const items: [Item, Item[]]; const result = /*@*/items;", |file, nodes| {
            super::PROPERTY_LOOKUPS.with(|count| count.set(0));
            let structure = super::structure(file, nodes[0], &[]);
            assert_eq!(structure.key("kind"), Value::from("tuple"));
            assert_eq!(super::PROPERTY_LOOKUPS.with(std::cell::Cell::get), 0, "unrequested properties must not trigger member resolution, including within collection elements");
            let requested = vec![Value::from("kind"), Value::from("parts"), Value::from("property_names")];
            let projected = crate::expr::facts::with_args(vec![Value::list(Vec::new()), Value::list(requested)], || super::structure(file, nodes[0], &[]));
            assert!(super::PROPERTY_LOOKUPS.with(std::cell::Cell::get) > 0);
            assert!(projected.key("parts").items()[0].key("property_names").items().contains(&Value::from("value")));
        });
    }

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
    fn named_type_bindings_distinguish_visible_identity_from_shadowing() {
        for (source, kind, same) in [
            ("enum E { One } const result = /*@*/E.One;", "enum", true),
            ("enum E { One } const value = E.One; function f() { const E = 0; const result = /*@*/value; }", "const", false),
            ("enum E { One } const value = E.One; function f() { type E = number; const result = /*@*/value; }", "type", false),
            ("enum E { One } const value = E.One; function f() { enum E { Other } const result = /*@*/value; }", "enum", false),
        ] {
            with_native_file(source, |file, nodes| {
                assert!(file.type_flags(nodes[0]).unwrap_or_default().contains(&"enum"), "{source}");
                let fields = vec![Value::from("name_bindings")];
                let result = crate::expr::facts::with_args(vec![Value::list(Vec::new()), Value::list(fields)], || super::structure(file, nodes[0], &[]));
                let bindings = result.key("name_bindings").items();
                let binding = bindings.iter().find(|binding| binding.key("name").loose_eq(&Value::from("E"))).expect("enum name binding");
                assert_eq!(binding.key("kind"), Value::from(kind), "{source}");
                assert_eq!(binding.key("same_declaration"), Value::Bool(same), "{source}");
            });
        }
    }

}
