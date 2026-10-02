//! Gitignore-flavoured path matching shared by `lang`, `ignore` and
//! formatter `files`/`ignore` clauses:
//! - a glob without `/` matches any single path component (`*.rs`,
//!   `node_modules`, `target`)
//! - a glob with `/` matches the whole path relative to the project root
//!   (`src/**/*.rs`, `tests/fixtures/**`), or any of its parent directories

use std::path::Path;

use globset::{Glob, GlobSet, GlobSetBuilder};

#[derive(Debug, Clone)]
pub struct PathGlobs {
    component: GlobSet,
    path: GlobSet,
    empty: bool,
}

impl Default for PathGlobs {
    fn default() -> Self {
        PathGlobs { component: GlobSet::empty(), path: GlobSet::empty(), empty: true }
    }
}

impl PathGlobs {
    pub fn new<'a>(globs: impl IntoIterator<Item = &'a str>) -> Result<PathGlobs, (String, String)> {
        let mut component = GlobSetBuilder::new();
        let mut path = GlobSetBuilder::new();
        let mut empty = true;
        for g in globs {
            let trimmed = g.trim_start_matches("./").trim_end_matches('/');
            let glob = Glob::new(trimmed).map_err(|e| (g.to_string(), e.kind().to_string()))?;
            if trimmed.contains('/') {
                path.add(glob);
            } else {
                component.add(glob);
            }
            empty = false;
        }
        let build = |b: GlobSetBuilder| b.build().map_err(|e| ("globs".to_string(), e.to_string()));
        Ok(PathGlobs { component: build(component)?, path: build(path)?, empty })
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }

    /// `rel` must be relative to the project root.
    pub fn matches(&self, rel: &Path) -> bool {
        if self.empty {
            return false;
        }
        let unix = rel.to_string_lossy().replace('\\', "/");
        if unix.split('/').any(|c| self.component.is_match(c)) {
            return true;
        }
        // A path glob also matches anything inside a matching directory.
        let mut prefix = unix.as_str();
        loop {
            if self.path.is_match(prefix) {
                return true;
            }
            match prefix.rfind('/') {
                Some(i) => prefix = &prefix[..i],
                None => return false,
            }
        }
    }
}

/// `*`-only wildcard matching for rule names (`style-*`).
pub fn wildcard(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => {
            let Some(rest) = name.strip_prefix(head) else { return false };
            if !tail.contains('*') {
                return rest.len() >= tail.len() && rest.ends_with(tail);
            }
            (0..=rest.len()).filter(|&i| rest.is_char_boundary(i)).any(|i| wildcard(tail, &rest[i..]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_and_paths() {
        let g = PathGlobs::new(["*.rs", "target", "tests/fixtures/**", "docs/a.md"]).unwrap();
        assert!(g.matches(Path::new("src/main.rs")));
        assert!(g.matches(Path::new("target/debug/x.txt")));
        assert!(g.matches(Path::new("tests/fixtures/deep/x.txt")));
        assert!(g.matches(Path::new("docs/a.md")));
        assert!(!g.matches(Path::new("docs/b.md")));
        assert!(!PathGlobs::default().matches(Path::new("x")));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("*", "anything"));
        assert!(wildcard("style-*", "style-x"));
        assert!(wildcard("*-case*", "fn-case-snake"));
        assert!(!wildcard("style-*", "styl"));
    }
}
