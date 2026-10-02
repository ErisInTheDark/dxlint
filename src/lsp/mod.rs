//! Language server. Open documents are linted on every change; the rest of
//! the workspace is scanned in the background and streamed file by file.
//! `.zlint` edits reload the config live, with errors shown in place.

mod convert;
mod scan;
mod transport;
mod uri;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{mpsc, Arc};

use serde_json::{json, Value};

use scan::Shared;
use transport::Outbox;
use zoomie_lint::config::source::{normalize_path, SourceProvider};
use zoomie_lint::config::{self};
use zoomie_lint::engine::{build, Linter};
use zoomie_lint::line_index::LineIndex;

struct Doc {
    uri: String,
    text: String,
    version: i64,
}

/// Config reads see unsaved editor buffers.
struct Overlay<'a>(&'a HashMap<PathBuf, Doc>);

impl SourceProvider for Overlay<'_> {
    fn read(&self, path: &Path) -> io::Result<String> {
        match self.0.get(&normalize_path(path)) {
            Some(doc) => Ok(doc.text.clone()),
            None => std::fs::read_to_string(path),
        }
    }
}

struct Server {
    out: Outbox,
    shared: Arc<Shared>,
    root: Option<PathBuf>,
    linter: Option<Arc<Linter>>,
    config_path: Option<PathBuf>,
    config_files: HashSet<PathBuf>,
    config_error_uris: HashSet<String>,
    docs: HashMap<PathBuf, Doc>,
    scan_workspace: bool,
    progress: bool,
    warned_no_config: bool,
    /// Open documents whose diagnostics are out of date.
    dirty: HashSet<PathBuf>,
    reload_pending: bool,
}

pub fn run() -> ExitCode {
    let (out, writer) = Outbox::spawn();
    let mut server = Server {
        out: out.clone(),
        shared: Arc::new(Shared::default()),
        root: None,
        linter: None,
        config_path: None,
        config_files: HashSet::new(),
        config_error_uris: HashSet::new(),
        docs: HashMap::new(),
        scan_workspace: true,
        progress: false,
        warned_no_config: false,
        dirty: HashSet::new(),
        reload_pending: false,
    };
    // Reading happens on its own thread so the loop below can see how much
    // input is already waiting, and lint once per burst of edits instead of
    // once per keystroke.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        loop {
            let msg = transport::read(&mut input);
            let done = !matches!(msg, Ok(Some(_)));
            if tx.send(msg).is_err() || done {
                break;
            }
        }
    });
    let mut shutdown = false;
    let code = 'serve: loop {
        let mut next = rx.recv().ok();
        while let Some(msg) = next.take() {
            let msg = match msg {
                Ok(Some(m)) => m,
                Ok(None) => break 'serve if shutdown { ExitCode::SUCCESS } else { ExitCode::from(1) },
                Err(e) => {
                    eprintln!("zlint lsp: {e}");
                    break 'serve ExitCode::from(1);
                }
            };
            if server.handle(&msg, &mut shutdown) {
                break 'serve if shutdown { ExitCode::SUCCESS } else { ExitCode::from(1) };
            }
            next = rx.try_recv().ok();
        }
        server.flush();
    };
    // Stop background scans and let the writer drain.
    server.shared.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    drop(server);
    drop(out);
    let _ = writer.join();
    code
}

impl Server {
    /// Handles one message. Returns true on `exit`.
    fn handle(&mut self, msg: &Value, shutdown: &mut bool) -> bool {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = msg.get("params").unwrap_or(&Value::Null);
        match msg.get("id").cloned() {
            // Responses to our own requests (progress tokens) need nothing.
            Some(_) if method.is_empty() => {}
            Some(id) => {
                // Requests see the latest config and diagnostics.
                self.flush();
                match method {
                    "initialize" => {
                        let result = self.initialize(params);
                        self.out.respond(id, result);
                    }
                    "shutdown" => {
                        *shutdown = true;
                        self.out.respond(id, Value::Null);
                    }
                    "textDocument/codeAction" => self.out.respond(id, self.code_actions(params)),
                    "textDocument/formatting" => self.out.respond(id, self.format(params)),
                    _ => self.out.respond_error(id, -32601, &format!("method not supported: {method}")),
                }
            }
            None => match method {
                "exit" => return true,
                "initialized" | "workspace/didChangeConfiguration" => self.reload_pending = true,
                "textDocument/didOpen" => self.did_open(params),
                "textDocument/didChange" => self.did_change(params),
                "textDocument/didClose" => self.did_close(params),
                "workspace/didChangeWatchedFiles" => self.watched_files(params),
                _ => {}
            },
        }
        false
    }

