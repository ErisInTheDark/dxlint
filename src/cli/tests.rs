//! The CLI entry end to end: config discovery, imports, exit codes,
//! output formats and fixing files in place.

use std::path::{Path, PathBuf};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("output capture").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Output {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join("dxlint-cli-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        std::fs::write(full, text).expect("write");
    }
    dir
}

fn dxlint(dir: &Path, args: &[&str]) -> Output {
    dx_input(dir, args, "")
}

fn dx_input(dir: &Path, args: &[&str], input: &str) -> Output {
    let stdout = Capture::default();
    let stderr = Capture::default();
    let code = super::run(
        std::iter::once("dxlint").chain(args.iter().copied()),
        dir,
        io::Cursor::new(input.as_bytes().to_vec()),
        anstream::StripStream::new(Box::new(stdout.clone()) as Box<dyn Write + Send>),
        anstream::StripStream::new(Box::new(stderr.clone()) as Box<dyn Write + Send>),
    );
    let stdout = stdout.0.lock().expect("stdout capture").clone();
    let stderr = stderr.0.lock().expect("stderr capture").clone();
    Output { code: i32::from(code), stdout, stderr }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

const RULES: &str = r#"
lang t "*.t"
in .t match shout /\b[A-Z]{3,}\b/ {
  rule no-shout warn "`{text}` is shouting" fix as lower(text)
}
"#;

fn conformance_matches_standalone(dir: &Path, cases: &[(&str, Vec<(&Path, &str)>)]) -> Vec<serde_json::Value> {
    let config_path = dir.join(".dx");
    let requests: Vec<_> = cases.iter().enumerate().map(|(id, (config, sources))| serde_json::json!({
        "id": id.to_string(), "config": config,
        "sources": sources.iter().map(|(path, text)| serde_json::json!({ "path": path, "text": text })).collect::<Vec<_>>(),
    })).collect();
    let expected: Vec<_> = cases.iter().map(|(config, sources)| {
        let records = std::iter::once(serde_json::json!({ "path": config_path, "text": config }))
            .chain(sources.iter().map(|(path, text)| serde_json::json!({ "path": path, "text": text })))
            .map(|record| record.to_string()).collect::<Vec<_>>().join("\n");
        dx_input(dir, &["--sources", "-", "--no-cache", "--format", "jsonl", "--fix-preview", "--jobs", "1"], &records)
    }).collect();
    let input = requests.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    let actual = dx_input(dir, &["conformance", "--config", config_path.to_str().unwrap()], &input);
    assert_eq!(actual.code, 0, "{}", String::from_utf8_lossy(&actual.stderr));
    assert!(actual.stderr.is_empty(), "worker output must be inside replies");
    let replies: Vec<serde_json::Value> = stdout(&actual).lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(replies.len(), expected.len());
    for (id, (reply, expected)) in replies.iter().zip(&expected).enumerate() {
        assert_eq!(reply["id"], id.to_string());
        assert_eq!(reply["code"], expected.code);
        assert_eq!(reply["stdout"], stdout(expected));
        assert_eq!(reply["stderr"], String::from_utf8_lossy(&expected.stderr).to_string());
    }
    replies
}

#[test]
fn conformance_reuses_config_without_leaking_sources_or_config_imports() {
    let dir = std::env::temp_dir().join("dxlint-cli-tests").join("conformance-config");
    let a = dir.join("a.t");
    let b = dir.join("b.t");
    let base = dir.with_extension("dxlint");
    let config = format!("import \"../{}\"\n", base.file_name().unwrap().to_string_lossy());
    let changed = format!("{config}rule no-shout error\n");
    let replacement = RULES.replace("lower(text)", "\"quiet\"");
    let replies = conformance_matches_standalone(&dir, &[
        (&config, vec![(&base, RULES), (&a, "LOUD"), (&b, "BAD")]),
        (&config, vec![(&base, RULES), (&a, "soft")]),
        (&config, vec![(&base, &replacement), (&a, "LOUD")]),
        (&changed, vec![(&base, RULES), (&a, "LOUD")]),
        ("in absent rule broken\n", vec![(&a, "LOUD")]),
        (&config, vec![(&base, RULES), (&a, "LOUD")]),
    ]);
    assert_ne!(replies[0]["stdout"], replies[1]["stdout"]);
    assert_ne!(replies[2]["stdout"], replies[5]["stdout"], "changed imported rules must rebuild");
    assert_eq!(replies[3]["code"], 1);
    assert_eq!(replies[4]["code"], 2);
    assert_eq!(replies[5]["code"], 0);
    for path in [&a, &b, &base, &dir.join(".dx")] {
        assert!(crate::engine::reads::buffer(path).is_none(), "worker leaked {}", path.display());
        assert!(!path.exists(), "worker wrote {}", path.display());
    }
}

#[test]
fn conformance_resets_imports_globals_and_project_settings() {
    let dir = std::env::temp_dir().join("dxlint-cli-tests").join("conformance-project");
    let main = dir.join("main.ts");
    let dep = dir.join("dep.ts");
    let globals = dir.join("globals.d.ts");
    let settings = dir.join("tsconfig.json");
    let config = "import .ts\nin ts_identifier rule probe info \"{text}: {if node always number then 'number' else if node always string then 'string' else if node always boolean then 'boolean' else 'unknown'}\" { when text == \"x\" || text == \"G\" }\n";
    let replies = conformance_matches_standalone(&dir, &[
        (config, vec![(&main, "import { x } from './dep';\nx;\nG;\n"),
            (&dep, "export const x: number = 1;"), (&globals, "declare const G: boolean;"),
            (&settings, "{\"compilerOptions\":{\"strictNullChecks\":true}}")]),
        (config, vec![(&main, "import { x } from './dep';\nx;\nG;\n"),
            (&dep, "export const x: string = '';"), (&settings, "{}")]),
        (config, vec![(&main, "import { x } from './dep';\nx;\nG;\n")]),
    ]);
    assert_ne!(replies[0]["stdout"], replies[1]["stdout"]);
    assert_ne!(replies[1]["stdout"], replies[2]["stdout"]);
}

#[test]
fn conformance_bad_frames_restore_overlays_before_the_next_request() {
    let dir = std::env::temp_dir().join("dxlint-cli-tests").join("conformance-bad-frame");
    let file = dir.join("a.t");
    let config_path = dir.join(".dx");
    let source = serde_json::json!({ "path": file, "text": "LOUD" });
    let frames = [
        "{".to_owned(),
        serde_json::json!({ "id": "duplicate", "config": RULES, "sources": [source.clone(), source.clone()] }).to_string(),
        serde_json::json!({ "id": "good", "config": RULES, "sources": [source] }).to_string(),
    ].join("\n");
    let out = dx_input(&dir, &["conformance", "--config", config_path.to_str().unwrap()], &frames);
    assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
    let replies: Vec<serde_json::Value> = stdout(&out).lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0]["id"], serde_json::Value::Null);
    assert_eq!(replies[0]["code"], 2);
    assert_eq!(replies[1]["id"], "duplicate");
    assert_eq!(replies[1]["code"], 2);
    assert_eq!(replies[2]["id"], "good");
    assert_eq!(replies[2]["code"], 0);
    assert!(!replies[2]["stdout"].as_str().unwrap().is_empty());
    assert!(crate::engine::reads::buffer(&file).is_none());
    assert!(crate::engine::reads::buffer(&config_path).is_none());
}

