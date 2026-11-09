//! Compiled language schemas and immutable, file-scoped settings.

use std::path::Path;

use crate::config::ast::{Config, Expr, ExprKind, Name, OptionDef};
use crate::config::error::ConfigError;
use crate::config::schema::{Schema, Schemas};
use crate::expr::{Ctx, Scope, Value};
use crate::grammar::Grammar;
use crate::names::Names;
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

pub struct LanguageOptions {
    languages: Vec<Language>,
}

impl LanguageOptions {
    pub fn compile(config: &Config, grammar: &Grammar, names: &Names, schemas: &Schemas, queries: &'static [dx_ir::expr::QuerySpec], errors: &mut Vec<ConfigError>) -> Self {
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
                if let Some(value) = option_value(&owner, name, expression, &option.schema, grammar, names, schemas, queries, errors) {
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

/// A constant option value, checked against its schema; `set` lines and
/// language settings both use it.
pub fn option_value(
    rule: &str,
    name: &Name,
    value: &Expr,
    schema: &Schema,
    grammar: &Grammar,
    names: &Names,
    schemas: &Schemas,
    queries: &'static [dx_ir::expr::QuerySpec],
    errors: &mut Vec<ConfigError>,
) -> Option<Value<'static>> {
    // A bare word where enum members go is one, checked like a string:
    // `set mode never`.
    let lookup = |name: &str| schemas.get(name).map(|d| &d.schema);
    let word = match &value.kind {
        ExprKind::Var(path) => match path.as_slice() {
            [word] if schema.has_enum(&lookup) => Some(Value::from(word.text.clone())),
            _ => None,
        },
        _ => None,
    };
    let evaluated = match word {
        Some(word) => Ok(word),
        None => {
            let compiled = Scope::constants(grammar).with_names(names).with_queries(queries).compile(value, errors)?;
            Ctx::eval_static(&compiled, &[]).map(|v| v.into_owned())
        }
    };
    match evaluated {
        Ok(v) => match schema.check_with(&v, schemas, &|check, value| Ctx::accepts(grammar, check, value)) {
            Ok(v) => Some(v),
            Err(misfit) => {
                errors.push(
                    ConfigError::new(value.span, format!("option `{}` of `{rule}`: {misfit}", name.text))
                        .note(format!("`{}` takes {schema}", name.text)),
                );
                None
            }
        },
        Err(err) => {
            errors.push(ConfigError::new(value.span, format!("option values must be constants: {err}")));
            None
        }
    }
}
