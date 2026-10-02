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
fn lookbehind_tells_regex_literals_from_division() {
    let l = linter(r#"
        in .t | paren match re = /(?<=(?:^|[=(,:!&|?{};]|\breturn)[ \t]*)\// * /\/[a-z]*/
        in re match re_escape = /\\./
        in .t | paren match word = /\bx\b/
        format words on word "x"
    "#);
    // `a / x / b` is division: both `x`s are code. In the regex, `x` is hidden.
    let text = "a / x / b; f(/x\\/x/g); return /x/; y = c /x/ d";
    let diags = lint(&l, text);
    let starts: Vec<usize> = diags.iter().map(|d| d.start).collect();
    let xs: Vec<usize> = text.match_indices('x').map(|(i, _)| i).collect();
    assert_eq!(starts, vec![xs[0], xs[4]], "{diags:#?}");
}

#[test]
fn lookahead_checks_without_consuming() {
    let l = linter(r#"
        over .t match semi_line = /;(?=[ \t]*\r?\n[ \t]*\S)/
        format no-semi on semi_line "semicolon before code" = ""
    "#);
    let fixed = l.fix(Path::new("/root/file.t"), "a;\r\nb;\r\n\r\nc;\n");
    assert_eq!(fixed.text, "a\r\nb;\r\n\r\nc;\n");
}

#[test]
fn specialisations_inherit_children_and_win_ties() {
    let l = linter(r#"
        match blk = /\{/ * /\}/
        match obj like blk = /(?<==[ \t]*)\{/ * /\}/
        in .t | blk match blk
        in .t | blk match obj
        in blk match item = /\w+/
        format items on item "item" { message "{text} in {parent}" }
        format blocks on blk "block" { message "{if before ~ /.*=\s*/ then 'object' else 'block'}" }
    "#);
    let text = "x = { a } { b }";
    let messages: Vec<String> = lint(&l, text).into_iter().map(|d| d.message).collect();
    // `{ a }` is an `obj` (it inherits `item` from `blk`, and the `blocks`
    // formatter on `blk` sees it); `{ b }` is a plain `blk`.
    assert_eq!(messages, vec!["object", "a in obj", "block", "b in blk"]);
}

#[test]
fn enclosing_sees_through_overlays_into_comments_and_strings() {
    let l = linter(r#"
        over .t | paren match bad = /BAD/
        format where on bad "x" { message "{enclosing}" }
    "#);
    let messages: Vec<String> = lint(&l, "BAD // BAD\n(BAD \"BAD\")").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, vec![".t", "comment", "paren", "str"]);
}

#[test]
fn report_points_at_node_change_or_end() {
    let text = "ab   cd";
    let span_for = |report: &str| {
        let l = linter(&format!(
            r#"
            in .t match pair = /\w+ +\w+/
            format one-space on pair "x" {{
              report {report}
              re_replace(text, / +/, " ")
            }}
        "#
        ));
        let d = &lint(&l, text)[0];
        (d.start, d.end)
    };
    assert_eq!(span_for("node"), (0, 7));
    assert_eq!(span_for("change"), (3, 5));
    assert_eq!(span_for("end"), (7, 7));
}

#[test]
fn parent_before_spans_lines_within_the_parent_only() {
    let l = linter(r#"
        in paren match word = /\w+/
        format w on word "x" { message "[{parent_before}]" }
    "#);
    let messages: Vec<String> = lint(&l, "z (a\nb)").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, vec!["[(]", "[(a\n]"]);
}

#[test]
fn indent_follows_the_inside_of_declared_layers() {
    let l = linter(r#"
        in paren indent
        over .t | paren match line = /^[ \t]*\S/
        format ind on line "x" { message "[{if indented then indent_anchor else 'none'}|{indent_closes}|{indent_hangs}]" }
    "#);
    let text = "x (\n  a (b,\n    c)\n)";
    let messages: Vec<String> = lint(&l, text).into_iter().map(|d| d.message).collect();
    assert_eq!(
        messages,
        vec![
            "[none|false|false]", // not inside anything
            "[|false|false]",     // inside the outer paren, opened on an unindented line
            "[  |false|true]",    // inside `(b,` which hangs off a line indented by two
            "[|true|false]",      // the outer paren's end
        ]
    );
}

#[test]
fn indent_can_target_a_field_only() {
    let l = linter(r#"
        in .t match pair = /\[/ body:paren /\]/
        in pair.body indent
        over .t | paren match line = /^[ \t]*\S/
        format ind on line "x" { message "[{if indented then indent_anchor else 'none'}]" }
    "#);
    let text = "[(\na\n)] (\nb\n)";
    let messages: Vec<String> = lint(&l, text).into_iter().map(|d| d.message).collect();
    // `a` is inside the pair's body; `b` is in a paren that is not a body.
    assert_eq!(messages, vec!["[none]", "[]", "[]", "[none]", "[none]"]);
}

#[test]
fn broken_parts_leave_the_rest_linting() {
    let src = format!(
        r#"{LANG}
        in .t match bad = /x/ nope
        in .t match w = /W/
        format works on w "x" {{}}
        format broken on missing "x" {{}}
        set works.nope = 1
        set broken.severity = error
    "#
    );
    let built = build(config::load_text("test.zlint", &src, &Disk), PathBuf::from("/root"));
    // Settings for `broken` add no errors of their own.
    assert_eq!(built.errors.len(), 3, "{:#?}", built.errors);
    let l = built.linter.expect("a broken config still builds a linter");
    // `bad` never matches (and never crashes); `works` still reports.
    assert_eq!(lint(&l, "x W").iter().map(|d| d.rule.as_str()).collect::<Vec<_>>(), vec!["works"]);
}

#[test]
fn a_mistake_in_a_macro_is_reported_once() {
    let errs = errors(r#"
        macro bad = no_such_name
        in .t match w = /\w+/
        format a on w "x" { when bad }
        format b on w "x" { when bad }
    "#);
    assert_eq!(errs.len(), 1, "{errs:?}");
}

#[test]
fn parent_indent_is_the_indentation_where_the_layer_opens() {
    let l = linter(r#"
        in .t | paren match word = /\w+/
        format w on word "x" { message "[{parent_indent}|{parent_line}]" }
    "#);
    let messages: Vec<String> = lint(&l, "  z (\n\t\ta\n)").into_iter().map(|d| d.message).collect();
    // `z` sits in the file (no indentation); `a` in a paren opened on an
    // indented line, whatever its own indentation.
    assert_eq!(messages, vec!["[|]", "[  |  z (]"]);
}

#[test]
fn line_context_survives_nodes_ending_inside_crlf() {
    let l = linter(r#"
        in .t match upto_cr = /x[^\n]*/
        format ctx on upto_cr "x" { message "[{after}]" }
    "#);
    assert_eq!(lint(&l, "x a\r\nb")[0].message, "[]");
}

#[test]
fn regex_functions_and_line_context() {
    let l = linter(r#"
        in .t match word = /\b[a-z]+\b/
        format ctx on word "x" {
          when re_test(default(next_line, ''), "^end") || prev_line == null
          message "{text}|{before}|{after}|{re_replace(text, '([aeiou])', '<$1>')}"
        }
    "#);
    let diags = lint(&l, "one two\nmid\nend");
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(messages, vec!["one|| two|<o>n<e>", "two|one ||tw<o>", "mid|||m<i>d"]);
}

#[test]
fn patterns_that_could_match_empty_never_produce_empty_nodes() {
    let l = linter(r#"
        in .t match xs = /x*/
        in .t match lazy = /y*?z?/
        format found on xs | lazy "found" { message "{text}" }
    "#);
    let diags = lint(&l, "a xx b yyz y");
    let found: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    // Lazy repeats stop at the first non-empty match.
    assert_eq!(found, vec!["xx", "y", "yz", "y"]);
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
        match empty = /^(?=x)/
        in .t match loop = loop2 /x/
        match loop2 = loop /y/
        format f on nope "x"
        format g on a.missing "x"
        set nobody.severity = off
    "#);
    let all = errs.join("\n");
    assert!(all.contains("unknown matcher `parn`") && all.contains("did you mean `paren`?"), "{all}");
    assert!(all.contains("can only match empty text"), "{all}");
    assert!(all.contains("left recursion"), "{all}");
    assert!(all.contains("unknown matcher `nope`"), "{all}");
    assert!(all.contains("no field `missing`"), "{all}");
    assert!(all.contains("no formatter matches `nobody`"), "{all}");
}

#[test]
fn a_broken_pattern_is_one_error_not_one_per_use() {
    let errs = errors(r#"
        in .t match broken = /(/
        in broken match a = /a/
        in broken match b = /b/
        format f on broken "x"
    "#);
    assert_eq!(errs.len(), 1, "{errs:#?}");
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