#[test]
fn conformance_large_frames_survive_partial_writes() {
    struct Partial(Capture);
    impl Write for Partial {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.write(&bytes[..bytes.len().min(8192)])
        }
        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    let dir = std::env::temp_dir().join("dxlint-cli-tests").join("conformance-large-frames");
    let config_path = dir.join(".dx");
    let id = format!("{}{}", "x".repeat(8184),
        "\u{e9}\u{fc}\u{1f989}\u{009b}\u{2028}\u{2029} quotes \" slash \\ newline\nreturn\r".repeat(4096));
    let text = "LOUD ".repeat(128);
    let frames = [
        serde_json::json!({ "id": id, "config": RULES, "sources": [{ "path": dir.join("a.t"), "text": text }] }),
        serde_json::json!({ "id": "next", "config": RULES, "sources": [] }),
    ];
    let input = frames.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    let stdout = Capture::default();
    let stderr = Capture::default();
    let code = super::run(
        ["dxlint", "conformance", "--config", config_path.to_str().unwrap()],
        &dir,
        io::Cursor::new(input.into_bytes()),
        anstream::AutoStream::new(
            Box::new(Partial(stdout.clone())) as Box<dyn Write + Send>,
            anstream::ColorChoice::Auto,
        ),
        stderr.clone(),
    );
    assert_eq!(code, 0);
    assert!(stderr.0.lock().unwrap().is_empty());
    let bytes = stdout.0.lock().unwrap();
    let output = std::str::from_utf8(&bytes).unwrap();
    let replies: Vec<serde_json::Value> = output.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["id"], id);
    let diagnostics: Vec<serde_json::Value> = replies[0]["stdout"].as_str().unwrap().lines()
        .map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(diagnostics.iter().filter(|record| record["kind"] != "fix-preview").count(), 128);
    assert_eq!(replies[1]["id"], "next");
    assert_eq!(replies[1]["stdout"], "");
}

