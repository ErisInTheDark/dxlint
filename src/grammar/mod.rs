//! The runtime layer grammar: every matcher compiled, wired to its parents,
//! and pre-resolved per language so parsing a file is pure table lookups.

mod parse;
mod states;
mod text;
mod tree;

pub use parse::{full_match, Edge, Node, Parser, ROOT, TEXT};
pub use tree::Tree;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use regex_automata::meta::Cache;

use crate::config::ast::{Config, MatcherDef, Parent};
use crate::config::error::ConfigError;
use crate::config::source::Span;
use crate::path_globs::PathGlobs;
use crate::pattern::class::CharClass;
use crate::pattern::compile::{compile, Inst, Labels, Program};
use crate::pattern::dfa::{guard_prefix, is_pure, Dfa};
use crate::pattern::prefilter::{first_set, max_len, ByteSet, PairSet, Prefilter, PrevSet};
use crate::pattern::Pat;

pub struct Matcher {
    pub name: String,
    pub span: Span,
    pub pat: Pat,
    pub prog: Program,
    pub first: ByteSet,
    /// Where overlay searches for this matcher may start.
    pub prefilter: Prefilter,
    /// Which first two bytes can start a match, when that is selective.
    pub pairs: Option<PairSet>,
    /// Which byte can come right before a match (from a leading lookbehind).
    pub prev: Option<PrevSet>,
    /// The whole matcher as a DFA, when it is pure regex.
    pub dfa: Option<Dfa>,
    /// A DFA for the pure leading part of an impure matcher: a cheap way to
    /// reject positions before running the interpreter.
    pub guard: Option<Dfa>,
    /// Does the program record labelled text (so nodes need the
    /// interpreter even when a DFA decided the match)?
    pub has_captures: bool,
    /// Does the pattern contain * (which decides where children go)?
    pub has_skip: bool,
    /// The matcher this one specialises (`match x like base`).
    pub like: Option<u32>,
    /// Attached with `over` somewhere: its nodes lie on top of the text.
    pub is_overlay: bool,
    /// Languages this matcher is restricted to; `None` means every language.
    gate: Option<Vec<usize>>,
    children: Vec<u32>,
    overlays: Vec<u32>,
}

impl Matcher {
    /// Field labels of this matcher's pattern, and which matcher each
    /// labelled reference points at.
    pub fn fields(&self) -> Vec<(String, Option<String>)> {
        self.pat.labels().into_iter().map(|l| (l.clone(), self.pat.label_target(&l).map(str::to_string))).collect()
    }
}

pub struct Lang {
    pub name: String,
    pub span: Span,
    pub globs: Vec<String>,
    files: PathGlobs,
    children: Vec<u32>,
    overlays: Vec<u32>,
}

impl Lang {
    /// Does this language own `path` (relative to the project root)?
    pub fn owns(&self, rel_path: &Path) -> bool {
        self.files.matches(rel_path)
    }
}

/// Everything about one language, resolved for fast parsing.
pub struct View {
    children: Vec<Vec<u32>>,
    child_first: Vec<ByteSet>,
    overlays: Vec<Vec<u32>>,
    root_children: Vec<u32>,
    root_prefilter: Prefilter,
    root_overlays: Vec<u32>,
}

pub struct Grammar {
    pub matchers: Vec<Matcher>,
    pub langs: Vec<Lang>,
    pub labels: Labels,
    /// `in x indent` / `in x.field indent`: (matcher, field) pairs whose
    /// insides are indented.
    indents: Vec<(u32, Option<u16>)>,
    /// Every group's final members.
    groups: HashMap<String, Vec<Parent>>,
    views: Vec<View>,
    by_name: HashMap<String, u32>,
    /// Warm DFA caches, reused across files (building one is not cheap).
    cache_pool: Mutex<Vec<DfaCaches>>,
}

/// Per-matcher lazy DFA caches for one parser.
#[derive(Default)]
pub struct DfaCaches {
    pub dfa: Vec<Option<Cache>>,
    pub guard: Vec<Option<Cache>>,
}

impl Grammar {
    pub fn take_caches(&self) -> DfaCaches {
        let pooled = self.cache_pool.lock().ok().and_then(|mut p| p.pop());
        pooled.unwrap_or_else(|| {
            let n = self.matchers.len();
            DfaCaches { dfa: (0..n).map(|_| None).collect(), guard: (0..n).map(|_| None).collect() }
        })
    }

