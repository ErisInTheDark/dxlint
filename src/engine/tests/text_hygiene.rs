use std::path::{Path, PathBuf};

use super::{build, config, Disk, Linter};

fn text_linter(rules: &str) -> Linter {
    let source = format!("import .ts\nimport .css\nimport .json\nimport .markdown\n{rules}");
    let built = build(config::load_text("test.zlint", &source, &Disk), PathBuf::from("/root"));
    assert!(built.errors.is_empty(), "{:#?}", built.errors);
    built.linter.expect("no errors")
}

#[test]
fn standalone_text_languages_build_without_list_adapters() {
    for language in ["css", "markdown"] {
        let source = format!("import .{language}\nrule *\n");
        let built = super::super::build::build_for_lint(config::load_text("test.zlint", &source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
    }
}

#[test]
fn line_hygiene_runs_on_css_json_and_markdown_without_typescript_parsing() {
    for (file, source) in [
        ("a.css", "a { color: red; }  \r\n"),
        ("a.jsonc", "{ \"key\": \"value\" }  \r\n"),
        ("a.md", "Some text  \r\n"),
    ] {
        let lint = text_linter("rule no-trailing-spaces\nrule linebreak-style");
        let diagnostics = lint.lint(Path::new(file), source);
        assert_eq!(diagnostics.len(), 2, "{file}: {diagnostics:#?}");
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.rule != "zlint"));
    }
    let lint = text_linter("rule eol-last");
    assert_eq!(lint.lint(Path::new("a.css"), "a {}").len(), 1);
    assert!(lint.lint(Path::new("a.css"), "").is_empty());
}

#[test]
fn trailing_space_fixes_keep_blank_line_and_template_exemptions() {
    let lint = text_linter("rule no-trailing-spaces {\n set skipBlankLines true\n set ignoreComments true\n}");
    let source = "const value = `raw  \n${\n  1  \n}`;\n/* comment  \n */\n  \n";
    let diagnostics = lint.lint(Path::new("a.ts"), source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let (fixed, _) = crate::engine::apply_fixes(source, &diagnostics);
    assert!(fixed.contains("raw  \n"));
    assert!(fixed.contains("comment  \n"));
    assert!(fixed.contains("  1\n"));
    assert!(fixed.ends_with("  \n"));
}

#[test]
fn json_list_layout_ignores_nested_commas_and_honours_node_overrides() {
    let lint = text_linter("rule list-style");
    for source in [
        "{\n  \"foo\": [\"bar\", \"baz\"]\n}",
        "{\n  \"foo\": [\n    \"bar\",\n    \"baz\"\n  ]\n}",
        "{ \"foo\": [{ \"text\": \"],comma,\" }, [1, 2]] }",
    ] {
        assert!(lint.lint(Path::new("a.jsonc"), source).is_empty(), "{source}");
    }
    let source = "{\n  \"foo\": [\"bar\",\n  \"baz\"],\n}";
    assert_eq!(lint.lint(Path::new("a.jsonc"), source).len(), 1);
    let override_lint = text_linter(r#"rule list-style { set overrides { JSONObjectExpression: { singleLine: { spacing: "never" } } } }"#);
    assert!(override_lint.lint(Path::new("a.jsonc"), "{\"foo\": [1, 2]}").is_empty());
    assert_eq!(lint.lint(Path::new("a.jsonc"), "{\"foo\": [1, 2]}").len(), 2);
}

#[test]
fn list_node_overrides_keep_unrelated_bracket_defaults() {
    for (file, source, kind) in [
        ("a.jsonc", r#"{ "array": [ ], "object": {} }"#, "JSONObjectExpression"),
        ("a.ts", r#"const value = { "array": [ ], "object": {} };"#, "ObjectExpression"),
    ] {
        let lint = text_linter(&format!(r#"rule list-style {{ set empty "never"
          set overrides {{ {kind}: {{ empty: "always" }} }}
        }}"#));
        let diagnostics = lint.lint(Path::new(file), source);
        assert_eq!(diagnostics.len(), 2, "{file}: {diagnostics:#?}");
    }
    let lint = text_linter(r#"rule list-style { set overrides { "{}": "off" } }"#);
    assert!(lint.lint(Path::new("a.jsonc"), r#"{"array":[1, 2]}"#).is_empty());
    let lint = text_linter(r#"rule list-style { set overrides { "{}": { singleLine: { maxItems: 2 } } } }"#);
    assert!(lint.lint(Path::new("a.jsonc"), r#"{ "value": 1 }"#).is_empty());
    let lint = text_linter(r#"rule list-style { set overrides { "{}": { singleLine: { spacing: "never" } } } }"#);
    assert!(lint.lint(Path::new("a.jsonc"), r#"{"value": 1}"#).is_empty());
}
