//! `dxlint [paths]`: walk, lint (and fix) in parallel, report.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use ignore::{WalkBuilder, WalkState};
use serde::Serialize;

use super::{load_for_lint, render, ConfigArg, Format, LintArgs, Output, Project, Sources};
use crate::config::ast::Severity;
use crate::config::source::normalize_path;
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

/// `DXLINT_PROFILE_EVERY=<seconds>` with `--profile`: print the busiest
/// semantic queries while the run is still going (for runs that hang).
fn watch_every() -> Option<std::time::Duration> {
    let seconds: u64 = std::env::var("DXLINT_PROFILE_EVERY").ok()?.parse().ok()?;
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

/// Items tagged with their place in a sequence, released in that order as
/// soon as each one's predecessors are.
pub(super) fn in_order<T>(tagged: impl IntoIterator<Item = (usize, T)>) -> impl Iterator<Item = T> {
    let mut tagged = tagged.into_iter();
    let mut waiting = std::collections::BTreeMap::new();
    let mut due = 0;
    std::iter::from_fn(move || loop {
        if let Some(item) = waiting.remove(&due) {
            due += 1;
            return Some(item);
        }
        let (index, item) = tagged.next()?;
        waiting.insert(index, item);
    })
}

/// Walks `roots`, leaving out `.git` and every entry `keep` refuses.
pub(super) fn walker(roots: &[PathBuf], jobs: usize, keep: impl Fn(&ignore::DirEntry) -> bool + Send + Sync + 'static) -> WalkBuilder {
    let mut walker = WalkBuilder::new(&roots[0]);
    for root in &roots[1..] { walker.add(root); }
    walker.hidden(false).threads(jobs).filter_entry(move |entry| entry.file_name() != ".git" && keep(entry));
    walker
}

fn lint_file(linter: &Linter, run: &LintRun<'_>, cache: Option<&Cache>, path: &Path, fix: bool) -> Msg {
    // Another Workbench thread's file: reported, never rewritten.
    let fix = fix && !super::workbench::holds(path);
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
    if let Some(diags) = cache.and_then(|c| c.lookup(linter, &rel, &text)) {
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

/// What a run found, summed over every config it linted with.
struct Tally {
    errors: usize,
    warnings: usize,
    other: usize,
    fixable: usize,
    fixed: usize,
    files: usize,
    failed: bool,
    config_ok: bool,
}

impl Tally {
    fn new() -> Self {
        Tally { errors: 0, warnings: 0, other: 0, fixable: 0, fixed: 0, files: 0, failed: false, config_ok: true }
    }

    /// The summary line, and the exit code: a broken config still lints
    /// with what built, but never passes.
    fn finish(&self, output: &mut Output<'_>, format: Format) -> u8 {
        if format == Format::Human {
            if self.fixed > 0 {
                output.out(format_args!("fixed {} problem{}", self.fixed, if self.fixed == 1 { "" } else { "s" }));
            }
            render::summary(output, self.errors, self.warnings, self.other, self.fixable, self.files);
        }
        let _ = output.stdout.flush();
        if !self.config_ok {
            2
        } else if self.failed || self.errors > 0 {
            1
        } else {
            0
        }
    }
}

/// Which files one config lints.
struct Scope<'a> {
    roots: Vec<PathBuf>,
    /// Every path named on the command line: nested projects inside them
    /// are linted by their own config, so skipping them says nothing.
    named: &'a [PathBuf],
    /// The whole project, so the cache forgets files that are gone.
    whole: bool,
    /// The files under `roots`, walked before the config was built.
    walked: Option<Walked>,
}

/// Each named path is linted with its nearest `.dx` (or `--config`), so a
/// file gets the same config as in the editor. `None` roots: the whole
/// project of the current directory's config.
fn configs(output: &mut Output<'_>, paths: &[PathBuf], arg: &ConfigArg) -> Result<Vec<(PathBuf, Option<Vec<PathBuf>>)>, u8> {
    let explicit = arg.config.as_ref().map(|p| normalize_path(&output.cwd.join(p)));
    if paths.is_empty() {
        return match explicit.or_else(|| crate::config::find(output.cwd)) {
            Some(config) => Ok(vec![(config, None)]),
            None => {
                render::error(output, &crate::config::not_found_message());
                Err(2)
            }
        };
    }
    let mut by_config: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> = std::collections::BTreeMap::new();
    for path in paths {
        let full = normalize_path(&output.cwd.join(path));
        let dir = if full.is_dir() { full.as_path() } else { full.parent().unwrap_or(&full) };
        let Some(config) = explicit.clone().or_else(|| crate::config::find(dir)) else {
            render::error(output, &format!("{}: {}", path.display(), crate::config::not_found_message()));
            return Err(2);
        };
        by_config.entry(config).or_default().push(full);
    }
    Ok(by_config.into_iter().map(|(config, roots)| (config, Some(roots))).collect())
}

fn lint(output: &mut Output<'_>, args: LintArgs, config: &ConfigArg, sources: Option<&Sources>) -> u8 {
    let started = Instant::now();
    if sources.is_some() {
        crate::config::build_one_config();
        let mut project = match load_for_lint(output, config) {
            Ok(project) => project,
            Err(code) => return code,
        };
        return prepared(output, args, &mut project, sources, started);
    }
    let configs = match configs(output, &args.paths, config) {
        Ok(configs) => configs,
        Err(code) => return code,
    };
    if configs.len() == 1 {
        crate::config::build_one_config();
    }
    let named: Vec<PathBuf> = args.paths.iter().map(|p| normalize_path(&output.cwd.join(p))).collect();
    let jobs = args.jobs.map_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()), |n| n.get());
    // In a Workbench agent's shell, read the arc view while configs load.
    let mut view = (super::workbench::active() && !args.no_workbench_view).then(|| super::workbench::start(output.cwd.to_path_buf()));
    let mut tally = Tally::new();
    for (path, roots) in configs {
        let whole = roots.is_none();
        let roots = roots.unwrap_or_else(|| vec![path.parent().map(Path::to_path_buf).unwrap_or_default()]);
        // The walk comes between reading and building the config: it skips
        // what the config ignores, and the nested configs it finds may fold
        // into this one, which is then built once with them. `--config` is
        // that one config alone.
        let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut walked = None;
        let nested = |ignores: &crate::path_globs::PathGlobs| {
            let found = walk(&roots, jobs, &base, ignores);
            let nested = if config.config.is_some() { Vec::new() } else { found.nested.clone() };
            walked = Some(found);
            nested
        };
        let loaded = super::load_tree_with(output, &path, nested, &crate::config::source::Disk, true);
        if let Some(handle) = view.take() {
            match handle.join().unwrap_or_else(|_| Err("reading it panicked".into())) {
                Ok(found) => super::workbench::apply(&found),
                Err(error) => render::error(output, &format!("workbench view not applied ({error}); linting the files on disk")),
            }
        }
        // A config file another thread holds: read it again as the view has it.
        let config_held = loaded.as_ref().is_ok_and(|(project, _)| project.linter.sources.files()
            .any(|(_, file)| file.path.as_deref().is_some_and(super::workbench::holds)));
        let loaded = if config_held {
            let nested = walked.as_ref().filter(|_| config.config.is_none()).map(|w| w.nested.clone()).unwrap_or_default();
            super::load_tree_with(output, &path, |_| nested, &crate::config::source::Disk, true)
        } else {
            loaded
        };
        let (mut project, folded) = match loaded {
            Ok(project) => project,
            Err(_) => {
                tally.config_ok = false;
                continue;
            }
        };
        let walked = walked.map(|walked| Walked { folded, ..walked });
        lint_project(output, &args, &mut project, None, Scope { roots, named: &named, whole, walked }, started, &mut tally);
    }
    tally.finish(output, args.format)
}

