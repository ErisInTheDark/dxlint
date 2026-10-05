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
lang t "*.t"
in .t match shout /\b[A-Z]{3,}\b/ {
  rule no-shout warn "`{text}` is shouting" fix as lower(text)
}
"#;

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
        let dir = project("cli-runtime-config", &[(".zlint", &source), ("test.t", "X")]);
        let out = zlint(&dir, &["--no-cache", "--format", "jsonl", "test.t"]);
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        let diagnostics: Vec<serde_json::Value> = stdout(&out).lines().map(|line| serde_json::from_str(line).expect("json")).collect();
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0]["fixable"], true);

        let check = zlint(&dir, &["check"]);
        assert_eq!(check.status.code(), Some(2), "{}", String::from_utf8_lossy(&check.stderr));

        let fixed = zlint(&dir, &["--no-cache", "--fix", "test.t"]);
        assert_eq!(fixed.status.code(), Some(0), "{}", String::from_utf8_lossy(&fixed.stderr));
        assert_eq!(std::fs::read_to_string(dir.join("test.t")).unwrap(), "x");

        std::fs::write(dir.join(".zlint"), format!("{source}\nrule deferred warn\n")).unwrap();
        let activated = zlint(&dir, &["--no-cache", "test.t"]);
        assert_eq!(activated.status.code(), Some(2), "{}", String::from_utf8_lossy(&activated.stderr));
    }
}

