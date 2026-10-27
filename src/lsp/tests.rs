//! Drives the LSP entry over framed streams like an editor would.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};

const CONFIG: &str = r#"
lang t "*.t"
match paren /\(/ until /\)/
in .t, paren match paren
in .t, paren match str /"/ until /"/
in .t match bad /\bBAD\b/
in bad rule no-bad warn "no BAD here" fix as "good"
"#;

struct Client {
    server: JoinHandle<u8>,
    stdin: PipeWriter,
    rx: Receiver<Value>,
    next_id: i64,
}

struct PipeReader {
    rx: Receiver<Vec<u8>>,
    pending: io::Cursor<Vec<u8>>,
}

impl Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let n = self.pending.read(buf)?;
            if n != 0 {
                return Ok(n);
            }
            match self.rx.recv() {
                Ok(bytes) => self.pending = io::Cursor::new(bytes),
                Err(_) => return Ok(0),
            }
        }
    }
}

struct PipeWriter(Sender<Vec<u8>>);

impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.send(buf.to_vec()).map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn pipe() -> (PipeWriter, PipeReader) {
    let (tx, rx) = mpsc::channel();
    (PipeWriter(tx), PipeReader { rx, pending: io::Cursor::new(Vec::new()) })
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
        let (stdin, input) = pipe();
        let (output, stdout) = pipe();
        let server = std::thread::spawn(move || super::run(input, output, &mut io::sink()));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            while let Some(msg) = read_message(&mut r) {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });
        let mut c = Client { server, stdin, rx, next_id: 0 };
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

    /// [`Client::diagnostics_for`] `file`, also returning every diagnostics
    /// message for `watched` that arrived meanwhile.
    fn diagnostics_watching(&mut self, file: &Path, watched: &Path) -> (Vec<Value>, Vec<Value>) {
        let (u, w) = (uri(file), uri(watched));
        let mut seen = Vec::new();
        loop {
            let msg = self.wait("publishDiagnostics", |m| m["method"] == "textDocument/publishDiagnostics");
            let got = msg["params"]["uri"].as_str().unwrap_or("");
            let diags = msg["params"]["diagnostics"].as_array().cloned().unwrap_or_default();
            if same_uri(got, &w) { seen.push(Value::Array(diags.clone())) }
            if same_uri(got, &u) { return (diags, seen) }
        }
    }

    /// Writes `text` to `file` and tells the server the editor saved it.
    fn save(&mut self, file: &Path, text: &str) {
        std::fs::write(file, text).expect("write");
        self.notify("textDocument/didSave", json!({ "textDocument": { "uri": uri(file) } }));
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
        drop(self.stdin);
        let code = self.server.join().expect("exit");
        assert_eq!(code, 0);
    }
}

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("dxlint-lsp-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    std::fs::write(dir.join(".dx"), CONFIG).expect("config");
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
fn rescans_leave_open_buffers_diagnostics_alone() {
    let dir = project("lsp-rescan-open");
    let config = dir.join(".dx");
    let mut c = Client::start(&dir);
    let scan_end = |m: &Value| m["method"] == "$/progress" && m["params"]["value"]["kind"] == "end";
    c.wait("first scan", scan_end);

    let open = dir.join("open.t");
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(&open), "languageId": "t", "version": 1, "text": "BAD\n" } }));
    assert_eq!(c.diagnostics_for(&open).len(), 1);

    // A config change rescans the workspace; the buffer is the editor's, so
    // the scan's sweep of stale files must not clear it.
    std::fs::write(&config, format!("{CONFIG}# touched\n")).expect("config");
    c.notify("workspace/didChangeWatchedFiles", json!({ "changes": [{ "uri": uri(&config), "type": 2 }] }));
    let u = uri(&open);
    loop {
        let m = c.wait("rescan", |m| scan_end(m) || m["method"] == "textDocument/publishDiagnostics");
        if scan_end(&m) {
            break;
        }
        if same_uri(m["params"]["uri"].as_str().unwrap_or(""), &u) {
            assert_eq!(m["params"]["diagnostics"].as_array().map(Vec::len), Some(1), "{m:#}");
        }
    }
    c.shutdown();
}

