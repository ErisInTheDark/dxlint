//! Gitignore-flavoured path matching shared by `lang`, `ignore` and
//! formatter `files`/`ignore` clauses:
//! - a glob without `/` matches any single path component (`*.rs`,
//!   `node_modules`, `target`)
//! - a glob with `/` matches the whole path relative to the project root
//!   (`src/**/*.rs`, `tests/fixtures/**`), or any of its parent directories
//! - `!glob` excludes; the last glob that matches decides, as in `.gitignore`

use std::path::Path;

use globset::{Glob, GlobSet, GlobSetBuilder};

#[derive(Debug, Clone)]
pub struct PathGlobs {
    component: GlobSet,
    path: GlobSet,
    /// Each set's glob indices in list order, with whether it is `!`-negated;
    /// empty when nothing is negated, so plain lists stay on `is_match`.
    order: Option<(Vec<(usize, bool)>, Vec<(usize, bool)>)>,
    empty: bool,
}

impl Default for PathGlobs {
    fn default() -> Self {
        PathGlobs { component: GlobSet::empty(), path: GlobSet::empty(), order: None, empty: true }
    }
}

impl PathGlobs {
    pub fn new<'a>(globs: impl IntoIterator<Item = &'a str>) -> Result<PathGlobs, (String, String)> {
        let mut component = GlobSetBuilder::new();
        let mut path = GlobSetBuilder::new();
        let (mut component_order, mut path_order) = (Vec::new(), Vec::new());
        let mut negated_any = false;
        let mut empty = true;
        for (i, g) in globs.into_iter().enumerate() {
            let (negated, pattern) = match g.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, g),
            };
            negated_any |= negated;
            let trimmed = pattern.trim_start_matches("./").trim_end_matches('/');
            let glob = Glob::new(trimmed).map_err(|e| (g.to_string(), e.kind().to_string()))?;
            if trimmed.contains('/') {
                path.add(glob);
                path_order.push((i, negated));
            } else {
                component.add(glob);
                component_order.push((i, negated));
            }
            empty = false;
        }
        let build = |b: GlobSetBuilder| b.build().map_err(|e| ("globs".to_string(), e.to_string()));
        let order = negated_any.then_some((component_order, path_order));
        Ok(PathGlobs { component: build(component)?, path: build(path)?, order, empty })
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
        // A path glob also matches anything inside a matching directory.
        let prefixes = || std::iter::successors(Some(unix.as_str()), |p| p.rfind('/').map(|i| &p[..i]));
        let Some((component_order, path_order)) = &self.order else {
            return unix.split('/').any(|c| self.component.is_match(c)) || prefixes().any(|p| self.path.is_match(p));
        };
        let last = unix.split('/').flat_map(|c| self.component.matches(c).into_iter().map(|m| component_order[m]))
            .chain(prefixes().flat_map(|p| self.path.matches(p).into_iter().map(|m| path_order[m])))
            .max_by_key(|&(i, _)| i);
        last.is_some_and(|(_, negated)| !negated)
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
    fn the_last_matching_glob_decides_and_bang_negates() {
        let g = PathGlobs::new(["src/**", "!src/generated/**", "src/generated/keep.ts", "!*.snap"]).unwrap();
        assert!(g.matches(Path::new("src/a.ts")));
        assert!(!g.matches(Path::new("src/generated/b.ts")));
        assert!(g.matches(Path::new("src/generated/keep.ts")));
        assert!(!g.matches(Path::new("src/x.snap")));
        assert!(!g.matches(Path::new("lib/a.ts")));
        // A folder pattern covers what is inside it, in its place in the order.
        let g = PathGlobs::new(["!dist/types", "dist"]).unwrap();
        assert!(g.matches(Path::new("dist/types/a.d.ts")));
        let g = PathGlobs::new(["dist", "!dist/types"]).unwrap();
        assert!(!g.matches(Path::new("dist/types/a.d.ts")));
        assert!(g.matches(Path::new("dist/a.js")));
        assert!(!PathGlobs::new(["!x"]).unwrap().matches(Path::new("x")));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("*", "anything"));
        assert!(wildcard("style-*", "style-x"));
        assert!(wildcard("*-case*", "fn-case-snake"));
        assert!(!wildcard("style-*", "styl"));
    }
}
