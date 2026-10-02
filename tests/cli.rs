//! The `zlint` binary end to end: config discovery, imports, exit codes,
//! output formats and fixing files in place.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        std::fs::write(full, text).expect("write");
    }
    dir
}

fn zlint(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zlint")).args(args).current_dir(dir).env("NO_COLOR", "1").output().expect("run zlint")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

const RULES: &str = r#"
lang t = "*.t"
in .t match shout = /\b[A-Z]{3,}\b/
format no-shout on shout "`{text}` is shouting" = lower(text)
"#;

#[test]
fn relative_imports_errors_exit_codes_and_fixing() {
    let dir = project(
        "cli-basic",
        &[
            ("zoomie.zlint", "import \"rules/shout\"\nset no-shout.severity = error\n"),
            ("rules/shout.zlint", RULES),
            ("src/a.t", "fine LOUD\nquiet # zlint-ignore\nNOPE # zlint-ignore no-shout\n"),
        ],
    );
    // Found from a subdirectory; the error makes the exit code 1.
    let out = zlint(&dir.join("src"), &["--format", "jsonl"]);
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    let lines: Vec<serde_json::Value> = stdout(&out).lines().map(|l| serde_json::from_str(l).expect("json")).collect();
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert_eq!(lines[0]["message"], "`LOUD` is shouting");
    assert_eq!(lines[0]["severity"], "error");
    assert_eq!((lines[0]["line"].as_u64(), lines[0]["col"].as_u64()), (Some(1), Some(6)));

    let out = zlint(&dir, &["--fix"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let fixed = std::fs::read_to_string(dir.join("src/a.t")).expect("read");
    assert!(fixed.starts_with("fine loud\n"), "{fixed}");
}

#[test]
fn config_errors_point_into_the_imported_file() {
    let dir = project(
        "cli-config-error",
        &[("zoomie.zlint", "import \"broken\"\n"), ("broken.zlint", "lang t = \"*.t\"\nformat x on nope \"x\"\n")],
    );
    let out = zlint(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown matcher `nope`") && err.contains("broken.zlint:2:13"), "{err}");
}

#[test]
fn examples_find_what_their_comments_promise() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    for (dir, expected) in [("js", vec!["todo", "arrow-parens", "strict-equality", "strict-equality", "arrow-parens", "no-console", "no-console"]),
        ("rust", vec!["sorted-use-lists", "annotation-colon", "expect-lowercase", "annotation-colon"])]
    {
        let out = zlint(&examples.join(dir), &["--format", "jsonl"]);
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
    let out = zlint(&dir, &["--config", "nope.zlint"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}
