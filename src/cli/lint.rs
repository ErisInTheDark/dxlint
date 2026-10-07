//! `zlint [paths]`: walk, lint (and fix) in parallel, report.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Instant;

use ignore::{WalkBuilder, WalkState};
use serde::Serialize;

use super::{load_for_lint, render, ConfigArg, Format, LintArgs};
use zoomie_lint::config::ast::Severity;
use zoomie_lint::engine::cache::{self, Cache};
use zoomie_lint::engine::{Diagnostic, Edit, Linter};
use zoomie_lint::line_index::LineIndex;

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
    line: usize,
    col: usize,
    end_line: usize,
    end_col: usize,
    fixable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<JsonFix<'a>>,
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
            line: line + 1,
            col: col + 1,
            end_line: end_line + 1,
            end_col: end_col + 1,
            fixable: diagnostic.fix.is_some(),
            fix: diagnostic.fix.as_deref().map(|edits| JsonFix { edits }),
        }
    }
}

fn display(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn lint_file(linter: &Linter, cache: Option<&Cache>, path: &Path, fix: bool) -> Msg {
    let text = match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => return Msg::File(FileResult { path: path.to_path_buf(), text: String::new(), diags: Vec::new(), fixed: 0 }),
        },
        Err(e) => return Msg::Error(path.to_path_buf(), e.to_string()),
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
                let (diags, reads) = linter.lint_tracked(path, &text);
                cache.store(&rel, &text, &diags, reads.into());
                diags
            }
            None => linter.lint(path, &text),
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

pub fn run(args: LintArgs, config: &ConfigArg) -> ExitCode {
    let started = Instant::now();
    let (mut linter, config_ok, load_time, build_time) = match load_for_lint(config) {
        Ok(p) => (p.linter, p.config_ok, p.load_time, p.build_time),
        Err(code) => return code,
    };
    if args.profile {
        linter.enable_profile();
    }
    // Profiling measures linting, so it never reads results instead.
    let cache = (!args.no_cache && !args.profile).then(|| Cache::open(&linter));
    let cwd = std::env::current_dir().unwrap_or_default();
    let roots: Vec<PathBuf> = if args.paths.is_empty() { vec![linter.root.clone()] } else { args.paths.iter().map(|p| cwd.join(p)).collect() };
    let jobs = args.jobs.map_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()), |n| n.get());

    let mut walker = WalkBuilder::new(&roots[0]);
    for r in &roots[1..] {
        walker.add(r);
    }
    walker.hidden(false).threads(jobs).filter_entry(|e| e.file_name() != ".git");

    let (tx, rx) = mpsc::channel::<Msg>();
    let fix = args.fix;
    let format = args.format;
    let mut errors = 0;
    let mut warnings = 0;
    let mut other = 0;
    let mut fixable = 0;
    let mut fixed_total = 0;
    let mut files = 0usize;
    let mut bytes = 0usize;
    let mut failed = false;

    std::thread::scope(|s| {
        let linter = &mut linter;
        let cache = cache.as_ref();
        let explicit: Vec<PathBuf> = roots.iter().filter(|r| r.is_file()).cloned().collect();
        s.spawn(move || {
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
            let mut queue = found.into_inner().expect("file list");
            queue.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let next = std::sync::atomic::AtomicUsize::new(0);
            let threads = jobs.min(queue.len().max(1));
            // File and rule workers share one budget, rather than multiplying it.
            linter.set_rule_jobs(jobs / threads);
            let linter = &*linter;
            std::thread::scope(|w| {
                for _ in 0..threads {
                    let tx = tx.clone();
                    let (queue, next) = (&queue, &next);
                    w.spawn(move || {
                        while let Some((_, path)) = queue.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                            if tx.send(lint_file(linter, cache, path, fix)).is_err() {
                                return;
                            }
                        }
                    });
                }
            });
            drop(tx);
        });

        let mut results: Vec<FileResult> = Vec::new();
        // Stdout is line-buffered: one write system call per diagnostic
        // otherwise, which dominated kernel time on large outputs.
        let mut stdout = std::io::BufWriter::with_capacity(1 << 16, std::io::stdout().lock());
        for msg in rx {
            match msg {
                Msg::Error(path, e) => {
                    render::error(&format!("{}: {e}", display(&path, &cwd)));
                    failed = true;
                }
                Msg::File(r) => {
                    files += 1;
                    bytes += r.text.len();
                    fixed_total += r.fixed;
                    for d in &r.diags {
                        match d.severity {
                            Severity::Error => errors += 1,
                            Severity::Warn => warnings += 1,
                            _ => other += 1,
                        }
                        fixable += usize::from(d.fix.is_some());
                    }
                    match format {
                        Format::Jsonl => {
                            let name = display(&r.path, &cwd);
                            let lines = LineIndex::new(&r.text);
                            for d in &r.diags {
                                let json = JsonDiag::new(&name, &r.text, &lines, d);
                                let _ = writeln!(stdout, "{}", serde_json::to_string(&json).unwrap_or_default());
                            }
                        }
                        Format::Human if !r.diags.is_empty() => results.push(r),
                        Format::Human => {}
                    }
                }
            }
        }
        let _ = stdout.flush();
        drop(stdout);
        // Human output is sorted so runs are reproducible.
        results.sort_by(|a, b| a.path.cmp(&b.path));
        for r in &results {
            render::diagnostics(&display(&r.path, &cwd), &r.text, &r.diags);
        }
    });

    if format == Format::Human {
        if fixed_total > 0 {
            println!("fixed {fixed_total} problem{}", if fixed_total == 1 { "" } else { "s" });
        }
        render::summary(errors, warnings, other, fixable, files);
    }
    if args.profile {
        super::info::profile(&linter);
    }
    let cached = cache.as_ref().map_or(0, Cache::hits);
    if let Some(cache) = cache {
        // A run over the whole project forgets files that are gone.
        if let Err(e) = cache.save(args.paths.is_empty()) {
            render::error(&format!("cannot write {}: {e}", cache::FILE_NAME));
        }
    }
    if args.stats {
        let secs = started.elapsed().as_secs_f64();
        eprintln!(
            "{files} files ({cached} from cache), {:.1} MiB in {:.1} ms ({:.0} MiB/s)",
            bytes as f64 / 1048576.0,
            secs * 1000.0,
            bytes as f64 / 1048576.0 / secs.max(1e-9)
        );
        eprintln!(
            "  config {:.1} ms (reading {:.1}, building {:.1})",
            (load_time + build_time).as_secs_f64() * 1000.0,
            load_time.as_secs_f64() * 1000.0,
            build_time.as_secs_f64() * 1000.0
        );
    }
    // A broken config still lints with what built, but never passes.
    if !config_ok {
        ExitCode::from(2)
    } else if failed || errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoomie_lint::engine::Edit;

    #[test]
    fn json_preserves_every_edit_of_an_atomic_fix() {
        let source = "\u{e9} abc";
        let diagnostic = Diagnostic {
            rule: "replace".into(),
            severity: Severity::Warn,
            message: "replace".into(),
            start: 3,
            end: source.len(),
            fix: Some(vec![
                Edit { start: 0, end: 2, text: "E".into() },
                Edit { start: 4, end: 4, text: "!".into() },
            ]),
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
