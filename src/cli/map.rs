//! `dxlint map`: serves a config's `transform`s over stdio, as a TypeScript
//! content mapper (typescript-go `internal/contentmapper`) and to bundlers.
//! One long-lived process: configs load once, files map in milliseconds.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{load_tree_with, Output, Project};
use crate::config::{self, source::Disk};
use crate::lsp::transport;

const PROTOCOL_VERSION: u64 = 1;

/// What each config loaded so far offers; `None` when it did not load.
#[derive(Default)]
struct Projects(HashMap<PathBuf, Option<Project>>);

impl Projects {
    fn of(&mut self, output: &mut Output<'_>, directory: &Path) -> Option<&Project> {
        let nearest = config::find(directory)?;
        self.0.entry(nearest.clone()).or_insert_with(|| {
            let (path, between) = config::loader(&nearest, &Disk);
            load_tree_with(output, &path, |_| between, None, true).ok().map(|(project, _)| project)
        }).as_ref()
    }
}

pub(super) fn run(output: &mut Output<'_>, input: &mut impl BufRead, out: &mut impl Write) -> u8 {
    let mut projects = Projects::default();
    loop {
        let message = match transport::read(input) {
            Ok(Some(message)) => message,
            Ok(None) => return 0,
            Err(error) => {
                output.err(format_args!("dxlint map: {error}"));
                return 1;
            }
        };
        // Notifications want no answer.
        let Some(id) = message.get("id").cloned() else { continue };
        let answer = answer(&mut projects, output, message["method"].as_str().unwrap_or(""), &message["params"]);
        let reply = match answer {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": error } }),
        };
        if transport::write(out, &reply).is_err() { return 1 }
    }
}

fn answer(projects: &mut Projects, output: &mut Output<'_>, method: &str, params: &Value) -> Result<Value, String> {
    match method {
        // Positions are bytes: what every edit and node here counts in.
        "initialize" => Ok(json!({ "protocolVersion": PROTOCOL_VERSION, "positionEncoding": "utf-8", "diagnosticSource": "dxlint" })),
        "openProject" => {
            // A project opening anew is a config that may have changed.
            projects.0.clear();
            Ok(open_project(projects, output, params["configFileName"].as_str().unwrap_or("")))
        }
        "closeProject" => Ok(Value::Null),
        "transform" => transform(projects, output, params),
        _ => Err(format!("unknown method `{method}`")),
    }
}

/// What the mapper's output depends on besides a file's text: its config.
fn open_project(projects: &mut Projects, output: &mut Output<'_>, tsconfig: &str) -> Value {
    let directory = Path::new(tsconfig).parent().unwrap_or(Path::new("."));
    let Some(nearest) = config::find(directory) else { return json!({ "configIdentity": "no config" }) };
    let identity = projects.of(output, directory).map_or(0, |project| project.linter.fingerprint);
    json!({ "configIdentity": format!("{identity:x}"), "watchedFiles": [nearest] })
}

