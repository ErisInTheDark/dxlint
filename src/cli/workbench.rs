//! Agents in Workbench lint their arc view: files other live threads hold
//! read as the view has them, the rest from disk. `wb git arc held` names the
//! held paths and a baseline tree with their bytes; git reads them. Nothing
//! else in dxlint knows about Workbench: the view becomes read overlays.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{OnceLock, RwLock};

use crate::config::source::normalize_path;
use crate::engine::reads;

/// Whether this process runs in a Workbench agent's shell. Never in tests:
/// their results must not depend on the shell running them.
pub fn active() -> bool {
    !cfg!(test) && std::env::var_os("WORKBENCH_THREAD_ID").is_some_and(|id| !id.is_empty())
}

/// Held files as they read in the view: their text, or absent there.
#[derive(Debug, Default, PartialEq)]
pub struct View {
    pub texts: Vec<(PathBuf, String)>,
    pub absent: Vec<PathBuf>,
}

fn held() -> &'static RwLock<HashSet<PathBuf>> {
    static HELD: OnceLock<RwLock<HashSet<PathBuf>>> = OnceLock::new();
    HELD.get_or_init(Default::default)
}

/// Whether `path` is a file another thread holds: `--fix` leaves it alone.
pub fn holds(path: &Path) -> bool {
    let held = held().read().expect("held files");
    !held.is_empty() && held.contains(&normalize_path(path))
}

/// Reads the view for the repository holding `dir` on another thread.
pub fn start(dir: PathBuf) -> std::thread::JoinHandle<Result<View, String>> {
    std::thread::spawn(move || read(&dir))
}

/// From now on, held files read as the view has them.
pub fn apply(view: &View) {
    for (path, text) in &view.texts {
        reads::set_buffer(path, text);
    }
    for path in &view.absent {
        reads::hide_file(path);
    }
    held().write().expect("held files").extend(view.texts.iter().map(|(path, _)| path.clone()).chain(view.absent.iter().cloned()));
}

fn read(dir: &Path) -> Result<View, String> {
    let held = run(Command::new("wb").args(["git", "arc", "held"]).current_dir(dir)
        // A tool calling wb, as Workbench asks scripts to say.
        .env("THIS_IS_A_MANAGED_SCRIPT", "1"), None);
    let json = match held {
        Ok(json) => json,
        // A repository Workbench does not manage has no view: the disk is it.
        Err(error) if rejection(&error).as_deref() == Some("unmanagedRepository") => return Ok(View::default()),
        Err(error) => return Err(error),
    };
    let Some((repo, baseline, held)) = repository(&json, dir)? else { return Ok(View::default()) };
    if held.is_empty() {
        return Ok(View::default());
    }
    // Held folders hold every file below them.
    let listing = run(Command::new("git").arg("-C").arg(&repo).args(["ls-tree", "-r", "-z", "--name-only", &baseline, "--"]).args(&held), None)?;
    let files: Vec<&str> = listing.split('\0').filter(|path| !path.is_empty()).collect();
    let requests: String = files.iter().map(|path| format!("{baseline}:{path}\n")).collect();
    // `--filters`: the bytes a checkout would write (line endings included).
    let contents = run_bytes(Command::new("git").arg("-C").arg(&repo).args(["cat-file", "--batch", "--filters"]), Some(requests))?;
    let texts = files.iter().zip(batch_texts(&contents)?).map(|(path, text)| (normalize_path(&repo.join(path)), text)).collect::<Vec<_>>();
    let in_view: HashSet<&Path> = texts.iter().map(|(path, _)| path.as_path()).collect();
    let absent = held.iter().flat_map(|path| disk_files(&repo.join(path))).filter(|path| !in_view.contains(path.as_path())).collect();
    Ok(View { texts, absent })
}

