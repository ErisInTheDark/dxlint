//! Interpolation policy as clauses: unions need every arm, intersections
//! need one arm. Each clause lists options or named types that permit it.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field, Kind};
use crate::native::ts::summary::{type_syn, DeclKind, TypeSyn};
use crate::native::ts::types::{Checker, Ty};
use super::specifiers::{named_gate, named_gates};

type Clauses = Vec<Vec<Value<'static>>>;
const MAX_CLAUSES: usize = 256;

pub(super) fn display(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let ty = file.constrained_type_of(node);
    if matches!(ty, Ty::Unknown | Ty::IntrinsicUnknown) {
        let s = &file.syntax;
        if let Some(annotation) = file.bindings().refs.get(&s.unparen(node)).filter(|_| matches!(file.type_of(node), Ty::Unknown | Ty::IntrinsicUnknown)).and_then(|&decl| s.parent(decl)).and_then(|parent| s.get(parent, Field::Type)) {
            if s.is(annotation, Kind::TypeRef) { return Value::from(s.text(annotation).to_string()) }
        }
        return Value::from("unknown");
    }
    Value::from(file.typing().checker.print(&ty))
}

pub(super) fn checks(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let ty = file.constrained_type_of(node);
    let ck = &file.typing().checker;
    let Some(mut checks) = clauses(&ty, ck, 0) else { return Value::Null };
    // Evaluation expands type aliases, but an explicit alias remains an
    // allowed type specifier even when its target has no named symbol.
    let s = &file.syntax;
    let bare = s.unparen(node);
    if let Some(&decl) = file.bindings().refs.get(&bare).filter(|_| matches!(ty, Ty::Obj(..))) {
        if let Some(annotation) = s.parent(decl).and_then(|parent| s.get(parent, Field::Type)) {
            if let TypeSyn::Ref(path, _) = type_syn(s, annotation) {
                for r in ck.type_decls(&path, &file.env_at(annotation)) {
                    if ck.summary_of(r.file).is_some_and(|summary| summary.decls[r.decl as usize].kind == DeclKind::Alias) {
                        let gate = named_gate(ck, r);
                        for clause in &mut checks { clause.push(gate.clone()); }
                    }
                }
            }
        }
    }
    Value::list(checks.into_iter().map(Value::list).collect())
}

fn option(name: &'static str) -> Value<'static> {
    Value::record(vec![("option".into(), Value::from(name))])
}

fn disjoin(left: Clauses, right: Clauses) -> Option<Clauses> {
    if left.len().saturating_mul(right.len()) > MAX_CLAUSES { return None }
    Some(left.into_iter().flat_map(|a| right.iter().map(move |b| [a.clone(), b.clone()].concat())).collect())
}

fn clauses(ty: &Ty, ck: &Checker<'_>, depth: usize) -> Option<Clauses> {
    if depth > 64 { return None }
    let single = |name| vec![vec![option(name)]];
    let out = match ty {
        Ty::EnumMember(member) => {
            let mut out = single(member.primitive()?);
            let gates = named_gates(&member.enum_type(), ck, &mut Vec::new());
            for clause in &mut out { clause.extend(gates.clone()); }
            out
        }
        Ty::Union(parts) => {
            let mut out = Vec::new();
            for part in parts {
                out.extend(clauses(part, ck, depth + 1)?);
                if out.len() > MAX_CLAUSES { return None }
            }
            out
        }
        Ty::Inter(parts) => {
            let mut out = vec![vec![]];
            for part in parts { out = disjoin(out, clauses(part, ck, depth + 1)?)?; }
            out
        }
        Ty::Prim("string") | Ty::Lit(_, "string") => vec![],
        Ty::Any => single("any"),
        Ty::Never => single("never"),
        Ty::Null | Ty::Undefined => single("nullish"),
        Ty::Prim("boolean") | Ty::Lit(_, "boolean") => single("boolean"),
        Ty::Prim("number" | "bigint") | Ty::Lit(_, "number" | "bigint") => single("number"),
        Ty::Array(element) => {
            let mut out = single("array");
            out.extend(clauses(&element.element, ck, depth + 1)?);
            out
        }
        Ty::Tuple(elements) => {
            let mut out = single("array");
            for element in elements.iter() {
                out.extend(clauses(element, ck, depth + 1)?);
                if out.len() > MAX_CLAUSES { return None }
            }
            out
        }
        Ty::Named(sym, args) => {
            let mut out = if ck.is_enum(sym) {
                single(if ck.is_string_enum(sym) { "string" } else { "number" })
            } else if super::is_array(ty, ck) {
                let mut out = single("array");
                if let Some(element) = args.first() { out.extend(clauses(element, ck, depth + 1)?); }
                out
            } else if ck.decl_name(sym).as_deref() == Some("RegExp") {
                single("regexp")
            } else {
                vec![vec![]]
            };
            let gates = named_gates(ty, ck, &mut Vec::new());
            for clause in &mut out { clause.extend(gates.clone()); }
            out
        }
        _ => vec![vec![]],
    };
    (out.len() <= MAX_CLAUSES).then_some(out)
}
