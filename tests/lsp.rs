//! Drives `zlint lsp` over stdio like an editor would.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

const CONFIG: &str = r#"
lang t "*.t"
match paren /\(/ until /\)/
in .t or paren match paren
in .t or paren match str /"/ until /"/
in .t match bad /\bBAD\b/
in bad rule no-bad warn "no BAD here" fix "good"
"#;

struct Client {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: i64,
}

fn uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.trim_start_matches('/');
    format!("file:///{}", s.replace(' ', "%20"))
}

fn same_uri(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace("%3A", ":").replace("%3a", ":").to_lowercase();
    norm(a) == norm(b)
}

fn read_message(r: &mut impl BufRead) -> Option<Value> {
    let mut len = 0;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok()?;
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

impl Client {
    fn start(root: &Path) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zlint"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn zlint lsp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout: ChildStdout = child.stdout.take().expect("stdout");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            while let Some(msg) = read_message(&mut r) {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });
        let mut c = Client { child, stdin, rx, next_id: 0 };
        let init = c.request("initialize", json!({ "rootUri": uri(root), "capabilities": { "window": { "workDoneProgress": true } } }));
        assert_eq!(init["capabilities"]["documentFormattingProvider"], true);
        c.notify("initialized", json!({}));
        c
    }

    fn send(&mut self, msg: Value) {
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write");
        self.stdin.flush().expect("flush");
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Next message matching `pred`, skipping everything else.
    fn wait(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        loop {
            match self.rx.recv_timeout(Duration::from_secs(20)) {
                Ok(msg) if pred(&msg) => return msg,
                Ok(_) => {}
                Err(_) => panic!("timed out waiting for {what}"),
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let msg = self.wait(method, |m| m["id"] == id && m.get("method").is_none());
        msg["result"].clone()
    }

    fn diagnostics_for(&mut self, file: &Path) -> Vec<Value> {
        let u = uri(file);
        let msg = self.wait("publishDiagnostics", |m| {
            m["method"] == "textDocument/publishDiagnostics" && same_uri(m["params"]["uri"].as_str().unwrap_or(""), &u)
        });
        msg["params"]["diagnostics"].as_array().cloned().unwrap_or_default()
    }

    /// Diagnostics for each file, in whatever order the scan publishes them.
    fn diagnostics_for_each(&mut self, files: &[&Path]) -> Vec<Vec<Value>> {
        let uris: Vec<String> = files.iter().map(|f| uri(f)).collect();
        let mut found: Vec<Option<Vec<Value>>> = vec![None; files.len()];
        while found.iter().any(Option::is_none) {
            let msg = self.wait("publishDiagnostics", |m| m["method"] == "textDocument/publishDiagnostics");
            let got = msg["params"]["uri"].as_str().unwrap_or("");
            if let Some(i) = uris.iter().position(|u| same_uri(got, u)) {
                found[i] = Some(msg["params"]["diagnostics"].as_array().cloned().unwrap_or_default());
            }
        }
        found.into_iter().flatten().collect()
    }

    fn shutdown(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        let status = self.child.wait().expect("exit");
        assert!(status.success());
    }
}

fn project(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    std::fs::write(dir.join("zoomie.zlint"), CONFIG).expect("config");
    dir
}

#[test]
fn streams_workspace_diagnostics_and_serves_fixes() {
    let dir = project("lsp-stream");
    let on_disk = dir.join("disk.t");
    std::fs::write(&on_disk, "BAD \"BAD\" (BAD)\n").expect("write");
    let mut c = Client::start(&dir);

    // The background scan streams the unopened file. ad only lives at the
    // top level, so the ones in the string and the parens do not count.
    let diags = c.diagnostics_for(&on_disk);
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert_eq!(diags[0]["code"], "no-bad");
    assert_eq!(diags[0]["range"]["start"]["character"], 0);

    // Open a buffer with unsaved content; it is linted from the buffer.
    let open = dir.join("open.t");
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri(&open), "languageId": "t", "version": 1, "text": "ok\nBAD\n" } }),
    );
    let diags = c.diagnostics_for(&open);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0]["range"]["start"]["line"], 1);

    // Quick fix comes straight from the diagnostic's data.
    let actions = c.request(
        "textDocument/codeAction",
        json!({ "textDocument": { "uri": uri(&open) }, "range": diags[0]["range"], "context": { "diagnostics": diags } }),
    );
    let fix = actions.as_array().unwrap().iter().find(|a| a["kind"] == "quickfix").expect("quickfix");
    let edits = fix["edit"]["changes"].as_object().unwrap().values().next().unwrap();
    assert_eq!(edits[0]["newText"], "good");

    // Incremental edit: replace `BAD` on line 1 with `fine`.
    c.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri(&open), "version": 2 },
            "contentChanges": [{ "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 3 } }, "text": "fine BAD" }]
        }),
    );
    let diags = c.diagnostics_for(&open);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0]["range"]["start"]["character"], 5);

    // Formatting applies every fix.
    let edits = c.request("textDocument/formatting", json!({ "textDocument": { "uri": uri(&open) }, "options": {} }));
    assert_eq!(edits[0]["newText"], "ok\nfine good\n");

    c.shutdown();
}

