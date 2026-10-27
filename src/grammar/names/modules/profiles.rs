use crate::config::ast::{Config, Expr, ExprKind};
use crate::config::ast::modules::{Setting, SettingValue};
use crate::config::error::ConfigError;
use crate::config::source::Span;
use crate::expr::{Ctx, Scope, Value};
use crate::grammar::Grammar;
use super::compile::{Mode, Profile};
use super::resolution::{Search, Settings};
use super::super::Vocabulary;

pub fn compile(config: &Config, grammar: &Grammar, vocabulary: &Vocabulary, errors: &mut Vec<ConfigError>, mode: &mut dyn FnMut(usize, usize) -> u32) -> Vec<Profile> {
    let mut profiles = Vec::new();
    let mut names = std::collections::HashMap::new();
    for (definition, source) in config.module_resolutions.iter().enumerate() {
        if let Some(previous) = names.insert(source.name.text.clone(), source.name.span) {
            errors.push(ConfigError::new(source.name.span, "module resolution profile is already defined").related(previous, "first defined here"));
            continue;
        }
        let before = errors.len();
        let mut settings = Settings::default();
        let mut compiler = Compiler { grammar, vocabulary, errors };
        compiler.apply(&source.settings, ROOT, &mut settings);
        let mut modes = Vec::new();
        for (index, source) in source.modes.iter().enumerate() {
            let mut settings = settings.clone();
            compiler.apply(&source.settings, ROOT, &mut settings);
            modes.push(Mode { when: mode(definition, index), settings });
        }
        if errors.len() == before { profiles.push(Profile { name: source.name.text.clone().into(), settings, modes }); }
    }
    profiles
}

enum Field {
    Text(fn(&mut Settings) -> &mut Box<str>, Option<&'static [&'static str]>),
    Projection(fn(&mut Settings) -> &mut Box<str>),
    Strings(fn(&mut Settings) -> &mut Vec<Box<str>>, Option<&'static [&'static str]>),
    Bool(fn(&mut Settings) -> &mut bool),
    Block(&'static [Control]),
    Search,
    Scopes,
    Substitutions,
    Format,
}

