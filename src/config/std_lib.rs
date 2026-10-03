//! The built-in library, embedded in the binary. Languages and their rule
//! definitions are imported as `import .ts` (stored as `std/ts`, with its
//! parts as `std/ts/problems` ...); rulesets that turn rules on are
//! `import rules:recommended`.

use std::collections::HashMap;
use std::sync::OnceLock;

static FILES: &[(&str, &str)] = &[
    ("core", include_str!("../../std/core.zlint")),
    ("whitespace", include_str!("../../std/whitespace.zlint")),
    ("rust", include_str!("../../std/rust.zlint")),
    ("rust/format", include_str!("../../std/rust/format.zlint")),
    ("js", include_str!("../../std/js.zlint")),
    ("js/format", include_str!("../../std/js/format.zlint")),
    ("js/flow", include_str!("../../std/js/flow.zlint")),
    ("js/problems", include_str!("../../std/js/problems.zlint")),
    ("js/license", include_str!("../../std/js/license.zlint")),
    ("ts", include_str!("../../std/ts.zlint")),
    ("ts/helpers", include_str!("../../std/ts/helpers.zlint")),
    ("ts/scopes", include_str!("../../std/ts/scopes.zlint")),
    ("ts/problems", include_str!("../../std/ts/problems.zlint")),
    ("ts/style", include_str!("../../std/ts/style.zlint")),
    ("ts/typed", include_str!("../../std/ts/typed.zlint")),
    ("ts/layout", include_str!("../../std/ts/layout.zlint")),
    ("ts/structure", include_str!("../../std/ts/structure.zlint")),
    ("ts/scoping", include_str!("../../std/ts/scoping.zlint")),
    ("ts/suggestions", include_str!("../../std/ts/suggestions.zlint")),
    ("ts/restrictions", include_str!("../../std/ts/restrictions.zlint")),
    ("python", include_str!("../../std/python.zlint")),
    ("c", include_str!("../../std/c.zlint")),
    ("zlint", include_str!("../../std/zlint.zlint")),
    ("markdown", include_str!("../../std/markdown.zlint")),
    ("toml", include_str!("../../std/toml.zlint")),
];

static RULESETS: &[(&str, &str)] = &[
    ("correctness", include_str!("../../rules/correctness.zlint")),
    ("suspicious", include_str!("../../rules/suspicious.zlint")),
    ("conventions", include_str!("../../rules/conventions.zlint")),
    ("format", include_str!("../../rules/format.zlint")),
    ("types", include_str!("../../rules/types.zlint")),
    ("recommended", include_str!("../../rules/recommended.zlint")),
];

/// Short import names (file extensions) of the languages.
const ALIASES: &[(&str, &str)] = &[("rs", "rust"), ("py", "python"), ("md", "markdown")];

/// `rs` -> `rust`: what `import .rs` loads.
pub fn long_name(name: &str) -> String {
    let (head, rest) = name.split_once('/').map_or((name, None), |(h, r)| (h, Some(r)));
    let head = ALIASES.iter().find(|(short, _)| *short == head).map_or(head, |(_, long)| long);
    match rest {
        Some(rest) => format!("{head}/{rest}"),
        None => head.to_string(),
    }
}

/// `rust` -> `rs`: how a module is written in an import.
pub fn short_name(name: &str) -> &str {
    let name = name.strip_suffix(".zlint").unwrap_or(name);
    ALIASES.iter().find(|(_, long)| *long == name).map_or(name, |(short, _)| short)
}

pub fn get(name: &str) -> Option<&'static str> {
    let name = name.strip_suffix(".zlint").unwrap_or(name);
    FILES.iter().find(|(n, _)| *n == name).map(|(_, text)| *text)
}

pub fn names() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(n, _)| *n)
}

pub fn ruleset(name: &str) -> Option<&'static str> {
    RULESETS.iter().find(|(n, _)| *n == name).map(|(_, text)| *text)
}

pub fn rulesets() -> impl Iterator<Item = &'static str> {
    RULESETS.iter().map(|(n, _)| *n)
}

/// The language whose import defines rule `name` (`std/rust/format` counts
/// as `rust`), for "add `import .rs`" hints. Found by scanning the embedded
/// text for `rule <name>`, once.
pub fn defining_language(name: &str) -> Option<&'static str> {
    static INDEX: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut index = HashMap::new();
        for (module, text) in FILES {
            let lang = module.split('/').next().unwrap_or(module);
            for line in text.lines() {
                let mut words = line.split_whitespace();
                while let Some(w) = words.next() {
                    if w == "rule" {
                        if let Some(rule) = words.next().filter(|r| r.starts_with(|c: char| c.is_ascii_alphabetic())) {
                            index.entry(rule.to_string()).or_insert(lang);
                        }
                    }
                }
            }
        }
        index
    });
    index.get(name).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_and_long_names_round_trip() {
        assert_eq!(long_name("rs"), "rust");
        assert_eq!(long_name("ts/problems"), "ts/problems");
        assert_eq!(short_name("rust"), "rs");
        assert_eq!(short_name("ts"), "ts");
    }

    #[test]
    fn every_module_and_ruleset_exists() {
        assert!(names().all(|n| get(n).is_some()));
        assert!(rulesets().all(|n| ruleset(n).is_some()));
    }
}
