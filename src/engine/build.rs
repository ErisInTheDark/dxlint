//! Compiles a parsed configuration into a [`Linter`].

use std::cell::RefCell;
use std::path::PathBuf;

use super::symbols::Symbols;
use super::{InertRule, Linter, OptionSlot, Rule, Target};
use crate::config::ast::{Config, Dropped, Expr, ExprKind, Name, Parent, RuleDef, Severity, Target as AstTarget, Use};
use crate::config::schema::{Schema, Schemas};
use crate::config::error::ConfigError;
use crate::config::source::SourceMap;
use crate::config::Loaded;
use crate::expr::{Attr, AttrNames, Attributes, CExpr, Ctx, Scope, Value};
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
    /// Every name, its definitions and uses; absent in runtime-only builds.
    pub symbols: Option<std::sync::Arc<Symbols>>,
}

/// How expressions resolved their names, collected while compiling.
type Uses = RefCell<Vec<(crate::config::source::Span, crate::expr::Resolved)>>;

/// Compile-time prerequisites of a helper, flattened into its consuming rules.
#[derive(Default)]
struct ReadDependencies {
    layers: Vec<u32>,
    helpers: Vec<usize>,
    /// Every token kind, when some are built (a read would see the others missing).
    all_tokens: bool,
    /// Every token kind, always: reads of nodes' own `token` field.
    own_tokens: bool,
    requires_native: bool,
}

struct ReadRequirements {
    layers: Vec<u32>,
    all_tokens: bool,
    own_tokens: bool,
    requires_native: bool,
}

impl ReadDependencies {
    fn collect(g: &Grammar, attrs: &AttrNames, uses: &[(crate::config::source::Span, crate::expr::Resolved)], layers: Vec<u32>, all_tokens: bool) -> Self {
        use crate::expr::Resolved;
        let helpers = uses.iter().filter_map(|(_, resolved)| match resolved {
            Resolved::Attr(name) => attrs.get(name).map(usize::from),
            Resolved::UserFunction(name) => g.function(name).map(|id| attrs.len() + id),
            _ => None,
        }).collect();
        let own_tokens = uses.iter().any(|(_, resolved)| matches!(resolved, Resolved::Field(name) if name == "token"));
        let requires_native = layers.iter().any(|&m| is_native_layer(g, m))
            || uses.iter().any(|(_, resolved)| match resolved {
                Resolved::Fact(_) => true,
                Resolved::Layer(name) => g.matcher(name).is_some_and(|m| is_native_layer(g, m)),
                Resolved::Group(name) => g.group(name).is_some_and(|members| members.iter().any(|member| match member {
                    Parent::Matcher(name) => g.matcher(&name.text).is_some_and(|m| is_native_layer(g, m)),
                    _ => false,
                })),
                _ => false,
            });
        Self { layers, helpers, all_tokens, own_tokens, requires_native }
    }

    fn extend(&mut self, other: Self) {
        self.layers.extend(other.layers);
        self.helpers.extend(other.helpers);
        self.all_tokens |= other.all_tokens;
        self.own_tokens |= other.own_tokens;
        self.requires_native |= other.requires_native;
    }

    fn resolve(self, helpers: &[Self]) -> ReadRequirements {
        let mut layers = self.layers;
        let mut all_tokens = self.all_tokens;
        let mut own_tokens = self.own_tokens;
        let mut requires_native = self.requires_native;
        let mut pending = self.helpers;
        let mut seen = vec![false; helpers.len()];
        // Recursive functions and cross-node attributes can refer back to themselves.
        while let Some(id) = pending.pop() {
            if std::mem::replace(&mut seen[id], true) {
                continue;
            }
            let dependency = &helpers[id];
            layers.extend(&dependency.layers);
            all_tokens |= dependency.all_tokens;
            own_tokens |= dependency.own_tokens;
            requires_native |= dependency.requires_native;
            pending.extend(&dependency.helpers);
        }
        ReadRequirements { layers: unique(layers), all_tokens, own_tokens, requires_native }
    }
}

pub fn build(loaded: Loaded, root: PathBuf) -> Built {
    compile(loaded, root, true)
}

/// Compiles enabled rules and reachable helpers without tooling symbols.
pub fn build_for_lint(loaded: Loaded, root: PathBuf) -> Built {
    compile(loaded, root, false)
}