struct Control { name: &'static str, field: Field }

const ROOT: &[Control] = &[
    Control { name: "config", field: Field::Block(CONFIG) },
    Control { name: "packages", field: Field::Block(PACKAGES) },
    Control { name: "globals", field: Field::Block(GLOBALS) },
    Control { name: "extensions", field: Field::Strings(|s| &mut s.extensions, None) },
    Control { name: "candidates", field: Field::Strings(|s| &mut s.candidates, Some(&["dir", "name", "ext"])) },
    Control { name: "conditions", field: Field::Strings(|s| &mut s.conditions, None) },
    Control { name: "star_excludes", field: Field::Strings(|s| &mut s.star_excludes, None) },
    Control { name: "ambient_scopes", field: Field::Scopes },
    Control { name: "search", field: Field::Search },
    Control { name: "substitutions", field: Field::Substitutions },
];

const CONFIG: &[Control] = &[
    Control { name: "files", field: Field::Strings(|s| &mut s.config.files, None) },
    Control { name: "source_files", field: Field::Text(|s| &mut s.config.source_files, None) },
    Control { name: "include", field: Field::Text(|s| &mut s.config.include, None) },
    Control { name: "exclude", field: Field::Text(|s| &mut s.config.exclude, None) },
    Control { name: "extends", field: Field::Text(|s| &mut s.config.extends, None) },
    Control { name: "references", field: Field::Projection(|s| &mut s.config.references) },
    Control { name: "options", field: Field::Text(|s| &mut s.config.options, None) },
    Control { name: "paths", field: Field::Text(|s| &mut s.config.paths, None) },
    Control { name: "base", field: Field::Text(|s| &mut s.config.base, None) },
    Control { name: "format", field: Field::Format },
];

const PACKAGES: &[Control] = &[
    Control { name: "directories", field: Field::Strings(|s| &mut s.packages.directories, None) },
    Control { name: "metadata", field: Field::Text(|s| &mut s.packages.metadata, None) },
    Control { name: "entries", field: Field::Strings(|s| &mut s.packages.entries, None) },
    Control { name: "exports", field: Field::Text(|s| &mut s.packages.exports, None) },
    Control { name: "imports", field: Field::Text(|s| &mut s.packages.imports, None) },
    Control { name: "conditions", field: Field::Strings(|s| &mut s.conditions, None) },
    Control { name: "scoped_prefix", field: Field::Text(|s| &mut s.packages.scoped_prefix, None) },
    Control { name: "type_directory", field: Field::Text(|s| &mut s.packages.type_directory, None) },
    Control { name: "type_join", field: Field::Text(|s| &mut s.packages.type_join, None) },
];

const GLOBALS: &[Control] = &[
    Control { name: "libraries", field: Field::Strings(|s| &mut s.globals.libraries, Some(&["package_dir", "name"])) },
    Control { name: "library_prefix", field: Field::Text(|s| &mut s.globals.library_prefix, None) },
    Control { name: "library_option", field: Field::Text(|s| &mut s.globals.library_option, None) },
    Control { name: "default_libraries", field: Field::Strings(|s| &mut s.globals.default_libraries, None) },
    Control { name: "types_option", field: Field::Text(|s| &mut s.globals.types_option, None) },
    Control { name: "type_roots_option", field: Field::Text(|s| &mut s.globals.type_roots_option, None) },
    Control { name: "type_directories", field: Field::Strings(|s| &mut s.globals.type_directories, None) },
    Control { name: "project_files", field: Field::Strings(|s| &mut s.globals.project_files, None) },
    Control { name: "references", field: Field::Bool(|s| &mut s.globals.references) },
    Control { name: "name_hints", field: Field::Strings(|s| &mut s.globals.name_hints, None) },
    Control { name: "ambient_hints", field: Field::Strings(|s| &mut s.globals.ambient_hints, None) },
];

struct Compiler<'a> {
    grammar: &'a Grammar,
    vocabulary: &'a Vocabulary,
    errors: &'a mut Vec<ConfigError>,
}

