//! Applying fixes. Formatters are allowed to overlap (an outer node and a
//! node inside it), so fixing runs in passes: apply every non-overlapping
//! fix, lint again, repeat until nothing changes.

use std::path::Path;

use super::{Diagnostic, Edit, Linter};

const MAX_PASSES: usize = 10;

pub struct Fixed {
    pub text: String,
    pub applied: usize,
    /// Problems left after fixing (unfixable, or still failing).
    pub remaining: Vec<Diagnostic>,
}

/// Applies as many fixes as possible without overlaps; returns the new text
/// and how many fixes were used.
pub fn apply_fixes(text: &str, diags: &[Diagnostic]) -> (String, usize) {
    let mut fixes: Vec<&Vec<Edit>> = diags.iter().filter_map(|d| d.fix.as_ref()).filter(|f| !f.is_empty()).collect();
    fixes.sort_by_key(|f| f.iter().map(|e| e.start).min());
    let mut chosen: Vec<&Edit> = Vec::new();
    let mut taken: Vec<(usize, usize)> = Vec::new();
    let mut applied = 0;
    for fix in fixes {
        let overlaps = fix.iter().any(|e| taken.iter().any(|&t| conflicts((e.start, e.end), t)));
        if overlaps {
            continue;
        }
        for e in fix {
            taken.push((e.start, e.end));
            chosen.push(e);
        }
        applied += 1;
    }
    chosen.sort_by_key(|e| (e.start, e.end));
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for e in chosen {
        if e.start < pos {
            continue;
        }
        out.push_str(&text[pos..e.start]);
        out.push_str(&e.text);
        pos = e.end;
    }
    out.push_str(&text[pos..]);
    (out, applied)
}

/// The smallest edit turning `source[start..end]` into `want`: common
/// leading and trailing text is left alone, so fixes touch only what
/// changes and rarely overlap.
pub fn minimal_edit(source: &str, start: usize, end: usize, want: &str) -> Edit {
    let have = &source[start..end];
    let mut prefix = have.bytes().zip(want.bytes()).take_while(|(a, b)| a == b).count();
    while !have.is_char_boundary(prefix) || !want.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let (have_rest, want_rest) = (&have[prefix..], &want[prefix..]);
    let mut suffix = have_rest.bytes().rev().zip(want_rest.bytes().rev()).take_while(|(a, b)| a == b).count();
    while !have_rest.is_char_boundary(have_rest.len() - suffix) || !want_rest.is_char_boundary(want_rest.len() - suffix) {
        suffix -= 1;
    }
    Edit {
        start: start + prefix,
        end: end - suffix,
        text: want_rest[..want_rest.len() - suffix].to_string(),
    }
}

/// Ranges conflict when they share any byte, or when an insertion sits at
/// the same start as another edit (their order would be ambiguous).
fn conflicts((s1, e1): (usize, usize), (s2, e2): (usize, usize)) -> bool {
    s1.max(s2) < e1.min(e2) || (s1 == s2 && (s1 == e1 || s2 == e2)) || (s1 == e1 && s2 < s1 && s1 < e2) || (s2 == e2 && s1 < s2 && s2 < e1)
}

impl Linter {
    pub fn fix(&self, path: &Path, text: &str) -> Fixed {
        let mut current = text.to_string();
        let mut applied = 0;
        for _ in 0..MAX_PASSES {
            let diags = self.lint(path, &current);
            let (next, n) = apply_fixes(&current, &diags);
            if n == 0 || next == current {
                return Fixed { text: current, applied, remaining: diags };
            }
            applied += n;
            current = next;
        }
        let remaining = self.lint(path, &current);
        Fixed { text: current, applied, remaining }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ast::Severity;

    fn fix(start: usize, end: usize, text: &str) -> Diagnostic {
        Diagnostic {
            rule: "r".into(),
            severity: Severity::Warn,
            message: String::new(),
            message_index: Some(0),
            start,
            end,
            fix: Some(vec![Edit { start, end, text: text.into() }]),
        }
    }

    #[test]
    fn minimal_edits_keep_shared_text() {
        let src = "xx\n\n\n\nyy";
        let e = minimal_edit(src, 2, 6, "\n\n");
        assert_eq!((e.start, e.end, e.text.as_str()), (4, 6, ""));
        let e = minimal_edit("aéb", 0, 4, "aèb");
        assert_eq!((e.start, e.end, e.text.as_str()), (1, 3, "è"));
        let e = minimal_edit("ab", 0, 2, "ab!");
        assert_eq!((e.start, e.end, e.text.as_str()), (2, 2, "!"));
    }

    #[test]
    fn overlapping_fixes_wait_for_the_next_pass() {
        let (out, n) = apply_fixes("abcdef", &[fix(0, 4, "X"), fix(2, 3, "Y"), fix(4, 6, "Z")]);
        assert_eq!((out.as_str(), n), ("XZ", 2));
    }
}