fn compile(loaded: Loaded, root: PathBuf, tooling: bool) -> Built {
    let Loaded { mut config, sources, mut errors, fingerprint } = loaded;
    // `rule name ...` lines: which rules are on, and their extra clauses
    // (merged before compiling); option values are set after.
    let option_values = configure(&mut config, &mut errors);
    if tooling {
        super::prune::prune(&mut config);
    } else {
        super::prune::prune_for_lint(&mut config, &mut errors);
    }
    let schemas = schemas(&config, &mut errors);
    let grammar = Grammar::build(&config, &mut errors);
    let uses: Uses = RefCell::default();
    let attr_names = AttrNames::new(config.attrs.iter().map(|def| def.name.text.as_str()));
    let mut function_reads: Vec<_> = grammar.functions.iter().map(|_| ReadDependencies::default()).collect();
    for def in &config.functions {
        let Some(i) = grammar.function(&def.name.text) else { continue };
        let params = def.params.iter().map(|p| p.text.clone()).collect();
        // Parameters holding nodes read attributes too (`d.initialised`).
        let layer_reads = RefCell::default();
        let uses_before = uses.borrow().len();
        let scope = Scope { uses: Some(&uses), attrs: &attr_names, layer_reads: Some(&layer_reads), ..Scope::function_body(&grammar, &def.name.text, params) };
        if let Some(body) = scope.compile(&def.body, &mut errors) {
            let _ = grammar.functions[i].body.set(body);
        }
        let function_uses = uses.borrow();
        let function_uses = &function_uses[uses_before..];
        let (children, siblings) = reads_neighbours(function_uses);
        // A function's node parameters can hold tokens of any kind.
        function_reads[i] = ReadDependencies::collect(&grammar, &attr_names, function_uses, layer_reads.into_inner(), children || siblings);
    }
    for condition in &grammar.names.conditions {
        let Some(when) = &config.names[condition.item].when else { continue };
        let scope = Scope { fields: target_fields(&grammar, Target::Matcher(condition.matcher)), attrs: &attr_names, ..Scope::new(&grammar) };
        if let Some(e) = scope.compile(when, &mut errors) {
            let _ = condition.expr.set(e);
        }
    }
    for operand in &grammar.names.operands {
        let Some(source) = config.names.get(operand.item).and_then(|def| def.item.operand(operand.part)) else { continue };
        let scope = Scope { fields: target_fields(&grammar, Target::Matcher(operand.matcher)), attrs: &attr_names, ..Scope::new(&grammar) };
        if let Some(e) = scope.compile(source, &mut errors) {
            let _ = operand.expr.set(e);
        }
    }
    let (attrs, mut helper_reads) = attributes(&config, &grammar, &attr_names, &uses, &mut errors);
    helper_reads.extend(function_reads);
    let forwards = forwards(&config, &grammar, &attr_names, &mut errors);
    for &(matcher, attr, _) in &forwards {
        helper_reads[attr as usize].requires_native |= is_native_layer(&grammar, matcher);
    }
    let language_options = super::language_options::LanguageOptions::compile(&config, &grammar, &schemas, &mut errors);

    let mut rules: Vec<Rule> = Vec::new();
    let mut inert = Vec::new();
    let as_inert = |def: &RuleDef| {
        let targets: Vec<String> = def
            .targets
            .iter()
            .map(|t| match t {
                AstTarget::Matcher(n) => n.text.clone(),
                AstTarget::Field(n, f) => format!("{}.{}", n.text, f.text),
                AstTarget::Lang(n) => format!(".{}", n.text),
            })
            .collect();
        InertRule { name: def.name.text.clone(), span: def.name.span, target_text: targets.join(", "), default_severity: def.severity.unwrap_or(Severity::Warn) }
    };
    for def in &config.rules {
        if def.inert {
            inert.push(as_inert(def));
            continue;
        }
        // An off library rule that does not compile (the config redefined
        // a layer it reads) is the library's business until it is turned on.
        if def.defined_only && !def.enabled() {
            let mut quiet = Vec::new();
            match rule(def, &grammar, &schemas, &attr_names, &uses, &helper_reads, &mut quiet) {
                Some(r) if quiet.is_empty() && !def.broken => rules.push(r),
                _ => inert.push(as_inert(def)),
            }
            continue;
        }
        let before = errors.len();
        if let Some(r) = rule(def, &grammar, &schemas, &attr_names, &uses, &helper_reads, &mut errors).filter(|_| !def.broken) {
            rules.push(r);
        } else if def.defined_only && !def.broken && errors.len() == before {
            inert.push(as_inert(def));
        }
    }
    // Option values, in order; a rule that failed to build was reported.
    for (rule_name, config_index, option_index) in &option_values {
        let (name, value) = &config.configs[*config_index].options[*option_index];
        let Some(f) = rules.iter_mut().find(|r| r.name == *rule_name) else {
            // Off bodies are not compiled at runtime, but their explicit
            // settings still have to fit the declared option schemas.
            let dormant = inert.iter().any(|r| r.name == *rule_name);
            if !tooling || dormant {
                if let Some(def) = config.rules.iter().find(|r| r.name.text == *rule_name && (!r.enabled() || dormant)) {
                    if let Some(option) = def.options.iter().find(|o| o.name.text == name.text) {
                        option_value(rule_name, name, value, &option.schema, &grammar, &schemas, &mut errors);
                    }
                }
            }
            continue;
        };
        let Some(slot) = f.options.iter_mut().find(|o| o.name == name.text) else {
            let mut err = ConfigError::new(name.span, format!("no option `{}` on `{}`", name.text, rule_name))
                .suggest(&name.text, f.options.iter().map(|o| o.name.as_str()));
            if f.options.is_empty() {
                err = err.note("this rule has no options");
            }
            errors.push(err);
            continue;
        };
        if let Some(value) = option_value(rule_name, name, value, &slot.schema, &grammar, &schemas, &mut errors) {
            slot.value = value;
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
    let symbols = tooling.then(|| std::sync::Arc::new(Symbols::build(&config, &grammar, &rules, &inert, uses.into_inner(), sources.clone())));
    for rule in &mut rules {
        rule.specialise_options(&grammar.functions);
    }
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
    let mut by_field: crate::fx_hash::FxHashMap<(u32, u16), Vec<usize>> = Default::default();
    let mut by_link: crate::fx_hash::FxHashMap<u32, Vec<(usize, Vec<usize>)>> = Default::default();
    let mut by_lang = Vec::new();
    for (i, f) in rules.iter().enumerate().filter(|(_, f)| !f.never_reports()) {
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
                Target::Link(m, link) => {
                    for x in 0..grammar.matchers.len() {
                        if !grammar.is_a(x as u32, m) { continue; }
                        let links = by_link.entry(x as u32).or_default();
                        match links.iter_mut().find(|(l, _)| *l == link) {
                            Some((_, rules)) if !rules.contains(&i) => rules.push(i),
                            Some(_) => {}
                            None => links.push((link, vec![i])),
                        }
                    }
                }
                Target::Lang(lang) => by_lang.push((i, lang)),
            }
        }
    }
    let attrs = Attributes::new(attrs, &forwards, &grammar);
    let mut linter = Linter {
        projects: super::frontend::Projects::new(&grammar.natives),
        reach: grammar.reach(),
        profile: None,
        split_at: 128 * 1024,
        rule_jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
        grammar,
        language_options,
        rules,
        inert,
        sources: sources.clone(),
        root,
        ignores,
        by_matcher,
        by_field,
        by_link,
        by_lang,
        attrs,
        fingerprint,
    };
    errors.extend(linter.lower_conditions(&config));
    Built { linter: Some(linter), errors, sources, symbols }
}

