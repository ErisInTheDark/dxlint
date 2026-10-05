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
fn mid_line_anchors_stop_at_their_own_line() {
    // The consequent is anchored at the conditional, a line above the
    // arrow's `{`: the `{` stands for its own (`?`) line.
    let src = "x = cond\n  ? () => {\n      return 1;\n    }\n  : 2;\n";
    assert_eq!(levels("indent in ts_block\nindent ts_conditional.consequent by 2", src), ["0:0", "0:0", "1:2", "0:2", "0:0"]);
    // Anchored at its `?` instead, in its own class, it sits a level past it.
    let rules = "indent in ts_block\nindent ts_conditional.consequent from token_neighbours.before.start class branches";
    assert_eq!(levels(rules, src), ["0:0", "0:0", "2:2", "1:2", "0:0"]);
}
