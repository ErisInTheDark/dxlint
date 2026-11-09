//! File-scoped language option tests: they build a linter and lint files.

use std::path::{Path, PathBuf};

use crate::config::{load_text, source::Disk};
use crate::engine::{build, Linter};

fn with_rules(settings: &str, rules: &str) -> Linter {
    let source = format!("import .ts\n{settings}\n{rules}\n");
    let built = build(load_text("test.dx", &source, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    built.linter.expect("compiled")
}

#[test]
fn rule_workers_receive_the_same_file_environment() {
    let mut linter = with_rules(r#"language .js {
      set sourceType "commonjs"
      set globals { app: "readonly" }
    }"#, "rule es-no-global-assign\nin ts_program rule wrapper { when source_environment.root_function\nwarn \"wrapped\" }");
    let source = "app = 1;";
    linter.set_rule_jobs(1);
    let serial = linter.lint(Path::new("/root/a.js"), source);
    assert_eq!(serial.len(), 2);
    linter.split_at = 1;
    linter.set_rule_jobs(2);
    let parallel = linter.lint(Path::new("/root/a.js"), source);
    let reports = |diagnostics: Vec<crate::engine::Diagnostic>| diagnostics.into_iter()
        .map(|diagnostic| (diagnostic.rule, diagnostic.start, diagnostic.message)).collect::<Vec<_>>();
    assert_eq!(reports(serial), reports(parallel));
}

#[test]
fn language_schemas_and_filtered_settings_stay_independent() {
    let linter = with_rules(r#"
import .json
language .json {
  option ecmaVersion boolean = true
  set ecmaVersion false
}
language .js {
  files "legacy/**"
  set ecmaVersion 3
}
"#, "rule es-no-unused-expressions");
    let json = linter.grammar.lang("json").expect("JSON language");
    let edition = linter.language_options.values(json, Path::new("legacy/a.json"))
        .filter(|(name, _)| *name == "ecmaVersion").last().map(|(_, value)| value);
    assert_eq!(edition, Some(&crate::expr::Value::Bool(false)));
}

#[test]
fn environment_preserves_matching_patch_order_in_rule_workers() {
    let mut linter = with_rules(r#"
language .js { set globals { app: "readonly" } }
language .js {
  files "legacy/**"
  set globals { app: "off" }
}
"#, r#"
in ts_program rule environment:patches {
  let records { value environment.settings; filter (setting) setting.name == "globals"; map (setting) setting.value.app }
  let ordered join(records, "|")
  when ordered == "|readonly|off"
  warn "matched"
}
in ts_program rule environment:edition {
  let edition { value environment.settings; filter (setting) setting.name == "ecmaVersion"; last }
  when edition.value == null
  warn "default"
}
"#);
    linter.set_rule_jobs(1);
    assert_eq!(linter.lint(Path::new("/root/legacy/a.js"), "").len(), 2);
    assert_eq!(linter.lint(Path::new("/root/a.js"), "").len(), 1);
    linter.split_at = 1;
    linter.set_rule_jobs(2);
    assert_eq!(linter.lint(Path::new("/root/legacy/a.js"), "").len(), 2);
    assert_eq!(linter.lint(Path::new("/root/a.js"), "").len(), 1);
}