impl Compiler<'_> {
    fn apply(&mut self, source: &[Setting], controls: &[Control], settings: &mut Settings) {
        let mut seen = std::collections::HashSet::new();
        for setting in source {
            if !seen.insert(&setting.key.text) {
                self.errors.push(ConfigError::new(setting.key.span, "module resolution setting is repeated"));
                continue;
            }
            let Some(control) = controls.iter().find(|control| control.name == setting.key.text) else {
                self.errors.push(ConfigError::new(setting.key.span, format!("unknown module resolution setting `{}`", setting.key.text)).suggest(&setting.key.text, controls.iter().map(|control| control.name)));
                continue;
            };
            if let Err(error) = self.field(setting, &control.field, settings) { self.errors.push(error) }
        }
    }

    fn field(&mut self, setting: &Setting, field: &Field, settings: &mut Settings) -> Result<(), ConfigError> {
        let span = setting.key.span;
        if let Field::Block(controls) = field {
            let SettingValue::Block(source) = &setting.value else { return Err(ConfigError::new(span, "expected a settings block")) };
            self.apply(source, controls, settings);
            return Ok(());
        }
        if matches!(field, Field::Substitutions) {
            let SettingValue::Block(source) = &setting.value else { return Err(ConfigError::new(span, "expected a substitutions block")) };
            let mut seen = std::collections::HashSet::new();
            for setting in source {
                if !seen.insert(&setting.key.text) { return Err(ConfigError::new(setting.key.span, "extension substitution is repeated")) }
                let value = self.value(&setting.value, setting.key.span, false)?;
                let values = strings(&value, setting.key.span)?;
                if let Some((_, previous)) = settings.substitutions.iter_mut().find(|(key, _)| **key == setting.key.text) {
                    *previous = values;
                } else {
                    settings.substitutions.push((setting.key.text.clone().into(), values));
                }
            }
            return Ok(());
        }
        let value = self.value(&setting.value, span, matches!(field, Field::Search | Field::Scopes | Field::Format))?;
        match field {
            Field::Text(target, variables) => {
                let Value::Str(text) = value else { return Err(ConfigError::new(span, "expected text")) };
                if let Some(variables) = variables { template(&text, variables, span)?; }
                *target(settings) = text.to_string().into();
            }
            Field::Projection(target) => {
                let Value::Str(text) = value else { return Err(ConfigError::new(span, "expected a metadata projection")) };
                super::metadata::projection(&text).map_err(|()| ConfigError::new(span, "invalid metadata projection"))?;
                *target(settings) = text.to_string().into();
            }
            Field::Strings(target, variables) => {
                let values = strings(&value, span)?;
                if let Some(variables) = variables { for value in &values { template(value, variables, span)?; } }
                *target(settings) = values;
            }
            Field::Bool(target) => {
                let Value::Bool(value) = value else { return Err(ConfigError::new(span, "expected true or false")) };
                *target(settings) = value;
            }
            Field::Search => {
                settings.search = strings(&value, span)?.iter().map(|name| match &**name {
                    "relative" => Ok(Search::Relative),
                    "config_paths" => Ok(Search::Paths),
                    "config_base" => Ok(Search::Base),
                    "ambient" => Ok(Search::Ambient),
                    "packages" => Ok(Search::Packages),
                    _ => Err(ConfigError::new(span, format!("unknown search operation `{name}`"))),
                }).collect::<Result<_, _>>()?;
            }
            Field::Scopes => {
                settings.ambient_scopes = strings(&value, span)?.iter().try_fold(0, |classes, name| {
                    let bit = self.vocabulary.scope_class(name);
                    if bit == 0 { Err(ConfigError::new(span, format!("unknown scope class `{name}`"))) } else { Ok(classes | bit) }
                })?;
            }
            Field::Format => {
                if !matches!(value, Value::Str(ref value) if &**value == "jsonc") { return Err(ConfigError::new(span, "unsupported metadata format")) }
            }
            Field::Block(_) | Field::Substitutions => unreachable!(),
        }
        Ok(())
    }

    fn value(&mut self, value: &SettingValue, span: Span, words: bool) -> Result<Value<'static>, ConfigError> {
        let SettingValue::Literal(value) = value else { return Err(ConfigError::new(span, "expected a constant value")) };
        let value = if words { enum_words(value) } else { value.clone() };
        let compiled = Scope::constants(self.grammar).compile(&value, self.errors).ok_or_else(|| ConfigError::new(span, "module resolution settings must be constants"))?;
        Ctx::eval_static(&compiled, &[]).map(|value| value.into_owned()).map_err(|error| ConfigError::new(span, format!("module resolution settings must be constants: {error}")))
    }
}

fn enum_words(expr: &Expr) -> Expr {
    let kind = match &expr.kind {
        ExprKind::Var(path) if path.len() == 1 => ExprKind::Str(path[0].text.clone()),
        ExprKind::List(values) => ExprKind::List(values.iter().map(enum_words).collect()),
        kind => kind.clone(),
    };
    Expr { kind, span: expr.span }
}

fn strings(value: &Value<'_>, span: Span) -> Result<Vec<Box<str>>, ConfigError> {
    let Value::List(values) = value else { return Err(ConfigError::new(span, "expected a list of text")) };
    values.iter().map(|value| match value {
        Value::Str(text) => Ok(text.to_string().into()),
        _ => Err(ConfigError::new(span, "expected a list of text")),
    }).collect()
}