#[test]
fn jsonl_profile_counts_emitted_reports_and_includes_unfired_rules() {
    let dir = project("jsonl-profile", &[
        (".dx", "lang t \"*.t\"\nin .t match letter /[a-z]/ {\nrule hit warn \"hit\"\nrule quiet warn \"quiet\" { when false }\n}\n"),
        ("a.t", "ab"), ("b.t", "cd"),
    ]);
    for jobs in ["1", "3"] {
        let out = dxlint(&dir, &["--no-cache", "--profile", "--format", "jsonl", "--jobs", jobs]);
        assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
        let records: Vec<serde_json::Value> = stdout(&out).lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        let profiles: Vec<_> = records.iter().filter(|record| record["kind"] == "rule-profile").collect();
        assert_eq!(profiles.len(), 2, "every compiled rule has a profile, including unfired rules");
        for profile in profiles {
            let emitted = records.iter().filter(|record| record["kind"] != "rule-profile" && record["rule"] == profile["rule"]).count();
            assert_eq!(profile["fired"].as_u64(), Some(emitted as u64));
            assert!(profile["evaluated"].as_u64().is_some_and(|count| count >= emitted as u64));
            assert!(profile["ms"].as_f64().is_some_and(|ms| ms.is_finite() && ms >= 0.0));
        }
    }
    let out = dxlint(&dir, &["--no-cache", "--format", "jsonl"]);
    assert!(stdout(&out).lines().all(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["kind"] != "rule-profile"));
}

#[test]
fn fix_preview_is_one_readonly_pass_for_disk_and_packed_sources() {
    let rules = "lang t \"*.t\"\nin .t match letter /[ab]/ { rule capitalise warn \"lowercase\" fix as upper(text) }\n";
    let files = [(".dx", rules), ("a.t", "ab"), ("clean.t", "XY"), ("ignored.js", "ab")];
    let dir = project("fix-preview", &files);
    let packed_dir = project("fix-preview-packed", &[("keep", "unchanged")]);
    let pack = files.into_iter().map(|(path, text)| serde_json::json!({ "path": path, "text": text }).to_string()).collect::<Vec<_>>().join("\n");
    for jobs in ["1", "3"] {
        for packed in [false, true] {
            let mut args = vec!["--fix-preview", "--format", "jsonl", "--jobs", jobs];
            if packed {
                args.extend(["--sources", "-"]);
            }
            let out = dx_input(if packed { &packed_dir } else { &dir }, &args, &pack);
            assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
            let records: Vec<serde_json::Value> = stdout(&out).lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            assert_eq!(records.len(), 4, "{records:?}");
            let diagnostics: Vec<_> = records.iter().filter(|record| record["kind"] != "fix-preview").collect();
            assert_eq!(diagnostics.len(), 2);
            assert!(diagnostics.iter().all(|record| record["file"] == "a.t" && record["fixable"] == true));
            for (file, text) in [("a.t", "Ab"), ("clean.t", "XY")] {
                let previews: Vec<_> = records.iter().enumerate().filter(|(_, record)| record["file"] == file && record["kind"] == "fix-preview").collect();
                assert_eq!(previews.len(), 1, "{records:?}");
                let (index, preview) = previews[0];
                assert_eq!(preview["text"], text);
                assert!(records[index + 1..].iter().all(|record| record["file"] != file), "preview precedes diagnostics");
            }
            assert_eq!(std::fs::read_to_string(dir.join("a.t")).unwrap(), "ab");
            assert!(!dir.join(".dxcache").exists());
            assert_eq!(std::fs::read_dir(&packed_dir).unwrap().count(), 1);
            assert!(!crate::engine::reads::is_file(&packed_dir.join("a.t")), "invocation leaked buffers");
        }
    }
    for args in [
        vec!["--fix-preview"],
        vec!["--fix-preview", "--format", "jsonl", "--fix"],
    ] {
        let out = dxlint(&dir, &args);
        assert_eq!(out.code, 2);
        assert!(out.stdout.is_empty());
        assert_eq!(std::fs::read_to_string(dir.join("a.t")).unwrap(), "ab");
        assert!(!dir.join(".dxcache").exists());
    }
    std::fs::remove_dir_all(dir).expect("project cleanup");
    std::fs::remove_dir_all(packed_dir).expect("packed project cleanup");
}