/// The repository holding `dir` in `wb git arc held` output: its root, the
/// baseline tree and the paths other threads hold, relative to the root.
fn repository(json: &str, dir: &Path) -> Result<Option<(PathBuf, String, Vec<String>)>, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("unexpected `wb git arc held` output: {e}"))?;
    let dir = normalize_path(dir);
    let repos = value["repositories"].as_array().into_iter().flatten();
    let Some(repo) = repos.filter_map(|repo| Some((normalize_path(Path::new(repo["repoRoot"].as_str()?)), repo)))
        .filter(|(root, _)| dir.starts_with(root)).max_by_key(|(root, _)| root.components().count()) else { return Ok(None) };
    let (root, repo) = repo;
    let baseline = repo["baseline"].as_str().ok_or("`wb git arc held` gave no baseline")?.to_string();
    // Held entries are paths, or owners listing them.
    let held = repo["held"].as_array().into_iter().flatten().flat_map(|entry| match entry.as_str() {
        Some(path) => vec![path.to_string()],
        None => entry["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect(),
    }).collect();
    Ok(Some((root, baseline, held)))
}

/// The reason of a wb rejection (`rejection {"reason": ...}` in its output).
fn rejection(output: &str) -> Option<String> {
    let line = output.lines().find_map(|line| line.trim().strip_prefix("rejection "))?;
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    value["reason"].as_str().map(String::from)
}

/// `git cat-file --batch` output: each object's content, in order.
fn batch_texts(output: &[u8]) -> Result<Vec<String>, String> {
    let mut texts = Vec::new();
    let mut rest = output;
    while !rest.is_empty() {
        let newline = rest.iter().position(|&b| b == b'\n').ok_or("truncated `git cat-file` output")?;
        let header = std::str::from_utf8(&rest[..newline]).map_err(|e| e.to_string())?;
        let size: usize = header.rsplit(' ').next().and_then(|s| s.parse().ok()).ok_or_else(|| format!("unexpected `git cat-file` header: {header}"))?;
        let body = rest.get(newline + 1..newline + 1 + size).ok_or("truncated `git cat-file` output")?;
        texts.push(String::from_utf8_lossy(body).into_owned());
        rest = rest.get(newline + 2 + size..).unwrap_or_default();
    }
    Ok(texts)
}

/// The files at or below `path` on disk.
fn disk_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![normalize_path(path)];
    }
    ignore::WalkBuilder::new(path).standard_filters(false).build().flatten()
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .map(|entry| normalize_path(entry.path())).collect()
}

fn run(command: &mut Command, input: Option<String>) -> Result<String, String> {
    run_bytes(command, input).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn run_bytes(command: &mut Command, input: Option<String>) -> Result<Vec<u8>, String> {
    let name = command.get_program().to_string_lossy().into_owned();
    let mut child = command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("cannot run `{name}`: {e}"))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| format!("`{name}` failed: {e}"))?;
    if !out.status.success() {
        return Err(format!("`{name}` failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_innermost_repository_holding_the_directory_gives_tree_and_held_paths() {
        let json = r#"{"repositories":[
            {"repoRoot":"/w","head":"h","baseline":"outer","held":[]},
            {"repoRoot":"/w/inner","head":"h","baseline":"inner","held":[{"paths":["src/a.ts","lib"]},"b.ts"]}
        ]}"#;
        let (root, tree, held) = repository(json, Path::new("/w/inner/src")).unwrap().unwrap();
        assert_eq!((root, tree.as_str(), held), (normalize_path(Path::new("/w/inner")), "inner", vec!["src/a.ts".to_string(), "lib".into(), "b.ts".into()]));
        assert_eq!(repository(json, Path::new("/elsewhere")).unwrap(), None);
        assert!(repository("not json", Path::new("/w")).is_err());
    }

    #[test]
    fn rejections_give_their_reason() {
        let failure = "`wb` failed: arc failure arcHeld rejection\nrejection {\"reason\":\"unmanagedRepository\"}\ndiagnostic The managed thread has no Git arc identity";
        assert_eq!(rejection(failure).as_deref(), Some("unmanagedRepository"));
        assert_eq!(rejection("`wb` failed: cannot connect"), None);
    }

    #[test]
    fn batches_split_by_size() {
        // Contents may hold newlines; sizes delimit them.
        let batch = b"aaa blob 4\na\nb\n\nccc blob 0\n\n";
        assert_eq!(batch_texts(batch).unwrap(), ["a\nb\n", ""]);
        assert!(batch_texts(b"aaa blob 9\nshort").is_err());
    }
}
