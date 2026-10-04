//! Enum member consistency across local merged declarations.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};
use std::collections::HashMap;

struct FirstMembers(HashMap<u32, u32>);

fn first_members(file: &NativeFile<'_>) -> FirstMembers {
    let s = &file.syntax;
    let mut enums: Vec<_> = (0..s.tree.nodes.len() as u32).filter(|&node| s.is(node, K::Enum)).collect();
    enums.sort_by_key(|&node| s.tree.node(node).start);
    let mut groups = HashMap::new();
    let mut members = HashMap::new();
    for node in enums {
        let Some(first) = s.all(node, F::Member).next() else { continue };
        let first = *groups.entry(group(file, node)).or_insert(first);
        members.insert(node, first);
    }
    FirstMembers(members)
}

#[derive(Clone, Copy, PartialEq)]
enum MemberType { Number, String, Unknown }

fn member_type(file: &NativeFile<'_>, member: u32) -> MemberType {
    let s = &file.syntax;
    let Some(init) = s.get(member, F::Init) else { return MemberType::Number };
    match s.kind(init) {
        Some(K::String | K::Template) => MemberType::String,
        Some(K::Bool | K::Null | K::Regex | K::BigInt) => MemberType::Unknown,
        Some(K::Member) => {
            let ck = &file.typing().checker;
            let base = s.get(init, F::Object).map(|object| file.type_of(object));
            let property = s.word(init, F::Property);
            let base = base.as_ref().zip(property).and_then(|(base, property)| ck.enum_member_base(base, property)).unwrap_or_else(|| file.type_of(init));
            if matches!(base, Ty::Prim("string") | Ty::Lit(_, "string")) { MemberType::String } else { MemberType::Number }
        }
        _ => match file.type_of(init) {
            Ty::Prim("string") | Ty::Lit(_, "string") => MemberType::String,
            _ => MemberType::Number,
        },
    }
}

fn group(file: &NativeFile<'_>, node: u32) -> (Vec<String>, Option<u32>) {
    let s = &file.syntax;
    let mut names = vec![s.word(node, F::Name).unwrap_or("").to_string()];
    let mut merge = file.bindings().decls.get(&s.get(node, F::Name).unwrap_or(node)).is_some_and(|binding| binding.exported);
    let mut parent = s.parent(node);
    while let Some(owner) = parent {
        if s.is(owner, K::Namespace) {
            if !merge { return (names, Some(owner)) }
            names.push(s.word(owner, F::Name).unwrap_or("global").to_string());
            merge = s.get(owner, F::Name).and_then(|name| file.bindings().decls.get(&name)).is_some_and(|binding| binding.exported);
        } else if matches!(s.kind(owner), Some(K::Function | K::Method | K::Arrow))
            || s.is(owner, K::Block) && !s.parent(owner).is_some_and(|parent| s.is(parent, K::Namespace)) {
            return (names, Some(owner));
        }
        parent = s.parent(owner);
    }
    (names, None)
}

pub(super) fn mixed(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(first) = s.all(node, F::Member).next() else { return Value::Null };
    let Some(name) = s.get(node, F::Name) else { return Value::Null };
    let bindings = file.bindings();
    let canonical = bindings.merged.get(&name).copied().unwrap_or(name);
    let previous = s.parent(canonical).filter(|&owner| s.is(owner, K::Enum) && s.tree.node(owner).start < s.tree.node(node).start)
        .and_then(|owner| s.all(owner, F::Member).next())
        .or_else(|| file.cached(first_members).0.get(&node).copied());
    let ck = &file.typing().checker;
    let imported = bindings.decls.iter().any(|(&decl, binding)| binding.kind == "import" && s.text(decl) == s.text(name))
        .then(|| ck.type_decls(&[s.text(name).into()], &crate::native::ts::types::Env::file(ck.file)))
        .and_then(|decls| decls.into_iter().find_map(|decl| {
            if decl.file == ck.file { return None }
            let summary = ck.summary_of(decl.file)?;
            let owner = &summary.decls[decl.decl as usize];
            if owner.kind != crate::native::ts::summary::DeclKind::Enum { return None }
            let member = owner.members.first()?;
            let ty = member.ty.as_ref().map_or(Ty::Prim("number"), |ty| ck.eval(ty, &ck.decl_env(decl)));
            Some(if matches!(ty, Ty::Prim("string") | Ty::Lit(_, "string")) { MemberType::String } else { MemberType::Number })
        }));
    let desired = imported.unwrap_or_else(|| member_type(file, previous.unwrap_or(first)));
    if desired == MemberType::Unknown { return Value::Null }
    for member in s.all(node, F::Member) {
        let current = member_type(file, member);
        if current == MemberType::Unknown { return Value::Null }
        if current != desired {
            let report = s.get(member, F::Init).unwrap_or(member);
            return Value::from(s.tree.node(report).start as usize);
        }
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn merged_enums_keep_the_first_member_kind_but_invalid_initialisers_stop_analysis() {
        let linter = native_linter("rule no-mixed-enums");
        let code = "enum E { A = 'a' } enum E { B = 1 } enum F { A = false, B = 'b' } enum G { A = 'a', B = 2, C = 3 }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn namespace_merging_stops_at_private_declarations() {
        let linter = native_linter("rule no-mixed-enums");
        let code = "namespace N { export enum E { A = 1 } enum Private { A = 1 } } namespace N { export enum E { B = 'b' } enum Private { B = 'b' } } namespace Outer { namespace Inner { export enum E { A = 1 } } } namespace Outer { namespace Inner { export enum E { B = 'b' } } } { enum BlockE { A = 1 } } { enum BlockE { B = 'b' } }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }
}