#[test]
fn packed_sources_resolve_virtual_projects_without_writing_files() {
    let dir = std::env::temp_dir().join("dxlint-cli-tests").join("packed-sources");
    std::fs::create_dir_all(&dir).expect("empty project directory");
    let pack = [
        (".dx", "import shared\n"),
        ("shared.dx", "import .ts\nin ts_identifier rule typed info \"{text}: {if node always number then 'number' else if node always string then 'string' else if node always boolean then 'boolean' else 'unknown'}\" { when reference_kind == \"import\" }\n"),
        ("dir/index.ts", "export const x: number = 1;\n"),
        ("main.ts", "import { x } from './dir';\nx;\n"),
    ].into_iter().map(|(path, text)| serde_json::json!({ "path": path, "text": text }).to_string()).collect::<Vec<_>>().join("\n");
    for jobs in ["1", "3"] {
        let out = dx_input(&dir, &["--sources", "-", "--no-cache", "--format", "jsonl", "--stats", "--jobs", jobs], &pack);
        assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
        let reports: Vec<serde_json::Value> = stdout(&out).lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert!(!reports.is_empty(), "typed import was not checked");
        assert!(reports.iter().all(|report| report["file"] == "main.ts" && report["message"] == "x: number"), "{reports:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("2 files"), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }
    assert!(!crate::engine::reads::is_file(&dir.join("main.ts")), "invocation leaked buffers");
    std::fs::remove_dir(&dir).expect("empty project cleanup");
}

#[test]
fn packed_input_errors_restore_existing_buffers_before_any_lint() {
    use crate::engine::reads;
    let dir = std::env::temp_dir().join("dxlint-cli-tests").join("packed-input-errors");
    let file = dir.join("main.ts");
    reads::set_buffer(&file, "original");
    let record = serde_json::json!({ "path": "main.ts", "text": "replacement" }).to_string();
    for tail in [
        "not json".to_owned(),
        serde_json::json!({ "path": "sub/../main.ts", "text": "duplicate" }).to_string(),
    ] {
        let out = dx_input(&dir, &["--sources", "-", "--no-cache", "--format", "jsonl"], &format!("{record}\n{tail}"));
        assert_eq!(out.code, 2);
        assert!(out.stdout.is_empty());
        assert_eq!(reads::read(&file).as_deref(), Some("original"));
    }
    for extra in ["main.ts", "--fix"] {
        let out = dx_input(&dir, &["--sources", "-", extra], &record);
        assert_eq!(out.code, 2);
        assert_eq!(reads::read(&file).as_deref(), Some("original"));
    }
    reads::drop_buffer(&file);
}

#[test]
fn packed_config_is_available_to_tooling_commands() {
    let dir = std::env::temp_dir().join("dxlint-cli-tests").join("packed-tooling");
    let pack = [
        (".dx", "import rules/shout\n"),
        ("rules/shout.dx", RULES),
        ("main.t", "LOUD"),
    ].into_iter().map(|(path, text)| serde_json::json!({ "path": path, "text": text }).to_string()).collect::<Vec<_>>().join("\n");
    let out = dx_input(&dir, &["rules", "--sources", "-", "--config", ".dx"], &pack);
    assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).contains("no-shout"));
    for args in [
        vec!["--sources", "-", "check"],
        vec!["--sources", "-", "tree", "main.t"],
        vec!["--sources", "-", "eval", "shout", "lower(text)", "main.t"],
    ] {
        let out = dx_input(&dir, &args, &pack);
        assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
    }
    let out = dx_input(&dir, &["--sources", "-", "lsp"], "");
    assert_eq!(out.code, 2);
    assert!(!crate::engine::reads::is_file(&dir.join(".dx")));
}