#[test]
fn nested_configs_lint_their_own_files() {
    let dir = project("lsp-nested");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).expect("sub dir");
    std::fs::write(sub.join("zoomie.zlint"), CONFIG.replace("no BAD here", "nested says no")).expect("nested config");
    let top = dir.join("top.t");
    let inner = sub.join("inner.t");
    std::fs::write(&top, "BAD\n").expect("write");
    std::fs::write(&inner, "BAD\n").expect("write");
    let mut c = Client::start(&dir);

    // The scan lints each file with its nearest config.
    let diags = c.diagnostics_for_each(&[&top, &inner]);
    assert_eq!(diags[0][0]["message"], "no BAD here");
    assert_eq!(diags[1][0]["message"], "nested says no");

    // So does an open buffer.
    let open = sub.join("open.t");
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri(&open), "languageId": "t", "version": 1, "text": "BAD\n" } }),
    );
    assert_eq!(c.diagnostics_for(&open)[0]["message"], "nested says no");
    c.shutdown();
}

#[test]
fn scan_reaches_configs_below_uncovered_and_ignored_dirs() {
    // The workspace root has no config; `proj` has one that ignores
    // `skipped`, which still holds a nested config of its own.
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lsp-skip");
    let _ = std::fs::remove_dir_all(&dir);
    let deep = dir.join("proj").join("skipped").join("sub");
    std::fs::create_dir_all(&deep).expect("dirs");
    std::fs::write(dir.join("proj").join("zoomie.zlint"), format!("{CONFIG}\nignore \"skipped\"\n")).expect("config");
    std::fs::write(deep.join("zoomie.zlint"), CONFIG.replace("no BAD here", "deep says no")).expect("deep config");
    let file = deep.join("f.t");
    std::fs::write(&file, "BAD\n").expect("write");
    let mut c = Client::start(&dir);
    assert_eq!(c.diagnostics_for(&file)[0]["message"], "deep says no");
    c.shutdown();
}

#[test]
fn gitignored_files_lint_only_under_gitignored_configs() {
    let dir = project("lsp-ignored");
    // `.gitignore` only applies inside a repository.
    std::fs::create_dir_all(dir.join(".git")).expect(".git");
    std::fs::write(dir.join(".gitignore"), "/scratch\n").expect("gitignore");
    let scratch = dir.join("scratch").join("s");
    std::fs::create_dir_all(&scratch).expect("dirs");
    std::fs::write(scratch.join("zoomie.zlint"), CONFIG.replace("no BAD here", "scratch says no")).expect("config");
    let tab = scratch.join("tab.t");
    std::fs::write(&tab, "BAD\n").expect("write");
    let mut c = Client::start(&dir);

    // A tab that was never opened, under a gitignored config, uses it.
    c.notify("zlint/focus", json!({ "visible": [], "tabs": [uri(&tab)] }));
    assert_eq!(c.diagnostics_for(&tab)[0]["message"], "scratch says no");

    // Buffers: a gitignored file is only linted (and fixed) by a gitignored
    // config, never by the tracked root one.
    let mut format = |path: &Path| {
        c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(path), "languageId": "t", "version": 1, "text": "BAD\n" } }));
        c.request("textDocument/formatting", json!({ "textDocument": { "uri": uri(path) }, "options": {} }))
    };
    assert_eq!(format(&dir.join("scratch").join("plain.t")), json!([]));
    assert_eq!(format(&scratch.join("open.t"))[0]["newText"], "good\n");
    c.shutdown();
}

#[test]
fn config_errors_show_up_in_the_config_and_recover() {
    let dir = project("lsp-config");
    let file = dir.join("a.t");
    std::fs::write(&file, "BAD\n").expect("write");
    let config = dir.join("zoomie.zlint");
    let mut c = Client::start(&dir);
    assert_eq!(c.diagnostics_for(&file).len(), 1);

    // A broken unsaved config reports in place.
    let broken = CONFIG.replace("in bad rule", "in bas rule");
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri(&config), "languageId": "zlint", "version": 1, "text": broken } }),
    );
    let errors = c.diagnostics_for(&config);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(errors[0]["message"].as_str().unwrap().contains("did you mean `bad`?"));

    // Fixing it clears the errors and relints with the new rules.
    let renamed = CONFIG.replace("no BAD here", "BAD is not allowed");
    c.notify(
        "textDocument/didChange",
        json!({ "textDocument": { "uri": uri(&config), "version": 2 }, "contentChanges": [{ "text": renamed }] }),
    );
    assert!(c.diagnostics_for(&config).is_empty());
    let diags = c.diagnostics_for(&file);
    assert_eq!(diags[0]["message"], "BAD is not allowed");
    c.shutdown();
}