/// What a walk found before any config was built: every file (with its
/// size) and the nested configs, and after loading, which of them folded.
struct Walked {
    files: Vec<(u64, PathBuf)>,
    nested: Vec<PathBuf>,
    folded: Vec<PathBuf>,
}

/// Every file under `roots`, minus folders the config at `base` ignores
/// (named roots are walked whatever it ignores).
fn walk(roots: &[PathBuf], jobs: usize, base: &Path, ignores: &crate::path_globs::PathGlobs) -> Walked {
    let found: std::sync::Mutex<Vec<(u64, PathBuf)>> = std::sync::Mutex::default();
    let (base, ignores) = (base.to_path_buf(), ignores.clone());
    walker(roots, jobs, move |entry| {
        entry.depth() == 0 || !entry.file_type().is_some_and(|t| t.is_dir())
            || entry.path().strip_prefix(&base).map_or(true, |rel| !ignores.matches(rel))
    }).build_parallel().run(|| {
        let found = &found;
        Box::new(move |entry| {
            let Ok(entry) = entry else { return WalkState::Continue };
            if entry.file_type().is_some_and(|t| t.is_file()) {
                let size = entry.metadata().map_or(0, |m| m.len());
                found.lock().expect("file list").push((size, entry.path().to_path_buf()));
            }
            WalkState::Continue
        })
    });
    // Named roots may lie inside each other (`dxlint . sub`): each file once.
    let mut files = found.into_inner().expect("file list");
    files.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    files.dedup_by(|a, b| a.1 == b.1);
    let mut nested: Vec<PathBuf> = files.iter().map(|(_, path)| path)
        .filter(|path| path.file_name().is_some_and(|n| n == crate::config::CONFIG_NAME) && path.parent().is_some_and(|dir| !roots.iter().any(|r| r == dir)))
        .map(|path| normalize_path(path))
        .collect();
    nested.sort();
    Walked { files, nested, folded: Vec::new() }
}

