//! Terminal output: config errors and diagnostics with source snippets.

use anstyle::{AnsiColor, Style};

use super::Output;
use crate::config::ast::Severity;
use crate::config::error::ConfigError;
use crate::config::source::SourceMap;
use crate::engine::Diagnostic;
use crate::line_index::LineIndex;

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();

fn color(c: AnsiColor) -> Style {
    Style::new().fg_color(Some(c.into())).bold()
}

pub fn severity_style(s: Severity) -> Style {
    match s {
        Severity::Error => color(AnsiColor::Red),
        Severity::Warn => color(AnsiColor::Yellow),
        Severity::Info => color(AnsiColor::Blue),
        Severity::Hint | Severity::Off => color(AnsiColor::Cyan),
    }
}

pub fn error(output: &mut Output<'_>, msg: &str) {
    let red = color(AnsiColor::Red);
    output.err(format_args!("{red}error{red:#}{BOLD}: {msg}{BOLD:#}"));
}

/// A source line with a `^^^` underline under `start..end` (clamped to the line).
fn snippet(out: &mut String, text: &str, lines: &LineIndex, start: usize, end: usize, style: Style) {
    let line = lines.line_of(start);
    let line_text = lines.line_text(text, line);
    let line_start = lines.line_start(line);
    let col = text[line_start..start.min(line_start + line_text.len())].chars().count();
    let end_on_line = end.min(line_start + line_text.len()).max(start);
    let width = text[start.min(end_on_line)..end_on_line].chars().count().max(1);
    let num = (line + 1).to_string();
    let pad = " ".repeat(num.len());
    let shown: String = line_text.replace('\t', " ");
    out.push_str(&format!("{DIM}{pad} |{DIM:#}\n{DIM}{num} |{DIM:#} {shown}\n{DIM}{pad} |{DIM:#} {}{style}{}{style:#}\n", " ".repeat(col), "^".repeat(width)));
}

pub fn config_errors(output: &mut Output<'_>, errors: &[ConfigError], sources: &SourceMap) {
    let red = color(AnsiColor::Red);
    for e in errors {
        let mut out = format!("{red}error{red:#}{BOLD}: {}{BOLD:#}\n", e.message);
        if let Some(span) = e.span {
            let file = sources.get(span.file);
            let (line, col) = file.lines.line_col(&file.text, span.start as usize);
            out.push_str(&format!("  {DIM}-->{DIM:#} {}:{}:{}\n", file.name, line + 1, col + 1));
            snippet(&mut out, &file.text, &file.lines, span.start as usize, span.end as usize, red);
        }
        for note in &e.notes {
            out.push_str(&format!("  {BOLD}= note:{BOLD:#} {note}\n"));
        }
        for (span, label) in &e.related {
            let file = sources.get(span.file);
            let (line, col) = file.lines.line_col(&file.text, span.start as usize);
            out.push_str(&format!("  {DIM}-->{DIM:#} {}:{}:{} {label}\n", file.name, line + 1, col + 1));
            snippet(&mut out, &file.text, &file.lines, span.start as usize, span.end as usize, DIM);
        }
        output.err(format_args!("{out}"));
    }
    let n = errors.len();
    error(output, &format!("config has {n} error{}", if n == 1 { "" } else { "s" }));
}

pub fn diagnostics(output: &mut Output<'_>, name: &str, text: &str, diags: &[Diagnostic]) {
    if diags.is_empty() {
        return;
    }
    let lines = LineIndex::new(text);
    let mut out = String::new();
    for d in diags {
        let style = severity_style(d.severity);
        let (line, col) = lines.line_col(text, d.start);
        let fixable = if d.fix.is_some() { format!(" {DIM}(fixable){DIM:#}") } else { String::new() };
        out.push_str(&format!(
            "{style}{}{style:#}{BOLD}: {}{BOLD:#} {DIM}[{}]{DIM:#}{fixable}\n  {DIM}-->{DIM:#} {name}:{}:{}\n",
            d.severity.as_str(),
            d.message,
            d.rule,
            line + 1,
            col + 1
        ));
        snippet(&mut out, text, &lines, d.start, d.end, style);
        out.push('\n');
    }
    let _ = output.stdout.write_all(out.as_bytes());
}

pub fn summary(output: &mut Output<'_>, errors: usize, warnings: usize, other: usize, fixable: usize, files: usize) {
    let total = errors + warnings + other;
    if total == 0 {
        let green = color(AnsiColor::Green);
        output.out(format_args!("{green}ok{green:#} {DIM}{files} file{} clean{DIM:#}", if files == 1 { "" } else { "s" }));
        return;
    }
    let red = color(AnsiColor::Red);
    let yellow = color(AnsiColor::Yellow);
    let mut parts = Vec::new();
    if errors > 0 {
        parts.push(format!("{red}{errors} error{}{red:#}", if errors == 1 { "" } else { "s" }));
    }
    if warnings > 0 {
        parts.push(format!("{yellow}{warnings} warning{}{yellow:#}", if warnings == 1 { "" } else { "s" }));
    }
    if other > 0 {
        parts.push(format!("{other} other"));
    }
    let fix_hint = if fixable > 0 { format!(", {fixable} fixable with --fix") } else { String::new() };
    output.out(format_args!("{} {DIM}in {files} file{}{fix_hint}{DIM:#}", parts.join(", "), if files == 1 { "" } else { "s" }));
}
