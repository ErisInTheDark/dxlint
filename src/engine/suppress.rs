//! `dxlint-ignore` directives, written in any comment syntax:
//!
//! ```text
//! // dxlint-ignore                  -> next line (comment alone on its line)
//! x = 1 // dxlint-ignore rule-a     -> this line, only rule-a
//! # dxlint-ignore-file style-*      -> whole file, rules matching style-*
//! /* dxlint-ignore-start rule-a */  -> from here...
//! /* dxlint-ignore-end rule-a */    -> ...to here (or the end of the file)
//! ```
//!
//! A rule name also matches every `family:variant` formatter of that family:
//! `dxlint-ignore semi` silences `semi:never` too.

use std::collections::HashMap;

use memchr::memmem;

use super::diagnostic::Diagnostic;
use crate::line_index::LineIndex;
use crate::path_globs::wildcard;

const DIRECTIVE: &str = "dxlint-ignore";

/// Rule patterns; empty means "every rule".
type Rules = Vec<String>;

#[derive(Default)]
pub struct Suppressions {
    /// Zero-based line ranges `[from, to)` where rules are off.
    ranges: Vec<(usize, usize, Rules)>,
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
    let family = rule.split(':').next().unwrap_or(rule);
    rules.is_empty() || rules.iter().any(|r| wildcard(r, rule) || wildcard(r, family))
}

impl Suppressions {
    pub fn parse(text: &str, lines: &LineIndex) -> Suppressions {
        let mut out = Suppressions::default();
        // Open `-start` ranges: (first line, rules).
        let mut open: Vec<(usize, Rules)> = Vec::new();
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
            let rest = &text[at + DIRECTIVE.len()..line_end];
            let kind = ["-file", "-start", "-end"].into_iter().find(|k| rest.starts_with(k)).unwrap_or("");
            let rest = &rest[kind.len()..];
            if rest.chars().next().is_some_and(|c| !c.is_whitespace() && c != ',' && c != '*') {
                continue;
            }
            let rules: Rules = rest
                .split("*/")
                .next()
                .unwrap_or(rest)
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|t| !t.is_empty())
                .take_while(|t| t.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '*' | '.' | ':')))
                .map(str::to_string)
                .collect();
            match kind {
                "-file" => out.ranges.push((0, usize::MAX, rules)),
                "-start" => open.push((line, rules)),
                "-end" => {
                    // A bare end closes every open range; a named one closes
                    // the ranges for exactly those rules.
                    let (closed, kept): (Vec<_>, Vec<_>) =
                        open.drain(..).partition(|(_, r)| rules.is_empty() || r.iter().all(|x| rules.contains(x)));
                    open = kept;
                    out.ranges.extend(closed.into_iter().map(|(from, r)| (from, line + 1, r)));
                }
                _ => {
                    let alone = before.chars().all(|c| !c.is_alphanumeric());
                    let target = if alone { line + 1 } else { line };
                    let existing = out.lines.remove(&target);
                    out.lines.insert(target, merged(existing, rules));
                }
            }
        }
        out.ranges.extend(open.into_iter().map(|(from, r)| (from, usize::MAX, r)));
        out
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty() && self.lines.is_empty()
    }

    pub fn allows(&self, d: &Diagnostic, lines: &LineIndex) -> bool {
        let line = lines.line_of(d.start);
        if self.ranges.iter().any(|(from, to, rules)| *from <= line && line < *to && covers(rules, &d.rule)) {
            return false;
        }
        !self.lines.get(&line).is_some_and(|r| covers(r, &d.rule))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ast::Severity;

    fn diag(rule: &str, text: &str, needle: &str) -> Diagnostic {
        let start = text.find(needle).unwrap();
        Diagnostic::new(rule.into(), Severity::Warn, String::new(), Some(0), start, start + 1)
    }

    fn check(text: &str, cases: &[(&str, &str, bool)]) {
        let lines = LineIndex::new(text);
        let s = Suppressions::parse(text, &lines);
        for (rule, needle, allowed) in cases {
            assert_eq!(s.allows(&diag(rule, text, needle), &lines), *allowed, "{rule} at {needle}");
        }
    }

    #[test]
    fn line_and_next_line_directives() {
        check(
            "// dxlint-ignore\nA\nB // dxlint-ignore rule-b\nC\n",
            &[("any", "A", false), ("rule-b", "B", false), ("rule-x", "B", true), ("any", "C", true)],
        );
    }

    #[test]
    fn file_directive_with_globs_and_prose_mentions() {
        check("/* dxlint-ignore-file style-* */\nsee the dxlint-ignore docs\nX\n", &[("style-a", "X", false), ("other", "X", true)]);
    }

    #[test]
    fn ranges_open_and_close() {
        let text = "/* dxlint-ignore-start semi */\nA\n/* dxlint-ignore-end semi */\nB\n/*dxlint-ignore-start*/\nC\n";
        check(text, &[("semi:never", "A", false), ("quotes", "A", true), ("semi", "B", true), ("any", "C", false)]);
    }

    #[test]
    fn families_match_their_variants() {
        check("X // dxlint-ignore semi\n", &[("semi:never", "X", false), ("semicolon", "X", true)]);
    }
}
