//! `dxlint map`: a project's `transform`s over stdio, one long-lived process
//! (configs load once, files map in milliseconds), or `dxlint map <files>`
//! to print what files map to. The protocol it speaks is dxlint's own
//! (`initialize`, `map`), or a `protocol` declared in `.dx` (`--protocol`),
//! such as std's TypeScript content mapper: adapters translate the neutral
//! mapping ([`crate::engine::transform::Mapped`]) into what a tool wants.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::json;

use super::{load_tree_with, ConfigArg, Output, Project};
use crate::config::{self, source::{normalize_path, Disk}};
use crate::engine::transform::Mapped;
use crate::expr::Value;
use crate::lsp::transport;

const PROTOCOL_VERSION: u64 = 1;

#[derive(clap::Args)]
pub struct MapArgs {
    /// Print what these files map to instead of serving requests
    pub paths: Vec<PathBuf>,
    /// Serve the `protocol` a `.dx` file declares (`.ts/content-mapper`, a
    /// std module, or a path) instead of dxlint's own
    #[arg(long, value_name = "MODULE|FILE")]
    pub protocol: Option<String>,
}

/// Loads configs as files ask for them, and maps files with them.
struct Server {
    cwd: PathBuf,
    /// `--config`: one config for every path.
    config: Option<PathBuf>,
    /// What each config loaded so far offers; `None` when it did not load.
    projects: HashMap<PathBuf, Option<Project>>,
}

impl Server {
    /// The config `path` (a file or directory) falls under.
    fn config_of(&self, path: &Path) -> Option<PathBuf> {
        if let Some(config) = &self.config { return Some(config.clone()) }
        let path = normalize_path(&self.cwd.join(path));
        let directory = if path.is_dir() { path.as_path() } else { path.parent()? };
        config::find(directory)
    }

    fn project(&mut self, path: &Path) -> Result<&Project, String> {
        let config = self.config_of(path).ok_or_else(|| format!("no `.dx` config for {}", path.display()))?;
        let explicit = self.config.is_some();
        let cwd = self.cwd.clone();
        self.projects.entry(config.clone()).or_insert_with(|| {
            let (path, between) = if explicit { (config.clone(), Vec::new()) } else { config::loader(&config, &Disk) };
            let (mut stdout, mut stderr) = (std::io::sink(), std::io::stderr());
            let mut output = Output { cwd: &cwd, stdout: &mut stdout, stderr: &mut stderr };
            load_tree_with(&mut output, &path, |_| between, None, true).ok().map(|(project, _)| project)
        }).as_ref().ok_or_else(|| format!("the `.dx` config for {} does not load", path.display()))
    }

    fn mapped(&mut self, path: &str, text: &str) -> Result<Mapped, String> {
        let path = normalize_path(&self.cwd.join(path));
        Ok(self.project(&path)?.linter.mapped(&path, text))
    }
}

impl crate::expr::protocol::Host for Server {
    fn map_file(&mut self, path: &str, text: &str) -> Result<Value<'static>, String> {
        Ok(mapped_value(&self.mapped(path, text)?))
    }

    /// Reads the config afresh: a tool asks when a project (re)opens.
    fn config_identity(&mut self, path: &str) -> Result<Value<'static>, String> {
        let Some(config) = self.config_of(Path::new(path)) else { return Ok(Value::from("no config")) };
        self.projects.remove(&config);
        Ok(Value::from(format!("{:x}", self.project(Path::new(path))?.linter.fingerprint)))
    }

    fn config_files(&mut self, path: &str) -> Result<Value<'static>, String> {
        Ok(Value::list(self.config_of(Path::new(path)).into_iter().map(|config| Value::from(config.display().to_string())).collect()))
    }
}

/// A mapping as protocol methods read it.
fn mapped_value(mapped: &Mapped) -> Value<'static> {
    let span = |range: &std::ops::Range<usize>| Value::record(vec![
        ("offset".into(), Value::from(range.start)),
        ("end_offset".into(), Value::from(range.end)),
    ]);
    Value::record(vec![
        ("text".into(), Value::from(mapped.text.clone())),
        ("pieces".into(), Value::list(mapped.pieces.iter().map(|piece| Value::record(vec![
            ("output".into(), span(&piece.output)),
            ("source".into(), span(&piece.source)),
            ("kind".into(), Value::from(piece.kind.name())),
        ])).collect())),
        ("problems".into(), Value::list(mapped.problems.iter().map(|problem| Value::record(vec![
            ("message".into(), Value::from(problem.message.clone())),
            ("offset".into(), Value::from(problem.start)),
            ("end_offset".into(), Value::from(problem.end)),
        ])).collect())),
    ])
}

