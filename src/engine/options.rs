//! Option values have the shape of their defaults: the same type, records
//! only with keys the default has, and list items like the default's first
//! item (or the option's declared `item` shape).

use crate::expr::Value;

/// Whether `value` fits an option whose default is `default` (and whose
/// list items look like `item`, when declared). The error names the path
/// to the first misfit: `[1].blankLine is a num, expected a str`.
pub fn check(default: &Value, item: Option<&Value>, value: &Value) -> Result<(), String> {
    fits(default, item, value, &mut String::new())
}

fn fits(shape: &Value, item: Option<&Value>, value: &Value, path: &mut String) -> Result<(), String> {
    let at = |path: &str| if path.is_empty() { "the value".to_string() } else { path.trim_start_matches('.').to_string() };
    match (shape, value) {
        // A null default takes anything; null clears any option.
        (Value::Null, _) | (_, Value::Null) => Ok(()),
        (Value::List(items), Value::List(values)) => {
            let Some(shape) = item.or(items.first()) else { return Ok(()) };
            for (i, v) in values.iter().enumerate() {
                let len = path.len();
                path.push_str(&format!("[{i}]"));
                fits(shape, None, v, path)?;
                path.truncate(len);
            }
            Ok(())
        }
        (Value::Record(keys), Value::Record(entries)) => {
            for (k, v) in entries {
                let Some((_, shape)) = keys.iter().find(|(key, _)| key == k) else {
                    let known: Vec<&str> = keys.iter().map(|(k, _)| &**k).collect();
                    return Err(format!("{} has no key `{k}` (keys: {})", at(path), known.join(", ")));
                };
                let len = path.len();
                path.push('.');
                path.push_str(k);
                fits(shape, None, v, path)?;
                path.truncate(len);
            }
            Ok(())
        }
        (a, b) if std::mem::discriminant(a) == std::mem::discriminant(b) => Ok(()),
        (a, b) => Err(format!("{} is a {}, expected a {}", at(path), b.type_name(), a.type_name())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(entries: &[(&str, Value<'static>)]) -> Value<'static> {
        Value::Record(entries.iter().map(|(k, v)| ((*k).into(), v.clone())).collect())
    }

    #[test]
    fn values_take_the_shape_of_the_default() {
        let item = rec(&[("blankLine", Value::from("any")), ("prev", Value::from("*"))]);
        let default = Value::List(Vec::new());
        let good = Value::List(vec![rec(&[("blankLine", Value::from("always"))])]);
        assert_eq!(check(&default, Some(&item), &good), Ok(()));
        let bad = Value::List(vec![good.items()[0].clone(), rec(&[("blankLine", Value::Num(1.0))])]);
        assert_eq!(check(&default, Some(&item), &bad), Err("[1].blankLine is a num, expected a str".into()));
        let unknown = Value::List(vec![rec(&[("blank", Value::from("x"))])]);
        assert!(check(&default, Some(&item), &unknown).unwrap_err().contains("no key `blank`"));
        assert_eq!(check(&Value::Num(1.0), None, &Value::from("x")), Err("the value is a str, expected a num".into()));
    }
}
