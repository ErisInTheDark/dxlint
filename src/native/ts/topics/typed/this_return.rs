//! Return-this annotations need a this-valued return and no class-valued return.

use super::*;

pub(super) fn offset(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(annotation) = s.get(node, Field::ReturnType) else { return Value::Null };
    if !matches!(s.kind(node), Some(Kind::Method | Kind::Arrow | Kind::Function)) { return Value::Null }
    let owner = match s.kind(node) {
        Some(Kind::Method) => s.parent(node),
        _ => s.parent(node).filter(|&parent| s.is(parent, Kind::ClassProp) && s.get(parent, Field::Init) == Some(node)).and_then(|parent| s.parent(parent)),
    };
    let Some(class) = owner.filter(|&owner| s.is(owner, Kind::Class)) else { return Value::Null };
    let Some(name) = s.word(class, Field::Name) else { return Value::Null };
    fn target(file: &NativeFile<'_>, annotation: u32, name: &str) -> Option<u32> {
        let s = &file.syntax;
        let annotation = super::constituents::bare(file, annotation);
        if s.is(annotation, Kind::TypeRef) && s.word(annotation, Field::Name) == Some(name) {
            Some(annotation)
        } else if s.is(annotation, Kind::Union) {
            s.all(annotation, Field::Type).find_map(|part| target(file, part, name))
        } else { None }
    }
    let Some(annotation) = target(file, annotation, name) else { return Value::Null };
    if s.all(node, Field::Param).next().and_then(|param| s.word(param, Field::Name)) == Some("this") { return Value::Null }
    let Some(body) = s.get(node, Field::Body) else { return Value::Null };
    if !s.is(body, Kind::Block) {
        return if is_this(file, body, 0) { Value::from(s.tree.node(annotation).start as usize) } else { Value::Null };
    }
    let class_type = file.type_of(annotation);
    fn contains_class(ty: &Ty, class: &Ty) -> bool {
        match (ty, class) {
            (Ty::Named(a, _), Ty::Named(b, _)) => a == b,
            (Ty::Union(parts), _) => parts.iter().any(|part| contains_class(part, class)),
            _ => false,
        }
    }
    let mut returned_this = false;
    for returned in file.return_sites(node).filter(|&node| s.is(node, Kind::Return)).filter_map(|node| s.get(node, Field::Arg)) {
        if is_this(file, returned, 0) {
            returned_this = true;
        } else if contains_class(&file.type_of(returned), &class_type) {
            return Value::Null;
        }
    }
    if returned_this { Value::from(s.tree.node(annotation).start as usize) } else { Value::Null }
}

fn is_this(file: &NativeFile<'_>, node: u32, depth: usize) -> bool {
    if depth >= 16 { return false }
    let s = &file.syntax;
    let node = s.unparen(node);
    if s.is(node, Kind::This) { return true }
    if !s.is(node, Kind::Ident) { return false }
    let Some(decl) = file.decl_of(node) else { return false };
    if file.binding(decl).is_none_or(|binding| binding.writes != 0) { return false }
    let Some(owner) = s.parent(decl).filter(|&owner| s.is(owner, Kind::Declarator) && s.get(owner, Field::Type).is_none()) else { return false };
    s.get(owner, Field::Init).is_some_and(|init| is_this(file, init, depth + 1))
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn nested_returns_do_not_escape_their_function_and_class_returns_block_this() {
        let linter = native_linter("rule prefer-return-this-type");
        let code = "class A { a(): A { function nested() { return new A(); } return this; } b(): A { const self = this; return self; } c(): A { if (ok) return new A(); return this; } d(this: A): A { return this; } }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }
}