#[test]
fn json_fix_edits_reproduce_fixing_unicode_files_and_survive_cache_replay() {
    let config = r#"
lang t "*.t"
in .t {
  match shout /LOUD/ { rule lower warn "lower" fix as lower(text) }
  match remove /DEL/ { rule remove warn "remove" fix as "" }
  match insert /\+/ { rule insert warn "insert" fix space_after as " " }
  match question /\?/ { rule question warn "question" }
}
"#;
    let source = "\u{e9} LOUD DEL +?";
    let dir = project("cli-json-fix-edits", &[(".dx", config), ("a.t", source)]);
    let mut first = None;
    for cached in [false, true] {
        let out = dxlint(&dir, &["--jobs", "1", "--format", "jsonl", "--stats", "a.t"]);
        assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
        let reports: Vec<serde_json::Value> = stdout(&out).lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(reports.len(), 4);
        if cached {
            assert!(String::from_utf8_lossy(&out.stderr).contains("1 from cache"));
            assert_eq!(first.as_ref(), Some(&reports));
        }
        let mut edits = Vec::new();
        for report in &reports {
            if report["fixable"] == true {
                for edit in report["fix"]["edits"].as_array().expect("fix edits") {
                    let start = edit["start"].as_u64().unwrap() as usize;
                    let end = edit["end"].as_u64().unwrap() as usize;
                    assert!(source.is_char_boundary(start) && source.is_char_boundary(end));
                    edits.push((start, end, edit["text"].as_str().unwrap().to_owned()));
                }
            } else {
                assert!(report["fix"].is_null());
            }
        }
        assert_eq!(edits.len(), 3);
        edits.sort_by_key(|&(start, end, _)| (start, end));
        let mut external = source.to_owned();
        for (start, end, replacement) in edits.into_iter().rev() {
            external.replace_range(start..end, &replacement);
        }
        assert_eq!(external, "\u{e9} loud  + ?");
        first = Some(reports);
    }
    let fixed = dxlint(&dir, &["--jobs", "1", "--fix", "a.t"]);
    assert_eq!(fixed.code, 0, "{}", String::from_utf8_lossy(&fixed.stderr));
    assert_eq!(std::fs::read_to_string(dir.join("a.t")).unwrap(), "\u{e9} loud  + ?");
}

#[test]
fn worker_limits_preserve_single_file_and_project_diagnostics() {
    let dir = project("cli-jobs", &[(".dx", RULES), ("a.t", "LOUD SMALL"), ("b.t", "QUIET")]);
    for paths in [&["a.t"][..], &["a.t", "b.t"][..]] {
        let run = |jobs: &str| {
            let mut args = vec!["--jobs", jobs, "--no-cache", "--format", "jsonl"];
            args.extend(paths);
            let out = dxlint(&dir, &args);
            assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
            let mut diagnostics: Vec<String> = stdout(&out).lines().map(str::to_owned).collect();
            diagnostics.sort();
            diagnostics
        };
        let serial = run("1");
        assert!(!serial.is_empty());
        assert_eq!(serial, run("3"));
    }
}

