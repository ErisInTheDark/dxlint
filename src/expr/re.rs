//! Regex functions for expressions (`re_test`, `re_replace`, ...). Patterns
//! use full `regex` syntax, run in linear time, and compile once per
//! pattern string for the whole process.
//!
//! Rules call these on every node they check, from every worker thread, so
//! the hot path takes no lock: each thread keeps its own pattern table with a
//! search cache per regex (the regex's shared pool would make threads queue).
//! The process-wide table only builds a pattern the first time any thread
//! needs it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use regex_automata::meta::{Cache, Regex};
use regex_automata::util::iter::Searcher;
use regex_automata::Input;

use super::value::Value;
use crate::fx_hash::FxHashMap;

fn shared() -> &'static Mutex<HashMap<String, Arc<Regex>>> {
    static SHARED: OnceLock<Mutex<HashMap<String, Arc<Regex>>>> = OnceLock::new();
    SHARED.get_or_init(Default::default)
}

/// The compiled pattern, or a readable error.
pub fn compile(pattern: &str) -> Result<Arc<Regex>, String> {
    if let Some(re) = shared().lock().ok().and_then(|c| c.get(pattern).cloned()) {
        return Ok(re);
    }
    let re = Arc::new(Regex::new(pattern).map_err(|e| format!("invalid regex `{pattern}`: {e}"))?);
    if let Ok(mut c) = shared().lock() {
        c.insert(pattern.to_string(), re.clone());
    }
    Ok(re)
}

struct Local {
    re: Arc<Regex>,
    cache: Cache,
}

thread_local! {
    static LOCAL: RefCell<FxHashMap<Box<str>, Local>> = RefCell::default();
}

/// Runs `f` with `pattern`'s regex and this thread's cache for it.
fn with<R>(pattern: &str, f: impl FnOnce(&Regex, &mut Cache) -> R) -> Result<R, String> {
    LOCAL.with(|local| {
        let mut local = local.borrow_mut();
        if let Some(l) = local.get_mut(pattern) {
            return Ok(f(&l.re, &mut l.cache));
        }
        let re = compile(pattern)?;
        let cache = re.create_cache();
        let l = local.entry(pattern.into()).or_insert(Local { re, cache });
        Ok(f(&l.re, &mut l.cache))
    })
}

pub fn test<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let hay = a[0].text();
    let found = with(&a[1].text(), |re, cache| re.search_half_with(cache, &Input::new(hay.as_bytes()).earliest(true)).is_some())?;
    Ok(Value::Bool(found))
}

pub fn find<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let text = a[0].text();
    let found = with(&a[1].text(), |re, cache| re.search_with(cache, &Input::new(text.as_bytes())))?;
    Ok(match found {
        Some(m) => Value::from(text[m.range()].to_string()),
        None => Value::Null,
    })
}

pub fn count<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let text = a[0].text();
    let n = with(&a[1].text(), |re, cache| {
        let mut it = Searcher::new(Input::new(text.as_bytes()));
        let mut n = 0usize;
        while it.advance(|input| Ok(re.search_with(cache, input))).is_some() {
            n += 1;
        }
        n
    })?;
    Ok(Value::from(n))
}

/// Replaces every match; `$1`, `${name}` and `$$` work in the replacement.
pub fn replace<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let text = a[0].text();
    let replacement = a[2].text();
    let out = with(&a[1].text(), |re, cache| {
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        let matches = Searcher::new(Input::new(text.as_bytes())).into_captures_iter(re.create_captures(), |input, caps| {
            re.search_captures_with(cache, input, caps);
            Ok(())
        });
        for caps in matches.infallible() {
            let Some(m) = caps.get_match() else { continue };
            out.push_str(&text[last..m.start()]);
            caps.interpolate_string_into(&text, &replacement, &mut out);
            last = m.end();
        }
        out.push_str(&text[last..]);
        out
    })?;
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
        assert_eq!(call(replace, &["aaa", "a*", "-"]), Value::from("-"));
        assert_eq!(call(replace, &["abc", "", "-"]), Value::from("-a-b-c-"));
    }

    #[test]
    fn find_count_and_test() {
        assert_eq!(call(find, &["ab12cd", r"\d+"]), Value::from("12"));
        assert_eq!(call(find, &["ab", r"\d"]), Value::Null);
        assert_eq!(call(count, &["a,b,c", ","]), Value::from(2usize));
        assert_eq!(call(count, &["abc", ""]), Value::from(4usize));
        assert_eq!(call(test, &["Abc", "^[A-Z]"]), Value::Bool(true));
        assert_eq!(call(test, &["abc", "^[A-Z]"]), Value::Bool(false));
        assert!(compile("(").is_err());
        assert!(test(&[Value::from("x"), Value::from("(")]).is_err());
    }
}
