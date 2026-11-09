//! Library layers (matchers written inside `define { ... }`) take part in
//! parsing only while something needs them, so importing a language's rule
//! definitions does not change the tree a config sees, nor what its own
//! matchers win ties against.
//!
//! Needed are: what turned-on rules target or mention (names, field paths,
//! `is` lists, pattern references, and string literals naming a layer, as
//! in `parent == "js_class_body"`), everything a config's own items refer
//! to, and then, transitively, the pattern references, `like` bases and
//! parents (every member of a parent group) of needed layers, and what
//! needed attributes read. So a needed layer sits in the same tree as with
//! the whole library present; layers only other rules need stay out.
//! Off rules that read a pruned layer or attribute stay defined but are not
//! compiled (`RuleDef::inert`).

use std::collections::HashSet;

use crate::config::ast::{calls, effects, Config, Expr, ExprKind, Mentions, Name, NameItem, Parent, PipelineStage, Target, TemplatePart, Unwrap};
use crate::fx_hash::{FxHashMap, FxHashSet};
use crate::pattern::Pat;

/// Names borrowed from the config they were read from.
pub(super) type Names<'a> = FxHashSet<&'a str>;

/// Runtime compilation keeps only helpers reachable from enabled rules
/// and explicit grammar items. Tooling still uses [`prune`].
pub fn prune_for_lint(config: &mut Config, errors: &mut Vec<crate::config::error::ConfigError>) {
    crate::grammar::function_declarations(config, errors, &crate::expr::is_builtin_call);
    prune_config(config, true);
}

/// Every name an expression mentions: path segments, function arguments,
/// `is` names, pattern references and string literals. An over-estimate,
/// which only ever keeps more.
pub(super) fn expr_names<'a>(e: &'a Expr, out: &mut Names<'a>) {
    match &e.kind {
        ExprKind::Str(s) => {
            out.insert(s);
        }
        ExprKind::Template(parts) => {
            for p in parts {
                match p {
                    TemplatePart::Lit(s) => {
                        out.insert(s);
                    }
                    TemplatePart::Expr(e, _) => expr_names(e, out),
                }
            }
        }
        ExprKind::Num(_) | ExprKind::Bool(_) | ExprKind::Null => {}
        ExprKind::List(items) => items.iter().for_each(|i| expr_names(i, out)),
        ExprKind::Record(entries) => entries.iter().for_each(|(_, v)| expr_names(v, out)),
        ExprKind::Lambda(_, body) => expr_names(body, out),
        // `f(x).attr` reads `attr` on node values too.
        ExprKind::Key(base, key) => {
            expr_names(base, out);
            out.insert(&key.text);
        }
        ExprKind::Var(path) => path_names(path, out),
        // A config `fn` needs what its body reads.
        ExprKind::Call(name, args) => {
            out.insert(&name.text);
            args.iter().for_each(|a| expr_names(a, out));
        }
        ExprKind::NodeCall(receiver, name, args) => {
            expr_names(receiver, out);
            out.insert(&name.text);
            args.iter().for_each(|a| expr_names(a, out));
        }
        ExprKind::Unary(_, a) => expr_names(a, out),
        ExprKind::Binary(_, a, b) | ExprKind::Nullish(a, b) => {
            expr_names(a, out);
            expr_names(b, out);
        }
        ExprKind::Pipeline(steps) => {
            for step in steps {
                match &step.stage {
                    PipelineStage::Value(value) | PipelineStage::Then(_, value) => expr_names(value, out),
                    PipelineStage::Call(name, args) => {
                        out.insert(&name.text);
                        args.iter().for_each(|arg| expr_names(arg, out));
                    }
                }
            }
        }
        ExprKind::If(c, a, b) => {
            expr_names(c, out);
            expr_names(a, out);
            expr_names(b, out);
        }
        ExprKind::Test(value, pat) => {
            expr_names(value, out);
            pattern_names(pat, out);
        }
        ExprKind::Is(value, names) => {
            expr_names(value, out);
            out.extend(names.iter().filter_map(|n| n.kind()).map(|n| n.text.as_str()));
        }
        ExprKind::Trait(value, _, traits) => {
            expr_names(value, out);
            path_names(traits, out);
        }
    }
}