    /// Does the deferred work of a burst of messages: at most one config
    /// reload, and one lint per changed document.
    fn flush(&mut self) {
        if std::mem::take(&mut self.reload_pending) {
            self.reload_config();
        }
        for path in std::mem::take(&mut self.dirty) {
            self.publish_doc(&path);
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let root_uri = params["workspaceFolders"][0]["uri"].as_str().or_else(|| params["rootUri"].as_str());
        self.root = root_uri.and_then(uri::to_path).or_else(|| params["rootPath"].as_str().map(|p| normalize_path(Path::new(p))));
        self.progress = params["capabilities"]["window"]["workDoneProgress"].as_bool().unwrap_or(false);
        if let Some(scan) = params["initializationOptions"]["scanWorkspace"].as_bool() {
            self.scan_workspace = scan;
        }
        json!({
            "capabilities": {
                "textDocumentSync": { "openClose": true, "change": 2 },
                "codeActionProvider": { "codeActionKinds": ["quickfix", "source.fixAll.zlint"] },
                "documentFormattingProvider": true,
            },
            "serverInfo": { "name": "zlint", "version": env!("CARGO_PKG_VERSION") },
        })
    }

    fn show(&self, kind: u8, message: &str) {
        self.out.notify("window/showMessage", json!({ "type": kind, "message": message }));
    }

    fn reload_config(&mut self) {
        let start = self.root.clone().or_else(|| self.docs.keys().next().and_then(|p| p.parent().map(Path::to_path_buf)));
        let Some(path) = start.and_then(|s| config::find(&s)) else {
            if !self.warned_no_config {
                self.warned_no_config = true;
                self.show(3, &format!("zlint: no {} found in the workspace; linting is off", config::CONFIG_NAME));
            }
            return;
        };
        let root = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let built = build(config::load_file(&path, &Overlay(&self.docs)), root);
        self.config_path = Some(path);
        self.config_files = built.sources.files().filter_map(|(_, f)| f.path.clone()).collect();

        // Config errors appear in the .zlint files they point at.
        let mut by_uri: HashMap<String, Vec<Value>> = HashMap::new();
        for e in &built.errors {
            let file = e.span.and_then(|s| built.sources.get(s.file).path.clone()).or_else(|| self.config_path.clone());
            if let Some(file) = file {
                let u = self.docs.get(&file).map(|d| d.uri.clone()).unwrap_or_else(|| uri::from_path(&file));
                by_uri.entry(u).or_default().push(convert::config_error(&built.sources, e));
            }
        }
        for stale in self.config_error_uris.iter().filter(|u| !by_uri.contains_key(*u)) {
            self.out.notify("textDocument/publishDiagnostics", json!({ "uri": stale, "diagnostics": [] }));
        }
        self.config_error_uris = by_uri.keys().cloned().collect();
        for (u, diags) in by_uri {
            self.out.notify("textDocument/publishDiagnostics", json!({ "uri": u, "diagnostics": diags }));
        }

        // A broken config keeps the last good linter running.
        let Some(linter) = built.linter else { return };
        self.linter = Some(Arc::new(linter));
        self.dirty.extend(self.docs.keys().cloned());
        if self.scan_workspace {
            if let Some(l) = &self.linter {
                scan::start(l.clone(), self.shared.clone(), self.out.clone(), self.progress);
            }
        }
    }

    fn is_config(&self, path: &Path) -> bool {
        self.config_files.contains(path) || path.file_name().is_some_and(|n| n == config::CONFIG_NAME)
    }

    fn publish_doc(&self, path: &Path) {
        let Some(doc) = self.docs.get(path) else { return };
        if self.config_error_uris.contains(&doc.uri) {
            return;
        }
        let diags = match &self.linter {
            Some(l) => convert::diagnostics(&doc.text, &l.lint(path, &doc.text)),
            None => json!([]),
        };
        self.shared.publish(&self.out, &doc.uri, Some(doc.version), diags);
    }

    /// After a file leaves the editor, its diagnostics come from disk.
    fn publish_disk(&self, path: &Path, u: &str) {
        let diags = match (&self.linter, self.scan_workspace) {
            (Some(l), true) if l.wants(path) => match std::fs::read_to_string(path) {
                Ok(text) => convert::diagnostics(&text, &l.lint(path, &text)),
                Err(_) => json!([]),
            },
            _ => json!([]),
        };
        self.shared.publish(&self.out, u, None, diags);
    }

    fn doc_path(params: &Value) -> Option<(PathBuf, String)> {
        let u = params["textDocument"]["uri"].as_str()?;
        Some((uri::to_path(u)?, u.to_string()))
    }

    fn did_open(&mut self, params: &Value) {
        let Some((path, u)) = Self::doc_path(params) else { return };
        let td = &params["textDocument"];
        let doc = Doc { uri: u, text: td["text"].as_str().unwrap_or_default().to_string(), version: td["version"].as_i64().unwrap_or(0) };
        self.docs.insert(path.clone(), doc);
        self.shared.open.lock().expect("lock").insert(path.clone());
        if (self.linter.is_none() && self.config_path.is_none()) || self.is_config(&path) {
            self.reload_pending = true;
        }
        if !self.is_config(&path) {
            self.dirty.insert(path);
        }
    }

    fn did_change(&mut self, params: &Value) {
        let Some((path, _)) = Self::doc_path(params) else { return };
        let Some(doc) = self.docs.get_mut(&path) else { return };
        doc.version = params["textDocument"]["version"].as_i64().unwrap_or(doc.version);
        for change in params["contentChanges"].as_array().into_iter().flatten() {
            let text = change["text"].as_str().unwrap_or_default();
            match change.get("range") {
                Some(range) => {
                    let lines = LineIndex::new(&doc.text);
                    let pos = |p: &Value| {
                        lines.from_utf16(&doc.text, p["line"].as_u64().unwrap_or(0) as u32, p["character"].as_u64().unwrap_or(0) as u32)
                    };
                    let (start, end) = (pos(&range["start"]), pos(&range["end"]));
                    doc.text.replace_range(start.min(end)..end, text);
                }
                None => doc.text = text.to_string(),
            }
        }
        if self.is_config(&path) {
            self.reload_pending = true;
        } else {
            self.dirty.insert(path);
        }
    }

    fn did_close(&mut self, params: &Value) {
        let Some((path, u)) = Self::doc_path(params) else { return };
        self.docs.remove(&path);
        self.shared.open.lock().expect("lock").remove(&path);
        if self.is_config(&path) {
            self.reload_pending = true;
        } else {
            self.publish_disk(&path, &u);
        }
    }

    fn watched_files(&mut self, params: &Value) {
        let mut reload = false;
        for change in params["changes"].as_array().into_iter().flatten() {
            let Some(u) = change["uri"].as_str() else { continue };
            let Some(path) = uri::to_path(u) else { continue };
            if self.is_config(&path) {
                reload = true;
            } else if !self.docs.contains_key(&path) {
                // 3 = deleted
                if change["type"].as_u64() == Some(3) {
                    self.shared.publish(&self.out, u, None, json!([]));
                } else {
                    self.publish_disk(&path, u);
                }
            }
        }
        self.reload_pending |= reload;
    }

    fn fix_all(&self, path: &Path) -> Option<Value> {
        let doc = self.docs.get(path)?;
        let linter = self.linter.as_ref()?;
        let fixed = linter.fix(path, &doc.text);
        (fixed.applied > 0 && fixed.text != doc.text).then(|| convert::whole_document_edit(&doc.text, &fixed.text))
    }

    fn code_actions(&self, params: &Value) -> Value {
        let Some((path, u)) = Self::doc_path(params) else { return json!([]) };
        let only: Vec<&str> = params["context"]["only"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let wants = |kind: &str| only.is_empty() || only.iter().any(|o| kind == *o || kind.starts_with(&format!("{o}.")));
        let mut actions = Vec::new();
        let mut fixable = 0;
        for d in params["context"]["diagnostics"].as_array().into_iter().flatten() {
            if d["source"] != "zlint" {
                continue;
            }
            let Some(fix) = d["data"]["fix"].as_array() else { continue };
            fixable += 1;
            if wants("quickfix") {
                actions.push(json!({
                    "title": format!("Fix: {}", d["message"].as_str().unwrap_or_default()),
                    "kind": "quickfix",
                    "diagnostics": [d],
                    "isPreferred": true,
                    "edit": { "changes": { u.clone(): fix } },
                }));
            }
        }
        if (fixable > 0 && wants("quickfix")) || (!only.is_empty() && wants("source.fixAll.zlint")) {
            if let Some(edit) = self.fix_all(&path) {
                let kind = if wants("source.fixAll.zlint") && !only.is_empty() { "source.fixAll.zlint" } else { "quickfix" };
                actions.push(json!({
                    "title": "Fix all zlint problems in this file",
                    "kind": kind,
                    "edit": { "changes": { u: [edit] } },
                }));
            }
        }
        Value::Array(actions)
    }

    fn format(&self, params: &Value) -> Value {
        let Some((path, _)) = Self::doc_path(params) else { return Value::Null };
        match self.fix_all(&path) {
            Some(edit) => json!([edit]),
            None => json!([]),
        }
    }
}
