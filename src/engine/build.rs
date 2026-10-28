//! Compiles a parsed configuration into a [`Linter`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use super::scopes::{Change, Scoped, Scopes, Setting};
use super::symbols::Symbols;
use super::fix::{Clause, FixPlan, PartPlan};
use super::{InertRule, Linter, NodeSelection, OptionSlot, Rule, Target};
use crate::config::ast::{Config, Dropped, Expr, ExprKind, FixDef, Name, Parent, RuleDef, Severity, Target as AstTarget, Use};
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
    /// Wall time of each build phase, in order (shown by `--stats`).
    pub phases: Vec<(&'static str, std::time::Duration)>,
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
    /// Field labels read (`x.name`): semantic-only units must build what
    /// they can hold.
    fields: Vec<u16>,
}

struct ReadRequirements {
    layers: Vec<u32>,
    all_tokens: bool,
    own_tokens: bool,
    requires_native: bool,
    /// Helpers (attribute ids, then function ids after them) whose own
    /// reads asked for tokens.
    token_helpers: Vec<usize>,
    fields: Vec<u16>,
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
        let fields = uses.iter().filter_map(|(_, resolved)| match resolved {
            Resolved::Field(name) => g.labels.get(name),
            _ => None,
        }).collect();
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
        Self { layers, helpers, all_tokens, own_tokens, requires_native, fields }
    }

    fn extend(&mut self, other: Self) {
        self.layers.extend(other.layers);
        self.helpers.extend(other.helpers);
        self.fields.extend(other.fields);
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
        let mut token_helpers = Vec::new();
        let mut fields = self.fields;
        // Recursive functions and cross-node attributes can refer back to themselves.
        while let Some(id) = pending.pop() {
            if std::mem::replace(&mut seen[id], true) {
                continue;
            }
            let dependency = &helpers[id];
            layers.extend(&dependency.layers);
            fields.extend(&dependency.fields);
            all_tokens |= dependency.all_tokens;
            own_tokens |= dependency.own_tokens;
            if dependency.all_tokens || dependency.own_tokens { token_helpers.push(id) }
            requires_native |= dependency.requires_native;
            pending.extend(&dependency.helpers);
        }
        fields.sort_unstable();
        fields.dedup();
        ReadRequirements { layers: unique(layers), all_tokens, own_tokens, requires_native, token_helpers, fields }
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
    let mut phases = Vec::new();
    let mut clock = std::time::Instant::now();
    let mut phase = |name: &'static str| {
        let now = std::time::Instant::now();
        phases.push((name, now - clock));
        clock = now;
    };
    // `rule name ...` lines: which rules are on, and their extra clauses
    // (merged before compiling); option values are set after.
    let settings = configure(&mut config, &mut errors);
    if tooling {
        super::prune::prune(&mut config);
    } else {
        super::prune::prune_for_lint(&mut config, &mut errors);
    }
    phase("prune");
    let schemas = schemas(&config, &mut errors);
    let grammar = Grammar::build(&config, &mut errors);
    phase("grammar");
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
    let mut name_reads = ReadDependencies::default();
    for def in &config.names {
        // `runs nothing` on a token says only how such a node executes when
        // it exists: it asks for no node to be built.
        let inert = def.when.is_none() && matches!(&def.item,
            crate::config::ast::NameItem::Execution(crate::config::ast::effects::Item::Runs(crate::config::ast::effects::Flow::Nothing)));
        for name in &def.targets {
            if inert && grammar.matcher(&name.text).and_then(|m| grammar.native_kind(m)).is_some_and(|kind| kind.is_token()) { continue }
            if let Some((targets, _)) = resolve_target(&AstTarget::Matcher(name.clone()), &grammar, None, &mut Vec::new()) {
                name_reads.layers.extend(targets.into_iter().filter_map(|target| match target { Target::Matcher(layer) => Some(layer), _ => None }));
            }
        }
    }
    for condition in &grammar.names.conditions {
        let Some(when) = &config.names[condition.item].when else { continue };
        let layers = RefCell::default();
        let before = uses.borrow().len();
        let scope = Scope { uses: Some(&uses), layer_reads: Some(&layers), fields: target_fields(&grammar, Target::Matcher(condition.matcher)), attrs: &attr_names, ..Scope::new(&grammar) };
        if let Some(e) = scope.compile(when, &mut errors) {
            let _ = condition.expr.set(e);
        }
        let recorded = uses.borrow();
        let recorded = &recorded[before..];
        let (children, siblings) = reads_neighbours(recorded);
        name_reads.extend(ReadDependencies::collect(&grammar, &attr_names, recorded, layers.into_inner(), children || siblings));
    }
    for operand in &grammar.names.operands {
        let Some(source) = config.operand(operand.of, operand.part) else { continue };
        let layers = RefCell::default();
        let before = uses.borrow().len();
        let params = config.operand_parameter(operand.of).into_iter().map(str::to_owned).collect();
        let scope = Scope { params, uses: Some(&uses), layer_reads: Some(&layers), fields: target_fields(&grammar, Target::Matcher(operand.matcher)), attrs: &attr_names, ..Scope::new(&grammar) };
        if let Some(e) = scope.compile(source, &mut errors) {
            let _ = operand.expr.set(e);
        }
        let recorded = uses.borrow();
        let recorded = &recorded[before..];
        let (children, siblings) = reads_neighbours(recorded);
        name_reads.extend(ReadDependencies::collect(&grammar, &attr_names, recorded, layers.into_inner(), children || siblings));
    }
    phase("functions and names");
    let (attrs, mut helper_reads) = attributes(&config, &grammar, &attr_names, &uses, &mut errors);
    phase("attributes");
    helper_reads.extend(function_reads);
    let forwards = forwards(&config, &grammar, &attr_names, &mut errors);
    for &(matcher, attr, _) in &forwards {
        helper_reads[attr as usize].requires_native |= is_native_layer(&grammar, matcher);
    }
    // Recipes reach `let` attributes through path steps; their reads are
    // semantic reads too.
    name_reads.helpers.extend(grammar.names.recipe_attributes.iter().map(|&attr| usize::from(attr)));
    name_reads.fields.extend(&grammar.names.recipe_fields);
    for &(_, attr, from) in &forwards {
        helper_reads[attr as usize].fields.push(from);
    }
    let direct_tokens = name_reads.all_tokens || name_reads.own_tokens;
    let mut unit_reads = name_reads.resolve(&helper_reads);
    // Fields read semantically may lead to layer or token nodes: every kind
    // a field of that label can hold, from any owner (owners are not
    // tracked here); an unknown target widens to everything.
    let mut unit_field_layers = Vec::new();
    for &label in &unit_reads.fields {
        unit_reads.own_tokens |= grammar.labels.get("token") == Some(label);
        let mut pattern = Vec::new();
        for matcher in 0..grammar.matchers.len() as u32 {
            let targets = grammar.field_targets(matcher, label);
            // An unresolved field shape may hold anything, tokens included.
            unit_reads.own_tokens |= targets.any;
            unit_reads.layers.extend(&targets.kinds);
            pattern.extend(targets.kinds.iter().filter(|&&kind| grammar.native_kind(kind).is_none())
                .map(|&kind| grammar.matchers[kind as usize].name.clone()));
        }
        pattern.sort_unstable();
        pattern.dedup();
        if !pattern.is_empty() { unit_field_layers.push(format!("{} -> {}", grammar.labels.name(label), pattern.join(" "))) }
        unit_reads.layers = unique(std::mem::take(&mut unit_reads.layers));
    }
    // Why semantic-only units attach tokens (shown by --stats).
    let unit_token_reasons: Vec<String> = direct_tokens.then(|| "name-item conditions, operands or recipe fields".to_owned()).into_iter()
        .chain(unit_reads.token_helpers.iter().map(|&id| match <[String]>::get(&attr_names, id) {
            Some(name) => format!("let {name}"),
            None => format!("fn {}", grammar.functions.get(id - attr_names.len()).map_or("?", |f| f.name.as_str())),
        }))
        .chain(unit_field_layers.into_iter().map(|layers| format!("field {layers}")))
        .chain(unit_reads.layers.iter().filter(|&&layer| grammar.native_kind(layer).is_some_and(|kind| kind.is_token())
            || grammar.native_hosts[layer as usize].iter().any(|&host| grammar.native_kind(host).is_some_and(|kind| kind.is_token())))
            .map(|&layer| format!("layer {}", grammar.matchers[layer as usize].name)))
        .collect();
    let language_options = super::language_options::LanguageOptions::compile(&config, &grammar, &schemas, &mut errors);
    phase("unit demand");

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
    // Each definition's compiled copy for files outside every `files` block.
    let mut base: Vec<Option<usize>> = vec![None; config.rules.len()];
    for (d, def) in config.rules.iter().enumerate() {
        if def.inert {
            inert.push(as_inert(def));
            continue;
        }
        // An off library rule that does not compile (the config redefined
        // a layer it reads) is the library's business until it is turned on.
        if def.defined_only && !def.live() {
            let mut quiet = Vec::new();
            match rule(def, &grammar, &schemas, &attr_names, &uses, &helper_reads, &mut quiet) {
                Some(r) if quiet.is_empty() && !def.broken => {
                    base[d] = Some(rules.len());
                    rules.push(r);
                }
                _ => inert.push(as_inert(def)),
            }
            continue;
        }
        let before = errors.len();
        if let Some(r) = rule(def, &grammar, &schemas, &attr_names, &uses, &helper_reads, &mut errors).filter(|_| !def.broken) {
            base[d] = Some(rules.len());
            rules.push(r);
        } else if def.defined_only && !def.broken && errors.len() == before {
            inert.push(as_inert(def));
        }
    }
    // Option values, in order, per configuring line; a rule that failed to
    // build was reported.
    let mut converted: Vec<Vec<Vec<(usize, Value<'static>)>>> = vec![Vec::new(); config.rules.len()];
    for (d, def_settings) in settings.iter().enumerate() {
        let def = &config.rules[d];
        let Some(b) = base[d] else {
            // Off bodies are not compiled at runtime, but their explicit
            // settings still have to fit the declared option schemas.
            let dormant = inert.iter().any(|r| r.name == def.name.text);
            if (!tooling || dormant) && (!def.enabled() || dormant) {
                for &(ci, oi) in def_settings.iter().flat_map(|s| &s.options) {
                    let (name, value) = &config.configs[ci].options[oi];
                    if let Some(option) = def.options.iter().find(|o| o.name.text == name.text) {
                        option_value(&def.name.text, name, value, &option.schema, &grammar, &schemas, &mut errors);
                    }
                }
            }
            continue;
        };
        let f = &mut rules[b];
        for setting in def_settings {
            let mut set = Vec::new();
            for &(ci, oi) in &setting.options {
                let (name, value) = &config.configs[ci].options[oi];
                let Some(slot) = f.options.iter().position(|o| o.name == name.text) else {
                    let mut err = ConfigError::new(name.span, format!("no option `{}` on `{}`", name.text, f.name))
                        .suggest(&name.text, f.options.iter().map(|o| o.name.as_str()));
                    if f.options.is_empty() {
                        err = err.note("this rule has no options");
                    }
                    errors.push(err);
                    continue;
                };
                if let Some(value) = option_value(&f.name, name, value, &f.options[slot].schema, &grammar, &schemas, &mut errors) {
                    if setting.scope.is_none() {
                        f.options[slot].value = value.clone();
                    }
                    set.push((slot, value));
                }
            }
            converted[d].push(set);
        }
    }
    let scoped = scoped_copies(&config, &settings, converted, &base, &mut rules, |def| {
        rule(def, &grammar, &schemas, &attr_names, &RefCell::default(), &helper_reads, &mut Vec::new())
    });
    let scope_globs = config.scopes.iter().map(|scope| PathGlobs::new(scope.globs.iter().map(|(g, _)| g.as_str())).unwrap_or_else(|(glob, e)| {
        errors.push(ConfigError::new(scope.globs.iter().find(|(g, _)| *g == glob).map_or(scope.span, |(_, s)| *s), format!("invalid glob `{glob}`: {e}")));
        PathGlobs::default()
    })).collect();
    let labels = config.scopes.iter().map(|scope| scope.globs.iter().map(|(g, _)| format!("\"{g}\"")).collect::<Vec<_>>().join(" ")).collect();
    let scopes = Scopes::new(scope_globs, labels, scoped);

    phase("rules");
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
    let symbols = tooling.then(|| std::sync::Arc::new(Symbols::build(&config, &grammar, &rules, &scopes, &inert, uses.into_inner(), sources.clone())));
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
    let mut by_selection: crate::fx_hash::FxHashMap<u32, Vec<(NodeSelection, Vec<usize>)>> = Default::default();
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
                Target::Nodes(m, selection) => {
                    for x in 0..grammar.matchers.len() {
                        if !grammar.is_a(x as u32, m) { continue; }
                        let selections = by_selection.entry(x as u32).or_default();
                        match selections.iter_mut().find(|(s, _)| *s == selection) {
                            Some((_, rules)) if !rules.contains(&i) => rules.push(i),
                            Some(_) => {}
                            None => selections.push((selection, vec![i])),
                        }
                    }
                }
                Target::Lang(lang) => by_lang.push((i, lang)),
            }
        }
    }
    phase("symbols and dispatch");
    let attrs = Attributes::new(attrs, &forwards, &grammar);
    phase("attribute lowering");
    let reach = grammar.reach();
    phase("reach");
    // Reads of a layer also select its specialisations (`x like y`).
    let mut unit_layers: Vec<u32> = (0..grammar.matchers.len() as u32)
        .filter(|&m| unit_reads.layers.iter().any(|&layer| grammar.is_a(m, layer)))
        .collect();
    unit_layers.extend(&unit_reads.layers);
    let unit_layers = unique(unit_layers);
    let unit_overlays: Vec<bool> = reach.iter().map(|reachable| unit_layers.iter().any(|&layer| reachable[layer as usize])).collect();
    // Per front end, the token kinds semantic-only units attach: those the
    // semantic layers are or live in; every kind when a read could see the
    // others missing (as for linted files, `Linter::lint`).
    let unit_token_kinds: Vec<Vec<bool>> = grammar.natives.iter().enumerate().map(|(front, native)| {
        let count = native.frontend.kinds().len();
        let mut wanted = vec![false; count];
        let mut mark = |m: u32| {
            if let Some(kind) = grammar.native_kind(m).filter(|kind| usize::from(kind.front) == front && kind.is_token()) {
                wanted[usize::from(kind.kind)] = true;
            }
        };
        for &layer in &unit_layers {
            mark(layer);
            for &host in &grammar.native_hosts[layer as usize] { mark(host) }
        }
        if wanted.iter().any(|&on| on) && unit_reads.all_tokens || unit_reads.own_tokens {
            for (kind, on) in wanted.iter_mut().enumerate() { *on |= native.frontend.is_token(kind as u16) }
        }
        wanted
    }).collect();
    let mut linter = Linter {
        invalidation: Default::default(),
        invalidated: Default::default(),
        units: Default::default(),
        discoveries: Default::default(),
        programs: Default::default(),
        scopes,
        unit_overlays,
        unit_token_reasons,
        unit_token_kinds,
        unit_child_layers: !unit_reads.all_tokens && !unit_reads.own_tokens,
        semantic_layers: unit_layers.iter().copied().filter(|&m| grammar.native_kind(m).is_none()).collect(),
        reach,
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
        by_selection,
        by_lang,
        attrs,
        fingerprint,
    };
    phase("attributes and reach");
    errors.extend(linter.lower_conditions(&config));
    phase("lowering");
    linter.shape_records();
    phase("record shapes");
    Built { linter: Some(linter), errors, sources, symbols, phases }
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
        let source = "import .rs\nimport .ts\nimport .py\nimport .c\nimport .dx\nimport .md\nimport .toml\nrule *\n";
        let built = build_for_lint(config::load_text("test.dx", source, &Disk), PathBuf::from("/root"));
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
                let built = compile(config::load_text("test.dx", &source, &Disk), PathBuf::from("/root"));
                assert!(built.errors.is_empty(), "{:#?}", built.errors);
            }
            let source = "lang t \"*.t\"\ngroup items\nin items rule item-rule warn 'item'\n";
            let built = compile(config::load_text("test.dx", source, &Disk), PathBuf::from("/root"));
            assert!(!built.errors.is_empty());
            let source = "lang t \"*.t\"\ndefine {\ngroup items\nin items rule item-rule warn 'item' { option max int = 1 }\n}\nrule item-rule { set max 'bad' }\n";
            let built = compile(config::load_text("test.dx", source, &Disk), PathBuf::from("/root"));
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
        let runtime = build_for_lint(config::load_text("test.dx", source, &Disk), root.clone());
        let tooling = build(config::load_text("test.dx", source, &Disk), root);
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