#[test]
fn dx_files_have_hover_definitions_references_and_completion() {
    let dir = project("lsp-config-docs");
    let config = dir.join(".dx");
    let file = dir.join("f.t");
    std::fs::write(&file, "BAD\n").expect("write");
    let mut c = Client::start(&dir);

    // Diagnostics link their rule's definition.
    let diags = c.diagnostics_for(&file);
    let href = diags[0]["codeDescription"]["href"].as_str().unwrap_or_default();
    assert!(href.ends_with(".dx#L7"), "{href}");

    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(&config), "languageId": "dxlint", "version": 1, "text": format!("{CONFIG}rule ") } }));
    let at = |line: u32, character: u32| json!({ "textDocument": { "uri": uri(&config) }, "position": { "line": line, "character": character } });

    // `bad` in `in bad rule no-bad`: the layer, defined a line above.
    let hover = c.request("textDocument/hover", at(6, 4));
    let text = hover["contents"]["value"].as_str().unwrap_or_default();
    assert!(text.contains("**layer** `bad`") && text.contains("no-bad"), "{text}");
    let defs = c.request("textDocument/definition", at(6, 4));
    assert_eq!(defs[0]["range"]["start"]["line"], 5, "{defs:#}");
    assert!(same_uri(defs[0]["uri"].as_str().unwrap_or_default(), &uri(&config)));
    let refs = c.request("textDocument/references", json!({ "textDocument": { "uri": uri(&config) }, "position": { "line": 6, "character": 4 }, "context": { "includeDeclaration": false } }));
    assert_eq!(refs.as_array().map(Vec::len), Some(1), "{refs:#}");

    // Keywords explain themselves; layer lists complete layers.
    let hover = c.request("textDocument/hover", at(6, 9));
    assert!(hover["contents"]["value"].as_str().unwrap_or_default().contains("**keyword** `rule`"), "{hover:#}");
    let items = c.request("textDocument/completion", at(6, 3));
    let labels: Vec<&str> = items["items"].as_array().into_iter().flatten().filter_map(|i| i["label"].as_str()).collect();
    assert!(labels.contains(&"paren") && labels.contains(&"t"), "{labels:?}");
    // `rule ` without `in` turns a defined rule on: rules complete.
    let items = c.request("textDocument/completion", at(7, 5));
    let labels: Vec<&str> = items["items"].as_array().into_iter().flatten().filter_map(|i| i["label"].as_str()).collect();
    assert_eq!(labels, vec!["no-bad"], "{items:#}");

    // The outline lists what the file defines.
    let outline = c.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri(&config) } }));
    let names: Vec<&str> = outline.as_array().into_iter().flatten().filter_map(|s| s["name"].as_str()).collect();
    assert!(names.contains(&"no-bad") && names.contains(&"str"), "{names:?}");
    c.shutdown();
}

#[test]
fn rule_lines_complete_each_variants_default_option() {
    let dir = project("lsp-default-options");
    let config = dir.join(".dx");
    let variants = "schema dangle_mode enum(never, always-multiline)\n\
        define {\n\
        in bad rule dangle:arrays warn \"x\" { default option mode dangle_mode = never }\n\
        in bad rule dangle:objects warn \"x\" { default option mode enum(never, only-multiline) = never }\n\
        in bad rule plain warn \"x\" { option mode enum(a, b) = a }\n\
        }\n";
    let base = format!("{CONFIG}{variants}");
    std::fs::write(&config, &base).expect("config");
    let mut c = Client::start(&dir);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(&config), "languageId": "dxlint", "version": 1, "text": base } }));
    // Completes at the end of `typed`'s line `row`, typed after the rules.
    let mut version = 1;
    let mut labels_at = |typed: &str, row: usize| -> Vec<String> {        version += 1;
        let text = format!("{base}{typed}\n");
        c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri(&config), "version": version }, "contentChanges": [{ "text": text }] }));
        let cursor = typed.lines().nth(row).unwrap_or_default();
        let at = json!({ "textDocument": { "uri": uri(&config) }, "position": { "line": base.lines().count() + row, "character": cursor.len() } });
        let items = c.request("textDocument/completion", at);
        items["items"].as_array().into_iter().flatten().filter_map(|i| i["label"].as_str().map(String::from)).collect()
    };
    // After the name: severities, plus `set` only for rules with a default option.
    let after_name = labels_at("rule dangle:arrays ", 0);
    assert!(after_name.iter().any(|l| l == "error") && after_name.iter().any(|l| l == "set"), "{after_name:?}");
    let plain = labels_at("rule plain ", 0);
    assert!(plain.iter().any(|l| l == "error") && !plain.iter().any(|l| l == "set"), "{plain:?}");
    // After `set`: the words of that variant's own default option, also by
    // name inside the variant's block.
    assert_eq!(labels_at("rule dangle:arrays error set ", 0), ["never", "always-multiline"]);
    assert_eq!(labels_at("rule dangle:objects set ", 0), ["never", "only-multiline"]);
    // Half-typed `set` lines break their rule's build; it still completes.
    assert_eq!(labels_at("rule dangle:objects {\n  set mode \n}", 1), ["never", "only-multiline"]);
    assert_eq!(labels_at("rule dangle:objects {\n  set \n}", 1), ["mode"]);
    c.shutdown();
}

