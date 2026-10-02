//! Drives `zlint lsp` over stdio like an editor would.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

const CONFIG: &str = r#"
lang t = "*.t"
match paren = /\(/ * /\)/
in .t | paren match paren
in .t | paren match str = /"/ * /"/
in .t match bad = /\bBAD\b/
format no-bad on bad "no BAD here" = "good"
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
fn config_errors_show_up_in_the_config_and_recover() {
    let dir = project("lsp-config");
    let file = dir.join("a.t");
    std::fs::write(&file, "BAD\n").expect("write");
    let config = dir.join("zoomie.zlint");
    let mut c = Client::start(&dir);
    assert_eq!(c.diagnostics_for(&file).len(), 1);

    // A broken unsaved config reports in place.
    let broken = CONFIG.replace("format no-bad on bad", "format no-bad on bas");
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
