//! Engine contracts and shared helpers. Rule and DSL behaviour lives in
//! `fixtures/engine`.

use std::path::{Path, PathBuf};

use super::{build, Diagnostic, Linter};
use crate::config::source::Disk;
use crate::config::{self, ast::Severity};

mod text_hygiene;

mod ts_naming;
mod ts_core_rules;

const LANG: &str = r#"
lang t "*.t"
match paren /\(/ until /\)/
in .t, paren {
  match paren
  match str /"/ until /"/ {
    match escape /\\./
  }
  match comment /\/\// until /$/
}
"#;

/// A linter for `rules` (no config errors allowed). Shared with other
/// modules' tests, as are `errors`, `lint` and `native_linter`.
pub(crate) fn linter(rules: &str) -> Linter {
    let src = format!("{LANG}\n{rules}");
    let built = build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    built.linter.expect("no errors")
}

pub(crate) fn errors(rules: &str) -> Vec<String> {
    let src = format!("{LANG}\n{rules}");
    build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"))
        .errors
        .into_iter()
        .map(|e| format!("{} {}", e.message, e.notes.join(" ")))
        .collect()
}

pub(crate) fn lint(l: &Linter, text: &str) -> Vec<Diagnostic> {
    l.lint(Path::new("/root/file.t"), text)
}

