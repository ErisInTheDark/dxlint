//! How editors colour a file: its `highlight`s' nodes, each with the token
//! type it names. Language-neutral: a highlight is written like a rule
//! (`in <layer> highlight name <token type>`) and runs only when asked.

use std::path::Path;

use super::Linter;
use crate::config::ast::{Name, RuleKind};
use crate::config::error::ConfigError;

/// The token types a highlight may name: the editor's standard semantic
/// token types, so a theme colours them without any setup.
pub const TOKEN_TYPES: [&str; 23] = [
    "namespace", "type", "class", "enum", "interface", "struct", "typeParameter", "parameter", "variable", "property",
    "enumMember", "event", "function", "method", "macro", "keyword", "modifier", "comment", "string", "number", "regexp",
    "operator", "decorator",
];

/// A stretch of a file to colour, by byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Highlight {
    pub start: usize,
    pub end: usize,
    pub token_type: String,
}

/// Reports a token type no editor knows.
pub(super) fn check_token_type(token_type: &Name, errors: &mut Vec<ConfigError>) {
    if !TOKEN_TYPES.contains(&token_type.text.as_str()) {
        errors.push(ConfigError::new(token_type.span, format!("`{}` is not a token type", token_type.text))
            .suggest(&token_type.text, TOKEN_TYPES)
            .note(format!("token types: {}", TOKEN_TYPES.join(", "))));
    }
}

impl Linter {
    /// Whether some `highlight` is defined: nothing to ask a file otherwise.
    pub fn highlights_anything(&self) -> bool {
        self.rules.iter().any(|rule| matches!(rule.kind, RuleKind::Highlight(_)))
    }

    /// What `text` of `path` looks like to its highlights, in text order;
    /// where two cover the same bytes, the inner (later starting, earlier
    /// ending) one wins.
    pub fn highlights(&self, path: &Path, text: &str) -> Vec<Highlight> {
        if !self.highlights_anything() { return Vec::new() }
        let only: Vec<bool> = self.rules.iter().map(|rule| matches!(rule.kind, RuleKind::Highlight(_))).collect();
        let mut found: Vec<Highlight> = self.run().lint_only(path, text, &only).into_iter()
            .filter(|d| d.start < d.end)
            .map(|d| Highlight { start: d.start, end: d.end, token_type: d.message })
            .collect();
        found.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
        flatten(found)
    }
}

/// Non-overlapping highlights: an inner one splits the one around it.
fn flatten(nested: Vec<Highlight>) -> Vec<Highlight> {
    let mut flat: Vec<Highlight> = Vec::new();
    // Outer highlights still open, innermost last.
    let mut open: Vec<Highlight> = Vec::new();
    let mut at = 0;
    let emit = |flat: &mut Vec<Highlight>, open: &[Highlight], from: usize, to: usize| {
        if let Some(innermost) = open.last().filter(|_| from < to) {
            flat.push(Highlight { start: from, end: to, token_type: innermost.token_type.clone() });
        }
    };
    for next in nested {
        // Close what ends before `next` starts.
        while let Some(end) = open.last().map(|h| h.end).filter(|&end| end <= next.start) {
            emit(&mut flat, &open, at, end);
            at = at.max(end);
            open.pop();
        }
        emit(&mut flat, &open, at, next.start);
        at = at.max(next.start);
        open.push(next);
    }
    while let Some(end) = open.last().map(|h| h.end) {
        emit(&mut flat, &open, at, end);
        at = at.max(end);
        open.pop();
    }
    flat
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(start: usize, end: usize, token_type: &str) -> Highlight {
        Highlight { start, end, token_type: token_type.into() }
    }

    #[test]
    fn inner_highlights_split_the_ones_around_them() {
        // `state quality = 1` as a property with `state` a keyword inside it.
        assert_eq!(flatten(vec![h(0, 17, "property"), h(0, 5, "keyword"), h(16, 17, "number")]), vec![
            h(0, 5, "keyword"), h(5, 16, "property"), h(16, 17, "number"),
        ]);
        // Side by side, and one ending where the next starts.
        assert_eq!(flatten(vec![h(0, 2, "keyword"), h(2, 4, "string"), h(6, 8, "number")]), vec![
            h(0, 2, "keyword"), h(2, 4, "string"), h(6, 8, "number"),
        ]);
    }
}