fn path_names<'a>(path: &'a [Name], out: &mut Names<'a>) {
    out.extend(path.iter().map(|name| name.text.as_str()));
}

fn value_names<'a>(value: &'a Unwrap, out: &mut Names<'a>) {
    match value {
        Unwrap::Path(path) => path_names(path, out),
        Unwrap::Spread { path, value, .. } => {
            path_names(path, out);
            if let Some(value) = value { value_names(value, out); }
        }
        Unwrap::Text(text) => { out.insert(text); }
        Unwrap::Traits(traits) => path_names(traits, out),
        Unwrap::Returned { default } => value_names(default, out),
        Unwrap::Either(left, right) => {
            value_names(left, out);
            value_names(right, out);
        }
        Unwrap::Union(values) | Unwrap::Intersection(values) => {
            values.iter().for_each(|value| value_names(value, out));
        }
        Unwrap::From(source, key, classes) => {
            value_names(source, out);
            value_names(key, out);
            path_names(classes, out);
        }
        Unwrap::Scope { classes, from } => {
            path_names(classes, out);
            value_names(from, out);
        }
        Unwrap::Not(value) | Unwrap::Keys { of: value } | Unwrap::Argument { of: value, .. } => value_names(value, out),
        Unwrap::Excludes { traits, from } => {
            path_names(traits, out);
            if let Some(from) = from { value_names(from, out); }
        }
        Unwrap::Includes { value, from } | Unwrap::Has { key: value, from } => {
            value_names(value, out);
            if let Some(from) = from { value_names(from, out); }
        }
        Unwrap::Current | Unwrap::Unknown | Unwrap::Ordinal(_) => {}
        Unwrap::Select { check, pattern, yes, no, distribute, .. } => {
            for value in [check, pattern, yes, no] { value_names(value, out) }
            if let Some(path) = distribute { path_names(path, out) }
        }
        Unwrap::Product { shape, sites, value, optional, repeated, values, readonly } => {
            out.insert(&shape.text);
            path_names(sites, out);
            if let Some(value) = value { value_names(value, out) }
            for path in [optional, repeated, values, readonly].into_iter().flatten() { path_names(path, out) }
        }
        Unwrap::Instance { name, class, arguments } => {
            out.insert(&name.text);
            out.insert(&class.text);
            arguments.iter().for_each(|value| value_names(value, out));
        }
        Unwrap::Modified { source, required: _, readonly: _, flags } => {
            value_names(source, out);
            for (flag, _) in flags { out.insert(&flag.text); }
        }
    }
}

fn flow_names<'a>(flow: &'a effects::Flow, out: &mut Names<'a>) {
    match flow {
        effects::Flow::Nothing => {}
        effects::Flow::Field(path) => path_names(path, out),
        effects::Flow::Then(flows) | effects::Flow::Or(flows) => {
            flows.iter().for_each(|flow| flow_names(flow, out));
        }
        effects::Flow::Maybe(flow) => flow_names(flow, out),
        effects::Flow::Repeat { body, test, next, first: _ } => {
            flow_names(body, out);
            if let Some(test) = test { path_names(test, out); }
            if let Some(next) = next { flow_names(next, out); }
        }
        effects::Flow::Select { entries, test, body } => {
            path_names(entries, out);
            path_names(test, out);
            path_names(body, out);
        }
    }
}

