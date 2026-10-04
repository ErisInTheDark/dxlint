//! End-to-end behaviour of the engine: `.zlint` source in, diagnostics and
//! fixes out.

use std::path::{Path, PathBuf};

use super::{build, Diagnostic, Linter};
use crate::config::source::Disk;
use crate::config::{self, ast::Severity};

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
/// modules' tests, as are `errors`, `lint`, `native_linter` and
/// `rule_messages`.
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

fn spans<'a>(diags: &[Diagnostic], text: &'a str) -> Vec<&'a str> {
    diags.iter().map(|d| &text[d.start..d.end]).collect()
}

#[test]
fn strings_comments_and_escapes_hide_text_from_their_parent() {
    let l = linter(r#"
        in .t, paren match todo /TODO/ {
          rule no-todo warn "no todos"
        }
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
        in .t match call name:/\w+/ :paren
        in call.paren rule calls warn "{call.name}{text}"
    "#);
    let text = r#"f(")", g(x), "\")") + h()"#;
    let diags = lint(&l, text);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(messages, vec![r#"f(")", g(x), "\")")"#, "h()"]);
}

#[test]
fn nested_layers_recurse_through_fields() {
    let l = linter(r#"
        in .t, args match call name:/\w+/ args:args
        match args /\(/ until /\)/
        in call.name rule each-call warn "{text}"
    "#);
    let diags = lint(&l, "a(b(c()), d())");
    let names: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c", "d"]);
}

#[test]
fn fixes_report_only_differences_and_fix_them() {
    let l = linter(r#"
        in .t, paren match param name:/\w+/ /\s*:\s*/ ty:/\w+/ {
          rule colon warn "one space after `:`" fix as "{name}: {ty}"
        }
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
fn fixes_can_transform_text_with_functions() {
    let l = linter(r#"
        in .t match def /fn /ws? name:/\w+/
        match ws /[ \t]+/
        in def.name rule snake warn "snake case" fix as snake_case(text)
    "#);
    let fixed = l.fix(Path::new("/root/file.t"), "fn doThing fn ok_name fn HTTPServer");
    assert_eq!(fixed.text, "fn do_thing fn ok_name fn http_server");
}

#[test]
fn repeated_fields_are_lists() {
    let l = linter(r#"
        in .t match use /use \{/ item:/\w+/ (/, */ item:/\w+/)* /\}/ {
          rule sorted warn "sorted imports" fix as "use {{{join(sort(item), ', ')}}}"
        }
    "#);
    let fixed = l.fix(Path::new("/root/file.t"), "use {b, c,a}");
    assert_eq!(fixed.text, "use {a, b, c}");
}

#[test]
fn context_blocks_give_every_line_their_layers() {
    let l = linter(r#"
        in .t {
          match kw /\bkw\b/ {
            rule kw-rule info "keyword"
            match kw_tail /!/
          }
          match brk /\[/ until /\]/ {
            match word /\w+/ {
              rule words warn "{text} in {parent}"
            }
          }
        }
        in kw_tail rule tails warn "tail"
    "#);
    let diags = lint(&l, "kw [a] x");
    let got: Vec<(&str, &str)> = diags.iter().map(|d| (d.rule.as_str(), d.message.as_str())).collect();
    // `kw` has no `!` after it, so `kw_tail` never matches inside it.
    assert_eq!(got, vec![("kw-rule", "keyword"), ("words", "a in brk")]);
    assert_eq!(diags[0].severity, Severity::Info);
}

#[test]
fn rules_on_a_group_check_every_member() {
    let l = linter(r#"
        in .t match a /A/
        in .t match b /B/
        group ab a
        group ab b
        in ab rule letters warn "{text}"
    "#);
    let messages: Vec<String> = lint(&l, "A B C").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, vec!["A", "B"]);
}

#[test]
fn overlays_see_text_that_layers_consumed() {
    let l = linter(r#"
        over .t match trailing /[ \t]+$/ {
          rule trailing-ws warn "trailing whitespace" fix as ""
        }
    "#);
    let text = "a  \n\"multi  \nline\" \n";
    let fixed = l.fix(Path::new("/root/file.t"), text);
    assert_eq!(fixed.text, "a\n\"multi\nline\"\n");
}

#[test]
fn overlays_stay_searched_when_only_their_children_are_checked() {
    let l = linter(r#"
        over .t match line /^[^\n]+/ {
          match shout /[A-Z]{3,}/ {
            rule no-shout warn "quiet please"
          }
        }
    "#);
    assert_eq!(lint(&l, "ok\nNOPE x\n").len(), 1);
}

#[test]
fn pair_filters_do_not_hide_real_matches() {
    let l = linter(r#"
        in .t {
          match raw /\bb?r"/ until /"/ {
            rule raw-str warn "raw"
          }
          match word /\b[a-z]+\b/ {
            rule words warn "word"
          }
        }
    "#);
    let text = "for r\"x\" br\"y\" bar";
    let diags = lint(&l, text);
    let got: Vec<(&str, &str)> = diags.iter().map(|d| (d.rule.as_str(), &text[d.start..d.end])).collect();
    assert_eq!(got, vec![("words", "for"), ("raw-str", "r\"x\""), ("raw-str", "br\"y\""), ("words", "bar")]);
}

#[test]
fn unclosed_groups_fail_without_eating_the_rest_of_the_file() {
    let l = linter(r#"
        in .t match call name:/\w+/ :paren {
          rule calls warn "{name}"
        }
    "#);
    let diags = lint(&l, "a( b(1) c(2)");
    let names: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(names, vec!["b", "c"]);
}

#[test]
fn lookbehind_tells_regex_literals_from_division() {
    let l = linter(r#"
        in .t, paren match re /(?<=(?:^|[=(,:!&|?{};]|\breturn)[ \t]*)\// until /\/[a-z]*/ {
          match re_escape /\\./
        }
        in .t, paren match word /\bx\b/ {
          rule words warn "x"
        }
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
        over .t match semi_line /;(?=[ \t]*\r?\n[ \t]*\S)/ {
          rule no-semi warn "semicolon before code" fix as ""
        }
    "#);
    let fixed = l.fix(Path::new("/root/file.t"), "a;\r\nb;\r\n\r\nc;\n");
    assert_eq!(fixed.text, "a\r\nb;\r\n\r\nc;\n");
}

#[test]
fn specialisations_inherit_children_and_win_ties() {
    let l = linter(r#"
        match blk /\{/ until /\}/
        match obj like blk /(?<==[ \t]*)\{/ until /\}/
        in .t, blk {
          match blk
          match obj
        }
        in blk match item /\w+/ {
          rule items warn "{text} in {parent}"
        }
        in blk rule blocks warn "{if before ~ /.*=\s*/ then 'object' else 'block'}"
    "#);
    let text = "x = { a } { b }";
    let messages: Vec<String> = lint(&l, text).into_iter().map(|d| d.message).collect();
    // `{ a }` is an `obj` (it inherits `item` from `blk`, and the `blocks`
    // rule on `blk` sees it); `{ b }` is a plain `blk`.
    assert_eq!(messages, vec!["object", "a in obj", "block", "b in blk"]);
}

#[test]
fn enclosing_sees_through_overlays_into_comments_and_strings() {
    let l = linter(r#"
        over .t, paren match bad /BAD/ {
          rule where warn "{enclosing}"
        }
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
            in .t match pair /\w+ +\w+/
            in pair rule one-space warn "x" {{
              report {report}
              fix as re_replace(text, / +/, " ")
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
        in paren match word /\w+/ {
          rule w warn "[{parent_before}]"
        }
    "#);
    let messages: Vec<String> = lint(&l, "z (a\nb)").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, vec!["[(]", "[(a\n]"]);
}

#[test]
fn indent_follows_the_inside_of_declared_layers() {
    let l = linter(r#"
        in paren indent
        over .t, paren match line /^[ \t]*\S/ {
          rule ind warn "[{if indented then indent_anchor else 'none'}|{indent_closes}|{indent_hangs}]"
        }
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
        in .t match pair /\[/ body:paren /\]/
        in pair.body indent
        over .t, paren match line /^[ \t]*\S/ {
          rule ind warn "[{if indented then indent_anchor else 'none'}]"
        }
    "#);
    let text = "[(\na\n)] (\nb\n)";
    let messages: Vec<String> = lint(&l, text).into_iter().map(|d| d.message).collect();
    // `a` is inside the pair's body; `b` is in a paren that is not a body.
    assert_eq!(messages, vec!["[none]", "[]", "[]", "[none]", "[none]"]);
}

#[test]
fn indent_inside_a_matcher_block_indents_that_matcher() {
    let l = linter(r#"
        in .t match blk /\[/ until /\]/ {
          indent
        }
        over .t, blk match line /^[ \t]*\S/ {
          rule ind warn "{indented}"
        }
    "#);
    let messages: Vec<String> = lint(&l, "[\na\n]").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, vec!["false", "true", "true"]);
}

#[test]
fn lines_inside_a_fixed_opener_follow_its_fixed_indentation() {
    let l = linter(r#"
        in paren indent
        over .t, paren match line /^[ \t]*\S/ {
          macro level (if !indented then "" else if indent_closes then indent_anchor else indent_anchor "  ")
          rule ind warn "indent" {
            when slice(text, 0, -1) != level
            fix as level slice(text, -1)
          }
        }
    "#);
    // Only the over-indented opener is wrong: its lines are right relative to
    // where it belongs, so they are not reported, and one pass fixes it all.
    let text = "x (\n    a (\n    b\n  )\n)";
    let diags = lint(&l, text);
    assert_eq!(diags.len(), 1, "{diags:#?}");
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

/// Runs `f` on `text` parsed as `/root/a.ts`, with the node of each `/*@*/`
/// marker (the node starting right after it, outermost first): native
/// owners test their queries directly with this.
pub(crate) fn with_native_file<T>(text: &str, f: impl FnOnce(&crate::native::ts::facts::NativeFile, &[u32]) -> T) -> T {
    use crate::grammar::Tree;
    use crate::native::ts::{facts::NativeFile, syntax::Syntax};
    let l = native_linter("");
    let front = l.grammar.natives.first().expect("native ts");
    let parsed = crate::native::ts::parse(text, false).expect("parses");
    let tree = Tree::from_native(front.remap(parsed.nodes), parsed.root);
    let path = Path::new("/root/a.ts");
    let file = NativeFile::new(Syntax::new(&tree, text, front), path, &l.project);
    let marks: Vec<u32> = text.match_indices("/*@*/").map(|(at, m)| {
        let start = at + m.len() + text[at + m.len()..].len() - text[at + m.len()..].trim_start().len();
        tree.order.iter().copied().find(|&n| tree.node(n).start as usize == start && n != tree.root).expect("a node at the marker")
    }).collect();
    f(&file, &marks)
}

#[test]
fn native_layers_carry_fields_and_attributes() {
    let l = native_linter(r#"
        in ts_logical let ops 1 + left.ops + right.ops
        in ts_parenthesized let ops expression.ops
        in ts_call let ops sum(argument.ops)
        in ts_if.test rule complex warn "{ops} in `{text}`" { when ops > 1 }
        in ts_call.callee rule callee info "{text}"
    "#);
    let text = "if (a && b || c) {}\nif ((a && b)) f(x || y && z);\nif (g(a || b, c && d)) {}\n";
    let messages: Vec<String> = l.lint(Path::new("/root/file.ts"), text).into_iter().map(|d| d.message).collect();
    assert_eq!(messages, vec!["2 in `a && b || c`", "f", "g", "2 in `g(a || b, c && d)`"]);
    // Other files never get the native parse.
    assert!(l.lint(Path::new("/root/file.t"), text).is_empty());
}

#[test]
fn bindings_count_value_type_and_self_uses() {
    let l = native_linter(r#"
        in ts_identifier rule b info "{text}:{declaration_kind}:{uses}/{value_uses}/{self_uses}/{writes}" { when uses != null }
    "#);
    let text = "import { A, B, C } from \"m\";\nlet x: A = 1;\ntype T = typeof B;\nconst f = () => f();\ninterface D {}\nconst D = 1;\nx = C;\nexport { D };\n";
    let messages: Vec<String> = l.lint(Path::new("/root/file.ts"), text).into_iter().map(|d| d.message).collect();
    assert_eq!(
        messages,
        vec![
            "A:import:1/0/0/0",     // a type position only
            "B:import:1/0/0/0",     // `typeof B` in a type needs no runtime value
            "C:import:1/1/0/0",
            "x:let:0/0/0/1",        // written, never read
            "T:type:0/0/0/0",
            "f:const:1/1/1/0",      // only its own body calls it
            "D:interface:1/1/0/0",  // merged with `const D`: one binding
        ]
    );
}

#[test]
fn files_the_native_parser_cannot_read_say_so() {
    let l = native_linter("in ts_call rule c warn \"call\"");
    let diags = l.lint(Path::new("/root/file.ts"), "f(;\n");
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert!(diags[0].message.starts_with("could not parse this file as TypeScript"), "{}", diags[0].message);
}

/// Writes `files` under a fresh directory in `target/` and lints `main`
/// with `rules`, returning the messages.
pub(crate) fn lint_project(name: &str, files: &[(&str, &str)], main: &str, rules: &str) -> Vec<String> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("test-projects").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
    }
    let src = format!("import .ts\n{rules}");
    let built = build(config::load_text("test.zlint", &src, &Disk), dir.clone());
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    let l = built.linter.unwrap();
    let text = std::fs::read_to_string(dir.join(main)).unwrap();
    l.lint(&dir.join(main), &text).into_iter().map(|d| d.message).collect()
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
fn declared_types_follow_imports_members_and_initializers() {
    let messages = lint_project(
        "types-basic",
        &[
            ("tsconfig.json", "{ \"compilerOptions\": { \"paths\": { \"@app/*\": [\"./src/*\"] } } }"),
            (
                "src/a.ts",
                "import { Emitter } from \"./events\";\n@decorate\nexport default class Store<T = string> extends Emitter.Host<number> {\n  value!: T;\n  save(): Task<T> { return 0 as never }\n  stop(): never { throw 0 }\n}\nexport interface Box { store: Store<number>; }\ninterface Task<R> { then(ok: (r: R) => void): void; }\n",
            ),
            ("src/events.ts", "export namespace Emitter {\n  export class Host<E> {\n    on(event: E): this { return this }\n  }\n}\n"),
            (
                "src/b.ts",
                "import Store, { type Box } from \"@app/a\";\nclass Holder {\n  store = new Store();\n  run(box: Box) {\n    this.store.save();\n    box.store.value;\n    this.store.stop();\n    this.store.on(1);\n  }\n}\n",
            ),
        ],
        "src/b.ts",
        "in ts_expression_statement.expression rule t info \"{default(type, '?')}\"",
    );
    assert_eq!(messages, vec!["Task<string>", "number", "never", "Store<string>"]);
}

/// The type of every expression statement in a single file.
fn statement_types(text: &str) -> Vec<String> {
    let l = native_linter("in ts_expression_statement.expression rule t info \"{default(type, '?')}\"");
    l.lint(Path::new("/root/file.ts"), text).into_iter().map(|d| d.message).collect()
}

#[test]
fn narrowing_follows_guards_until_a_reassignment() {
    let text = "\
interface Foo { a: number }
declare const b: string | number;
declare const maybe: Foo | null;
function f(x: Foo | undefined, y: string | undefined) {
  if (x) { x; }
  if (typeof b === \"string\") { b; } else { b; }
  if (!x) return;
  x;
  if (y) { y = undefined; y; }
  maybe && maybe;
}
";
    // After `y = undefined` the guard no longer holds (zlint does not narrow
    // by assignment): back to the declared type. `maybe && maybe` is the
    // falsy part of the left plus the narrowed right.
    assert_eq!(statement_types(text), vec!["Foo", "string", "number", "Foo", "undefined", "string | undefined", "null | Foo"]);
}

#[test]
fn overloads_and_same_file_returns() {
    let text = "\
function over(x: string): string;
function over(x: number): number;
function over(x: any): any { return x; }
function plain() { return 1; }
function loop() { return loop(); }
over(1);
over(\"a\");
plain();
loop();
";
    // The implementation signature is not callable; unannotated functions
    // of this file are typed from their returns (recursion: unknown).
    assert_eq!(statement_types(text), vec!["number", "string", "number", "?"]);
}

#[test]
fn redundant_type_arguments_use_mutual_assignability() {
    let l = native_linter(
        "in ts_call.type_argument, ts_type_reference.type_argument rule t info \"{text}:{type_argument_redundant}\" { when type_argument_redundant != null }",
    );
    let text = "\
enum E { A, B }
enum S { A = \"a\" }
declare function make<T = number>(): T;
declare function same<T>(x: T): T;
class Box<T = string> {}
make<E>();
make<S>();
make<string>();
same<number>(1);
let b: Box<string>;
";
    let messages: Vec<String> = l.lint(Path::new("/root/file.ts"), text).into_iter().map(|d| d.message).collect();
    // A numeric enum and `number` are assignable both ways; a string enum is not.
    assert_eq!(messages, vec!["E:default", "number:inferred", "string:default"]);
}

#[test]
fn class_members_functions_and_constructors() {
    let l = native_linter(
        "in ts_class_property rule m info \"{key}:{member_reads}/{member_writes}/{member_constructor_writes}\"\n\
         in ts_function rule f info \"{name}:{uses_this}:{uses_arguments}:{has_yield}\"\n\
         in ts_method rule c info \"{super_calls}\" { when super_calls != null }",
    );
    let text = "\
class C {
  private a = 1;
  private b = 2;
  #c = 3;
  constructor() { this.b = 3; setTimeout(() => { this.a = 4; }); }
  m(other: C) { other.b = 5; return this.#c; }
}
class D extends C { constructor(x: boolean) { if (x) { super(); } } }
class E extends C { constructor() { super(); } }
function p() { return this; }
function q() { return () => arguments; }
function* r() { yield 1; }
";
    let messages: Vec<String> = l.lint(Path::new("/root/file.ts"), text).into_iter().map(|d| d.message).collect();
    assert_eq!(
        messages,
        vec![
            // A callback in the constructor runs later; `other.b` is the same private member.
            "a:0/1/0",
            "b:0/1/1",
            "#c:1/0/0",
            // Only constructors of derived classes call `super()`.
            "some",
            "all",
            "p:true:false:false",
            "q:false:true:false",
            "r:false:false:true",
        ]
    );
}

#[test]
fn regexes_and_regexp_patterns() {
    let l = native_linter(
        "in ts_regex, ts_call, ts_new rule r info \"{f.check}:{f.kind}@{f.offset}\" { each f in default(regex_findings, []) }\n\
         in ts_call, ts_new rule e info \"{default(regex_flags_error(default(regex_flags, ''), []), regex_pattern_error)}\" { when regex_pattern_error != null || regex_flags_error(default(regex_flags, ''), []) != null }",
    );
    // Literals, and patterns of `RegExp` (an alias too) written in strings:
    // findings point into the string.
    let text = "/a  b/;\n/(a)|\\1/;\nnew RegExp(\"(\");\nRegExp(\"a\", \"gg\");\nconst R = RegExp; R(\"x\\\\1(y)\");\n";
    let messages: Vec<String> = l.lint(Path::new("/root/file.ts"), text).into_iter().map(|d| d.message).collect();
    assert_eq!(
        messages,
        vec![
            "spaces:2@2",
            "backref:disjunctive@13",
            "Invalid regular expression: /(/: Unterminated group",
            "Duplicate flags ('g') supplied to RegExp constructor",
            // At the `\\1` in the string.
            "backref:forward@76",
        ]
    );
}

#[test]
fn facts_of_fields_and_parents_and_sibling_builtins() {
    let l = native_linter(
        "in ts_call rule c info \"{callee}:{callee.resolved}\"\n\
         in ts_call.argument rule a info \"{index}:{text}:{parent.type_flags == null}\"\n\
         in ts_expression_statement rule g info \"{default(gap_before, 'first')}\" { when index > 0 }",
    );
    let text = "import { f } from \"m\";\nf(a, b);\n\n// note\ng(c);\n";
    let messages: Vec<String> = l.lint(Path::new("/root/file.ts"), text).into_iter().map(|d| d.message).collect();
    assert_eq!(messages, vec!["f:import", "\n", "0:a:true", "1:b:true", "g:global", "\n\n// note\n", "0:c:false"]);
}

#[test]
fn docs_follow_imports_through_barrels() {
    let messages = lint_project(
        "docs-barrel",
        &[
            ("a.ts", "/**\n * Old.\n * @deprecated use b\n */\nexport function a() {}\nexport function b() {}\n"),
            ("index.ts", "export * from \"./a\";\n"),
            ("main.ts", "import { a, b } from \"./index\";\na();\nb();\n"),
        ],
        "main.ts",
        "in ts_identifier rule d info \"{text}: {declaration_doc}\" { when imported_from != null && declaration_doc != null }",
    );
    assert_eq!(messages, vec!["a: Old.\n@deprecated use b", "a: Old.\n@deprecated use b"]);
}

#[test]
fn parent_reads_the_parent_layers_fields_and_attributes() {
    let l = native_linter(r#"
        in ts_call let arity count(argument)
        in ts_call.argument rule p info "{parent.callee}/{parent.arity}"
    "#);
    let messages: Vec<String> = l.lint(Path::new("/root/file.ts"), "f(1, 2);\n").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, vec!["f/2", "f/2"]);
}

#[test]
fn native_layers_have_no_pattern_children() {
    let src = "import .ts\nin ts_call match x /x/\nmatch ts_if /y/\nin x let text 1\n";
    let errors: Vec<String> = build(config::load_text("test.zlint", src, &Disk), PathBuf::from("/root"))
        .errors
        .into_iter()
        .map(|e| e.message)
        .collect();
    assert_eq!(errors.len(), 3, "{errors:#?}");
    assert!(errors[0].contains("already a matcher") && errors[1].contains("native layer") && errors[2].contains("built-in"), "{errors:#?}");
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
        macro bad no_such_name
        in .t match w /\w+/ {
          rule a warn "x" { when bad }
          rule b warn "x" { when bad }
        }
    "#);
    assert_eq!(errs.len(), 1, "{errs:?}");
}

#[test]
fn parent_indent_is_the_indentation_where_the_layer_opens() {
    let l = linter(r#"
        in .t, paren match word /\w+/ {
          rule w warn "[{parent_indent}|{parent_line}]"
        }
    "#);
    let messages: Vec<String> = lint(&l, "  z (\n\t\ta\n)").into_iter().map(|d| d.message).collect();
    // `z` sits in the file (no indentation); `a` in a paren opened on an
    // indented line, whatever its own indentation.
    assert_eq!(messages, vec!["[|]", "[  |  z (]"]);
}

#[test]
fn line_context_survives_nodes_ending_inside_crlf() {
    let l = linter(r#"
        in .t match upto_cr /x[^\n]*/ {
          rule ctx warn "[{after}]"
        }
    "#);
    assert_eq!(lint(&l, "x a\r\nb")[0].message, "[]");
}

#[test]
fn regex_functions_and_line_context() {
    let l = linter(r#"
        in .t match word /\b[a-z]+\b/ {
          rule ctx {
            when re_test(default(next_line, ''), "^end") || previous_line == null
            warn "{text}|{before}|{after}|{re_replace(text, '([aeiou])', '<$1>')}"
          }
        }
    "#);
    let diags = lint(&l, "one two\nmid\nend");
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(messages, vec!["one|| two|<o>n<e>", "two|one ||tw<o>", "mid|||m<i>d"]);
}

#[test]
fn values_side_by_side_join_text_and_lists() {
    let l = linter(r#"
        in .t match word /\b[a-z]+\b/ {
          rule j warn "{text len(text) null '!'}|{join([text] ['b'] 'c', '-')}|{len(text) + 1}"
        }
    "#);
    assert_eq!(lint(&l, "ab")[0].message, "ab2!|ab-b-c|3");
}

#[test]
fn patterns_that_could_match_empty_never_produce_empty_nodes() {
    let l = linter(r#"
        in .t {
          match xs /x*/
          match lazy /y*?z?/
        }
        in xs, lazy rule found warn "{text}"
    "#);
    let diags = lint(&l, "a xx b yyz y");
    let found: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    // Lazy repeats stop at the first non-empty match.
    assert_eq!(found, vec!["xx", "y", "yz", "y"]);
}

#[test]
fn longest_child_wins_and_ties_go_to_the_first() {
    let l = linter(r#"
        in .t {
          match short /ab/ { rule s warn "short" }
          match long /abc/ { rule l warn "long" }
          match tie /abc/ { rule t warn "tie" }
        }
    "#);
    let diags = lint(&l, "abc ab");
    let rules: Vec<&str> = diags.iter().map(|d| d.rule.as_str()).collect();
    assert_eq!(rules, vec!["l", "s"]);
}

#[test]
fn options_defaults_sets_and_messages() {
    let l = linter(r#"
        over .t match line /^[^\n]+/ {
          rule long-line warn "max {max}" {
            option max int(0..) = 10 "max chars"
            when len(text) > max
          }
        }
        rule long-line error { set max 5 }
    "#);
    let diags = lint(&l, "123456\n12345\n");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Error);
    assert_eq!(diags[0].message, "max 5");
}

#[test]
fn ancestors_and_builtins_are_visible_from_fields() {
    let l = linter(r#"
        in .t match fun /fn /name:/\w+/ :paren
        in fun.paren rule where warn "{fun.name}@{line}:{column} [{indent}]"
    "#);
    let diags = lint(&l, "x\n  fn go(a)");
    assert_eq!(diags[0].message, "go@2:8 [  ]");
}

#[test]
fn value_tests_match_whole_values() {
    let l = linter(r#"
        match snake /[a-z_][a-z0-9_]*/
        in .t match word /\w+/ {
          rule not-snake warn "x" { when !(text ~ snake) }
          rule has-digit warn "y" { when text ~ /.*\d.*/ }
        }
    "#);
    let diags = lint(&l, "ok Bad x1");
    let got: Vec<(&str, &str)> = diags.iter().map(|d| (d.rule.as_str(), &"ok Bad x1"[d.start..d.end])).collect();
    assert_eq!(got, vec![("not-snake", "Bad"), ("has-digit", "x1")]);
}

#[test]
fn macros_generate_rules() {
    let l = linter(r#"
        macro forbid(name, pat, why) {
          in .t match name pat {
            rule name error why
          }
        }
        forbid(dbg, "dbg!", "remove dbg! calls")
        forbid(todo_macro, "todo!", "finish this")
    "#);
    let diags = lint(&l, "dbg!(x); todo!()");
    let got: Vec<(&str, &str)> = diags.iter().map(|d| (d.rule.as_str(), d.message.as_str())).collect();
    assert_eq!(got, vec![("dbg", "remove dbg! calls"), ("todo_macro", "finish this")]);
    assert_eq!(diags[0].severity, Severity::Error);
}

#[test]
fn language_gates_keep_children_out_of_other_languages() {
    let l = linter(r#"
        lang u "*.u"
        in .u, paren match ustr /'/ until /'/
        in .t, .u match word /\bword\b/ {
          rule w warn "word"
        }
    "#);
    // In `.t`, `'` is not a string, so `word` is visible.
    assert_eq!(l.lint(Path::new("/root/a.t"), "'word'").len(), 1);
    assert_eq!(l.lint(Path::new("/root/a.u"), "'word'").len(), 0);
}

#[test]
fn ignore_comments_suppress() {
    let l = linter(r#"
        in .t match x /X/ {
          rule no-x warn "no x"
        }
    "#);
    let diags = lint(&l, "// zlint-ignore\nX\nX // zlint-ignore other\nX // zlint-ignore no-x\n");
    assert_eq!(diags.len(), 1);
}

#[test]
fn file_and_ignore_globs() {
    let l = linter(r#"
        ignore "vendor"
        in .t match x /X/ {
          rule no-x warn "no x" { ignore "tests/**" "other/**" }
        }
    "#);
    assert_eq!(l.lint(Path::new("/root/src/a.t"), "X").len(), 1);
    assert_eq!(l.lint(Path::new("/root/tests/a.t"), "X").len(), 0);
    assert_eq!(l.lint(Path::new("/root/other/a.t"), "X").len(), 0);
    assert_eq!(l.lint(Path::new("/root/vendor/a.t"), "X").len(), 0);
    assert_eq!(l.lint(Path::new("/root/src/a.txt"), "X").len(), 0);
}

#[test]
fn whole_file_targets() {
    let l = linter(r#"
        in .t rule header warn "files start with a header" {
          when !starts_with(text, "// header")
          fix as "// header\n" text
        }
    "#);
    let fixed = l.fix(Path::new("/root/file.t"), "body\n");
    assert_eq!(fixed.text, "// header\nbody\n");
}

/// The rules a config ends up with, and their messages on `text`.
pub(crate) fn rule_messages(l: &Linter, text: &str) -> Vec<String> {
    lint(l, text).into_iter().map(|d| format!("{}: {}", d.rule, d.message)).collect()
}

#[test]
fn override_replaces_definitions_in_place_and_forgets_their_settings() {
    let l = linter(r#"
        in .t match word /\b[a-z]+\b/
        in word rule shout warn "old {text}"
        rule shout error
        macro twice(x) x x
        override in word rule shout info "new {twice(text)}"
        override macro twice(x) x "+" x
        in word rule echo warn "{twice(text)}"
        override in .t match word /\b[a-z]{2}\b/
    "#);
    let diags = lint(&l, "ab abc");
    // One word now (the new pattern); the old rule's `set` went with it; the
    // replaced macro applies from the `override` on, not before.
    assert_eq!(rule_messages(&l, "ab abc"), vec!["echo: ab+ab", "shout: new abab"]);
    assert_eq!(diags[1].severity, Severity::Info);
}

#[test]
fn redefinitions_need_override() {
    let errs = errors(r#"
        in .t match word /\w+/
        in word rule r warn "x"
        in word rule r warn "y"
        macro m 1
        macro m 2
        override in word rule ghost warn "z"
    "#);
    let all = errs.join("\n");
    assert!(all.contains("rule `r` is defined twice") && all.contains("override in <layer> rule r"), "{all}");
    assert!(all.contains("macro `m` is defined twice") && all.contains("override macro m"), "{all}");
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
fn drop_detaches_layers_groups_attributes_and_ignores() {
    let l = linter(r#"
        group words word, num
        in .t, paren match word /[a-z]+/
        in .t match num /[0-9]+/
        in word, num let shown text "!"
        in words rule w warn "{shown}"
        drop in paren match word
        drop group words num
        drop in num let shown
        ignore "gen/**"
        drop ignore "gen/**"
    "#);
    // `num` left the group, `word` is no longer searched inside parens.
    assert_eq!(rule_messages(&l, "ab (cd) 12"), vec!["w: ab!"]);
    assert!(l.wants(Path::new("/root/gen/x.t")));
}

#[test]
fn defined_rules_stay_off_until_a_rule_line_turns_them_on() {
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
    assert_eq!(diags[3].message, "shy ab");
}

#[test]
fn configuring_works_before_the_definition_and_rulesets_skip_what_is_missing() {
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
fn library_layers_only_parse_while_something_needs_them() {
    let src = r#"
        define {
          in .t match lib_word /[a-z]+/ { rule lib warn "lib" }
          in .t match lib_other /[0-9]+/ { rule other warn "other" { when parent == "lib_word" } }
        }
        in .t match mine /[a-z]+/ { rule mine warn "mine" }
    "#;
    // Off, the library's layer does not take `ab` from `mine` (it would win
    // the tie: it comes first).
    assert_eq!(rule_messages(&linter(src), "ab"), vec!["mine: mine"]);
    // On, it does; and the off rule reading pruned layers is listed, not lost.
    let l = linter(&format!("{src}\nrule lib\n"));
    assert_eq!(rule_messages(&l, "ab"), vec!["lib: lib"]);
    let l = linter(src);
    assert!(l.inert.iter().any(|r| r.name == "lib") && l.inert.iter().any(|r| r.name == "other"), "{:?}", l.inert.iter().map(|r| &r.name).collect::<Vec<_>>());
}

#[test]
fn options_are_checked_against_their_types_and_records_get_defaults() {
    let l = linter(r#"
        schema word_choice {
          word: string = "*"  "which word"
          say: string
        }
        in .t match word /[a-z]+/ {
          rule pick warn "{find_last(config, c => c.word == text || c.word == '*').say}" {
            option config word_choice[] = [] "what to say about which word"
            when any(config, c => c.word == text || c.word == "*")
          }
        }
        rule pick {
          set config [
            { say: "any" },
            { word: "ab", say: "ab!" },
          ]
        }
    "#);
    assert_eq!(rule_messages(&l, "ab cd"), vec!["pick: ab!", "pick: any"]);
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
fn each_reports_once_per_item_where_report_at_says() {
    // A macro parameter named like a key is not substituted after `.`;
    // `\u{..}` in a string is an escape, not a hole.
    let l = linter(r#"
        macro of(word) filter(words, w => w.word == word)
        macro words [{ word: "ab", at: 1 }, { word: "cd", at: 0 }]
        in .t match line /[a-z ]+/ {
          rule pick warn "{w.word}{'\u{21}'}" {
            each w in of("ab") of("cd")
            report at offset + w.at
          }
        }
    "#);
    let diags = lint(&l, "xyz");
    let found: Vec<(usize, &str)> = diags.iter().map(|d| (d.start, d.message.as_str())).collect();
    assert_eq!(found, vec![(0, "cd!"), (1, "ab!")]);
}

#[test]
fn lambdas_map_filter_and_test_lists() {
    let l = linter(r#"
        in .t match word /[a-z]+/ {
          rule each info "{join(map(split(text, ''), c => upper(c)), '')}|{join(filter([1, 2, 3, 4], n => n % 2 == 0), ',')}|{all([1, 2], n => n > 0)}|{find([1, 2, 3], n => n > 5)}|{keys({ a: 1, b: 2 })}"
        }
    "#);
    assert_eq!(rule_messages(&l, "ab"), vec!["each: AB|2,4|true||a, b"]);
}

#[test]
fn facts_called_with_arguments_see_them_only_during_the_call() {
    use crate::expr::{facts, Value};
    let inner = facts::with_args(vec![Value::Num(1.0)], || {
        let nested = facts::with_args(vec![Value::Num(2.0)], || Value::list(facts::args()));
        Value::list(vec![nested, Value::list(facts::args())])
    });
    assert_eq!(inner, Value::list(vec![Value::list(vec![Value::Num(2.0)]), Value::list(vec![Value::Num(1.0)])]));
    assert!(facts::args().is_empty());
    // A fact reads the same whether named or called.
    let l = native_linter("in ts_identifier rule u warn \"{uses}/{uses()}\" { when uses != null }");
    let messages: Vec<String> = l.lint(Path::new("/root/a.ts"), "let a = 1; a;").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, ["1/1"]);
}

#[test]
fn tokens_are_layers_under_the_node_that_owns_them() {
    let l = native_linter(r#"
        in ts_comma rule c info "{parent}:{space_before.spaced}/{space_after.spaced}:{next_token}" fix space_after as " "
        in ts_keyword rule k info "{text}<{parent}"
    "#);
    let text = "f(a ,b, [1,,2]);\nreturn;";
    let diags = l.lint(Path::new("/root/a.ts"), text);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(messages, ["ts_call:true/false:ts_identifier", "ts_array:false/false:ts_comma", "ts_array:false/false:ts_number", "return<ts_return"]);
    // A comma followed by a space already is left alone by the fix.
    let (fixed, _) = super::apply_fixes(text, &diags);
    assert_eq!(fixed, "f(a , b, [1, , 2]);\nreturn;");
}

#[test]
fn rule_lets_read_on_other_nodes_carry_state_without_loops() {
    // A prefix maximum along siblings, from the rule's own option.
    let l = native_linter(r#"
        in ts_expression_statement rule order info "{rank}<{frontier}" {
          option order string[] = ["a", "b", "c"]
          let rank index_of(order, text)
          let frontier if previous.frontier == null then rank else max(previous.frontier, rank)
          when previous.frontier != null && rank < previous.frontier
        }
    "#);
    let messages: Vec<String> = l.lint(Path::new("/root/a.ts"), "a\nc\nb\nc\na").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, ["1<2", "0<2"]);
    // A `let` naming itself directly is an error: it reads on other nodes.
    let built = build(config::load_text("t.zlint", "import .ts\nin ts_identifier rule r { let x x + 1 }\n", &Disk), PathBuf::from("/root"));
    assert_eq!(built.errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["`x` refers to itself"]);
}

#[test]
fn load_time_constructs_start_with_an_at() {
    let l = native_linter(r#"
        @macro named(n) (text == n)
        in ts_identifier rule r info "{entry}" {
          option names string[] = ["a", "b"]
          @each entry in names
          when @named(entry)
        }
    "#);
    let messages: Vec<String> = l.lint(Path::new("/root/a.ts"), "a; b; c;").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, ["a", "b"]);
    // `@each` reads options only.
    let built = build(config::load_text("t.zlint", "import .ts\nin ts_identifier rule r { @each x in [text] }\n", &Disk), PathBuf::from("/root"));
    assert_eq!(built.errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["`text` is not a constant"]);
}

#[test]
fn links_lead_to_nodes_read_like_the_node_itself() {
    let l = native_linter(r#"
        in ts_identifier rule d info "{declaration.line}:{count(references)}:{declaration is ts_identifier}:{references.line}" {
          when declaration != null
        }
    "#);
    let messages: Vec<String> = l.lint(Path::new("/root/a.ts"), "let x = 1;\nx;\nx;").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, ["1:2:true:2, 3", "1:0:true:", "1:0:true:"]);
    let l = native_linter(r#"
        in ts_identifier rule d info "{count(declarations)}:{scope is ts_function}:{writer is ts_assignment}" { when text == "a" }
    "#);
    let messages: Vec<String> = l.lint(Path::new("/root/a.ts"), "function f() { var a; var a = 1; a = 2; }").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, ["2:true:false", "2:true:false", "2:true:true"]);
    let l = native_linter(r#"in ts_identifier rule n info "{identifier_name}""#);
    let messages: Vec<String> = l.lint(Path::new("/root/a.ts"), r"foo;").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, ["foo"]);
}

#[test]
fn own_brackets_and_variant_families() {
    // `opener.x` / `closer.x` read the node's own brackets; a family name
    // turns on every variant.
    let l = native_linter(r#"
        define {
          in ts_block rule pad:top info "{opener.space_after.blank_lines}" { when opener.line != closer.line }
          in ts_block rule pad:bottom info "{closer.space_before.blank_lines}" { when opener.line != closer.line }
        }
        rule pad
    "#);
    let messages: Vec<String> = l.lint(Path::new("/root/a.ts"), "if (a) {\n\n  b;\n}\nif (c) { d; }").into_iter().map(|d| d.message).collect();
    // pad:bottom, then pad:top (same span, by rule name).
    assert_eq!(messages, ["0", "1"]);
    // A class's first opener is its `<`: name the bracket wanted.
    let l = native_linter(r#"in ts_class rule c info "{opener.text}/{opener.ts_open_brace.text}/{closer.ts_close_brace.line}""#);
    let messages: Vec<String> = l.lint(Path::new("/root/a.ts"), "class A<T> {\n}").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, ["</{/2"]);
}

#[test]
fn builtins_read_on_a_fields_nodes() {
    let l = native_linter("in ts_if rule i info \"{alternate.layer}@{alternate.offset}|{[] consequent.layer}\"");
    let messages: Vec<String> = l.lint(Path::new("/root/a.ts"), "if (a) b; else if (c) d;").into_iter().map(|d| d.message).collect();
    assert_eq!(messages, ["ts_if@15|ts_expression_statement", "@|ts_expression_statement"]);
}

#[test]
fn config_functions_compute_from_their_arguments() {
    let l = linter(r#"
        fn twice(x) = x * 2
        fn fact(n) = if n <= 1 then 1 else n * fact(n - 1)
        fn shout(words) = join(map(words, w => upper(w) "!"), " ")
        in .t match word /[a-z]+/ {
          rule f info "{twice(len(text))}|{fact(4)}|{shout([text, 'x'])}"
        }
    "#);
    assert_eq!(rule_messages(&l, "ab"), vec!["f: 4|24|AB! X!"]);
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
fn reduce_folds_in_order_and_records_read_computed_keys() {
    // Each step sees the previous state: a running maximum of corrected
    // values, not of the originals.
    let l = linter(r#"
        in .t match word /[a-z]+/ {
          rule fold info "{join(reduce([3, 1, 4], [], (seen, n) => seen [n + count(seen)]), ',')}|{get({ ab: 1 }, text)}|{index_of(['x', text], text)}|{index_of([], 1)}"
        }
    "#);
    assert_eq!(rule_messages(&l, "ab"), vec!["fold: 3,2,6|1|1|-1"]);
    let l = linter(r#"in .t match word /[a-z0-9]+/ { rule c info "{compare_text('a10', text)}/{compare_text('a10', text, true)}/{compare_text('B', 'a')}/{compare_text('B', 'a', false, true)}" }"#);
    assert_eq!(rule_messages(&l, "a2"), vec!["c: -1/1/-1/1"]);
    let l = linter(r#"in .t match word /[a-z0-9]+/ { rule c info "{js_compare('10', '9')}/{js_compare(10, '9')}/{js_compare('', 0)}/{js_compare('x', 1)}/{js_compare({ type: 'bigint', value: '-1' }, -2)}" }"#);
    assert_eq!(rule_messages(&l, "a"), vec!["c: -1/1/0//1"]);
}

#[test]
fn configuring_adds_conditions_and_replaces_messages() {
    let l = linter(r#"
        in .t match word /[a-z]+/ { rule w warn "word" }
        rule w info "short {text}" { when len(text) < 3 }
    "#);
    let diags = lint(&l, "ab abcd");
    assert_eq!(rule_messages(&l, "ab abcd"), vec!["w: short ab"]);
    assert_eq!(diags[0].severity, Severity::Info);
}

#[test]
fn children_and_siblings_read_neighbouring_nodes() {
    let l = native_linter(r#"
        in ts_call rule kids info "{join([] children.layer, ',')}"
        in ts_call.argument rule around info "{default(previous, '-')}<{text}>{default(next, '-')}"
        in ts_expression_statement rule first info "first" { when previous == null && index == 0 }
    "#);
    let messages: Vec<String> = l.lint(Path::new("/root/file.ts"), "f(a, b);\n").into_iter().map(|d| d.message).collect();
    // Children in order, nulls kept (the callee is an identifier layer, the
    // arguments too); siblings only within the same field.
    assert_eq!(messages, vec!["ts_identifier,ts_identifier,ts_identifier", "first", "-<a>b", "a<b>-"]);
}

#[test]
fn is_tests_layer_names_against_layers_groups_and_specialisations() {
    let l = linter(r#"
        match word /[a-z]+/
        match shout like word /[A-Z]+/
        group things word, paren
        in .t, paren match word
        in .t, paren match shout
        in word rule w info "{layer is word}/{layer is (paren, shout)}/{layer not (paren, shout)}/{parent is things}"
    "#);
    assert_eq!(rule_messages(&l, "ab (CD)"), vec!["w: true/false/true/false", "w: true/true/false/true"]);
}

#[test]
fn forward_reads_attributes_through_wrappers() {
    let l = native_linter(r#"
        in ts_identifier let size len(text)
        in ts_identifier let kind_of "id"
        in ts_parenthesized, ts_non_null forward size from expression
        in ts_expression_statement rule s info "{expression.size}/{default(expression.kind_of, '-')}"
    "#);
    let messages: Vec<String> = l.lint(Path::new("/root/file.ts"), "((abc))!;\n").into_iter().map(|d| d.message).collect();
    // Only the attributes named pass through.
    assert_eq!(messages, vec!["3/-"]);
}

#[test]
fn rule_lets_name_values_and_long_conditions_span_lines() {
    let l = linter(r#"
        in .t match word /[a-z]+/ {
          rule long {
            let size len(text)
            let big size > 3
            when big
              && starts_with(text, "a")
              || text == "zz"
            warn "{size} letters"
          }
        }
    "#);
    assert_eq!(rule_messages(&l, "abcd abc zz bbbbb"), vec!["long: 4 letters", "long: 2 letters"]);
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
fn runtime_failures_become_visible_diagnostics() {
    let l = linter(r#"
        in .t match x /X+/ {
          rule boom warn "x" { when num(text) > 1 }
        }
    "#);
    let diags = lint(&l, "XX");
    assert_eq!(diags[0].severity, Severity::Error);
    assert!(diags[0].message.contains("failed"), "{}", diags[0].message);
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
fn std_library_parses_and_lints() {
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
    let l = built.linter.unwrap();
    let text = "fn main() {\n    let s = \"dbg!(x)\"; // dbg!\n    let c = '\"'; dbg!(s);\n}\n";
    let diags = l.lint(Path::new("/root/src/main.rs"), text);
    let rules: Vec<&str> = diags.iter().map(|d| d.rule.as_str()).collect();
    // Only the config's own rule: every built-in one is off.
    assert_eq!(rules, vec!["no-dbg"], "{diags:#?}");

    // Every built-in rule and ruleset compiles when turned on.
    let all = format!("{src}\nimport rules:correctness\nimport rules:suspicious\nimport rules:conventions\nimport rules:format\nimport rules:types\nimport rules:recommended\nrule *\n");
    let built = build(config::load_text("test.zlint", &all, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    let l = built.linter.unwrap();
    assert!(l.inert.is_empty() && l.rules.iter().all(|r| r.severity != Severity::Off));
}