pub(super) fn run(output: &mut Output<'_>, args: &MapArgs, config: &ConfigArg, input: &mut impl BufRead, out: &mut impl Write) -> u8 {
    let mut server = Server {
        cwd: output.cwd.to_path_buf(),
        config: config.config.as_ref().map(|path| normalize_path(&output.cwd.join(path))),
        projects: HashMap::new(),
    };
    if !args.paths.is_empty() {
        return preview(output, &mut server, &args.paths);
    }
    let protocol = match &args.protocol {
        Some(spec) => match declared(output, spec) {
            Ok(protocol) => Some(protocol),
            Err(code) => return code,
        },
        None => None,
    };
    match protocol {
        Some(protocol) => {
            // Its methods map files through this server.
            let _host = crate::expr::protocol::install(Box::new(server));
            serve(output, input, out, |method, params| protocol.answer(method, params))
        }
        None => serve(output, input, out, |method, params| own(&mut server, method, params)),
    }
}

/// Answers each request until input ends.
fn serve(output: &mut Output<'_>, input: &mut impl BufRead, out: &mut impl Write,
    mut answer: impl FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, String>) -> u8
{
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
        let reply = match answer(message["method"].as_str().unwrap_or(""), &message["params"]) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": error } }),
        };
        if transport::write(out, &reply).is_err() { return 1 }
    }
}

/// dxlint's own protocol.
fn own(server: &mut Server, method: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    match method {
        "initialize" => Ok(json!({ "protocolVersion": PROTOCOL_VERSION, "positionEncoding": "utf-8" })),
        "map" => {
            let path = params["path"].as_str().ok_or("map without path")?;
            let text = params["text"].as_str().ok_or("map without text")?;
            let mapped = server.mapped(path, text)?;
            let mut result = to_json(&mapped_value(&mapped));
            if params["sourceMap"].as_bool() == Some(true) {
                result["sourceMap"] = serde_json::Value::String(mapped.source_map(path, text));
            }
            Ok(result)
        }
        _ => Err(format!("unknown method `{method}`")),
    }
}

/// A `protocol` declared in `.dx`, and the config holding its methods.
struct Declared {
    linter: crate::engine::Linter,
    methods: Vec<(String, String)>,
}

impl Declared {
    fn answer(&self, method: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let (_, function) = self.methods.iter().find(|(name, _)| name == method).ok_or_else(|| format!("unknown method `{method}`"))?;
        crate::expr::Ctx::call_static(&self.linter.grammar, function, vec![from_json(params)]).map(|answer| to_json(&answer))
    }
}

/// `.ts/content-mapper` (a std module) or a `.dx` file declaring one `protocol`.
fn declared(output: &mut Output<'_>, spec: &str) -> Result<Declared, u8> {
    let loaded = if spec.starts_with('.') && !spec.starts_with("./") && !spec.starts_with("..") {
        config::load_text("protocol.dx", &format!("import {spec}\n"), &Disk)
    } else {
        config::load_tree(&normalize_path(&output.cwd.join(spec)), |_| Vec::new(), None, &Disk).0
    };
    let protocols = loaded.config.protocols.clone();
    let built = crate::engine::build_for_lint(loaded, output.cwd.to_path_buf());
    if !built.errors.is_empty() {
        super::render::config_errors(output, &built.errors, &built.sources);
        return Err(2);
    }
    let [protocol] = protocols.as_slice() else {
        let names: Vec<&str> = protocols.iter().map(|protocol| protocol.name.text.as_str()).collect();
        super::render::error(output, &format!("`{spec}` must declare one `protocol`, it declares {}", if names.is_empty() { "none".to_owned() } else { names.join(", ") }));
        return Err(2);
    };
    let methods = protocol.methods.iter().map(|(name, function)| (name.text.clone(), function.clone())).collect();
    Ok(Declared { linter: built.linter.ok_or(2u8)?, methods })
}