fn item_names<'a>(item: &'a NameItem, out: &mut Names<'a>) {
    match item {
        NameItem::Scope { classes: _, within } => path_names(within, out),
        NameItem::Declares { path, keyed, filter, .. } => {
            path_names(path, out);
            if let Some(keyed) = keyed { expr_names(keyed, out); }
            if let Some(filter) = filter { expr_names(filter, out); }
        }
        NameItem::Resolves { path, keyed, outside, .. } => {
            path_names(path, out);
            if let Some(keyed) = keyed { expr_names(keyed, out); }
            if let Some(outside) = outside { expr_names(outside, out); }
        }
        NameItem::Module(item) => item.exprs().for_each(|expr| expr_names(expr, out)),
        NameItem::Calls(item) => match item {
            calls::Item::Signature { parameters, receiver, result } => {
                path_names(parameters, out);
                if let Some(receiver) = receiver { path_names(receiver, out); }
                value_names(result, out);
            }
            calls::Item::Sources(value, result) => {
                value_names(value, out);
                if let Some(result) = result { value_names(result, out); }
            }
            calls::Item::Invoke { callee, arguments, construct: _, receiver } => {
                value_names(callee, out);
                path_names(arguments, out);
                if let Some(receiver) = receiver { value_names(receiver, out); }
            }
            calls::Item::Expected { field, value, .. } => {
                path_names(field, out);
                value_names(value, out);
            }
            calls::Item::Identity(expr) => expr_names(expr, out),
            calls::Item::DiscardsResult | calls::Item::Optional | calls::Item::Flag(_) |calls::Item::ParameterVariance(_) | calls::Item::InferLastOverload | calls::Item::ExactFirstOverload => {}
        },
        NameItem::Generics(path) | NameItem::Arguments(path) | NameItem::Opaque(path) => path_names(path, out),
        NameItem::Widens { value } | NameItem::Defines { value } | NameItem::Refers { value } | NameItem::Constrains { value } | NameItem::Defaults { value }
        | NameItem::Enumerates { value, .. } => value_names(value, out),
        NameItem::Pack(shape) => { out.insert(&shape.text); }
        NameItem::Unwraps { value, .. }
        | NameItem::Members { classes: _, closed: _, inherits: value } => {
            if let Some(value) = value { value_names(value, out); }
        }
        NameItem::Execution(item) => match item {
            effects::Item::Assigned { target, value } => {
                path_names(target, out);
                if let Some(value) = value { value_names(value, out); }
            }
            effects::Item::Runs(flow) => flow_names(flow, out),
            effects::Item::Entry(path) | effects::Item::Labels(path) => path_names(path, out),
            effects::Item::Catches { handler, effects: _, source } => {
                path_names(handler, out);
                path_names(source, out);
            }
            effects::Item::After { finalizer, sources } => {
                path_names(finalizer, out);
                sources.iter().for_each(|source| path_names(source, out));
            }
        },
        NameItem::Fits { traits, views, .. } => {
            path_names(traits, out);
            path_names(views, out);
        }
        NameItem::Carries { description, arguments, value } => {
            if let Some(arguments) = arguments { path_names(arguments, out); }
            expr_names(description, out);
            if let Some(value) = value { value_names(value, out); }
        }
        NameItem::Indexed { path, mode, value, .. } => {
            path_names(path, out);
            match mode {
                crate::config::ast::values::IndexedMode::Keyed(key) => value_names(key, out),
                crate::config::ast::values::IndexedMode::Keys { bind: Some(bind) } => path_names(bind, out),
                _ => {}
            }
            if let Some(value) = value { value_names(value, out); }
        }
        NameItem::Member { classes: _, key, value } => {
            value_names(key, out);
            value_names(value, out);
        }
        NameItem::Propagates { field, value } => {
            path_names(field, out);
            value_names(value, out);
        }
        NameItem::Targets(groups) => groups.iter().flatten().for_each(|path| path_names(path, out)),
    }
}

fn pattern_names<'a>(pat: &'a Pat, out: &mut Names<'a>) {
    pat.walk(&mut |p| {
        if let Pat::Ref { name, .. } = p {
            out.insert(name);
        }
    });
}