fn transform(projects: &mut Projects, output: &mut Output<'_>, params: &Value) -> Result<Value, String> {
    let file = params["fileName"].as_str().ok_or("transform without fileName")?;
    let content = params["content"].as_str().ok_or("transform without content")?;
    let path = Path::new(file);
    let directory = path.parent().ok_or("transform of a file in no directory")?;
    let Some(project) = projects.of(output, directory) else {
        return Err(format!("no loadable `.dx` config for {file}"));
    };
    let mapped = project.linter.mapped(path, content);
    let diagnostics: Vec<Value> = mapped.problems.iter().map(|problem| json!({
        "messageText": problem.message,
        "start": problem.start,
        "length": problem.end - problem.start,
    })).collect();
    let mut result = json!({ "text": mapped.text, "extension": ".ts", "mappings": mapped.span_map(), "diagnostics": diagnostics });
    // Bundlers ask: TypeScript maps with `mappings`.
    if params["sourceMap"].as_bool() == Some(true) {
        result["sourceMap"] = Value::String(mapped.source_map(file, content));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str, config: &str, wts: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dxlint-map-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".dx"), config).unwrap();
        std::fs::write(dir.join("a.wts"), wts).unwrap();
        dir
    }

    fn serve(requests: &[Value], cwd: &Path) -> Vec<Value> {
        let mut input = Vec::new();
        for request in requests { transport::write(&mut input, request).unwrap() }
        let (mut stdout, mut stderr, mut replies) = (Vec::new(), Vec::new(), Vec::new());
        let mut output = Output { cwd, stdout: &mut stdout, stderr: &mut stderr };
        run(&mut output, &mut std::io::BufReader::new(&input[..]), &mut replies);
        let mut reader = std::io::BufReader::new(&replies[..]);
        std::iter::from_fn(|| transport::read(&mut reader).unwrap()).collect()
    }

    #[test]
    fn a_session_maps_a_file_with_its_transforms_only() {
        let config = concat!(
            "import .ts\n",
            "lang js \"*.ts\" \"*.wts\"\n",
            "language .js {\n  files \"*.wts\"\n  set memberModifiers [\"state\"]\n}\n",
            "in ts_class_property let state_word { value modifier.nodes; find (m) m == \"state\" }\n",
            "in ts_class_property transform state-field {\n",
            "  when state_word != null\n",
            "  fix {\n",
            "    at state_word as \"\"\n",
            "    at { start: end_offset, end: end_offset } as \" // {key}\"\n",
            "  }\n",
            "}\n",
        );
        let text = "class A {\n\tstate x = 1;\n}\n";
        let dir = dir("session", config, text);
        let file = dir.join("a.wts");
        let replies = serve(&[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "positionEncodings": ["utf-8"] } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "transform", "params": { "fileName": file, "content": text, "sourceMap": true } }),
        ], &dir);
        assert_eq!(replies[0]["result"]["positionEncoding"], "utf-8");
        let result = &replies[1]["result"];
        assert_eq!(result["diagnostics"], json!([]), "{result}");
        let output = result["text"].as_str().unwrap();
        assert!(output.contains("x = 1; // x"), "{output}");
        assert!(!output.contains("state"), "{output}");
        // What it maps back to is the original text at those offsets.
        for segment in result["mappings"].as_array().unwrap() {
            let (at, length, from) = (segment[0].as_u64().unwrap() as usize, segment[1].as_u64().unwrap() as usize, segment[2].as_u64().unwrap() as usize);
            assert_eq!(&output[at..at + length], &text[from..from + length]);
        }
        assert!(result["sourceMap"].as_str().unwrap().contains("\"version\":3"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// The worked example: everything stays on its line, and every verbatim
    /// stretch really is the original's text.
    #[test]
    fn state_members_map_keeping_their_lines() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/engine/transform-state");
        let config = std::fs::read_to_string(root.join(".dx")).unwrap();
        let text = std::fs::read_to_string(root.join("item.wts")).unwrap();
        let built = crate::engine::build(crate::config::load_text(".dx", &config, &Disk), root.clone());
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
        let mapped = built.linter.expect("compiled").mapped(&root.join("item.wts"), &text);
        assert_eq!(mapped.problems, Vec::new());
        assert_eq!(mapped.text.lines().count(), text.lines().count());
        for segment in &mapped.segments {
            assert_eq!(mapped.text[segment.output..segment.output + segment.length], text[segment.original..segment.original + segment.length]);
        }
        // A read of this file's state is a mask; anything further is a guard.
        assert!(mapped.text.contains(r#"own: ["quality", "seed"]"#), "{}", mapped.text);
        assert!(mapped.text.contains("const s1 = this.mainHand?.attack;"), "{}", mapped.text);
        assert!(mapped.text.contains(r#"volatile: ["listed"]"#), "{}", mapped.text);
    }

    #[test]
    fn linting_never_runs_a_transform() {
        let config = "import .ts\nin ts_class_property transform t error \"mapped only\" { }\n";
        let dir = dir("lint", config, "");
        let built = crate::engine::build(crate::config::load_text(".dx", config, &Disk), dir.clone());
        let linter = built.linter.expect("compiled");
        assert!(linter.lint(&dir.join("a.ts"), "class A { x = 1; }").is_empty());
        assert_eq!(linter.mapped(&dir.join("a.ts"), "class A { x = 1; }").problems.len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }
}