#[test]
fn dx_library_reads_the_built_in_library_from_disk() {
    // A library dir whose `whitespace` module defines a different rule.
    let library = project("library-override", &[
        ("std/whitespace.dx", "in .* layer text match bang /!/ {\n  rule bang warn \"bang\"\n}\n"),
        ("std/test-extra/nested.dx", "in .* layer text match extra /!/ {\n  rule library-extra warn \"extra\"\n}\n"),
        ("rules/test-extra/nested.dx", "rule library-extra\n"),
    ]);
    let dir = project("library-user", &[(".dx", "lang t \"*.t\"\nimport .whitespace\n"), ("a.t", "hi!\n")]);
    const CHILD: &str = "DXLINT_CLI_LIBRARY_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        if std::env::var_os("DXLINT_LIBRARY").is_some() {
            let names: Vec<_> = crate::config::std_lib::names().collect();
            assert!(names.contains(&"test-extra/nested"));
            assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
            assert_eq!(crate::config::std_lib::get("ts"), None);
            assert_eq!(crate::config::std_lib::ruleset("recommended"), None);
            assert!(crate::config::std_lib::get("test-extra/nested").is_some());
            assert!(crate::config::std_lib::rulesets().any(|name| name == "test-extra/nested"));
            assert!(crate::config::std_lib::ruleset("test-extra/nested").is_some());
            assert_eq!(crate::config::std_lib::defining_language("library-extra"), Some("test-extra"));
        } else {
            assert!(crate::config::std_lib::get("ts").is_some());
            assert!(!crate::config::std_lib::names().any(|name| name == "test-extra/nested"));
        }
        print!("{}", stdout(&dxlint(&dir, &["--no-cache"])));
        return;
    }
    let run = |library: Option<&Path>| {
        let mut cmd = std::process::Command::new(std::env::current_exe().expect("test executable"));
        cmd.args(["--exact", "cli::tests::dx_library_reads_the_built_in_library_from_disk", "--nocapture"])
            .env(CHILD, "1").env_remove("DXLINT_LIBRARY");
        if let Some(l) = library {
            cmd.env("DXLINT_LIBRARY", l);
        }
        let output = cmd.output().expect("run isolated library test");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    assert!(run(Some(&library)).contains("[bang]"));
    assert!(!run(None).contains("[bang]"));
}

#[test]
fn lint_defers_unreachable_bodies_but_check_and_activation_validate_them() {
    for definition in [
        "define {\n  in token rule deferred warn \"bad\" { when dormant() }\n}",
        "in token rule deferred off \"bad\" { when dormant() }",
    ] {
        let source = format!(r#"
lang t "*.t"
in .t match token /X/
fn dormant() = missing()
{definition}
in token rule active warn "active" fix as lower(text)
"#);
        let dir = project("cli-runtime-config", &[(".dx", &source), ("test.t", "X")]);
        let out = dxlint(&dir, &["--no-cache", "--format", "jsonl", "test.t"]);
        assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
        let diagnostics: Vec<serde_json::Value> = stdout(&out).lines().map(|line| serde_json::from_str(line).expect("json")).collect();
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0]["fixable"], true);

        let check = dxlint(&dir, &["check"]);
        assert_eq!(check.code, 2, "{}", String::from_utf8_lossy(&check.stderr));

        let fixed = dxlint(&dir, &["--no-cache", "--fix", "test.t"]);
        assert_eq!(fixed.code, 0, "{}", String::from_utf8_lossy(&fixed.stderr));
        assert_eq!(std::fs::read_to_string(dir.join("test.t")).unwrap(), "x");

        std::fs::write(dir.join(".dx"), format!("{source}\nrule deferred warn\n")).unwrap();
        let activated = dxlint(&dir, &["--no-cache", "test.t"]);
        assert_eq!(activated.code, 2, "{}", String::from_utf8_lossy(&activated.stderr));
    }
}