/// `dxlint map <files>`: each file's mapped text, problems to stderr.
fn preview(output: &mut Output<'_>, server: &mut Server, paths: &[PathBuf]) -> u8 {
    let mut code = 0;
    for path in paths {
        let text = match std::fs::read_to_string(output.cwd.join(path)) {
            Ok(text) => text,
            Err(error) => {
                output.err(format_args!("{}: {error}", path.display()));
                code = 1;
                continue;
            }
        };
        let mapped = match server.mapped(&path.to_string_lossy(), &text) {
            Ok(mapped) => mapped,
            Err(error) => {
                output.err(format_args!("{}: {error}", path.display()));
                code = 1;
                continue;
            }
        };
        if paths.len() > 1 { output.out(format_args!("==> {} <==", path.display())) }
        let shown = mapped.text.strip_suffix('\n').unwrap_or(&mapped.text);
        output.out(format_args!("{shown}"));
        let lines = crate::line_index::LineIndex::new(&text);
        for problem in &mapped.problems {
            let (line, column) = lines.line_col(&text, problem.start);
            output.err(format_args!("{}:{}:{}: {}", path.display(), line + 1, column + 1, problem.message));
            code = 1;
        }
    }
    code
}

fn from_json(value: &serde_json::Value) -> Value<'static> {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => Value::Num(n.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(s) => Value::from(s.clone()),
        serde_json::Value::Array(items) => Value::list(items.iter().map(from_json).collect()),
        serde_json::Value::Object(entries) => Value::record(entries.iter().map(|(key, value)| (key.clone().into(), from_json(value))).collect::<Vec<_>>()),
    }
}

