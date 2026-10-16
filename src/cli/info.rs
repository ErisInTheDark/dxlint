//! `zlint rules`, `zlint tree`, `zlint functions`.

use std::path::Path;
use anstyle::{AnsiColor, Style};

use super::{render, Output};
use crate::config::ast::Severity;
use crate::config::source::{Disk, SourceProvider};
use crate::engine::symbols::Symbols;
use crate::engine::Linter;
use crate::expr::show;
use crate::expr::functions::FUNCTIONS;
use crate::expr::Builtin;
use crate::grammar::Tree;

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const CYAN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)));

/// `zlint rules`: every rule the config defines, the ones that are off
/// included (with the severity `rule name` turns them on at).
pub fn rules(output: &mut Output<'_>, linter: &Linter) -> u8 {
    enum Row<'a> {
        Compiled(&'a crate::engine::Rule),
        Inert(&'a crate::engine::InertRule),
    }
    let mut rows: Vec<(&str, Row)> = linter.rules.iter().map(|r| (r.name.as_str(), Row::Compiled(r))).collect();
    rows.extend(linter.inert.iter().map(|r| (r.name.as_str(), Row::Inert(r))));
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let off = |default: Severity| format!("{DIM}off (default {}){DIM:#}", default.as_str());
    for (_, row) in rows {
        let f = match row {
            Row::Inert(r) => {
                output.out(format_args!("{BOLD}{}{BOLD:#} {} {DIM}in{DIM:#} {CYAN}{}{CYAN:#}", r.name, off(r.default_severity), r.target_text));
                continue;
            }
            Row::Compiled(f) => f,
        };
        let sev = render::severity_style(f.severity);
        let sev_text = if f.severity == Severity::Off { off(f.default_severity) } else { format!("{sev}{}{sev:#}", f.severity.as_str()) };
        let fix = if f.has_fix() { format!(" {DIM}fixable{DIM:#}") } else { String::new() };
        output.out(format_args!("{BOLD}{}{BOLD:#} {sev_text} {DIM}in{DIM:#} {CYAN}{}{CYAN:#}{fix}", f.name, f.target_text));
        if let Some(d) = f.message_template() {
            output.out(format_args!("    {d}"));
        }
        for o in f.options() {
            let changed = if o.value != o.default { format!(" {DIM}(default {}){DIM:#}", show(&o.default)) } else { String::new() };
            let desc = o.description.as_deref().map(|d| format!(" {DIM}- {d}{DIM:#}")).unwrap_or_default();
            output.out(format_args!("    {CYAN}{}{CYAN:#}{DIM}: {}{DIM:#} = {}{changed}{desc}", o.name, o.schema, show(&o.value)));
        }
    }
    0
}

/// `zlint rules --dependencies`: per enabled rule, one JSON line with the
/// file defining it and every config file its compiled checks read: the
/// attributes, functions and layers they reach, and for rules asking type
/// questions every library file (their declarations answer them).
pub fn rule_dependencies(output: &mut Output<'_>, linter: &Linter, symbols: &Symbols) -> u8 {
    use crate::engine::symbols::Kind;
    use crate::expr::CExpr;
    let file_name = |file: crate::config::source::FileId| {
        let source = linter.sources.get(file);
        let library = source.name.starts_with("std/") || source.name.starts_with("rules/");
        if library { return format!("{}.zlint", source.name) }
        // Config files as paths relative to the config's root, `/`-separated.
        let path = source.path.as_deref().unwrap_or(std::path::Path::new(&source.name));
        let relative = path.strip_prefix(&linter.root).unwrap_or(path);
        relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
    };
    let library: Vec<String> = linter.sources.files()
        .filter(|(_, source)| source.name.starts_with("std/"))
        .map(|(file, _)| file_name(file)).collect();
    let files_of = |kind: Kind, name: &str, into: &mut std::collections::BTreeSet<String>| {
        if let Some(id) = symbols.get(kind, name) {
            into.extend(symbols.symbols[id].defs.iter().map(|span| file_name(span.file)));
        }
    };
    // A layer's nodes depend on every layer that can hold them, every
    // layer inside them and their specialisations.
    let grammar = &linter.grammar;
    let reach_table = grammar.reach();
    let reach = &reach_table;
    let related =|layer: u32| (0..grammar.matchers.len() as u32).filter(move |&other| {
        reach[other as usize][layer as usize] || reach[layer as usize][other as usize] || grammar.is_a(other, layer)
    });
    let mut rules: Vec<_> = linter.rules.iter().filter(|rule| rule.severity != Severity::Off).collect();
    rules.sort_by(|a, b| a.name.cmp(&b.name));
    for rule in rules {
        let mut depends = std::collections::BTreeSet::new();
        let mut semantic = false;
        let mut attrs = std::collections::BTreeSet::new();
        let mut functions = std::collections::BTreeSet::new();
        let mut layers: std::collections::BTreeSet<u32> = rule.layers().collect();
        let mut pending: Vec<&CExpr> = rule.expressions().collect();
        while let Some(expr) = pending.pop() {
            match expr {
                CExpr::Attr(id) | CExpr::FieldAttr(_, id) | CExpr::ParentAttr(id) if attrs.insert(*id) => {
                    if let Some((name, defs)) = linter.attrs.definition(*id) {
                        files_of(Kind::Attr, name, &mut depends);
                        pending.extend(defs);
                    }
                }
                CExpr::UserCall(id, _) if functions.insert(*id) => {
                    if let Some(function) = grammar.functions.get(*id as usize) {
                        files_of(Kind::Function, &function.name, &mut depends);
                        pending.extend(function.body.get());
                    }
                }
                CExpr::Trait(..) | CExpr::Semantic(..) | CExpr::QueryCall(..) => semantic = true,
                CExpr::Ancestor(matcher, _) | CExpr::OnAncestor(matcher, _) => { layers.insert(*matcher); }
                CExpr::OwnChild(_, matchers, _) => layers.extend(matchers),
                CExpr::Descendants(selection) | CExpr::DescendantsOf(selection, _) | CExpr::Overlapping(selection, _)
                | CExpr::Containing(selection) => layers.extend(selection.kinds()),
                CExpr::Is(_, tests) => for test in tests {
                    if let crate::expr::IsTest::Kind(matchers) | crate::expr::IsTest::Field(matchers, _) = test { layers.extend(matchers) }
                },
                _ => {}
            }
            expr.for_each_child(&mut |child| pending.push(child));
        }
        let layers: std::collections::BTreeSet<u32> = layers.into_iter().flat_map(|layer| related(layer)).collect();
        for layer in layers {
            if let Some(matcher) = grammar.matchers.get(layer as usize) {
                files_of(Kind::Layer, &matcher.name, &mut depends);
            }
        }
        if semantic { depends.extend(library.iter().cloned()) }
        let file = file_name(rule.span.file);
        depends.remove(&file);
        let row = serde_json::json!({ "rule": rule.name, "file": file, "depends": depends });
        output.out(format_args!("{row}"));
    }
    0
}

/// `zlint describe name`: everything of that name, as hover shows it, and
/// where each is used.
pub fn describe(output: &mut Output<'_>, symbols: &Symbols, name: &str) -> u8 {
    let found = symbols.named(name);
    if found.is_empty() {
        render::error(output, &format!("nothing named `{name}` in this configuration"));
        let names = symbols.symbols.iter().map(|s| s.name.as_str());
        if let Some(best) = crate::config::error::closest(name, names) {
            output.err(format_args!("  did you mean `{best}`?"));
        }
        return 1;
    }
    // Paths relative to where zlint runs.
    let cwd = output.cwd;
    let prefix = format!("{}{}", cwd.display(), std::path::MAIN_SEPARATOR);
    let local = |s: String| s.replace(&prefix, "");
    for (i, id) in found.into_iter().enumerate() {
        if i > 0 {
            output.out(format_args!("{DIM}----{DIM:#}"));
        }
        output.out(format_args!("{}", local(symbols.describe(id))));
        let uses: Vec<String> = symbols.uses_of(id).map(|s| local(symbols.location(s))).collect();
        if !uses.is_empty() {
            output.out(format_args!("\nused at:"));
            for u in uses {
                output.out(format_args!("  {u}"));
            }
        }
    }
    0
}

/// A file's text and the language that owns it.
fn read_source(output: &mut Output<'_>, linter: &Linter, file: &Path) -> Result<(String, usize), u8> {
    let path = output.cwd.join(file);
    let text = match Disk.read(&path) {
        Ok(t) => t,
        Err(e) => {
            render::error(output, &format!("cannot read {}: {e}", file.display()));
            return Err(2);
        }
    };
    let rel = linter.relative(&path);
    let Some(lang) = linter.grammar.lang_for(&rel) else {
        render::error(output, &format!("no `lang` owns {}", file.display()));
        return Err(2);
    };
    Ok((text, lang))
}

fn node_name(g: &crate::grammar::Grammar, tree: &Tree, lang: usize, id: u32) -> String {
    if tree.is_root(id) {
        format!(".{}", g.langs[lang].name)
    } else if tree.is_text(id) {
        String::new()
    } else {
        g.matchers[tree.node(id).matcher as usize].name.clone()
    }
}

fn snippet(text: &str, start: usize, end: usize) -> String {
    let mut s: String = text[start..end].chars().take(60).collect();
    let shown = s.len();
    s = s.replace('\r', "").replace('\n', "⏎").replace('\t', "→");
    if end - start > shown {
        s.push('…');
    }
    s
}

/// `zlint tree --at LINE:COL`: the layers covering a position, then what
/// every child matcher of the innermost one does there, so a span that went
/// to the wrong layer (or to none) can be traced to the matcher that took
/// it or the prefilter that skipped it.
pub fn explain(output: &mut Output<'_>, linter: &Linter, file: &Path, at: &str) -> u8 {
    let (text, lang) = match read_source(output, linter, file) {
        Ok(x) => x,
        Err(code) => return code,
    };
    let lines = crate::line_index::LineIndex::new(&text);
    let parsed = at.split_once(':').and_then(|(l, c)| Some((l.trim().parse::<usize>().ok()?, c.trim().parse::<usize>().ok()?)));
    let Some((line, col)) = parsed.filter(|&(l, c)| l >= 1 && c >= 1 && l <= lines.line_count()) else {
        render::error(output, &format!("`--at` wants LINE:COL inside the file, got `{at}`"));
        return 2;
    };
    let line_start = lines.line_start(line - 1);
    let pos = text[line_start..].char_indices().nth(col - 1).map_or(text.len(), |(i, _)| line_start + i);
    let g = &linter.grammar;
    let tree = Tree::parse(g, lang, &text);

    output.out(format_args!("{BOLD}layers at {line}:{col}{BOLD:#}"));
    let inner = tree.innermost_at(g, pos as u32, u32::MAX).unwrap_or(tree.root);
    let mut path = vec![inner];
    while let Some(p) = tree.parent(*path.last().expect("non-empty")) {
        path.push(p);
    }
    for (depth, &id) in path.iter().rev().filter(|&&id| !tree.is_text(id)).enumerate() {
        let n = tree.node(id);
        let (l, c) = lines.line_col(&text, n.start as usize);
        output.out(format_args!("{}{BOLD}{}{BOLD:#} {DIM}{}:{}{DIM:#} {}", "  ".repeat(depth), node_name(g, &tree, lang, id), l + 1, c + 1, snippet(&text, n.start as usize, n.end as usize)));
    }

    // The layer whose scan decides what starts here: the innermost node that
    // starts before `pos` (a node starting exactly here is the winner).
    let mut container = inner;
    while container != tree.root && tree.node(container).start as usize >= pos {
        container = tree.parent(container).unwrap_or(tree.root);
    }
    let winner = tree
        .node(container)
        .children
        .iter()
        .map(|e| e.node)
        .find(|&c| tree.node(c).start as usize == pos && !tree.is_text(c))
        .map(|c| tree.node(c).matcher);
    let parent = (!tree.is_root(container)).then(|| tree.node(container).matcher);
    let limit = tree.node(container).end as usize;
    let (scanned, candidates) = crate::grammar::probe(g, lang, &text, parent, pos, limit);
    let byte = text[pos..].chars().next().map_or("end of file".into(), |c| format!("{c:?}"));
    output.out(format_args!("\n{BOLD}candidates{BOLD:#} {DIM}children of{DIM:#} {} {DIM}at{DIM:#} {byte}", node_name(g, &tree, lang, container)));
    if !scanned {
        output.out(format_args!("  {DIM}the scan of this layer skips this position (no child can start with it){DIM:#}"));
    }
    use crate::grammar::Probe;
    let best = candidates.iter().filter_map(|&(c, p)| if let Probe::Match(e) = p { Some((c, e)) } else { None }).max_by_key(|&(_, e)| e);
    let ruled_out = candidates.iter().filter(|(_, p)| matches!(p, Probe::FirstByte)).count();
    for (c, outcome) in candidates {
        let name = &g.matchers[c as usize].name;
        let what = match outcome {
            Probe::FirstByte => format!("{DIM}cannot start with this character{DIM:#}"),
            Probe::Prefilter => format!("{DIM}ruled out by its lookbehind / two-byte prefilter{DIM:#}"),
            Probe::NoMatch => "no match".into(),
            Probe::Match(e) => format!("matches {}", snippet(&text, pos, e)),
            Probe::TooLong(e) => format!("matches {} {DIM}but runs past the layer's end{DIM:#}", snippet(&text, pos, e)),
        };
        let mark = if winner == Some(c) {
            format!(" {BOLD}<- in the tree{BOLD:#}")
        } else if winner.is_none() && best.is_some_and(|(b, _)| b == c) {
            format!(" {BOLD}<- longest, yet not in the tree{BOLD:#}")
        } else {
            String::new()
        };
        if !matches!(outcome, Probe::FirstByte) {
            output.out(format_args!("  {CYAN}{name}{CYAN:#} {what}{mark}"));
        }
    }
    if ruled_out > 0 {
        output.out(format_args!("  {DIM}(+{ruled_out} matchers that cannot start with this character){DIM:#}"));
    }
    if winner.is_none() && best.is_none() {
        output.out(format_args!("  {DIM}nothing starts here: the text belongs to the layer above{DIM:#}"));
    }
    0
}

pub fn tree(output: &mut Output<'_>, linter: &Linter, file: &Path, all: bool) -> u8 {
    let (text, lang) = match read_source(output, linter, file) {
        Ok(x) => x,
        Err(code) => return code,
    };
    let g = &linter.grammar;
    let tree = Tree::parse(g, lang, &text);
    let lines = crate::line_index::LineIndex::new(&text);
    for &id in &tree.order {
        let mut depth = 0;
        let mut p = tree.parent(id);
        while let Some(pp) = p {
            depth += 1;
            p = tree.parent(pp);
        }
        let node = tree.node(id);
        let name = node_name(g, &tree, lang, id);
        let label = tree.label(id).map(|l| format!("{}: ", g.labels.name(l))).unwrap_or_default();
        if !all && label.is_empty() && depth > 1 && is_noise(&name) {
            continue;
        }
        let (line, col) = lines.line_col(&text, node.start as usize);
        let snippet = snippet(&text, node.start as usize, node.end as usize);
        output.out(format_args!(
            "{}{CYAN}{label}{CYAN:#}{BOLD}{name}{BOLD:#} {DIM}{}:{}{DIM:#} {snippet}",
            "  ".repeat(depth),
            line + 1,
            col + 1
        ));
    }
    if !all {
        output.out(format_args!("{DIM}(nested brackets/escapes hidden; pass --all to see every node){DIM:#}"));
    }
    0
}

fn is_noise(name: &str) -> bool {
    matches!(name, "parentheses" | "brackets" | "braces") || name.ends_with("escape")
}

/// Human profiling ranks hot paths; JSONL adds every rule's emitted-report count.
pub fn profile(output: &mut Output<'_>, linter: &Linter, format: super::Format, fired: &crate::fx_hash::FxHashMap<String, u64>) {
    use crate::engine::Profile;
    let Some(p) = &linter.profile else { return };
    let ms = |nanos: u64| nanos as f64 / 1e6;
    output.err(format_args!(
        "{BOLD}profile{BOLD:#} {DIM}(summed over threads){DIM:#}\n  parsing {:.1} ms {DIM}(native {:.1}, tokens {:.1}, pattern layers {:.1}){DIM:#}, rules {:.1} ms",
        ms(Profile::get(&p.parse_nanos)),
        ms(Profile::get(&p.native_nanos)),
        ms(Profile::get(&p.token_nanos)),
        ms(Profile::get(&p.layer_nanos)),
        ms(Profile::get(&p.rules_nanos))
    ));
    if Profile::get(&p.names_trees) > 0 {
        output.err(format_args!(
            "  names {:.1} ms {DIM}({} trees; `when` conditions {:.1} ms over {} runs){DIM:#}",
            ms(Profile::get(&p.names_nanos)),
            Profile::get(&p.names_trees),
            ms(Profile::get(&p.name_condition_nanos)),
            Profile::get(&p.name_conditions)
        ));
    }
    let mut matchers: Vec<(u64, u64, &str)> = linter
        .grammar
        .matchers
        .iter()
        .enumerate()
        .map(|(i, m)| (Profile::get(&p.matcher_steps[i]), Profile::get(&p.matcher_runs[i]), m.name.as_str()))
        .filter(|(steps, _, _)| *steps > 0)
        .collect();
    matchers.sort_by_key(|m| std::cmp::Reverse(m.0));
    output.err(format_args!("\n  {BOLD}matchers{BOLD:#} {DIM}by interpreter steps (DFA-only matchers do not show up){DIM:#}"));
    for (steps, runs, name) in matchers.iter().take(15) {
        output.err(format_args!("  {steps:>14} steps {runs:>11} runs  {CYAN}{name}{CYAN:#}"));
    }
    let mut overlays: Vec<(u64, u64, &str)> = linter
        .grammar
        .matchers
        .iter()
        .enumerate()
        .map(|(i, m)| (Profile::get(&p.overlay_nanos[i]), Profile::get(&p.overlay_nodes[i]), m.name.as_str()))
        .filter(|(nanos, _, _)| *nanos > 0)
        .collect();
    overlays.sort_by_key(|o| std::cmp::Reverse(o.0));
    output.err(format_args!("\n  {BOLD}layer searches{BOLD:#} {DIM}by time, part of parsing{DIM:#}"));
    for (nanos, nodes, name) in overlays.iter().take(10) {
        output.err(format_args!("  {:>11.1} ms {nodes:>11} nodes  {CYAN}{name}{CYAN:#}", ms(*nanos)));
    }
    let mut rules: Vec<(u64, u64, &str)> = linter
        .rules
        .iter()
        .enumerate()
        .map(|(i, f)| (Profile::get(&p.rule_nanos[i]), Profile::get(&p.rule_evals[i]), f.name.as_str()))
        .filter(|(_, evals, _)| *evals > 0)
        .collect();
    rules.sort_by_key(|r| std::cmp::Reverse(r.0));
    let mut attributes: Vec<(u64, u64, u64, &str)> = (0..linter.attrs.count())
        .filter_map(|i| {
            let calls = Profile::get(p.attr_calls.get(i)?);
            let (name, _) = linter.attrs.definition(u16::try_from(i).ok()?)?;
            (calls > 0).then(|| (Profile::get(&p.attr_self_nanos[i]), Profile::get(&p.attr_nanos[i]), calls, name))
        })
        .collect();
    attributes.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut functions: Vec<(u64, u64, u64, &str)> = linter.grammar.functions.iter().enumerate()
        .filter_map(|(i, function)| {
            let calls = Profile::get(p.fn_calls.get(i)?);
            (calls > 0).then(|| (Profile::get(&p.fn_self_nanos[i]), Profile::get(&p.fn_nanos[i]), calls, function.name.as_str()))
        })
        .collect();
    functions.sort_by_key(|f| std::cmp::Reverse(f.0));
    if format == super::Format::Jsonl {
        for (index, rule) in linter.rules.iter().enumerate() {
            let row = serde_json::json!({
                "kind": "rule-profile",
                "rule": rule.name,
                "ms": ms(Profile::get(&p.rule_nanos[index])),
                "evaluated": Profile::get(&p.rule_evals[index]),
                "fired": fired.get(&rule.name).copied().unwrap_or(0),
            });
            output.out(format_args!("{row}"));
        }
        for (nanos, file) in p.slowest_files() {
            let row = serde_json::json!({ "kind": "file-profile", "file": file, "ms": ms(nanos) });
            output.out(format_args!("{row}"));
        }
        for (self_nanos, nanos, calls, name) in &attributes {
            let row = serde_json::json!({ "kind": "attribute-profile", "attribute": name, "self_ms": ms(*self_nanos), "ms": ms(*nanos), "reads": calls });
            output.out(format_args!("{row}"));
        }
        for (self_nanos, nanos, calls, name) in &functions {
            let row = serde_json::json!({ "kind": "function-profile", "function": name, "self_ms": ms(*self_nanos), "ms": ms(*nanos), "calls": calls });
            output.out(format_args!("{row}"));
        }
    } else {
        output.err(format_args!("\n  {BOLD}rules{BOLD:#} {DIM}by time{DIM:#}"));
        for (nanos, evals, name) in rules.iter().take(15) {
            output.err(format_args!("  {:>11.1} ms {evals:>11} nodes  {CYAN}{name}{CYAN:#}", ms(*nanos)));
        }
        output.err(format_args!("\n  {BOLD}files{BOLD:#} {DIM}by rules' wall time, slowest first{DIM:#}"));
        for (nanos, file) in p.slowest_files() {
            output.err(format_args!("  {:>11.1} ms  {file}", ms(nanos)));
        }
        output.err(format_args!("\n  {BOLD}lets{BOLD:#} {DIM}by self time (total includes lets and queries they read; reads include memo hits){DIM:#}"));
        for (self_nanos, nanos, calls, name) in attributes.iter().take(25) {
            output.err(format_args!("  {:>11.1} ms self {:>11.1} ms total {calls:>11} reads  {CYAN}{name}{CYAN:#}", ms(*self_nanos), ms(*nanos)));
        }
        output.err(format_args!("\n  {BOLD}fns{BOLD:#} {DIM}by self time (arguments excluded; total includes what they call){DIM:#}"));
        for (self_nanos, nanos, calls, name) in functions.iter().take(25) {
            output.err(format_args!("  {:>11.1} ms self {:>11.1} ms total {calls:>11} calls  {CYAN}{name}{CYAN:#}", ms(*self_nanos), ms(*nanos)));
        }
    }
    let mut facts: Vec<(u64, u64, &str)> = crate::expr::facts::FACTS
        .iter()
        .enumerate()
        .map(|(i, f)| (Profile::get(&p.fact_nanos[i]), Profile::get(&p.fact_calls[i]), f.name))
        .filter(|(_, calls, _)| *calls > 0)
        .collect();
    facts.sort_by_key(|f| std::cmp::Reverse(f.0));
    output.err(format_args!("\n  {BOLD}facts{BOLD:#} {DIM}by time, not in the rules' (a whole-file analysis counts on the first fact needing it){DIM:#}"));
    for (nanos, calls, name) in facts.iter().take(15) {
        output.err(format_args!("  {:>11.1} ms {calls:>11} calls  {CYAN}{name}{CYAN:#}", ms(*nanos)));
    }
    output.err(format_args!("\n  {BOLD}node queries{BOLD:#} {DIM}part of the rules' time{DIM:#}"));
    for query in crate::engine::Query::ALL {
        let at = query as usize;
        let calls = Profile::get(&p.query_calls[at]);
        if calls > 0 {
            output.err(format_args!(
                "  {:>11.1} ms {calls:>11} calls {:>11} nodes  {CYAN}{}{CYAN:#}",
                ms(Profile::get(&p.query_nanos[at])),
                Profile::get(&p.query_nodes[at]),
                query.name()
            ));
        }
    }
    output.err(format_args!(
        "\n  {BOLD}semantic work{BOLD:#}: {} budget-limit hits, {} session state resets, {} effects graph build attempts, {} read-catalogue build attempts",
        Profile::get(&p.semantic_budget_limits),
        Profile::get(&p.semantic_state_discards),
        Profile::get(&p.effect_graph_builds),
        Profile::get(&p.effect_read_catalogue_builds)
    ));
    output.err(format_args!(
        "  program catalogue builds: {}; catalogue read validations: {}",
        Profile::get(&p.program_catalogue_builds),
        Profile::get(&p.catalogue_read_validations)
    ));
    output.err(format_args!(
        "  memo entries discarded at limit: {}",
        Profile::get(&p.semantic_memo_discards)
    ));
    output.err(format_args!(
        "  semantic budget: {} steps total, {} max per root question, {} questions over half budget",
        Profile::get(&p.semantic_budget_steps_total),
        Profile::get(&p.semantic_budget_steps_max),
        Profile::get(&p.semantic_budget_steps_over_half)
    ));
    output.err(format_args!(
        "  effects builds (inclusive; interrupted attempts included): {:.1} ms graphs, {:.1} ms read catalogues; after discarding the same key: {:.1} ms graphs, {:.1} ms read catalogues",
        ms(Profile::get(&p.effect_graph_build_nanos)),
        ms(Profile::get(&p.effect_read_catalogue_build_nanos)),
        ms(Profile::get(&p.effect_graph_rebuild_nanos)),
        ms(Profile::get(&p.effect_read_catalogue_rebuild_nanos))
    ));
    let graph_successes = Profile::get(&p.effect_graph_successes);
    output.err(format_args!(
        "  graph build outcomes: {} complete ({:.1} ms), {} unknown ({:.1} ms); entry_for: {} calls ({:.1} ms inclusive)",
        graph_successes,
        ms(Profile::get(&p.effect_graph_success_nanos)),
        Profile::get(&p.effect_graph_builds).saturating_sub(graph_successes),
        ms(Profile::get(&p.effect_graph_unknown_nanos)),
        Profile::get(&p.effect_entry_calls),
        ms(Profile::get(&p.effect_entry_nanos))
    ));
    let failed_graphs = p.effect_failures();
    output.err(format_args!(
        "  units: {} files parsed for semantic questions ({:.1} ms)",
        Profile::get(&p.unit_parses),
        ms(Profile::get(&p.unit_parse_nanos)),
    ));
    let (tokens, layers, matchers, masked) = linter.unit_plan();
    output.err(format_args!(
        "  foreign-unit plan: {tokens} attached token kinds; layer mask {layers}/{matchers} matchers; child layers {}",
        if masked { "masked" } else { "all (a semantic read sees raw children)" },
    ));
    if !linter.unit_token_reasons().is_empty() {
        output.err(format_args!("  foreign-unit demand from: {}", linter.unit_token_reasons().join(", ")));
    }
    output.err(format_args!(
        "  module work: {} index builds ({:.1} ms), {} plan lookups ({:.1} ms, index builds excluded)",
        Profile::get(&p.module_index_calls),
        ms(Profile::get(&p.module_index_nanos)),
        Profile::get(&p.module_plans_calls),
        ms(Profile::get(&p.module_plans_nanos))
    ));
    if !failed_graphs.is_empty() {
        output.err(format_args!("  failed graph entries (top 10, inclusive; frames combined):"));
        for (path, node, cause, row) in failed_graphs.into_iter().take(10) {
            output.err(format_args!("  {:>11.1} ms {:>11} attempts  {} node {node} {}",
                ms(row.nanos), row.calls, path.display(), cause.name()));
        }
    }
    let mut semantic: Vec<_> = p.semantic_queries.iter()
        .map(|(&name, query)| (Profile::get(&query.nanos), Profile::get(&query.self_nanos), Profile::get(&query.calls), Profile::get(&query.limited), name))
        .filter(|(_, _, calls, ..)| *calls > 0).collect();
    semantic.sort_by_key(|row| std::cmp::Reverse(row.0));
    output.err(format_args!("\n  {BOLD}semantic queries{BOLD:#} {DIM}inclusive time (nested calls overlap), then self time; limits: root questions it asked that ran out of budget{DIM:#}"));
    for (nanos, own, calls, limited, name) in semantic {
        output.err(format_args!("  {:>11.1} ms {:>11.1} ms self {calls:>11} calls {limited:>9} limits  {CYAN}{name}{CYAN:#}", ms(nanos), ms(own)));
    }
    let unattributed = Profile::get(&p.unattributed_names.limited);
    if unattributed > 0 {
        output.err(format_args!("  {unattributed:>9} budget limits outside any semantic query"));
    }
    let mut values: Vec<_> = linter.grammar.matchers.iter().enumerate()
        .map(|(index, matcher)| (Profile::get(&p.value_nanos[index]), Profile::get(&p.value_self_nanos[index]), Profile::get(&p.value_calls[index]), matcher.name.as_str()))
        .filter(|(_, _, calls, _)| *calls > 0).collect();
    values.sort_by_key(|row| std::cmp::Reverse(row.1));
    if !values.is_empty() {
        output.err(format_args!("\n  {BOLD}value evaluations{BOLD:#} {DIM}top 15 by self time; inclusive time overlaps nested aliases, cache hits included{DIM:#}"));
        for (nanos, own, calls, name) in values.into_iter().take(15) {
            output.err(format_args!("  {:>11.1} ms {:>11.1} ms self {calls:>11} calls  {CYAN}{name}{CYAN:#}", ms(nanos), ms(own)));
        }
    }
    let mut names: Vec<_> = p.semantic_queries.iter()
        .map(|(&name, query)| (Profile::get(&query.names_nanos), Profile::get(&query.names_trees), name))
        .chain(std::iter::once((Profile::get(&p.unattributed_names.names_nanos),
            Profile::get(&p.unattributed_names.names_trees), "unattributed")))
        .filter(|(_, trees, _)| *trees > 0).collect();
    names.sort_by_key(|row| std::cmp::Reverse(row.0));
    if !names.is_empty() {
        output.err(format_args!("\n  {BOLD}name resolution{BOLD:#} {DIM}by asking query; cold tree loads only{DIM:#}"));
        for (nanos, trees, name) in names {
            output.err(format_args!("  {:>11.1} ms {trees:>11} trees  {CYAN}{name}{CYAN:#}", ms(nanos)));
        }
    }
}

pub fn functions(output: &mut Output<'_>) -> u8 {
    output.out(format_args!("{BOLD}variables{BOLD:#}"));
    for b in Builtin::ALL {
        let (name, doc) = (b.name(), b.doc());
        output.out(format_args!("  {CYAN}{name}{CYAN:#} {DIM}- {doc}{DIM:#}"));
    }
    for f in crate::expr::facts::FACTS.iter() {
        output.out(format_args!("  {CYAN}{}{CYAN:#} {DIM}- {}{DIM:#}", f.name, f.doc));
    }
    output.out(format_args!("  {DIM}plus the node's fields by label, options by name, and `matcher` / `matcher.field` for ancestors{DIM:#}"));
    output.out(format_args!("\n{BOLD}functions{BOLD:#}"));
    for (signature, doc) in crate::expr::SPECIAL_FORMS {
        output.out(format_args!("  {CYAN}{signature}{CYAN:#} {DIM}- {doc}{DIM:#}"));
    }
    for f in FUNCTIONS {
        output.out(format_args!("  {CYAN}{}{CYAN:#} {DIM}- {}{DIM:#}", f.signature, f.doc));
    }
    0
}
