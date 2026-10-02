//! End-to-end behaviour of the engine: `.zlint` source in, diagnostics and
//! fixes out.

use std::path::{Path, PathBuf};

use super::{build, Diagnostic, Linter};
use crate::config::source::Disk;
use crate::config::{self, ast::Severity};

const LANG: &str = r#"
lang t = "*.t"
match paren = /\(/ * /\)/
in .t | paren match paren
in .t | paren match str = /"/ * /"/
in str match escape = /\\./
in .t | paren match comment = /\/\// * /$/
"#;

fn linter(rules: &str) -> Linter {
    let src = format!("{LANG}\n{rules}");
    let built = build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    built.linter.expect("no errors")
}

fn errors(rules: &str) -> Vec<String> {
    let src = format!("{LANG}\n{rules}");
    build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"))
        .errors
        .into_iter()
        .map(|e| format!("{} {}", e.message, e.notes.join(" ")))
        .collect()
}

fn lint(l: &Linter, text: &str) -> Vec<Diagnostic> {
    l.lint(Path::new("/root/file.t"), text)
}

fn spans<'a>(diags: &[Diagnostic], text: &'a str) -> Vec<&'a str> {
    diags.iter().map(|d| &text[d.start..d.end]).collect()
}

#[test]
fn strings_comments_and_escapes_hide_text_from_their_parent() {
    let l = linter(r#"
        in .t | paren match todo = /TODO/
        format no-todo on todo "no todos"
    "#);
    let text = r#"TODO "TODO \" TODO" // TODO
(x, "(", TODO)"#;
    let diags = lint(&l, text);
    assert_eq!(diags.len(), 2, "{diags:#?}");
    assert_eq!(diags[0].start, 0);
    assert_eq!(&text[diags[1].start..diags[1].end], "TODO");
    assert!(diags[1].start > text.find('\n').unwrap());
}

#[test]
fn nested_brackets_close_at_the_right_place() {
    let l = linter(r#"
        in .t match call = name:/\w+/ :paren
        format calls on call.paren "args" { message "{call.name}{text}" }
    "#);
    let text = r#"f(")", g(x), "\")") + h()"#;
    let diags = lint(&l, text);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(messages, vec![r#"f(")", g(x), "\")")"#, "h()"]);
}

#[test]
fn nested_layers_recurse_through_fields() {
    let l = linter(r#"
        in .t | args match call = name:/\w+/ args:args
        match args = /\(/ * /\)/
        format each-call on call.name "call" { message "{text}" }
    "#);
    let diags = lint(&l, "a(b(c()), d())");
    let names: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c", "d"]);
}

#[test]
fn shapes_report_only_differences_and_fix_them() {
    let l = linter(r#"
        in .t | paren match param = name:/\w+/ /\s*:\s*/ ty:/\w+/
        format colon on param "one space after `:`" = "{name}: {ty}"
    "#);
    let text = "(a:int, b: int, c  :  int)";
    let diags = lint(&l, text);
    assert_eq!(spans(&diags, text), vec!["a:int", "c  :  int"]);
    let fixed = l.fix(Path::new("/root/file.t"), text);
    assert_eq!(fixed.text, "(a: int, b: int, c: int)");
    assert_eq!(fixed.applied, 2);
    assert!(fixed.remaining.is_empty());
}

#[test]
fn shapes_can_transform_text_with_functions() {
    let l = linter(r#"
        in .t match def = /fn /ws? name:/\w+/
        match ws = /[ \t]+/
        format snake on def.name "snake case" = snake_case(text)
    "#);
    let fixed = l.fix(Path::new("/root/file.t"), "fn doThing fn ok_name fn HTTPServer");
    assert_eq!(fixed.text, "fn do_thing fn ok_name fn http_server");
}

#[test]
fn repeated_fields_are_lists() {
    let l = linter(r#"
        in .t match use = /use \{/ item:/\w+/ (/, */ item:/\w+/)* /\}/
        format sorted on use "sorted imports" = "use {{{join(sort(item), ', ')}}}"
    "#);
    let fixed = l.fix(Path::new("/root/file.t"), "use {b, c,a}");
    assert_eq!(fixed.text, "use {a, b, c}");
}

#[test]
fn overlays_see_text_that_layers_consumed() {
    let l = linter(r#"
        over .t match trailing = /[ \t]+$/
        format trailing-ws on trailing "trailing whitespace" = ""
    "#);
    let text = "a  \n\"multi  \nline\" \n";
    let fixed = l.fix(Path::new("/root/file.t"), text);
    assert_eq!(fixed.text, "a\n\"multi\nline\"\n");
}

#[test]
fn overlays_stay_searched_when_only_their_children_are_formatted() {
    let l = linter(r#"
        over .t match line = /^[^\n]+/
        in line match shout = /[A-Z]{3,}/
        format no-shout on shout "quiet please"
    "#);
    assert_eq!(lint(&l, "ok\nNOPE x\n").len(), 1);
}

#[test]
fn pair_filters_do_not_hide_real_matches() {
    let l = linter(r#"
        in .t match raw = /\bb?r"/ * /"/
        in .t match word = /\b[a-z]+\b/
        format raw-str on raw "raw"
        format words on word "word"
    "#);
    let text = "for r\"x\" br\"y\" bar";
    let diags = lint(&l, text);
    let got: Vec<(&str, &str)> = diags.iter().map(|d| (d.rule.as_str(), &text[d.start..d.end])).collect();
    assert_eq!(got, vec![("words", "for"), ("raw-str", "r\"x\""), ("raw-str", "br\"y\""), ("words", "bar")]);
}

#[test]
fn unclosed_groups_fail_without_eating_the_rest_of_the_file() {
    let l = linter(r#"
        in .t match call = name:/\w+/ :paren
        format calls on call "call" { message "{name}" }
    "#);
    let diags = lint(&l, "a( b(1) c(2)");
    let names: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(names, vec!["b", "c"]);
}

#[test]
fn longest_child_wins_and_ties_go_to_the_first() {
    let l = linter(r#"
        in .t match short = /ab/
        in .t match long = /abc/
        in .t match tie = /abc/
        format s on short "short"
        format l on long "long"
        format t on tie "tie"
    "#);
    let diags = lint(&l, "abc ab");
    let rules: Vec<&str> = diags.iter().map(|d| d.rule.as_str()).collect();
    assert_eq!(rules, vec!["l", "s"]);
}

#[test]
fn options_defaults_sets_and_descriptions() {
    let l = linter(r#"
        over .t match line = /^[^\n]+/
        format long-line on line "max {max}" {
          option max = 10 "max chars"
          when len(text) > max
        }
        set long-line.max = 5
        set long-line.severity = error
    "#);
    let diags = lint(&l, "123456\n12345\n");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Error);
    assert_eq!(diags[0].message, "max 5");
}

#[test]
fn ancestors_and_builtins_are_visible_from_fields() {
    let l = linter(r#"
        in .t match fun = /fn /name:/\w+/ :paren
        format where on fun.paren "x" { message "{fun.name}@{line}:{col} [{indent}]" }
    "#);
    let diags = lint(&l, "x\n  fn go(a)");
    assert_eq!(diags[0].message, "go@2:8 [  ]");
}

#[test]
fn value_tests_match_whole_values() {
    let l = linter(r#"
        match snake = /[a-z_][a-z0-9_]*/
        in .t match word = /\w+/
        format not-snake on word "x" { when !(text ~ snake) }
        format has-digit on word "y" { when text ~ /.*\d.*/ }
    "#);
    let diags = lint(&l, "ok Bad x1");
    let got: Vec<(&str, &str)> = diags.iter().map(|d| (d.rule.as_str(), &"ok Bad x1"[d.start..d.end])).collect();
    assert_eq!(got, vec![("not-snake", "Bad"), ("has-digit", "x1")]);
}

#[test]
fn macros_generate_rules() {
    let l = linter(r#"
        macro forbid(name, pat, why) {
          in .t match name = pat
          format name on name why { severity error }
        }
        forbid(dbg, "dbg!", "remove dbg! calls")
        forbid(todo_macro, "todo!", "finish this")
    "#);
    let diags = lint(&l, "dbg!(x); todo!()");
    let got: Vec<(&str, &str)> = diags.iter().map(|d| (d.rule.as_str(), d.message.as_str())).collect();
    assert_eq!(got, vec![("dbg", "remove dbg! calls"), ("todo_macro", "finish this")]);
}

#[test]
fn language_gates_keep_children_out_of_other_languages() {
    let l = linter(r#"
        lang u = "*.u"
        in .u | paren match ustr = /'/ * /'/
        in .t | .u match word = /\bword\b/
        format w on word "word"
    "#);
    // In `.t`, `'` is not a string, so `word` is visible.
    assert_eq!(l.lint(Path::new("/root/a.t"), "'word'").len(), 1);
    assert_eq!(l.lint(Path::new("/root/a.u"), "'word'").len(), 0);
}

#[test]
fn ignore_comments_suppress() {
    let l = linter(r#"
        in .t match x = /X/
        format no-x on x "no x"
    "#);
    let diags = lint(&l, "// zlint-ignore\nX\nX // zlint-ignore other\nX // zlint-ignore no-x\n");
    assert_eq!(diags.len(), 1);
}

#[test]
fn file_and_ignore_globs() {
    let l = linter(r#"
        ignore "vendor"
        in .t match x = /X/
        format no-x on x "no x" { ignore "tests/**" }
    "#);
    assert_eq!(l.lint(Path::new("/root/src/a.t"), "X").len(), 1);
    assert_eq!(l.lint(Path::new("/root/tests/a.t"), "X").len(), 0);
    assert_eq!(l.lint(Path::new("/root/vendor/a.t"), "X").len(), 0);
    assert_eq!(l.lint(Path::new("/root/src/a.txt"), "X").len(), 0);
}

#[test]
fn whole_file_targets() {
    let l = linter(r#"
        format header on .t "files start with a header" {
          when !starts_with(text, "// header")
          "// header\n" + text
        }
    "#);
    let fixed = l.fix(Path::new("/root/file.t"), "body\n");
    assert_eq!(fixed.text, "// header\nbody\n");
}

#[test]
fn runtime_failures_become_visible_diagnostics() {
    let l = linter(r#"
        in .t match x = /X+/
        format boom on x "x" { when num(text) > 1 }
    "#);
    let diags = lint(&l, "XX");
    assert_eq!(diags[0].severity, Severity::Error);
    assert!(diags[0].message.contains("failed"), "{}", diags[0].message);
}

#[test]
fn config_errors_explain_themselves() {
    let errs = errors(r#"
        in .t match a = parn
        match empty = /x?/
        in .t match loop = loop2 /x/
        match loop2 = loop /y/
        format f on nope "x"
        format g on a.missing "x"
        set nobody.severity = off
    "#);
    let all = errs.join("\n");
    assert!(all.contains("unknown matcher `parn`") && all.contains("did you mean `paren`?"), "{all}");
    assert!(all.contains("can match empty text"), "{all}");
    assert!(all.contains("left recursion"), "{all}");
    assert!(all.contains("unknown matcher `nope`"), "{all}");
    assert!(all.contains("no field `missing`"), "{all}");
    assert!(all.contains("no formatter matches `nobody`"), "{all}");
}

#[test]
fn option_types_are_checked() {
    let errs = errors(r#"
        in .t match x = /X/
        format f on x "x" { option max = 3 }
        set f.max = "three"
    "#);
    assert!(errs.iter().any(|e| e.contains("is a num")), "{errs:?}");
}

#[test]
fn std_library_parses_and_lints() {
    let src = r#"
        import "std/rust"
        import "std/js"
        import "std/python"
        import "std/c"
        import "std/zlint"
        import "std/markdown"
        import "std/toml"
        import "std/whitespace"
        import "std/rust"
        in rs_code match dbg = /\bdbg!/
        format no-dbg on dbg "no dbg"
    "#;
    let built = build(config::load_text("test.zlint", src, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    let l = built.linter.unwrap();
    let text = "fn main() {\n    let s = \"dbg!(x)\"; // dbg!\n    let c = '\"'; dbg!(s);\n}\n";
    let diags = l.lint(Path::new("/root/src/main.rs"), text);
    let rules: Vec<&str> = diags.iter().map(|d| d.rule.as_str()).collect();
    assert_eq!(rules, vec!["no-dbg"], "{diags:#?}");
}
