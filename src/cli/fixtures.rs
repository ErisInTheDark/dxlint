//! On-disk projects with compact rule identities and full fixed text.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::{load_from_with, Output};
use crate::config::source::Disk;
use crate::engine::cache::{Cache, FILE_NAME};
use crate::engine::{apply_fixes, Linter};
use crate::line_index::LineIndex;

#[derive(Default, PartialEq, Eq)]
struct Actual {
    diagnostics: Vec<String>,
    fixed_diagnostics: Vec<String>,
    fixed: BTreeMap<PathBuf, String>,
    fix_preview: BTreeMap<PathBuf, String>,
    /// Each suggestion's changed lines, placeholders marked.
    suggestions: Vec<String>,
    /// Warm replay must preserve messages and atomic edits too.
    identity: Vec<Vec<u8>>,
}

fn entries(path: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let mut entries = fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?
        .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn scenarios(path: &Path, found: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    if path.join(".dx").is_file() { found.insert(path.to_path_buf()); }
    for entry in entries(path)? {
        if matches!(entry.file_name().to_str(), Some("actual" | "expected" | "edits" | ".git")) { continue; }
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() { scenarios(&entry.path(), found)?; }
    }
    Ok(())
}

fn reports(name: &str, text: &str, diagnostics: &[crate::engine::Diagnostic], local: &BTreeSet<&str>) -> Vec<String> {
    let lines = LineIndex::new(text);
    diagnostics.iter().map(|diagnostic| {
        let (line, col) = lines.line_col(text, diagnostic.start);
        let (end_line, end_col) = lines.line_col(text, diagnostic.end);
        let index = diagnostic.message_index.map_or_else(|| "error".into(), |index| index.to_string());
        let message = if local.contains(diagnostic.rule.as_str()) {
            format!(" {}", diagnostic.message.replace('\\', "\\\\").replace('\r', "\\r").replace('\n', "\\n"))
        } else { String::new() };
        format!("{name}:{}:{}-{}:{} {}#{index}{message}", line + 1, col + 1, end_line + 1, end_col + 1, diagnostic.rule)
    }).collect()
}

/// One block per suggestion: the report's identity and the suggestion's
/// title, then the lines it changes as they read afterwards, with each
/// placeholder in `«»`.
fn suggestions(name: &str, text: &str, diagnostics: &[crate::engine::Diagnostic]) -> Vec<String> {
    let lines = LineIndex::new(text);
    let mut blocks = Vec::new();
    for diagnostic in diagnostics {
        let (line, col) = lines.line_col(text, diagnostic.start);
        let (end_line, end_col) = lines.line_col(text, diagnostic.end);
        let index = diagnostic.message_index.map_or_else(|| "error".into(), |index| index.to_string());
        for suggestion in &diagnostic.suggestions {
            let fix = &suggestion.fix;
            let (Some(first), Some(last)) = (fix.edits.first(), fix.edits.last()) else { continue };
            let after = fix.apply(text);
            let from = lines.line_start(lines.line_of(first.start));
            // Text after the last edit moves by the whole fix's change in length.
            let last_end = (last.end + after.len()).saturating_sub(text.len());
            let to = after[last_end..].find('\n').map_or(after.len(), |i| last_end + i);
            let mut shown = String::new();
            let mut pos = from;
            let mut marks: Vec<_> = fix.placeholders.iter().filter(|r| r.start >= from && r.end <= to).collect();
            marks.sort_by_key(|r| r.start);
            for mark in marks {
                shown.push_str(&after[pos..mark.start]);
                shown.push('«');
                shown.push_str(&after[mark.clone()]);
                shown.push('»');
                pos = mark.end;
            }
            shown.push_str(&after[pos..to]);
            let body: Vec<String> = shown.lines().map(|line| format!("  {line}")).collect();
            blocks.push(format!("{name}:{}:{}-{}:{} {}#{index} > {}\n{}",
                line + 1, col + 1, end_line + 1, end_col + 1, diagnostic.rule, suggestion.title, body.join("\n")));
        }
    }
    blocks
}

/// Per `edits/<name>/` (edited texts of inputs, by relative path): the other
/// inputs whose last lint read something those unsaved edits break, as an
/// editor would re-lint them.
fn dirtied(linter: &Linter, root: &Path, files: &[PathBuf]) -> Result<BTreeMap<PathBuf, String>, String> {
    let edits = root.join("edits");
    let mut out = BTreeMap::new();
    if !edits.is_dir() { return Ok(out) }
    for entry in entries(&edits)? {
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() { continue }
        let mut edited = BTreeMap::new();
        fixed_outputs(&entry.path(), &entry.path(), &mut edited)?;
        let run = linter.run();
        let mut reads = Vec::new();
        for file in files.iter().filter(|file| linter.wants(file)) {
            let Ok(text) = fs::read_to_string(file) else { continue };
            reads.push((file, run.lint_tracked(file, &text).1));
        }
        drop(run);
        let paths: Vec<PathBuf> = edited.keys().map(|relative| root.join(relative)).collect();
        for (path, text) in paths.iter().zip(edited.values()) { crate::engine::reads::set_buffer(path, text) }
        let stale = linter.invalidate(&paths);
        let mut names: Vec<String> = reads.iter()
            .filter(|(file, reads)| !paths.contains(file) && linter.is_stale(&stale, reads))
            .map(|(file, _)| file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/"))
            .collect();
        for path in &paths { crate::engine::reads::drop_buffer(path) }
        linter.invalidate(&paths);
        names.sort();
        out.insert(PathBuf::from(entry.file_name()), diagnostic_text(&names));
    }
    Ok(out)
}

fn collect(linter: &Linter, files: &[PathBuf], cache: Option<&Cache>, warm: bool) -> Result<Actual, String> {
    let local: BTreeSet<_> = linter.rules.iter().filter(|rule|
        linter.sources.get(rule.message_file).path.as_deref() == Some(linter.root.join(".dx").as_path()))
        .map(|rule| rule.name.as_str()).collect();
    let mut actual = Actual::default();
    for file in files {
        let text = match String::from_utf8(fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?) {
            Ok(text) => text,
            Err(_) => continue,
        };
        if !linter.wants(file) { continue; }
        let relative = file.strip_prefix(&linter.root).map_err(|e| e.to_string())?;
        let name = relative.to_string_lossy().replace('\\', "/");
        let diagnostics = if warm {
            cache.and_then(|cache| cache.lookup(linter, &name, &text)).ok_or_else(|| format!("cache missed {name}"))?
        } else if let Some(cache) = cache {
            let (diagnostics, reads) = linter.lint_tracked(file, &text);
            cache.store(&name, &text, &diagnostics, reads.into());
            diagnostics
        } else {
            linter.lint(file, &text)
        };
        actual.identity.push(serde_json::to_vec(&diagnostics).map_err(|e| e.to_string())?);
        actual.diagnostics.extend(reports(&name, &text, &diagnostics, &local));
        actual.suggestions.extend(suggestions(&name, &text, &diagnostics));
        if diagnostics.iter().any(|diagnostic| diagnostic.fix.is_some()) {
            actual.fix_preview.insert(relative.to_path_buf(), apply_fixes(&text, &diagnostics).0);
        }
        let fixed = linter.fix(file, &text);
        let remaining = linter.lint(file, &fixed.text);
        actual.fixed_diagnostics.extend(reports(&name, &fixed.text, &remaining, &local));
        if fixed.text != text { actual.fixed.insert(relative.to_path_buf(), fixed.text); }
    }
    actual.diagnostics.sort();
    actual.fixed_diagnostics.sort();
    Ok(actual)
}

fn diagnostic_text(lines: &[String]) -> String {
    if lines.is_empty() { String::new() } else { lines.join("\n") + "\n" }
}

fn clear_owned(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() =>
            fs::remove_dir_all(path).map_err(|e| format!("{}: {e}", path.display())),
        Ok(_) => Err(format!("refusing non-directory output {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn write_outputs(root: &Path, files: &BTreeMap<PathBuf, String>) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    for (relative, text) in files {
        let file = root.join(relative);
        fs::create_dir_all(file.parent().expect("output parent")).map_err(|e| e.to_string())?;
        fs::write(file, text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_actual(root: &Path, actual: &Actual, bless: bool) -> Result<(), String> {
    let actual_dir = root.join("actual");
    clear_owned(&actual_dir)?;
    write_outputs(&actual_dir.join("fixed"), &actual.fixed)?;
    write_outputs(&actual_dir.join("fix-preview"), &actual.fix_preview)?;
    fs::write(actual_dir.join("diagnostics"), diagnostic_text(&actual.diagnostics)).map_err(|e| e.to_string())?;
    fs::write(actual_dir.join("fixed-diagnostics"), diagnostic_text(&actual.fixed_diagnostics)).map_err(|e| e.to_string())?;
    fs::write(actual_dir.join("suggestions"), diagnostic_text(&actual.suggestions)).map_err(|e| e.to_string())?;
    if bless {
        let expected = root.join("expected");
        if fs::symlink_metadata(&expected).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err("refusing symlink expected directory".into());
        }
        clear_owned(&expected.join("fixed"))?;
        write_outputs(&expected.join("fixed"), &actual.fixed)?;
        fs::copy(actual_dir.join("diagnostics"), expected.join("diagnostics")).map_err(|e| e.to_string())?;
        if expected.join("fixed-diagnostics").is_file() {
            fs::copy(actual_dir.join("fixed-diagnostics"), expected.join("fixed-diagnostics")).map_err(|e| e.to_string())?;
        }
        if !actual.suggestions.is_empty() || expected.join("suggestions").is_file() {
            fs::copy(actual_dir.join("suggestions"), expected.join("suggestions")).map_err(|e| e.to_string())?;
        }
        let preview_dir = expected.join("fix-preview");
        let opted_in = preview_dir.is_dir();
        let previews: BTreeMap<_, _> = actual.fix_preview.iter().filter(|(relative, text)|
            opted_in || actual.fixed.get(*relative) != Some(*text))
            .map(|(relative, text)| (relative.clone(), text.clone())).collect();
        clear_owned(&preview_dir)?;
        if opted_in || !previews.is_empty() {
            write_outputs(&preview_dir, &previews)?;
        }
    }
    Ok(())
}

fn fixed_outputs(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<(), String> {
    if !path.exists() { return Ok(()); }
    for entry in entries(path)? {
        let file = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() { fixed_outputs(root, &file, files)?; }
        else if kind.is_file() {
            files.insert(file.strip_prefix(root).map_err(|e| e.to_string())?.to_path_buf(),
                fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?);
        } else { return Err(format!("unsupported expected output {}", file.display())); }
    }
    Ok(())
}

/// Diagnostic lists compare line by line: line endings and trailing blank
/// lines carry no diagnostics, so they never fail a fixture.
fn same_lines(expected: &str, actual: &str) -> bool {
    let lines = |text: &str| text.trim_end_matches(['\r', '\n']).lines().map(str::to_owned).collect::<Vec<_>>();
    lines(expected) == lines(actual)
}

fn difference(expected: &str, actual: &str) -> String {
    let expected: Vec<_> = expected.lines().collect();
    let actual: Vec<_> = actual.lines().collect();
    let mut diff = String::new();
    for i in 0..expected.len().max(actual.len()) {
        if expected.get(i) == actual.get(i) { continue; }
        if let Some(line) = expected.get(i) { diff.push_str(&format!("- {line}\n")); }
        if let Some(line) = actual.get(i) { diff.push_str(&format!("+ {line}\n")); }
    }
    if diff.is_empty() { diff.push_str("line ending or final newline differs\n"); }
    diff
}

fn scenario(output: &mut Output<'_>, root: &Path, bless: bool) -> Result<Vec<String>, String> {
    let project = load_from_with(output, &root.join(".dx"), &Disk, true).map_err(|_| "configuration failed".to_string())?;
    if !project.config_ok { return Err("configuration failed".into()); }
    let outputs = |entry: &ignore::DirEntry| !matches!(entry.file_name().to_str(), Some("actual" | "expected" | "edits"));
    let mut files = super::lint::walker(&[root.to_path_buf()], 1, outputs).build()
        .map(|entry| entry.map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?.into_iter()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(|entry| entry.into_path())
        .filter(|path| path.extension().is_none_or(|ext| ext != "dx") && path.file_name().is_none_or(|name| name != FILE_NAME))
        .collect::<Vec<_>>();
    files.sort();
    let marker = root.join(FILE_NAME);
    if fs::symlink_metadata(&marker).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("refusing symlink cache marker".into());
    }
    let original_cache = marker.is_file().then(|| fs::read(&marker)).transpose().map_err(|e| e.to_string())?;
    let result = (|| {
        let cache = original_cache.as_ref().map(|_| Cache::open(&project.linter));
        let actual = collect(&project.linter, &files, cache.as_ref(), false)?;
        let replay: Result<(), String> = (|| {
            if let Some(cache) = cache {
                cache.save(true).map_err(|e| e.to_string())?;
                let warm = collect(&project.linter, &files, Some(&Cache::open(&project.linter)), true)?;
                if actual != warm { return Err("cold/warm outputs differ".into()); }
            }
            Ok(())
        })();
        write_actual(root, &actual, bless && replay.is_ok())?;
        replay?;
        let expected = root.join("expected");
        let dirtied = dirtied(&project.linter, root, &files)?;
        write_outputs(&root.join("actual/dirtied"), &dirtied)?;
        if bless && (!dirtied.is_empty() || expected.join("dirtied").is_dir()) {
            clear_owned(&expected.join("dirtied"))?;
            write_outputs(&expected.join("dirtied"), &dirtied)?;
        }
        let diagnostics = fs::read_to_string(expected.join("diagnostics")).map_err(|e| format!("expected/diagnostics: {e}"))?;
        let mut differences = Vec::new();
        let text = diagnostic_text(&actual.diagnostics);
        if !same_lines(&diagnostics, &text) { differences.push(format!("diagnostics:\n{}", difference(&diagnostics, &text))); }
        if expected.join("fixed-diagnostics").is_file() {
            let diagnostics = fs::read_to_string(expected.join("fixed-diagnostics")).map_err(|e| e.to_string())?;
            let text = diagnostic_text(&actual.fixed_diagnostics);
            if !same_lines(&diagnostics, &text) { differences.push(format!("fixed-diagnostics:\n{}", difference(&diagnostics, &text))); }
        }
        let text = diagnostic_text(&actual.suggestions);
        match fs::read_to_string(expected.join("suggestions")) {
            Ok(suggestions) if !same_lines(&suggestions, &text) => differences.push(format!("suggestions:\n{}", difference(&suggestions, &text))),
            Err(_) if !actual.suggestions.is_empty() => differences.push("suggestions: offered, but expected/suggestions is missing".into()),
            _ => {}
        }
        let fixed_root = expected.join("fixed");
        let mut fixed = BTreeMap::new();
        fixed_outputs(&fixed_root, &fixed_root, &mut fixed)?;
        for relative in fixed.keys().chain(actual.fixed.keys()).collect::<BTreeSet<_>>() {
            match (fixed.get(relative), actual.fixed.get(relative)) {
                (Some(expected), Some(actual)) if expected != actual =>
                    differences.push(format!("fixed/{}:\n{}", relative.display(), difference(expected, actual))),
                (Some(_), None) => differences.push(format!("fixed/{}: expected change, input unchanged", relative.display())),
                (None, Some(_)) => differences.push(format!("fixed/{}: missing expected change", relative.display())),
                _ => {}
            }
        }
        let dirtied_root = expected.join("dirtied");
        let mut expected_dirtied = BTreeMap::new();
        fixed_outputs(&dirtied_root, &dirtied_root, &mut expected_dirtied)?;
        for name in expected_dirtied.keys().chain(dirtied.keys()).collect::<BTreeSet<_>>() {
            let (wanted, got) = (expected_dirtied.get(name).map_or("", String::as_str), dirtied.get(name).map_or("", String::as_str));
            if !dirtied.contains_key(name) {
                differences.push(format!("dirtied/{}: no edits/{0}", name.display()));
            } else if !expected_dirtied.contains_key(name) {
                differences.push(format!("dirtied/{}: missing expected", name.display()));
            } else if !same_lines(wanted, got) {
                differences.push(format!("dirtied/{}:\n{}", name.display(), difference(wanted, got)));
            }
        }
        let preview_root = expected.join("fix-preview");
        let mut previews = BTreeMap::new();
        fixed_outputs(&preview_root, &preview_root, &mut previews)?;
        for (relative, expected) in previews {
            match actual.fix_preview.get(&relative) {
                Some(actual) if expected != *actual =>
                    differences.push(format!("fix-preview/{}:\n{}", relative.display(), difference(&expected, actual))),
                None => differences.push(format!("fix-preview/{}: expected fix, input has no fixes", relative.display())),
                _ => {}
            }
        }
        Ok(differences)
    })();
    if let Some(bytes) = original_cache { fs::write(marker, bytes).map_err(|e| format!("restore cache marker: {e}"))?; }
    result
}

/// One scenario's verdict and everything it printed.
struct Checked {
    passed: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn check(cwd: &Path, root: &Path, bless: bool) -> Checked {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut output = Output { cwd, stdout: &mut stdout, stderr: &mut stderr };
    let name = root.strip_prefix(cwd).unwrap_or(root).display();
    let passed = match scenario(&mut output, root, bless) {
        Ok(differences) if differences.is_empty() => {
            output.out(format_args!("ok {name}"));
            true
        }
        result => {
            let differences = match result { Ok(differences) => differences, Err(error) => vec![error] };
            output.err(format_args!("FAIL {name}\n{}", differences.join("\n")));
            false
        }
    };
    Checked { passed, stdout, stderr }
}

/// Scenarios run on `jobs` threads (each keeps its own library parse) and
/// report in path order.
pub(super) fn run(output: &mut Output<'_>, paths: &[PathBuf], bless: bool, jobs: usize) -> u8 {
    let paths = if paths.is_empty() { vec![PathBuf::from("fixtures")] } else { paths.to_vec() };
    let mut found = BTreeSet::new();
    let mut failures = 0;
    for path in paths {
        if let Err(error) = scenarios(&output.cwd.join(path), &mut found) {
            output.err(format_args!("{error}"));
            failures += 1;
        }
    }
    if found.is_empty() { output.err(format_args!("no fixture scenarios found")); return 1; }
    let roots: Vec<PathBuf> = found.into_iter().collect();
    let cwd = output.cwd;
    let (tx, rx) = std::sync::mpsc::channel();
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..jobs.clamp(1, roots.len()) {
            let (tx, roots, next) = (tx.clone(), &roots, &next);
            std::thread::Builder::new().stack_size(crate::engine::WORKER_STACK).spawn_scoped(s, move || loop {
                let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(root) = roots.get(index) else { break };
                if tx.send((index, check(cwd, root, bless))).is_err() { break; }
            }).expect("fixture worker thread");
        }
        drop(tx);
        for checked in super::lint::in_order(rx) {
            failures += usize::from(!checked.passed);
            let _ = output.stdout.write_all(&checked.stdout);
            let _ = output.stderr.write_all(&checked.stderr);
        }
    });
    output.out(format_args!("{} scenarios, {failures} failed", roots.len()));
    u8::from(failures > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Project(PathBuf);

    impl Project {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("dxlint-fixtures-{name}-{}", std::process::id()));
            fs::create_dir(&root).expect("fresh test project");
            Self(root)
        }

        fn scenario(&self, name: &str) -> PathBuf {
            let root = self.0.join(name);
            fs::create_dir_all(root.join("expected")).unwrap();
            fs::write(root.join(".dx"), "lang t \"*.t\"\n").unwrap();
            fs::write(root.join("input.t"), "sample").unwrap();
            fs::write(root.join("expected/diagnostics"), "").unwrap();
            root
        }
    }

    impl Drop for Project {
        fn drop(&mut self) { fs::remove_dir_all(&self.0).expect("remove test project"); }
    }

    #[test]
    fn bless_and_replay_preserve_inputs_and_remove_stale_outputs() {
        let project = Project::new("bless");
        fs::write(project.0.join(".dx"), r#"
            lang t "*.t"
            in .t match word /[a-z]+/ {
                rule probe {
                    warn if text == "a" then "{text}\nvalue" else text
                    fix if text == "a" as "b"
                }
            }
        "#).unwrap();
        fs::write(project.0.join("input.t"), "a").unwrap();
        fs::write(project.0.join(FILE_NAME), "cache marker").unwrap();
        fs::create_dir(project.0.join("expected")).unwrap();
        fs::write(project.0.join("expected/fixed-diagnostics"), "").unwrap();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut output = Output { cwd: &project.0, stdout: &mut stdout, stderr: &mut stderr };
        assert!(scenario(&mut output, &project.0, true).unwrap().is_empty());
        assert_eq!(fs::read_to_string(project.0.join("input.t")).unwrap(), "a");
        assert_eq!(fs::read_to_string(project.0.join(FILE_NAME)).unwrap(), "cache marker");
        assert_eq!(fs::read_to_string(project.0.join("expected/fixed/input.t")).unwrap(), "b");
        assert_eq!(fs::read_to_string(project.0.join("actual/fix-preview/input.t")).unwrap(), "b");
        assert!(!project.0.join("expected/fix-preview").exists(), "bless added redundant preview expectations");
        let diagnostics = fs::read_to_string(project.0.join("expected/diagnostics")).unwrap();
        assert!(diagnostics.ends_with("probe#0 a\\nvalue\n"));
        assert!(fs::read_to_string(project.0.join("expected/fixed-diagnostics")).unwrap().ends_with("probe#1 b\n"));
        fs::write(project.0.join("actual/fixed/stale.t"), "stale").unwrap();
        assert!(scenario(&mut output, &project.0, false).unwrap().is_empty());
        assert!(!project.0.join("actual/fixed/stale.t").exists());
        fs::create_dir(project.0.join("expected/fix-preview")).unwrap();
        fs::write(project.0.join("expected/fix-preview/input.t"), "wrong").unwrap();
        assert!(!scenario(&mut output, &project.0, false).unwrap().is_empty(), "preview mismatch was ignored");
        fs::write(project.0.join("expected/fix-preview/stale.t"), "stale").unwrap();
        assert!(scenario(&mut output, &project.0, true).unwrap().is_empty());
        assert_eq!(fs::read_to_string(project.0.join("expected/fix-preview/input.t")).unwrap(), "b");
        assert!(!project.0.join("expected/fix-preview/stale.t").exists());
        fs::write(project.0.join("input.t"), "c").unwrap();
        assert!(scenario(&mut output, &project.0, true).unwrap().is_empty());
        assert!(!project.0.join("actual/fixed/input.t").exists());
        assert!(!project.0.join("expected/fixed/input.t").exists());
        assert!(!project.0.join("actual/fix-preview/input.t").exists());
        assert!(!project.0.join("expected/fix-preview/input.t").exists());
        assert_eq!(fs::read_to_string(project.0.join(FILE_NAME)).unwrap(), "cache marker");
        fs::remove_dir(project.0.join("expected/fix-preview")).unwrap();
        let actual = Actual {
            fixed: BTreeMap::from([(PathBuf::from("input.t"), "c".into()), (PathBuf::from("same.t"), "z".into())]),
            fix_preview: BTreeMap::from([(PathBuf::from("input.t"), "b".into()), (PathBuf::from("same.t"), "z".into())]),
            ..Actual::default()
        };
        write_actual(&project.0, &actual, true).unwrap();
        assert_eq!(fs::read_to_string(project.0.join("expected/fix-preview/input.t")).unwrap(), "b");
        assert!(!project.0.join("expected/fix-preview/same.t").exists(), "bless added an unrelated redundant preview");
    }

    #[test]
    fn discovery_deduplicates_scenarios_and_ignores_outputs() {
        let project = Project::new("discovery");
        let first = project.scenario("fixtures/alpha");
        let last = project.scenario("fixtures/zeta");
        for ignored in ["actual", "expected", ".git"] {
            project.scenario(&format!("fixtures/{ignored}/hidden"));
        }
        let mut found = BTreeSet::new();
        scenarios(&project.0.join("fixtures"), &mut found).unwrap();
        scenarios(&first, &mut found).unwrap();
        assert_eq!(found, BTreeSet::from([first, last]));
    }

    #[test]
    fn runner_selects_paths_and_keeps_running_after_failures() {
        let project = Project::new("runner");
        let bad = project.scenario("fixtures/bad");
        let good = project.scenario("fixtures/good");
        fs::remove_file(bad.join("expected/diagnostics")).unwrap();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut output = Output { cwd: &project.0, stdout: &mut stdout, stderr: &mut stderr };
        assert_eq!(run(&mut output, &[], false, 2), 1);
        assert!(good.join("actual/diagnostics").is_file());
        fs::write(bad.join("expected/diagnostics"), "").unwrap();
        assert_eq!(run(&mut output, &[], false, 1), 0);
        fs::remove_file(good.join("expected/diagnostics")).unwrap();
        assert_eq!(run(&mut output, &[PathBuf::from("fixtures/bad")], false, 1), 0);
        assert_eq!(run(&mut output, &[], false, 2), 1);
        fs::create_dir(project.0.join("empty")).unwrap();
        assert_eq!(run(&mut output, &[PathBuf::from("empty")], false, 1), 1);
    }
}