/// The definitions some `files` block configures, each with a compiled copy
/// per distinct option set the blocks can give it: every combination of
/// the blocks setting its options, over its own values. Past
/// [`MAX_COMBINED`] blocks or [`MAX_COPIES`] option sets, one copy reads
/// options at runtime instead.
fn scoped_copies(
    config: &Config,
    settings: &[Vec<Setting>],
    mut converted: Vec<Vec<Vec<(usize, Value<'static>)>>>,
    base: &[Option<usize>],
    rules: &mut Vec<Rule>,
    compile: impl Fn(&RuleDef) -> Option<Rule>,
) -> Vec<Scoped> {
    use super::scopes::{MAX_COMBINED, MAX_COPIES};
    let mut out = Vec::new();
    for (d, def_settings) in settings.iter().enumerate() {
        let Some(b) = base[d] else { continue };
        if def_settings.iter().all(|s| s.scope.is_none()) {
            continue;
        }
        let def = &config.rules[d];
        let mut scoped = Scoped {
            copies: vec![b],
            default: def.severity.unwrap_or(Severity::Warn),
            defined_only: def.defined_only,
            settings: def_settings.clone(),
            values: std::mem::take(&mut converted[d]),
            by_key: Default::default(),
            unfolded: None,
            slots: rules[b].options.len(),
        };
        let outside = scoped.key(|scope| scope.is_none());
        scoped.by_key.insert(outside, b);
        let blocks = scoped.option_scopes();
        if blocks.is_empty() {
            out.push(scoped);
            continue;
        }
        let own = rules[b].option_values();
        // Copies run only where a file's settings pick them.
        let compile = |def: &RuleDef| compile(def).map(|mut copy| {
            copy.severity = Severity::Off;
            copy
        });
        // Every combination of the blocks that set options: which values win.
        let keys: Option<Vec<Vec<Option<usize>>>> = (blocks.len() <= MAX_COMBINED).then(|| (0..1usize << blocks.len()).map(|mask| {
            scoped.key(|scope| scope.is_none_or(|s| blocks.iter().position(|&b| b == s).is_none_or(|bit| mask >> bit & 1 == 1)))
        }).collect());
        let mut sets: Vec<Vec<Value<'static>>> = vec![own.clone()];
        let mut chosen: Vec<(Vec<Option<usize>>, usize)> = Vec::new();
        for key in keys.iter().flatten() {
            let values = scoped.options(key, &own);
            let at = sets.iter().position(|set| *set == values).unwrap_or_else(|| {
                sets.push(values);
                sets.len() - 1
            });
            chosen.push((key.clone(), at));
        }
        if keys.is_none() || sets.len() - 1 > MAX_COPIES {
            if let Some(mut copy) = compile(def) {
                copy.unfolded = true;
                copy.options.iter_mut().zip(&own).for_each(|(slot, value)| slot.value = value.clone());
                scoped.unfolded = Some(rules.len());
                scoped.copies.push(rules.len());
                rules.push(copy);
            }
            out.push(scoped);
            continue;
        }
        let mut copies = vec![Some(b)];
        for set in &sets[1..] {
            copies.push(compile(def).map(|mut copy| {
                copy.options.iter_mut().zip(set).for_each(|(slot, value)| slot.value = value.clone());
                scoped.copies.push(rules.len());
                rules.push(copy);
                rules.len() - 1
            }));
        }
        for (key, at) in chosen {
            if let Some(copy) = copies[at] {
                scoped.by_key.insert(key, copy);
            }
        }
        out.push(scoped);
    }
    out
}

/// Applies `rule name ...` lines to the definitions, in order: turning
/// rules on or off, replacing messages and fixes, adding clauses. Lines in
/// `files` blocks change no definition. Returns, per definition, every line
/// that configures it (in order, blocks included): its severity changes and
/// the option values to set once the rules are compiled.
fn configure(config: &mut crate::config::ast::Config, errors: &mut Vec<ConfigError>) -> Vec<Vec<Setting>> {
    let mut settings: Vec<Vec<Setting>> = vec![Vec::new(); config.rules.len()];
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
        // `severity <level>` sees, per file, the rules on there at this point.
        if c.on_only {
            let change = Change::OnlyIfOn(c.patch.severity.unwrap_or(Severity::Warn));
            for &i in &matching {
                settings[i].push(Setting { scope: c.scope, change, options: Vec::new() });
            }
            if c.scope.is_none() {
                matching.retain(|&i| config.rules[i].enabled());
                for &i in &matching {
                    config.rules[i].configured = c.patch.severity;
                }
            }
            continue;
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
        let mut owned: Vec<Vec<(usize, usize)>> = vec![Vec::new(); matching.len()];
        for (oi, (name, _)) in c.options.iter().enumerate() {
            let mut owners = 0;
            for (m, &i) in matching.iter().enumerate() {
                if config.rules[i].options.iter().any(|o| o.name.text == name.text) {
                    owned[m].push((ci, oi));
                    owners += 1;
                }
            }
            if owners == 0 {
                let names = matching.iter().flat_map(|&i| config.rules[i].options.iter().map(|o| o.name.text.as_str()));
                errors.push(ConfigError::new(name.span, format!("no option `{}` on `{}`", name.text, pattern.text)).suggest(&name.text, names));
            }
        }
        let change = match patch.severity {
            Some(s) => Change::Set(s),
            None => Change::On,
        };
        for (&i, options) in matching.iter().zip(owned) {
            settings[i].push(Setting { scope: c.scope, change, options });
            if c.scope.is_some() {
                config.rules[i].scoped_on |= !matches!(change, Change::Set(Severity::Off));
            }
        }
        if c.scope.is_some() {
            continue;
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
            }
            def.suggestions.extend(patch.suggestions.iter().cloned());
            if patch.report.is_some() {
                def.report = patch.report;
                def.report_at = None;
            }
            if let Some(at) = &patch.report_at {
                def.report_at = Some(at.clone());
            }
            def.when.extend(patch.when.iter().cloned());
            def.locals.extend(patch.locals.iter().cloned());
            def.item_when.extend(patch.item_when.iter().cloned());
            def.item_locals.extend(patch.item_locals.iter().cloned());
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
    settings
}

/// Labels available on a node of `target`.
fn target_fields(g: &Grammar, target: Target) -> Vec<(String, u16)> {
    let names = match target {
        Target::Matcher(crate::grammar::ROOT | crate::grammar::TEXT) => Vec::new(),
        Target::Matcher(m) => g.matchers[m as usize].labels(),
        Target::Field(m, label) => {
            let pat = &g.matchers[m as usize].pat;
            let label_name = g.labels.name(label);
            match pat.label_target(label_name).and_then(|t| g.matcher(t)) {
                Some(t) => g.matchers[t as usize].labels(),
                None => capture_inner_labels(pat, label_name),
            }
        }
        Target::Nodes(_, _) | Target::Lang(_) => Vec::new(),
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
    let mut native_layers: Vec<Option<bool>> = vec![None; g.matchers.len()];
    // Identifies each compiled body: definitions sharing one were compiled once.
    let mut next_body = 0u32;
    let functions: HashMap<&str, &crate::config::ast::FnDef> = config.functions.iter().map(|f| (f.name.text.as_str(), f)).collect();
    for def in &config.attrs {
        if crate::expr::is_builtin(&def.name.text) {
            errors.push(ConfigError::new(def.name.span, format!("`{}` is a built-in variable and cannot name an attribute", def.name.text)));
            continue;
        }
        let id = usize::from(names.get(&def.name.text).expect("collected"));
        // Compiling consults a layer's fields only as "has a field named
        // N" (a hit is the same global label everywhere), so the body
        // compiles alike on every layer agreeing on the labels the body (or a
        // function it calls) names: compile once per such agreement.
        let mentioned = mentioned_labels(g, &def.expr, &functions);
        let mut shared: Vec<(Vec<u16>, CExpr, u32, Vec<(crate::config::source::Span, crate::expr::Resolved)>, Vec<u32>)> = Vec::new();
        for target in &def.targets {
            let Some((resolved, _)) = resolve_target(&AstTarget::Matcher(target.clone()), g, None, errors) else { continue };
            // Through a group: a default its members' own definitions refine.
            let through_group = !(resolved.len() == 1 && matches!(resolved[0], Target::Matcher(m) if g.matcher(&target.text) == Some(m)));
            for t in resolved {
                let Target::Matcher(m) = t else {
                    errors.push(ConfigError::new(target.span, "attributes belong to layers, not languages"));
                    continue;
                };
                match attrs[id].defs.iter().position(|(d, ..)| *d == m) {
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
                let fields = target_fields(g, t);
                let agreement: Vec<u16> = fields.iter().map(|&(_, label)| label).filter(|label| mentioned.contains(label)).collect();
                let (compiled, attribute_uses, reads) = match shared.iter().find(|(known, ..)| *known == agreement) {
                    Some((_, e, body, attribute_uses, reads)) => (Some((e.clone(), *body)), attribute_uses.clone(), reads.clone()),
                    None => {
                        let layer_reads = RefCell::default();
                        let scope = Scope { fields: fields.clone(), attrs: names, uses: Some(uses), layer_reads: Some(&layer_reads), ..Scope::new(g) };
                        let (uses_before, errors_before) = (uses.borrow().len(), errors.len());
                        let compiled = scope.compile(&def.expr, errors).map(|e| {
                            next_body += 1;
                            (e, next_body)
                        });
                        let attribute_uses = uses.borrow()[uses_before..].to_vec();
                        let reads = layer_reads.into_inner();
                        if let Some((e, body)) = compiled.as_ref().filter(|_| errors.len() == errors_before) {
                            shared.push((agreement, e.clone(), *body, attribute_uses.clone(), reads.clone()));
                        }
                        (compiled, attribute_uses, reads)
                    }
                };
                if let Some((e, body)) = compiled {
                    attrs[id].defs.push((m, e, body));
                }
                let (children, siblings) = reads_neighbours(&attribute_uses);
                dependencies[id].extend(ReadDependencies::collect(g, names, &attribute_uses, reads, children || is_token_layer(g, m) && siblings));
                dependencies[id].requires_native |= *native_layers[m as usize].get_or_insert_with(|| is_native_layer(g, m));
            }
        }
    }
    // A node reads the first definition whose layer it is: the most specific
    // first (a member before its group, a layer before its base), so
    // `in ts_node let x ...` gives a default that `in ts_binary let x ...`
    // refines. Otherwise definition order.
    for attr in &mut attrs {
        let targets: Vec<u32> = attr.defs.iter().map(|(m, ..)| *m).collect();
        let broader = |m: u32| targets.iter().filter(|&&t| t != m && g.is_a(m, t)).count();
        attr.defs.sort_by_key(|(m, ..)| std::cmp::Reverse(broader(*m)));
    }
    (attrs, dependencies)
}

/// Labels named anywhere in `expr` or the functions it calls (an
/// over-estimate: only ever splits compiles more finely).
fn mentioned_labels(g: &Grammar, expr: &crate::config::ast::Expr, functions: &HashMap<&str, &crate::config::ast::FnDef>) -> Vec<u16> {
    let mut names = std::collections::HashSet::new();
    super::prune::expr_names(expr, &mut names);
    let mut pending: Vec<String> = names.iter().cloned().collect();
    let mut visited = std::collections::HashSet::new();
    while let Some(name) = pending.pop() {
        let Some(function) = functions.get(name.as_str()) else { continue };
        if !visited.insert(name) { continue }
        let mut inner = std::collections::HashSet::new();
        super::prune::expr_names(&function.body, &mut inner);
        for found in inner {
            if names.insert(found.clone()) { pending.push(found) }
        }
    }
    let mut labels: Vec<u16> = names.iter().filter_map(|name| g.labels.get(name)).collect();
    labels.sort_unstable();
    labels.dedup();
    labels
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
        let (resolved, text) = resolve_target(t, g, Some(attrs), errors)?;
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
    for (name, e) in &def.item_locals {
        item_read.set(false);
        if let Some(c) = scope.compile(e, errors) {
            scope.inlined.push((name.text.clone(), c, item_read.get()));
        }
    }
    let (mut node_when, mut when) = (Vec::new(), Vec::new());
    for e in &def.when {
        item_read.set(false);
        if let Some(c) = scope.compile(e, errors) {
            if def.each_block && item_read.get() {
                errors.push(ConfigError::new(e.span, "this `when` runs once per node, so it cannot read the `each` item")
                    .note("move it inside the `each ... { }` block"));
            } else if def.each.is_none() || item_read.get() { when.push(c) } else { node_when.push(c) }
        }
    }
    for e in &def.item_when {
        if let Some(c) = scope.compile(e, errors) { when.push(c) }
    }
    scope.param_read = None;
    let message = def.message.as_ref().and_then(|m| scope.compile(m, errors));
    let fix = def.fix.as_ref().and_then(|fix| fix_plan(&scope, None, fix, errors));
    let suggestions = def.suggestions.iter().filter_map(|s| fix_plan(&scope, Some(&s.title), &s.fix, errors)).collect();
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
    let mut reads = ReadDependencies::collect(g, attrs, rule_uses, layer_reads.into_inner(), false);
    reads.helpers.extend(targets.iter().filter_map(|target| match target {
        Target::Nodes(_, NodeSelection::Attribute(attr)) => Some(usize::from(*attr)),
        _ => None,
    }));
    let ReadRequirements { layers: layer_reads, all_tokens: helper_tokens, own_tokens, requires_native, .. } = reads.resolve(helpers);
    let requires_native = requires_native || targets.iter().any(|target| match target {
        Target::Matcher(m) | Target::Field(m, _) | Target::Nodes(m, NodeSelection::Attribute(_)) => is_native_layer(g, *m),
        Target::Lang(_) => false,
    });
    let needs_tokens = |m| is_token_layer(g, m) || g.native_hosts[m as usize].iter().any(|&host| is_token_layer(g, host));
    let on_tokens = layer_reads.iter().any(|&m| needs_tokens(m)) || targets.iter().any(|target| match target {
        Target::Matcher(m) | Target::Field(m, _) | Target::Nodes(m, _) => needs_tokens(*m),
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
        live: def.live(),
        unfolded: false,
        options,
        files,
        ignore,
        node_when,
        when,
        node_when_vm: Vec::new(),
        when_vm: Vec::new(),
        each_vm: None,
        report_at_vm: None,
        locals_vm: Vec::new(),
        locals,
        message,
        message_file: def.message.as_ref().map_or(def.name.span.file, |message| message.span.file),
        message_plan: None,
        message_vm: None,
        fix,
        suggestions,
        indents: crate::expr::IndentPlan::new(indents, g),
        indent_reset,
        report: def.report.unwrap_or_default(),
        report_at,
        each,
    })
}

/// A `fix` or `suggest` clause compiled; every expression is compiled (and
/// its mistakes reported) even after one fails.
fn fix_plan(scope: &Scope<'_>, title: Option<&Expr>, fix: &FixDef, errors: &mut Vec<ConfigError>) -> Option<FixPlan> {
    let mut clause = |e: &Expr| scope.compile(e, errors).map(Clause::new);
    let title = title.map(&mut clause);
    let when = fix.when.as_ref().map(&mut clause);
    let parts: Vec<_> = fix.parts.iter().map(|part| (part.target, part.at.as_ref().map(&mut clause), clause(&part.text))).collect();
    // Written but not compiled: the whole clause is left out.
    let compiled = |c: Option<Option<Clause>>| c.map(|c| c.ok_or(())).transpose().ok();
    let parts = parts.into_iter()
        .map(|(target, at, text)| Some(PartPlan { target, at: compiled(at)?, text: text? }))
        .collect::<Option<Vec<_>>>()?;
    Some(FixPlan { title: compiled(title)?, when: compiled(when)?, parts })
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
            let Some((resolved, _)) = resolve_target(&AstTarget::Matcher(target.clone()), g, None, errors) else { continue };
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
fn resolve_target(target: &AstTarget, g: &Grammar, attrs: Option<&AttrNames>, errors: &mut Vec<ConfigError>) -> Option<(Vec<Target>, String)> {
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
            match (label, attrs.and_then(|attrs| attrs.get(&field.text))) {
                (Some(label), _) => (vec![Target::Field(m, label)], format!("{}.{}", layer.text, field.text)),
                (None, Some(attr)) => (vec![Target::Nodes(m, NodeSelection::Attribute(attr))], format!("{}.{}", layer.text, field.text)),
                (None, None) => {
                    let names = labels.iter().map(String::as_str)
                        .chain(attrs.into_iter().flat_map(|attrs| attrs.iter().map(String::as_str)));
                    errors.push(
                        ConfigError::new(field.span, format!("matcher `{}` has no field or attribute `{}`", layer.text, field.text))
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
        let built = build(crate::config::load_text("test.dx", &source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
        built.linter.expect("valid config")
    }

    #[test]
    fn module_modes_compile_without_a_syntax_matcher() {
        let linter = native(r#"
            module_resolution probe {
                search [relative]
                when resolution.config.enabled == true {
                    search [packages]
                }
            }
            in ts_program uses module_resolution probe
        "#);
        let mode = &linter.grammar.names.module_resolutions[0].modes[0];
        assert!(linter.grammar.names.operands[mode.when as usize].lowered.get().is_some());
    }

    #[test]
    fn each_items_cannot_hide_names_visible_in_the_rule() {
        let mut failures = Vec::new();
        for (target, prefix, item, list, reject) in [
            ("ts_function", "", "name", "[1]", true),
            ("ts_program", "", "text", "[1]", true),
            ("ts_program", "in ts_program let item [1]\n", "item", "item", true),
            ("ts_program", "", "item", "[1]", false),
            ("ts_program", "", "name", "[1]", false),
        ] {
            let source = format!("lang js \"*.ts\"\nnative ts\n{prefix}in {target} rule probe {{\neach {item} in {list}\nwarn \"{{{item}}}\"\n}}");
            let built = build(crate::config::load_text("each.dx", &source, &Disk), PathBuf::from("/root"));
            let item_start = source.find("each ").unwrap() + 5;
            let collision = built.errors.len() == 1 && built.errors[0].span.is_some_and(|span|
                span.start as usize == item_start && span.end as usize == item_start + item.len());
            if collision != reject || !reject && !built.errors.is_empty() {
                failures.push((source, reject, built.errors));
            }
        }
        for setup in ["let item 1", "option item number = 1 \"item\""] {
            let source = format!("lang js \"*.ts\"\nnative ts\nin ts_program rule probe {{\n{setup}\neach item in [1]\nwarn \"{{item}}\"\n}}");
            let built = build(crate::config::load_text("each.dx", &source, &Disk), PathBuf::from("/root"));
            let item_start = source.find("each ").unwrap() + 5;
            if built.errors.len() != 1 || !built.errors[0].span.is_some_and(|span|
                span.start as usize == item_start && span.end as usize == item_start + 4) {
                failures.push((source, true, built.errors));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn attribute_targets_select_nodes_without_a_native_frontend() {
        let config = r#"
            lang plain "*.t"
            in .plain match document /[\s\S]+/
            in document match item /[a-z]/
            in item let selected if text == "a" then parent.children.nodes else [node, node]
            in item.selected, item rule selected info text
            define {
                in item let disabled 7
                in item.disabled rule inactive info text
            }
        "#;
        for runtime in [false, true] {
            let loaded = crate::config::load_text("selected.dx", config, &Disk);
            let built = if runtime { build_for_lint(loaded, PathBuf::from("/root")) }
                else { build(loaded, PathBuf::from("/root")) };
            assert!(built.errors.is_empty(), "{:?}", built.errors);
            let mut linter = built.linter.expect("attribute targets");
            linter.enable_profile();
            let diagnostics = linter.lint(Path::new("/root/file.t"), "ab");
            assert_eq!(diagnostics.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(), ["a", "b"]);
            assert_eq!(super::super::Profile::get(&linter.profile.as_ref().expect("profile").rule_evals[0]), 2);
        }
    }

    #[test]
    fn attribute_targets_report_failures_but_allow_absent_nodes() {
        for (expression, failed) in [("null", false), ("[]", false), ("7", true), ("selected", true)] {
            let config = format!("lang plain \"*.t\"\nin .plain match item /a/\nin item let selected {expression}\nin item.selected rule selected info text\n");
            let built = build(crate::config::load_text("selected.dx", &config, &Disk), PathBuf::from("/root"));
            assert!(built.errors.is_empty(), "{:?}", built.errors);
            let diagnostics = built.linter.expect("attribute targets").lint(Path::new("/root/file.t"), "a");
            if failed {
                assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
                assert_eq!(diagnostics[0].severity, Severity::Error);
            } else {
                assert!(diagnostics.is_empty(), "{diagnostics:?}");
            }
        }
    }

    #[test]
    fn linked_targets_keep_selected_context_and_run_once_per_node() {
        let built = build(crate::config::load_text("targets.dx", r#"
            import .ts
            in ts_identifier let checked text == "value" || text == "later"
            in ts_declarator.name, ts_identifier.declaration, ts_identifier.declarations rule linked info checked {
                option replacement string = "renamed"
                let start offset
                report at start
                fix as replacement
            }
        "#, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:?}", built.errors);
        let mut linter = built.linter.expect("attribute target config");
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
        let linked = linter.rules.iter().position(|rule| rule.name == "linked").expect("linked rule");
        assert_eq!(super::super::Profile::get(&linter.profile.as_ref().expect("profile").rule_evals[linked]), starts.len() as u64);
        assert!(linter.lint(Path::new("/root/empty.ts"), "").is_empty());
        assert!(linter.lint(Path::new("/root/file.t"), source).is_empty());

        let fields = native("in ts_call.callee rule field info text");
        let source = "call(value);";
        let diagnostics = fields.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(&source[diagnostics[0].start..diagnostics[0].end], "call");

        let built = build(crate::config::load_text("field.dx",
            "lang plain \"*.t\"\nin .plain match holder /@/ declaration:/[a-z]+/\nin holder let declaration node\nin holder.declaration rule field info text", &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:?}", built.errors);
        let diagnostics = built.linter.expect("field config").lint(Path::new("/root/file.t"), "@value");
        assert_eq!(diagnostics.len(), 1, "real fields win over node-valued attributes");
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
            let built = build(crate::config::load_text("test.dx", &format!("lang js \"*.ts\"\nnative ts\n{rule}{config}\n"), &Disk), PathBuf::from("/root"));
            assert!(built.errors.iter().any(|e| e.message.contains(problem)), "{config}: {:#?}", built.errors);
        }
        let without = "define {\nin ts_call rule plain warn \"x\" { option mode enum(a, b) = a }\n}\nrule plain set b\n";
        let built = build(crate::config::load_text("test.dx", &format!("lang js \"*.ts\"\nnative ts\n{without}"), &Disk), PathBuf::from("/root"));
        assert!(built.errors.iter().any(|e| e.message.contains("has no default option")), "{:#?}", built.errors);
        let severity_word = "define {\nin ts_call rule bad warn \"x\" { default option mode enum(off, on) = on }\n}\n";
        let built = build(crate::config::load_text("test.dx", &format!("lang js \"*.ts\"\nnative ts\n{severity_word}"), &Disk), PathBuf::from("/root"));
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
        let built = build(crate::config::load_text("test.dx", source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.iter().any(|e| e.message.contains("no attribute `missing`")), "{:#?}", built.errors);
    }
}
