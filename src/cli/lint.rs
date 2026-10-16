//! `zlint [paths]`: walk, lint (and fix) in parallel, report.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use ignore::{WalkBuilder, WalkState};
use serde::Serialize;

use super::{load_for_lint, render, ConfigArg, Format, LintArgs, Output, Project, Sources};
use crate::config::ast::Severity;
use crate::engine::cache::{self, Cache};
use crate::engine::{apply_fixes, Diagnostic, Edit, Linter, LintRun};
use crate::line_index::LineIndex;
use crate::engine::reads;

struct FileResult {
    path: PathBuf,
    text: String,
    diags: Vec<Diagnostic>,
    fixed: usize,
}

enum Msg {
    File(FileResult),
    Error(PathBuf, String),
}

#[derive(Serialize)]
struct JsonDiag<'a> {
    file: &'a str,
    rule: &'a str,
    severity: Severity,
    message: &'a str,
    message_index: Option<usize>,
    line: usize,
    col: usize,
    end_line: usize,
    end_col: usize,
    fixable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<JsonFix<'a>>,
}

#[derive(Serialize)]
struct JsonFixPreview<'a> {
    file: &'a str,
    kind: &'static str,
    text: String,
}

#[derive(Serialize)]
struct JsonFix<'a> {
    #[serde(serialize_with = "serialize_edits")]
    edits: &'a [Edit],
}

#[derive(Serialize)]
struct JsonEdit<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn serialize_edits<S: serde::Serializer>(edits: &[Edit], serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut sequence = serializer.serialize_seq(Some(edits.len()))?;
    for edit in edits {
        sequence.serialize_element(&JsonEdit { start: edit.start, end: edit.end, text: &edit.text })?;
    }
    sequence.end()
}

impl<'a> JsonDiag<'a> {
    fn new(file: &'a str, text: &str, lines: &LineIndex, diagnostic: &'a Diagnostic) -> Self {
        let (line, col) = lines.line_col(text, diagnostic.start);
        let (end_line, end_col) = lines.line_col(text, diagnostic.end);
        Self {
            file,
            rule: &diagnostic.rule,
            severity: diagnostic.severity,
            message: &diagnostic.message,
            message_index: diagnostic.message_index,
            line: line + 1,
            col: col + 1,
            end_line: end_line + 1,
            end_col: end_col + 1,
            fixable: diagnostic.fix.is_some(),
            fix: diagnostic.fix.as_ref().map(|fix| JsonFix { edits: &fix.edits }),
        }
    }
}

fn display(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// `ZLINT_PROFILE_EVERY=<seconds>` with `--profile`: print the busiest
/// semantic queries while the run is still going (for runs that hang).
fn watch_every() -> Option<std::time::Duration> {
    let seconds: u64 = std::env::var("ZLINT_PROFILE_EVERY").ok()?.parse().ok()?;
    (seconds > 0).then(|| std::time::Duration::from_secs(seconds))
}

fn watch(linter: &Linter, profile: &crate::engine::Profile, every: std::time::Duration, working: &std::sync::atomic::AtomicUsize) {
    let attribute = |index: usize| u16::try_from(index).ok()
        .and_then(|index| linter.attrs.definition(index)).map_or_else(|| index.to_string(), |(name, _)| name.to_string());
    let tick = std::time::Duration::from_millis(100);
    let mut waited = std::time::Duration::ZERO;
    while working.load(std::sync::atomic::Ordering::Relaxed) > 0 {
        std::thread::sleep(tick);
        waited += tick;
        if waited >= every {
            waited = std::time::Duration::ZERO;
            eprintln!("{}", profile.watch_line(attribute));
        }
    }
}

pub(super) fn walker(roots: &[PathBuf], jobs: usize, exclude: &'static [&'static str]) -> WalkBuilder {
    let mut walker = WalkBuilder::new(&roots[0]);
    for root in &roots[1..] { walker.add(root); }
    walker.hidden(false).threads(jobs).filter_entry(move |entry|
        entry.file_name() != ".git" && !exclude.iter().any(|name| entry.file_name() == *name));
    walker
}

fn lint_file(linter: &Linter, run: &LintRun<'_>, cache: Option<&Cache>, path: &Path, fix: bool) -> Msg {
    let text = if let Some(text) = reads::buffer(path) {
        text.to_string()
    } else {
        match std::fs::read(path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(t) => t,
                Err(_) => return Msg::File(FileResult { path: path.to_path_buf(), text: String::new(), diags: Vec::new(), fixed: 0 }),
            },
            Err(e) => return Msg::Error(path.to_path_buf(), e.to_string()),
        }
    };
    let rel = linter.relative(path).to_string_lossy().replace('\\', "/");
    // With `--fix`, a cached result only saves work when there is nothing
    // to fix.
    if let Some((diags, _)) = cache.and_then(|c| c.lookup(linter, &rel, &text)) {
        if !fix || diags.iter().all(|d| d.fix.is_none()) {
            return Msg::File(FileResult { path: path.to_path_buf(), text, diags, fixed: 0 });
        }
    }
    if !fix {
        let diags = match cache {
            Some(cache) => {
                let (diags, reads) = run.lint_tracked(path, &text);
                cache.store(&rel, &text, &diags, reads.into());
                diags
            }
            None => run.lint(path, &text),
        };
        return Msg::File(FileResult { path: path.to_path_buf(), text, diags, fixed: 0 });
    }
    let fixed = linter.fix(path, &text);
    if fixed.applied > 0 && fixed.text != text {
        if let Err(e) = std::fs::write(path, &fixed.text) {
            return Msg::Error(path.to_path_buf(), format!("cannot write fixes: {e}"));
        }
    }
    Msg::File(FileResult { path: path.to_path_buf(), text: fixed.text, diags: fixed.remaining, fixed: fixed.applied })
}