    pub fn give_caches(&self, caches: DfaCaches) {
        if caches.dfa.len() == self.matchers.len() {
            if let Ok(mut pool) = self.cache_pool.lock() {
                pool.push(caches);
            }
        }
    }

    pub fn matcher(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    /// Is matcher `m` `base` itself, or a specialisation of it?
    pub fn is_a(&self, mut m: u32, base: u32) -> bool {
        loop {
            if m == base {
                return true;
            }
            match self.matchers.get(m as usize).and_then(|x| x.like) {
                Some(next) => m = next,
                None => return false,
            }
        }
    }

    /// For every matcher, which matchers can appear inside its nodes (via
    /// children, overlays or references), itself included. Lets the linter
    /// skip overlay searches no active rule could ever see.
    pub fn reach(&self) -> Vec<Vec<bool>> {
        let n = self.matchers.len();
        let edges: Vec<Vec<u32>> = self
            .matchers
            .iter()
            .map(|m| {
                let mut out: Vec<u32> = m.children.iter().chain(&m.overlays).copied().collect();
                m.pat.walk(&mut |p| {
                    if let Pat::Ref { name, .. } = p {
                        out.extend(self.matcher(name));
                    }
                });
                out
            })
            .collect();
        (0..n)
            .map(|start| {
                let mut seen = vec![false; n];
                let mut stack = vec![start as u32];
                while let Some(m) = stack.pop() {
                    if !std::mem::replace(&mut seen[m as usize], true) {
                        stack.extend(&edges[m as usize]);
                    }
                }
                seen
            })
            .collect()
    }

    /// The pattern of a pure matcher (one a DFA can inline).
    pub fn pure_pat(&self, name: &str) -> Option<&Pat> {
        let m = &self.matchers[self.matcher(name)? as usize];
        m.dfa.as_ref().map(|_| &m.pat)
    }

    pub fn lang(&self, name: &str) -> Option<usize> {
        self.langs.iter().position(|l| l.name == name)
    }

    pub fn lang_for(&self, rel_path: &Path) -> Option<usize> {
        // Later definitions win, so users can override std languages.
        self.langs.iter().rposition(|l| l.owns(rel_path))
    }

    pub fn view(&self, lang: usize) -> &View {
        &self.views[lang]
    }

    pub fn build(config: &Config, errors: &mut Vec<ConfigError>) -> Grammar {
        let mut langs: Vec<Lang> = Vec::new();
        for def in &config.langs {
            if let Some(prev) = langs.iter().position(|l| l.name == def.name.text) {
                // Redefining a language replaces its file globs.
                langs.remove(prev);
            }
            let files = match PathGlobs::new(def.globs.iter().map(|(g, _)| g.as_str())) {
                Ok(f) => f,
                Err((glob, e)) => {
                    let span = def.globs.iter().find(|(g, _)| *g == glob).map_or(def.name.span, |(_, s)| *s);
                    errors.push(ConfigError::new(span, format!("invalid glob `{glob}`: {e}")));
                    PathGlobs::default()
                }
            };
            langs.push(Lang {
                name: def.name.text.clone(),
                span: def.name.span,
                globs: def.globs.iter().map(|(g, _)| g.clone()).collect(),
                files,
                children: Vec::new(),
                overlays: Vec::new(),
            });
        }

        // Pass 1: names, so patterns may reference matchers defined later.
        let mut by_name: HashMap<String, u32> = HashMap::new();
        let mut defs = Vec::new();
        for def in &config.matchers {
            let Some((pat, span)) = &def.pattern else { continue };
            if let Some(&prev) = by_name.get(&def.name.text) {
                let prev_span = config.matchers.iter().filter(|d| d.pattern.is_some()).nth(prev as usize).map(|d| d.name.span);
                let mut err = ConfigError::new(def.name.span, format!("matcher `{}` is defined twice", def.name.text));
                if prev_span.is_some() {
                    err = err.note("use `in parent match name` (no pattern) to attach an existing matcher to more parents");
                }
                errors.push(err);
                continue;
            }
            by_name.insert(def.name.text.clone(), defs.len() as u32);
            defs.push((def, pat, *span));
        }

        // Pass 2: compile.
        let mut labels = Labels::default();
        let mut matchers = Vec::with_capacity(defs.len());
        for (def, pat, span) in &defs {
            let mut pat = (*pat).clone();
            let prog = match compile(&pat, &by_name, &mut labels) {
                Ok(p) => p,
                Err(e) => {
                    let mut err = ConfigError::new(*span, format!("in matcher `{}`: {e}", def.name.text));
                    if let Some(unknown) = e.strip_prefix("unknown matcher `").and_then(|r| r.strip_suffix('`')) {
                        err = err.suggest(unknown, by_name.keys().map(String::as_str));
                    }
                    errors.push(err);
                    // The rest of the config still lints: this matcher just
                    // never matches.
                    pat = Pat::Class(CharClass::new(Vec::new(), Vec::new(), false));
                    compile(&pat, &by_name, &mut labels).unwrap_or_default()
                }
            };
            // Patterns may be able to match empty text (the engine skips
            // empty matches), but not *only* empty text.
            if max_len(&pat) == Some(0) {
                errors.push(
                    ConfigError::new(*span, format!("matcher `{}` can only match empty text", def.name.text))
                        .note("layers never match empty text, so this matcher would never match"),
                );
            }
            let prev = PrevSet::build(&pat);
            matchers.push(Matcher {
                name: def.name.text.clone(),
                span: def.name.span,
                pat,
                prog,
                first: ByteSet::default(),
                prefilter: Prefilter::All,
                pairs: None,
                prev,
                dfa: None,
                guard: None,
                has_captures: false,
                has_skip: false,
                like: None,
                is_overlay: false,
                gate: Some(Vec::new()),
                children: Vec::new(),
                overlays: Vec::new(),
            });
        }

        // Pass 3: parents (definitions and `in x match y` attachments), with
        // groups resolved to their final members.
        let groups = resolve_groups(config, &by_name, errors);
        let mut ungated = vec![false; matchers.len()];
        for def in &config.matchers {
            let Some(&id) = by_name.get(&def.name.text) else {
                if def.pattern.is_none() {
                    errors.push(
                        ConfigError::new(def.name.span, format!("unknown matcher `{}`", def.name.text))
                            .suggest(&def.name.text, by_name.keys().map(String::as_str)),
                    );
                }
                continue;
            };
            let parents: Vec<&Parent> = def
                .parents
                .iter()
                .flat_map(|p| match p {
                    Parent::Matcher(n) if groups.contains_key(&n.text) => groups[&n.text].iter().collect(),
                    other => vec![other],
                })
                .collect();
            if def.overlay && !def.parents.is_empty() {
                matchers[id as usize].is_overlay = true;
            }
            for parent in parents {
                match parent {
                    Parent::Lang(name) => match langs.iter().position(|l| l.name == name.text) {
                        Some(li) => {
                            let lang = &mut langs[li];
                            push_unique(if def.overlay { &mut lang.overlays } else { &mut lang.children }, id);
                            if let Some(gate) = &mut matchers[id as usize].gate {
                                if !gate.contains(&li) {
                                    gate.push(li);
                                }
                            }
                        }
                        None => errors.push(
                            ConfigError::new(name.span, format!("unknown language `.{}`", name.text))
                                .note("declare it with `lang name = \"*.ext\"`")
                                .suggest(&name.text, langs.iter().map(|l| l.name.as_str())),
                        ),
                    },
                    Parent::AnyLang(_) => {
                        for lang in &mut langs {
                            push_unique(if def.overlay { &mut lang.overlays } else { &mut lang.children }, id);
                        }
                        ungated[id as usize] = true;
                    }
                    Parent::Matcher(name) => match by_name.get(&name.text) {
                        Some(&pid) => {
                            let p = &mut matchers[pid as usize];
                            push_unique(if def.overlay { &mut p.overlays } else { &mut p.children }, id);
                        }
                        None => errors.push(
                            ConfigError::new(name.span, format!("unknown parent matcher `{}`", name.text))
                                .suggest(&name.text, by_name.keys().map(String::as_str)),
                        ),
                    },
                }
            }
        }
        for (m, ungated) in matchers.iter_mut().zip(ungated) {
            if ungated || m.gate.as_ref().is_some_and(Vec::is_empty) {
                m.gate = None;
            }
        }
        resolve_likes(&mut matchers, &defs, &by_name, errors);
        let indents = resolve_indents(config, &groups, &by_name, &matchers, &labels, errors);

        // First-byte sets: a fixpoint, since references can be mutual.
        loop {
            let mut changed = false;
            for i in 0..matchers.len() {
                let mut set = ByteSet::default();
                let refs = |name: &str| by_name.get(name).map(|&j| matchers[j as usize].first.clone()).unwrap_or_default();
                first_set(&matchers[i].pat, &refs, &mut set);
                if set != matchers[i].first {
                    matchers[i].first = set;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for i in 0..matchers.len() {
            let refs = |name: &str| by_name.get(name).map(|&j| matchers[j as usize].first.clone()).unwrap_or_default();
            let pre = Prefilter::build(&matchers[i].pat, &refs);
            let pairs = PairSet::build(&matchers[i].pat, &refs);
            matchers[i].prefilter = pre;
            matchers[i].pairs = pairs;
        }

        compile_fast_paths(&mut matchers, &by_name, errors);
        check_left_recursion(&matchers, &by_name, errors);

        let views = (0..langs.len()).map(|li| build_view(li, &langs[li], &matchers)).collect();
        Grammar { matchers, langs, labels, indents, groups, views, by_name, cache_pool: Mutex::new(Vec::new()) }
    }

    /// A group's members (`group name a or .lang`), nested groups flattened.
    pub fn group(&self, name: &str) -> Option<&[Parent]> {
        self.groups.get(name).map(Vec::as_slice)
    }

    /// Is `node` (of `matcher`, labelled `label` inside a node of `owner`)
    /// declared indenting? Specialisations count as their base.
    pub fn indents(&self, matcher: u32, label: Option<u16>, owner: Option<u32>) -> bool {
        self.indents.iter().any(|&(m, field)| match field {
            None => matcher != ROOT && matcher != TEXT && self.is_a(matcher, m),
            Some(f) => label == Some(f) && owner.is_some_and(|o| self.is_a(o, m)),
        })
    }
}

/// `in a or b.field indent` targets, with groups expanded.
fn resolve_indents(
    config: &Config,
    groups: &HashMap<String, Vec<Parent>>,
    by_name: &HashMap<String, u32>,
    matchers: &[Matcher],
    labels: &Labels,
    errors: &mut Vec<ConfigError>,
) -> Vec<(u32, Option<u16>)> {
    let mut out = Vec::new();
    for def in &config.indents {
        for (parent, field) in &def.targets {
            let members: Vec<&Parent> = match parent {
                Parent::Matcher(n) if groups.contains_key(&n.text) => groups[&n.text].iter().collect(),
                other => vec![other],
            };
            for member in members {
                let name = match member {
                    Parent::Matcher(name) => name,
                    Parent::Lang(name) => {
                        errors.push(ConfigError::new(name.span, "a language root has no inside to indent").note("indent its brackets or blocks instead"));
                        continue;
                    }
                    Parent::AnyLang(span) => {
                        errors.push(ConfigError::new(*span, "a language root has no inside to indent").note("indent its brackets or blocks instead"));
                        continue;
                    }
                };
                let Some(&id) = by_name.get(&name.text) else {
                    let err = ConfigError::new(name.span, format!("unknown matcher `{}`", name.text));
                    errors.push(err.suggest(&name.text, by_name.keys().map(String::as_str)));
                    continue;
                };
                let label = match field {
                    None => None,
                    Some(f) => {
                        let has = matchers[id as usize].fields().iter().any(|(l, _)| *l == f.text);
                        match labels.get(&f.text).filter(|_| has) {
                            Some(l) => Some(l),
                            None => {
                                errors.push(ConfigError::new(f.span, format!("`{}` has no field `{}`", name.text, f.text)));
                                continue;
                            }
                        }
                    }
                };
                if !out.contains(&(id, label)) {
                    out.push((id, label));
                }
            }
        }
    }
    out
}

/// DFAs for pure matchers and guards, and `*` fast-forward stop sets.
fn compile_fast_paths(matchers: &mut [Matcher], by_name: &HashMap<String, u32>, errors: &mut Vec<ConfigError>) {
    // A DFA cannot skip an empty match to find a longer one, so patterns
    // that could match empty always run on the interpreter.
    let pure: Vec<bool> = matchers.iter().map(|m| is_pure(&m.pat) && !m.pat.nullable()).collect();
    let mut built = Vec::with_capacity(matchers.len());
    {
        let ms: &[Matcher] = matchers;
        let resolve = |name: &str| by_name.get(name).filter(|&&j| pure[j as usize]).map(|&j| &ms[j as usize].pat);
        for (i, m) in ms.iter().enumerate() {
            let result = if pure[i] {
                Dfa::new(&m.pat, &resolve).map(|d| (Some(d), None))
            } else {
                guard_prefix(&m.pat, &resolve).map(|g| Dfa::new(&g, &resolve)).transpose().map(|g| (None, g))
            };
            match result {
                Ok(pair) => built.push(pair),
                Err(e) => {
                    errors.push(ConfigError::new(m.span, format!("matcher `{}` cannot compile: {e}", m.name)));
                    built.push((None, None));
                }
            }
        }
    }
    let firsts: Vec<ByteSet> = matchers.iter().map(|m| m.first.clone()).collect();
    for (m, (dfa, guard)) in matchers.iter_mut().zip(built) {
        m.dfa = dfa;
        m.guard = guard;
        m.has_captures = m.prog.insts.iter().any(|i| matches!(i, Inst::Save(_)));
        m.has_skip = m.prog.insts.iter().any(|i| matches!(i, Inst::Skip(_)));
        let call_first = |id: u32| firsts[id as usize].clone();
        for pc in 1..m.prog.insts.len() {
            if let (Inst::Skip(idx), Inst::Split(after, _)) = (m.prog.insts[pc], m.prog.insts[pc - 1]) {
                m.prog.skip_stops[idx as usize] = m.prog.first_from(after as usize, &call_first);
            }
        }
    }
}

/// Final members of every `group`, with nested groups flattened. Every
/// `group name ...` line adds members. A group cannot share a name with a
/// matcher.
fn resolve_groups(config: &Config, by_name: &HashMap<String, u32>, errors: &mut Vec<ConfigError>) -> HashMap<String, Vec<Parent>> {
    let mut raw: HashMap<String, Vec<Parent>> = HashMap::new();
    for g in &config.groups {
        if by_name.contains_key(&g.name.text) {
            errors.push(ConfigError::new(g.name.span, format!("`{}` is already a matcher; groups need their own name", g.name.text)));
            continue;
        }
        raw.entry(g.name.text.clone()).or_default().extend(g.parents.iter().cloned());
    }
    fn flatten(name: &str, raw: &HashMap<String, Vec<Parent>>, seen: &mut Vec<String>, out: &mut Vec<Parent>) {
        if seen.iter().any(|s| s == name) {
            return;
        }
        seen.push(name.to_string());
        for p in &raw[name] {
            match p {
                Parent::Matcher(n) if raw.contains_key(&n.text) => flatten(&n.text, raw, seen, out),
                other => out.push(other.clone()),
            }
        }
    }
    raw.keys()
        .map(|name| {
            let mut out = Vec::new();
            flatten(name, &raw, &mut Vec::new(), &mut out);
            (name.clone(), out)
        })
        .collect()
}

/// Resolves `like` bases and gives every specialisation its bases'
/// children and overlays (own ones first, so they win ties).
fn resolve_likes(matchers: &mut [Matcher], defs: &[(&MatcherDef, &Pat, Span)], by_name: &HashMap<String, u32>, errors: &mut Vec<ConfigError>) {
    for (i, (def, _, _)) in defs.iter().enumerate() {
        let Some(base) = &def.like else { continue };
        match by_name.get(&base.text) {
            Some(&b) => matchers[i].like = Some(b),
            None => errors.push(
                ConfigError::new(base.span, format!("unknown matcher `{}`", base.text)).suggest(&base.text, by_name.keys().map(String::as_str)),
            ),
        }
    }
    // Reject cycles before walking chains.
    for start in 0..matchers.len() {
        let mut seen = vec![false; matchers.len()];
        let mut m = start;
        while let Some(next) = matchers[m].like {
            if std::mem::replace(&mut seen[m], true) {
                errors.push(ConfigError::new(matchers[start].span, format!("matcher `{}` is `like` itself", matchers[start].name)));
                matchers[start].like = None;
                break;
            }
            m = next as usize;
        }
    }
    let originals: Vec<(Vec<u32>, Vec<u32>)> = matchers.iter().map(|m| (m.children.clone(), m.overlays.clone())).collect();
    for i in 0..matchers.len() {
        let mut base = matchers[i].like;
        while let Some(b) = base {
            let (children, overlays) = &originals[b as usize];
            for &c in children {
                push_unique(&mut matchers[i].children, c);
            }
            for &o in overlays {
                push_unique(&mut matchers[i].overlays, o);
            }
            base = matchers[b as usize].like;
        }
    }
}

fn push_unique(list: &mut Vec<u32>, id: u32) {
    if !list.contains(&id) {
        list.push(id);
    }
}

fn active(m: &Matcher, lang: usize) -> bool {
    m.gate.as_ref().is_none_or(|g| g.contains(&lang))
}

fn build_view(li: usize, lang: &Lang, matchers: &[Matcher]) -> View {
    let gated = |list: &[u32]| -> Vec<u32> { list.iter().copied().filter(|&c| active(&matchers[c as usize], li)).collect() };
    let union = |list: &[u32]| {
        let mut set = ByteSet::default();
        for &c in list {
            set.union(&matchers[c as usize].first);
        }
        set
    };
    let children: Vec<Vec<u32>> = matchers.iter().map(|m| gated(&m.children)).collect();
    let child_first = children.iter().map(|c| union(c)).collect();
    let root_children = gated(&lang.children);
    View {
        children,
        child_first,
        overlays: matchers.iter().map(|m| gated(&m.overlays)).collect(),
        root_prefilter: Prefilter::from_set(&union(&root_children)),
        root_children,
        root_overlays: gated(&lang.overlays),
    }
}

/// Matchers that can call themselves without consuming text would loop
/// forever; reject them up front with the cycle spelled out.
fn check_left_recursion(matchers: &[Matcher], by_name: &HashMap<String, u32>, errors: &mut Vec<ConfigError>) {
    let leading: Vec<Vec<u32>> = matchers
        .iter()
        .map(|m| {
            let mut out = Vec::new();
            leading_calls(&m.pat, by_name, &m.children, &mut out);
            out
        })
        .collect();
    let mut state = vec![0u8; matchers.len()]; // 0 new, 1 on stack, 2 done
    let mut path = Vec::new();
    for start in 0..matchers.len() {
        if let Some(cycle) = visit(start as u32, &leading, &mut state, &mut path) {
            let names: Vec<&str> = cycle.iter().map(|&i| matchers[i as usize].name.as_str()).collect();
            errors.push(
                ConfigError::new(matchers[cycle[0] as usize].span, format!("left recursion: {}", names.join(" -> ")))
                    .note("a matcher may only reach itself after consuming text; use `in parent match child` for nesting"),
            );
            return;
        }
    }
}

fn visit(i: u32, leading: &[Vec<u32>], state: &mut [u8], path: &mut Vec<u32>) -> Option<Vec<u32>> {
    match state[i as usize] {
        1 => {
            let from = path.iter().position(|&p| p == i).unwrap_or(0);
            let mut cycle = path[from..].to_vec();
            cycle.push(i);
            return Some(cycle);
        }
        2 => return None,
        _ => {}
    }
    state[i as usize] = 1;
    path.push(i);
    for &next in &leading[i as usize] {
        if let Some(c) = visit(next, leading, state, path) {
            return Some(c);
        }
    }
    path.pop();
    state[i as usize] = 2;
    None
}

/// Matchers `pat` may call at its very first position. Returns nullable.
fn leading_calls(pat: &Pat, by_name: &HashMap<String, u32>, children: &[u32], out: &mut Vec<u32>) -> bool {
    match pat {
        Pat::Ref { name, .. } => {
            if let Some(&id) = by_name.get(name) {
                out.push(id);
            }
            false
        }
        Pat::Skip => {
            out.extend_from_slice(children);
            true
        }
        Pat::Seq(items) => items.iter().all(|p| leading_calls(p, by_name, children, out)),
        Pat::Alt(items) => items.iter().fold(false, |n, p| leading_calls(p, by_name, children, out) | n),
        Pat::Repeat { inner, min, .. } => leading_calls(inner, by_name, children, out) || *min == 0,
        Pat::Capture { inner, .. } => leading_calls(inner, by_name, children, out),
        other => other.nullable(),
    }
}
