//! Background workspace scan: lints every file on disk and streams each
//! file's diagnostics to the client as soon as it is done, with progress.
//! A newer scan (config change) cancels older ones via the generation.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ignore::{WalkBuilder, WalkState};
use serde_json::json;

use super::transport::Outbox;
use super::{convert, uri};
use zoomie_lint::engine::Linter;

/// State shared between the request loop and scans.
#[derive(Default)]
pub struct Shared {
    /// Files open in the editor; their diagnostics come from the buffer.
    pub open: Mutex<HashSet<PathBuf>>,
    /// URIs that currently show at least one diagnostic.
    pub published: Mutex<HashSet<String>>,
    pub generation: AtomicU64,
}

impl Shared {
    /// Publishes, skipping the message when a clean file was already clean.
    pub fn publish(&self, out: &Outbox, uri: &str, version: Option<i64>, diagnostics: serde_json::Value) {
        let empty = diagnostics.as_array().is_some_and(Vec::is_empty);
        {
            let mut published = self.published.lock().expect("lock");
            if empty && !published.remove(uri) {
                return;
            }
            if !empty {
                published.insert(uri.to_string());
            }
        }
        let mut params = json!({ "uri": uri, "diagnostics": diagnostics });
        if let Some(v) = version {
            params["version"] = json!(v);
        }
        out.notify("textDocument/publishDiagnostics", params);
    }
}

pub fn start(linter: Arc<Linter>, shared: Arc<Shared>, out: Outbox, progress: bool) {
    let generation = shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || run(linter, shared, out, progress, generation));
}

fn run(linter: Arc<Linter>, shared: Arc<Shared>, out: Outbox, progress: bool, generation: u64) {
    let token = format!("zlint-scan-{generation}");
    if progress {
        out.request(&format!("progress-{generation}"), "window/workDoneProgress/create", json!({ "token": token }));
        out.notify(
            "$/progress",
            json!({ "token": token, "value": { "kind": "begin", "title": "zlint", "message": "scanning workspace", "cancellable": false } }),
        );
    }
    let files = AtomicUsize::new(0);
    let problems = AtomicUsize::new(0);
    let touched = Mutex::new(HashSet::new());
    let current = || shared.generation.load(Ordering::SeqCst) == generation;

    WalkBuilder::new(&linter.root).hidden(false).filter_entry(|e| e.file_name() != ".git").build_parallel().run(|| {
        Box::new(|entry| {
            if !current() {
                return WalkState::Quit;
            }
            let Ok(entry) = entry else { return WalkState::Continue };
            let path = entry.path();
            if !entry.file_type().is_some_and(|t| t.is_file()) || !linter.wants(path) {
                return WalkState::Continue;
            }
            if shared.open.lock().expect("lock").contains(path) {
                return WalkState::Continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else { return WalkState::Continue };
            let diags = linter.lint(path, &text);
            let u = uri::from_path(path);
            problems.fetch_add(diags.len(), Ordering::Relaxed);
            touched.lock().expect("lock").insert(u.clone());
            // The editor may have opened the file while it was being linted;
            // then the buffer's diagnostics win.
            if current() && !shared.open.lock().expect("lock").contains(path) {
                shared.publish(&out, &u, None, convert::diagnostics(&text, &diags));
            }
            let n = files.fetch_add(1, Ordering::Relaxed) + 1;
            if progress && n.is_multiple_of(100) {
                out.notify("$/progress", json!({ "token": token, "value": { "kind": "report", "message": format!("{n} files") } }));
            }
            WalkState::Continue
        })
    });

    if current() {
        // Files that showed problems before but were not seen now (deleted,
        // or no longer owned by any language) get cleared.
        let touched = touched.into_inner().expect("lock");
        let open = shared.open.lock().expect("lock").clone();
        let open_uris: HashSet<String> = open.iter().map(|p| uri::from_path(p)).collect();
        let stale: Vec<String> =
            shared.published.lock().expect("lock").iter().filter(|u| !touched.contains(*u) && !open_uris.contains(*u)).cloned().collect();
        for u in stale {
            shared.publish(&out, &u, None, json!([]));
        }
    }
    if progress {
        let message = format!("{} files, {} problems", files.load(Ordering::Relaxed), problems.load(Ordering::Relaxed));
        out.notify("$/progress", json!({ "token": token, "value": { "kind": "end", "message": message } }));
    }
}