fn parent_name(p: &Parent) -> Option<&str> {
    match p {
        Parent::Matcher(n) => Some(&n.text),
        _ => None,
    }
}

/// Drops the library layers (and library attributes, groups members,
/// indents and forwards on them) that nothing needs, and marks the off
/// rules that would read them as inert.
pub fn prune(config: &mut Config) {
    prune_config(config, false);
}

/// What pruning decided, owned so the config can change after.
struct Decision {
    /// Library layers nothing needs.
    gone: HashSet<String>,
    /// Library attributes nothing needs.
    unneeded_attrs: HashSet<String>,
    /// Functions to keep at runtime; for tooling, those whose reads must be
    /// checked against what pruning drops, with those reads.
    functions: FunctionPlan,
}

enum FunctionPlan {
    /// Runtime: keep only these.
    Keep(HashSet<String>),
    /// Tooling: unneeded helpers and what each reads.
    Check(Vec<(String, Vec<String>)>),
}

fn prune_config(config: &mut Config, runtime: bool) {
    let Some(decision) = decide(config, runtime) else { return };
    let gone = |n: &str| decision.gone.contains(n);
    config.matchers.retain(|m| !gone(&m.name.text));
    for m in &mut config.matchers {
        m.parents.retain(|p| parent_name(p).is_none_or(|n| !gone(n)));
    }
    for g in &mut config.groups {
        g.parents.retain(|p| parent_name(p).is_none_or(|n| !gone(n)));
    }
    let mut dropped_attrs: HashSet<String> = HashSet::new();
    for a in &mut config.attrs {
        if a.library && decision.unneeded_attrs.contains(&a.name.text) {
            a.targets.clear();
        }
        a.targets.retain(|t| !gone(&t.text));
    }
    for a in &config.attrs {
        dropped_attrs.insert(a.name.text.clone());
    }
    config.attrs.retain(|a| !a.targets.is_empty());
    for a in &config.attrs {
        dropped_attrs.remove(&a.name.text);
    }
    for f in &mut config.forwards {
        f.targets.retain(|t| !gone(&t.text));
        f.attrs.retain(|a| !dropped_attrs.contains(&a.text));
    }
    config.forwards.retain(|f| !f.targets.is_empty() && !f.attrs.is_empty());
    match &decision.functions {
        FunctionPlan::Keep(kept) => config.functions.retain(|f| kept.contains(&f.name.text)),
        FunctionPlan::Check(unneeded) => {
            // Tooling keeps unneeded helpers, but not ones that read (or call
            // helpers that read) a pruned layer or attribute: they cannot compile.
            let mut unusable: HashSet<&str> = HashSet::new();
            loop {
                let before = unusable.len();
                for (name, reads) in unneeded {
                    if reads.iter().any(|n| gone(n) || dropped_attrs.contains(n) || unusable.contains(n.as_str())) {
                        unusable.insert(name);
                    }
                }
                if unusable.len() == before {
                    break;
                }
            }
            config.functions.retain(|f| !unusable.contains(f.name.text.as_str()));
        }
    }

    // Runtime builds skip every rule that is off ([`super::build`]); tooling
    // lists those reading what was pruned as inert.
    if runtime { return }
    for r in config.rules.iter_mut().filter(|r| !r.live()) {
        let inert = {
            let mut names = Names::default();
            for t in &r.targets {
                if let Target::Matcher(n) | Target::Field(n, _) = t {
                    names.insert(&n.text);
                }
            }
            rule_names(r, &mut names);
            names.iter().any(|n| gone(n) || dropped_attrs.contains(*n))
        };
        if inert != r.inert { std::sync::Arc::make_mut(r).inert = inert }
    }
}

