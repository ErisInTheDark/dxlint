//! Unsafe value categories. Missing named annotations are error provenance;
//! unsupported type syntax and explicit `unknown` remain unknown.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::summary::{type_syn, TypeSyn};
use crate::native::ts::types::Ty;

pub(super) fn kind(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    category(file, node).map_or(Value::Null, Value::from)
}

pub(super) fn call(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    if let Some(kind) = category(file, node) { return Value::from(kind) }
    let construct = file.syntax.parent(node).is_some_and(|parent| file.syntax.is(parent, K::New));
    if file.typing().checker.unsafe_builtin_function(&file.constrained_type_of(node), construct) { Value::from("function") } else { Value::Null }
}

pub(super) fn key(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let bare = file.syntax.unparen(node);
    Value::Record(vec![
        ("kind".into(), category(file, bare).map_or(Value::Null, Value::from)),
        ("text".into(), Value::from(file.syntax.text(bare).to_string())),
        ("offset".into(), Value::from(file.syntax.tree.node(bare).start as usize)),
    ])
}

pub(super) fn this_kind(file: &NativeFile<'_>, mut node: u32) -> Value<'static> {
    let s = &file.syntax;
    loop {
        node = s.unparen(node);
        if s.is(node, K::This) { return kind(file, node) }
        let field = match s.kind(node) {
            Some(K::Member | K::Index) => F::Object,
            Some(K::As | K::Satisfies | K::NonNull | K::TypeAssertion) => F::Expr,
            _ => return Value::Null,
        };
        let Some(inner) = s.get(node, field) else { return Value::Null };
        node = inner;
    }
}

pub(super) fn category(file: &NativeFile<'_>, node: u32) -> Option<&'static str> {
    fn unresolved(ty: &Ty) -> bool {
        match ty {
            Ty::Unknown | Ty::IntrinsicUnknown | Ty::Any => true,
            Ty::Union(parts) | Ty::Inter(parts) => parts.iter().any(unresolved),
            _ => false,
        }
    }
    let ty = file.constrained_type_of(node);
    if unresolved(&ty) && error_origin(file, node, 0) { return Some("error") }
    matches!(ty, Ty::Any).then_some("any")
}

/// Missing type/value provenance of an array element or one tuple position.
pub(super) fn element_error(file: &NativeFile<'_>, node: u32, index: Option<usize>) -> bool {
    fn check(file: &NativeFile<'_>, node: u32, index: Option<usize>, depth: usize) -> bool {
        if depth >= 64 { return false }
        let s = &file.syntax;
        let annotation_error = |annotation| {
            if s.is(annotation, K::ArrayType) {
                s.get(annotation, F::Type).is_some_and(|element| missing_annotation(file, element))
            } else if s.is(annotation, K::TupleType) {
                index.and_then(|index| s.all(annotation, F::Element).nth(index)).and_then(|element| s.get(element, F::Type)).is_some_and(|element| missing_annotation(file, element))
            } else { false }
        };
        match s.kind(node) {
            Some(K::Ident) => file.decl_of(node).and_then(|decl| s.parent(decl)).is_some_and(|owner| {
                s.get(owner, F::Type).map_or_else(|| s.get(owner, F::Init).is_some_and(|init| check(file, init, index, depth + 1)), annotation_error)
            }),
            Some(K::Paren | K::Satisfies | K::NonNull) => s.get(node, F::Expr).is_some_and(|expr| check(file, expr, index, depth + 1)),
            Some(K::As | K::TypeAssertion) => s.get(node, F::Type).is_some_and(|annotation| {
                if s.text(annotation) == "const" { s.get(node, F::Expr).is_some_and(|expr| check(file, expr, index, depth + 1)) } else { annotation_error(annotation) }
            }),
            Some(K::Array) => index.and_then(|index| s.all(node, F::Element).nth(index)).is_some_and(|element| error_origin(file, element, depth + 1)),
            _ => false,
        }
    }
    check(file, node, index, 0)
}

pub(super) fn missing_annotation(file: &NativeFile<'_>, annotation: u32) -> bool {
    let s = &file.syntax;
    if matches!(s.kind(annotation), Some(K::Union | K::Intersection | K::ParenType)) {
        return s.all(annotation, F::Type).any(|part| missing_annotation(file, part));
    }
    if !s.is(annotation, K::TypeRef) { return false }
    if s.get(annotation, F::Name).and_then(|name| file.decl_of(name)).and_then(|decl| s.parent(decl)).is_some_and(|owner| matches!(s.kind(owner), Some(K::TypeAlias | K::Interface | K::Class | K::Enum | K::TypeParam))) {
        return false;
    }
    let ck = &file.typing().checker;
    let env = file.env_at(annotation);
    match type_syn(s, annotation) {
        TypeSyn::Ref(path, _) if path.len() == 1 => !env.tparams.iter().any(|(name, _)| name == &path[0]) && ck.lookup(&env, &path[0]).is_empty(),
        _ => false,
    }
}

fn error_origin(file: &NativeFile<'_>, node: u32, depth: usize) -> bool {
    if depth >= 64 { return false }
    let s = &file.syntax;
    let recurse = |node| error_origin(file, node, depth + 1);
    match s.kind(node) {
        Some(K::Ident) => {
            let Some(&decl) = file.bindings().refs.get(&node) else {
                return file.bindings().unresolved.contains(&node) && file.global_value(node).is_none()
                    && file.typing().checker.lookup(&file.env_at(node), s.text(node)).is_empty();
            };
            let Some(owner) = s.parent(decl) else { return false };
            if let Some(annotation) = s.get(owner, F::Type) { return missing_annotation(file, annotation) }
            s.get(owner, F::Init).is_some_and(recurse)
        }
        Some(K::As | K::TypeAssertion) => s.get(node, F::Type).is_some_and(|annotation| missing_annotation(file, annotation)),
        Some(K::Paren | K::NonNull | K::Satisfies) => s.get(node, F::Expr).is_some_and(recurse),
        Some(K::Member | K::Index) => s.get(node, F::Object).is_some_and(recurse),
        Some(K::Call | K::New) => s.get(node, F::Callee).is_some_and(recurse),
        Some(K::Assign) if s.word(node, F::Op) == Some("=") => s.get(node, F::Right).is_some_and(recurse),
        Some(K::Object) => s.all(node, F::Property).filter(|&property| s.is(property, K::Spread)).filter_map(|property| s.get(property, F::Arg)).any(recurse),
        Some(K::This) => {
            let mut parent = s.parent(node);
            while let Some(owner) = parent {
                if matches!(s.kind(owner), Some(K::Function | K::Method)) {
                    return s.all(owner, F::Param).find(|&param| s.word(param, F::Name) == Some("this")).and_then(|param| s.get(param, F::Type)).is_some_and(|annotation| missing_annotation(file, annotation));
                }
                parent = s.parent(owner);
            }
            false
        }
        _ => false,
    }
}
