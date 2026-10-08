//! Documentation and origins of resolved symbols and call signatures.

use crate::expr::facts::Fact;
use crate::expr::Value;

use super::super::facts::{Facts, NativeFile};
use super::super::kinds::{Field as F, Kind as K};
use super::super::modules::{DeclRef, ExportAlias};
use super::super::summary::{DeclKind, MemberKind};
use super::super::types::{Checker, Ty};
use super::typed::specifiers::file_gate;

pub const FACTS: &[Fact] = &[Fact {
    name: "symbol_metadata",
    doc: "native layers: resolved declaration and alias docs, kinds and provenance, plus the selected call signature",
    get: |f, n| f.with_file(metadata, n),
}];

fn origin(ck: &Checker<'_>, declaration: DeclRef, name: &str) -> Value<'static> {
    let gate = file_gate(ck, declaration.file, name.trim_start_matches('#'));
    let module = ck.summary_of(declaration.file).and_then(|summary| {
        summary.decls[declaration.decl as usize].module.map(|i| summary.ambient_modules[i].0.clone())
    });
    let mut origins = vec![gate];
    if let Some(module) = module {
        let Value::Record(entries) = &origins[0] else { return Value::list(Vec::new()) };
        let package = Value::record(entries.iter().map(|(key, value)| {
            let value = match &**key {
                "from" => Value::from("package"),
                "package" => Value::text_copy(&module),
                _ => value.clone(),
            };
            (key.clone(), value)
        }).collect::<Vec<_>>());
        origins.push(package);
    }
    Value::list(origins)
}

fn declaration(ck: &Checker<'_>, reference: DeclRef) -> Option<Value<'static>> {
    let summary = ck.summary_of(reference.file)?;
    let d = &summary.decls[reference.decl as usize];
    let kind = match d.kind {
        DeclKind::Class => "class", DeclKind::Interface => "interface",
        DeclKind::Alias => "type_alias", DeclKind::Function => "function",
        DeclKind::Var => "variable", DeclKind::Enum => "enum",
        DeclKind::Namespace => "namespace", DeclKind::Import { .. } | DeclKind::ImportAlias(_) => "import",
    };
    Some(Value::record(vec![
        ("name".into(), Value::text_copy(&d.name)),
        ("kind".into(), Value::from(kind)),
        ("doc".into(), d.doc.as_deref().map_or(Value::Null, Value::text_copy)),
        ("signature_docs".into(), Value::list(d.sigs.iter().map(|sig| sig.doc.as_deref().map_or(Value::Null, Value::text_copy)).collect())),
        ("provenance".into(), origin(ck, reference, &d.name)),
    ]))
}

fn alias(file: &NativeFile<'_>, alias: &ExportAlias) -> Value<'static> {
    let path = file.project.path(alias.file).to_string_lossy().replace('\\', "/");
    Value::record(vec![
        ("name".into(), Value::text_copy(&alias.name)),
        ("kind".into(), Value::from("alias")),
        ("doc".into(), alias.doc.as_deref().map_or(Value::Null, Value::text_copy)),
        ("path".into(), Value::from(path)),
        ("module".into(), alias.module.as_deref().map_or(Value::Null, Value::text_copy)),
    ])
}

/// The receiver and key whose property this syntax denotes.
fn property(file: &NativeFile<'_>, node: u32) -> Option<(Ty, String)> {
    let s = &file.syntax;
    let parent = s.parent(node);
    let access = if s.is(node, K::Index) { Some(node) } else {
        parent.filter(|&p| s.is(p, K::Member) && s.field(node) == Some(F::Property))
    };
    if let Some(access) = access {
        let object = s.get(access, F::Object)?;
        let key = if s.is(access, K::Index) { s.get(access, F::Index)? } else { node };
        let name = if !s.is(access, K::Index) {
            super::super::statics::identifier_name(s.text(key))
        } else {
            file.static_name(key).or_else(|| match file.type_of(key) {
                Ty::Lit(value, "string") => super::super::regex::cook(&value),
                Ty::Lit(value, "number") => Some(value.to_string()),
                _ => None,
            })?
        };
        return Some((file.type_of(object), name));
    }
    let parent = parent?;
    if s.is(parent, K::QualifiedName) && s.field(node) == Some(F::Right) {
        let path = super::super::summary::path_of(s, s.get(parent, F::Left)?);
        let object = file.typing().checker.value_path(&path, &file.env_at(node));
        return Some((object, super::super::statics::identifier_name(s.text(node))));
    }
    if s.is(parent, K::JsxAttribute) && s.field(node) == Some(F::Name) {
        let component = s.get(s.parent(parent)?, F::Name)?;
        let ck = &file.typing().checker;
        let component = file.type_of(component);
        let props = match component {
            Ty::Class(symbol) => ck.member(&Ty::Named(symbol, Vec::new()), "props"),
            _ => ck.contextual_parameter(&component, 0),
        };
        return Some((props, s.text(node).to_string()));
    }
    if s.is(parent, K::PatternProp) && s.field(node) == Some(F::Key) {
        let pattern = s.parent(parent)?;
        let owner = s.parent(pattern)?;
        let object = if s.field(pattern) == Some(F::Name) && s.is(owner, K::Declarator) {
            s.get(owner, F::Init).map(|init| file.type_of(init))
                .or_else(|| s.get(owner, F::Type).map(|annotation| file.typing().checker.eval(&super::super::summary::type_syn(s, annotation), &file.env_at(annotation))))
        } else if s.field(pattern) == Some(F::Left) && s.is(owner, K::Assign) {
            s.get(owner, F::Right).map(|value| file.type_of(value))
        } else {
            None
        }?;
        return Some((object, file.static_name(parent)?));
    }
    if s.is(parent, K::Prop) && s.field(node) == Some(F::Key) {
        return Some((file.contextual_type_of(s.parent(parent)?), file.static_name(parent)?));
    }
    None
}

fn signature(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut callee = node;
    if let Some(parent) = s.parent(callee) {
        if s.is(parent, K::Member) && s.field(callee) == Some(F::Property) { callee = parent; }
    }
    while let Some(parent) = s.parent(callee) {
        if matches!(s.kind(parent), Some(K::Paren | K::NonNull | K::Instantiation)) && s.field(callee) == Some(F::Expr) {
            callee = parent;
        } else { break }
    }
    let Some(call) = s.parent(callee) else { return Value::Null };
    let args = match (s.kind(call), s.field(callee)) {
        (Some(K::Call | K::New), Some(F::Callee)) => s.all(call, F::Arg).map(|arg| file.type_of(arg)).collect(),
        (Some(K::TaggedTemplate), Some(F::Tag)) => {
            let mut args = vec![Ty::Unknown];
            if let Some(template) = s.get(call, F::Quasi) { args.extend(s.all(template, F::Expr).map(|arg| file.type_of(arg))); }
            args
        }
        _ => return Value::Null,
    };
    let ck = &file.typing().checker;
    let Some((sig, _)) = ck.signature_for(&file.type_of(callee), &args, s.is(call, K::New)) else { return Value::Null };
    Value::record(vec![
        ("doc".into(), sig.doc.as_deref().map_or(Value::Null, Value::text_copy)),
        ("kind".into(), Value::from(if s.is(call, K::New) { "construct" } else { "call" })),
    ])
}

fn metadata(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let ck = &file.typing().checker;
    let export = s.parent(node).filter(|&parent| s.is(parent, K::ExportSpec));
    let lookup_node = export.filter(|_| s.field(node) == Some(F::Exported)).and_then(|specifier| s.get(specifier, F::Local)).unwrap_or(node);
    let own_doc = export.and_then(|specifier| {
        super::super::docs::leading_doc(s.src, s.tree.node(specifier).start as usize).or_else(|| {
            s.parent(specifier).and_then(|export| super::super::docs::leading_doc(s.src, s.tree.node(export).start as usize))
        })
    });
    let mut declarations = Vec::new();
    let mut aliases = Vec::new();
    let property = property(file, node);
    let name = property.as_ref().map_or_else(|| s.text(node).to_string(), |(_, name)| name.clone());
    if let Some((object, name)) = property {
        let object = match object {
            Ty::Module(module) => {
                let resolved = file.project.export_resolution(module.0, &name);
                declarations.extend(resolved.declarations.into_iter().filter_map(|d| declaration(ck, d)));
                aliases.extend(resolved.aliases.iter().map(|a| alias(file, a)));
                Ty::Unknown
            }
            Ty::Ns(symbol) => {
                let is_enum = symbol.iter().any(|reference| ck.summary_of(reference.file).is_some_and(|summary| {
                    summary.decls[reference.decl as usize].kind == DeclKind::Enum
                }));
                if is_enum {
                    Ty::Named(symbol, Vec::new())
                } else {
                    declarations.extend(ck.namespace_member(&symbol, &name).into_iter().filter_map(|d| declaration(ck, d)));
                    Ty::Unknown
                }
            }
            object => object,
        };
        for d in ck.member_resolution(&object, &name).declarations {
            let doc = d.origin.filter(|origin| origin.file == ck.file).and_then(|origin| {
                super::super::docs::leading_doc(s.src, s.tree.node(origin.node).start as usize)
            });
            let provenance = if let Some(owner) = d.owner {
                origin(ck, owner, &name)
            } else {
                Value::list(d.origin.into_iter().map(|origin| file_gate(ck, origin.file, &name)).collect())
            };
            declarations.push(Value::record(vec![
                ("name".into(), Value::text_copy(&d.member.name)),
                ("kind".into(), Value::from(match d.member.kind {
                    MemberKind::Method => "method", MemberKind::Prop => "property",
                    MemberKind::Call => "call", MemberKind::Construct => "construct", MemberKind::Index => "index",
                })),
                ("doc".into(), doc.as_deref().or(d.member.doc.as_deref()).map_or(Value::Null, Value::text_copy)),
                ("signature_docs".into(), Value::list(d.member.sigs.iter().map(|sig| sig.doc.as_deref().map_or(Value::Null, Value::text_copy)).collect())),
                ("provenance".into(), provenance),
            ]));
        }
    } else if let Some((from, name)) = file.import_of(lookup_node) {
        if let Some(module) = file.project.resolve(ck.file, &from) {
            let resolved = file.project.export_resolution(module, &name);
            declarations.extend(resolved.declarations.into_iter().filter_map(|d| declaration(ck, d)));
            aliases.extend(resolved.aliases.iter().map(|a| alias(file, a)));
        }
    } else if s.is(node, K::Ident) {
        let local = file.decl_of(lookup_node);
        let references = ck.lookup(&file.env_at(lookup_node), s.text(lookup_node));
        let matches = local.is_none() || references.iter().any(|reference| {
            ck.summary_of(reference.file).is_some_and(|summary| {
                reference.file == ck.file && local.is_some_and(|local| {
                    let start = summary.decls[reference.decl as usize].start;
                    let mut at = Some(local);
                    while let Some(node) = at {
                        if s.tree.node(node).start == start { return true }
                        at = s.parent(node);
                    }
                    false
                })
            })
        });
        if matches { declarations.extend(references.into_iter().filter_map(|reference| declaration(ck, reference))); }
        if declarations.is_empty() {
            if let Some(doc) = file.decl_doc(lookup_node) {
                declarations.push(Value::record(vec![
                    ("name".into(), Value::text_copy(s.text(node))),
                    ("kind".into(), Value::from("local")),
                    ("doc".into(), Value::from(doc)),
                    ("signature_docs".into(), Value::list(Vec::new())),
                    ("provenance".into(), Value::list(vec![Value::record(vec![
                        ("name".into(), Value::text_copy(s.text(node))),
                        ("from".into(), Value::from("file")),
                        ("path".into(), Value::from(file.path.to_string_lossy().replace('\\', "/"))),
                    ])])),
                ]));
            }
        }
    }
    Value::record(vec![
        ("name".into(), Value::from(name)),
        ("own_doc".into(), own_doc.map_or(Value::Null, Value::from)),
        ("declarations".into(), Value::list(declarations)),
        ("aliases".into(), Value::list(aliases)),
        ("selected_signature".into(), signature(file, node)),
    ])
}
