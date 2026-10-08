//! Item-stage values and ordering; node reads and parameter frames stay with Ctx.

use super::{CExpr, Each, Value};

#[derive(Debug)]
pub struct Stage {
    pub kind: Each,
    pub depth: u16,
    pub arity: u8,
    pub input: CExpr,
    pub args: Vec<CExpr>,
    pub body: CExpr,
}

pub(super) fn run<'a>(
    kind: Each,
    input: &Value<'a>,
    args: &[Value<'a>],
    arity: u8,
    mut body: impl FnMut(&[Value<'a>]) -> Result<Value<'a>, String>,
) -> Result<Value<'a>, String> {
    let items = input.item_slice();
    if matches!(kind, Each::Fold | Each::Reduce) {
        let (mut state, rest) = if kind == Each::Fold {
            (args[0].clone(), items)
        } else {
            let Some((first, rest)) = items.split_first() else { return Ok(Value::Null) };
            (first.clone(), rest)
        };
        for item in rest {
            state = body(&[state, item.clone()])?;
        }
        return Ok(state);
    }
    if kind == Each::Zip {
        return Ok(Value::list(items.iter().zip(args[0].item_slice())
            .map(|(a, b)| body(&[a.clone(), b.clone()])).collect::<Result<_, _>>()?));
    }

    let mut call = |index: usize, item: &Value<'a>| {
        let bindings = [item.clone(), Value::from(index)];
        body(&bindings[..arity as usize])
    };
    if kind == Each::FindLast {
        for (index, item) in items.iter().enumerate().rev() {
            if call(index, item)?.truthy() { return Ok(item.clone()); }
        }
        return Ok(Value::Null);
    }

    let mut out = Vec::new();
    let mut no = Vec::new();
    let mut keys = Vec::new();
    let mut groups: Vec<(Value<'a>, Vec<Value<'a>>)> = Vec::new();
    let mut sorted = Vec::new();
    let mut selected: Option<(Value<'a>, Value<'a>)> = None;
    let mut skipping = true;
    for (index, item) in items.iter().enumerate() {
        if kind == Each::SkipWhile && !skipping {
            out.push(item.clone());
            continue;
        }
        let value = call(index, item)?;
        match kind {
            Each::Map => out.push(value),
            Each::FlatMap => out.extend_from_slice(value.item_slice()),
            Each::Filter => if value.truthy() { out.push(item.clone()); },
            Each::Find => if value.truthy() { return Ok(item.clone()); },
            Each::Any => if value.truthy() { return Ok(Value::Bool(true)); },
            Each::All => if !value.truthy() { return Ok(Value::Bool(false)); },
            Each::TakeWhile => {
                if !value.truthy() { break; }
                out.push(item.clone());
            }
            Each::SkipWhile => {
                skipping = value.truthy();
                if !skipping { out.push(item.clone()); }
            }
            Each::UniqueBy => {
                if !keys.iter().any(|key: &Value<'a>| key.loose_eq(&value)) {
                    keys.push(value);
                    out.push(item.clone());
                }
            }
            Each::MinBy | Each::MaxBy => {
                let replace = selected.as_ref().is_none_or(|(key, _)| {
                    let order = value.compare(key);
                    if kind == Each::MinBy { order.is_lt() } else { order.is_gt() }
                });
                if replace { selected = Some((value, item.clone())); }
            }
            Each::SortBy => sorted.push((value, item.clone())),
            Each::GroupBy => {
                if let Some((_, entries)) = groups.iter_mut().find(|(key, _)| key.loose_eq(&value)) {
                    entries.push(item.clone());
                } else {
                    groups.push((value, vec![item.clone()]));
                }
            }
            Each::Partition => if value.truthy() { out.push(item.clone()); } else { no.push(item.clone()); },
            Each::FindLast | Each::Fold | Each::Reduce | Each::Zip => unreachable!("handled before item traversal"),
        }
    }
    Ok(match kind {
        Each::All => Value::Bool(true),
        Each::Any => Value::Bool(false),
        Each::Find => Value::Null,
        Each::MinBy | Each::MaxBy => selected.map_or(Value::Null, |(_, item)| item),
        Each::SortBy => {
            sorted.sort_by(|(a, _), (b, _)| a.compare(b));
            Value::list(sorted.into_iter().map(|(_, item)| item).collect())
        }
        Each::GroupBy => Value::list(groups.into_iter().map(|(key, items)| Value::record(vec![
            ("key".into(), key), ("items".into(), Value::list(items)),
        ])).collect()),
        Each::Partition => Value::record(vec![("yes".into(), Value::list(out)), ("no".into(), Value::list(no))]),
        _ => Value::list(out),
    })
}