/// One built project over its own root (conformance's packed sources).
pub(super) fn prepared(output: &mut Output<'_>, args: LintArgs, project: &mut Project, sources: Option<&Sources>, started: Instant) -> u8 {
    let mut tally = Tally::new();
    let roots = vec![project.linter.root.clone()];
    lint_project(output, &args, project, sources, Scope { roots, named: &[], whole: true, walked: None }, started, &mut tally);
    tally.finish(output, args.format)
}

/// The folders a walk skipped for their own `.dx` that nobody named and the
/// config does not ignore.
fn skipped_note(output: &mut Output<'_>, linter: &Linter, skipped: Vec<PathBuf>, named: &[PathBuf]) {
    let mut shown: Vec<String> = skipped.into_iter()
        .filter(|dir| !linter.is_ignored(dir) && !named.iter().any(|path| path.starts_with(dir)))
        .map(|dir| display(&dir, output.cwd))
        .collect();
    if shown.is_empty() {
        return;
    }
    shown.sort();
    let (has, them) = if shown.len() == 1 { ("has", "it") } else { ("each has", "them") };
    render::note(output, &format!("skipped {} ({has} its own .dx; name {them} to lint {them} too)", shown.join(", ")));
}

fn lint_project(output: &mut Output<'_>, args: &LintArgs, project: &mut Project, sources: Option<&Sources>, scope: Scope<'_>, started: Instant, tally: &mut Tally) {
    let Project { linter, config_ok, load_time, lex_time, build_time, build_phases, .. } = project;
    tally.config_ok &= *config_ok;
    if args.profile {
        linter.enable_profile();
    }
    if let Some(units) = args.semantic_budget {
        crate::grammar::names::question::Budget::set_per_question(units.get());
    }
    // Profiling measures linting, so it never reads results instead; another
    // budget gives other results than cached ones.
    let cache = (!args.no_cache && !args.profile && !args.fix_preview && args.semantic_budget.is_none()).then(|| Cache::open(&linter));
    let cwd = output.cwd;
    let Scope { roots, named, whole, walked } = scope;
    let jobs = args.jobs.map_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()), |n| n.get());

    let packed = sources.map(|sources| sources.files.iter().filter(|(_, path)| linter.wants(path)).cloned().collect::<Vec<_>>());

    // Workers send each file's result with its place in the queue; results
    // are reported in queue order, so output never depends on timing.
    let (tx, rx) = mpsc::channel::<(usize, Msg)>();
    let fix = args.fix;
    let format = args.format;
    let mut fired = crate::fx_hash::FxHashMap::<String, u64>::default();
    let mut files = 0usize;
    let mut bytes = 0usize;
    let skipped = std::sync::Arc::new(std::sync::Mutex::new(Vec::<PathBuf>::new()));

    std::thread::scope(|s| {
        let linter = &mut *linter;
        let cache = cache.as_ref();
        let explicit: Vec<PathBuf> = roots.iter().filter(|r| r.is_file()).cloned().collect();
        let skipping = skipped.clone();
        s.spawn(move || {
            let mut queue = if let Some(queue) = packed {
                queue
            } else {
                let Walked { files, nested, folded } = walked.unwrap_or_else(|| walk(&roots, jobs, &linter.root, &crate::path_globs::PathGlobs::default()));
                // A folder with its own `.dx` below a root is another project,
                // unless that config folded into this one.
                let apart: Vec<&Path> = nested.iter().filter(|c| !folded.contains(c)).filter_map(|c| c.parent()).collect();
                let mut outermost: Vec<PathBuf> = apart.iter().filter(|dir| !apart.iter().any(|other| other != *dir && dir.starts_with(other))).map(|dir| dir.to_path_buf()).collect();
                skipping.lock().expect("skipped folders").append(&mut outermost);
                // Lint the biggest files first: a big file found late would
                // otherwise be the tail every other thread waits on.
                files.into_iter().filter(|(_, path)| {
                    // Explicitly named files are linted even if ignored by git;
                    // files a Workbench view lacks are not there at all.
                    !apart.iter().any(|dir| path.starts_with(dir))
                        && !matches!(reads::overlaid(&normalize_path(path)), Some(None))
                        && (linter.wants(path) || (explicit.iter().any(|e| e == path) && !linter.is_ignored(path)))
                }).collect()
            };
            queue.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let next = std::sync::atomic::AtomicUsize::new(0);
            let threads = jobs.min(queue.len().max(1));
            // File and rule workers share one budget, rather than multiplying it.
            linter.set_rule_jobs(jobs / threads);
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
                        let _pool = crate::alloc::own_pool();
                        loop {
                            let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some((_, path)) = queue.get(index) else { break };
                            if tx.send((index, lint_file(linter, run, cache, path, fix))).is_err() {
                                break;
                            }
                            crate::alloc::trim();
                        }
                        working.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    }).expect("lint worker thread");
                }
            });
            drop(tx);
        });

        let mut results: Vec<FileResult> = Vec::new();
        for msg in in_order(rx) {
            match msg {
                Msg::Error(path, e) => {
                    render::error(output, &format!("{}: {e}", display(&path, cwd)));
                    tally.failed = true;
                }
                Msg::File(r) => {
                    files += 1;
                    bytes += r.text.len();
                    tally.fixed += r.fixed;
                    for d in &r.diags {
                        if args.profile && format == Format::Jsonl && d.message_index.is_some() {
                            *fired.entry(d.rule.clone()).or_default() += 1;
                        }
                        match d.severity {
                            Severity::Error => tally.errors += 1,
                            Severity::Warn => tally.warnings += 1,
                            _ => tally.other += 1,
                        }
                        tally.fixable += usize::from(d.fix.is_some());
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

    tally.files += files;
    let skipped = std::mem::take(&mut *skipped.lock().expect("skipped folders"));
    skipped_note(output, linter, skipped, named);
    if args.profile {
        super::info::profile(output, linter, format, &fired);
    }
    let cached = cache.as_ref().map_or(0, Cache::hits);
    if let Some(cache) = cache {
        if let Err(e) = cache.save(whole && sources.is_none()) {
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
        output.err(format_args!("  config reading: lexing {:.1} ms", lex_time.as_secs_f64() * 1000.0));
        let phases: Vec<String> = build_phases.iter().map(|(name, time)| format!("{name} {:.1}", time.as_secs_f64() * 1000.0)).collect();
        output.err(format_args!("  config build phases (ms): {}", phases.join(", ")));
        if !linter.scopes.is_empty() {
            output.err(format_args!("  files blocks: {} rule copies for their option values", linter.scopes.copies()));
        }
        if let Some(summary) = crate::alloc::heap::summary() {
            output.err(format_args!("  {summary}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Edit, Fix};

    #[test]
    fn results_report_in_queue_order_whatever_order_they_finish_in() {
        let finished = [(2, "c"), (0, "a"), (3, "d"), (1, "b")];
        assert_eq!(in_order(finished).collect::<Vec<_>>(), ["a", "b", "c", "d"]);
    }

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
