//! Function return information and promise callback facts.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::summary::sig;

fn name_groups(ty: &Ty, ck: &Checker<'_>, depth: usize) -> Vec<Vec<String>> {
    if depth >= 64 { return vec![Vec::new()] }
    match ty {
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().flat_map(|part| name_groups(part, ck, depth + 1)).collect(),
        Ty::Named(symbols, args) => {
            let mut names = Vec::new();
            let mut groups = Vec::new();
            for &reference in symbols.iter() {
                let Some(summary) = ck.summary_of(reference.file) else { continue };
                let decl = &summary.decls[reference.decl as usize];
                names.push(decl.name.to_string());
                let mut env = ck.decl_env(reference);
                env.tparams = ck.bind_tparams(&decl.type_params, args.clone(), &env);
                for base in ck.bases(decl) { groups.extend(name_groups(&ck.eval(base, &env), ck, depth + 1)); }
            }
            if groups.is_empty() { groups.push(Vec::new()); }
            for group in &mut groups { group.extend(names.iter().cloned()); }
            groups
        }
        _ => vec![Vec::new()],
    }
}

pub(super) fn return_info(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !matches!(s.kind(node), Some(K::Function | K::Arrow | K::Method)) { return Value::Null }
    let ck = &file.typing().checker;
    let signature = sig(s, node);
    let mut returns = Vec::new();
    if let Some(name) = s.get(node, F::Name) {
        let named = file.type_of(name);
        if matches!(&named, Ty::Fn(callable) if callable.sigs.len() > 1) { returns = ck.declared_returns(&named); }
    }
    if returns.is_empty() {
        returns.push(signature.ret.as_ref().map_or_else(
            || file.own_return(&signature, ck.file).unwrap_or(Ty::Unknown),
            |ret| ck.eval(ret, &file.env_at(node)),
        ));
    }
    Value::record(vec![
        ("explicit".into(), Value::Bool(s.get(node, F::ReturnType).is_some())),
        ("returns".into(), Value::list(returns.iter().map(|ty| Value::record(vec![
            ("unknown".into(), Value::Bool(matches!(ty, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown))),
            ("union".into(), Value::Bool(matches!(ty, Ty::Union(_)))),
            ("groups".into(), Value::list(name_groups(ty, ck, 0).into_iter().map(|group| Value::list(group.into_iter().map(Value::from).collect())).collect())),
        ])).collect())),
    ])
}
