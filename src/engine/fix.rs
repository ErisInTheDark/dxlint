//! Applying fixes. Formatters are allowed to overlap (an outer node and a
//! node inside it), so fixing runs in passes: apply every non-overlapping
//! fix, lint again, repeat until nothing changes.

use std::ops::Range;
use std::path::Path;

use super::{Diagnostic, Edit, Fix, Linter};
use crate::config::ast::FixTarget;
use crate::expr::CExpr;

const MAX_PASSES: usize = 10;

/// A compiled `fix` or `suggest` clause.
pub(super) struct FixPlan {
    /// `suggest "title"`; `None` for a rule's own fix.
    pub title: Option<Clause>,
    pub when: Option<Clause>,
    pub parts: Vec<PartPlan>,
}

/// One region of a [`FixPlan`].
pub(super) struct PartPlan {
    pub target: FixTarget,
    pub at: Option<Clause>,
    pub text: Clause,
}

/// An expression and its VM form ([`Linter::lower_conditions`]).
pub(super) struct Clause {
    pub expr: CExpr,
    pub vm: Option<crate::vm::VExpr>,
}

impl Clause {
    pub fn new(expr: CExpr) -> Self {
        Clause { expr, vm: None }
    }
}

impl FixPlan {
    pub fn clauses(&self) -> impl Iterator<Item = &Clause> {
        self.title.iter().chain(&self.when).chain(self.parts.iter().flat_map(|part| part.at.iter().chain([&part.text])))
    }

    pub fn clauses_mut(&mut self) -> impl Iterator<Item = &mut Clause> {
        self.title.iter_mut().chain(&mut self.when).chain(self.parts.iter_mut().flat_map(|part| part.at.iter_mut().chain([&mut part.text])))
    }
}

/// `placeholder(name)`: a name the user is expected to choose. Fix text
/// carries it between two Unicode noncharacters (reserved for exactly this
/// kind of internal use); [`Fix::from_regions`] strips them and records where
/// each placeholder lands.
pub mod placeholder {
    use std::ops::Range;

    const OPEN: char = '\u{FDD0}';
    const CLOSE: char = '\u{FDD1}';

    pub fn mark(name: &str) -> String {
        format!("{OPEN}{name}{CLOSE}")
    }

    /// The text without marks, and each placeholder's range in it.
    pub fn strip(text: &str) -> (String, Vec<Range<usize>>) {
        if !text.contains(OPEN) {
            return (text.to_string(), Vec::new());
        }
        let mut out = String::with_capacity(text.len());
        let mut marks = Vec::new();
        let mut open = None;
        for c in text.chars() {
            match c {
                OPEN => open = Some(out.len()),
                CLOSE => {
                    if let Some(start) = open.take() {
                        marks.push(start..out.len());
                    }
                }
                c => out.push(c),
            }
        }
        (out, marks)
    }
}

/// One region of a fix: `source[start..end]` should read `text`, which may
/// still carry placeholder marks.
pub(super) struct Region {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Fix {
    /// Regions rewritten together, as minimal edits. Unchanged regions add
    /// no edit; a fix with no edits changes nothing.
    pub(super) fn from_regions(source: &str, mut regions: Vec<Region>) -> Result<Fix, String> {
        regions.sort_by_key(|region| (region.start, region.end));
        if regions.windows(2).any(|pair| pair[1].start < pair[0].end) {
            return Err("fix regions overlap".to_owned());
        }
        let mut edits = Vec::new();
        let mut placeholders = Vec::new();
        let mut range: Option<Range<usize>> = None;
        // How far text after the regions so far moves once they are applied.
        let mut shift = 0isize;
        for region in regions {
            let (text, marks) = placeholder::strip(&region.text);
            let at = region.start.wrapping_add_signed(shift);
            placeholders.extend(marks.into_iter().map(|mark| at + mark.start..at + mark.end));
            shift += text.len() as isize - (region.end - region.start) as isize;
            if text != source[region.start..region.end] {
                edits.push(minimal_edit(source, region.start, region.end, &text));
                range = Some(match range {
                    Some(range) => range.start..region.end,
                    None => region.start..region.end,
                });
            }
        }
        let range = range.unwrap_or(0..0);
        Ok(Fix { range, edits, placeholders })
    }