#[cfg(test)]
mod tests {
    use super::{run, Each, Value};

    fn numbers(items: &[usize]) -> Value<'static> {
        Value::list(items.iter().copied().map(Value::from).collect())
    }

    #[test]
    fn reverse_search_and_prefix_stages_keep_original_indices_and_stop() {
        let input = numbers(&[7, 8, 9, 10]);
        let mut visited = Vec::new();
        let found = run(Each::FindLast, &input, &[], 2, |args| {
            let index = args[1].num()? as usize;
            visited.push(index);
            Ok(Value::Bool(index == 2))
        }).unwrap();
        assert_eq!(found, Value::from(9usize));
        assert_eq!(visited, [3, 2]);
        for (stage, expected) in [(Each::TakeWhile, numbers(&[7, 8])), (Each::SkipWhile, numbers(&[9, 10]))] {
            let mut visited = Vec::new();
            let result = run(stage, &input, &[], 2, |args| {
                let index = args[1].num()? as usize;
                visited.push(index);
                Ok(Value::Bool(index < 2))
            }).unwrap();
            assert_eq!(result, expected);
            assert_eq!(visited, [0, 1, 2]);
        }
    }

    #[test]
    fn keyed_stages_are_stable_and_visit_each_item_once() {
        let input = numbers(&[3, 2, 5, 4]);
        for (stage, expected) in [
            (Each::SortBy, numbers(&[2, 4, 3, 5])),
            (Each::UniqueBy, numbers(&[3, 2])),
            (Each::MinBy, Value::from(2usize)),
            (Each::MaxBy, Value::from(3usize)),
            (Each::GroupBy, Value::list(vec![
                Value::record(vec![("key".into(), Value::from(1usize)), ("items".into(), numbers(&[3, 5]))]),
                Value::record(vec![("key".into(), Value::from(0usize)), ("items".into(), numbers(&[2, 4]))]),
            ])),
        ] {
            let mut visited = Vec::new();
            let result = run(stage, &input, &[], 2, |args| {
                visited.push(args[1].num()? as usize);
                Ok(Value::Num(args[0].num()? % 2.0))
            }).unwrap();
            assert_eq!(result, expected);
            assert_eq!(visited, [0, 1, 2, 3]);
        }
    }

    #[test]
    fn reductions_seed_once_zip_stops_and_callback_errors_propagate() {
        let input = numbers(&[1, 2, 3]);
        let add = |args: &[Value<'static>]| Ok(Value::Num(args[0].num()? + args[1].num()?));
        assert_eq!(run(Each::Reduce, &input, &[], 2, add).unwrap(), Value::from(6usize));
        assert_eq!(run(Each::Fold, &input, &[Value::from(10usize)], 2, add).unwrap(), Value::from(16usize));
        assert_eq!(run(Each::Reduce, &Value::Null, &[], 2, |_| panic!("empty reduction")).unwrap(), Value::Null);
        assert_eq!(run(Each::Fold, &Value::Null, &[Value::from(10usize)], 2, |_| panic!("empty fold")).unwrap(), Value::from(10usize));
        let mut visited = 0;
        assert_eq!(run(Each::Zip, &input, &[numbers(&[10, 20])], 2, |args| {
            visited += 1;
            add(args)
        }).unwrap(), numbers(&[11, 22]));
        assert_eq!(visited, 2);
        for stage in [Each::Map, Each::Filter, Each::Find, Each::SortBy, Each::GroupBy, Each::Partition] {
            assert_eq!(run(stage, &input, &[], 1, |_| Err("callback failed".into())).unwrap_err(), "callback failed");
        }
    }
}