/// What nothing needs, read from the config as it stands; `None` when
/// tooling has nothing to drop.
fn decide(config: &Config, runtime: bool) -> Option<Decision> {
    let library: Names = config.matchers.iter().filter(|m| m.library).map(|m| m.name.text.as_str()).collect();
    if library.is_empty() && !runtime {
        return None;
    }
    let mut needed = Names::default();

    // What turned-on rules and the config's own items mention.
    for r in config.rules.iter().filter(|r| r.live()) {
        for t in &r.targets {
            if let Target::Field(_, field) = t { needed.insert(&field.text); }
            let name = match t {
                Target::Matcher(n) | Target::Field(n, _) => &n.text,
                Target::Lang(_) => continue,
            };
            needed.insert(name);
            // A rule on a group checks every member.
            for g in config.groups.iter().filter(|g| g.name.text == *name) {
                needed.extend(g.parents.iter().filter_map(parent_name));
            }
        }
        rule_names(r, &mut needed);
    }
    for m in config.matchers.iter().filter(|m| !m.library) {
        needed.insert(&m.name.text);
    }
    for g in config.groups.iter().filter(|g| !g.library) {
        needed.insert(&g.name.text);
        needed.extend(g.parents.iter().filter_map(parent_name));
    }
    for a in config.attrs.iter().filter(|a| !a.library) {
        path_names(&a.targets, &mut needed);
        needed.insert(&a.name.text);
    }
    for f in config.forwards.iter().filter(|f| !f.library) {
        path_names(&f.targets, &mut needed);
        path_names(&f.attrs, &mut needed);
    }
    // Name items always apply, including their helper dependencies.
    for n in &config.names {
        mentioned(&n.mentions, &mut needed, |found| {
            path_names(&n.targets, found);
            if let Some(when) = &n.when {
                expr_names(when, found);
            }
            item_names(&n.item, found);
        });
    }
    for profile in &config.module_resolutions {
        for mode in &profile.modes {
            expr_names(&mode.when, &mut needed);
        }
        for setting in profile.settings.iter().filter(|setting| setting.key.text == "references") {
            let crate::config::ast::modules::SettingValue::Block(kinds) = &setting.value else { continue };
            needed.extend(kinds.iter().map(|kind| kind.key.text.as_str()));
        }
    }
    for shape in &config.products {
        for value in [&shape.inherits, &shape.readonly_inherits, &shape.optional].into_iter().flatten() {
            value_names(value, &mut needed);
        }
    }
    // Protocol methods answer `dxlint map` requests.
    for protocol in &config.protocols {
        needed.extend(protocol.methods.iter().map(|(_, function)| function.as_str()));
    }
    // Option values are checked while the config builds.
    for schema in &config.schemas {
        if let crate::config::schema::Schema::Checked(_, check) = &schema.schema { needed.insert(&check.text); }
    }
    if runtime {
        for c in &config.configs {
            for (_, value) in &c.options {
                expr_names(value, &mut needed);
            }
        }
    }

    // Build each dependency set once; closure traversal must not scan the
    // entire library again for every name.
    let mut dependencies: FxHashMap<&str, Names> = FxHashMap::default();
    for m in &config.matchers {
        let found = dependencies.entry(&m.name.text).or_default();
        if let Some((pat, _)) = &m.pattern {
            mentioned(&m.mentions, found, |found| pattern_names(pat, found));
        }
        found.extend(m.like.iter().map(|l| l.text.as_str()));
        found.extend(m.parents.iter().filter_map(parent_name));
    }
    for a in &config.attrs {
        let found = dependencies.entry(&a.name.text).or_default();
        mentioned(&a.mentions, found, |found| expr_names(&a.expr, found));
        if runtime {
            path_names(&a.targets, found);
        }
    }
    for f in &config.functions {
        mentioned(&f.mentions, dependencies.entry(&f.name.text).or_default(), |found| expr_names(&f.body, found));
    }
    for g in &config.groups {
        dependencies.entry(&g.name.text).or_default().extend(g.parents.iter().filter_map(parent_name));
    }
    if runtime {
        for f in &config.forwards {
            for a in &f.attrs {
                path_names(&f.targets, dependencies.entry(&a.text).or_default());
            }
        }
    }

    let mut queue: Vec<&str> = needed.iter().copied().collect();
    while let Some(name) = queue.pop() {
        for &n in dependencies.get(name).into_iter().flatten() {
            if needed.insert(n) {
                queue.push(n);
            }
        }
    }

    let gone: HashSet<String> = library.iter().filter(|n| !needed.contains(*n)).map(|n| n.to_string()).collect();
    if gone.is_empty() && !runtime {
        return None;
    }
    let unneeded_attrs = config.attrs.iter().filter(|a| a.library && !needed.contains(a.name.text.as_str())).map(|a| a.name.text.clone()).collect();
    let functions = if runtime {
        FunctionPlan::Keep(config.functions.iter().filter(|f| needed.contains(f.name.text.as_str())).map(|f| f.name.text.clone()).collect())
    } else {
        FunctionPlan::Check(config.functions.iter().filter(|f| !needed.contains(f.name.text.as_str()))
            .map(|f| (f.name.text.clone(), dependencies[f.name.text.as_str()].iter().map(|n| n.to_string()).collect()))
            .collect())
    };
    Some(Decision { gone, unneeded_attrs, functions })
}