pub(super) fn option_value(
    rule: &str,
    name: &Name,
    value: &Expr,
    schema: &Schema,
    grammar: &Grammar,
    schemas: &Schemas,
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
            let compiled = Scope::constants(grammar).compile(value, errors)?;
            Ctx::eval_static(&compiled, &[]).map(|v| v.into_owned())
        }
    };
    match evaluated {
        Ok(v) => match schema.check(&v, schemas) {
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

#[cfg(test)]
mod startup_tests {
    use std::path::Path;

    use crate::config::{self, source::Disk};

    use super::*;

    #[test]
    fn all_std_rules_compile_at_runtime() {
        let source = "import .rs\nimport .ts\nimport .py\nimport .c\nimport .zlint\nimport .md\nimport .toml\nrule *\n";
        let built = build_for_lint(config::load_text("test.zlint", source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
    }

    #[test]
    fn open_library_groups_can_wait_for_language_adapters() {
        for compile in [build, build_for_lint] {
            for adapters in [
                "",
                "in .t match item /x/\ngroup items item\n",
                "in .t match item /x/\ngroup items item\ndrop group items item\n",
            ] {
                let source = format!("lang t \"*.t\"\ndefine {{\ngroup items\nin items rule item-rule warn 'item'\n}}\n{adapters}rule *\n");
                let built = compile(config::load_text("test.zlint", &source, &Disk), PathBuf::from("/root"));
                assert!(built.errors.is_empty(), "{:#?}", built.errors);
            }
            let source = "lang t \"*.t\"\ngroup items\nin items rule item-rule warn 'item'\n";
            let built = compile(config::load_text("test.zlint", source, &Disk), PathBuf::from("/root"));
            assert!(!built.errors.is_empty());
            let source = "lang t \"*.t\"\ndefine {\ngroup items\nin items rule item-rule warn 'item' { option max int = 1 }\n}\nrule item-rule { set max 'bad' }\n";
            let built = compile(config::load_text("test.zlint", source, &Disk), PathBuf::from("/root"));
            assert!(!built.errors.is_empty());
        }
    }

    #[test]
    fn runtime_and_tooling_preserve_helpers_added_by_rule_configuration() {
        let source = r#"
lang t "*.t"
define {
  in .t match leaf /X/
  in .t match wrapper /</ value:leaf />/
  group words leaf
  in words let letter upper(text)
  in wrapper forward letter from value
  fn read(n) = n.letter
  fn accepted(n) = read(n) == "X"
  in wrapper rule active warn "unused"
}
rule active warn "{read(node)}" fix as lower(text) { when accepted(node) }
"#;
        let root = PathBuf::from("/root");
        let runtime = build_for_lint(config::load_text("test.zlint", source, &Disk), root.clone());
        let tooling = build(config::load_text("test.zlint", source, &Disk), root);
        assert!(runtime.errors.is_empty(), "{:#?}", runtime.errors);
        assert!(tooling.errors.is_empty(), "{:#?}", tooling.errors);
        let runtime = runtime.linter.unwrap();
        let tooling = tooling.linter.unwrap();
        let path = Path::new("/root/test.t");
        let summaries = |linter: &Linter| linter.lint(path, "<X>").into_iter().map(|d| (d.rule, d.severity, d.message, d.start, d.end, d.fix)).collect::<Vec<_>>();
        assert_eq!(summaries(&runtime), summaries(&tooling));
        assert_eq!(runtime.fix(path, "<X>").text, tooling.fix(path, "<X>").text);
        assert_eq!(runtime.fix(path, "<X>").text, "<x>");
    }
}

/// Applies `rule name ...` lines to the definitions, in order: turning
/// rules on or off, replacing messages and fixes, adding clauses. Returns
/// the option values to set once the rules are compiled, as (rule, config
/// index, option index).
fn configure(config: &mut crate::config::ast::Config, errors: &mut Vec<ConfigError>) -> Vec<(String, usize, usize)> {
    let mut values = Vec::new();
    let mut configs = std::mem::take(&mut config.configs);
    for (ci, c) in configs.iter_mut().enumerate() {
        let pattern = &c.patch.name;
        let mut matching: Vec<usize> = (0..config.rules.len()).filter(|&i| wildcard(&pattern.text, &config.rules[i].name.text)).collect();
        // A family name also configures its variants (`comma-spacing:before`,
        // `:after`), with or without a rule of the plain name.
        if !pattern.text.contains(['*', ':']) {
            let family = format!("{}:", pattern.text);
            matching.extend((0..config.rules.len()).filter(|&i| config.rules[i].name.text.starts_with(&family)));
        }
        if matching.is_empty() {
            if c.soft {
                continue;
            }
            if let Some(d) = Dropped::find(&config.dropped, &pattern.text) {
                errors.push(d.still_used(pattern.span));
                continue;
            }
            let mut err = ConfigError::new(pattern.span, format!("no rule `{}`", pattern.text))
                .suggest(&pattern.text, config.rules.iter().map(|r| r.name.text.as_str()));
            if let Some(lang) = crate::config::std_lib::defining_language(&pattern.text) {
                err = err.note(format!("`{}` comes with `import .{}`", pattern.text, crate::config::std_lib::short_name(lang)));
            }
            errors.push(err);
            continue;
        }
        // `rule name set <value>`: each matching rule's default option.
        if let Some(value) = c.value.take() {
            let mut names: Vec<String> = Vec::new();
            for &i in &matching {
                match config.rules[i].options.iter().find(|o| o.primary) {
                    Some(o) if !names.contains(&o.name.text) => names.push(o.name.text.clone()),
                    Some(_) => {}
                    None if c.soft => {}
                    None => errors.push(
                        ConfigError::new(value.span, format!("`{}` has no default option to set", config.rules[i].name.text))
                            .note("set a named option instead: `rule name { set option value }`"),
                    ),
                }
            }
            for name in names {
                c.options.push((Name { text: name, span: value.span }, value.clone()));
            }
        }
        let patch = &c.patch;
        for (oi, (name, _)) in c.options.iter().enumerate() {
            let owners: Vec<usize> = matching.iter().copied().filter(|&i| config.rules[i].options.iter().any(|o| o.name.text == name.text)).collect();
            if owners.is_empty() {
                let names = matching.iter().flat_map(|&i| config.rules[i].options.iter().map(|o| o.name.text.as_str()));
                errors.push(ConfigError::new(name.span, format!("no option `{}` on `{}`", name.text, pattern.text)).suggest(&name.text, names));
            }
            values.extend(owners.into_iter().map(|i| (config.rules[i].name.text.clone(), ci, oi)));
        }
        for &i in &matching {
            let def = &mut config.rules[i];
            match patch.severity {
                Some(s) => def.configured = Some(s),
                // `rule x`: on at its default, unless it already is on.
                None if !def.enabled() => {
                    // Keep library provenance: its adapter group may still be empty.
                    def.configured = Some(def.severity.unwrap_or(Severity::Warn));
                }
                None => {}
            }
            if let Some(m) = &patch.message {
                def.message = Some(m.clone());
            }
            if let Some(f) = &patch.fix {
                def.fix = Some(f.clone());
                def.fix_target = patch.fix_target;
                def.fix_at = patch.fix_at.clone();
                def.fix_when = patch.fix_when.clone();
            }
            if patch.report.is_some() {
                def.report = patch.report;
                def.report_at = None;
            }
            if let Some(at) = &patch.report_at {
                def.report_at = Some(at.clone());
            }
            def.when.extend(patch.when.iter().cloned());
            def.locals.extend(patch.locals.iter().cloned());
            def.files.extend(patch.files.iter().cloned());
            def.ignore.extend(patch.ignore.iter().cloned());
            def.indents.extend(patch.indents.iter().cloned());
            if patch.indent_reset.is_some() {
                def.indent_reset = patch.indent_reset.clone();
            }
            def.broken |= patch.broken;
        }
    }
    config.configs = configs;
    values
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
        Target::Link(_, _) | Target::Lang(_) => Vec::new(),
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
/// Attribute evaluation and the parsing prerequisites of each attribute.
fn attributes(
    config: &crate::config::ast::Config,
    g: &Grammar,
    names: &AttrNames,
    uses: &Uses,
    errors: &mut Vec<ConfigError>,
) -> (Vec<Attr>, Vec<ReadDependencies>) {
    let mut dependencies: Vec<_> = names.iter().map(|_| ReadDependencies::default()).collect();
    let mut attrs: Vec<Attr> = names.iter().map(|n| Attr { name: n.clone(), defs: Vec::new() }).collect();
    // Per attribute: layers whose definition came through a group.
    let mut group_defined: Vec<Vec<u32>> = names.iter().map(|_| Vec::new()).collect();
    for def in &config.attrs {
        if crate::expr::is_builtin(&def.name.text) {
            errors.push(ConfigError::new(def.name.span, format!("`{}` is a built-in variable and cannot name an attribute", def.name.text)));
            continue;
        }
        let id = usize::from(names.get(&def.name.text).expect("collected"));
        for target in &def.targets {
            let Some((resolved, _)) = resolve_target(&AstTarget::Matcher(target.clone()), g, errors) else { continue };
            // Through a group: a default its members' own definitions refine.
            let through_group = !(resolved.len() == 1 && matches!(resolved[0], Target::Matcher(m) if g.matcher(&target.text) == Some(m)));
            for t in resolved {
                let Target::Matcher(m) = t else {
                    errors.push(ConfigError::new(target.span, "attributes belong to layers, not languages"));
                    continue;
                };
                match attrs[id].defs.iter().position(|(d, _)| *d == m) {
                    // The member's own definition is already there.
                    Some(_) if through_group && !group_defined[id].contains(&m) => continue,
                    // A group's: the member's own replaces it.
                    Some(i) if !through_group && group_defined[id].contains(&m) => {
                        attrs[id].defs.remove(i);
                        group_defined[id].retain(|&x| x != m);
                    }
                    Some(_) => {
                        errors.push(
                            ConfigError::new(
                                def.name.span,
                                format!("attribute `{}` is defined twice for `{}`", def.name.text, g.matchers[m as usize].name),
                            )
                            .note(format!("`override in {} let {} ...` replaces it", g.matchers[m as usize].name, def.name.text)),
                        );
                        continue;
                    }
                    None => {}
                }
                if through_group {
                    group_defined[id].push(m);
                }
                let layer_reads = RefCell::default();
                let scope = Scope { fields: target_fields(g, t), attrs: names, uses: Some(uses), layer_reads: Some(&layer_reads), ..Scope::new(g) };
                let uses_before = uses.borrow().len();
                if let Some(e) = scope.compile(&def.expr, errors) {
                    attrs[id].defs.push((m, e));
                }
                let attribute_uses = uses.borrow();
                let attribute_uses = &attribute_uses[uses_before..];
                let (children, siblings) = reads_neighbours(attribute_uses);
                dependencies[id].extend(ReadDependencies::collect(g, names, attribute_uses, layer_reads.into_inner(), children || is_token_layer(g, m) && siblings));
                dependencies[id].requires_native |= is_native_layer(g, m);
            }
        }
    }
    // A node reads the first definition whose layer it is: the most specific
    // first (a member before its group, a layer before its base), so
    // `in ts_node let x ...` gives a default that `in ts_binary let x ...`
    // refines. Otherwise definition order.
    for attr in &mut attrs {
        let targets: Vec<u32> = attr.defs.iter().map(|(m, _)| *m).collect();
        let broader = |m: u32| targets.iter().filter(|&&t| t != m && g.is_a(m, t)).count();
        attr.defs.sort_by_key(|(m, _)| std::cmp::Reverse(broader(*m)));
    }
    (attrs, dependencies)
}

fn unique(mut ids: Vec<u32>) -> Vec<u32> {
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn is_token_layer(g: &Grammar, m: u32) -> bool {
    g.native_kind(m).is_some_and(|k| k.is_token())
}

fn is_native_layer(g: &Grammar, m: u32) -> bool {
    let native = |m: u32| g.native_kind(m).is_some() || !g.native_hosts[m as usize].is_empty();
    // A base also matches its native-hosted specialisations.
    native(m) || g.matchers.iter().enumerate().any(|(id, _)| native(id as u32) && g.is_a(id as u32, m))
}

/// Whether these reads include `children.x`, and `previous.x` / `next.x`.
fn reads_neighbours(uses: &[(crate::config::source::Span, crate::expr::Resolved)]) -> (bool, bool) {
    let read = |names: &[&str]| uses.iter().any(|(_, r)| matches!(r, crate::expr::Resolved::Relative(n) if names.contains(n)));
    (read(&["children"]), read(&["previous", "next"]))
}

/// Named option schemas, after checking every schema name used refers to
/// one.
fn schemas(config: &Config, errors: &mut Vec<ConfigError>) -> Schemas {
    let named: Schemas = config.schemas.iter().map(|s| (s.name.text.clone(), s.clone())).collect();
    for u in &config.uses {
        if let Use::Schema(name) = u {
            if !named.contains_key(&name.text) {
                errors.push(
                    ConfigError::new(name.span, format!("unknown type `{}`", name.text))
                        .suggest(&name.text, named.keys().map(String::as_str).chain(["string", "number", "int", "boolean", "any", "null", "regex"])),
                );
            }
        }
    }
    named
}

fn rule(def: &RuleDef, g: &Grammar, schemas: &Schemas, attrs: &AttrNames, uses: &Uses, helpers: &[ReadDependencies], errors: &mut Vec<ConfigError>) -> Option<Rule> {
    let mut targets = Vec::new();
    let mut texts = Vec::new();
    for t in &def.targets {
        let (resolved, text) = resolve_target(t, g, errors)?;
        targets.extend(resolved);
        texts.push(text);
    }
    let target_text = texts.join(", ");
    if targets.is_empty() {
        if !def.defined_only {
            errors.push(ConfigError::new(def.name.span, format!("rule `{}` has nothing to check: its group is empty", def.name.text)));
        }
        return None;
    }

    let before = errors.len();
    let mut options: Vec<OptionSlot> = Vec::new();
    for opt in &def.options {
        if options.iter().any(|o| o.name == opt.name.text) {
            errors.push(ConfigError::new(opt.name.span, format!("option `{}` is declared twice", opt.name.text)));
            continue;
        }
        match opt.schema.check(&opt.default, schemas) {
            Ok(v) => options.push(OptionSlot {
                name: opt.name.text.clone(),
                description: opt.description.clone(),
                schema: opt.schema.clone(),
                default: v.clone(),
                value: v,
                primary: opt.primary,
            }),
            Err(misfit) => errors.push(ConfigError::new(opt.default_span, format!("the default of option `{}` does not fit its type: {misfit}", opt.name.text))),
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
    let layer_reads: RefCell<Vec<u32>> = RefCell::default();
    let item_read = std::cell::Cell::new(false);
    let mut scope = Scope { fields, options: option_names, attrs, rule: Some(&def.name.text), uses: Some(uses), layer_reads: Some(&layer_reads), ..Scope::new(g) };
    let uses_before = uses.borrow().len();
    // Each `let` sees the ones before it.
    let mut locals = Vec::new();
    for (name, e) in &def.locals {
        // A `let` reads itself only on other nodes: `previous.depth + 1`.
        scope.defining = Some(name.text.clone());
        locals.push(scope.compile(e, errors).unwrap_or(CExpr::Const(Value::Null)));
        scope.defining = None;
        scope.locals.push(name.text.clone());
    }

    // `each x in list`: the list is per node; `x` is in scope for the rest
    // (not for `let`s, which are kept per node).
    // `@each`: the list may read the rule's options and nothing of a node.
    let each = def.each.as_ref().and_then(|(_, list)| {
        if def.each_options {
            Scope { options: scope.options.clone(), uses: Some(uses), ..Scope::constants(g) }.compile(list, errors)
        } else {
            scope.compile(list, errors)
        }
    });
    // `when`s not reading the item are checked once, before the list is built.
    if let Some((name, _)) = &def.each {
        if let Some(kind) = scope.item_name_collision(&name.text) {
            errors.push(ConfigError::new(name.span, format!("each item `{}` has the same name as an existing {kind}", name.text))
                .note("choose another item name"));
        }
        scope.param_read = Some((scope.params.len() as u16, &item_read));
        scope.params.push(name.text.clone());
    }
    let (mut node_when, mut when) = (Vec::new(), Vec::new());
    for e in &def.when {
        item_read.set(false);
        if let Some(c) = scope.compile(e, errors) {
            if def.each.is_none() || item_read.get() { when.push(c) } else { node_when.push(c) }
        }
    }
    scope.param_read = None;
    let message = def.message.as_ref().and_then(|m| scope.compile(m, errors));
    let fix = def.fix.as_ref().and_then(|s| scope.compile(s, errors));
    let fix_when = def.fix_when.as_ref().and_then(|s| scope.compile(s, errors));
    let fix_at = def.fix_at.as_ref().and_then(|s| scope.compile(s, errors));
    let report_at = def.report_at.as_ref().and_then(|s| scope.compile(s, errors));
    // `indent X by n`: `n` reads X (its fields when X names only layers) and
    // the rule's options; X's layers must be built.
    // Indent classes by name, numbered within the rule; 0 is the unnamed one.
    let mut classes: Vec<&str> = Vec::new();
    let indents: Vec<crate::expr::IndentRule> = def
        .indents
        .iter()
        .map(|decl| {
            let class = decl.class.as_deref().map_or(0, |name| match classes.iter().position(|c| *c == name) {
                Some(i) => i as u32 + 1,
                None => {
                    classes.push(name);
                    classes.len() as u32
                }
            });
            let targets = g.indent_targets(&decl.targets, errors);
            layer_reads.borrow_mut().extend(targets.iter().map(|&(m, _)| m));
            // `by` and `from` read X: its fields when X names only layers.
            let mut fields = None::<Vec<_>>;
            for &(m, field) in &targets {
                let these = if field.is_some() { Vec::new() } else { target_fields(g, Target::Matcher(m)) };
                fields = Some(match fields {
                    None => these,
                    Some(mut f) => {
                        f.retain(|x| these.contains(x));
                        f
                    }
                });
            }
            let on_target = Scope {
                fields: fields.unwrap_or_default(),
                options: scope.options.clone(),
                attrs,
                uses: Some(uses),
                layer_reads: Some(&layer_reads),
                ..Scope::new(g)
            };
            let by = decl.by.as_ref().and_then(|e| on_target.compile(e, errors));
            let from = decl.from.as_ref().and_then(|e| on_target.compile(e, errors));
            crate::expr::IndentRule { inside: decl.inside, dedent: decl.dedent, targets, by, from, class }
        })
        .collect();

    let reset_scope = Scope {
        options: scope.options.clone(),
        attrs,
        uses: Some(uses),
        layer_reads: Some(&layer_reads),
        ..Scope::new(g)
    };
    let indent_reset = def.indent_reset.as_ref().and_then(|e| reset_scope.compile(e, errors));

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
    // Reading brackets or descendants of token and trivia layers needs
    // those nodes.
    let rule_uses = uses.borrow();
    let rule_uses = &rule_uses[uses_before..];
    let (reads_children, reads_siblings) = reads_neighbours(rule_uses);
    let ReadRequirements { layers: layer_reads, all_tokens: helper_tokens, own_tokens, requires_native } =
        ReadDependencies::collect(g, attrs, rule_uses, layer_reads.into_inner(), false).resolve(helpers);
    let requires_native = requires_native || targets.iter().any(|target| match target {
        Target::Matcher(m) | Target::Field(m, _) => is_native_layer(g, *m),
        Target::Link(_, _) => true,
        Target::Lang(_) => false,
    });
    let needs_tokens = |m| is_token_layer(g, m) || g.native_hosts[m as usize].iter().any(|&host| is_token_layer(g, host));
    let on_tokens = layer_reads.iter().any(|&m| needs_tokens(m)) || targets.iter().any(|target| match target {
        Target::Matcher(m) | Target::Field(m, _) | Target::Link(m, _) => needs_tokens(*m),
        Target::Lang(_) => false,
    });
    let target_layers = targets.iter().map(|t| match t { Target::Matcher(m) => Some(*m), _ => None }).collect::<Option<Vec<u32>>>().unwrap_or_default();
    // `ts_member.token` (an `indent` or rule target): nodes' own tokens.
    let token_label = g.labels.get("token");
    let targets_tokens = token_label.is_some()
        && (indents.iter().flat_map(|i| &i.targets).any(|t| t.1 == token_label) || targets.iter().any(|t| matches!(t, Target::Field(_, l) if Some(*l) == token_label)));
    Some(Rule {
        layer_reads,
        requires_native,
        all_tokens: reads_children || helper_tokens || on_tokens && reads_siblings,
        token_fields: targets_tokens || own_tokens,
        target_layers,
        name: def.name.text.clone(),
        span: def.name.span,
        targets,
        target_text,
        severity: match def.configured {
            Some(s) => s,
            None if def.defined_only => Severity::Off,
            None => def.severity.unwrap_or(Severity::Warn),
        },
        default_severity: def.severity.unwrap_or(Severity::Warn),
        options,
        files,
        ignore,
        node_when,
        when,
        node_when_vm: Vec::new(),
        when_vm: Vec::new(),
        each_vm: None,
        fix_when_vm: None,
        fix_vm: None,
        fix_at_vm: None,
        report_at_vm: None,
        locals_vm: Vec::new(),
        locals,
        message,
        message_file: def.message.as_ref().map_or(def.name.span.file, |message| message.span.file),
        message_plan: None,
        message_vm: None,
        fix,
        fix_when,
        fix_at,
        fix_target: def.fix_target,
        indents: crate::expr::IndentPlan::new(indents, g),
        indent_reset,
        report: def.report.unwrap_or_default(),
        report_at,
        each,
    })
}

/// `in a forward x from field` as (matcher, attribute, label); groups give
/// one per member.
fn forwards(config: &crate::config::ast::Config, g: &Grammar, attr_names: &AttrNames, errors: &mut Vec<ConfigError>) -> Vec<(u32, u16, u16)> {
    let mut out: Vec<(u32, u16, u16)> = Vec::new();
    for def in &config.forwards {
        let mut attrs = Vec::new();
        for a in &def.attrs {
            match attr_names.get(&a.text) {
                Some(i) => attrs.push((i, a)),
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
            let label = g.labels.get(&field.text).filter(|_| labels.contains(&field.text));
            match (label, crate::expr::facts::link(&field.text)) {
                (Some(label), _) => (vec![Target::Field(m, label)], format!("{}.{}", layer.text, field.text)),
                (None, Some(link)) => (vec![Target::Link(m, link)], format!("{}.{}", layer.text, field.text)),
                (None, None) => {
                    let names = labels.iter().map(String::as_str).chain(crate::expr::facts::LINKS.iter().map(|link| link.name));
                    errors.push(
                        ConfigError::new(field.span, format!("matcher `{}` has no field `{}` or link of that name", layer.text, field.text))
                            .suggest(&field.text, names),
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

#[cfg(test)]
mod demand_tests {
    use super::*;
    use crate::config::source::Disk;
    use std::path::Path;

    fn native(rules: &str) -> Linter {
        let source = format!("lang js \"*.ts\"\nnative ts\ngroup opener ts_open_paren, ts_open_bracket, ts_open_brace, ts_open_angle\n{rules}");
        let built = build(crate::config::load_text("test.zlint", &source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
        built.linter.expect("valid config")
    }

    #[test]
    fn each_items_cannot_hide_names_visible_in_the_rule() {
        let mut failures = Vec::new();
        for (target, prefix, item, list, reject) in [
            ("ts_function", "", "name", "[1]", true),
            ("ts_program", "", "declaration", "[1]", true),
            ("ts_program", "", "global", "[1]", true),
            ("ts_program", "", "text", "[1]", true),
            ("ts_program", "in ts_program let item [1]\n", "item", "item", true),
            ("ts_program", "", "item", "[1]", false),
            ("ts_program", "", "name", "[1]", false),
        ] {
            let source = format!("lang js \"*.ts\"\nnative ts\n{prefix}in {target} rule probe {{\neach {item} in {list}\nwarn \"{{{item}}}\"\n}}");
            let built = build(crate::config::load_text("each.zlint", &source, &Disk), PathBuf::from("/root"));
            let item_start = source.find("each ").unwrap() + 5;
            let collision = built.errors.len() == 1 && built.errors[0].span.is_some_and(|span|
                span.start as usize == item_start && span.end as usize == item_start + item.len());
            if collision != reject || !reject && !built.errors.is_empty() {
                failures.push((source, reject, built.errors));
            }
        }
        for setup in ["let item 1", "option item number = 1 \"item\""] {
            let source = format!("lang js \"*.ts\"\nnative ts\nin ts_program rule probe {{\n{setup}\neach item in [1]\nwarn \"{{item}}\"\n}}");
            let built = build(crate::config::load_text("each.zlint", &source, &Disk), PathBuf::from("/root"));
            let item_start = source.find("each ").unwrap() + 5;
            if built.errors.len() != 1 || !built.errors[0].span.is_some_and(|span|
                span.start as usize == item_start && span.end as usize == item_start + 4) {
                failures.push((source, true, built.errors));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn linked_targets_keep_selected_context_and_run_once_per_node() {
        let mut linter = native(r#"
            in ts_identifier let checked text == "value" || text == "later"
            in ts_declarator.name, ts_identifier.declaration, ts_identifier.declarations rule linked info checked {
                option replacement string = "renamed"
                let start offset
                report at start
                fix as replacement
            }
        "#);
        linter.enable_profile();
        let source = "let value = 1; value; value; missing; let later = 2; later;";
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        let starts: Vec<_> = ["value", "later"].map(|name| source.find(name).expect("declaration")).into();
        assert_eq!(diagnostics.iter().map(|d| d.start).collect::<Vec<_>>(), starts);
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.message, "true");
            assert_eq!(diagnostic.start, diagnostic.end);
            let name = if diagnostic.start == starts[0] { "value" } else { "later" };
            assert!(source[diagnostic.start..].starts_with(name));
            assert_eq!(diagnostic.fix.as_ref().map(|fix| fix.edits.as_slice()), Some(&[super::super::Edit {
                start: diagnostic.start, end: diagnostic.start + name.len(), text: "renamed".into(),
            }][..]));
        }
        assert_eq!(super::super::Profile::get(&linter.profile.as_ref().expect("profile").rule_evals[0]), starts.len() as u64);
        assert!(linter.lint(Path::new("/root/empty.ts"), "").is_empty());
        assert!(linter.lint(Path::new("/root/file.t"), source).is_empty());

        let fields = native("in ts_call.callee rule field info text");
        let source = "call(value);";
        let diagnostics = fields.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(&source[diagnostics[0].start..diagnostics[0].end], "call");

        let built = build(crate::config::load_text("field.zlint",
            "lang plain \"*.t\"\nin .plain match holder /@/ declaration:/[a-z]+/\nin holder.declaration rule field info text", &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:?}", built.errors);
        let diagnostics = built.linter.expect("field config").lint(Path::new("/root/file.t"), "@value");
        assert_eq!(diagnostics.len(), 1, "real fields win over registered links without a native front end");
        assert_eq!(diagnostics[0].message, "value");
    }

    #[test]
    fn default_option_configuration_keeps_severity_and_rejects_invalid_words() {
        let rule = "define {\nin ts_call rule probe info \"{mode}\" {\n  default option mode enum(always, never, always-multiline) = never \"policy\"\n}\n}\n";
        use crate::config::ast::Severity::{Error, Info};
        for (config, expected) in [
            ("rule probe", Info),
            ("rule probe set always", Info),
            ("rule probe error set always-multiline", Error),
            ("rule probe set always error", Error),
            ("rule probe error \"custom\" set always", Error),
            ("rule probe { set mode always }", Info),
            ("rule probe { set mode \"always\" }", Info),
        ] {
            let linter = native(&format!("{rule}{config}"));
            assert_eq!(linter.rules.iter().find(|rule| rule.name == "probe").unwrap().severity, expected, "{config}");
        }
        for (config, problem) in [
            ("rule probe set sometimes", "must be enum(always, never, always-multiline)"),
            ("rule probe set always set never", "already sets its default option"),
        ] {
            let built = build(crate::config::load_text("test.zlint", &format!("lang js \"*.ts\"\nnative ts\n{rule}{config}\n"), &Disk), PathBuf::from("/root"));
            assert!(built.errors.iter().any(|e| e.message.contains(problem)), "{config}: {:#?}", built.errors);
        }
        let without = "define {\nin ts_call rule plain warn \"x\" { option mode enum(a, b) = a }\n}\nrule plain set b\n";
        let built = build(crate::config::load_text("test.zlint", &format!("lang js \"*.ts\"\nnative ts\n{without}"), &Disk), PathBuf::from("/root"));
        assert!(built.errors.iter().any(|e| e.message.contains("has no default option")), "{:#?}", built.errors);
        let severity_word = "define {\nin ts_call rule bad warn \"x\" { default option mode enum(off, on) = on }\n}\n";
        let built = build(crate::config::load_text("test.zlint", &format!("lang js \"*.ts\"\nnative ts\n{severity_word}"), &Disk), PathBuf::from("/root"));
        assert!(built.errors.iter().any(|e| e.message.contains("cannot take `off`")), "{:#?}", built.errors);
    }

    #[test]
    fn whens_not_reading_the_each_item_skip_building_the_list() {
        // The list fails to evaluate: only nodes passing the item-free
        // `when` ever build it.
        let rule = |guard: &str| format!("in ts_call rule probe warn \"{{x}}\" {{\n  each x in 1 - \"a\"\n  when {guard}\n  when x != null\n}}\nrule probe\n");
        let messages = |guard: &str| native(&rule(guard)).lint(Path::new("/root/file.ts"), "f();").into_iter().map(|d| d.message).collect::<Vec<_>>();
        assert_eq!(messages("false"), Vec::<String>::new());
        assert!(messages("true").iter().any(|m| m.contains("failed here")), "{:?}", messages("true"));
    }

    #[test]
    fn overrides_reject_missing_attribute_definitions() {
        let source = "lang js \"*.ts\"\nnative ts\nin ts_call let demo 1\noverride in ts_binary let missing 2\n";
        let built = build(crate::config::load_text("test.zlint", source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.iter().any(|e| e.message.contains("no attribute `missing`")), "{:#?}", built.errors);
    }
}
