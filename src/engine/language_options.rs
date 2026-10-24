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

pub(crate) struct LanguageOptions {
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

    pub fn environment(&self, language: usize, path: &Path) -> Value<'static> {
        Value::record(vec![("settings".into(), Value::list(self.values(language, path)
            .map(|(name, value)| Value::record(vec![
                ("name".into(), Value::from(name.to_owned())),
                ("value".into(), value.clone()),
            ])).collect()))])
    }
}

#[cfg(test)]
mod tests {
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

}
