use serde::Serialize;

use crate::config::ast::Severity;

/// Replace `start..end` (byte offsets) with `text`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// A rewrite reserves its declared range even when its edits are smaller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fix {
    pub range: std::ops::Range<usize>,
    /// Sorted, non-overlapping.
    pub edits: Vec<Edit>,
    /// Names left for the user to choose (`placeholder(...)`), as ranges of
    /// the text with only this fix applied.
    pub placeholders: Vec<std::ops::Range<usize>>,
}

/// A fix offered as a choice (`suggest "title" ...`), never applied by
/// `--fix`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    pub title: String,
    pub fix: Fix,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    /// Source-order message leaf; absent for engine/evaluation failures.
    pub message_index: Option<usize>,
    pub start: usize,
    pub end: usize,
    /// All edits apply together or not at all.
    pub fix: Option<Fix>,
    pub suggestions: Vec<Suggestion>,
}

/// Rule name for problems the engine itself reports.
pub const ENGINE_RULE: &str = "dxlint";

impl Diagnostic {
    /// A report without fixes.
    pub fn new(rule: String, severity: Severity, message: String, message_index: Option<usize>, start: usize, end: usize) -> Self {
        Diagnostic { rule, severity, message, message_index, start, end, fix: None, suggestions: Vec::new() }
    }

    fn key(&self) -> (usize, usize, &str, &str, Option<usize>) {
        (self.start, self.end, &self.rule, &self.message, self.message_index)
    }

    /// Sorts by place, and keeps one report per problem: a rule reaching a
    /// node twice (through several targets) reports once. A different
    /// message or fix at one place is a different problem.
    pub fn dedup(diags: &mut Vec<Diagnostic>) {
        diags.sort_by(|a, b| a.key().cmp(&b.key()));
        let mut kept: Vec<Diagnostic> = Vec::with_capacity(diags.len());
        for d in diags.drain(..) {
            // Fixes are not ordered: compare with every report at this key.
            if !kept.iter().rev().take_while(|k| k.key() == d.key()).any(|k| k.fix == d.fix && k.suggestions == d.suggestions) {
                kept.push(d);
            }
        }
        *diags = kept;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: usize, message: &str, fix: Option<&str>) -> Diagnostic {
        let fix = fix.map(|text| Fix { range: start..start + 1, edits: vec![Edit { start, end: start + 1, text: text.into() }], placeholders: Vec::new() });
        Diagnostic { fix, ..Diagnostic::new("r".into(), Severity::Warn, message.into(), Some(0), start, start + 1) }
    }

    #[test]
    fn only_identical_reports_merge() {
        let mut diags = vec![at(3, "a", Some("x")), at(0, "a", None), at(3, "a", Some("y")), at(3, "a", Some("x")), at(3, "b", Some("x"))];
        Diagnostic::dedup(&mut diags);
        let kept: Vec<_> = diags.iter().map(|d| (d.start, d.message.as_str(), d.fix.as_ref().map(|f| f.edits[0].text.as_str()))).collect();
        // Same place and message but another fix is another problem (one
        // per line of a JSX text, say); the same fix again is not.
        assert_eq!(kept, vec![(0, "a", None), (3, "a", Some("x")), (3, "a", Some("y")), (3, "b", Some("x"))]);
    }
}
