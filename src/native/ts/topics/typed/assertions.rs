use super::*;

pub(super) fn unsafe_assertion(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !matches!(s.kind(node), Some(Kind::As | Kind::TypeAssertion)) { return Value::Null }
    let (Some(expr), Some(annotation)) = (s.get(node, Field::Expr), s.get(node, Field::Type)) else { return Value::Null };
    if s.is(annotation, Kind::TypeRef) && s.word(annotation, Field::Name) == Some("const") { return Value::Null }
    let source = file.type_of(expr);
    let missing_target = super::unsafe_types::missing_annotation(file, annotation);
    let target = if missing_target { Ty::Any } else { file.type_of(annotation) };
    if source.same_identity(&target) { return Value::Null }
    let ck = &file.typing().checker;
    let finding = |kind, ty: String| Value::record(vec![
        ("kind".into(), Value::from(kind)),
        ("type".into(), Value::from(ty)),
    ]);
    let any_name = |at| if at == annotation && missing_target || super::unsafe_types::category(file, at) == Some("error") { "error typed" } else { "`any`" };
    if matches!((&source, &target), (Ty::Unknown | Ty::IntrinsicUnknown, Ty::Any)) {
        return finding("to-any", "`any`".into());
    }
    if !super::transfers::empty_map(file, expr) && source.unsafe_assignment_to(&target) {
        return finding("from-any", any_name(expr).into());
    }
    if target.unsafe_assignment_to(&source) {
        return finding("to-any", any_name(annotation).into());
    }
    if ck.assignable(&source, &target) == Some(true) { return Value::Null }
    let mut direct = source.clone();
    let mut visited = Vec::new();
    while let Ty::Param(name) = &direct {
        if visited.contains(name) { break }
        visited.push(name.clone());
        let Some(bound) = file.type_parameter_bound(name, expr) else { break };
        if ck.assignable(&bound, &target) == Some(true) { return Value::Null }
        direct = bound;
    }
    let bounded = file.constrained_type_of(expr);
    if ck.assignable(&bounded, &target) == Some(true) { return Value::Null }
    if let Ty::Param(name) = &target {
        let constraint = file.constrained_type_of(annotation);
        if matches!(constraint, Ty::Param(_)) { return finding("unconstrained", ck.print(&target)) }
        if matches!(file.type_parameter_bound(name, annotation), Some(Ty::Any)) || ck.assignable(&bounded, &constraint) == Some(true) { return finding("constraint", ck.print(&target)) }
        if ck.assignable(&bounded, &constraint) == Some(false) { return finding("narrow", ck.print(&target)) }
    }
    // An unsupported syntax type is not proof of a narrowing assertion.
    if ck.assignable(&bounded, &target) == Some(false) || matches!(source, Ty::Param(_)) && !matches!(target, Ty::Unknown | Ty::Param(_)) {
        return finding("narrow", ck.print(&target));
    }
    Value::Null
}

pub(super) fn non_null(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !matches!(s.kind(node), Some(Kind::As | Kind::TypeAssertion)) {
        return Value::Bool(false);
    }
    let (Some(expr), Some(annotation)) = (s.get(node, Field::Expr), s.get(node, Field::Type)) else { return Value::Bool(false) };
    if s.is(annotation, Kind::TypeRef) && s.word(annotation, Field::Name) == Some("const") {
        return Value::Bool(false);
    }
    let original = file.type_of(expr);
    let asserted = file.type_of(annotation);
    if matches!(original, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown) || matches!(asserted, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown) {
        return Value::Bool(false);
    }
    let parts = |ty: Ty| match ty { Ty::Union(parts) => parts, ty => vec![ty] };
    let original = parts(original);
    let asserted = parts(asserted);
    let stripped: Vec<_> = original.iter().filter(|ty| !matches!(ty, Ty::Null | Ty::Undefined)).collect();
    let constrained = file.constrained_type_of(annotation);
    fn nullable(ty: &Ty) -> bool {
        match ty {
            Ty::Null | Ty::Undefined | Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) => true,
            Ty::Union(parts) => parts.iter().any(nullable),
            _ => false,
        }
    }
    Value::Bool(stripped.len() != original.len() && !nullable(&constrained)
        && asserted.iter().all(|a| stripped.iter().any(|b| a.same_identity(b)))
        && stripped.iter().all(|a| asserted.iter().any(|b| a.same_identity(b))))
}

pub(super) fn reduce(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !matches!(s.kind(node), Some(Kind::As | Kind::TypeAssertion)) {
        return Value::Bool(false);
    }
    let Some(call) = s.parent(node).filter(|&parent| s.is(parent, Kind::Call) && s.all(parent, Field::Arg).nth(1) == Some(node)) else { return Value::Bool(false) };
    let Some(callee) = s.get(call, Field::Callee) else { return Value::Bool(false) };
    if crate::native::ts::statics::property_name(s, callee).as_deref() != Some("reduce") {
        return Value::Bool(false);
    }
    let (Some(object), Some(expr), Some(annotation)) = (s.get(callee, Field::Object), s.get(node, Field::Expr), s.get(node, Field::Type)) else { return Value::Bool(false) };
    let ck = &file.typing().checker;
    fn arrays(ty: &Ty, ck: &Checker<'_>) -> bool {
        match ty {
            Ty::Inter(parts) | Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|part| arrays(part, ck)),
            _ => is_array(ty, ck),
        }
    }
    let initial = file.constrained_type_of(expr);
    let target = file.type_of(annotation);
    let assignable = if s.is(s.unparen(expr), Kind::Object) { ck.assignable_fresh_object(&initial, &target) } else { ck.assignable(&initial, &target) };
    Value::Bool(arrays(&file.constrained_type_of(object), ck) && assignable == Some(true))
}
