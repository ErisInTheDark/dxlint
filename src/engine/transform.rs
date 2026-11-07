//! Mapping a file: its `transform`s' edits, applied once and strictly. What a
//! linter may skip and retry is a miscompile here, so overlapping edits are
//! errors. Everything between edits is verbatim: that is the span map.

use std::path::Path;

use super::{Diagnostic, Linter};

/// A verbatim stretch of the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub output: usize,
    pub original: usize,
    pub length: usize,
}

/// A problem in the original text, by byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Default)]
pub struct Mapped {
    pub text: String,
    /// In output order; what no segment covers is synthesized.
    pub segments: Vec<Segment>,
    pub problems: Vec<Problem>,
}

impl Linter {
    /// `text` of `path` as its transforms map it.
    pub fn mapped(&self, path: &Path, text: &str) -> Mapped {
        let only: Vec<bool> = self.rules.iter().map(|rule| rule.transform).collect();
        let run = self.run();
        apply(text, run.lint_only(path, text, &only))
    }
}

fn apply(text: &str, diagnostics: Vec<Diagnostic>) -> Mapped {
    let mut mapped = Mapped::default();
    let mut edits = Vec::new();
    for diagnostic in diagnostics {
        match diagnostic.fix {
            Some(fix) => edits.extend(fix.edits.into_iter().map(|edit| (edit, diagnostic.rule.clone()))),
            None => mapped.problems.push(Problem { message: diagnostic.message, start: diagnostic.start, end: diagnostic.end }),
        }
    }
    // Stable: insertions at one place stay in the order they were made.
    edits.sort_by_key(|(edit, _)| (edit.start, edit.end));
    let mut at = 0;
    let mut last: Option<&str> = None;
    for (edit, rule) in &edits {
        if edit.start < at {
            mapped.problems.push(Problem {
                message: format!("transform `{rule}` edits text `{}` already edits", last.unwrap_or(rule)),
                start: edit.start,
                end: edit.end,
            });
            continue;
        }
        verbatim(&mut mapped, text, at, edit.start);
        mapped.text.push_str(&edit.text);
        at = edit.end;
        last = Some(rule);
    }
    verbatim(&mut mapped, text, at, text.len());
    mapped
}

fn verbatim(mapped: &mut Mapped, text: &str, from: usize, to: usize) {
    if from == to { return }
    mapped.segments.push(Segment { output: mapped.text.len(), original: from, length: to - from });
    mapped.text.push_str(&text[from..to]);
}

impl Mapped {
    /// The tuples TypeScript's span map reads: (virtual start, length,
    /// original start, length, verbatim).
    pub fn span_map(&self) -> Vec<[usize; 5]> {
        self.segments.iter().map(|segment| [segment.output, segment.length, segment.original, segment.length, 0]).collect()
    }

    /// A source map (version 3) of the output, back to `original` in `file`.
    pub fn source_map(&self, file: &str, original: &str) -> String {
        let columns = OriginalColumns::new(original);
        let mut writer = MapWriter { text: &self.text, mappings: String::new(), at: 0, column: 0, first: true, written_column: 0, line: 0, source_column: 0 };
        for segment in &self.segments {
            writer.advance(segment.output);
            writer.mark(columns.at(segment.original));
            // Each output line of a verbatim stretch starts a mapping too.
            let end = segment.output + segment.length;
            while let Some(newline) = self.text[writer.at..end].find('\n') {
                let line_start = writer.at + newline + 1;
                writer.advance(line_start);
                if line_start < end { writer.mark(columns.at(segment.original + (line_start - segment.output))) }
            }
            writer.advance(end);
        }
        writer.advance(self.text.len());
        serde_json::json!({ "version": 3, "sources": [file], "sourcesContent": [original], "names": [], "mappings": writer.mappings }).to_string()
    }
}

/// A source map's `mappings`, written walking the output.
struct MapWriter<'a> {
    text: &'a str,
    mappings: String,
    at: usize,
    column: i64,
    first: bool,
    written_column: i64,
    line: i64,
    source_column: i64,
}

