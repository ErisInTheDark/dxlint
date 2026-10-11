//! Declaration syntax and receiver contracts, requested separately from type shape.

use std::sync::Arc;

use crate::expr::Value;
use crate::native::ts::kinds::{Kind, KINDS};
use crate::native::ts::summary::{self, DeclKind, InitializerKind, MemberKind, TypeSyn};
use crate::native::ts::topics::typed::specifiers;
use crate::native::ts::types::{Callable, Ty};
use super::TypeStructure;

pub(super) fn scalar_fields(fields: &[String]) -> Vec<String> {
    fields.iter().filter(|field| !matches!(field.as_str(),
        "members" | "member_types" | "call_signatures" | "member_metadata" | "call_signature_metadata")).cloned().collect()
}

pub(super) fn headers(projection: &TypeStructure<'_, '_>, ty: &Ty, depth: usize) -> Value<'static> {
    let fields = scalar_fields(projection.fields);
    let receiver_profile = TypeStructure { fields: &fields, ..*projection };
    projection.ck.call_signature_headers(ty, projection.constrained).map_or(Value::Null, |headers| {
        Value::list(headers.into_iter().map(|(signature, receiver)| {
            let receiver = receiver.map_or(Value::Null, |ty| {
                let parameter = &signature.syntax.params[0];
                let annotation = parameter.annotation_kind.map_or(Value::Null, |kind| {
                    Value::record(vec![
                        ("kind".into(), Value::from(KINDS[kind as usize].0)),
                        ("keyword".into(), match &parameter.ty {
                            Some(TypeSyn::Kw(keyword)) if kind == Kind::TypeKeyword as u16 => Value::from(keyword.to_string()),
                            _ => Value::Null,
                        }),
                    ])
                });
                Value::record(vec![
                    ("name".into(), Value::from(parameter.name.to_string())),
                    ("annotation".into(), annotation),
                    ("type".into(), receiver_profile.describe(&ty, &[], Vec::new(), depth + 1)),
                ])
            });
            Value::record(vec![("receiver".into(), receiver)])
        }).collect())
    })
}

