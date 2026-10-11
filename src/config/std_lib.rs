//! The built-in library: `std/` and `rules/` of the library directory
//! ([`root`]), read from disk (nothing is compiled in). Languages and their
//! rule definitions are imported as `import .ts` (stored as `std/ts`, with
//! its parts as `std/ts/problems` ...); rulesets that turn rules on are
//! `import rules:recommended`. Adding a module is adding its file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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

/// The library directory: `ZLINT_LIBRARY=<dir>`, else the nearest directory
/// holding both `std/` and `rules/`, from the executable's own directory up
/// (a checkout, or an installed package). `None` when there is none. Root,
/// inventory and successfully read texts are cached for the process.
pub fn root() -> Option<&'static Path> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        if let Some(dir) = std::env::var_os("ZLINT_LIBRARY") {
            return Some(PathBuf::from(dir));
        }
        let exe = std::env::current_exe().ok()?;
        let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
        exe.ancestors().skip(1).find(|dir| dir.join("std").is_dir() && dir.join("rules").is_dir()).map(Path::to_path_buf)
    })
    .as_deref()
}

fn from_disk(dir: &str, name: &str) -> Option<&'static str> {
    static TEXTS: OnceLock<std::sync::Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let root = root()?;
    let key = format!("{dir}/{name}");
    let mut texts = TEXTS.get_or_init(Default::default).lock().expect("library lock");
    if let Some(&text) = texts.get(&key) {
        return Some(text);
    }
    let text: &'static str = std::fs::read_to_string(root.join(format!("{key}.zlint"))).ok()?.leak();
    texts.insert(key, text);
    Some(text)
}

fn disk_names(dir: &str) -> Vec<&'static str> {
    fn scan(path: &Path, prefix: &str, names: &mut Vec<&'static str>) {
        let Ok(entries) = std::fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
            let name = format!("{prefix}{name}");
            if path.is_dir() {
                scan(&path, &format!("{name}/"), names);
            } else if let Some(name) = name.strip_suffix(".zlint").filter(|_| path.is_file()) {
                names.push(name.to_owned().leak());
            }
        }
    }
    let mut names = Vec::new();
    if let Some(root) = root() {
        scan(&root.join(dir), "", &mut names);
    }
    names.sort_unstable();
    names
}

pub fn get(name: &str) -> Option<&'static str> {
    from_disk("std", name.strip_suffix(".zlint").unwrap_or(name))
}

pub fn names() -> impl Iterator<Item = &'static str> {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| disk_names("std")).iter().copied()
}

pub fn ruleset(name: &str) -> Option<&'static str> {
    from_disk("rules", name)
}

pub fn rulesets() -> impl Iterator<Item = &'static str> {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| disk_names("rules")).iter().copied()
}

/// The language whose import defines rule `name` (`std/rust/format` counts
/// as `rust`), for "add `import .rs`" hints. Found by scanning the selected
/// library text for `rule <name>`, once.
pub fn defining_language(name: &str) -> Option<&'static str> {
    static INDEX: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut index = HashMap::new();
        for module in names() {
            let Some(text) = get(module) else { continue };
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
