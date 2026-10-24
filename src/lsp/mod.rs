//! Language server. Open documents are linted on every change; the rest of
//! the workspace is scanned in the background and streamed file by file.
//! Each file uses its nearest `.dx` config. `.dx` edits reload configs
//! live, with errors shown in place.

mod config_docs;
mod convert;
mod projects;
mod scan;
mod transport;
mod uri;

use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

use serde_json::{json, Value};

use projects::Projects;
use scan::Shared;
use transport::Outbox;
use crate::config::source::{normalize_path, SourceProvider};
use crate::config::{self};
use crate::engine::Linter;
use crate::line_index::LineIndex;
use crate::engine::Stale;
use crate::engine::reads::{self, Reads};

#[cfg(test)]
mod tests;

struct Doc {
    uri: String,
    text: String,
    version: i64,
    /// Whether [`Server::editor_config`] lets this file be linted at all.
    lint: bool,
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
    /// Hover in source files shows the layers and facts at the cursor
    /// (`dxlint.inspectHover`).
    inspect_hover: bool,
    warned_no_config: bool,
    /// Open documents whose diagnostics are out of date.
    dirty: HashSet<PathBuf>,
    /// Find every config again (startup, a `.dx` config created or deleted).
    rediscover: bool,
    /// Configs to reload.
    stale: HashSet<PathBuf>,
    /// Files in visible editors, most important first (`dxlint/focus`).
    visible: Vec<PathBuf>,
    /// Files open in editor tabs (`dxlint/focus`) that scans reach; they lint
    /// them first.
    tabs: Vec<PathBuf>,
    /// Gitignored tabs under a gitignored config. Scans never reach them, so
    /// the request loop lints them from disk.
    ignored_tabs: HashSet<PathBuf>,
    /// Ignored tabs whose diagnostics are out of date.
    dirty_tabs: HashSet<PathBuf>,
    /// Source files edited, created or deleted since the last flush: what
    /// other files learned from them is out of date.
    changed: HashSet<PathBuf>,
}