#[test]
fn lint_validates_configured_options_even_when_the_rule_is_off() {
    for (value, status) in [("2", 0), ("\"bad\"", 2), ("unknown()", 2)] {
        let source = format!(
            "lang t \"*.t\"\nin .t match token /X/\ndefine {{\n in token rule dormant warn \"x\" {{ option max int = 1 }}\n}}\nrule dormant off {{ set max {value} }}\n"
        );
        let dir = project("cli-runtime-off-options", &[(".zlint", &source), ("test.t", "X")]);
        let out = zlint(&dir, &["--no-cache", "test.t"]);
        assert_eq!(out.status.code(), Some(status), "{value}: {}", String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn relative_imports_errors_exit_codes_and_fixing() {
    let dir = project(
        "cli-basic",
        &[
            (".zlint", "import rules/shout\nrule no-shout error\n"),
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
fn cache_replays_results_and_relints_exactly_what_changed() {
    let rules = "import .ts\nin ts_identifier rule typed info \"{text}: {type}\" { when text == \"G\" || imported_from != null }\n";
    let dir = project(
        "cli-cache",
        &[
            (".zlint", rules),
            ("tsconfig.json", "{}"),
            ("a/index.ts", "export const x: number = f();\nfunction f() { return 1 }\n"),
            ("b.ts", "import { x } from \"./a\";\nx;\nG;\n"),
        ],
    );
    // b's messages, and the `--stats` line.
    let run = || {
        let out = zlint(&dir, &["--format", "jsonl", "--stats"]);
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

    // A body edit leaves what b sees alone: only a is linted again.
    write("a/index.ts", "export const x: number = f();\nfunction f() { return 2 }\n");
    let (same, stats) = run();
    assert!(same == cold && stats.contains("(1 from cache)"), "{same:?} {stats}");

    // An export's type is what b sees.
    write("a/index.ts", "export const x: string = \"\";\n");
    let (changed, stats) = run();
    assert!(changed[0].contains("string") && stats.contains("(0 from cache)"), "{changed:?} {stats}");

    // A new `a.ts` takes over `./a` from `a/index.ts`.
    write("a.ts", "export const x: boolean = true;\n");
    let (moved, stats) = run();
    assert!(moved[0].contains("boolean") && stats.contains("3 files (1 from cache)"), "{moved:?} {stats}");

    // A new global declaration reaches every file that asked about types
    // (the others never looked).
    // Unknown names are `any`, as TypeScript reports them.
    assert_eq!(moved.last().map(String::as_str), Some("G: any"));
    write("g.d.ts", "declare const G: number;\n");
    let (global, stats) = run();
    assert!(global.last().is_some_and(|g| g == "G: number") && stats.contains("4 files (2 from cache)"), "{global:?} {stats}");

    // New rules start afresh (comments aside, which change nothing).
    write(".zlint", &format!("# note\n{rules}"));
    assert!(run().1.contains("(4 from cache)"));
    write(".zlint", &rules.replace("{text}: ", "{text} is "));
    let (renamed, stats) = run();
    assert!(renamed[0].starts_with("x is ") && stats.contains("(0 from cache)"), "{renamed:?} {stats}");
}

#[test]
fn describe_explains_names_and_config_goes_anywhere() {
    let dir = project("cli-describe", &[("rules.zlint", RULES), ("a.t", "x\n")]);
    // `-c` before or after the subcommand.
    for args in [&["-c", "rules.zlint", "describe", "no-shout"][..], &["describe", "no-shout", "-c", "rules.zlint"][..]] {
        let out = zlint(&dir, args);
        let text = stdout(&out);
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(text.contains("**rule** `no-shout`") && text.contains("checks `shout`") && text.contains("rules.zlint:4"), "{text}");
    }
    let out = zlint(&dir, &["-c", "rules.zlint", "describe", "shoutt"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("did you mean `shout`?"));
}

#[test]
fn config_errors_point_into_the_imported_file() {
    let dir = project(
        "cli-config-error",
        &[(".zlint", "import broken\n"), ("broken.zlint", "lang t \"*.t\"\nin nope rule x\n")],
    );
    let out = zlint(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown matcher `nope`") && err.contains("broken.zlint:2:4"), "{err}");
}

#[test]
fn a_broken_config_still_lints_with_what_works() {
    let config = format!("{RULES}in nope rule x\nrule no-shout {{ set max 3 }}\n");
    let dir = project("cli-partly-broken", &[(".zlint", config.as_str()), ("a.t", "LOUD\n")]);
    let out = zlint(&dir, &["--format", "jsonl"]);
    // The errors are printed and fail the run, but the working rule ran.
    assert_eq!(out.status.code(), Some(2));
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

#[test]
fn configs_are_named_dot_zlint_and_import_each_other() {
    // A config under its old name is pointed at, not silently ignored.
    let dir = project("cli-old-name", &[("zoomie.zlint", RULES), ("a.t", "LOUD\n")]);
    let out = zlint(&dir, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("rename it"), "{}", String::from_utf8_lossy(&out.stderr));

    // A nested config builds on its parent's by importing it.
    let dir = project("cli-dot-zlint", &[(".zlint", RULES), ("sub/.zlint", "import ../.zlint\nrule no-shout error\n"), ("sub/a.t", "LOUD\n")]);
    let out = zlint(&dir.join("sub"), &["--format", "jsonl"]);
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).contains("\"severity\":\"error\""), "{}", stdout(&out));
}

#[test]
fn built_in_rules_are_off_until_turned_on() {
    let ts = "if (x = 1) {}\nvar y = 1;\n";
    let lint = |name: &str, config: &str| {
        let dir = project(name, &[(".zlint", config), ("a.ts", ts)]);
        let out = zlint(&dir, &["--format", "jsonl"]);
        let mut rules: Vec<String> = stdout(&out)
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("json")["rule"].as_str().unwrap_or_default().to_string())
            .collect();
        rules.sort();
        (rules, String::from_utf8_lossy(&out.stderr).into_owned())
    };
    assert_eq!(lint("cli-off", "import .ts\n").0, Vec::<String>::new());
    assert_eq!(lint("cli-one", "import .ts\nrule no-cond-assign\n").0, vec!["no-cond-assign"]);
    // A ruleset turns on its category; `rule x off` takes one back.
    let (rules, _) = lint("cli-ruleset", "import .ts\nimport rules:correctness\nimport rules:conventions\nrule no-var off\n");
    assert!(rules.contains(&"no-cond-assign".to_string()) && !rules.contains(&"no-var".to_string()), "{rules:?}");
    // A rule of a language not imported says which import brings it.
    let (_, err) = lint("cli-hint", "import .ts\nrule rs-indent\n");
    assert!(err.contains("comes with `import .rs`"), "{err}");
}
