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

use crate::config::ast::{Config, Expr, ExprKind, Parent, Target, TemplatePart};
use crate::pattern::Pat;

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
        ExprKind::Key(base, _) => expr_names(base, out),
        ExprKind::Var(path) => out.extend(path.iter().map(|n| n.text.clone())),
        ExprKind::Call(_, args) => args.iter().for_each(|a| expr_names(a, out)),
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
            out.extend(names.iter().map(|n| n.text.clone()));
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
    let library: HashSet<&str> = config.matchers.iter().filter(|m| m.library).map(|m| m.name.text.as_str()).collect();
    if library.is_empty() {
        return;
    }
    let groups: HashSet<&str> = config.groups.iter().map(|g| g.name.text.as_str()).collect();
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
    for i in config.indents.iter().filter(|i| !i.library) {
        needed.extend(i.targets.iter().filter_map(|(p, _)| parent_name(p)).map(str::to_string));
    }
    for f in config.forwards.iter().filter(|f| !f.library) {
        needed.extend(f.targets.iter().map(|t| t.text.clone()));
        needed.extend(f.attrs.iter().map(|a| a.text.clone()));
    }

    // What needed layers and attributes need in turn.
    let mut queue: Vec<String> = needed.iter().cloned().collect();
    while let Some(name) = queue.pop() {
        let mut found = HashSet::new();
        for m in config.matchers.iter().filter(|m| m.name.text == name) {
            if let Some((pat, _)) = &m.pattern {
                pattern_names(pat, &mut found);
            }
            found.extend(m.like.iter().map(|l| l.text.clone()));
            // Its parents keep the tree it sits in as it would be with every
            // library layer present; a group parent means all its members.
            for p in m.parents.iter().filter_map(parent_name) {
                found.insert(p.to_string());
                if groups.contains(p) {
                    for g in config.groups.iter().filter(|g| g.name.text == p) {
                        found.extend(g.parents.iter().filter_map(parent_name).map(str::to_string));
                    }
                }
            }
        }
        for a in config.attrs.iter().filter(|a| a.name.text == name) {
            expr_names(&a.expr, &mut found);
        }
        for n in found {
            if needed.insert(n.clone()) {
                queue.push(n);
            }
        }
    }

    let pruned: HashSet<String> = library.iter().filter(|n| !needed.contains(**n)).map(|n| n.to_string()).collect();
    if pruned.is_empty() {
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
    for i in &mut config.indents {
        i.targets.retain(|(p, _)| parent_name(p).is_none_or(|n| !gone(n)));
    }
    config.indents.retain(|i| !i.targets.is_empty());
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

    for r in config.rules.iter_mut().filter(|r| !r.enabled()) {
        let mut names = HashSet::new();
        for t in &r.targets {
            if let Target::Matcher(n) | Target::Field(n, _) = t {
                names.insert(n.text.clone());
            }
        }
        rule_names(r, &mut names);
        r.inert = names.iter().any(|n| gone(n) || dropped_attrs.contains(n));
    }
}

/// Names a rule's clauses mention.
fn rule_names(r: &crate::config::ast::RuleDef, out: &mut HashSet<String>) {
    for e in r.when.iter().chain(&r.message).chain(&r.fix).chain(r.locals.iter().map(|(_, e)| e)) {
        expr_names(e, out);
    }
}
