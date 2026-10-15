//! Type-specifier provenance shared by typed option policies.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::modules::{DeclRef, FileId};
use crate::native::ts::summary::{type_syn, DeclKind, TypeSyn};

pub(crate) fn named_gate(ck: &Checker<'_>, decl: DeclRef) -> Value<'static> {
    let name = ck.summary_of(decl.file).map_or_else(String::new, |summary| summary.decls[decl.decl as usize].name.to_string());
    file_gate(ck, decl.file, &name)
}

pub(crate) fn file_gate(ck: &Checker<'_>, file: FileId, name: &str) -> Value<'static> {
    let path = ck.project.path(file).to_string_lossy().replace('\\', "/");
    let package = path.split("/node_modules/").nth(1).map(|tail| {
        let mut parts = tail.split('/');
        let first = parts.next().unwrap_or("");
        if first.starts_with('@') { format!("{first}/{}", parts.next().unwrap_or("")) } else { first.to_string() }
    });
    let from = if ck.is_lib_file(file) { "lib" } else if package.is_some() { "package" } else { "file" };
    Value::record(vec![
        ("name".into(), Value::text_copy(name)),
        ("from".into(), Value::from(from)),
        ("path".into(), Value::from(path)),
        ("package".into(), package.map_or(Value::Null, Value::from)),
    ])
}

pub(super) fn named_types(ty: &Ty, ck: &Checker<'_>, seen: &mut Vec<DeclRef>) -> Vec<(DeclRef, Ty)> {
    if let Ty::EnumMember(member) = ty { return named_types(&member.enum_type(), ck, seen) }
    let Ty::Named(sym, args) = ty else { return Vec::new() };
    let mut gates = Vec::new();
    for &r in sym.iter() {
        if seen.contains(&r) { continue }
        seen.push(r);
        gates.push((r, Ty::Named(vec![r].into(), args.clone())));
        let Some(summary) = ck.summary_of(r.file) else { continue };
        let decl = &summary.decls[r.decl as usize];
        let mut env = ck.decl_env(r);
        env.tparams = ck.bind_tparams(&decl.type_params, args.clone(), &env);
        for base in ck.bases(decl) { gates.extend(named_types(&ck.eval(base, &env), ck, seen)); }
    }
    gates
}

fn candidates(ty: &Ty, ck: &Checker<'_>, depth: usize) -> Vec<Value<'static>> {
    if depth >= 64 { return Vec::new() }
    match ty {
        Ty::EnumMember(member) => member.symbols.iter().map(|&decl| named_gate(ck, decl)).collect(),
        Ty::Named(sym, _) | Ty::Class(sym) => sym.iter().map(|&decl| named_gate(ck, decl)).collect(),
        Ty::Inter(parts) => parts.iter().flat_map(|part| candidates(part, ck, depth + 1)).collect(),
        Ty::Prim(name) | Ty::Lit(_, name) => vec![Value::record(vec![("name".into(), Value::from(*name)), ("from".into(), Value::from("lib"))])],
        Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Never | Ty::Void | Ty::Undefined | Ty::Null => {
            let name = match ty { Ty::Any => "any", Ty::Unknown | Ty::IntrinsicUnknown => "unknown", Ty::Never => "never", Ty::Void => "void", Ty::Undefined => "undefined", _ => "null" };
            vec![Value::record(vec![("name".into(), Value::from(name)), ("from".into(), Value::from("lib"))])]
        }
        _ => Vec::new(),
    }
}

pub(super) fn annotated_aliases(file: &NativeFile<'_>, node: u32) -> Vec<DeclRef> {
    let s = &file.syntax;
    let bare = s.unparen(node);
    let annotation = if s.is(bare, K::As) { s.get(bare, F::Type) } else { file.decl_of(bare).and_then(|decl| s.parent(decl)).and_then(|owner| s.get(owner, F::Type)) };
    let Some(annotation) = annotation else { return Vec::new() };
    let TypeSyn::Ref(path, _) = type_syn(s, annotation) else { return Vec::new() };
    let ck = &file.typing().checker;
    ck.type_decls(&path, &file.env_at(annotation)).into_iter().filter(|decl| {
        ck.summary_of(decl.file).is_some_and(|summary| summary.decls[decl.decl as usize].kind == DeclKind::Alias)
    }).collect()
}

pub(super) fn retained_alias(ck: &Checker<'_>, r: DeclRef, seen: &mut Vec<DeclRef>) -> Option<DeclRef> {
    if seen.contains(&r) { return None }
    seen.push(r);
    let summary = ck.summary_of(r.file)?;
    let decl = &summary.decls[r.decl as usize];
    match decl.ty.as_ref()? {
        TypeSyn::Obj(_) | TypeSyn::Union(_) | TypeSyn::Inter(_) | TypeSyn::Array(_) | TypeSyn::Tuple(_) | TypeSyn::Readonly(_) => Some(r),
        TypeSyn::Ref(path, _) => {
            let retained = ck.type_decls(path, &ck.decl_env(r)).into_iter().find_map(|target| {
                let summary = ck.summary_of(target.file)?;
                (summary.decls[target.decl as usize].kind == DeclKind::Alias).then(|| retained_alias(ck, target, seen)).flatten()
            })?;
            Some(if decl.type_params.is_empty() { retained } else { r })
        }
        _ => None,
    }
}

/// A single allowed specifier must match every union arm; intersections need one candidate.
pub(super) fn groups(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    if super::unsafe_types::category(file, node) == Some("error") { return Value::list(vec![Value::list(Vec::new())]) }
    let ty = file.type_of(node);
    let ck = &file.typing().checker;
    let mut groups = match &ty {
        Ty::Union(parts) => parts.iter().map(|part| candidates(part, ck, 0)).collect::<Vec<_>>(),
        _ => vec![candidates(&ty, ck, 0)],
    };
    if !matches!(ty, Ty::Union(_)) {
        groups[0].extend(annotated_aliases(file, node).into_iter().map(|decl| named_gate(ck, decl)));
        if matches!(ty, Ty::Fn(_)) && file.syntax.is(file.syntax.unparen(node), K::Ident) {
            groups[0].extend(ck.lookup(&file.env_at(node), file.syntax.text(file.syntax.unparen(node))).into_iter().filter(|r| {
                ck.summary_of(r.file).is_some_and(|s| s.decls[r.decl as usize].kind == DeclKind::Function)
            }).map(|decl| named_gate(ck, decl)));
        }
    }
    Value::list(groups.into_iter().map(Value::list).collect())
}