pub(super) fn run(output: &mut Output<'_>, args: LintArgs, config: &ConfigArg, sources: Option<&Sources>) -> u8 {
    let mut stdout = std::io::BufWriter::with_capacity(1 << 16, &mut *output.stdout);
    let mut buffered = Output { cwd: output.cwd, stdout: &mut stdout, stderr: &mut *output.stderr };
    let code = lint(&mut buffered, args, config, sources);
    let _ = buffered.stdout.flush();
    code
}

fn lint(output: &mut Output<'_>, args: LintArgs, config: &ConfigArg, sources: Option<&Sources>) -> u8 {
    let started = Instant::now();
    let mut project = match load_for_lint(output, config) {
        Ok(project) => project,
        Err(code) => return code,
    };
    prepared(output, args, &mut project, sources, started)
}

pub(super) fn prepared(output: &mut Output<'_>, args: LintArgs, project: &mut Project, sources: Option<&Sources>, started: Instant) -> u8 {
    let Project { linter, config_ok, load_time, build_time, build_phases, .. } = project;
    if args.profile {
        linter.enable_profile();
    }
    // Profiling measures linting, so it never reads results instead.
    let cache = (!args.no_cache && !args.profile && !args.fix_preview).then(|| Cache::open(&linter));
    let cwd = output.cwd;
    let roots: Vec<PathBuf> = if args.paths.is_empty() { vec![linter.root.clone()] } else { args.paths.iter().map(|p| cwd.join(p)).collect() };
    let jobs = args.jobs.map_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()), |n| n.get());

    let packed = sources.map(|sources| sources.files.iter().filter(|(_, path)| linter.wants(path)).cloned().collect::<Vec<_>>());

    let (tx, rx) = mpsc::channel::<Msg>();
    let fix = args.fix;
    let format = args.format;
    let mut fired = crate::fx_hash::FxHashMap::<String, u64>::default();
    let mut errors = 0;
    let mut warnings = 0;
    let mut other = 0;
    let mut fixable = 0;
    let mut fixed_total = 0;
    let mut files = 0usize;
    let mut bytes = 0usize;
    let mut failed = false;

    std::thread::scope(|s| {
        let linter = &mut *linter;
        let cache = cache.as_ref();
        let explicit: Vec<PathBuf> = roots.iter().filter(|r| r.is_file()).cloned().collect();
        s.spawn(move || {
            let mut queue = if let Some(queue) = packed {
                queue
            } else {
                let walker = walker(&roots, jobs, &[]);
                // Walk first, then lint the biggest files first: a big file found
                // late would otherwise be the tail every other thread waits on.
                let found: std::sync::Mutex<Vec<(u64, PathBuf)>> = std::sync::Mutex::default();
                walker.build_parallel().run(|| {
                    let linter = &*linter;
                    let explicit = &explicit;
                    let found = &found;
                    Box::new(move |entry| {
                        let Ok(entry) = entry else { return WalkState::Continue };
                        if !entry.file_type().is_some_and(|t| t.is_file()) {
                            return WalkState::Continue;
                        }
                        let path = entry.path();
                        // Explicitly named files are linted even if ignored by git.
                        let wanted = linter.wants(path) || (explicit.iter().any(|e| e == path) && !linter.is_ignored(path));
                        if wanted {
                            let size = entry.metadata().map_or(0, |m| m.len());
                            found.lock().expect("file list").push((size, path.to_path_buf()));
                        }
                        WalkState::Continue
                    })
                });
                found.into_inner().expect("file list")
            };
            queue.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let next = std::sync::atomic::AtomicUsize::new(0);
            let threads = jobs.min(queue.len().max(1));
            // File and rule workers share one budget, rather than multiplying it.
            linter.set_rule_jobs(jobs / threads);
            linter.set_discovery_jobs(jobs / threads);
            let linter = &*linter;
            let run = linter.run();
            let working = std::sync::atomic::AtomicUsize::new(threads);
            std::thread::scope(|w| {
                if let (Some(every), Some(profile)) = (watch_every(), linter.profile.as_ref()) {
                    let working = &working;
                    w.spawn(move || watch(linter, profile, every, working));
                }
                for _ in 0..threads {
                    let tx = tx.clone();
                    let (queue, next, working) = (&queue, &next, &working);
                    let run = &run;
                    std::thread::Builder::new().stack_size(crate::engine::WORKER_STACK).spawn_scoped(w, move || {
                        while let Some((_, path)) = queue.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                            if tx.send(lint_file(linter, run, cache, path, fix)).is_err() {
                                break;
                            }
                        }
                        working.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    }).expect("lint worker thread");
                }
            });
            drop(tx);
        });

        let mut results: Vec<FileResult> = Vec::new();
        for msg in rx {
            match msg {
                Msg::Error(path, e) => {
                    render::error(output, &format!("{}: {e}", display(&path, cwd)));
                    failed = true;
                }
                Msg::File(r) => {
                    files += 1;
                    bytes += r.text.len();
                    fixed_total += r.fixed;
                    for d in &r.diags {
                        if args.profile && format == Format::Jsonl && d.message_index.is_some() {
                            *fired.entry(d.rule.clone()).or_default() += 1;
                        }
                        match d.severity {
                            Severity::Error => errors += 1,
                            Severity::Warn => warnings += 1,
                            _ => other += 1,
                        }
                        fixable += usize::from(d.fix.is_some());
                    }
                    match format {
                        Format::Jsonl => {
                            let name = display(&r.path, cwd);
                            let lines = LineIndex::new(&r.text);
                            for d in &r.diags {
                                let json = JsonDiag::new(&name, &r.text, &lines, d);
                                let _ = writeln!(output.stdout, "{}", serde_json::to_string(&json).unwrap_or_default());
                            }
                            if args.fix_preview {
                                let json = JsonFixPreview { file: &name, kind: "fix-preview", text: apply_fixes(&r.text, &r.diags).0 };
                                let _ = writeln!(output.stdout, "{}", serde_json::to_string(&json).unwrap_or_default());
                            }
                        }
                        Format::Human if !r.diags.is_empty() => results.push(r),
                        Format::Human => {}
                    }
                }
            }
        }
        let _ = output.stdout.flush();
        // Human output is sorted so runs are reproducible.
        results.sort_by(|a, b| a.path.cmp(&b.path));
        for r in &results {
            render::diagnostics(output, &display(&r.path, cwd), &r.text, &r.diags);
        }
    });

    if format == Format::Human {
        if fixed_total > 0 {
            output.out(format_args!("fixed {fixed_total} problem{}", if fixed_total == 1 { "" } else { "s" }));
        }
        render::summary(output, errors, warnings, other, fixable, files);
    }
    let _ = output.stdout.flush();
    if args.profile {
        super::info::profile(output, &linter, format, &fired);
    }
    let cached = cache.as_ref().map_or(0, Cache::hits);
    if let Some(cache) = cache {
        // A run over the whole project forgets files that are gone.
        if let Err(e) = cache.save(sources.is_none() && args.paths.is_empty()) {
            render::error(output, &format!("cannot write {}: {e}", cache::FILE_NAME));
        }
    }
    if args.stats {
        let secs = started.elapsed().as_secs_f64();
        output.err(format_args!(
            "{files} files ({cached} from cache), {:.1} MiB in {:.1} ms ({:.0} MiB/s)",
            bytes as f64 / 1048576.0,
            secs * 1000.0,
            bytes as f64 / 1048576.0 / secs.max(1e-9)
        ));
        output.err(format_args!(
            "  config {:.1} ms (reading {:.1}, building {:.1})",
            (*load_time + *build_time).as_secs_f64() * 1000.0,
            load_time.as_secs_f64() * 1000.0,
            build_time.as_secs_f64() * 1000.0
        ));
        let phases: Vec<String> = build_phases.iter().map(|(name, time)| format!("{name} {:.1}", time.as_secs_f64() * 1000.0)).collect();
        output.err(format_args!("  config build phases (ms): {}", phases.join(", ")));
    }
    // A broken config still lints with what built, but never passes.
    if !*config_ok {
        2
    } else if failed || errors > 0 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Edit, Fix};

    #[test]
    fn json_preserves_every_edit_of_an_atomic_fix() {
        let source = "\u{e9} abc";
        let diagnostic = Diagnostic {
            rule: "replace".into(),
            severity: Severity::Warn,
            message: "replace".into(),
            message_index: Some(0),
            start: 3,
            end: source.len(),
            fix: Some(Fix { range: 0..source.len(), edits: vec![
                Edit { start: 0, end: 2, text: "E".into() },
                Edit { start: 4, end: 4, text: "!".into() },
            ] }),
        };
        let json = serde_json::to_value(JsonDiag::new("test", source, &LineIndex::new(source), &diagnostic)).unwrap();
        let edits = json["fix"]["edits"].as_array().expect("fix edits");
        assert_eq!(edits.len(), 2);
        let mut external = source.to_owned();
        for edit in edits.iter().rev() {
            let start = edit["start"].as_u64().unwrap() as usize;
            let end = edit["end"].as_u64().unwrap() as usize;
            external.replace_range(start..end, edit["text"].as_str().unwrap());
        }
        assert_eq!(external, "E a!bc");
    }
}
