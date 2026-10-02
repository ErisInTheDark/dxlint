//! `zlint-ignore` directives, written in any comment syntax:
//!
//! ```text
//! // zlint-ignore                  -> next line (comment alone on its line)
//! x = 1 // zlint-ignore rule-a     -> this line, only rule-a
//! # zlint-ignore-file style-*      -> whole file, rules matching style-*
//! ```

use std::collections::HashMap;

use memchr::memmem;

use super::diagnostic::Diagnostic;
use crate::line_index::LineIndex;
use crate::path_globs::wildcard;

const DIRECTIVE: &str = "zlint-ignore";

/// Rule patterns; empty means "every rule".
type Rules = Vec<String>;

#[derive(Default)]
pub struct Suppressions {
    file: Option<Rules>,
    lines: HashMap<usize, Rules>,
}

/// Combines two directives for the same place; "every rule" absorbs lists.
fn merged(existing: Option<Rules>, rules: Rules) -> Rules {
    match existing {
        Some(e) if e.is_empty() => e,
        _ if rules.is_empty() => Vec::new(),
        Some(mut e) => {
            e.extend(rules);
            e
        }
        None => rules,
    }
}

fn covers(rules: &Rules, rule: &str) -> bool {
    rules.is_empty() || rules.iter().any(|r| wildcard(r, rule))
}

impl Suppressions {
    pub fn parse(text: &str, lines: &LineIndex) -> Suppressions {
        let mut out = Suppressions::default();
        for at in memmem::find_iter(text.as_bytes(), DIRECTIVE) {
            let line = lines.line_of(at);
            let line_start = lines.line_start(line);
            let line_end = lines.line_end(text, line);
            let before = &text[line_start..at];
            // Only directives at the start of a comment count: everything
            // before them on the line is comment punctuation (or code
            // followed by a comment opener).
            if before.chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                continue;
            }
            let mut rest = &text[at + DIRECTIVE.len()..line_end];
            let whole_file = rest.starts_with("-file");
            if whole_file {
                rest = &rest[5..];
            }
            if rest.chars().next().is_some_and(|c| !c.is_whitespace() && c != ',') {
                continue;
            }
            let rules: Rules = rest
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|t| !t.is_empty())
                .take_while(|t| t.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '*' | '.')))
                .map(str::to_string)
                .collect();
            if whole_file {
                out.file = Some(merged(out.file.take(), rules));
                continue;
            }
            let alone = before.chars().all(|c| !c.is_alphanumeric());
            let target = if alone { line + 1 } else { line };
            let existing = out.lines.remove(&target);
            out.lines.insert(target, merged(existing, rules));
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.file.is_none() && self.lines.is_empty()
    }

    pub fn allows(&self, d: &Diagnostic, lines: &LineIndex) -> bool {
        if self.file.as_ref().is_some_and(|r| covers(r, &d.rule)) {
            return false;
        }
        !self.lines.get(&lines.line_of(d.start)).is_some_and(|r| covers(r, &d.rule))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ast::Severity;

    fn diag(rule: &str, text: &str, needle: &str) -> Diagnostic {
        let start = text.find(needle).unwrap();
        Diagnostic { rule: rule.into(), severity: Severity::Warn, message: String::new(), start, end: start + 1, fix: None }
    }

    #[test]
    fn line_and_next_line_directives() {
        let text = "// zlint-ignore\nA\nB // zlint-ignore rule-b\nC\n";
        let lines = LineIndex::new(text);
        let s = Suppressions::parse(text, &lines);
        assert!(!s.allows(&diag("any", text, "A"), &lines));
        assert!(!s.allows(&diag("rule-b", text, "B"), &lines));
        assert!(s.allows(&diag("rule-x", text, "B"), &lines));
        assert!(s.allows(&diag("any", text, "C"), &lines));
    }

    #[test]
    fn file_directive_with_globs_and_prose_mentions() {
        let text = "/* zlint-ignore-file style-* */\nsee the zlint-ignore docs\nX\n";
        let lines = LineIndex::new(text);
        let s = Suppressions::parse(text, &lines);
        assert!(!s.allows(&diag("style-a", text, "X"), &lines));
        assert!(s.allows(&diag("other", text, "X"), &lines));
    }
}