    /// `text` with only this fix applied.
    pub fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut pos = 0;
        for edit in &self.edits {
            out.push_str(&text[pos..edit.start]);
            out.push_str(&edit.text);
            pos = edit.end;
        }
        out.push_str(&text[pos..]);
        out
    }
}

pub struct Fixed {
    pub text: String,
    pub applied: usize,
    /// Problems left after fixing (unfixable, or still failing).
    pub remaining: Vec<Diagnostic>,
}

/// Applies fixes in declared-range order; touching ranges and gaps within
/// atomic fixes are reserved for the next pass.
pub fn apply_fixes(text: &str, diags: &[Diagnostic]) -> (String, usize) {
    let mut fixes: Vec<_> = diags.iter().filter_map(|d| d.fix.as_ref()).filter(|f| !f.edits.is_empty()).collect();
    fixes.sort_by_key(|fix| (fix.range.start, fix.range.end));
    let mut chosen: Vec<&Edit> = Vec::new();
    let mut last_end = None;
    let mut applied = 0;
    for fix in fixes {
        if last_end.is_some_and(|previous| previous >= fix.range.start) {
            continue;
        }
        chosen.extend(fix.edits.iter());
        last_end = Some(fix.range.end);
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
        let fix = Fix { range: start..end, edits: vec![Edit { start, end, text: text.into() }], placeholders: Vec::new() };
        Diagnostic { fix: Some(fix), ..Diagnostic::new("r".into(), Severity::Warn, String::new(), Some(0), start, end) }
    }

    #[test]
    fn regions_apply_together_and_place_their_placeholders_after_the_edit() {
        let source = "if (a && b) go();";
        let name = placeholder::mark("ready");
        let regions = vec![
            Region { start: 4, end: 10, text: name.clone() },
            Region { start: 0, end: 0, text: format!("const {name} = a && b;\n") },
        ];
        let fix = Fix::from_regions(source, regions).unwrap();
        let after = fix.apply(source);
        assert_eq!(after, "const ready = a && b;\nif (ready) go();");
        let named: Vec<&str> = fix.placeholders.iter().map(|range| &after[range.clone()]).collect();
        assert_eq!(named, ["ready", "ready"]);
        assert_eq!(apply_fixes(source, &[Diagnostic { fix: Some(fix), ..Diagnostic::new("r".into(), Severity::Warn, String::new(), Some(0), 4, 10) }]).0, after);
        assert!(Fix::from_regions(source, vec![Region { start: 0, end: 5, text: "x".into() }, Region { start: 4, end: 6, text: "y".into() }]).is_err());
    }

    #[test]
    fn declared_rewrite_range_wins_over_a_nested_minimal_edit() {
        let linter = crate::engine::tests::linter(r#"
            in .t match outer /a/ inner:/b/ /c/ {
                rule outer warn "outer" fix as "abX"
            }
            in outer.inner rule inner warn "inner" fix as "B"
        "#);
        let diagnostics = linter.lint(Path::new("/root/file.t"), "abc");
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(apply_fixes("abc", &diagnostics), ("abX".into(), 1));
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
        assert_eq!((out.as_str(), n), ("Xef", 1));
    }

    #[test]
    fn an_atomic_fix_reserves_the_gaps_between_its_edits() {
        let mut grouped = fix(5, 6, "F");
        if let Some(fix) = &mut grouped.fix {
            fix.range.start = 0;
            fix.edits.push(Edit { start: 0, end: 1, text: "A".into() });
        }
        let (out, n) = apply_fixes("abcdefgh", &[grouped, fix(2, 3, "C"), fix(7, 8, "H")]);
        assert_eq!((out.as_str(), n), ("AbcdeFgH", 2));
    }

    #[test]
    fn the_shorter_covering_range_wins_at_the_same_start() {
        let (out, n) = apply_fixes("abc", &[fix(0, 2, "X"), fix(0, 1, "Y")]);
        assert_eq!((out.as_str(), n), ("Ybc", 1));
    }

    #[test]
    fn insertions_conflict_at_boundaries_but_can_start_a_file() {
        let (out, n) = apply_fixes("abc", &[fix(0, 1, "X"), fix(1, 1, "!")]);
        assert_eq!((out.as_str(), n), ("Xbc", 1));
        let (out, n) = apply_fixes("abc", &[fix(0, 1, "X"), fix(0, 0, "!")]);
        assert_eq!((out.as_str(), n), ("!abc", 1));
    }
}