#[test]
fn editing_an_export_relints_its_importers() {
    let dir = std::env::temp_dir().join("dxlint-lsp-tests").join("lsp-cross-file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    std::fs::write(dir.join(".dx"), "import .ts\nin ts_identifier rule typed info \"{text}: {if node always number then 'number' else if node always string then 'string' else if node always boolean then 'boolean' else 'unknown'}\" { when reference_kind == \"import\" }\n")
        .expect("config");
    let a = dir.join("a.ts");
    let open = dir.join("b.ts");
    let closed = dir.join("c.ts");
    let importer = "import { x } from \"./a\";\nx;\n";
    std::fs::write(&a, "export const x: number = 1;\n").expect("write");
    std::fs::write(&open, importer).expect("write");
    std::fs::write(&closed, importer).expect("write");
    let mut c = Client::start(&dir);
    assert_eq!(c.diagnostics_for_each(&[&open, &closed]).iter().map(|d| d[0]["message"].clone()).collect::<Vec<_>>(), ["x: number", "x: number"]);

    // Unsaved edits to `a` reach the open importer as you type; the closed
    // one waits for the save, and sees only the saved text.
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(&open), "languageId": "typescript", "version": 1, "text": importer } }));
    assert_eq!(c.diagnostics_for(&open)[0]["message"], "x: number");
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(&a), "languageId": "typescript", "version": 1, "text": "export const x: number = 1;\n" } }));
    let edit = |c: &mut Client, version: i64, text: &str| c.notify(
        "textDocument/didChange",
        json!({ "textDocument": { "uri": uri(&a), "version": version }, "contentChanges": [{ "text": text }] }),
    );
    edit(&mut c, 2, "export const x: string = \"\";\n");
    let (diags, mut early) = c.diagnostics_watching(&open, &closed);
    assert_eq!(diags[0]["message"], "x: string");
    edit(&mut c, 3, "export const x: boolean = true;\n");
    let (diags, more) = c.diagnostics_watching(&open, &closed);
    early.extend(more);
    assert_eq!(diags[0]["message"], "x: boolean", "{early:?}");
    assert!(early.is_empty(), "the closed importer relinted before the save: {early:?}");
    c.save(&a, "export const x: boolean = true;\n");
    assert_eq!(c.diagnostics_for(&closed)[0]["message"], "x: boolean");

    // Closing without saving goes back to what the disk says.
    edit(&mut c, 4, "export const x: string = \"\";\n");
    assert_eq!(c.diagnostics_for(&open)[0]["message"], "x: string");
    c.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri(&a) } }));
    assert_eq!(c.diagnostics_for(&open)[0]["message"], "x: boolean");
    c.shutdown();
}

#[test]
fn editing_a_file_deep_in_an_import_chain_relints_its_far_importers() {
    let dir = std::env::temp_dir().join("dxlint-lsp-tests").join("lsp-chain");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    std::fs::write(dir.join(".dx"), "import .ts\nin ts_identifier rule typed info \"{text}: {if node always number then 'number' else if node always string then 'string' else 'unknown'}\" { when reference_kind == \"import\" }\n")
        .expect("config");
    let a = dir.join("a.ts");
    let d = dir.join("d.ts");
    std::fs::write(&a, "import { x } from \"./b\";\nx;\n").expect("write");
    std::fs::write(dir.join("b.ts"), "export { v as x } from \"./c\";\n").expect("write");
    std::fs::write(dir.join("c.ts"), "import { w } from \"./d\";\nexport const v = w;\n").expect("write");
    std::fs::write(&d, "export declare const w: number;\n").expect("write");
    let mut c = Client::start(&dir);
    assert_eq!(c.diagnostics_for(&a)[0]["message"], "x: number");

    // Saving an edit three imports away reaches `a`, which is not open.
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(&d), "languageId": "typescript", "version": 1, "text": "export declare const w: number;\n" } }));
    c.notify(
        "textDocument/didChange",
        json!({ "textDocument": { "uri": uri(&d), "version": 2 }, "contentChanges": [{ "text": "export declare const w: string;\n" }] }),
    );
    c.save(&d, "export declare const w: string;\n");
    assert_eq!(c.diagnostics_for(&a)[0]["message"], "x: string");
    c.shutdown();
}

