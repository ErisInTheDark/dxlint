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

pub(crate) use dx_ir::re::compile;

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
    let pattern = a[1].text();
    if super::lookaround::has_lookaround(&pattern) {
        return Ok(Value::Bool(lookaround(&pattern)?.is_match(&hay)?));
    }
    let found = with(&pattern, |re, cache| re.search_half_with(cache, &Input::new(hay.as_bytes()).earliest(true)).is_some())?;
    Ok(Value::Bool(found))
}

/// Whether `pattern` compiles, lookaround groups included (`re_test` only).
pub fn valid(pattern: &str) -> bool {
    if super::lookaround::has_lookaround(pattern) { lookaround(pattern).is_ok() } else { compile(pattern).is_ok() }
}

fn lookaround(pattern: &str) -> Result<Arc<super::lookaround::Lookaround>, String> {
    static SHARED: OnceLock<Mutex<HashMap<String, Arc<super::lookaround::Lookaround>>>> = OnceLock::new();
    let shared = SHARED.get_or_init(Default::default);
    if let Some(found) = shared.lock().ok().and_then(|cache| cache.get(pattern).cloned()) { return Ok(found); }
    let compiled = Arc::new(super::lookaround::Lookaround::compile(pattern)?);
    if let Ok(mut cache) = shared.lock() {
        if cache.len() >= 256 { cache.clear(); }
        cache.insert(pattern.to_owned(), compiled.clone());
    }
    Ok(compiled)
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

/// Every match as `{text, offset, end_offset, groups}`: byte offsets in the
/// text, `groups` each numbered group's text (null when it took no part).
pub fn matches<'s>(a: &[Value<'s>]) -> Result<Value<'s>, String> {
    let text = a[0].text();
    let found = with(&a[1].text(), |re, cache| {
        let mut found = Vec::new();
        let matches = Searcher::new(Input::new(text.as_bytes())).into_captures_iter(re.create_captures(), |input, caps| {
            re.search_captures_with(cache, input, caps);
            Ok(())
        });
        for caps in matches.infallible() {
            let Some(m) = caps.get_match() else { continue };
            let groups = (1..caps.group_len()).map(|group| caps.get_group(group)
                .map_or(Value::Null, |span| Value::from(text[span.range()].to_string()))).collect();
            found.push(Value::record(vec![
                ("text".into(), Value::from(text[m.range()].to_string())),
                ("offset".into(), Value::from(m.start())),
                ("end_offset".into(), Value::from(m.end())),
                ("groups".into(), Value::list(groups)),
            ]));
        }
        found
    })?;
    Ok(Value::list(found))
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
    fn matches_list_every_match_with_groups_and_byte_offsets() {
        let found = call(matches, &["é1 x22", r"(\d)(\d)?"]);
        let Value::List(found) = found else { panic!("a list") };
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].key("offset"), Value::from(2usize));
        assert_eq!(found[0].key("groups"), Value::list(vec![Value::from("1"), Value::Null]));
        assert_eq!(found[1].key("text"), Value::from("22"));
        assert_eq!(found[1].key("end_offset"), Value::from(7usize));
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
