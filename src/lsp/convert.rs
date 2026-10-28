//! dxlint types -> LSP JSON.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::config::ast::Severity;
use crate::config::error::ConfigError;
use crate::config::source::SourceMap;
use crate::engine::{Diagnostic, Edit, Fix, Linter};
use crate::line_index::LineIndex;

pub fn range(text: &str, lines: &LineIndex, start: usize, end: usize) -> Value {
    let (sl, sc) = lines.to_utf16(text, start);
    let (el, ec) = lines.to_utf16(text, end);
    json!({ "start": { "line": sl, "character": sc }, "end": { "line": el, "character": ec } })
}

fn severity(s: Severity) -> u8 {
    match s {
        Severity::Error => 1,
        Severity::Warn => 2,
        Severity::Info => 3,
        Severity::Hint | Severity::Off => 4,
    }
}

pub fn text_edits(text: &str, lines: &LineIndex, edits: &[Edit]) -> Value {
    Value::Array(edits.iter().map(|e| json!({ "range": range(text, lines, e.start, e.end), "newText": e.text })).collect())
}

/// Fixes travel inside `data`, so code actions need no server-side state.
pub fn diagnostic(text: &str, lines: &LineIndex, d: &Diagnostic) -> Value {
    let mut v = json!({
        "range": range(text, lines, d.start, d.end),
        "severity": severity(d.severity),
        "code": d.rule,
        "source": "dxlint",
        "message": d.message,
    });
    if let Some(fix) = &d.fix {
        v["data"]["fix"] = text_edits(text, lines, &fix.edits);
        if let Some(at) = rename_at(text, lines, fix) {
            v["data"]["rename"] = at;
        }
    }
    if !d.suggestions.is_empty() {
        v["data"]["suggestions"] = Value::Array(d.suggestions.iter().map(|s| {
            let mut suggestion = json!({ "title": s.title, "edits": text_edits(text, lines, &s.fix.edits) });
            if let Some(at) = rename_at(text, lines, &s.fix) {
                suggestion["rename"] = at;
            }
            suggestion
        }).collect());
    }
    v
}

/// Where the fix's first placeholder starts once the fix is applied: the
/// position an editor renames from.
fn rename_at(text: &str, lines: &LineIndex, fix: &Fix) -> Option<Value> {
    let at = fix.placeholders.iter().map(|range| range.start).min()?;
    let (line, character) = position_after(text, lines, &fix.edits, at);
    Some(json!({ "line": line, "character": character }))
}

/// The UTF-16 position of `at`, an offset in `text` after `edits` (sorted,
/// non-overlapping). Only the text from the first edit's line up to `at` is
/// rebuilt.
fn position_after(text: &str, lines: &LineIndex, edits: &[Edit], at: usize) -> (u32, u32) {
    let Some(first) = edits.first().filter(|first| first.start < at) else { return lines.to_utf16(text, at) };
    let line = lines.line_of(first.start);
    let line_start = lines.line_start(line);
    let mut rebuilt = String::from(&text[line_start..first.start]);
    let mut pos = first.start;
    let wanted = at - line_start;
    for edit in edits {
        if rebuilt.len() >= wanted { break; }
        rebuilt.push_str(&text[pos..edit.start]);
        rebuilt.push_str(&edit.text);
        pos = edit.end;
    }
    if rebuilt.len() < wanted {
        rebuilt.push_str(&text[pos..(pos + wanted - rebuilt.len()).min(text.len())]);
    }
    rebuilt.truncate(wanted.min(rebuilt.len()));
    let newlines = rebuilt.matches('\n').count();
    let column = &rebuilt[rebuilt.rfind('\n').map_or(0, |i| i + 1)..];
    ((line + newlines) as u32, column.encode_utf16().count() as u32)
}

/// Diagnostics of one file; each rule code links to where the rule is
/// defined.
pub fn diagnostics(text: &str, diags: &[Diagnostic], linter: &Linter) -> Value {
    let lines = LineIndex::new(text);
    let mut links: HashMap<&str, Option<String>> = HashMap::new();
    Value::Array(
        diags
            .iter()
            .map(|d| {
                let mut v = diagnostic(text, &lines, d);
                if let Some(href) = links.entry(&d.rule).or_insert_with(|| rule_link(linter, &d.rule)) {
                    v["codeDescription"] = json!({ "href": href });
                }
                v
            })
            .collect(),
    )
}

/// `file:///rules.dx#L12`: where a rule is defined.
fn rule_link(linter: &Linter, rule: &str) -> Option<String> {
    let r = linter.rules.iter().find(|r| r.name == rule)?;
    let f = linter.sources.get(r.span.file);
    let path = super::config_docs::source_path(f)?;
    Some(format!("{}#L{}", super::uri::from_path(&path), f.lines.line_of(r.span.start as usize) + 1))
}

/// A config error in its file; `uri_of` names the files of related spans.
pub fn config_error(sources: &SourceMap, e: &ConfigError, uri_of: &dyn Fn(crate::config::source::FileId) -> Option<String>) -> Value {
    let span_range = |span: crate::config::source::Span| {
        let f = sources.get(span.file);
        range(&f.text, &f.lines, span.start as usize, span.end as usize)
    };
    let range = match e.span {
        Some(span) => span_range(span),
        None => json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }),
    };
    let mut message = e.message.clone();
    for note in &e.notes {
        message.push_str("\nnote: ");
        message.push_str(note);
    }
    let mut v = json!({ "range": range, "severity": 1, "source": "dxlint", "code": "config", "message": message });
    let related: Vec<Value> = e
        .related
        .iter()
        .filter_map(|(span, label)| {
            Some(json!({ "location": { "uri": uri_of(span.file)?, "range": span_range(*span) }, "message": label }))
        })
        .collect();
    if !related.is_empty() {
        v["relatedInformation"] = Value::Array(related);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_after_a_fix_match_the_fixed_text() {
        let text = "x\n\tif (a && b) {\n\t\tgo(\"é\");\n\t}\n";
        let edits = [
            Edit { start: 3, end: 3, text: "const ok = a && b;\n\t".into() },
            Edit { start: 7, end: 13, text: "ok".into() },
            Edit { start: 23, end: 25, text: "ü\nü".into() },
        ];
        let fix = Fix { range: 3..25, edits: edits.to_vec(), placeholders: Vec::new() };
        let after = fix.apply(text);
        let after_lines = LineIndex::new(&after);
        let lines = LineIndex::new(text);
        for at in (0..=after.len()).filter(|&at| after.is_char_boundary(at)) {
            assert_eq!(position_after(text, &lines, &edits, at), after_lines.to_utf16(&after, at), "offset {at}");
        }
    }
}

/// Replaces the whole document with `new_text`.
pub fn whole_document_edit(old: &str, new_text: &str) -> Value {
    let lines = LineIndex::new(old);
    json!({ "range": range(old, &lines, 0, old.len()), "newText": new_text })
}