fn template(value: &str, variables: &[&str], span: Span) -> Result<(), ConfigError> {
    let mut rest = value;
    while let Some(start) = rest.find('{') {
        if rest[..start].contains('}') { return Err(ConfigError::new(span, "unmatched template brace")) }
        rest = &rest[start + 1..];
        let Some(end) = rest.find('}') else { return Err(ConfigError::new(span, "unclosed template placeholder")) };
        let placeholder = &rest[..end];
        let (name, _) = super::resolution::TemplateTransform::parse(placeholder)
            .map_err(|_| ConfigError::new(span, format!("unknown template transform in `{placeholder}`")))?;
        if !variables.contains(&name) { return Err(ConfigError::new(span, format!("unknown template placeholder `{name}`"))) }
        rest = &rest[end + 1..];
    }
    if rest.contains('}') { return Err(ConfigError::new(span, "unmatched template brace")) }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ast::{Expr, ExprKind, Name};
    use crate::config::ast::modules::{Definition, Mode, Setting, SettingValue};
    use crate::config::source::{FileId, Span};
    use super::super::resolution::{Host, Resolver};
    use super::super::super::question::{Known, QueryContext, ScopeRef};
    use std::path::{Path, PathBuf};

    fn span() -> Span { Span::new(FileId(0), 0, 0) }
    fn name(text: &str) -> Name { Name { text: text.into(), span: span() } }
    fn string(text: &str) -> Expr { Expr { kind: ExprKind::Str(text.into()), span: span() } }
    fn list(values: &[&str]) -> Expr { Expr { kind: ExprKind::List(values.iter().map(|value| string(value)).collect()), span: span() } }
    fn setting(key: &str, value: Expr) -> Setting { Setting { key: name(key), value: SettingValue::Literal(value) } }
    fn config(settings: Vec<Setting>, modes: Vec<Mode>) -> Config {
        Config { module_resolutions: vec![Definition { name: name("example"), settings, modes }], ..Config::default() }
    }

    #[test]
    fn resolution_settings_reject_ignored_or_unexpandable_controls() {
        for settings in [
            vec![setting("unexpected_search", string("relative"))],
            vec![setting("candidates", list(&["{missing}/{name}"]))],
            vec![Setting { key: name("config"), value: SettingValue::Block(vec![setting("references", string("references[invalid].path"))]) }],
        ] {
            let config = config(settings, Vec::new());
            let grammar = Grammar::build(&config, &mut Vec::new());
            let mut errors = Vec::new();
            compile(&config, &grammar, &Vocabulary::default(), &mut errors, &mut |_, _| 0);
            assert!(!errors.is_empty());
        }
    }

    struct Files;
    impl Host for Files {
        fn program(&self) -> Known<u32> { Known::Yes(0) }
        fn path(&self, _: u32) -> Known<PathBuf> { Known::Yes("/root/main.ts".into()) }
        fn text(&self, _: &Path, _: &QueryContext) -> Known<Option<String>> { Known::Yes(None) }
        fn is_file(&self, path: &Path, _: &QueryContext) -> Known<bool> { Known::Yes(path == Path::new("/root/dep.ts")) }
        fn scope(&self, path: &Path, _: &QueryContext) -> Known<Option<ScopeRef>> {
            Known::Yes((path == Path::new("/root/dep.ts")).then_some(ScopeRef { file: 1, scope: 0 }))
        }
        fn ambient(&self, _: u32, _: &str, _: u64, _: &QueryContext) -> Known<Option<ScopeRef>> { Known::Yes(None) }
    }

    #[test]
    fn mode_overrides_keep_the_unmodified_search_policy() {
        let config = config(vec![
            setting("search", list(&["relative"])),
            setting("extensions", list(&[".ts"])),
            setting("candidates", list(&["{dir}/{name}{ext}"])),
        ], vec![Mode { when: Expr { kind: ExprKind::Bool(true), span: span() }, settings: vec![setting("conditions", list(&["custom"]))] }]);
        let grammar = Grammar::build(&config, &mut Vec::new());
        let mut errors = Vec::new();
        let profiles = compile(&config, &grammar, &Vocabulary::default(), &mut errors, &mut |_, _| 0);
        assert!(errors.is_empty());
        let profile = profiles.first().expect("valid profile is compiled");
        let mode = profile.modes.first().expect("mode is compiled");
        for settings in [&profile.settings, &mode.settings] {
            assert_eq!(Resolver { host: &Files, settings }.resolve(0, "./dep", &[], &QueryContext::default()), Known::Yes(Some(ScopeRef { file: 1, scope: 0 })));
        }
    }
}
