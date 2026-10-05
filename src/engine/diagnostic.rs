use serde::Serialize;

use crate::config::ast::Severity;

/// Replace `start..end` (byte offsets) with `text`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub start: usize,
    pub end: usize,
    /// All edits apply together or not at all.
    pub fix: Option<Vec<Edit>>,
}

/// Rule name for problems the engine itself reports.
pub const ENGINE_RULE: &str = "zlint";

impl Diagnostic {
    fn key(&self) -> (usize, usize, &str, &str) {
        (self.start, self.end, &self.rule, &self.message)
    }

    /// Sorts by place, and keeps one report per problem: a rule reaching a
    /// node twice (through several targets) reports once. A different
    /// message or fix at one place is a different problem.
    pub fn dedup(diags: &mut Vec<Diagnostic>) {
        diags.sort_by(|a, b| a.key().cmp(&b.key()));
        let mut kept: Vec<Diagnostic> = Vec::with_capacity(diags.len());
        for d in diags.drain(..) {
            // Fixes are not ordered: compare with every report at this key.
            if !kept.iter().rev().take_while(|k| k.key() == d.key()).any(|k| k.fix == d.fix) {
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
        let fix = fix.map(|text| vec![Edit { start, end: start + 1, text: text.into() }]);
        Diagnostic { rule: "r".into(), severity: Severity::Warn, message: message.into(), start, end: start + 1, fix }
    }

    #[test]
    fn only_identical_reports_merge() {
        let mut diags = vec![at(3, "a", Some("x")), at(0, "a", None), at(3, "a", Some("y")), at(3, "a", Some("x")), at(3, "b", Some("x"))];
        Diagnostic::dedup(&mut diags);
        let kept: Vec<_> = diags.iter().map(|d| (d.start, d.message.as_str(), d.fix.as_ref().map(|f| f[0].text.as_str()))).collect();
        // Same place and message but another fix is another problem (one
        // per line of a JSX text, say); the same fix again is not.
        assert_eq!(kept, vec![(0, "a", None), (3, "a", Some("x")), (3, "a", Some("y")), (3, "b", Some("x"))]);
    }
}
