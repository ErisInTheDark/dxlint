//! Compiles a parsed configuration into a [`Linter`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::discriminant;
use std::path::PathBuf;

use super::symbols::Symbols;
use super::{Linter, OptionSlot, Rule, Target};
use crate::config::ast::{Dropped, Parent, RuleDef, SetValue, Severity, Target as AstTarget};
use crate::config::error::ConfigError;
use crate::config::source::SourceMap;
use crate::config::Loaded;
use crate::expr::{Attr, CExpr, Ctx, Scope, Value};
use crate::grammar::Grammar;
use crate::path_globs::{wildcard, PathGlobs};
use crate::pattern::Pat;

pub struct Built {
    /// Always built: broken matchers, rules and settings are left out
    /// (and reported in `errors`), so one mistake never turns linting off.
    /// Still an `Option` for existing callers.
    pub linter: Option<Linter>,
    pub errors: Vec<ConfigError>,
    pub sources: std::sync::Arc<SourceMap>,
    /// Every name, its definitions and uses (for tooling).
    pub symbols: std::sync::Arc<Symbols>,
}

/// How expressions resolved their names, collected while compiling.
type Uses = RefCell<Vec<(crate::config::source::Span, crate::expr::Resolved)>>;

pub fn build(loaded: Loaded, root: PathBuf) -> Built {
    let Loaded { config, sources, mut errors, fingerprint } = loaded;
    let grammar = Grammar::build(&config, &mut errors);
    let uses: Uses = RefCell::default();
    let mut attr_names: Vec<String> = Vec::new();
    for def in &config.attrs {
        if !attr_names.contains(&def.name.text) {
            attr_names.push(def.name.text.clone());
        }
    }
    let attrs = attributes(&config, &grammar, &attr_names, &uses, &mut errors);
    let forwards = forwards(&config, &grammar, &attr_names, &mut errors);

    let mut rules: Vec<Rule> = Vec::new();
    // Rules that failed to build: already reported, so settings aimed at
    // them are quietly dropped instead of reported again. (The parser keeps
    // rule names unique.)
    let mut broken: Vec<&str> = Vec::new();
    for def in &config.rules {
        match rule(def, &grammar, &attr_names, &uses, &mut errors) {
            Some(r) if !def.broken => rules.push(r),
            _ => broken.push(&def.name.text),
        }
    }

    for set in &config.sets {
        let matching: Vec<usize> = (0..rules.len()).filter(|&i| wildcard(&set.target.text, &rules[i].name)).collect();
        if matching.is_empty() && (set.soft || broken.iter().any(|name| wildcard(&set.target.text, name))) {
            continue;
        }
        if matching.is_empty() {
            if let Some(d) = Dropped::find(&config.dropped, &set.target.text) {
                errors.push(d.still_used(set.target.span));
                continue;
            }
            errors.push(
                ConfigError::new(set.target.span, format!("no rule matches `{}`", set.target.text))
                    .suggest(&set.target.text, rules.iter().map(|f| f.name.as_str())),
            );
            continue;
        }
        let value = match &set.value {
            SetValue::Severity(s) => {
                for &i in &matching {
                    rules[i].severity = *s;
                }
                continue;
            }
            SetValue::Expr(e) => {
                let scope = Scope::constants(&grammar);
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
            let f = &mut rules[i];
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
                matching.iter().flat_map(|&i| rules[i].options.iter().map(|o| o.name.as_str())).collect();
            let mut err = ConfigError::new(set.key.span, format!("no option `{}` on `{}`", set.key.text, set.target.text))
                .suggest(&set.key.text, names.iter().copied());
            if names.is_empty() {
                err = err.note("this rule has no options; its severity can always be set (`set name off`)");
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
        let key = (e.span, e.message.clone(), e.related.first().map(|(s, _)| *s));
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });

    let sources = std::sync::Arc::new(sources);
    let symbols = std::sync::Arc::new(Symbols::build(&config, &grammar, &rules, uses.into_inner(), sources.clone()));
    // The rules as parsed, and where paths are relative to: what cached
    // results are only valid for.
    let fingerprint = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        fingerprint.hash(&mut h);
        root.hash(&mut h);
        h.finish()
    };
    let mut by_matcher = vec![Vec::new(); grammar.matchers.len()];
    let mut by_field: HashMap<(u32, u16), Vec<usize>> = HashMap::new();
    let mut by_lang = Vec::new();
    for (i, f) in rules.iter().enumerate() {
        for &target in &f.targets {
            match target {
                // Rules on a matcher also see its specialisations.
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
    let linter = Linter {
        reach: grammar.reach(),
        profile: None,
        grammar,
        rules,
        sources: sources.clone(),
        root,
        ignores,
        by_matcher,
        by_field,
        by_lang,
        attrs,
        forwards,
        project: Default::default(),
        fingerprint,
    };
    Built { linter: Some(linter), errors, sources, symbols }
}

/// Labels available on a node of `target`.
fn target_fields(g: &Grammar, target: Target) -> Vec<(String, u16)> {
    let names = match target {
        Target::Matcher(m) => g.matchers[m as usize].labels(),
        Target::Field(m, label) => {
            let pat = &g.matchers[m as usize].pat;
            let label_name = g.labels.name(label);
            match pat.label_target(label_name).and_then(|t| g.matcher(t)) {
                Some(t) => g.matchers[t as usize].labels(),
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

/// Compiles every `let` attribute on each layer that defines it.
fn attributes(config: &crate::config::ast::Config, g: &Grammar, names: &[String], uses: &Uses, errors: &mut Vec<ConfigError>) -> Vec<Attr> {
    let mut attrs: Vec<Attr> = names.iter().map(|n| Attr { name: n.clone(), defs: Vec::new() }).collect();
    for def in &config.attrs {
        if crate::expr::is_builtin(&def.name.text) {
            errors.push(ConfigError::new(def.name.span, format!("`{}` is a built-in variable and cannot name an attribute", def.name.text)));
            continue;
        }
        let id = names.iter().position(|n| *n == def.name.text).expect("collected");
        for target in &def.targets {
            let Some((resolved, _)) = resolve_target(&AstTarget::Matcher(target.clone()), g, errors) else { continue };
            for t in resolved {
                let Target::Matcher(m) = t else {
                    errors.push(ConfigError::new(target.span, "attributes belong to layers, not languages"));
                    continue;
                };
                if attrs[id].defs.iter().any(|(d, _)| *d == m) {
                    errors.push(
                        ConfigError::new(
                            def.name.span,
                            format!("attribute `{}` is defined twice for `{}`", def.name.text, g.matchers[m as usize].name),
                        )
                        .note(format!("`override in {} let {} ...` replaces it", g.matchers[m as usize].name, def.name.text)),
                    );
                    continue;
                }
                let scope = Scope { fields: target_fields(g, t), attrs: names, uses: Some(uses), ..Scope::new(g) };
                if let Some(e) = scope.compile(&def.expr, errors) {
                    attrs[id].defs.push((m, e));
                }
            }
        }
    }
    attrs
}

fn rule(def: &RuleDef, g: &Grammar, attrs: &[String], uses: &Uses, errors: &mut Vec<ConfigError>) -> Option<Rule> {
    let mut targets = Vec::new();
    let mut texts = Vec::new();
    for t in &def.targets {
        let (resolved, text) = resolve_target(t, g, errors)?;
        targets.extend(resolved);
        texts.push(text);
    }
    let target_text = texts.join(" or ");
    if targets.is_empty() {
        errors.push(ConfigError::new(def.name.span, format!("rule `{}` has nothing to check: its group is empty", def.name.text)));
        return None;
    }

    let before = errors.len();
    let mut options: Vec<OptionSlot> = Vec::new();
    for opt in &def.options {
        if options.iter().any(|o| o.name == opt.name.text) {
            errors.push(ConfigError::new(opt.name.span, format!("option `{}` is declared twice", opt.name.text)));
            continue;
        }
        let Some(compiled) = Scope::constants(g).compile(&opt.default, errors) else { continue };
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
    let mut scope = Scope { fields, options: option_names, attrs, rule: Some(&def.name.text), uses: Some(uses), ..Scope::new(g) };
    // Each `let` sees the ones before it.
    let mut locals = Vec::new();
    for (name, e) in &def.locals {
        locals.push(scope.compile(e, errors).unwrap_or(CExpr::Const(Value::Null)));
        scope.locals.push(name.text.clone());
    }

    let compile_all = |exprs: &[crate::config::ast::Expr], errors: &mut Vec<ConfigError>| -> Vec<CExpr> {
        exprs.iter().filter_map(|e| scope.compile(e, errors)).collect()
    };
    let when = compile_all(&def.when, errors);
    let message = def.message.as_ref().and_then(|m| scope.compile(m, errors));
    let fix = def.fix.as_ref().and_then(|s| scope.compile(s, errors));

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
    Some(Rule {
        name: def.name.text.clone(),
        span: def.name.span,
        targets,
        target_text,
        severity: def.severity.unwrap_or(Severity::Warn),
        options,
        files,
        ignore,
        when,
        locals,
        message,
        fix,
        report: def.report.unwrap_or_default(),
    })
}

/// `in a forward x from field` as (matcher, attribute, label); groups give
/// one per member.
fn forwards(config: &crate::config::ast::Config, g: &Grammar, attr_names: &[String], errors: &mut Vec<ConfigError>) -> Vec<(u32, u16, u16)> {
    let mut out: Vec<(u32, u16, u16)> = Vec::new();
    for def in &config.forwards {
        let mut attrs = Vec::new();
        for a in &def.attrs {
            match attr_names.iter().position(|n| *n == a.text) {
                Some(i) => attrs.push((i as u16, a)),
                None => errors.push(match Dropped::find(&config.dropped, &a.text) {
                    Some(d) => d.still_used(a.span),
                    None => ConfigError::new(a.span, format!("no attribute `{}`", a.text)).suggest(&a.text, attr_names.iter().map(String::as_str)),
                }),
            }
        }
        for target in &def.targets {
            let Some((resolved, _)) = resolve_target(&AstTarget::Matcher(target.clone()), g, errors) else { continue };
            for t in resolved {
                let Target::Matcher(m) = t else {
                    errors.push(ConfigError::new(target.span, "`forward` belongs to layers, not languages"));
                    continue;
                };
                let labels = g.matchers[m as usize].labels();
                let Some(label) = g.labels.get(&def.field.text).filter(|_| labels.contains(&def.field.text)) else {
                    errors.push(
                        ConfigError::new(def.field.span, format!("`{}` has no field `{}`", g.matchers[m as usize].name, def.field.text))
                            .suggest(&def.field.text, labels.iter().map(String::as_str)),
                    );
                    continue;
                };
                for &(attr, name) in &attrs {
                    if out.iter().any(|&(x, a, _)| x == m && a == attr) {
                        errors.push(ConfigError::new(name.span, format!("`{}` already forwards `{}`", g.matchers[m as usize].name, name.text)));
                    } else {
                        out.push((m, attr, label));
                    }
                }
            }
        }
    }
    out
}

/// A rule target as engine targets (a group gives one per member), and how
/// to show it.
fn resolve_target(target: &AstTarget, g: &Grammar, errors: &mut Vec<ConfigError>) -> Option<(Vec<Target>, String)> {
    Some(match target {
        AstTarget::Matcher(name) => match g.matcher(&name.text) {
            Some(m) => (vec![Target::Matcher(m)], name.text.clone()),
            None if g.group(&name.text).is_some() => {
                let mut out = Vec::new();
                for member in g.group(&name.text).unwrap_or_default() {
                    match member {
                        Parent::Matcher(n) => match g.matcher(&n.text) {
                            Some(m) => out.push(Target::Matcher(m)),
                            // Unknown members are reported where the group attaches them.
                            None => continue,
                        },
                        Parent::Lang(n) => out.extend(g.lang(&n.text).map(|l| Target::Lang(Some(l)))),
                        Parent::AnyLang(_) => out.push(Target::Lang(None)),
                    }
                }
                (out, name.text.clone())
            }
            None => {
                if let Some(d) = Dropped::find(&g.dropped, &name.text) {
                    errors.push(d.still_used(name.span));
                    return None;
                }
                let mut err = ConfigError::new(name.span, format!("unknown matcher `{}`", name.text))
                    .suggest(&name.text, g.matchers.iter().map(|m| m.name.as_str()));
                if let Some(owner) = g.matchers.iter().find(|m| m.labels().contains(&name.text)) {
                    err = err.note(format!("`{}` is a field of `{}`; target it as `{}.{}`", name.text, owner.name, owner.name, name.text));
                }
                errors.push(err);
                return None;
            }
        },
        AstTarget::Field(layer, field) => {
            let Some(m) = g.matcher(&layer.text) else {
                errors.push(match Dropped::find(&g.dropped, &layer.text) {
                    Some(d) => d.still_used(layer.span),
                    None => ConfigError::new(layer.span, format!("unknown matcher `{}`", layer.text))
                        .suggest(&layer.text, g.matchers.iter().map(|m| m.name.as_str())),
                });
                return None;
            };
            let labels = g.matchers[m as usize].labels();
            match g.labels.get(&field.text).filter(|_| labels.contains(&field.text)) {
                Some(l) => (vec![Target::Field(m, l)], format!("{}.{}", layer.text, field.text)),
                None => {
                    errors.push(
                        ConfigError::new(field.span, format!("matcher `{}` has no field `{}`", layer.text, field.text))
                            .suggest(&field.text, labels.iter().map(String::as_str)),
                    );
                    return None;
                }
            }
        }
        AstTarget::Lang(name) if name.text == "*" => (vec![Target::Lang(None)], ".*".into()),
        AstTarget::Lang(name) => match g.lang(&name.text) {
            Some(l) => (vec![Target::Lang(Some(l))], format!(".{}", name.text)),
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
