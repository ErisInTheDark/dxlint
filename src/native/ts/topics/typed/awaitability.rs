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
