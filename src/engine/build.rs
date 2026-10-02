//! Compiles a parsed configuration into a [`Linter`].

use std::collections::HashMap;
use std::mem::discriminant;
use std::path::PathBuf;

use super::{Formatter, Linter, OptionSlot, Target};
use crate::config::ast::{FormatDef, SetValue, Severity, Target as AstTarget};
use crate::config::error::ConfigError;
use crate::config::source::SourceMap;
use crate::config::Loaded;
use crate::expr::{CExpr, Ctx, Scope, Value};
use crate::grammar::Grammar;
use crate::path_globs::{wildcard, PathGlobs};
use crate::pattern::Pat;

pub struct Built {
    /// Always built: broken matchers, formatters and settings are left out
    /// (and reported in `errors`), so one mistake never turns linting off.
    /// Still an `Option` for existing callers.
    pub linter: Option<Linter>,
    pub errors: Vec<ConfigError>,
    pub sources: std::sync::Arc<SourceMap>,
}

pub fn build(loaded: Loaded, root: PathBuf) -> Built {
    let Loaded { config, sources, mut errors } = loaded;
    let grammar = Grammar::build(&config, &mut errors);

    let mut formatters: Vec<Formatter> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Formatters that failed to build: already reported, so settings aimed
    // at them are quietly dropped instead of reported again.
    let mut broken: Vec<&str> = Vec::new();
    for def in &config.formats {
        // Checked by name, so a broken first definition still counts.
        if !seen.insert(def.name.text.as_str()) {
            errors.push(
                ConfigError::new(def.name.span, format!("formatter `{}` is defined twice", def.name.text))
                    .note("configure existing formatters with `set name.option = value`"),
            );
            continue;
        }
        match formatter(def, &grammar, &mut errors) {
            Some(f) => formatters.push(f),
            None => broken.push(&def.name.text),
        }
    }

    for set in &config.sets {
        let matching: Vec<usize> =
            (0..formatters.len()).filter(|&i| wildcard(&set.target.text, &formatters[i].name)).collect();
        if matching.is_empty() && broken.iter().any(|name| wildcard(&set.target.text, name)) {
            continue;
        }
        if matching.is_empty() {
            errors.push(
                ConfigError::new(set.target.span, format!("no formatter matches `{}`", set.target.text))
                    .suggest(&set.target.text, formatters.iter().map(|f| f.name.as_str())),
            );
            continue;
        }
        let value = match &set.value {
            SetValue::Severity(s) => {
                for &i in &matching {
                    formatters[i].severity = *s;
                }
                continue;
            }
            SetValue::Expr(e) => {
                let scope = Scope { grammar: &grammar, fields: Vec::new(), options: Vec::new(), options_only: true };
                match scope.compile(e, &mut errors).map(|c| Ctx::eval_static(&c, &[])) {
                    Some(Ok(v)) => v,
                    Some(Err(err)) => {
                        errors.push(ConfigError::new(e.span, format!("option values must be constants: {err}")));
                        continue;
                    }
                    None => continue,
                }
            }
        };
        let mut any = false;
        for &i in &matching {
            let f = &mut formatters[i];
            if let Some(slot) = f.options.iter_mut().find(|o| o.name == set.key.text) {
                any = true;
                if !matches!(slot.default, Value::Null) && discriminant(&slot.default) != discriminant(&value) {
                    errors.push(ConfigError::new(
                        set.key.span,
                        format!(
                            "option `{}.{}` is a {}, but this value is a {}",
                            f.name,
                            slot.name,
                            slot.default.type_name(),
                            value.type_name()
                        ),
                    ));
                    continue;
                }
                slot.value = value.clone();
            }
        }
        if !any {
            let names: Vec<&str> =
                matching.iter().flat_map(|&i| formatters[i].options.iter().map(|o| o.name.as_str())).collect();
            let mut err = ConfigError::new(set.key.span, format!("no option `{}` on `{}`", set.key.text, set.target.text))
                .suggest(&set.key.text, names.iter().copied().chain(["severity"]));
            if names.is_empty() {
                err = err.note("this formatter has no options; `severity` can always be set");
            }
            errors.push(err);
        }
    }

    let ignores = match PathGlobs::new(config.ignores.iter().map(|(g, _)| g.as_str())) {
        Ok(g) => g,
        Err((glob, e)) => {
            let span = config.ignores.iter().find(|(g, _)| *g == glob).map(|(_, s)| *s);
            errors.push(ConfigError::new(span, format!("invalid glob `{glob}`: {e}")));
            PathGlobs::default()
        }
    };

    // A mistake inside a macro is one mistake, however often it expands.
    let mut seen = Vec::new();
    errors.retain(|e| {
        let key = (e.span, e.message.clone());
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });

    let sources = std::sync::Arc::new(sources);
    let mut by_matcher = vec![Vec::new(); grammar.matchers.len()];
    let mut by_field: HashMap<(u32, u16), Vec<usize>> = HashMap::new();
    let mut by_lang = Vec::new();
    for (i, f) in formatters.iter().enumerate() {
        for &target in &f.targets {
            match target {
                // Formatters on a matcher also see its specialisations.
                Target::Matcher(m) => {
                    for (x, list) in by_matcher.iter_mut().enumerate() {
                        if grammar.is_a(x as u32, m) && !list.contains(&i) {
                            list.push(i);
                        }
                    }
                }
                Target::Field(m, l) => by_field.entry((m, l)).or_default().push(i),
                Target::Lang(lang) => by_lang.push((i, lang)),
            }
        }
    }
    let linter =
        Linter { reach: grammar.reach(), profile: None, grammar, formatters, sources: sources.clone(), root, ignores, by_matcher, by_field, by_lang };
    Built { linter: Some(linter), errors, sources }
}

