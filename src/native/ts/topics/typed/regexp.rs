//! Match-to-exec suggestions require a provably nonglobal pattern.

use super::*;
use crate::native::ts::statics;

pub(super) fn exec(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(callee) = s.get(node, Field::Callee) else { return Value::Null };
    if !s.is(node, Kind::Call) || statics::property_name(s, callee).as_deref() != Some("match") { return Value::Null }
    let args: Vec<_> = s.all(node, Field::Arg).collect();
    let ([arg], Some(object), Some(property)) = (&args[..], s.get(callee, Field::Object), s.get(callee, Field::Property).or_else(|| s.get(callee, Field::Index))) else { return Value::Null };
    let ck = &file.typing().checker;
    if !strings(&file.constrained_type_of(object)) || !nonglobal(file, *arg, 0) { return Value::Null }
    if s.is(*arg, Kind::String) {
        let Some(pattern) = file.static_string(*arg) else { return Value::Null };
        if crate::native::ts::regex::pattern_error(&pattern, false, false).is_some() { return Value::Null }
    } else {
        fn argument(ty: &Ty, ck: &Checker<'_>) -> u8 {
            match ty {
                Ty::Union(parts) => parts.iter().fold(0, |flags, part| flags | argument(part, ck)),
                Ty::Prim("string") | Ty::Lit(_, "string") => 1,
                Ty::Named(sym, _) if ck.decl_name(sym).as_deref() == Some("RegExp") => 2,
                _ => 0,
            }
        }
        if !matches!(argument(&file.type_of(*arg), ck), 1 | 2) { return Value::Null }
    }
    Value::from(s.tree.node(property).start as usize)
}

fn strings(ty: &Ty) -> bool {
    match ty {
        Ty::Prim("string") | Ty::Lit(_, "string") => true,
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(strings),
        Ty::Inter(parts) => parts.iter().any(strings),
        _ => false,
    }
}

fn nonglobal(file: &NativeFile<'_>, node: u32, depth: usize) -> bool {
    if depth >= 16 { return false }
    let s = &file.syntax;
    let node = s.unparen(node);
    if let Some(site) = file.regex_site(node) {
        return site.flags.is_some_and(|flags| !flags.contains('g'));
    }
    if file.static_string(node).is_some() { return true }
    if matches!(s.kind(node), Some(Kind::Call | Kind::New)) && s.get(node, Field::Callee).is_some_and(|callee| s.is(callee, Kind::Ident) && s.text(callee) == "RegExp") {
        return s.all(node, Field::Arg).nth(1).is_none_or(|flags| file.static_string(flags).is_some_and(|flags| !flags.contains('g')) || file.static_number(flags).is_some()
            || s.is(flags, Kind::Ident) && s.text(flags) == "undefined" && file.decl_of(flags).is_none());
    }
    if s.is(node, Kind::Ident) {
        let Some(decl) = file.decl_of(node) else { return false };
        if !file.binding(decl).is_some_and(|binding| binding.writes == 0 && matches!(binding.kind, "const" | "let" | "var")) { return false }
        return s.parent(decl).filter(|&owner| s.is(owner, Kind::Declarator)).and_then(|owner| s.get(owner, Field::Init)).is_some_and(|init| nonglobal(file, init, depth + 1));
    }
    false
}