/// Adds what `find` adds, found once per item (`slot` is shared by every
/// copy of it).
fn mentioned<'a>(slot: &'a Mentions, out: &mut Names<'a>, find: impl FnOnce(&mut Names<'a>)) {
    let names = slot.get_or_init(|| {
        let mut found: Names<'a> = Names::default();
        find(&mut found);
        found.into_iter().map(Box::from).collect()
    });
    out.extend(names.iter().map(|name| &**name));
}

/// Names a rule's clauses mention.
fn rule_names<'a>(r: &'a crate::config::ast::RuleDef, out: &mut Names<'a>) {
    let indent_levels = r.indents.iter().flat_map(|i| i.by.iter().chain(&i.from));
    let fixes = r.fix.iter().flat_map(|f| f.exprs()).chain(r.suggestions.iter().flat_map(|s| s.exprs()));
    for e in r.when.iter().chain(&r.message).chain(fixes).chain(&r.report_at).chain(r.each.as_ref().map(|(_, e)| e)).chain(r.locals.iter().map(|(_, e)| e)).chain(&r.item_when).chain(r.item_locals.iter().map(|(_, e)| e)).chain(indent_levels).chain(&r.indent_reset) {
        expr_names(e, out);
    }
    for i in &r.indents {
        out.extend(i.targets.iter().filter_map(|(p, _)| parent_name(p)));
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::config::{self, source::Disk};
    use crate::engine::build;

    use super::*;

    #[test]
    fn lint_pruning_preserves_function_declaration_errors() {
        for definitions in ["fn unused() = 1\nfn unused() = 2", "fn len(x) = 1"] {
            let source = format!("lang t \"*.t\"\n{definitions}\n");
            let mut loaded = config::load_text("test.dx", &source, &Disk);
            assert!(loaded.errors.is_empty(), "{:#?}", loaded.errors);
            prune_for_lint(&mut loaded.config, &mut loaded.errors);
            let built = build(loaded, PathBuf::from("/root"));
            assert!(!built.errors.is_empty(), "{definitions}");
        }
    }

    #[test]
    fn tooling_pruning_drops_helpers_that_read_pruned_layers() {
        let source = r#"
lang t "*.t"
define {
  in .t match pair /</ close:/>/
  fn closer(n) = n is pair.close
  fn uses_closer(n) = closer(n)
}
in .t match token /X/
in token rule active warn "x"
"#;
        let built = build(config::load_text("test.dx", source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
        assert_eq!(built.linter.unwrap().lint(Path::new("/root/test.t"), "X").len(), 1);
    }
}
