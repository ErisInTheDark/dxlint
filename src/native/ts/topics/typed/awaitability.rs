use super::*;

pub(super) fn type_awaitability(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let checker = &file.typing().checker;
    let mut thenables = Vec::new();
    let mut ordinary = Vec::new();
    let mut complete = true;
    partition(&file.constrained_type_of(node), checker, &mut thenables, &mut ordinary, &mut complete);
    let any = !thenables.is_empty();
    let all = any && ordinary.is_empty() && complete;
    let equivalent = if any && !ordinary.is_empty() && complete {
        let awaited = Ty::union(thenables.iter().map(|ty| checker.awaited(ty)).collect());
        let ordinary = Ty::union(ordinary);
        checker.assignable(&awaited, &ordinary).zip(checker.assignable(&ordinary, &awaited))
            .map_or(Value::Null, |(forward, reverse)| Value::Bool(forward && reverse))
    } else { Value::Null };
    Value::record(vec![
        ("any".into(), Value::Bool(any)),
        ("all".into(), Value::Bool(all)),
        ("union_equivalent".into(), equivalent),
        ("complete".into(), Value::Bool(complete)),
    ])
}

fn partition(ty: &Ty, checker: &Checker<'_>, thenables: &mut Vec<Ty>, ordinary: &mut Vec<Ty>, complete: &mut bool) {
    match ty {
        Ty::Union(parts) => for part in parts { partition(part, checker, thenables, ordinary, complete) },
        Ty::Never => {}
        Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) => {
            *complete = false;
            ordinary.push(ty.clone());
        }
        _ if checker.is_thenable(ty) => thenables.push(ty.clone()),
        _ => ordinary.push(ty.clone()),
    }
}

pub(super) fn nonawaitable(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    if file.syntax.is(node, Kind::Spread) {
        return file.syntax.get(node, Field::Arg).map_or(Value::Bool(false), |arg| iterable(file, arg));
    }
    Value::Bool(never(&file.constrained_type_of(node), &file.typing().checker))
}

fn never(ty: &Ty, ck: &Checker<'_>) -> bool {
    match ty {
        Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) => false,
        Ty::Union(parts) => parts.iter().all(|part| never(part, ck)),
        _ => !ck.is_thenable(ty),
    }
}

fn contains_nonawaitable(ty: &Ty, ck: &Checker<'_>) -> bool {
    match ty {
        Ty::Never => false,
        Ty::Union(parts) => parts.iter().any(|part| contains_nonawaitable(part, ck)),
        _ => never(ty, ck),
    }
}

pub(super) fn iterable(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    Value::Bool(invalid_iterable(&file.constrained_type_of(node), &file.typing().checker))
}

fn invalid_iterable(ty: &Ty, ck: &Checker<'_>) -> bool {
    match ty {
        Ty::Array(element) => contains_nonawaitable(&element.element, ck),
        Ty::Tuple(elements) => elements.iter().any(|element| contains_nonawaitable(element, ck)),
        Ty::Union(parts) => parts.iter().any(|part| invalid_iterable(part, ck)),
        Ty::Inter(parts) => parts.iter().any(|part| invalid_iterable(part, ck)),
        Ty::Named(sym, args) if ["Array", "ReadonlyArray", "Iterable", "IterableIterator", "Generator", "Set", "ReadonlySet"].iter().any(|name| ck.is_builtin_symbol(sym, name)) => {
            args.first().is_some_and(|element| contains_nonawaitable(element, ck))
        }
        _ => false,
    }
}
