use super::*;

#[cfg(test)]
#[path = "awaitability/tests.rs"]
mod tests;

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

pub(super) fn for_of(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !s.is(node, Kind::ForOf) || !s.has_word(node, Field::Modifier, "await") {
        return Value::Bool(false);
    }
    let Some(right) = s.get(node, Field::Right) else { return Value::Bool(false) };
    Value::Bool(!matches!(file.type_of(right), Ty::Any) && !has_async_symbol(file, right, "asyncIterator"))
}

pub(super) fn using(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let result = s.parent(node).filter(|&parent| s.is(parent, Kind::Declarator)).and_then(|decl| s.parent(decl))
        .is_some_and(|var| s.word(var, Field::Kind) == Some("await using"));
    Value::Bool(result && !matches!(file.type_of(node), Ty::Any) && !has_async_symbol(file, node, "asyncDispose"))
}

pub(super) fn has_async_symbol(file: &NativeFile<'_>, node: u32, name: &str) -> bool {
    let ty = file.type_of(node);
    let mut members = Vec::new();
    file.typing().checker.member_names(&ty, &mut members);
    let key = format!("[Symbol.{name}]");
    if members.iter().any(|member| member.chars().filter(|c| !c.is_whitespace()).eq(key.chars())) {
        return true;
    }
    let s = &file.syntax;
    let node = s.unparen(node);
    s.is(node, Kind::Object) && s.all(node, Field::Property).any(|property| {
        s.get(property, Field::Key).filter(|&key| s.is(key, Kind::Computed))
            .and_then(|key| s.get(key, Field::Expr)).is_some_and(|expr| {
                s.is(expr, Kind::Member) && s.word(expr, Field::Property) == Some(name)
                    && s.get(expr, Field::Object).is_some_and(|object| file.global_value(object).as_deref() == Some("Symbol"))
            })
    })
}