#[test]
fn a_file_starting_to_declare_a_global_relints_its_readers() {
    let dir = std::env::temp_dir().join("dxlint-lsp-tests").join("lsp-global");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("node_modules/typescript/lib")).expect("tmp dir");
    std::fs::write(dir.join(".dx"), "import .ts\nignore \"node_modules\"\nin ts_identifier rule typed info \"{text}: {if node always boolean then 'boolean' else 'unknown'}\" { when text == \"G\" && parent is ts_expression_statement }\n")
        .expect("config");
    // Globals are read only once TypeScript's default libraries are there.
    std::fs::write(dir.join("node_modules/typescript/lib/lib.es2023.d.ts"), "interface Array<T> {}\n").expect("write");
    std::fs::write(dir.join("node_modules/typescript/lib/lib.dom.d.ts"), "interface Window {}\n").expect("write");
    let a = dir.join("a.ts");
    let mention = dir.join("mention.ts");
    let before = "export function f() { return \"G\"; }\n";
    std::fs::write(&a, "G;\n").expect("write");
    std::fs::write(&mention, before).expect("write");
    let mut c = Client::start(&dir);
    assert_eq!(c.diagnostics_for(&a)[0]["message"], "G: unknown");

    // `mention` only spelled G: its global lookup holds until it declares it.
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(&mention), "languageId": "typescript", "version": 1, "text": before } }));
    let declares = format!("{before}declare global {{ const G: boolean }}\n");
    c.notify(
        "textDocument/didChange",
        json!({ "textDocument": { "uri": uri(&mention), "version": 2 }, "contentChanges": [{ "text": declares }] }),
    );
    c.save(&mention, &declares);
    assert_eq!(c.diagnostics_for(&a)[0]["message"], "G: boolean");

    // Changed on disk by something else: read again at once.
    c.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri(&mention) } }));
    std::fs::write(&mention, before).expect("write");
    c.notify("workspace/didChangeWatchedFiles", json!({ "changes": [{ "uri": uri(&mention), "type": 2 }] }));
    assert_eq!(c.diagnostics_for(&a)[0]["message"], "G: unknown");
    c.shutdown();
}

#[test]
fn nested_configs_lint_their_own_files() {
    let dir = project("lsp-nested");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).expect("sub dir");
    std::fs::write(sub.join(".dx"), CONFIG.replace("no BAD here", "nested says no")).expect("nested config");
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
fn nested_configs_importing_their_parent_lint_with_its_linter_and_reload_it() {
    let dir = project("lsp-folded");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).expect("sub dir");
    let nested = sub.join(".dx");
    std::fs::write(&nested, "import ../.dx\nrule no-bad error\n").expect("nested config");
    let top = dir.join("top.t");
    let inner = sub.join("inner.t");
    std::fs::write(&top, "BAD\n").expect("write");
    std::fs::write(&inner, "BAD\n").expect("write");
    let mut c = Client::start(&dir);

    // LSP severities: 1 error, 2 warning, 3 information.
    let diags = c.diagnostics_for_each(&[&top, &inner]);
    assert_eq!((&diags[0][0]["severity"], &diags[1][0]["severity"]), (&json!(2), &json!(1)));

    // Editing the nested config reloads the parent it folded into.
    std::fs::write(&nested, "import ../.dx\nrule no-bad info\n").expect("nested config");
    c.notify("workspace/didChangeWatchedFiles", json!({ "changes": [{ "uri": uri(&nested), "type": 2 }] }));
    assert_eq!(c.diagnostics_for(&inner)[0]["severity"], json!(3));
    c.shutdown();
}

#[test]
fn scan_reaches_configs_below_uncovered_and_ignored_dirs() {
    // The workspace root has no config; `proj` has one that ignores
    // `skipped`, which still holds a nested config of its own.
    let dir = std::env::temp_dir().join("dxlint-lsp-tests").join("lsp-skip");
    let _ = std::fs::remove_dir_all(&dir);
    let deep = dir.join("proj").join("skipped").join("sub");
    std::fs::create_dir_all(&deep).expect("dirs");
    std::fs::write(dir.join("proj").join(".dx"), format!("{CONFIG}\nignore \"skipped\"\n")).expect("config");
    std::fs::write(deep.join(".dx"), CONFIG.replace("no BAD here", "deep says no")).expect("deep config");
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
    std::fs::write(scratch.join(".dx"), CONFIG.replace("no BAD here", "scratch says no")).expect("config");
    let tab = scratch.join("tab.t");
    std::fs::write(&tab, "BAD\n").expect("write");
    let mut c = Client::start(&dir);

    // A tab that was never opened, under a gitignored config, uses it.
    c.notify("dxlint/focus", json!({ "visible": [], "tabs": [uri(&tab)] }));
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
    let config = dir.join(".dx");
    let mut c = Client::start(&dir);
    assert_eq!(c.diagnostics_for(&file).len(), 1);

    // A broken unsaved config reports in place.
    let broken = CONFIG.replace("in bad rule", "in bas rule");
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri(&config), "languageId": "dxlint", "version": 1, "text": broken } }),
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
