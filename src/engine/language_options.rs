//! Compiled language schemas and immutable, file-scoped settings.

use std::path::Path;

use crate::config::ast::{Config, OptionDef};
use crate::config::error::ConfigError;
use crate::config::schema::Schemas;
use crate::expr::Value;
use crate::grammar::Grammar;
use crate::path_globs::PathGlobs;

#[derive(Default)]
struct Language {
    defaults: Vec<(String, Value<'static>)>,
    patches: Vec<Patch>,
}

struct Patch {
    files: PathGlobs,
    ignore: PathGlobs,
    values: Vec<(String, Value<'static>)>,
}

pub(super) struct LanguageOptions {
    languages: Vec<Language>,
}

impl LanguageOptions {
    pub fn compile(config: &Config, grammar: &Grammar, schemas: &Schemas, errors: &mut Vec<ConfigError>) -> Self {
        let mut languages: Vec<Language> = grammar.langs.iter().map(|_| Language::default()).collect();
        let mut declarations: Vec<Vec<&OptionDef>> = grammar.langs.iter().map(|_| Vec::new()).collect();
        for block in &config.language_options {
            let Some(language) = grammar.lang(&block.language.text) else {
                errors.push(ConfigError::new(block.language.span, format!("unknown language `.{}`", block.language.text)));
                continue;
            };
            for option in &block.options {
                if let Some(prior) = declarations[language].iter().find(|prior| prior.name.text == option.name.text) {
                    errors.push(ConfigError::new(option.name.span, format!("language option `{}` is already declared", option.name.text))
                        .related(prior.name.span, "first declared here"));
                    continue;
                }
                match option.schema.check(&option.default, schemas) {
                    Ok(value) => languages[language].defaults.push((option.name.text.clone(), value)),
                    Err(error) => errors.push(ConfigError::new(option.default_span, format!("language option `{}` default: {error}", option.name.text))),
                }
                declarations[language].push(option);
            }
        }
        for block in &config.language_options {
            let Some(language) = grammar.lang(&block.language.text) else { continue };
            let globs = |values: &[(String, crate::config::source::Span)], errors: &mut Vec<ConfigError>| {
                PathGlobs::new(values.iter().map(|(value, _)| value.as_str())).map_err(|(glob, message)| {
                    let span = values.iter().find(|(value, _)| *value == glob).map(|(_, span)| *span).unwrap_or(block.span);
                    errors.push(ConfigError::new(span, message));
                }).ok()
            };
            let Some(files) = globs(&block.files, errors) else { continue };
            let Some(ignore) = globs(&block.ignore, errors) else { continue };
            let mut values = Vec::new();
            for (name, expression) in &block.settings {
                let Some(option) = declarations[language].iter().find(|option| option.name.text == name.text) else {
                    errors.push(ConfigError::new(name.span, format!("unknown option `{}` of language `.{}`", name.text, block.language.text)));
                    continue;
                };
                let owner = format!(".{}", block.language.text);
                if let Some(value) = super::build::option_value(&owner, name, expression, &option.schema, grammar, schemas, errors) {
                    values.push((name.text.clone(), value));
                }
            }
            if !values.is_empty() {
                languages[language].patches.push(Patch { files, ignore, values });
            }
        }
        Self { languages }
    }

