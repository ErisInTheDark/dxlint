//! Type-specifier provenance shared by typed option policies.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::modules::DeclRef;
use crate::native::ts::summary::{type_syn, DeclKind, TypeSyn};

pub(super) fn named_gate(ck: &Checker<'_>, decl: DeclRef) -> Value<'static> {
    let name = ck.summary_of(decl.file).map_or_else(String::new, |summary| summary.decls[decl.decl as usize].name.to_string());
    let path = ck.project.path(decl.file).to_string_lossy().replace('\\', "/");
    let package = path.split("/node_modules/").nth(1).map(|tail| {
        let mut parts = tail.split('/');
        let first = parts.next().unwrap_or("");
        if first.starts_with('@') { format!("{first}/{}", parts.next().unwrap_or("")) } else { first.to_string() }
    });
    let from = if ck.is_lib_decl(decl) { "lib" } else if package.is_some() { "package" } else { "file" };
    Value::Record(vec![
        ("name".into(), Value::from(name)),
        ("from".into(), Value::from(from)),
        ("path".into(), Value::from(path)),
        ("package".into(), package.map_or(Value::Null, Value::from)),
    ])
}

/// Template policy also permits named base types.
pub(super) fn named_gates(ty: &Ty, ck: &Checker<'_>, seen: &mut Vec<DeclRef>) -> Vec<Value<'static>> {
    let Ty::Named(sym, args) = ty else { return Vec::new() };
    let mut gates = Vec::new();
    for &r in sym.iter() {
        if seen.contains(&r) { continue }
        seen.push(r);
        gates.push(named_gate(ck, r));
        let Some(summary) = ck.summary_of(r.file) else { continue };
        let decl = &summary.decls[r.decl as usize];
        let mut env = ck.decl_env(r);
        env.tparams = ck.bind_tparams(&decl.type_params, args.clone(), &env);
        for base in ck.bases(decl) { gates.extend(named_gates(&ck.eval(base, &env), ck, seen)); }
    }
    gates
}

fn candidates(ty: &Ty, ck: &Checker<'_>, depth: usize) -> Vec<Value<'static>> {
    if depth >= 64 { return Vec::new() }
    match ty {
        Ty::Named(sym, _) => sym.iter().map(|&decl| named_gate(ck, decl)).collect(),
        Ty::Inter(parts) => parts.iter().flat_map(|part| candidates(part, ck, depth + 1)).collect(),
        Ty::Prim(name) => vec![Value::Record(vec![("name".into(), Value::from(*name)), ("from".into(), Value::from("lib"))])],
        Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Never | Ty::Void | Ty::Undefined | Ty::Null => {
            let name = match ty { Ty::Any => "any", Ty::Unknown | Ty::IntrinsicUnknown => "unknown", Ty::Never => "never", Ty::Void => "void", Ty::Undefined => "undefined", _ => "null" };
            vec![Value::Record(vec![("name".into(), Value::from(name)), ("from".into(), Value::from("lib"))])]
        }
        _ => Vec::new(),
    }
}

/// A single allowed specifier must match every union arm; intersections need one candidate.
pub(super) fn groups(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    if super::unsafe_types::category(file, node) == Some("error") { return Value::List(vec![Value::List(Vec::new())]) }
    let ty = file.type_of(node);
    let ck = &file.typing().checker;
    let s = &file.syntax;
    let mut groups = match &ty {
        Ty::Union(parts) => parts.iter().map(|part| candidates(part, ck, 0)).collect::<Vec<_>>(),
        _ => vec![candidates(&ty, ck, 0)],
    };
    if !matches!(ty, Ty::Union(_)) {
        let bare = s.unparen(node);
        let annotation = if s.is(bare, K::As) { s.get(bare, F::Type) } else { file.decl_of(bare).and_then(|decl| s.parent(decl)).and_then(|owner| s.get(owner, F::Type)) };
        if let Some(annotation) = annotation {
            if let TypeSyn::Ref(path, _) = type_syn(s, annotation) {
                for decl in ck.type_decls(&path, &file.env_at(annotation)) {
                    if ck.summary_of(decl.file).is_some_and(|summary| summary.decls[decl.decl as usize].kind == DeclKind::Alias) {
                        groups[0].push(named_gate(ck, decl));
                    }
                }
            }
        }
    }
    Value::List(groups.into_iter().map(Value::List).collect())
}