#[test]
fn fix_application_counts_only_changed_matches() {
    let l = linter(r#"
        in .t, paren match param name:/\w+/ /\s*:\s*/ ty:/\w+/ {
          rule colon warn "one space after `:`" fix as "{name}: {ty}"
        }
    "#);
    let text = "(a:int, b: int, c  :  int)";
    let fixed = l.fix(Path::new("/root/file.t"), text);
    assert_eq!(fixed.applied, 2);
}

#[test]
fn context_blocks_preserve_message_severity() {
    let l = linter(r#"
        in .t {
          match kw /\bkw\b/ {
            rule kw-rule info "keyword"
            match kw_tail /!/
          }
          match brk /\[/ until /\]/ {
            match word /\w+/ {
              rule words warn "{text} in {kind_name(parent)}"
            }
          }
        }
        in kw_tail rule tails warn "tail"
    "#);
    let diags = lint(&l, "kw [a] x");
    assert_eq!(diags[0].severity, Severity::Info);
}

#[test]
fn big_files_report_the_same_when_their_rules_run_in_groups() {
    let mut l = native_linter("rule semi\nrule comma-spacing\nrule keyword-spacing\nrule no-unused-vars\nrule prefer-const\nrule eqeqeq\nrule indent-binary-ops");
    let text = (0..300).map(|i| format!("let a{i} = f(1 ,2)\nif(a{i} == 1){{ return }}\n")).collect::<String>();
    let path = Path::new("/root/big.ts");
    l.split_at = usize::MAX;
    let together = l.lint(path, &text);
    l.split_at = 1;
    assert!(together.len() > 300, "{}", together.len());
    for jobs in [1, 2, 4] {
        l.set_rule_jobs(jobs);
        let active = vec![true; l.rules.len()];
        assert_eq!(l.rule_groups(&active, text.len()).len(), jobs);
        let split = l.lint(path, &text);
        assert_eq!(together.iter().map(|d| (d.start, d.end, &d.rule, &d.message)).collect::<Vec<_>>(), split.iter().map(|d| (d.start, d.end, &d.rule, &d.message)).collect::<Vec<_>>());
    }
}

#[test]
fn name_classes_must_be_declared_or_opened_somewhere() {
    let errors = errors(r#"
        in paren {
          scope block
          declares value x in nearest blok
          resolves valeu
        }
    "#);
    assert!(errors.iter().any(|e| e.contains("no layer opens a scope of class `blok`")), "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("nothing declares class `valeu`")), "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("no layer has a field or attribute `x`")), "{errors:?}");
}

#[test]
fn failed_fix_conditions_produce_no_fix() {
    let l = linter(r#"
        in .t match word /[a-z]+/
        in word rule up warn "up" { fix if text == "a" as "X" }
    "#);
    let diags = lint(&l, "a b");
    assert_eq!(diags.iter().map(|d| d.fix.is_some()).collect::<Vec<_>>(), vec![true, false]);
}

#[test]
fn opener_indentation_is_corrected_in_one_fix_pass() {
    let l = linter(r#"
        in .t, paren layer lines match line /^[ \t]*\S/ {
          @macro level (if !indented then "" else if indent_closes then indent_anchor else indent_anchor "  ")
          rule ind warn "indent" {
            indent in paren
            when slice(text, 0, -1) != @level
            fix as @level slice(text, -1)
          }
        }
    "#);
    // Only the over-indented opener is wrong: its lines are right relative to
    // where it belongs, so they are not reported, and one pass fixes it all.
    let text = "x (\n    a (\n    b\n  )\n)";
    let diags = lint(&l, text);
    let (fixed, _) = super::apply_fixes(text, &diags);
    assert_eq!(fixed, "x (\n  a (\n    b\n  )\n)");
}

/// `rules` after `import .ts`; lint with `l.lint(Path::new("/root/file.ts"), text)`.
pub(crate) fn native_linter(rules: &str) -> Linter {
    let src = format!("import .ts\n{rules}");
    let built = build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    built.linter.expect("no errors")
}

/// Native-only trees keep the parser's kind and field ids.
pub(crate) fn native_front() -> crate::grammar::NativeFront {
    crate::grammar::NativeFront {
        name: "ts",
        frontend: &crate::native::ts::frontend::TS,
        base: 0,
        labels: crate::native::ts::kinds::ALL_FIELDS.iter().map(|&field| field as u16).collect(),
    }
}

/// The library's `.ts` linter, shared by native owner tests.
pub(crate) fn ts_linter() -> &'static Linter {
    static LINTER: std::sync::OnceLock<Linter> = std::sync::OnceLock::new();
    LINTER.get_or_init(|| native_linter(""))
}

/// `text` parsed at `path` with the library grammar's node ids, and the
/// front end to build its [`crate::native::ts::syntax::Syntax`] with.
pub(crate) fn ts_tree(text: &str, path: &Path) -> (crate::grammar::Tree, &'static crate::grammar::NativeFront) {
    let front = &ts_linter().grammar.natives[0];
    let parsed = front.frontend.parse(text, path).expect("parses");
    (crate::grammar::Tree::from_native(front.remap(parsed.nodes), parsed.root), front)
}

/// Gives `file` (on a [`ts_tree`]) its names, as linting does before facts.
pub(crate) fn give_names(file: &crate::native::ts::facts::NativeFile, tree: &crate::grammar::Tree, text: &str) {
    let linter = ts_linter();
    let lang = linter.grammar.lang_for(Path::new("a.ts")).expect("ts files are linted");
    let (lines, memo, reindented) = (crate::line_index::LineIndex::new(text), Default::default(), Default::default());
    linter.tree_ctx(lang, tree, (text, &lines, "a.ts"), &memo, &reindented, Some(file)).native_facts();
}

/// Runs `f` on `text` parsed as `/root/a.ts` with the library's `.ts`
/// (its names resolved, as linting does), with the node of each `/*@*/`
/// marker (the node starting right after it, outermost first): native
/// owners test their queries directly with this.
pub(crate) fn with_native_file<T>(text: &str, f: impl FnOnce(&crate::native::ts::facts::NativeFile, &[u32]) -> T) -> T {
    use crate::native::ts::{facts::NativeFile, modules::Project, syntax::Syntax};
    let path = Path::new("/root/a.ts");
    let (tree, front) = ts_tree(text, path);
    let project = Project::default();
    let file = NativeFile::new(Syntax::new(&tree, text, front), path, &project);
    give_names(&file, &tree, text);
    let marks: Vec<u32> = text.match_indices("/*@*/").map(|(at, m)| {
        let start = at + m.len() + text[at + m.len()..].len() - text[at + m.len()..].trim_start().len();
        tree.order.iter().copied().find(|&n| tree.node(n).start as usize == start && n != tree.root).expect("a node at the marker")
    }).collect();
    f(&file, &marks)
}

#[test]
fn each_tsconfig_sees_its_own_project_globals_whichever_asks_first() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("test-projects").join("globals-per-config");
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in [
        ("ui/tsconfig.json", "{}"),
        ("ui/screen.ts", "export class Screen {}\ndeclare global { let screen: Screen; }\n"),
        ("game/tsconfig.json", "{}"),
        ("game/game.ts", "export const game = 1;\n"),
    ] {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    // Both configs share their libraries; the game one asks first.
    let project = crate::native::ts::modules::Project::default();
    let game = project.file_id(&dir.join("game").join("game.ts"));
    let ui = project.file_id(&dir.join("ui").join("main.ts"));
    assert!(!project.globals(game).names.contains_key("screen"));
    assert!(project.globals(ui).names.contains_key("screen"));
}

#[test]
fn strict_implies_its_family_unless_an_option_says_otherwise() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("test-projects").join("strict-option");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("base.json"), r#"{ "compilerOptions": { "strict": true, "noImplicitThis": true } }"#).unwrap();
    std::fs::write(dir.join("tsconfig.json"), r#"{ "extends": "./base.json", "compilerOptions": { "noImplicitThis": false } }"#).unwrap();
    let project = crate::native::ts::modules::Project::default();
    let file = project.file_id(&dir.join("a.ts"));
    assert!(project.strict_option(file, "strictNullChecks"), "implied by the base's strict");
    assert!(!project.strict_option(file, "noImplicitThis"), "the nearer setting wins");
    assert!(!project.strict_option(file, "exactOptionalPropertyTypes"), "not in strict's family");
    let flags = project.compiler_flags(file);
    assert!(flags.contains(&("strictNullChecks".into(), true)) && flags.contains(&("noImplicitThis".into(), false)));
}

#[test]
fn native_layers_cannot_be_redefined_as_patterns() {
    let src = "import .ts\nmatch ts_if /y/\nin ts_call let text 1\n";
    let errors: Vec<String> = build(config::load_text("test.zlint", src, &Disk), PathBuf::from("/root"))
        .errors
        .into_iter()
        .map(|e| e.message)
        .collect();
    assert_eq!(errors.len(), 2, "{errors:#?}");
    assert!(errors[0].contains("already a matcher") && errors[1].contains("built-in"), "{errors:#?}");
}

#[test]
fn broken_parts_leave_the_rest_linting() {
    let src = format!(
        r#"{LANG}
        in .t match bad /x/ nope
        in .t match w /W/
        in w rule works warn "x"
        in missing rule broken warn "x"
        in w rule half-written warn "x" fix lower(text)
        rule works {{ set nope 1 }}
        rule broken error
        rule half-written error
    "#
    );
    let built = build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"));
    // Settings for broken rules add no errors of their own, and a rule whose
    // fix did not parse does not run without it (reporting every node).
    assert_eq!(built.errors.len(), 4, "{:#?}", built.errors);
    let l = built.linter.expect("a broken config still builds a linter");
    // `bad` never matches (and never crashes); `works` still reports.
    assert_eq!(lint(&l, "x W").iter().map(|d| d.rule.as_str()).collect::<Vec<_>>(), vec!["works"]);
}

#[test]
fn a_mistake_in_a_macro_is_reported_once() {
    let errs = errors(r#"
        @macro bad no_such_name
        in .t match w /\w+/ {
          rule a warn "x" { when @bad }
          rule b warn "x" { when @bad }
        }
    "#);
    assert_eq!(errs.len(), 1, "{errs:?}");
}

#[test]
fn option_settings_preserve_severity_overrides() {
    let l = linter(r#"
        in .t layer lines match line /^[^\n]+/ {
          rule long-line warn "max {max}" {
            option max int(0..) = 10 "max chars"
            when len(text) > max
          }
        }
        rule long-line error { set max 5 }
    "#);
    let diags = lint(&l, "123456\n12345\n");
    assert_eq!(diags[0].severity, Severity::Error);
}

#[test]
fn macro_generated_rules_preserve_severity() {
    let l = linter(r#"
        @macro forbid(name, pat, why) {
          in .t match name pat {
            rule name error why
          }
        }
        @forbid(dbg, "dbg!", "remove dbg! calls")
        @forbid(todo_macro, "todo!", "finish this")
    "#);
    let diags = lint(&l, "dbg!(x); todo!()");
    assert_eq!(diags[0].severity, Severity::Error);
}

#[test]
fn overridden_rules_forget_old_severity_settings() {
    let l = linter(r#"
        in .t match word /\b[a-z]+\b/
        in word rule shout warn "old {text}"
        rule shout error
        @macro twice(x) x x
        override in word rule shout info "new {@twice(text)}"
        override @macro twice(x) x "+" x
        in word rule echo warn "{@twice(text)}"
        override in .t match word /\b[a-z]{2}\b/
    "#);
    let diags = lint(&l, "ab abc");
    assert_eq!(diags[1].severity, Severity::Info);
}

#[test]
fn redefinitions_need_override() {
    let errs = errors(r#"
        in .t match word /\w+/
        in word rule r warn "x"
        in word rule r warn "y"
        @macro m 1
        @macro m 2
        override in word rule ghost warn "z"
    "#);
    let all = errs.join("\n");
    assert!(all.contains("rule `r` is defined twice") && all.contains("override in <layer> rule r"), "{all}");
    assert!(all.contains("macro `m` is defined twice"), "{all}");
    assert!(all.contains("nothing to override: no rule `ghost`"), "{all}");
}

#[test]
fn drop_removes_definitions_and_reports_what_still_uses_them() {
    let src = format!(
        "{LANG}\n{}",
        r#"
        in .t match word /\w+/
        in word rule a warn "a"
        in word rule b:one warn "b1"
        in word rule b:two warn "b2"
        rule a error
        rule b:* error
        drop rule a
        drop rule b:*
        rule a warn
        drop match word
    "#
    );
    let built = build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"));
    let messages: Vec<&str> = built.errors.iter().map(|e| e.message.as_str()).collect();
    // The settings above the drops went with the rules; the one below and
    // nothing else is an error. The dropped matcher's own rules went first,
    // so dropping it leaves no users.
    assert_eq!(messages, vec!["`a` is dropped here, but it is still used"], "{:#?}", built.errors);
    let err = &built.errors[0];
    let used = err.related[0].0;
    assert_eq!(&src[used.start as usize..used.end as usize], "a");
    assert_eq!(&src[err.span.unwrap().start as usize..err.span.unwrap().end as usize], "drop rule a");
}

#[test]
fn dropping_something_in_use_is_an_error_at_the_drop() {
    let src = format!(
        "{LANG}\n{}",
        r#"
        in .t match word /\w+/
        in word rule w warn "w"
        in .t match pair word /=/ word
        drop match word
    "#
    );
    let built = build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"));
    let uses: Vec<String> = built
        .errors
        .iter()
        .map(|e| {
            assert_eq!(e.message, "`word` is dropped here, but it is still used");
            let (span, _) = e.related[0];
            src[span.start as usize..span.end as usize].lines().next().unwrap_or_default().to_string()
        })
        .collect();
    // The rule's target and the other matcher's pattern.
    assert_eq!(uses.len(), 2, "{:#?}", built.errors);
}

#[test]
fn dropping_ignore_restores_file_eligibility() {
    let l = linter("ignore \"gen/**\"\ndrop ignore \"gen/**\"");
    assert!(l.wants(Path::new("/root/gen/x.t")));
}

#[test]
fn rule_activation_restores_definition_severity() {
    let l = linter(r#"
        define {
          in .t match word /[a-z]+/
          in word rule quiet error "quiet {text}"
          in word rule shy warn "shy {text}"
          in word rule loud:a info "a"
          in word rule loud:b info "b"
        }
        rule loud:*
        rule shy
        rule shy hint
        rule quiet
        rule quiet off
        rule quiet
    "#);
    let diags = lint(&l, "ab");
    let got: Vec<(&str, Severity)> = diags.iter().map(|d| (d.rule.as_str(), d.severity)).collect();
    // `rule x` turns on at the definition's severity, a severity alone keeps
    // the message, and `off` then `rule x` turns it on again.
    assert_eq!(got, vec![("loud:a", Severity::Info), ("loud:b", Severity::Info), ("quiet", Severity::Error), ("shy", Severity::Hint)]);
}

#[test]
fn early_rule_settings_and_partial_rulesets_compile() {
    let l = linter(r#"
        rule late error
        define {
          in .t match word /[a-z]+/ { rule late warn "late" }
        }
    "#);
    assert_eq!(lint(&l, "x")[0].severity, Severity::Error);
    // A built-in ruleset names rules of languages not imported.
    let src = format!("{LANG}\nimport rules:format\n");
    let built = build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
}

#[test]
fn pruned_library_rules_remain_in_inert_metadata() {
    let src = r#"
        define {
          in .t match lib_word /[a-z]+/ { rule lib warn "lib" }
          in .t match lib_other /[0-9]+/ { rule other warn "other" { when parent is lib_word } }
        }
        in .t match mine /[a-z]+/ { rule mine warn "mine" }
    "#;
    let l = linter(src);
    assert!(l.inert.iter().any(|r| r.name == "lib") && l.inert.iter().any(|r| r.name == "other"), "{:?}", l.inert.iter().map(|r| &r.name).collect::<Vec<_>>());
}

#[test]
fn invalid_option_types_defaults_and_legacy_syntax_are_rejected() {
    let errs = errors(r#"
        schema word_choice { word: string = "*" "which word"; say?: string }
        in .t match word /[a-z]+/ {
          rule pick warn "x" { option config word_choice[] = [] }
          rule count warn "x" { option max int(0..) = 2 }
          rule mode warn "x" { option style "a" | "b" = "c" }
          rule pattern warn "x" { option ignore regex = "" }
        }
        rule pick { set config [{ word: "x" }, { word: 1 }] }
        rule count { set max -1 }
        rule pattern { set ignore "(" }
    "#);
    let has = |s: &str| errs.iter().any(|e| e.contains(s));
    assert!(has("`[1].word` must be string, got 1 (which word)"), "{errs:?}");
    assert!(has("`max` of `count`: must be int(0..), got -1"), "{errs:?}");
    assert!(has("the default of option `style` does not fit its type"), "{errs:?}");
    assert!(has("is not a valid regex"), "{errs:?}");
    let old = errors(r#"
        in .t match word /[a-z]+/ {
          rule f warn "x" { option max 3 "old style" }
          rule g warn "x" { option kind strng = "" }
        }
    "#);
    assert!(old.iter().any(|e| e.contains("option `max` needs a default")), "{old:?}");
    assert!(old.iter().any(|e| e.contains("unknown type `strng`")), "{old:?}");
}

#[test]
fn nested_fact_arguments_are_restored_after_the_call() {
    use crate::expr::{facts, Value};
    let inner = facts::with_args(vec![Value::Num(1.0)], || {
        let nested = facts::with_args(vec![Value::Num(2.0)], || Value::list(facts::args()));
        Value::list(vec![nested, Value::list(facts::args())])
    });
    assert_eq!(inner, Value::list(vec![Value::list(vec![Value::Num(2.0)]), Value::list(vec![Value::Num(1.0)])]));
    assert!(facts::args().is_empty());
}

#[test]
fn rule_lets_reject_direct_self_reference() {
    // A `let` naming itself directly is an error: it reads on other nodes.
    let built = build(config::load_text("t.zlint", "import .ts\nin ts_identifier rule r { let x x + 1 }\n", &Disk), PathBuf::from("/root"));
    assert_eq!(built.errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["`x` refers to itself"]);
}

#[test]
fn load_time_iteration_rejects_node_values() {
    // `@each` reads options only.
    let built = build(config::load_text("t.zlint", "import .ts\nin ts_identifier rule r { @each x in [text] }\n", &Disk), PathBuf::from("/root"));
    assert_eq!(built.errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["`text` is not a constant"]);
}

#[test]
fn config_functions_see_no_node_and_take_no_builtin_names() {
    for (src, wanted) in [
        ("fn f(x) = text\n", "`text` is not a parameter of `f`"),
        ("fn upper(x) = x\n", "`upper` is a built-in function or fact; pick another name"),
        ("fn g(x) = x\nin .t match w /a/ { rule r info \"{g(1, 2)}\" }\n", "`g` takes 1 argument(s), got 2"),
    ] {
        let built = build(config::load_text("test.zlint", &format!("lang t \"*.t\"\n{src}"), &Disk), PathBuf::from("/root"));
        let messages: Vec<&str> = built.errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, [wanted], "{src}");
    }
}

#[test]
fn message_overrides_preserve_their_severity() {
    let l = linter(r#"
        in .t match word /[a-z]+/ { rule w warn "word" }
        rule w info "short {text}" { when len(text) < 3 }
    "#);
    let diags = lint(&l, "ab abcd");
    assert_eq!(diags[0].severity, Severity::Info);
}

#[test]
fn inspect_names_the_layers_fields_and_facts_at_a_position() {
    let l = native_linter("in ts_call rule c warn \"c\"");
    let text = "const n = 1;\nparseInt(\"2\");\n";
    let at = text.find("parseInt").unwrap() + 2;
    let md = l.inspect(Path::new("/root/file.ts"), text, at).expect("linted");
    // Innermost first, each with the field it sits in, then the facts.
    let identifier = md.find("`ts_identifier` — `callee` of `ts_call`").expect(&md);
    let call = md.find("`ts_call` — `expression` of `ts_expression_statement`").expect(&md);
    assert!(identifier < call, "{md}");
    assert!(md.contains("`resolved` = \"global\""), "{md}");
}

#[test]
fn runtime_failures_are_error_severity() {
    let l = linter(r#"
        in .t match x /X+/ {
          rule boom warn "x" { when num(text) > 1 }
        }
    "#);
    let diags = lint(&l, "XX");
    assert_eq!(diags[0].severity, Severity::Error);
}

#[test]
fn config_errors_explain_themselves() {
    let errs = errors(r#"
        in .t match a parn
        match empty /^(?=x)/
        in .t match loop loop2 /x/
        match loop2 loop /y/
        in nope rule f warn "x"
        in a.missing rule g warn "x"
        rule nobody off
    "#);
    let all = errs.join("\n");
    assert!(all.contains("unknown matcher `parn`") && all.contains("did you mean `paren`?"), "{all}");
    assert!(all.contains("can only match empty text"), "{all}");
    assert!(all.contains("left recursion"), "{all}");
    assert!(all.contains("unknown matcher `nope`"), "{all}");
    assert!(all.contains("no field `missing`"), "{all}");
    assert!(all.contains("no rule `nobody`"), "{all}");
}

#[test]
fn a_broken_pattern_is_one_error_not_one_per_use() {
    let errs = errors(r#"
        in .t match broken /(/ {
          match a /a/
          match b /b/
          rule f warn "x"
        }
    "#);
    assert_eq!(errs.len(), 1, "{errs:#?}");
}

#[test]
fn option_types_are_checked() {
    let errs = errors(r#"
        in .t match x /X/ {
          rule f warn "x" { option max number = 3 }
        }
        rule f { set max "three" }
    "#);
    assert!(errs.iter().any(|e| e.contains("must be number, got \"three\"")), "{errs:?}");
}

#[test]
fn all_std_rules_and_rulesets_compile_without_inert_targets() {
    let src = r#"
        import .rs
        import .ts
        import .py
        import .c
        import .zlint
        import .md
        import .toml
        import .rs
        in rs_code match dbg /\bdbg!/ {
          rule no-dbg warn "no dbg"
        }
    "#;
    let built = build(config::load_text("test.zlint", src, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    // Every built-in rule and ruleset compiles when turned on.
    let all = format!("{src}\nimport rules:correctness\nimport rules:suspicious\nimport rules:conventions\nimport rules:format\nimport rules:types\nimport rules:recommended\nrule *\n");
    let built = build(config::load_text("test.zlint", &all, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    let l = built.linter.unwrap();
    for rule in &l.inert {
        for target in rule.target_text.split(',') {
            assert!(l.grammar.group(target.trim()).is_some_and(|members| members.is_empty()), "{}: unexpected inert target {target}", rule.name);
        }
    }
    assert!(l.rules.iter().all(|r| r.severity != Severity::Off));
}
