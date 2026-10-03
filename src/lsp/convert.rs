//! zlint types -> LSP JSON.

use std::collections::HashMap;

use serde_json::{json, Value};

use zoomie_lint::config::ast::Severity;
use zoomie_lint::config::error::ConfigError;
use zoomie_lint::config::source::SourceMap;
use zoomie_lint::engine::{Diagnostic, Edit, Linter};
use zoomie_lint::line_index::LineIndex;

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
        "source": "zlint",
        "message": d.message,
    });
    if let Some(fix) = &d.fix {
        v["data"] = json!({ "fix": text_edits(text, lines, fix) });
    }
    v
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

/// `file:///rules.zlint#L12`: where a rule is defined.
fn rule_link(linter: &Linter, rule: &str) -> Option<String> {
    let r = linter.rules.iter().find(|r| r.name == rule)?;
    let f = linter.sources.get(r.span.file);
    let path = super::config_docs::source_path(f)?;
    Some(format!("{}#L{}", super::uri::from_path(&path), f.lines.line_of(r.span.start as usize) + 1))
}

/// A config error in its file; `uri_of` names the files of related spans.
pub fn config_error(sources: &SourceMap, e: &ConfigError, uri_of: &dyn Fn(zoomie_lint::config::source::FileId) -> Option<String>) -> Value {
    let span_range = |span: zoomie_lint::config::source::Span| {
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
    let mut v = json!({ "range": range, "severity": 1, "source": "zlint", "code": "config", "message": message });
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

/// Replaces the whole document with `new_text`.
pub fn whole_document_edit(old: &str, new_text: &str) -> Value {
    let lines = LineIndex::new(old);
    json!({ "range": range(old, &lines, 0, old.len()), "newText": new_text })
}
