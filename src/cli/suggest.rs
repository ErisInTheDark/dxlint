//! `dxlint suggest`: list files' suggestions, or apply one per file.

use std::path::{Path, PathBuf};

use anstyle::Style;

use super::{info, render, Output};
use crate::engine::cache::{self, Cache};
use crate::engine::{Diagnostic, Linter, LintRun};
use crate::line_index::LineIndex;

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();

pub struct Request<'a> {
    pub files: &'a [PathBuf],
    pub at: Option<&'a str>,
    pub rule: Option<&'a str>,
    /// 1-based, as listed.
    pub apply: Option<usize>,
    pub name: Option<&'a str>,
    /// Answer from `.dxcache` and keep whole lints there.
    pub cache: bool,
    pub stats: bool,
}

/// One config load and one run for every file: a file whose saved result
/// still holds is answered from the cache, and with `--rule` a file that
/// needs linting runs only that rule.
pub fn run(output: &mut Output<'_>, linter: &Linter, request: &Request<'_>) -> u8 {
    if request.name.is_some() && request.apply.is_none() {
        render::error(output, "--name fills the placeholders of an applied suggestion: add --apply N");
        return 2;
    }
    if request.at.is_some() && request.files.len() > 1 {
        render::error(output, "--at is a position in one file: name a single file with it");
        return 2;
    }
    let only = match request.rule {
        None => None,
        Some(rule) => {
            let only: Vec<bool> = linter.rules.iter().map(|r| !r.transform && names(rule, &r.name)).collect();
            if !only.contains(&true) {
                render::error(output, &format!("no rule `{rule}` in this config"));
                return 2;
            }
            Some(only)
        }
    };
    let started = std::time::Instant::now();
    let cache = request.cache.then(|| Cache::open(linter));
    let lint = Lint { linter, run: linter.run(), cache: cache.as_ref(), only: only.as_deref() };
    let code = request.files.iter().map(|file| suggest(output, &lint, request, file)).max().unwrap_or(0);
    drop(lint);
    if request.stats {
        let cached = cache.as_ref().map_or(0, Cache::hits);
        output.err(format_args!("{} files ({cached} from cache) in {:.1} ms", request.files.len(), started.elapsed().as_secs_f64() * 1000.0));
    }
    if let Some(cache) = cache {
        if let Err(e) = cache.save(false) {
            render::error(output, &format!("cannot write {}: {e}", cache::FILE_NAME));
        }
    }
    code
}

/// `rule`, or one of its parts (`rule:part`).
fn names(rule: &str, name: &str) -> bool {
    name == rule || name.strip_prefix(rule).is_some_and(|rest| rest.starts_with(':'))
}

struct Lint<'a> {
    linter: &'a Linter,
    run: LintRun<'a>,
    cache: Option<&'a Cache>,
    only: Option<&'a [bool]>,
}

impl Lint<'_> {
    fn diagnostics(&self, path: &Path, text: &str) -> Vec<Diagnostic> {
        let rel = self.linter.relative(path).to_string_lossy().replace('\\', "/");
        if let Some(saved) = self.cache.and_then(|cache| cache.result(&self.run, self.linter, &rel, path, text)) {
            return saved;
        }
        match (self.only, self.cache) {
            // Some rules' reports are no file's result: never stored.
            (Some(only), _) => self.run.lint_only(path, text, only),
            (None, Some(cache)) => {
                let (diags, reads) = self.run.lint_tracked(path, text);
                cache.store(&rel, text, &diags, reads.into());
                diags
            }
            (None, None) => self.run.lint(path, text),
        }
    }
}

fn suggest(output: &mut Output<'_>, lint: &Lint<'_>, request: &Request<'_>, file: &Path) -> u8 {
    let (text, _) = match info::read_source(output, lint.linter, file) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let path = output.cwd.join(file);
    let name = file.to_string_lossy().replace('\\', "/");
    let lines = LineIndex::new(&text);
    let at = match request.at.map(|at| info::position(&text, &lines, at).ok_or(at)).transpose() {
        Ok(at) => at.map(|(_, _, offset)| offset),
        Err(at) => {
            render::error(output, &format!("`--at` wants LINE:COL inside the file, got `{at}`"));
            return 2;
        }
    };
    let diagnostics = lint.diagnostics(&path, &text);
    let found: Vec<&Diagnostic> = diagnostics.iter()
        .filter(|d| !d.suggestions.is_empty() && request.rule.is_none_or(|rule| names(rule, &d.rule)) && at.is_none_or(|at| d.start <= at && at <= d.end))
        .collect();
    let place = |d: &Diagnostic| {
        let (line, col) = lines.line_col(&text, d.start);
        format!("{name}:{}:{}", line + 1, col + 1)
    };
    let here = if at.is_some() || request.rule.is_some() { " here" } else { "" };
    let in_file = if request.files.len() > 1 { format!(" in {name}") } else { String::new() };
    let Some(choice) = request.apply else {
        if found.is_empty() {
            output.err(format_args!("no suggestions{here}{in_file}"));
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
            render::error(output, &format!("no suggestions{here}{in_file}"));
            return 1;
        }
        many => {
            let places: Vec<String> = many.iter().map(|d| format!("{} [{}]", place(d), d.rule)).collect();
            render::error(output, &format!("{} problems have suggestions{here}{in_file}; narrow with --at or --rule:\n  {}", many.len(), places.join("\n  ")));
            return 2;
        }
    };
    let Some(suggestion) = choice.checked_sub(1).and_then(|i| d.suggestions.get(i)) else {
        render::error(output, &format!("--apply takes 1 to {} at {}", d.suggestions.len(), place(d)));
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