#[test]
fn lint_validates_configured_options_even_when_the_rule_is_off() {
    for (value, status) in [("2", 0), ("\"bad\"", 2), ("unknown()", 2)] {
        let source = format!(
            "lang t \"*.t\"\nin .t match token /X/\ndefine {{\n in token rule dormant warn \"x\" {{ option max int = 1 }}\n}}\nrule dormant off {{ set max {value} }}\n"
        );
        let dir = project("cli-runtime-off-options", &[(".dx", &source), ("test.t", "X")]);
        let out = dxlint(&dir, &["--no-cache", "test.t"]);
        assert_eq!(out.code, status, "{value}: {}", String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn relative_imports_errors_exit_codes_and_fixing() {
    let dir = project(
        "cli-basic",
        &[
            (".dx", "import rules/shout\nrule no-shout error\n"),
            ("rules/shout.dx", RULES),
            ("src/a.t", "fine LOUD\nquiet # dxlint-ignore\nNOPE # dxlint-ignore no-shout\n"),
        ],
    );
    // Found from a subdirectory; the error makes the exit code 1.
    let out = dxlint(&dir.join("src"), &["--format", "jsonl"]);
    assert_eq!(out.code, 1, "{}", String::from_utf8_lossy(&out.stderr));
    let lines: Vec<serde_json::Value> = stdout(&out).lines().map(|l| serde_json::from_str(l).expect("json")).collect();
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert_eq!(lines[0]["message"], "`LOUD` is shouting");
    assert_eq!(lines[0]["severity"], "error");
    assert_eq!((lines[0]["line"].as_u64(), lines[0]["col"].as_u64()), (Some(1), Some(6)));

    let out = dxlint(&dir, &["--fix"]);
    assert_eq!(out.code, 0, "{}", stdout(&out));
    let fixed = std::fs::read_to_string(dir.join("src/a.t")).expect("read");
    assert!(fixed.starts_with("fine loud\n"), "{fixed}");
}

#[test]
fn cache_replays_results_and_relints_exactly_what_changed() {
    let rules = "import .ts\nin ts_identifier rule typed info \"{text}: {if node always number then 'number' else if node always string then 'string' else if node always boolean then 'boolean' else 'unknown'}\" { when text == \"G\" || reference_kind == \"import\" }\n";
    let dir = project(
        "cli-cache",
        &[
            (".dx", rules),
            ("tsconfig.json", "{}"),
            ("a/index.ts", "export const x: number = f();\nfunction f() { return 1 }\n"),
            ("b.ts", "import { x } from \"./a\";\nx;\nG;\n"),
        ],
    );
    // b's messages, and the `--stats` line.
    let run = || {
        let out = dxlint(&dir, &["--format", "jsonl", "--stats"]);
        assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
        let messages: Vec<String> = stdout(&out)
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("json"))
            .filter(|d| d["file"] == "b.ts")
            .map(|d| d["message"].as_str().unwrap_or_default().to_string())
            .collect();
        (messages, String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let write = |path: &str, text: &str| std::fs::write(dir.join(path), text).expect("write");

    let (cold, stats) = run();
    assert!(cold[0].contains("number") && stats.contains("2 files (0 from cache)"), "{cold:?} {stats}");
    let (warm, stats) = run();
    assert!(warm == cold && stats.contains("2 files (2 from cache)"), "{warm:?} {stats}");

    // b's type answers read a's text, so any edit to a relints b too; what
    // b reports stays the same.
    write("a/index.ts", "export const x: number = f();\nfunction f() { return 2 }\n");
    let (same, stats) = run();
    assert!(same == cold && stats.contains("(0 from cache)"), "{same:?} {stats}");

    // An export's type is what b sees.
    write("a/index.ts", "export const x: string = \"\";\n");
    let (changed, stats) = run();
    assert!(changed[0].contains("string") && stats.contains("(0 from cache)"), "{changed:?} {stats}");

    // A new `a.ts` takes over `./a` from `a/index.ts`.
    write("a.ts", "export const x: boolean = true;\n");
    let (moved, stats) = run();
    assert!(moved[0].contains("boolean") && stats.contains("3 files (1 from cache)"), "{moved:?} {stats}");

    // An undeclared name has no known value.
    assert_eq!(moved.last().map(String::as_str), Some("G: unknown"));

    // New rules start afresh (comments aside, which change nothing).
    write(".dx", &format!("# note\n{rules}"));
    assert!(run().1.contains("(3 from cache)"));
    write(".dx", &rules.replace("{text}: ", "{text} is "));
    let (renamed, stats) = run();
    assert!(renamed[0].starts_with("x is ") && stats.contains("(0 from cache)"), "{renamed:?} {stats}");
}

#[test]
fn language_only_changes_invalidate_cached_diagnostics() {
    let rules = |setting| format!("import .ts\nlanguage .js {{ set globals {{ app: \"{setting}\" }} }}\nrule no-global-assign error\n");
    let dir = project("cli-language-cache", &[(".dx", &rules("readonly")), ("a.js", "app = 1;\n")]);
    let run = || {
        let out = dxlint(&dir, &["--jobs", "1", "--format", "jsonl", "a.js"]);
        assert_eq!(out.code, 1, "{}", String::from_utf8_lossy(&out.stderr));
        stdout(&out).lines().map(|line| serde_json::from_str::<serde_json::Value>(line).expect("diagnostic")).collect::<Vec<_>>()
    };
    let cold = run();
    assert_eq!(cold.len(), 1);
    assert_eq!(run(), cold);
    assert!(dir.join(".dxcache").is_file());
    std::fs::write(dir.join(".dx"), rules("writable")).expect("write config");
    let out = dxlint(&dir, &["--jobs", "1", "--format", "jsonl", "a.js"]);
    assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).is_empty());
    std::fs::write(dir.join(".dx"), rules("readonly")).expect("write config");
    assert_eq!(run(), cold);
}

#[test]
fn describe_explains_names_and_config_goes_anywhere() {
    let dir = project("cli-describe", &[("rules.dx", RULES), ("a.t", "x\n")]);
    // `-c` before or after the subcommand.
    for args in [&["-c", "rules.dx", "describe", "no-shout"][..], &["describe", "no-shout", "-c", "rules.dx"][..]] {
        let out = dxlint(&dir, args);
        let text = stdout(&out);
        assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stderr));
        assert!(text.contains("**rule** `no-shout`") && text.contains("checks `shout`") && text.contains("rules.dx:4"), "{text}");
    }
    let out = dxlint(&dir, &["-c", "rules.dx", "describe", "shoutt"]);
    assert_eq!(out.code, 1);
    assert!(String::from_utf8_lossy(&out.stderr).contains("did you mean `shout`?"));
}

