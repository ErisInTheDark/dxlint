//! Mapping a file: its `transform`s' rewrites, applied once and strictly.
//! What a linter may skip and retry is a miscompile here, so overlapping
//! rewrites are errors. Everything between rewrites is kept text; inside one,
//! a node's text interpolated into the template is a copy of that node.
//! Language-neutral: protocol adapters (`protocol` items) translate it.

use std::ops::Range;
use std::path::Path;

use super::{Diagnostic, Linter};
use crate::config::ast::{Expr, ExprKind, FixDef, Name, TemplatePart};

/// How a piece of the output relates to the original text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    /// Original text left in place.
    Kept,
    /// A node's text copied by a rewrite (`{node}`): where tools take you
    /// for that node; errors in it map back.
    Copy,
    /// A further copy whose errors map back, but which tools don't take you
    /// to (`{mirror(node)}`).
    Mirror,
    /// Other text naming the same thing (`{renamed(node, text)}`).
    Renamed,
}

impl PieceKind {
    pub fn name(self) -> &'static str {
        match self {
            PieceKind::Kept => "kept",
            PieceKind::Copy => "copy",
            PieceKind::Mirror => "mirror",
            PieceKind::Renamed => "renamed",
        }
    }
}

/// A stretch of the output that comes from the original; what no piece
/// covers is made up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub output: Range<usize>,
    pub source: Range<usize>,
    pub kind: PieceKind,
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
    /// In output order.
    pub pieces: Vec<Piece>,
    pub problems: Vec<Problem>,
}

/// Rewrite text carries each copied node's provenance between Unicode
/// noncharacters (as fix placeholders do) until [`apply`] strips it:
/// `OPEN kind start,end SEP text CLOSE`.
pub mod copy_mark {
    use super::PieceKind;

    pub(super) const OPEN: char = '\u{FDD2}';
    pub(super) const SEP: char = '\u{FDD3}';
    pub(super) const CLOSE: char = '\u{FDD4}';

    pub fn mark(kind: PieceKind, start: usize, end: usize, text: &str) -> String {
        let kind = match kind {
            PieceKind::Mirror => 'm',
            PieceKind::Renamed => 'r',
            _ => 'c',
        };
        format!("{OPEN}{kind}{start},{end}{SEP}{text}{CLOSE}")
    }

    pub(super) fn kind(c: char) -> PieceKind {
        match c {
            'm' => PieceKind::Mirror,
            'r' => PieceKind::Renamed,
            _ => PieceKind::Copy,
        }
    }
}

/// The internal function rewrite templates call around each interpolation.
pub const MAPPED_TEXT: &str = "mapped_text";

/// A transform's `rewrite` with every interpolation in its texts marked
/// as a copy of the node it reads (`{mirror(x)}` and `{renamed(x, text)}`
/// choose otherwise); text that is not a node stays made up.
pub fn marked_rewrite(fix: &FixDef) -> FixDef {
    let mut fix = fix.clone();
    for part in &mut fix.parts {
        part.text = marked(&part.text);
    }
    fix
}

fn marked(text: &Expr) -> Expr {
    match &text.kind {
        ExprKind::Template(parts) => Expr {
            kind: ExprKind::Template(parts.iter().map(|part| match part {
                TemplatePart::Expr(e, source) => TemplatePart::Expr(mark_call(e), source.clone()),
                other => other.clone(),
            }).collect()),
            span: text.span,
        },
        ExprKind::Str(_) => text.clone(),
        // Text side by side: each side marks its own interpolations.
        ExprKind::Binary(crate::config::ast::BinOp::Join, a, b) => Expr {
            kind: ExprKind::Binary(crate::config::ast::BinOp::Join, Box::new(marked(a)), Box::new(marked(b))),
            span: text.span,
        },
        _ => mark_call(text),
    }
}

/// `mapped_text(kind, value, value.offset, value.end_offset, text)`.
fn mark_call(e: &Expr) -> Expr {
    let at = |kind: ExprKind| Expr { kind, span: e.span };
    let name = |text: &str| Name { text: text.into(), span: e.span };
    let (kind, value, text) = match &e.kind {
        ExprKind::Call(f, args) if f.text == "mirror" && args.len() == 1 => ("mirror", &args[0], args[0].clone()),
        ExprKind::Call(f, args) if f.text == "renamed" && args.len() == 2 => ("renamed", &args[0], args[1].clone()),
        _ => ("copy", e, e.clone()),
    };
    let span_of = |key: &str| at(ExprKind::Key(Box::new(value.clone()), name(key)));
    at(ExprKind::Call(name(MAPPED_TEXT), vec![at(ExprKind::Str(kind.into())), value.clone(), span_of("offset"), span_of("end_offset"), text]))
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
        rewritten(&mut mapped, text, &edit.text);
        at = edit.end;
        last = Some(rule);
    }
    verbatim(&mut mapped, text, at, text.len());
    mapped
}