pub fn run<R: Read + Send + 'static, W: Write + Send + 'static>(input: R, output: W, stderr: &mut dyn Write) -> u8 {
    let (out, writer) = Outbox::spawn(output);
    let mut server = Server {
        out: out.clone(),
        shared: Arc::new(Shared::default()),
        roots: Vec::new(),
        projects: Projects::default(),
        docs: HashMap::new(),
        scan_workspace: true,
        progress: false,
        inspect_hover: false,
        warned_no_config: false,
        dirty: HashSet::new(),
        rediscover: false,
        stale: HashSet::new(),
        visible: Vec::new(),
        tabs: Vec::new(),
        ignored_tabs: HashSet::new(),
        dirty_tabs: HashSet::new(),
        changed: HashSet::new(),
    };
    // Reading happens on its own thread so the loop below can see how much
    // input is already waiting, and lint once per burst of edits instead of
    // once per keystroke.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut input = io::BufReader::new(input);
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
                Ok(None) => break 'serve if shutdown { 0 } else { 1 },
                Err(e) => {
                    writeln!(stderr, "dxlint lsp: {e}").expect("write stderr");
                    break 'serve 1;
                }
            };
            if server.handle(&msg, &mut shutdown) {
                break 'serve if shutdown { 0 } else { 1 };
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
                    "textDocument/hover" => self.out.respond(id, self.hover(params)),
                    "textDocument/definition" => self.out.respond(id, self.in_config(params, |c| c.definition(params))),
                    "textDocument/references" => self.out.respond(id, self.in_config(params, |c| c.references(params))),
                    "textDocument/documentSymbol" => self.out.respond(id, self.in_config(params, |c| c.outline())),
                    "textDocument/completion" => self.out.respond(id, self.in_config(params, |c| c.complete(params))),
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
                "dxlint/focus" => self.focus(params),
                _ => {}
            },
        }
        false
    }

    /// Does the deferred work of a burst of messages: at most one reload per
    /// config, one lint per changed document (visible ones first), and only
    /// then a rescan, so the scan never competes with what the user sees.
    fn flush(&mut self) {
        let rescan = self.reload_configs();
        self.relint_dependents();
        let mut dirty: Vec<PathBuf> = std::mem::take(&mut self.dirty).into_iter().collect();
        dirty.sort_by_key(|p| self.visible.iter().position(|v| v == p).unwrap_or(usize::MAX));
        for path in dirty {
            self.publish_doc(&path);
        }
        for path in std::mem::take(&mut self.dirty_tabs) {
            self.publish_ignored_tab(&path);
        }
        if rescan && self.scan_workspace {
            let roots = if self.roots.is_empty() { self.projects.dirs() } else { self.roots.clone() };
            scan::start(self.projects.linters(), roots, self.tabs.clone(), self.shared.clone(), self.out.clone(), self.progress);
        }
    }

    /// Makes every linter forget what the changed files said, and relints
    /// the files whose last lint read something that changed: open ones
    /// with the next flush, the rest in the background.
    fn relint_dependents(&mut self) {
        if self.changed.is_empty() {
            return;
        }
        let changed: Vec<PathBuf> = std::mem::take(&mut self.changed).into_iter().collect();
        let linters = self.projects.linters();
        let stale: Vec<(&Arc<Linter>, Stale)> = linters.all().map(|l| (l, l.invalidate(&changed))).collect();
        let reads: Vec<(PathBuf, Arc<Reads>)> = self.shared.reads.lock().expect("lock").iter().map(|(p, r)| (p.clone(), r.clone())).collect();
        let mut background = Vec::new();
        for (path, reads) in reads {
            let Some(linter) = linters.for_path(&path) else { continue };
            let Some((_, stale)) = stale.iter().find(|(l, _)| Arc::ptr_eq(l, linter)) else { continue };
            if changed.contains(&path) || !linter.is_stale(stale, &reads) {
                continue;
            }
            if self.docs.contains_key(&path) {
                self.dirty.insert(path);
            } else if self.ignored_tabs.contains(&path) {
                self.dirty_tabs.insert(path);
            } else {
                background.push(path);
            }
        }
        if !background.is_empty() {
            scan::relint(linters.clone(), background, self.shared.clone(), self.out.clone());
        }
    }

    /// A source file's text changed (buffer or disk), or it came or went.
    fn source_changed(&mut self, path: &Path) {
        match self.docs.get(path) {
            Some(doc) => reads::set_buffer(path, &doc.text),
            None => reads::drop_buffer(path),
        }
        self.changed.insert(path.to_path_buf());
    }

    /// The config for a file the user has in the editor (buffer or tab): its
    /// nearest one, unless the file is gitignored and that config is not.
    /// Tracked configs are for the project; a gitignored config is for the
    /// gitignored files below it.
    fn editor_config(&self, path: &Path) -> Option<PathBuf> {
        let config = path.parent().and_then(config::find)?;
        if !scan::walk_reaches(&self.roots, path) && scan::walk_reaches(&self.roots, &config) {
            return None;
        }
        Some(config)
    }

    /// Makes sure `config` gets loaded by the next flush.
    fn want_config(&mut self, config: PathBuf) {
        if !self.projects.is_loaded(&config) {
            self.stale.insert(config);
        }
    }

    /// The editor's visible files and tabs changed. Tabs not seen before jump
    /// the running scan's queue, or (if gitignored) get linted right away.
    fn focus(&mut self, params: &Value) {
        let paths = |key: &str| -> Vec<PathBuf> {
            params[key].as_array().into_iter().flatten().filter_map(|u| u.as_str().and_then(uri::to_path)).collect()
        };
        self.visible = paths("visible");
        let (old_tabs, old_ignored) = (std::mem::take(&mut self.tabs), std::mem::take(&mut self.ignored_tabs));
        let mut new_tabs = Vec::new();
        for tab in paths("tabs") {
            let known = old_tabs.contains(&tab) || old_ignored.contains(&tab);
            if known {
                if old_ignored.contains(&tab) {
                    self.ignored_tabs.insert(tab);
                } else {
                    self.tabs.push(tab);
                }
            } else {
                new_tabs.push(tab);
            }
        }
        for gone in old_ignored.iter().filter(|t| !self.ignored_tabs.contains(*t)) {
            self.drop_ignored_tab(gone);
        }
        let mut scan_next = Vec::new();
        for tab in new_tabs {
            self.add_tab(tab, &mut scan_next);
        }
        self.shared.prioritize(scan_next);
    }

    /// Sorts a tab: scans reach it, or it is gitignored under a gitignored
    /// config and linted here, or it is gitignored and never linted.
    fn add_tab(&mut self, tab: PathBuf, scan_next: &mut Vec<PathBuf>) {
        if scan::walk_reaches(&self.roots, &tab) {
            scan_next.push(tab.clone());
            self.tabs.push(tab);
        } else if let Some(config) = self.editor_config(&tab) {
            self.want_config(config);
            self.shared.editor.lock().expect("lock").insert(tab.clone());
            self.dirty_tabs.insert(tab.clone());
            self.ignored_tabs.insert(tab);
        }
    }

    /// A gitignored tab was closed: nothing lints it anymore.
    fn drop_ignored_tab(&self, tab: &Path) {
        if self.docs.contains_key(tab) {
            return;
        }
        self.shared.editor.lock().expect("lock").remove(tab);
        self.shared.publish(&self.out, &uri::from_path(tab), None, json!([]));
    }

    /// Re-sorts open buffers and tabs, after `.gitignore` or configs moved.
    fn reclassify_editor(&mut self) {
        let lint: Vec<(PathBuf, bool)> = self.docs.keys().map(|p| (p.clone(), self.editor_config(p).is_some())).collect();
        for (path, lint) in lint {
            if let Some(doc) = self.docs.get_mut(&path) {
                doc.lint = lint;
            }
            self.dirty.insert(path);
        }
        let tabs: Vec<PathBuf> = std::mem::take(&mut self.tabs).into_iter().chain(self.ignored_tabs.iter().cloned()).collect();
        for gone in std::mem::take(&mut self.ignored_tabs) {
            self.drop_ignored_tab(&gone);
        }
        let mut scan_next = Vec::new();
        for tab in tabs {
            self.add_tab(tab, &mut scan_next);
        }
        // A rediscovery rescans with the new tabs anyway.
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
        self.inspect_hover = params["initializationOptions"]["inspectHover"].as_bool().unwrap_or(false);
        json!({
            "capabilities": {
                "textDocumentSync": { "openClose": true, "change": 2 },
                "codeActionProvider": { "codeActionKinds": ["quickfix", "source.fixAll.dx"] },
                "documentFormattingProvider": true,
                "hoverProvider": true,
                "definitionProvider": true,
                "referencesProvider": true,
                "documentSymbolProvider": true,
                "completionProvider": { "triggerCharacters": [".", " ", "\"", "["] },
            },
            "serverInfo": { "name": "dxlint", "version": env!("CARGO_PKG_VERSION") },
        })
    }

    fn show(&self, kind: u8, message: &str) {
        self.out.notify("window/showMessage", json!({ "type": kind, "message": message }));
    }

    /// Loads stale configs, or every config after a rediscovery. When the
    /// linters changed, marks open buffers and ignored tabs for relinting.
    /// Returns whether the workspace needs a rescan: a config scans can reach
    /// changed (gitignored configs only matter to the editor).
    fn reload_configs(&mut self) -> bool {
        let mut rescan = false;
        if std::mem::take(&mut self.rediscover) {
            self.reclassify_editor();
            let found = if self.scan_workspace {
                projects::discover(&self.roots)
            } else {
                self.roots.iter().filter_map(|r| config::find(r)).collect()
            };
            let editor: Vec<PathBuf> = self.docs.keys().chain(&self.ignored_tabs).filter_map(|p| self.editor_config(p)).collect();
            // Reload everything kept, not just what was stale.
            self.stale.extend(found.into_iter().chain(editor));
            rescan = self.projects.retain(&self.stale);
            if self.stale.is_empty() && !self.warned_no_config {
                self.warned_no_config = true;
                self.show(3, &format!("dxlint: no {} found in the workspace; linting is off", config::CONFIG_NAME));
            }
        }
        let configs = std::mem::take(&mut self.stale);
        if configs.is_empty() && !rescan {
            return false;
        }
        let mut changed = rescan;
        let docs = &self.docs;
        let uri_of = |p: &Path| docs.get(p).map(|d| d.uri.clone()).unwrap_or_else(|| uri::from_path(p));
        for config in &configs {
            if self.projects.load(config, &Overlay(docs), &uri_of) {
                changed = true;
                rescan |= scan::walk_reaches(&self.roots, config);
            }
        }
        self.projects.publish_errors(&self.out);

        // A broken config keeps its last good linter, so nothing to relint.
        if changed {
            self.dirty.extend(self.docs.keys().cloned());
            self.dirty_tabs.extend(self.ignored_tabs.iter().cloned());
        }
        rescan
    }

    /// A `.dx` file changed in the editor or on disk: reload what reads it.
    fn config_touched(&mut self, path: &Path) {
        self.stale.extend(self.projects.owners_of(path));
    }

    fn publish_doc(&self, path: &Path) {
        let Some(doc) = self.docs.get(path) else { return };
        if self.projects.has_errors(&doc.uri) {
            return;
        }
        let diags = match self.projects.linter_for(path).filter(|_| doc.lint) {
            Some(l) => convert::diagnostics(&doc.text, &self.shared.lint(l, path, &doc.text), l),
            None => json!([]),
        };
        self.shared.publish(&self.out, &doc.uri, Some(doc.version), diags);
    }

    /// Lints a gitignored tab from disk, unless it is open as a buffer.
    fn publish_ignored_tab(&self, path: &Path) {
        if self.docs.contains_key(path) || !self.ignored_tabs.contains(path) {
            return;
        }
        self.publish_from_disk(path, &uri::from_path(path));
    }

    fn publish_from_disk(&self, path: &Path, u: &str) {
        let diags = match self.projects.linter_for(path) {
            Some(l) if l.wants(path) => match std::fs::read_to_string(path) {
                Ok(text) => convert::diagnostics(&text, &self.shared.lint(l, path, &text), l),
                Err(_) => json!([]),
            },
            _ => json!([]),
        };
        self.shared.publish(&self.out, u, None, diags);
    }

    /// A file the user does not have open changed on disk, or was just
    /// closed: relint it like a scan would. Scans never lint gitignored files.
    fn publish_disk(&self, path: &Path, u: &str) {
        if self.ignored_tabs.contains(path) {
            self.publish_ignored_tab(path);
        } else if self.scan_workspace
            && self.projects.linter_for(path).is_some_and(|l| l.wants(path))
            && scan::walk_reaches(&self.roots, path)
        {
            self.publish_from_disk(path, u);
        } else {
            self.shared.publish(&self.out, u, None, json!([]));
        }
    }

    fn doc_path(params: &Value) -> Option<(PathBuf, String)> {
        let u = params["textDocument"]["uri"].as_str()?;
        Some((uri::to_path(u)?, u.to_string()))
    }

    fn did_open(&mut self, params: &Value) {
        let Some((path, u)) = Self::doc_path(params) else { return };
        let td = &params["textDocument"];
        let config = if self.projects.is_config(&path) { None } else { self.editor_config(&path) };
        let text = td["text"].as_str().unwrap_or_default().to_string();
        let doc = Doc { uri: u, text, version: td["version"].as_i64().unwrap_or(0), lint: config.is_some() };
        self.docs.insert(path.clone(), doc);
        self.shared.editor.lock().expect("lock").insert(path.clone());
        if self.projects.is_config(&path) {
            self.config_touched(&path);
            return;
        }
        // Files outside the workspace, or under configs not loaded yet.
        if let Some(config) = config {
            self.want_config(config);
        }
        self.source_changed(&path);
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
            self.source_changed(&path);
            self.dirty.insert(path);
        }
    }

    fn did_close(&mut self, params: &Value) {
        let Some((path, u)) = Self::doc_path(params) else { return };
        self.docs.remove(&path);
        // A gitignored tab stays the request loop's to lint.
        if !self.ignored_tabs.contains(&path) {
            self.shared.editor.lock().expect("lock").remove(&path);
        }
        if self.projects.is_config(&path) {
            self.config_touched(&path);
        } else {
            // Back to the disk's text, for this file and what reads it.
            self.source_changed(&path);
            self.publish_disk(&path, &u);
        }
    }

    fn watched_files(&mut self, params: &Value) {
        for change in params["changes"].as_array().into_iter().flatten() {
            let Some(u) = change["uri"].as_str() else { continue };
            let Some(path) = uri::to_path(u) else { continue };
            // 1 = created, 3 = deleted
            let kind = change["type"].as_u64();
            let name = path.file_name();
            if name.is_some_and(|n| n == config::CONFIG_NAME) && matches!(kind, Some(1 | 3)) {
                // Which config is nearest may have changed for any file.
                self.rediscover = true;
            } else if name.is_some_and(|n| n == ".gitignore") {
                // What scans reach, and which editor files lint, may have changed.
                self.rediscover = true;
            } else if self.projects.is_config(&path) {
                self.config_touched(&path);
            } else if !self.docs.contains_key(&path) {
                self.source_changed(&path);
                if kind == Some(3) {
                    self.shared.reads.lock().expect("lock").remove(&path);
                    self.shared.publish(&self.out, u, None, json!([]));
                } else {
                    self.publish_disk(&path, u);
                }
            }
        }
    }

    fn fix_all(&self, path: &Path) -> Option<Value> {
        let doc = self.docs.get(path).filter(|d| d.lint)?;
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
            if d["source"] != "dxlint" {
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
        if (fixable > 0 && wants("quickfix")) || (!only.is_empty() && wants("source.fixAll.dx")) {
            if let Some(edit) = self.fix_all(&path) {
                let kind = if wants("source.fixAll.dx") && !only.is_empty() { "source.fixAll.dx" } else { "quickfix" };
                actions.push(json!({
                    "title": "Fix all dxlint problems in this file",
                    "kind": kind,
                    "edit": { "changes": { u: [edit] } },
                }));
            }
        }
        Value::Array(actions)
    }

    /// Runs `f` on the `.dx` file of a request (null elsewhere).
    fn in_config(&self, params: &Value, f: impl FnOnce(&config_docs::ConfigFile) -> Value) -> Value {
        let Some((path, _)) = Self::doc_path(params) else { return Value::Null };
        match self.projects.config_file(&path) {
            Some((symbols, file)) => f(&config_docs::ConfigFile { symbols: &symbols, file }),
            None => Value::Null,
        }
    }

    /// In `.dx` files: what the name is. In source files, when enabled:
    /// the layers and facts at the cursor.
    fn hover(&self, params: &Value) -> Value {
        let Some((path, _)) = Self::doc_path(params) else { return Value::Null };
        if let Some((symbols, file)) = self.projects.config_file(&path) {
            return config_docs::ConfigFile { symbols: &symbols, file }.hover(params);
        }
        if !self.inspect_hover {
            return Value::Null;
        }
        let Some(linter) = self.projects.linter_for(&path) else { return Value::Null };
        let text = match self.docs.get(&path) {
            Some(doc) => doc.text.clone(),
            None => match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => return Value::Null,
            },
        };
        let pos = &params["position"];
        let lines = LineIndex::new(&text);
        let offset = lines.from_utf16(&text, pos["line"].as_u64().unwrap_or(0) as u32, pos["character"].as_u64().unwrap_or(0) as u32);
        match linter.inspect(&path, &text, offset) {
            Some(md) => json!({ "contents": { "kind": "markdown", "value": format!("**dxlint**\n\n{md}") } }),
            None => Value::Null,
        }
    }

    fn format(&self, params: &Value) -> Value {
        let Some((path, _)) = Self::doc_path(params) else { return Value::Null };
        match self.fix_all(&path) {
            Some(edit) => json!([edit]),
            None => json!([]),
        }
    }
}