    pub fn values<'a>(&'a self, language: usize, path: &'a Path) -> impl Iterator<Item = (&'a str, &'a Value<'static>)> {
        let language = &self.languages[language];
        language.defaults.iter().chain(language.patches.iter()
            .filter(move |patch| (patch.files.is_empty() || patch.files.matches(path)) && !patch.ignore.matches(path))
            .flat_map(|patch| patch.values.iter()))
            .map(|(name, value)| (name.as_str(), value))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::config::{load_text, source::Disk};
    use crate::engine::{build, Linter};

    fn configured(settings: &str) -> Linter {
        with_rules(settings, "rule no-global-assign")
    }

    fn with_rules(settings: &str, rules: &str) -> Linter {
        let source = format!("import .ts\n{settings}\n{rules}\n");
        let built = build(load_text("test.zlint", &source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
        built.linter.expect("compiled")
    }

    #[test]
    fn scoped_globals_merge_and_later_matching_values_override() {
        let linter = configured(r#"
language .js { set globals { app: "readonly", other: "readonly" } }
language .js {
  files "scripts/**"
  ignore "scripts/generated/**"
  set globals { app: "writable" }
}
language .js { files "scripts/private/**"
  set globals { app: "off" }
}
"#);
        let count = |file| linter.lint(Path::new(file), "app = 1; other = 2;").len();
        assert_eq!(count("/root/main.js"), 2);
        assert_eq!(count("/root/scripts/main.js"), 1);
        assert_eq!(count("/root/scripts/generated/main.js"), 2);
        assert_eq!(count("/root/scripts/private/main.js"), 1);
        assert_eq!(count("/root/main.js"), 2);
    }

    #[test]
    fn real_global_comments_override_configuration_without_affecting_other_files() {
        let linter = configured(r#"language .js { set globals { app: "readonly" } }"#);
        assert!(linter.lint(Path::new("/root/a.js"), "/* global app: writable */\napp = 1;").is_empty());
        assert_eq!(linter.lint(Path::new("/root/b.js"), "app = 1;").len(), 1);
    }

    #[test]
    fn implied_strictness_is_an_input_not_a_fabricated_directive() {
        let linter = with_rules(r#"language .js { set sourceType "script"
          set impliedStrict true
        }"#, "in ts_function rule probe { when scoping_strict_code\nwarn \"strict\" }");
        assert_eq!(linter.lint(Path::new("/root/a.js"), "function f() {}").len(), 1);
        let plain = with_rules(r#"language .js { set sourceType "script" }"#,
            "in ts_function rule probe { when scoping_strict_code\nwarn \"strict\" }");
        assert!(plain.lint(Path::new("/root/a.mjs"), "function f() {}").is_empty());
    }

    #[test]
    fn script_return_wrappers_do_not_introduce_commonjs_globals() {
        let rules = "rule no-implicit-globals";
        let wrapped = with_rules(r#"language .js { set sourceType "script"
          set globalReturn true
        }"#, rules);
        assert!(wrapped.lint(Path::new("/root/a.js"), "var local = 1;").is_empty());
        let wrapped_globals = with_rules(r#"language .js { set sourceType "script"
          set globalReturn true
        }"#, "rule no-global-assign");
        assert!(wrapped_globals.lint(Path::new("/root/a.js"), "require = 2;").is_empty());
        let commonjs = with_rules(r#"language .js { set sourceType "commonjs" }"#, "rule no-global-assign");
        let diagnostics = commonjs.lint(Path::new("/root/a.js"), "var local = 1; require = 2;");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].rule, "no-global-assign");
    }

    #[test]
    fn all_string_directives_share_one_prefix_with_exact_strict_spelling() {
        let linter = with_rules("", "rule strict { set mode \"never\" }");
        assert_eq!(linter.lint(Path::new("/root/a.js"), "'other'; 'use strict';").len(), 1);
        for source in ["('use strict');", "'use\\x20strict';", "run(); 'use strict';"] {
            assert!(linter.lint(Path::new("/root/a.js"), source).is_empty(), "{source}");
        }
    }

    #[test]
    fn directive_exemptions_follow_edition_and_ignore_directives_policy() {
        for (edition, ignore, expected) in [(3, false, 1), (3, true, 0), (5, false, 0)] {
            let settings = format!("language .js {{ set ecmaVersion {edition} }}");
            let rules = format!("rule no-unused-expressions {{ set ignoreDirectives {ignore} }}");
            let linter = with_rules(&settings, &rules);
            assert_eq!(linter.lint(Path::new("/root/a.js"), "'use strict';").len(), expected);
        }
    }

    #[test]
    fn strict_non_simple_parameters_are_diagnosed_in_every_policy() {
        let settings = r#"language .js { set sourceType "script"
          set ecmaVersion 6
        }"#;
        let source = "function foo(a = 0) { 'use strict'; }";
        for mode in ["never", "global", "function", "safe"] {
            let linter = with_rules(settings, &format!("rule strict {{ set mode \"{mode}\" }}"));
            let diagnostics = linter.lint(Path::new("/root/a.js"), source);
            assert!(diagnostics.iter().any(|diagnostic| diagnostic.start == source.find("'use strict'").unwrap()
                && diagnostic.message.contains("non-simple parameter list")), "{mode}: {diagnostics:#?}");
        }
    }

    #[test]
    fn strict_function_policy_does_not_inherit_an_invalid_program_directive() {
        let functions = with_rules(r#"language .js { set sourceType "script" }"#,
            "rule strict { set mode \"function\" }");
        assert_eq!(functions.lint(Path::new("/root/a.js"),
            "'use strict'; (function() { 'use strict'; return true; }());").len(), 1);
    }

    #[test]
    fn strict_function_inheritance_precedes_class_context() {
        let functions = with_rules(r#"language .js { set sourceType "script" }"#,
            "rule strict { set mode \"function\" }");
        let source = "function foo() { 'use strict'; class C { static { function nested() { 'use strict'; } } } }";
        let diagnostics = functions.lint(Path::new("/root/a.js"), source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "Unnecessary 'use strict' directive.");
    }

    #[test]
    fn eval_this_distinguishes_plain_scripts_from_strict_script_wrappers() {
        for (wrapped, strict, expected) in [(false, true, 2), (true, false, 2), (true, true, 0)] {
            let settings = format!("language .js {{ set sourceType \"script\"\nset globalReturn {wrapped}\n}}");
            let linter = with_rules(&settings, "rule no-eval");
            let source = format!("{}this.eval('one'); (() => this.eval('two'))();",
                if strict { "'use strict'; " } else { "" });
            assert_eq!(linter.lint(Path::new("/root/a.js"), &source).len(), expected, "{wrapped} {strict}");
        }
        let module = with_rules(r#"language .js { set sourceType "module" }"#, "rule no-eval");
        assert!(module.lint(Path::new("/root/a.js"), "this.eval('one');").is_empty());
    }

    #[test]
    fn inner_function_allowance_requires_modern_strict_block_scoping() {
        for (edition, strict, policy, expected) in [
            (5, true, "allow", 2), (2015, true, "allow", 1),
            (2015, false, "allow", 2), (2015, true, "disallow", 2),
        ] {
            let settings = format!("language .js {{ set sourceType \"script\"\nset ecmaVersion {edition}\n}}");
            let rules = format!("rule no-inner-declarations {{ set mode \"both\"\nset blockScopedFunctions \"{policy}\"\n}}");
            let linter = with_rules(&settings, &rules);
            let source = format!("{}if (test) {{ function nested() {{}} var local; }}",
                if strict { "'use strict'; " } else { "" });
            assert_eq!(linter.lint(Path::new("/root/a.js"), &source).len(), expected, "{edition} {strict} {policy}");
        }
    }

    #[test]
    fn rule_workers_receive_the_same_file_environment() {
        let mut linter = with_rules(r#"language .js {
          set sourceType "commonjs"
          set globals { app: "readonly" }
        }"#, "rule no-global-assign\nin ts_program rule wrapper { when source_environment.root_function\nwarn \"wrapped\" }");
        let source = "app = 1;";
        linter.set_rule_jobs(1);
        let serial = linter.lint(Path::new("/root/a.js"), source);
        assert_eq!(serial.len(), 2);
        linter.split_at = 1;
        linter.set_rule_jobs(2);
        let parallel = linter.lint(Path::new("/root/a.js"), source);
        let reports = |diagnostics: Vec<super::super::Diagnostic>| diagnostics.into_iter()
            .map(|diagnostic| (diagnostic.rule, diagnostic.start, diagnostic.message)).collect::<Vec<_>>();
        assert_eq!(reports(serial), reports(parallel));
    }

    #[test]
    fn configured_globals_have_no_source_comment_provenance() {
        let globals = r#"language .js { set globals { app: "readonly" } }"#;
        let redeclare = with_rules(globals, "rule no-redeclare");
        let source = "var app;";
        let diagnostics = redeclare.lint(Path::new("/root/a.js"), source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].start, source.find("app").expect("declaration"));
        let disabled = with_rules(r#"language .js { set globals { app: "off" } }"#, "rule no-redeclare");
        assert!(disabled.lint(Path::new("/root/a.js"), source).is_empty());
        let unused = with_rules(globals, "rule no-unused-vars");
        assert!(unused.lint(Path::new("/root/a.js"), "run();").is_empty());
        assert_eq!(unused.lint(Path::new("/root/a.js"), "/* global app */\nrun();").len(), 1);
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
"#, "rule no-unused-expressions");
        assert_eq!(linter.lint(Path::new("/root/legacy/a.js"), "'use strict';").len(), 1);
        assert!(linter.lint(Path::new("/root/a.js"), "'use strict';").is_empty());
        let json = linter.grammar.lang("json").expect("JSON language");
        let edition = linter.language_options.values(json, Path::new("legacy/a.json"))
            .filter(|(name, _)| *name == "ecmaVersion").last().map(|(_, value)| value);
        assert_eq!(edition, Some(&crate::expr::Value::Bool(false)));
    }

}
