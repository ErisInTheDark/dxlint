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

use std::collections::{HashMap, HashSet};

use crate::config::ast::{Config, Expr, ExprKind, Parent, Target, TemplatePart};
use crate::pattern::Pat;

/// Runtime compilation keeps only helpers reachable from enabled rules
/// and explicit grammar items. Tooling still uses [`prune`].
pub fn prune_for_lint(config: &mut Config, errors: &mut Vec<crate::config::error::ConfigError>) {
    crate::grammar::function_declarations(config, errors);
    prune_config(config, true);
}

/// Every name an expression mentions: path segments, function arguments,
/// `is` names, pattern references and string literals. An over-estimate,
/// which only ever keeps more.
fn expr_names(e: &Expr, out: &mut HashSet<String>) {
    match &e.kind {
        ExprKind::Str(s) => {
            out.insert(s.clone());
        }
        ExprKind::Template(parts) => {
            for p in parts {
                match p {
                    TemplatePart::Lit(s) => {
                        out.insert(s.clone());
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
            out.insert(key.text.clone());
        }
        ExprKind::Var(path) => out.extend(path.iter().map(|n| n.text.clone())),
        // A config `fn` needs what its body reads.
        ExprKind::Call(name, args) => {
            out.insert(name.text.clone());
            args.iter().for_each(|a| expr_names(a, out));
        }
        ExprKind::Unary(_, a) => expr_names(a, out),
        ExprKind::Binary(_, a, b) => {
            expr_names(a, out);
            expr_names(b, out);
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
            out.extend(names.iter().filter_map(|n| n.kind()).map(|n| n.text.clone()));
        }
    }
}

fn pattern_names(pat: &Pat, out: &mut HashSet<String>) {
    pat.walk(&mut |p| {
        if let Pat::Ref { name, .. } = p {
            out.insert(name.clone());
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

fn prune_config(config: &mut Config, runtime: bool) {
    let library: HashSet<&str> = config.matchers.iter().filter(|m| m.library).map(|m| m.name.text.as_str()).collect();
    if library.is_empty() && !runtime {
        return;
    }
    let mut needed: HashSet<String> = HashSet::new();

    // What turned-on rules and the config's own items mention.
    for r in config.rules.iter().filter(|r| r.enabled()) {
        for t in &r.targets {
            let name = match t {
                Target::Matcher(n) | Target::Field(n, _) => &n.text,
                Target::Lang(_) => continue,
            };
            needed.insert(name.clone());
            // A rule on a group checks every member.
            for g in config.groups.iter().filter(|g| g.name.text == *name) {
                needed.extend(g.parents.iter().filter_map(parent_name).map(str::to_string));
            }
        }
        rule_names(r, &mut needed);
    }
    for m in config.matchers.iter().filter(|m| !m.library) {
        needed.insert(m.name.text.clone());
    }
    for g in config.groups.iter().filter(|g| !g.library) {
        needed.insert(g.name.text.clone());
        needed.extend(g.parents.iter().filter_map(parent_name).map(str::to_string));
    }
    for a in config.attrs.iter().filter(|a| !a.library) {
        needed.extend(a.targets.iter().map(|t| t.text.clone()));
        needed.insert(a.name.text.clone());
    }
    for f in config.forwards.iter().filter(|f| !f.library) {
        needed.extend(f.targets.iter().map(|t| t.text.clone()));
        needed.extend(f.attrs.iter().map(|a| a.text.clone()));
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
    let mut dependencies: HashMap<String, HashSet<String>> = HashMap::new();
    for m in &config.matchers {
        let found = dependencies.entry(m.name.text.clone()).or_default();
        if let Some((pat, _)) = &m.pattern {
            pattern_names(pat, found);
        }
        found.extend(m.like.iter().map(|l| l.text.clone()));
        found.extend(m.parents.iter().filter_map(parent_name).map(str::to_string));
    }
    for a in &config.attrs {
        let found = dependencies.entry(a.name.text.clone()).or_default();
        expr_names(&a.expr, found);
        if runtime {
            found.extend(a.targets.iter().map(|t| t.text.clone()));
        }
    }
    for f in &config.functions {
        expr_names(&f.body, dependencies.entry(f.name.text.clone()).or_default());
    }
    for g in &config.groups {
        dependencies.entry(g.name.text.clone()).or_default().extend(g.parents.iter().filter_map(parent_name).map(str::to_string));
    }
    if runtime {
        for f in &config.forwards {
            for a in &f.attrs {
                dependencies.entry(a.text.clone()).or_default().extend(f.targets.iter().map(|t| t.text.clone()));
            }
        }
    }

    let mut queue: Vec<String> = needed.iter().cloned().collect();
    while let Some(name) = queue.pop() {
        for n in dependencies.get(&name).into_iter().flatten() {
            if needed.insert(n.clone()) {
                queue.push(n.clone());
            }
        }
    }

    let pruned: HashSet<String> = library.iter().filter(|n| !needed.contains(**n)).map(|n| n.to_string()).collect();
    if pruned.is_empty() && !runtime {
        return;
    }
    let gone = |n: &str| pruned.contains(n);
    config.matchers.retain(|m| !gone(&m.name.text));
    for m in &mut config.matchers {
        m.parents.retain(|p| parent_name(p).is_none_or(|n| !gone(n)));
    }
    for g in &mut config.groups {
        g.parents.retain(|p| parent_name(p).is_none_or(|n| !gone(n)));
    }
    let mut dropped_attrs: HashSet<String> = HashSet::new();
    for a in &mut config.attrs {
        if a.library && !needed.contains(&a.name.text) {
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
    if runtime {
        config.functions.retain(|f| needed.contains(&f.name.text));
    } else {
        // Tooling keeps unneeded helpers, but not ones that read (or call
        // helpers that read) a pruned layer or attribute: they cannot compile.
        let mut unusable: HashSet<String> = HashSet::new();
        loop {
            let before = unusable.len();
            for f in config.functions.iter().filter(|f| !needed.contains(&f.name.text)) {
                let reads = &dependencies[&f.name.text];
                if reads.iter().any(|n| gone(n) || dropped_attrs.contains(n) || unusable.contains(n)) {
                    unusable.insert(f.name.text.clone());
                }
            }
            if unusable.len() == before {
                break;
            }
        }
        config.functions.retain(|f| !unusable.contains(&f.name.text));
    }

    for r in config.rules.iter_mut().filter(|r| !r.enabled()) {
        let mut names = HashSet::new();
        for t in &r.targets {
            if let Target::Matcher(n) | Target::Field(n, _) = t {
                names.insert(n.text.clone());
            }
        }
        rule_names(r, &mut names);
        r.inert = runtime || names.iter().any(|n| gone(n) || dropped_attrs.contains(n));
    }
}

/// Names a rule's clauses mention.
fn rule_names(r: &crate::config::ast::RuleDef, out: &mut HashSet<String>) {
    let indent_levels = r.indents.iter().flat_map(|i| i.by.iter().chain(&i.from));
    for e in r.when.iter().chain(&r.message).chain(&r.fix).chain(&r.fix_when).chain(&r.fix_at).chain(&r.report_at).chain(r.each.as_ref().map(|(_, e)| e)).chain(r.locals.iter().map(|(_, e)| e)).chain(indent_levels).chain(&r.indent_reset) {
        expr_names(e, out);
    }
    for i in &r.indents {
        out.extend(i.targets.iter().filter_map(|(p, _)| parent_name(p)).map(str::to_string));
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::config::{self, source::Disk};
    use crate::engine::build;

    use super::*;

    #[test]
    fn lint_pruning_skips_unreachable_helpers_without_removable_layers() {
        for definitions in [
            "fn dormant() = missing()\nin .t match token /X/",
            "define {\n in .t match token /X/\n in token let dormant missing()\n}",
        ] {
            let source = format!("lang t \"*.t\"\n{definitions}\nin token rule active warn \"x\"\n");
            let mut loaded = config::load_text("test.zlint", &source, &Disk);
            assert!(loaded.errors.is_empty(), "{:#?}", loaded.errors);
            prune_for_lint(&mut loaded.config, &mut loaded.errors);
            let built = build(loaded, PathBuf::from("/root"));
            assert!(built.errors.is_empty(), "{definitions}: {:#?}", built.errors);
            let diagnostics = built.linter.unwrap().lint(Path::new("/root/test.t"), "X");
            assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        }
    }

    #[test]
    fn lint_pruning_preserves_indirect_attributes_groups_and_forwards() {
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
}
in wrapper rule active warn "{read(node)}" fix as lower(text) { when accepted(node) }
"#;
        let mut loaded = config::load_text("test.zlint", source, &Disk);
        assert!(loaded.errors.is_empty(), "{:#?}", loaded.errors);
        prune_for_lint(&mut loaded.config, &mut loaded.errors);
        let built = build(loaded, PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
        let linter = built.linter.unwrap();
        let path = Path::new("/root/test.t");
        let diagnostics = linter.lint(path, "<X>");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].message, "X");
        assert_eq!(linter.fix(path, "<X>").text, "<x>");
    }

    #[test]
    fn lint_pruning_preserves_function_declaration_errors() {
        for definitions in ["fn unused() = 1\nfn unused() = 2", "fn len(x) = 1"] {
            let source = format!("lang t \"*.t\"\n{definitions}\n");
            let mut loaded = config::load_text("test.zlint", &source, &Disk);
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
        let built = build(config::load_text("test.zlint", source, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:#?}", built.errors);
        assert_eq!(built.linter.unwrap().lint(Path::new("/root/test.t"), "X").len(), 1);
    }
}