pub(super) fn members(projection: &TypeStructure<'_, '_>, ty: &Ty, properties: &[String], depth: usize) -> Value<'static> {
    let TypeStructure { ck, file, .. } = *projection;
    Value::list(properties.iter().map(|name| {
        let resolution = ck.member_resolution(ty, name);
        let declarations = resolution.declarations.iter().map(|declaration| {
            let origin = declaration.origin;
            let local = file.zip(origin).filter(|(_, origin)| origin.file == ck.file);
            let member = local.and_then(|(file, origin)| summary::member(&file.syntax, origin.node)).unwrap_or_else(|| declaration.member.clone());
            let environment = declaration.environment.clone().or_else(|| local.map(|(file, origin)| file.env_at(origin.node)));
            let owner = declaration.owner.and_then(|reference| ck.summary_of(reference.file).map(|summary| {
                let owner = &summary.decls[reference.decl as usize];
                Value::record(vec![
                    ("name".into(), Value::from(owner.name.to_string())),
                    ("kind".into(), Value::from(match owner.kind {
                        DeclKind::Class => "class", DeclKind::Interface => "interface", DeclKind::Alias => "alias", _ => "other",
                    })),
                    ("from".into(), Value::from(if ck.is_lib_decl(reference) { "lib" } else { "file" })),
                ])
            })).unwrap_or(Value::Null);
            let provenance = declaration.owner.map(|owner| specifiers::file_gate(ck, owner.file, name))
                .or_else(|| origin.map(|origin| specifiers::file_gate(ck, origin.file, name))).unwrap_or(Value::Null);
            let initializer_kind = member.initializer_syntax.as_ref().map_or(Value::Null, |initializer| Value::from(match initializer.kind {
                InitializerKind::Function => "function", InitializerKind::Arrow => "arrow", InitializerKind::Other => "other",
            }));
            let signatures = if !member.sigs.is_empty() { member.sigs.clone() }
                else { member.initializer_syntax.as_ref().and_then(|initializer| initializer.signature.clone()).into_iter().collect() };
            let signatures = environment.map_or_else(|| Value::list(Vec::new()), |env| {
                let callable = Ty::Fn(Arc::new(Callable::new(signatures, Arc::new(env), None)));
                headers(projection, &callable, depth)
            });
            Value::record(vec![
                ("owner".into(), owner),
                ("kind".into(), Value::from(match member.kind {
                    MemberKind::Prop => "property", MemberKind::Method => "method", MemberKind::Call => "call",
                    MemberKind::Construct => "construct", MemberKind::Index => "index", MemberKind::Mapped => "mapped",
                })),
                ("is_static".into(), Value::Bool(member.is_static)),
                ("initializer_kind".into(), initializer_kind),
                ("provenance".into(), provenance),
                ("signatures".into(), signatures),
            ])
        }).collect();
        Value::record(vec![("name".into(), Value::from(name.clone())), ("declarations".into(), Value::list(declarations))])
    }).collect())
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;
    use crate::expr::Value;
    use crate::native::ts::facts::NativeFile;

    fn project(file: &NativeFile<'_>, node: u32, names: &[&str], fields: &[&str], constrained: bool) -> Value<'static> {
        project_path(file, node, names, fields, constrained, &[])
    }

    fn project_path(file: &NativeFile<'_>, node: u32, names: &[&str], fields: &[&str], constrained: bool, path: &[&str]) -> Value<'static> {
        let names: Vec<_> = names.iter().map(|name| name.to_string()).collect();
        crate::expr::facts::with_args(vec![
            Value::list(names.iter().cloned().map(Value::from).collect()),
            Value::list(fields.iter().map(|field| Value::from(field.to_string())).collect()),
            Value::Bool(constrained),
            Value::list(path.iter().map(|part| Value::from(part.to_string())).collect()),
        ], || super::super::structure(file, node, &names))
    }

    fn declaration(value: &Value<'_>, name: &str) -> Value<'static> {
        value.key("member_metadata").items().iter().find(|member| member.key("name") == Value::from(name.to_string()))
            .expect("requested member metadata missing").key("declarations").items().first()
            .expect("declaration metadata missing").clone().into_owned()
    }

    #[test]
    fn inherited_static_metadata_retains_constructable_interface_declarations() {
        with_native_file(r#"
            interface Factory { new(): object; create(this: void): string; }
            declare const Factory: Factory;
            class Derived extends Factory {}
            const constructor = /*@*/Derived;
            class Override extends Factory { static create(this: void): number { return 1; } }
            const overrideConstructor = /*@*/Override;
        "#, |file, nodes| {
            for (node, expected) in nodes.iter().zip(["Factory", "Override"]) {
                let value = project(file, *node, &["create"], &["member_metadata", "kind"], true);
                let member = declaration(&value, "create");
                assert_eq!(member.key("owner").key("name"), Value::from(expected));
            }
        });
    }

    #[test]
    fn member_metadata_retains_raw_initializers_without_retyping_annotated_fields() {
        with_native_file(r#"
            class C {
                functionField: number = function(this: void) {};
                arrowField = () => {};
                wrappedField = (function(this: void) {});
                typedField: () => void;
                method(this: void) {}
                static method(this: void) {}
            }
            const instance = /*@*/new C();
            const constructor = /*@*/C;
            const object = /*@*/({ method(this: void) {}, functionField: function(this: void) {}, arrowField: () => {} });
        "#, |file, nodes| {
            let names = ["functionField", "arrowField", "wrappedField", "typedField", "method"];
            let metadata = project(file, nodes[0], &names, &["member_metadata"], true);
            for (name, expected) in [("functionField", "function"), ("arrowField", "arrow"), ("wrappedField", "other")] {
                assert_eq!(declaration(&metadata, name).key("initializer_kind"), Value::from(expected));
            }
            assert_eq!(declaration(&metadata, "typedField").key("initializer_kind"), Value::Null);
            assert_eq!(declaration(&metadata, "method").key("kind"), Value::from("method"));
            assert_eq!(declaration(&metadata, "method").key("is_static"), Value::Bool(false));
            assert_eq!(declaration(&project(file, nodes[1], &["method"], &["member_metadata"], true), "method").key("is_static"), Value::Bool(true));
            let fields = project(file, nodes[0], &["functionField"], &["member_types", "kind"], true);
            assert_eq!(fields.key("member_types").items()[0].key("type").key("kind"), Value::from("number"),
                "initializer metadata changed the annotated value type");
            let object = project(file, nodes[2], &["method", "functionField", "arrowField"], &["member_metadata"], true);
            assert_eq!(declaration(&object, "method").key("kind"), Value::from("method"), "literal declaration origin was lost");
            assert_eq!(declaration(&object, "functionField").key("initializer_kind"), Value::from("function"));
            assert_eq!(declaration(&object, "arrowField").key("initializer_kind"), Value::from("arrow"));
        });
    }

    #[test]
    fn receiver_metadata_distinguishes_identity_raw_syntax_and_resolved_type() {
        with_native_file(r#"
            type V = void;
            interface C {
                direct(this: void): void;
                alias(this: V): void;
                wrapped(this: (void)): void;
                ordinary(arg: void): void;
                untyped(this): void;
            }
            declare const value: C;
            const reflected = /*@*/value;
        "#, |file, nodes| {
            let metadata = project(file, nodes[0], &["direct", "alias", "wrapped", "ordinary", "untyped"], &["member_metadata", "kind"], true);
            let receiver = |name| declaration(&metadata, name).key("signatures").items()[0].key("receiver").into_owned();
            for (name, annotation) in [("direct", "ts_type_keyword"), ("alias", "ts_type_reference"), ("wrapped", "ts_parenthesized_type")] {
                let receiver = receiver(name);
                assert_eq!(receiver.key("name"), Value::from("this"));
                assert_eq!(receiver.key("annotation").key("kind"), Value::from(annotation));
                assert_eq!(receiver.key("type").key("kind"), Value::from("void"));
            }
            assert_eq!(receiver("ordinary"), Value::Null, "ordinary void parameter became a receiver");
            assert_eq!(receiver("untyped").key("annotation"), Value::Null);
        });
    }

    #[test]
    fn declared_receiver_headers_bind_inherited_generics_without_call_inference() {
        with_native_file(r#"
            interface Base<T> { run(this: T): void; }
            interface Derived extends Base<string> {}
            interface Callable<T> { (this: T): void; }
            interface Inherited extends Callable<number> {}
            declare const value: Derived;
            declare const callable: Inherited;
            declare function generic<T extends string>(this: T): void;
            const member = /*@*/value;
            const call = /*@*/callable;
            const open = /*@*/generic;
        "#, |file, nodes| {
            let metadata = project(file, nodes[0], &["run"], &["member_metadata", "kind"], true);
            let member = declaration(&metadata, "run");
            assert_eq!(member.key("signatures").items()[0].key("receiver").key("type").key("kind"), Value::from("string"));
            assert_eq!(member.key("provenance").key("from"), Value::from("file"));
            let callable = project(file, nodes[1], &[], &["call_signature_metadata", "kind"], true);
            assert_eq!(callable.key("call_signature_metadata").items()[0].key("receiver").key("type").key("kind"), Value::from("number"));
            for (constrained, expected) in [(true, "string"), (false, "parameter")] {
                let value = project(file, nodes[2], &[], &["call_signature_metadata", "kind"], constrained);
                assert_eq!(value.key("call_signature_metadata").items()[0].key("receiver").key("type").key("kind"), Value::from(expected));
            }
            let old = ["kind", "parts", "members", "call_signatures"];
            let old_only = project(file, nodes[0], &["run"], &old, true);
            let mut extended = old.to_vec();
            extended.extend(["member_metadata", "call_signature_metadata"]);
            let with_metadata = project(file, nodes[0], &["run"], &extended, true);
            for field in old {
                assert_eq!(old_only.key(field), with_metadata.key(field), "metadata changed {field}");
            }
        });
    }

    #[test]
    fn finite_member_paths_project_generic_union_receivers_without_expanding_properties() {
        with_native_file(r#"
            interface C { run(this: void): void; }
            interface Box<T> { child: T; }
            declare const container: Box<C> | { child: C };
            const direct = /*@*/container.child;
            const root = /*@*/container;
        "#, |file, nodes| {
            let fields = ["kind", "parts", "member_metadata"];
            assert_eq!(
                project_path(file, nodes[1], &["run"], &fields, true, &["child"]),
                project(file, nodes[0], &["run"], &fields, true),
                "finite lookup did not preserve the reached receiver's declarations",
            );
            let missing = project_path(file, nodes[1], &["run"], &fields, true, &["missing"]);
            assert!(missing.key("member_metadata").items().iter().all(|member| member.key("declarations").items().is_empty()),
                "missing path reused unrelated root declarations");
        });
    }
}
