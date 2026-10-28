//! `dxlint suggest`: list a file's suggestions, or apply one.

use std::path::Path;

use anstyle::Style;

use super::{info, render, Output};
use crate::engine::{Diagnostic, Linter};
use crate::line_index::LineIndex;

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();

pub struct Request<'a> {
    pub file: &'a Path,
    pub at: Option<&'a str>,
    pub rule: Option<&'a str>,
    /// 1-based, as listed.
    pub apply: Option<usize>,
    pub name: Option<&'a str>,
}

pub fn run(output: &mut Output<'_>, linter: &Linter, request: &Request<'_>) -> u8 {
    if request.name.is_some() && request.apply.is_none() {
        render::error(output, "--name fills the placeholders of an applied suggestion: add --apply N");
        return 2;
    }
    let (text, _) = match info::read_source(output, linter, request.file) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let path = output.cwd.join(request.file);
    let lines = LineIndex::new(&text);
    let at = match request.at.map(|at| info::position(&text, &lines, at).ok_or(at)).transpose() {
        Ok(at) => at.map(|(_, _, offset)| offset),
        Err(at) => {
            render::error(output, &format!("`--at` wants LINE:COL inside the file, got `{at}`"));
            return 2;
        }
    };
    let diagnostics = linter.lint(&path, &text);
    let rule_matches = |d: &Diagnostic| request.rule.is_none_or(|rule| d.rule == rule || d.rule.strip_prefix(rule).is_some_and(|rest| rest.starts_with(':')));
    let found: Vec<&Diagnostic> = diagnostics.iter()
        .filter(|d| !d.suggestions.is_empty() && rule_matches(d) && at.is_none_or(|at| d.start <= at && at <= d.end))
        .collect();
    let name = request.file.to_string_lossy().replace('\\', "/");
    let place = |d: &Diagnostic| {
        let (line, col) = lines.line_col(&text, d.start);
        format!("{name}:{}:{}", line + 1, col + 1)
    };
    let Some(choice) = request.apply else {
        if found.is_empty() {
            output.err(format_args!("no suggestions{}", if at.is_some() || request.rule.is_some() { " here" } else { "" }));
            return 1;
        }
        for d in &found {
            output.out(format_args!("{BOLD}{}{BOLD:#} {} {DIM}[{}]{DIM:#}", place(d), d.message, d.rule));
            for (i, suggestion) in d.suggestions.iter().enumerate() {
                let renames = if suggestion.fix.placeholders.is_empty() { "" } else { " (leaves a name to choose)" };
                output.out(format_args!("  {}. {}{DIM}{renames}{DIM:#}", i + 1, suggestion.title));
            }
        }
        return 0;
    };
    let d = match found.as_slice() {
        [d] => d,
        [] => {
            render::error(output, "no suggestions here");
            return 1;
        }
        many => {
            let places: Vec<String> = many.iter().map(|d| format!("{} [{}]", place(d), d.rule)).collect();
            render::error(output, &format!("{} problems have suggestions here; narrow with --at or --rule:\n  {}", many.len(), places.join("\n  ")));
            return 2;
        }
    };
    let Some(suggestion) = choice.checked_sub(1).and_then(|i| d.suggestions.get(i)) else {
        render::error(output, &format!("--apply takes 1 to {} here", d.suggestions.len()));
        return 2;
    };
    if super::workbench::holds(&path) {
        render::error(output, &format!("{name} is claimed by another Workbench thread: not rewritten"));
        return 2;
    }
    let mut after = suggestion.fix.apply(&text);
    let mut placeholders = suggestion.fix.placeholders.clone();
    if let Some(chosen) = request.name {
        placeholders.sort_by_key(|range| range.start);
        for range in placeholders.drain(..).rev() {
            after.replace_range(range, chosen);
        }
    }
    if let Err(e) = std::fs::write(&path, &after) {
        render::error(output, &format!("cannot write {name}: {e}"));
        return 2;
    }
    output.out(format_args!("applied \"{}\" at {}", suggestion.title, place(d)));
    let after_lines = LineIndex::new(&after);
    if let Some(first) = placeholders.iter().min_by_key(|range| range.start) {
        let (line, col) = after_lines.line_col(&after, first.start);
        output.out(format_args!("placeholder `{}` at {name}:{}:{}: rename it (or apply with --name)", &after[first.clone()], line + 1, col + 1));
    }
    0
}
