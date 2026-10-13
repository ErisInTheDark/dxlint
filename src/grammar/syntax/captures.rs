//! Private grammar captures have one owner on each successful path.

use std::collections::{BTreeMap, BTreeSet};

use crate::pattern::syntax::{Definition, Expr};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Count { Unused, Once, Repeated }

type Counts = BTreeSet<Count>;
type Uses = BTreeMap<String, Counts>;

fn unused() -> Counts { BTreeSet::from([Count::Unused]) }

fn sum(left: &Counts, right: &Counts) -> Counts {
    left.iter().flat_map(|left| right.iter().map(move |right| {
        match (*left, *right) {
            (Count::Unused, value) | (value, Count::Unused) => value,
            _ => Count::Repeated,
        }
    })).collect()
}

fn combine(left: Option<Uses>, right: Option<Uses>, alternatives: bool) -> Option<Uses> {
    let (left, right) = match (left, right) {
        (Some(left), Some(right)) => (left, right),
        (left, right) if alternatives => return left.or(right),
        _ => return None,
    };
    let names: BTreeSet<_> = left.keys().chain(right.keys()).cloned().collect();
    Some(names.into_iter().map(|name| {
        let left = left.get(&name).cloned().unwrap_or_else(unused);
        let right = right.get(&name).cloned().unwrap_or_else(unused);
        let counts = if alternatives { left.union(&right).copied().collect() } else { sum(&left, &right) };
        (name, counts)
    }).collect())
}

fn visit(expression: &Expr, names: &mut Vec<String>) -> Result<Option<Uses>, String> {
    match expression {
        Expr::Reuse(name) => Ok(Some(BTreeMap::from([(name.clone(), BTreeSet::from([Count::Once]))]))),
        Expr::Scope { capture, header, body } => {
            if !names.contains(capture) { names.push(capture.clone()); }
            let header = visit(header, names)?;
            let Some(mut body) = visit(body, names)? else {
                return Err(format!("private capture `{capture}` is never moved"));
            };
            if body.remove(capture).unwrap_or_else(unused) != BTreeSet::from([Count::Once]) {
                return Err(format!("private capture `{capture}` must move exactly once on every successful path"));
            }
            Ok(combine(header, Some(body), false))
        }
        Expr::Seq(items) => items.iter().try_fold(Some(Uses::new()), |uses, item| {
            Ok(combine(uses, visit(item, names)?, false))
        }),
        Expr::Choice(items) => {
            let mut items = items.iter();
            let first = match items.next() { Some(item) => visit(item, names)?, None => None };
            items.try_fold(first, |uses, item| Ok(combine(uses, visit(item, names)?, true)))
        }
        Expr::Flag { yes, no, .. } => Ok(combine(visit(yes, names)?, visit(no, names)?, true)),
        Expr::Repeat { inner, min, max } => {
            let Some(mut uses) = visit(inner, names)? else {
                return Ok((*min == 0).then(Uses::new));
            };
            for counts in uses.values_mut() {
                let mut repeated = Counts::new();
                if *min == 0 { repeated.insert(Count::Unused); }
                if *min <= 1 && max.is_none_or(|max| max >= 1) { repeated.extend(counts.iter().copied()); }
                if max.is_none_or(|max| max >= 2) { repeated.extend(sum(counts, counts)); }
                *counts = repeated;
            }
            Ok(Some(uses))
        }
        Expr::Bind { inner, .. } | Expr::Emit { inner, .. } | Expr::Context { inner, .. } |
        Expr::Try(inner) => visit(inner, names),
        Expr::Discard(inner) | Expr::Text(inner) | Expr::Look { inner, .. } => {
            let uses = visit(inner, names)?;
            if uses.as_ref().is_some_and(|uses| !uses.is_empty()) {
                return Err("private capture moves cannot be discarded".into());
            }
            if matches!(expression, Expr::Look { negative: true, .. }) {
                Ok(Some(Uses::new()))
            } else { Ok(uses) }
        }
        Expr::Pratt(precedence) => {
            let atom = visit(&precedence.atom, names)?;
            for operator in precedence.prefix.iter().map(|prefix| &prefix.operator)
                .chain(precedence.rows.iter().map(|row| &row.operator))
            {
                if visit(operator, names)?.is_some_and(|uses| !uses.is_empty()) {
                    return Err("private capture moves cannot repeat in precedence operators".into());
                }
            }
            if atom.as_ref().is_some_and(|uses| !uses.is_empty())
                && (!precedence.prefix.is_empty() || precedence.rows.iter().any(|row| row.operand))
            {
                return Err("private capture moves cannot cross recursive precedence operands".into());
            }
            Ok(atom)
        }
        _ => Ok(Some(Uses::new())),
    }
}

pub(super) fn validate(definition: &Definition) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for production in &definition.productions {
        let free = visit(&production.body, &mut names)?;
        if let Some(name) = free.as_ref().and_then(|uses| uses.keys().next()) {
            return Err(format!("private capture `{name}` is outside its scope"));
        }
    }
    Ok(names)
}
