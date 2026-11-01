//! Agents in Workbench lint their arc view: files other live threads hold
//! read as the view has them, the rest from disk. `wb git arc held
//! --contents` gives each held file's checkout bytes in the view (or that it
//! is absent there). Nothing else in dxlint knows about Workbench: the view
//! becomes read overlays.

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
    let held = run(Command::new("wb").args(["git", "arc", "held", "--contents"]).current_dir(dir)
        // A tool calling wb, as Workbench asks scripts to say.
        .env("THIS_IS_A_MANAGED_SCRIPT", "1"));
    match held {
        Ok(json) => view(&json, dir),
        // A repository Workbench does not manage has no view: the disk is it.
        Err(error) if rejection(&error).as_deref() == Some("unmanagedRepository") => Ok(View::default()),
        Err(error) => Err(error),
    }
}

/// The view of the innermost repository holding `dir`, from `wb git arc
/// held --contents` output: each held file's base64 bytes, or null when the
/// view has no such file.
fn view(json: &str, dir: &Path) -> Result<View, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("unexpected `wb git arc held` output: {e}"))?;
    let dir = normalize_path(dir);
    let repos = value["repositories"].as_array().into_iter().flatten();
    let Some((root, repo)) = repos.filter_map(|repo| Some((normalize_path(Path::new(repo["repoRoot"].as_str()?)), repo)))
        .filter(|(root, _)| dir.starts_with(root)).max_by_key(|(root, _)| root.components().count()) else { return Ok(View::default()) };
    let mut view = View::default();
    for (path, bytes) in repo["contents"].as_object().into_iter().flatten() {
        let path = normalize_path(&root.join(path));
        match bytes.as_str() {
            Some(encoded) => {
                let bytes = base64(encoded).ok_or_else(|| format!("`wb git arc held` gave {} undecodable contents", path.display()))?;
                view.texts.push((path, String::from_utf8_lossy(&bytes).into_owned()));
            }
            None => view.absent.push(path),
        }
    }
    Ok(view)
}

/// Standard base64 (padding optional); `None` on any other character.
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let (mut buffer, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|&c| c != b'=') {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        buffer = buffer << 6 | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
        }
    }
    Some(bytes)
}

/// The reason of a wb rejection (`rejection {"reason": ...}` in its output).
fn rejection(output: &str) -> Option<String> {
    let line = output.lines().find_map(|line| line.trim().strip_prefix("rejection "))?;
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    value["reason"].as_str().map(String::from)
}

fn run(command: &mut Command) -> Result<String, String> {
    let name = command.get_program().to_string_lossy().into_owned();
    let out = command.stdin(Stdio::null()).output().map_err(|e| format!("cannot run `{name}`: {e}"))?;
    if !out.status.success() {
        return Err(format!("`{name}` failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_innermost_repository_holding_the_directory_gives_its_held_contents() {
        let json = r#"{"repositories":[
            {"repoRoot":"/w","contents":{"outer.ts":"eA=="}},
            {"repoRoot":"/w/inner","contents":{"src/a.ts":"Y29uc3QgYSA9IDE7Cg==","gone.ts":null}}
        ]}"#;
        let inner = |path: &str| normalize_path(&Path::new("/w/inner").join(path));
        let view = view(json, Path::new("/w/inner/src")).unwrap();
        assert_eq!(view, View { texts: vec![(inner("src/a.ts"), "const a = 1;\n".into())], absent: vec![inner("gone.ts")] });
        assert_eq!(super::view(json, Path::new("/elsewhere")).unwrap(), View::default());
        assert!(super::view("not json", Path::new("/w")).is_err());
        assert!(super::view(r#"{"repositories":[{"repoRoot":"/w","contents":{"a":"not base64!"}}]}"#, Path::new("/w")).is_err());
    }

    #[test]
    fn rejections_give_their_reason() {
        let failure = "`wb` failed: arc failure arcHeld rejection\nrejection {\"reason\":\"unmanagedRepository\"}\ndiagnostic The managed thread has no Git arc identity";
        assert_eq!(rejection(failure).as_deref(), Some("unmanagedRepository"));
        assert_eq!(rejection("`wb` failed: cannot connect"), None);
    }
}