/// Labels available on a node of `target`.
fn target_fields(g: &Grammar, target: Target) -> Vec<(String, u16)> {
    let names = match target {
        Target::Matcher(m) => g.matchers[m as usize].pat.labels(),
        Target::Field(m, label) => {
            let pat = &g.matchers[m as usize].pat;
            let label_name = g.labels.name(label);
            match pat.label_target(label_name).and_then(|t| g.matcher(t)) {
                Some(t) => g.matchers[t as usize].pat.labels(),
                None => capture_inner_labels(pat, label_name),
            }
        }
        Target::Lang(_) => Vec::new(),
    };
    names.into_iter().filter_map(|n| g.labels.get(&n).map(|id| (n, id))).collect()
}

fn capture_inner_labels(pat: &Pat, label: &str) -> Vec<String> {
    let mut out = Vec::new();
    pat.walk(&mut |p| {
        if let Pat::Capture { name, inner } = p {
            if name == label {
                for l in inner.labels() {
                    if !out.contains(&l) {
                        out.push(l);
                    }
                }
            }
        }
    });
    out
}

fn formatter(def: &FormatDef, g: &Grammar, errors: &mut Vec<ConfigError>) -> Option<Formatter> {
    let mut targets = Vec::new();
    let mut texts = Vec::new();
    for t in &def.targets {
        let (target, text) = resolve_target(t, g, errors)?;
        targets.push(target);
        texts.push(text);
    }
    let target_text = texts.join(" | ");

    let before = errors.len();
    let mut options: Vec<OptionSlot> = Vec::new();
    for opt in &def.options {
        if options.iter().any(|o| o.name == opt.name.text) {
            errors.push(ConfigError::new(opt.name.span, format!("option `{}` is declared twice", opt.name.text)));
            continue;
        }
        let scope = Scope { grammar: g, fields: Vec::new(), options: Vec::new(), options_only: true };
        let Some(compiled) = scope.compile(&opt.default, errors) else { continue };
        match Ctx::eval_static(&compiled, &[]) {
            Ok(v) => options.push(OptionSlot {
                name: opt.name.text.clone(),
                description: opt.description.clone(),
                default: v.clone(),
                value: v,
            }),
            Err(e) => errors.push(ConfigError::new(opt.default.span, format!("option defaults must be constants: {e}"))),
        }
    }
    let option_names: Vec<String> = options.iter().map(|o| o.name.clone()).collect();

    // With several targets, only fields every target has are usable.
    let mut fields = target_fields(g, targets[0]);
    for &t in &targets[1..] {
        let other = target_fields(g, t);
        fields.retain(|f| other.contains(f));
    }
    for (field, _) in &fields {
        if let Some(opt) = def.options.iter().find(|o| o.name.text == *field) {
            errors.push(ConfigError::new(opt.name.span, format!("option `{field}` has the same name as a field of `{target_text}`")));
        }
    }
    let scope = Scope { grammar: g, fields, options: option_names, options_only: false };

    let compile_all = |exprs: &[crate::config::ast::Expr], errors: &mut Vec<ConfigError>| -> Vec<CExpr> {
        exprs.iter().filter_map(|e| scope.compile(e, errors)).collect()
    };
    let description = def.description.as_ref().and_then(|d| scope.compile(d, errors));
    let when = compile_all(&def.when, errors);
    let message = def.message.as_ref().and_then(|m| scope.compile(m, errors));
    let shape = def.shape.as_ref().and_then(|s| scope.compile(s, errors));

    let globs = |list: &[(String, crate::config::source::Span)], errors: &mut Vec<ConfigError>| {
        PathGlobs::new(list.iter().map(|(g, _)| g.as_str())).unwrap_or_else(|(glob, e)| {
            let span = list.iter().find(|(g, _)| *g == glob).map(|(_, s)| *s);
            errors.push(ConfigError::new(span, format!("invalid glob `{glob}`: {e}")));
            PathGlobs::default()
        })
    };
    let files = globs(&def.files, errors);
    let ignore = globs(&def.ignore, errors);

    if errors.len() > before {
        return None;
    }
    Some(Formatter {
        name: def.name.text.clone(),
        span: def.name.span,
        targets,
        target_text,
        severity: def.severity.unwrap_or(Severity::Warn),
        options,
        description,
        files,
        ignore,
        when,
        message,
        shape,
        report: def.report,
    })
}

