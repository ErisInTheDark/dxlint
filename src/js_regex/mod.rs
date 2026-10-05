//! Stateless JavaScript regex matching shared by selectors and rule options.

use regex_automata::meta;
use std::sync::{Arc, Mutex, OnceLock};

mod translate;
mod vm;

pub(crate) struct Regex {
    engine: Engine,
    legacy_case: bool,
}

enum Engine {
    Fast(meta::Regex),
    Boundary(vm::Program),
}

impl Regex {
    pub(crate) fn compile(pattern: &str, flags: &str) -> Result<Arc<Self>, String> {
        if pattern.len() > 65536 { return Err("JavaScript regex exceeds 64 KiB".into()); }
        if flags.len() > 4 { return Err("unsupported or repeated JavaScript regex flags".into()); }
        type Cache = crate::fx_hash::FxHashMap<(String, String), Arc<Regex>>;
        static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
        let cache = CACHE.get_or_init(Mutex::default);
        let key = (pattern.to_owned(), flags.to_owned());
        if let Some(regex) = cache.lock().map_err(|_| "JavaScript regex cache poisoned")?.get(&key) { return Ok(regex.clone()); }
        let mut seen = String::new();
        for flag in flags.chars() {
            if !matches!(flag, 'i' | 'm' | 's' | 'u') || seen.contains(flag) {
                return Err(format!("unsupported or repeated JavaScript regex flag: {flag}"));
            }
            seen.push(flag);
        }
        let legacy_case = flags.contains('i') && !flags.contains('u');
        if legacy_case && pattern.chars().any(legacy_gap) { return Err("unsupported legacy JavaScript regex case-fold group; use u".into()); }
        let translated = translate::pattern(pattern, flags, true)?;
        let engine = if translated.multiline || !translated.assertions.is_empty() {
            Engine::Boundary(vm::Program::compile(translated, flags)?)
        } else {
            Engine::Fast(meta::Regex::builder().syntax(syntax(flags)).build(&translated.source).map_err(|e| format!("invalid JavaScript regex: {e}"))?)
        };
        let regex = Arc::new(Self { engine, legacy_case });
        let mut cache = cache.lock().map_err(|_| "JavaScript regex cache poisoned")?;
        if cache.len() >= 256 { cache.clear(); }
        cache.insert(key, regex.clone());
        Ok(regex)
    }

    pub(crate) fn matches(&self, text: &str) -> Result<bool, String> {
        self.matches_with_budget(text, &mut vm::Budget::new(10_000_000))
    }

    fn matches_with_budget(&self, text: &str, budget: &mut vm::Budget) -> Result<bool, String> {
        if self.legacy_case && text.chars().any(legacy_gap) { return Err("unsupported legacy JavaScript regex case-fold group; use u".into()); }
        match &self.engine {
            Engine::Fast(regex) => Ok(regex.is_match(text)),
            Engine::Boundary(program) => program.matches(text, 0, false, budget),
        }
    }
}

fn syntax(flags: &str) -> regex_automata::util::syntax::Config {
    regex_automata::util::syntax::Config::new().case_insensitive(flags.contains('i')).nest_limit(128)
}

fn legacy_gap(c: char) -> bool {
    if c.is_ascii() { return false; }
    fn canonical(c: char) -> char {
        if c as u32 > 0xffff { return c; }
        let mut upper = c.to_uppercase();
        let first = upper.next().unwrap_or(c);
        if upper.next().is_some() || !c.is_ascii() && first.is_ascii() { c } else { first }
    }
    let mut folded = regex_syntax::hir::ClassUnicode::new([regex_syntax::hir::ClassUnicodeRange::new(c, c)]);
    folded.case_fold_simple();
    folded.iter().any(|range| (range.start() as u32..=range.end() as u32).filter_map(char::from_u32).any(|other| canonical(c) != canonical(other)))
}

pub(crate) fn test(text: &str, pattern: &str, flags: &str) -> Result<bool, String> {
    Regex::compile(pattern, flags)?.matches(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_anchors_keep_all_javascript_line_boundaries() {
        for separator in ["\n", "\r", "\u{2028}", "\u{2029}", "\r\n"] {
            assert!(test(&format!("before{separator}word{separator}after"), "^word$", "m").unwrap());
        }
        assert!(test("\r\n", "^\\n", "m").unwrap());
        assert!(test("a\rword", "\\r^word", "m").unwrap());
        assert!(!test("end\n", "end$", "").unwrap());
    }

    #[test]
    fn ordinary_unicode_folding_is_available_without_unicode_mode() {
        assert!(test("\u{c9}", "\u{e9}", "i").unwrap());
        assert!(test("\u{3c2}", "\u{3c3}", "i").unwrap());
        assert!(test("\u{414}", "\u{434}", "i").unwrap());
        assert!(test("\u{212a}", "k", "iu").unwrap());
        assert!(test("\u{212a}", "k", "i").is_err());
    }

    #[test]
    fn flat_assertions_test_the_current_input_offset_and_preserve_alternatives() {
        for value in ["foo", "foobar", "bar", "barista", ""] {
            assert!(!test(value, "^(?!foo)(?!bar).+$", "").unwrap());
        }
        assert!(test("other", "^(?!foo)(?!bar).+$", "").unwrap());
        assert!(!test("foo/bar", "foo/(?!bar)", "").unwrap());
        assert!(test("foo/baz", "foo/(?!bar)", "").unwrap());
        assert!(!test("@app/api/enums", "@app/(?!(api/enums$)).*", "").unwrap());
        assert!(test("@app/api/enums/foo", "@app/(?!(api/enums$)).*", "").unwrap());
        assert!(test("ab", "(?:a(?!b)|ab)", "").unwrap());
        assert!(test("ab", "a(?=b)b", "").unwrap());
        assert!(!test("ac", "a(?=b)b", "").unwrap());
        assert!(test("", "(?!)|(?=)", "").unwrap());
    }

    #[test]
    fn unsupported_and_malformed_patterns_remain_errors() {
        for pattern in ["(?=(?!a))", "(?<=a)b", "(a)\\1", "(?<named>a)", "(?!a", r"\cA"] {
            assert!(Regex::compile(pattern, "").is_err(), "{pattern}");
        }
    }

    #[test]
    fn javascript_character_classes_and_line_endings() {
        assert!(!test("\u{e9}", r"\w", "").unwrap());
        assert!(!test("\u{661}", r"\d", "").unwrap());
        assert!(test("\u{feff}", r"^\s$", "").unwrap());
        assert!(!test("\u{85}", r"^\s$", "").unwrap());
        assert!(!test("\r", r"^.$", "").unwrap());
        assert!(!test("\u{2028}", r"^.$", "").unwrap());
        assert!(!test("end\r\n", "end$", "").unwrap());
    }

    #[test]
    fn assertion_work_exhaustion_is_not_a_silent_miss() {
        let regex = Regex::compile("^(?!blocked).+$", "").unwrap();
        assert!(regex.matches_with_budget("allowed", &mut vm::Budget::new(0)).is_err());
        assert!(regex.matches_with_budget("allowed", &mut vm::Budget::new(1000)).unwrap());
    }

    #[test]
    fn quantified_assertions_follow_legacy_and_unicode_syntax() {
        assert!(test("", "(?=a){0}", "").unwrap());
        assert!(Regex::compile("(?=a)?", "u").is_err());
        assert!(Regex::compile("(?=a**){0}", "").is_err());
    }
}
