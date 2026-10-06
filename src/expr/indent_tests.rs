//! Anchors in the middle of a line: levels opened on one line collapse to
//! one, unless their indent lines are in different classes.

use std::path::Path;

use crate::engine::tests::native_linter;

fn levels(rules: &str, src: &str) -> Vec<String> {
    let l = native_linter(&format!(r#"in ts_line rule ind info "{{indent_level}}:{{len(indent_anchor)}}" {{ {rules} }}"#));
    l.lint(Path::new("/root/a.ts"), src).into_iter().map(|d| d.message).collect()
}

#[test]
fn levels_opened_on_one_line_collapse_within_a_class() {
    // `foo(bar, {` opens the call and the object on one line: one level.
    let src = "foo(bar, {\n  a\n})\n";
    assert_eq!(levels("indent in ts_object\nindent in ts_call", src), ["0:0", "1:0", "0:0"]);
    // In another class the call's level adds up.
    assert_eq!(levels("indent in ts_object\nindent in ts_call class calls", src), ["0:0", "2:0", "1:0"]);
}

#[test]
fn classes_give_mid_line_anchors_their_own_levels() {
    // `{` shares `var`'s line but sits a level in, as the declarators do.
    let src = "var foo = {\n    value: 1\n  },\n  bar = 2;\n";
    assert_eq!(levels("indent in ts_object\nindent in ts_variable_declaration class declarators", src), ["0:0", "2:0", "1:0", "1:0"]);
    assert_eq!(levels("indent in ts_object\nindent in ts_variable_declaration", src), ["0:0", "1:0", "0:0", "1:0"]);
}

#[test]
fn text_anchors_measure_from_the_text_itself() {
    // `from ""`: column 0, whatever the lines above hold.
    let src = "  {\n      x;\n  }\n";
    let rules = "indent in ts_block from \"\" by 1";
    assert_eq!(levels(rules, src), ["0:0", "1:0", "0:0"]);
    let rules = "indent in ts_block from \"\\t\"";
    assert_eq!(levels(rules, src), ["0:0", "1:1", "0:1"]);
}

#[test]
fn mid_line_anchors_sit_at_their_deciding_text_anchor() {
    // The call's `(` is in an expression measured from a text (three
    // spaces), so its arguments are a level past that, not past the
    // `    x = foo(` line.
    let src = "    x = foo(\n      a\n    );\n";
    let rules = "indent in ts_call\nindent ts_assignment.right from \"   \" by 0";
    assert_eq!(levels(rules, src), ["0:0", "1:3", "0:3"]);
}

#[test]
fn mid_line_anchors_stop_at_their_own_line() {
    // The consequent is anchored at the conditional, a line above the
    // arrow's `{`: the `{` stands for its own (`?`) line.
    let src = "x = cond\n  ? () => {\n      return 1;\n    }\n  : 2;\n";
    assert_eq!(levels("indent in ts_block\nindent ts_conditional.consequent by 2", src), ["0:0", "0:0", "1:2", "0:2", "0:0"]);
    // Anchored at its `?` instead, in its own class, it sits a level past it.
    let rules = "indent in ts_block\nindent ts_conditional.consequent from token_neighbours.before.start class branches";
    assert_eq!(levels(rules, src), ["0:0", "0:0", "2:2", "1:2", "0:0"]);
}

#[test]
fn lines_starting_inside_a_token_follow_their_anchor_up() {
    // The `{` shares a line with template text, which has no code
    // indentation: it sits where the call's tag line puts it.
    let src = "tag`raw\n        text`(() => {\n  value();\n});\n";
    let found = levels("indent in ts_call from 0\nindent in ts_block", src);
    assert_eq!(found[2..], ["1:0", "0:0"]);
    // Code lines still stand for themselves.
    let src = "tag(\n        x, () => {\n  value();\n});\n";
    let found = levels("indent in ts_call from 0\nindent in ts_block", src);
    assert_eq!(found[2..], ["1:8", "0:8"]);
}

#[test]
fn reset_uses_the_winning_container_and_keeps_nested_blocks_checked() {
    let src = "class Foo {\nfoo() {\nbar();\n}\n}\n";
    let rules = "indent reset is ts_class\nindent in ts_class\nindent in ts_method.body";
    assert_eq!(levels(rules, src), ["0:0", "0:0", "1:0", "0:0", "0:0"]);
}

#[test]
fn reset_preserves_node_form_indentation_including_alignment_reads() {
    let src = "[\n    foo(),\n    bar\n]\n";
    let rules = "indent reset is ts_call\nindent in ts_array\nindent ts_array.element from offset by 0";
    assert_eq!(levels(rules, src), ["0:0", "0:4", "1:0", "0:0"]);
}

#[test]
fn reset_applies_to_mid_line_virtual_anchors_before_class_levels() {
    let src = "    void (x = foo(\n      a\n    ));\n";
    let rules = "indent reset is ts_assignment\nindent in ts_call\nindent ts_assignment from \"\" class assignments by 3";
    assert_eq!(levels(rules, src), ["0:0", "1:4", "0:4"]);
}

#[test]
fn false_reset_preserves_normal_candidate_priority() {
    let src = "foo({\n  a\n})\n";
    let rules = "indent reset false\nindent in ts_object\nindent in ts_call class calls";
    assert_eq!(levels(rules, src), ["0:0", "2:0", "1:0"]);
}

#[test]
fn indentation_expression_errors_reach_the_rule() {
    let failures: Vec<_> = [
        "indent in ts_call from []",
        "indent in ts_call by []",
        "indent in ts_call\nindent ts_assignment.right from []",
        "indent in ts_call\nindent ts_assignment.right from \"\" class assignments by []",
    ].into_iter().filter(|rules| {
        let l = native_linter(&format!("in ts_line rule ind info \"{{indent_level}}:{{indent_anchor}}\" {{ {rules} }}"));
        let got = l.lint(Path::new("/root/a.ts"), "x = foo(\n  a\n);\n");
        !got.iter().any(|d| d.severity == crate::config::ast::Severity::Error)
    }).collect();
    assert!(failures.is_empty(), "errors swallowed by: {failures:?}");
}

#[test]
fn reset_predicate_errors_reach_the_rule() {
    for rules in [
        "indent reset js_re_test('', '[')\nindent in ts_call",
        "indent reset if is ts_assignment then js_re_test('', '[') else false\nindent in ts_call\nindent ts_assignment from \"\" class assignments",
    ] {
        let l = native_linter(&format!("in ts_line rule ind info \"{{indent_level}}:{{indent_anchor}}\" {{ {rules} }}"));
        let got = l.lint(Path::new("/root/a.ts"), "void (x = foo(\n  a\n));\n");
        assert!(got.iter().any(|d| d.severity == crate::config::ast::Severity::Error), "{rules}: {got:?}");
    }
}

#[test]
fn std_list_indentation_uses_ordinals_for_indexed_arguments() {
    let l = native_linter("rule indent");
    let got = l.lint(Path::new("/root/a.ts"), "foo(\n    object[key],\n    other[another]\n);\n");
    assert!(got.is_empty(), "{got:?}");
}
