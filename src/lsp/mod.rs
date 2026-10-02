//! Language server. Open documents are linted on every change; the rest of
//! the workspace is scanned in the background and streamed file by file.
//! Each file uses its nearest `zoomie.zlint`. `.zlint` edits reload configs
//! live, with errors shown in place.

mod convert;
mod projects;
mod scan;
mod transport;
mod uri;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{mpsc, Arc};

use serde_json::{json, Value};

use projects::Projects;
use scan::Shared;
use transport::Outbox;
use zoomie_lint::config::source::{normalize_path, SourceProvider};
use zoomie_lint::config::{self};
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
    /// Workspace folders.
    roots: Vec<PathBuf>,
    projects: Projects,
    docs: HashMap<PathBuf, Doc>,
    scan_workspace: bool,
    progress: bool,
    warned_no_config: bool,
    /// Open documents whose diagnostics are out of date.
    dirty: HashSet<PathBuf>,
    /// Find every config again (startup, a `zoomie.zlint` created or deleted).
    rediscover: bool,
    /// Configs to reload.
    stale: HashSet<PathBuf>,
}

pub fn run() -> ExitCode {
    let (out, writer) = Outbox::spawn();
    let mut server = Server {
        out: out.clone(),
        shared: Arc::new(Shared::default()),
        roots: Vec::new(),
        projects: Projects::default(),
        docs: HashMap::new(),
        scan_workspace: true,
        progress: false,
        warned_no_config: false,
        dirty: HashSet::new(),
        rediscover: false,
        stale: HashSet::new(),
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
                "initialized" | "workspace/didChangeConfiguration" => self.rediscover = true,
                "textDocument/didOpen" => self.did_open(params),
                "textDocument/didChange" => self.did_change(params),
                "textDocument/didClose" => self.did_close(params),
                "workspace/didChangeWatchedFiles" => self.watched_files(params),
                _ => {}
            },
        }
        false
    }

    /// Does the deferred work of a burst of messages: at most one reload per
    /// config, and one lint per changed document.
    fn flush(&mut self) {
        self.reload_configs();
        for path in std::mem::take(&mut self.dirty) {
            self.publish_doc(&path);
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let folders = params["workspaceFolders"].as_array().into_iter().flatten();
        self.roots = folders.filter_map(|f| f["uri"].as_str().and_then(uri::to_path)).collect();
        if self.roots.is_empty() {
            let root = params["rootUri"].as_str().and_then(uri::to_path);
            self.roots.extend(root.or_else(|| params["rootPath"].as_str().map(|p| normalize_path(Path::new(p)))));
        }
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

    /// Loads stale configs, or every config after a rediscovery, then relints
    /// and rescans when the linters changed.
    fn reload_configs(&mut self) {
        let mut configs = std::mem::take(&mut self.stale);
        let mut changed = false;
        if std::mem::take(&mut self.rediscover) {
            let found = if self.scan_workspace {
                projects::discover(&self.roots)
            } else {
                self.roots.iter().filter_map(|r| config::find(r)).collect()
            };
            let open = self.docs.keys().filter_map(|p| p.parent().and_then(config::find));
            configs = found.into_iter().chain(open).collect();
            changed = self.projects.retain(&configs);
            if configs.is_empty() && !self.warned_no_config {
                self.warned_no_config = true;
                self.show(3, &format!("zlint: no {} found in the workspace; linting is off", config::CONFIG_NAME));
            }
        }
        if configs.is_empty() && !changed {
            return;
        }
        let docs = &self.docs;
        let uri_of = |p: &Path| docs.get(p).map(|d| d.uri.clone()).unwrap_or_else(|| uri::from_path(p));
        for config in &configs {
            changed |= self.projects.load(config, &Overlay(docs), &uri_of);
        }
        self.projects.publish_errors(&self.out);

        // A broken config keeps its last good linter, so nothing to relint.
        if !changed {
            return;
        }
        self.dirty.extend(self.docs.keys().cloned());
        if self.scan_workspace {
            let roots = if self.roots.is_empty() { self.projects.dirs() } else { self.roots.clone() };
            scan::start(self.projects.linters(), roots, self.shared.clone(), self.out.clone(), self.progress);
        }
    }

    /// A `.zlint` file changed in the editor or on disk: reload what reads it.
    fn config_touched(&mut self, path: &Path) {
        self.stale.extend(self.projects.owners_of(path));
    }

    fn publish_doc(&self, path: &Path) {
        let Some(doc) = self.docs.get(path) else { return };
        if self.projects.has_errors(&doc.uri) {
            return;
        }
        let diags = match self.projects.linter_for(path) {
            Some(l) => convert::diagnostics(&doc.text, &l.lint(path, &doc.text)),
            None => json!([]),
        };
        self.shared.publish(&self.out, &doc.uri, Some(doc.version), diags);
    }

    /// After a file leaves the editor, its diagnostics come from disk.
    fn publish_disk(&self, path: &Path, u: &str) {
        let diags = match (self.projects.linter_for(path), self.scan_workspace) {
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
        if self.projects.is_config(&path) {
            self.config_touched(&path);
            return;
        }
        // Files outside the workspace, or under configs not loaded yet.
        if let Some(config) = path.parent().and_then(config::find) {
            if !self.projects.is_loaded(&config) {
                self.stale.insert(config);
            }
        }
        self.dirty.insert(path);
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
        if self.projects.is_config(&path) {
            self.config_touched(&path);
        } else {
            self.dirty.insert(path);
        }
    }

    fn did_close(&mut self, params: &Value) {
        let Some((path, u)) = Self::doc_path(params) else { return };
        self.docs.remove(&path);
        self.shared.open.lock().expect("lock").remove(&path);
        if self.projects.is_config(&path) {
            self.config_touched(&path);
        } else {
            self.publish_disk(&path, &u);
        }
    }

    fn watched_files(&mut self, params: &Value) {
        for change in params["changes"].as_array().into_iter().flatten() {
            let Some(u) = change["uri"].as_str() else { continue };
            let Some(path) = uri::to_path(u) else { continue };
            // 1 = created, 3 = deleted
            let kind = change["type"].as_u64();
            if path.file_name().is_some_and(|n| n == config::CONFIG_NAME) && matches!(kind, Some(1 | 3)) {
                // Which config is nearest may have changed for any file.
                self.rediscover = true;
            } else if self.projects.is_config(&path) {
                self.config_touched(&path);
            } else if !self.docs.contains_key(&path) {
                if kind == Some(3) {
                    self.shared.publish(&self.out, u, None, json!([]));
                } else {
                    self.publish_disk(&path, u);
                }
            }
        }
    }

    fn fix_all(&self, path: &Path) -> Option<Value> {
        let doc = self.docs.get(path)?;
        let linter = self.projects.linter_for(path)?;
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