impl MapWriter<'_> {
    fn advance(&mut self, to: usize) {
        for unit in self.text[self.at..to].chars() {
            if unit == '\n' {
                self.mappings.push(';');
                self.column = 0;
                self.written_column = 0;
                self.first = true;
            } else {
                self.column += unit.len_utf16() as i64;
            }
        }
        self.at = to;
    }

    fn mark(&mut self, (line, source_column): (i64, i64)) {
        if !std::mem::take(&mut self.first) { self.mappings.push(',') }
        vlq(&mut self.mappings, self.column - self.written_column);
        vlq(&mut self.mappings, 0);
        vlq(&mut self.mappings, line - self.line);
        vlq(&mut self.mappings, source_column - self.source_column);
        (self.written_column, self.line, self.source_column) = (self.column, line, source_column);
    }
}

/// Byte offsets of `original` as (line, UTF-16 column).
struct OriginalColumns<'a> {
    text: &'a str,
    lines: Vec<usize>,
}

impl<'a> OriginalColumns<'a> {
    fn new(text: &'a str) -> Self {
        let lines = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect();
        OriginalColumns { text, lines }
    }

    fn at(&self, offset: usize) -> (i64, i64) {
        let line = self.lines.partition_point(|&start| start <= offset) - 1;
        (line as i64, self.text[self.lines[line]..offset].chars().map(|c| c.len_utf16() as i64).sum())
    }
}

fn vlq(out: &mut String, value: i64) {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rest = if value < 0 { ((-value) << 1) | 1 } else { value << 1 } as u64;
    loop {
        let mut digit = (rest & 31) as usize;
        rest >>= 5;
        if rest != 0 { digit |= 32 }
        out.push(DIGITS[digit] as char);
        if rest == 0 { break }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ast::Severity;
    use crate::engine::{Edit, Fix};

    fn edit(start: usize, end: usize, text: &str) -> Diagnostic {
        let fix = Fix { range: start..end, edits: vec![Edit { start, end, text: text.into() }], placeholders: Vec::new() };
        Diagnostic { fix: Some(fix), ..Diagnostic::new("t".into(), Severity::Warn, String::new(), Some(0), start, end) }
    }

    #[test]
    fn what_edits_leave_alone_maps_back_exactly() {
        let mapped = apply("state x = 1;", vec![edit(0, 6, ""), edit(12, 12, " get x"), edit(6, 7, "$x")]);
        assert_eq!(mapped.text, "$x = 1; get x");
        assert_eq!(mapped.segments, vec![Segment { output: 2, original: 7, length: 5 }]);
        assert!(mapped.problems.is_empty());
    }

    #[test]
    fn overlapping_edits_are_problems_not_skips() {
        let mapped = apply("abcd", vec![edit(0, 3, "X"), edit(2, 4, "Y")]);
        assert_eq!(mapped.problems.len(), 1);
        assert_eq!((mapped.problems[0].start, mapped.problems[0].end), (2, 4));
    }

    #[test]
    fn a_diagnostic_without_a_fix_is_a_problem() {
        let mapped = apply("ab", vec![Diagnostic::new("t".into(), Severity::Error, "no".into(), Some(0), 0, 1)]);
        assert_eq!(mapped.text, "ab");
        assert_eq!(mapped.problems, vec![Problem { message: "no".into(), start: 0, end: 1 }]);
    }

    #[test]
    fn source_maps_point_each_line_at_its_original() {
        let original = "a\nbc\n";
        let mapped = apply(original, vec![edit(2, 2, "zz\n")]);
        assert_eq!(mapped.text, "a\nzz\nbc\n");
        let map: serde_json::Value = serde_json::from_str(&mapped.source_map("f.wts", original)).unwrap();
        // a: line 0; zz: nothing; bc: line 1.
        assert_eq!(map["mappings"], "AAAA;;AACA;");
    }
}
