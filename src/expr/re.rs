//! Regex functions for expressions (`re_test`, `re_replace`, ...). Patterns
//! use full `regex` syntax, run in linear time, and compile once per
//! pattern string for the whole process.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use regex_automata::meta::Regex;

use super::value::Value;

fn cache() -> &'static Mutex<HashMap<String, Arc<Regex>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Regex>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The compiled pattern, or a readable error.
pub fn compile(pattern: &str) -> Result<Arc<Regex>, String> {
    if let Some(re) = cache().lock().ok().and_then(|c| c.get(pattern).cloned()) {
        return Ok(re);
    }
    let re = Arc::new(Regex::new(pattern).map_err(|e| format!("invalid regex `{pattern}`: {e}"))?);
    if let Ok(mut c) = cache().lock() {
        c.insert(pattern.to_string(), re.clone());
    }
    Ok(re)
}

pub fn test<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    Ok(Value::Bool(compile(&a[1].text())?.is_match(a[0].text().as_bytes())))
}

pub fn find<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let text = a[0].text();
    Ok(match compile(&a[1].text())?.find(text.as_bytes()) {
        Some(m) => Value::from(text[m.range()].to_string()),
        None => Value::Null,
    })
}

pub fn count<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let text = a[0].text();
    Ok(Value::from(compile(&a[1].text())?.find_iter(text.as_bytes()).count()))
}

/// Replaces every match; `$1`, `${name}` and `$$` work in the replacement.
pub fn replace<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let text = a[0].text();
    let re = compile(&a[1].text())?;
    let replacement = a[2].text();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for caps in re.captures_iter(text.as_bytes()) {
        let Some(m) = caps.get_match() else { continue };
        out.push_str(&text[last..m.start()]);
        caps.interpolate_string_into(&text, &replacement, &mut out);
        last = m.end();
    }
    out.push_str(&text[last..]);
    Ok(Value::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(f: for<'s> fn(&[Value<'s>]) -> Result<Value<'s>, String>, args: &[&str]) -> Value<'static> {
        let args: Vec<Value> = args.iter().map(|a| Value::from(a.to_string())).collect();
        f(&args).unwrap().into_owned()
    }

    #[test]
    fn replace_interpolates_groups() {
        assert_eq!(call(replace, &["a=1, b=2", r"(\w)=(\d)", "$2:$1"]), Value::from("1:a, 2:b"));
        assert_eq!(call(replace, &["x\n}", r"([^,\s])(\s*\n\s*\})$", "$1,$2"]), Value::from("x,\n}"));
    }

    #[test]
    fn find_count_and_test() {
        assert_eq!(call(find, &["ab12cd", r"\d+"]), Value::from("12"));
        assert_eq!(call(find, &["ab", r"\d"]), Value::Null);
        assert_eq!(call(count, &["a,b,c", ","]), Value::from(2usize));
        assert_eq!(call(test, &["Abc", "^[A-Z]"]), Value::Bool(true));
        assert!(compile("(").is_err());
    }
}