fn verbatim(mapped: &mut Mapped, text: &str, from: usize, to: usize) {
    if from == to { return }
    let output = mapped.text.len();
    mapped.pieces.push(Piece { output: output..output + (to - from), source: from..to, kind: PieceKind::Kept });
    mapped.text.push_str(&text[from..to]);
}

/// A rewrite's text, its marked copies recorded as pieces. A copy whose
/// text is not the node's own names it under other text instead.
fn rewritten(mapped: &mut Mapped, original: &str, text: &str) {
    let mut rest = text;
    while let Some(open) = rest.find(copy_mark::OPEN) {
        mapped.text.push_str(&rest[..open]);
        let marked = &rest[open + copy_mark::OPEN.len_utf8()..];
        let (Some(sep), Some(close)) = (marked.find(copy_mark::SEP), marked.find(copy_mark::CLOSE)) else {
            rest = marked;
            continue;
        };
        let mut head = marked[..sep].chars();
        let kind = copy_mark::kind(head.next().unwrap_or('c'));
        let span = head.as_str().split_once(',').and_then(|(start, end)| Some(start.parse::<usize>().ok()?..end.parse::<usize>().ok()?));
        let copied = &marked[sep + copy_mark::SEP.len_utf8()..close];
        let output = mapped.text.len();
        mapped.text.push_str(copied);
        if let Some(source) = span.filter(|span| span.start < span.end && original.get(span.clone()).is_some()) {
            let kind = if kind != PieceKind::Renamed && original[source.clone()] != *copied { PieceKind::Renamed } else { kind };
            mapped.pieces.push(Piece { output: output..mapped.text.len(), source, kind });
        }
        rest = &marked[close + copy_mark::CLOSE.len_utf8()..];
    }
    mapped.text.push_str(rest);
}

impl Mapped {
    /// A source map (version 3) of the output, back to `original` in `file`:
    /// every piece is a mapping, copies included.
    pub fn source_map(&self, file: &str, original: &str) -> String {
        let columns = OriginalColumns::new(original);
        let mut writer = MapWriter { text: &self.text, mappings: String::new(), at: 0, column: 0, first: true, written_column: 0, line: 0, source_column: 0 };
        for piece in &self.pieces {
            writer.advance(piece.output.start);
            writer.mark(columns.at(piece.source.start));
            // Each output line of a same-text piece starts a mapping too.
            let end = piece.output.end;
            while piece.kind != PieceKind::Renamed {
                let Some(newline) = self.text[writer.at..end].find('\n') else { break };
                let line_start = writer.at + newline + 1;
                writer.advance(line_start);
                if line_start < end { writer.mark(columns.at(piece.source.start + (line_start - piece.output.start))) }
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
        assert_eq!(mapped.pieces, vec![Piece { output: 2..7, source: 7..12, kind: PieceKind::Kept }]);
        assert!(mapped.problems.is_empty());
    }

    /// Marked copies become pieces; one whose text is not the node's own
    /// names it instead, and text with no node stays made up.
    #[test]
    fn copies_in_rewrites_map_to_the_node_they_copy() {
        let original = "state x = 1;";
        let getter = format!(" get {} () {{ return {}; }}", copy_mark::mark(PieceKind::Copy, 6, 7, "x"), copy_mark::mark(PieceKind::Mirror, 6, 7, "x"));
        let field = copy_mark::mark(PieceKind::Copy, 6, 7, "$x");
        let mapped = apply(original, vec![edit(0, 7, &field), edit(12, 12, &getter)]);
        assert_eq!(mapped.text, "$x = 1; get x () { return x; }");
        assert_eq!(mapped.pieces, vec![
            Piece { output: 0..2, source: 6..7, kind: PieceKind::Renamed },
            Piece { output: 2..7, source: 7..12, kind: PieceKind::Kept },
            Piece { output: 12..13, source: 6..7, kind: PieceKind::Copy },
            Piece { output: 26..27, source: 6..7, kind: PieceKind::Mirror },
        ]);
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
        // A copy is a mapping too: `b` copied to the start of the output.
        let mapped = apply(original, vec![edit(0, 0, &copy_mark::mark(PieceKind::Copy, 2, 3, "b"))]);
        let map: serde_json::Value = serde_json::from_str(&mapped.source_map("f.wts", original)).unwrap();
        assert_eq!(map["mappings"], "AACA,CADA;AACA;");
    }
}