fn resolve_target(target: &AstTarget, g: &Grammar, errors: &mut Vec<ConfigError>) -> Option<(Target, String)> {
    Some(match target {
        AstTarget::Matcher(name) => match g.matcher(&name.text) {
            Some(m) => (Target::Matcher(m), name.text.clone()),
            None => {
                let mut err = ConfigError::new(name.span, format!("unknown matcher `{}`", name.text))
                    .suggest(&name.text, g.matchers.iter().map(|m| m.name.as_str()));
                if let Some(owner) = g.matchers.iter().find(|m| m.pat.labels().contains(&name.text)) {
                    err = err.note(format!("`{}` is a field of `{}`; target it as `{}.{}`", name.text, owner.name, owner.name, name.text));
                }
                errors.push(err);
                return None;
            }
        },
        AstTarget::Field(layer, field) => {
            let Some(m) = g.matcher(&layer.text) else {
                errors.push(
                    ConfigError::new(layer.span, format!("unknown matcher `{}`", layer.text))
                        .suggest(&layer.text, g.matchers.iter().map(|m| m.name.as_str())),
                );
                return None;
            };
            let labels = g.matchers[m as usize].pat.labels();
            match g.labels.get(&field.text).filter(|_| labels.contains(&field.text)) {
                Some(l) => (Target::Field(m, l), format!("{}.{}", layer.text, field.text)),
                None => {
                    errors.push(
                        ConfigError::new(field.span, format!("matcher `{}` has no field `{}`", layer.text, field.text))
                            .suggest(&field.text, labels.iter().map(String::as_str)),
                    );
                    return None;
                }
            }
        }
        AstTarget::Lang(name) if name.text == "*" => (Target::Lang(None), ".*".into()),
        AstTarget::Lang(name) => match g.lang(&name.text) {
            Some(l) => (Target::Lang(Some(l)), format!(".{}", name.text)),
            None => {
                errors.push(
                    ConfigError::new(name.span, format!("unknown language `.{}`", name.text))
                        .suggest(&name.text, g.langs.iter().map(|l| l.name.as_str())),
                );
                return None;
            }
        },
    })
}