fn to_json(value: &Value<'_>) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => json!(b),
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => json!(*n as i64),
        Value::Num(n) => json!(n),
        Value::List(items) => serde_json::Value::Array(items.iter().map(to_json).collect()),
        Value::Record(entries) => serde_json::Value::Object(entries.iter().map(|(key, value)| (key.to_string(), to_json(value))).collect()),
        other => serde_json::Value::String(other.text().into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::transform::PieceKind;

    const CONFIG: &str = concat!(
        "import .ts\n",
        "lang js \"*.ts\" \"*.wts\"\n",
        "extend parser ts {\n  files \"*.wts\"\n  production class_modifier |= /state/ same_line peek(accessor_next)\n}\n",
        "in ts_class_property let state_word { value modifier.nodes; find (m) m == \"state\" }\n",
        "in ts_class_property transform state-field {\n",
        "  when state_word != null\n",
        "  rewrite {\n",
        "    at { start: state_word.offset, end: key.offset } as \"\"\n",
        "    at { start: end_offset, end: end_offset } as \" get {key} () {{ return this.{mirror(key)}; }} // {str(key)}\"\n",
        "  }\n",
        "}\n",
    );
    const TEXT: &str = "class A {\n\tstate x = 1;\n}\n";

    fn dir(name: &str, config: &str, wts: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dxlint-map-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".dx"), config).unwrap();
        std::fs::write(dir.join("a.wts"), wts).unwrap();
        dir
    }

    fn serve(args: &MapArgs, requests: &[serde_json::Value], cwd: &Path) -> Vec<serde_json::Value> {
        let mut input = Vec::new();
        for request in requests { transport::write(&mut input, request).unwrap() }
        let (mut stdout, mut stderr, mut replies) = (Vec::new(), Vec::new(), Vec::new());
        let mut output = Output { cwd, stdout: &mut stdout, stderr: &mut stderr };
        run(&mut output, args, &ConfigArg { config: None }, &mut std::io::BufReader::new(&input[..]), &mut replies);
        let mut reader = std::io::BufReader::new(&replies[..]);
        std::iter::from_fn(|| transport::read(&mut reader).unwrap()).collect()
    }

    fn request(id: u64, method: &str, params: serde_json::Value) -> serde_json::Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    /// dxlint's own protocol: the mapped text, each piece pointing at the
    /// original text it came from, and a source map.
    #[test]
    fn the_own_protocol_maps_a_file_into_pieces() {
        let dir = dir("own", CONFIG, TEXT);
        let file = dir.join("a.wts");
        let args = MapArgs { paths: Vec::new(), protocol: None };
        let replies = serve(&args, &[request(1, "map", json!({ "path": file, "text": TEXT, "sourceMap": true }))], &dir);
        let result = &replies[0]["result"];
        assert_eq!(result["problems"], json!([]), "{result}");
        let output = result["text"].as_str().unwrap();
        assert_eq!(output, "class A {\n\tx = 1; get x () { return this.x; } // x\n}\n");
        let mut kinds = Vec::new();
        for piece in result["pieces"].as_array().unwrap() {
            let at = |span: &serde_json::Value| span["offset"].as_u64().unwrap() as usize..span["end_offset"].as_u64().unwrap() as usize;
            assert_eq!(&output[at(&piece["output"])], &TEXT[at(&piece["source"])]);
            kinds.push(piece["kind"].as_str().unwrap().to_owned());
        }
        // `str(key)` drops the node: the comment's `x` is made up.
        assert_eq!(kinds, ["kept", "kept", "copy", "mirror", "kept"]);
        assert!(result["sourceMap"].as_str().unwrap().contains("\"version\":3"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// std's TypeScript adapter: segments split so original spans are
    /// equal or disjoint (TypeScript's rule), verbatim text matching, the
    /// mirror taking part in no feature.
    #[test]
    fn the_typescript_adapter_translates_pieces_into_valid_span_maps() {
        let dir = dir("typescript", CONFIG, TEXT);
        let file = dir.join("a.wts");
        let args = MapArgs { paths: Vec::new(), protocol: Some(".ts/content-mapper".into()) };
        let replies = serve(&args, &[
            request(1, "initialize", json!({})),
            request(2, "openProject", json!({ "configFileName": dir.join("tsconfig.json") })),
            request(3, "transform", json!({ "fileName": file, "content": TEXT })),
        ], &dir);
        assert_eq!(replies[0]["result"]["positionEncoding"], "utf-8");
        assert!(replies[1]["result"]["configIdentity"].as_str().is_some_and(|identity| identity != "no config"), "{}", replies[1]);
        let result = &replies[2]["result"];
        let output = result["text"].as_str().unwrap();
        assert_eq!(result["diagnostics"], json!([]), "{result}");
        let segments: Vec<Vec<u64>> = serde_json::from_value(result["mappings"].clone()).unwrap();
        let mut originals: Vec<(u64, u64)> = Vec::new();
        for segment in &segments {
            let (virtual_start, length, original_start, original_length) = (segment[0] as usize, segment[1] as usize, segment[2] as usize, segment[3] as usize);
            assert_eq!(segment[4], 0, "{segment:?}");
            assert_eq!(&output[virtual_start..virtual_start + length], &TEXT[original_start..original_start + original_length]);
            originals.push((segment[2], segment[2] + segment[3]));
        }
        originals.sort_unstable();
        originals.dedup();
        assert!(originals.windows(2).all(|pair| pair[0].1 <= pair[1].0), "{originals:?}");
        let mirrors: Vec<&Vec<u64>> = segments.iter().filter(|segment| segment.len() == 6).collect();
        assert_eq!(mirrors.len(), 1);
        assert_eq!(mirrors[0][5], 0);
        std::fs::remove_dir_all(dir).ok();
    }

    /// `--config` maps every file with that config, not their nearest one.
    #[test]
    fn an_explicit_config_maps_every_file() {
        let dir = dir("explicit", "import .ts\n", TEXT);
        let other = dir.join("other.dx");
        std::fs::write(&other, CONFIG).unwrap();
        let mut server = Server { cwd: dir.clone(), config: Some(other), projects: HashMap::new() };
        let mapped = server.mapped(&dir.join("a.wts").to_string_lossy(), TEXT).unwrap();
        assert!(mapped.text.contains("get x ()"), "{}", mapped.text);
        std::fs::remove_dir_all(dir).ok();
    }

    /// The worked example: everything stays on its line, and every piece
    /// really is the original's text.
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
        for piece in mapped.pieces.iter().filter(|piece| piece.kind != PieceKind::Renamed) {
            assert_eq!(mapped.text[piece.output.clone()], text[piece.source.clone()]);
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

    /// A rule repairs with `fix`, a transform maps with `rewrite`; each
    /// word in the other's place is an error, as is a transform's `suggest`.
    #[test]
    fn rules_and_transforms_take_their_own_edit_clause() {
        let errors = |source: &str| crate::config::load_text("t.dx", &format!("import .ts\n{source}\n"), &Disk).errors.len();
        assert_eq!(errors("in ts_call transform t { rewrite as \"\" }"), 0);
        assert_eq!(errors("in ts_call rule r warn \"x\" { fix as \"\" }"), 0);
        assert_ne!(errors("in ts_call transform t { fix as \"\" }"), 0);
        assert_ne!(errors("in ts_call rule r warn \"x\" { rewrite as \"\" }"), 0);
        assert_ne!(errors("in ts_call transform t rewrite as \"\" {\n suggest \"s\" as \"\"\n}"), 0);
    }
}