#[test]
fn config_errors_point_into_the_imported_file() {
    let dir = project(
        "cli-config-error",
        &[(".dx", "import broken\n"), ("broken.dx", "lang t \"*.t\"\nin nope rule x\n")],
    );
    let out = dxlint(&dir, &["check"]);
    assert_eq!(out.code, 2);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown matcher `nope`") && err.contains("broken.dx:2:4"), "{err}");
}

#[test]
fn a_broken_config_still_lints_with_what_works() {
    let config = format!("{RULES}in nope rule x\nrule no-shout {{ set max 3 }}\n");
    let dir = project("cli-partly-broken", &[(".dx", config.as_str()), ("a.t", "LOUD\n")]);
    let out = dxlint(&dir, &["--format", "jsonl"]);
    // The errors are printed and fail the run, but the working rule ran.
    assert_eq!(out.code, 2);
    assert!(stdout(&out).contains("`LOUD` is shouting"), "{}", stdout(&out));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown matcher `nope`") && err.contains("no option `max`"), "{err}");
}

#[test]
fn examples_find_what_their_comments_promise() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    for (dir, expected) in [("js", vec!["todo", "arrow-parens", "strict-equality", "strict-equality", "arrow-parens", "no-console", "no-console"]),
        ("rust", vec!["sorted-use-lists", "annotation-colon", "expect-lowercase", "annotation-colon"])]
    {
        let out = dxlint(&examples.join(dir), &["--format", "jsonl"]);
        let rules: Vec<String> = stdout(&out)
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("json")["rule"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(rules, expected, "examples/{dir}: {}", String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn unreadable_config_is_explained() {
    let dir = project("cli-no-config", &[("a.t", "x")]);
    let out = dxlint(&dir, &["--config", "nope.dx"]);
    assert_eq!(out.code, 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}

#[test]
fn nested_configs_import_their_parent() {
    let dir = project("cli-dot-dx", &[(".dx", RULES), ("sub/.dx", "import ../.dx\nrule no-shout error\n"), ("sub/a.t", "LOUD\n")]);
    let out = dxlint(&dir.join("sub"), &["--format", "jsonl"]);
    assert_eq!(out.code, 1, "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).contains("\"severity\":\"error\""), "{}", stdout(&out));
}

#[test]
fn foreign_language_rules_explain_the_required_import() {
    let dir = project("cli-hint", &[(".dx", "import .ts\nrule rs-indent\n"), ("a.ts", "if (x = 1) {}\nvar y = 1;\n")]);
    let out = dxlint(&dir, &["--format", "jsonl"]);
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("comes with `import .rs`"), "{error}");
}
