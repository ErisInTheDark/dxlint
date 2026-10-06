//! `zlint rules`, `zlint tree`, `zlint functions`.

use std::path::Path;
use std::process::ExitCode;

use anstream::println;
use anstyle::{AnsiColor, Style};

use super::render;
use zoomie_lint::config::ast::Severity;
use zoomie_lint::engine::symbols::Symbols;
use zoomie_lint::engine::Linter;
use zoomie_lint::expr::show;
use zoomie_lint::expr::functions::FUNCTIONS;
use zoomie_lint::expr::BUILTIN_VARS;
use zoomie_lint::grammar::Tree;

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const CYAN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)));

/// `zlint rules`: every rule the config defines, the ones that are off
/// included (with the severity `rule name` turns them on at).
pub fn rules(linter: &Linter) -> ExitCode {
    enum Row<'a> {
        Compiled(&'a zoomie_lint::engine::Rule),
        Inert(&'a zoomie_lint::engine::InertRule),
    }
    let mut rows: Vec<(&str, Row)> = linter.rules.iter().map(|r| (r.name.as_str(), Row::Compiled(r))).collect();
    rows.extend(linter.inert.iter().map(|r| (r.name.as_str(), Row::Inert(r))));
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let off = |default: Severity| format!("{DIM}off (default {}){DIM:#}", default.as_str());
    for (_, row) in rows {
        let f = match row {
            Row::Inert(r) => {
                println!("{BOLD}{}{BOLD:#} {} {DIM}in{DIM:#} {CYAN}{}{CYAN:#}", r.name, off(r.default_severity), r.target_text);
                continue;
            }
            Row::Compiled(f) => f,
        };
        let sev = render::severity_style(f.severity);
        let sev_text = if f.severity == Severity::Off { off(f.default_severity) } else { format!("{sev}{}{sev:#}", f.severity.as_str()) };
        let fix = if f.has_fix() { format!(" {DIM}fixable{DIM:#}") } else { String::new() };
        println!("{BOLD}{}{BOLD:#} {sev_text} {DIM}in{DIM:#} {CYAN}{}{CYAN:#}{fix}", f.name, f.target_text);
        if let Some(d) = f.message_template() {
            println!("    {d}");
        }
        for o in f.options() {
            let changed = if o.value != o.default { format!(" {DIM}(default {}){DIM:#}", show(&o.default)) } else { String::new() };
            let desc = o.description.as_deref().map(|d| format!(" {DIM}- {d}{DIM:#}")).unwrap_or_default();
            println!("    {CYAN}{}{CYAN:#}{DIM}: {}{DIM:#} = {}{changed}{desc}", o.name, o.schema, show(&o.value));
        }
    }
    ExitCode::SUCCESS
}

/// `zlint describe name`: everything of that name, as hover shows it, and
/// where each is used.
pub fn describe(symbols: &Symbols, name: &str) -> ExitCode {
    let found = symbols.named(name);
    if found.is_empty() {
        render::error(&format!("nothing named `{name}` in this configuration"));
        let names = symbols.symbols.iter().map(|s| s.name.as_str());
        if let Some(best) = zoomie_lint::config::error::closest(name, names) {
            eprintln!("  did you mean `{best}`?");
        }
        return ExitCode::from(1);
    }
    // Paths relative to where zlint runs.
    let cwd = std::env::current_dir().unwrap_or_default();
    let prefix = format!("{}{}", cwd.display(), std::path::MAIN_SEPARATOR);
    let local = |s: String| s.replace(&prefix, "");
    for (i, id) in found.into_iter().enumerate() {
        if i > 0 {
            println!("{DIM}----{DIM:#}");
        }
        println!("{}", local(symbols.describe(id)));
        let uses: Vec<String> = symbols.uses_of(id).map(|s| local(symbols.location(s))).collect();
        if !uses.is_empty() {
            println!("\nused at:");
            for u in uses {
                println!("  {u}");
            }
        }
    }
    ExitCode::SUCCESS
}

/// A file's text and the language that owns it.
fn read_source(linter: &Linter, file: &Path) -> Result<(String, usize), ExitCode> {
    let path = std::env::current_dir().unwrap_or_default().join(file);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            render::error(&format!("cannot read {}: {e}", file.display()));
            return Err(ExitCode::from(2));
        }
    };
    let rel = linter.relative(&path);
    let Some(lang) = linter.grammar.lang_for(&rel) else {
        render::error(&format!("no `lang` owns {}", file.display()));
        return Err(ExitCode::from(2));
    };
    Ok((text, lang))
}

fn node_name(g: &zoomie_lint::grammar::Grammar, tree: &Tree, lang: usize, id: u32) -> String {
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
pub fn explain(linter: &Linter, file: &Path, at: &str) -> ExitCode {
    let (text, lang) = match read_source(linter, file) {
        Ok(x) => x,
        Err(code) => return code,
    };
    let lines = zoomie_lint::line_index::LineIndex::new(&text);
    let parsed = at.split_once(':').and_then(|(l, c)| Some((l.trim().parse::<usize>().ok()?, c.trim().parse::<usize>().ok()?)));
    let Some((line, col)) = parsed.filter(|&(l, c)| l >= 1 && c >= 1 && l <= lines.line_count()) else {
        render::error(&format!("`--at` wants LINE:COL inside the file, got `{at}`"));
        return ExitCode::from(2);
    };
    let line_start = lines.line_start(line - 1);
    let pos = text[line_start..].char_indices().nth(col - 1).map_or(text.len(), |(i, _)| line_start + i);
    let g = &linter.grammar;
    let tree = Tree::parse(g, lang, &text);

    println!("{BOLD}layers at {line}:{col}{BOLD:#}");
    let inner = tree.innermost_at(g, pos as u32, u32::MAX).unwrap_or(tree.root);
    let mut path = vec![inner];
    while let Some(p) = tree.parent(*path.last().expect("non-empty")) {
        path.push(p);
    }
    for (depth, &id) in path.iter().rev().filter(|&&id| !tree.is_text(id)).enumerate() {
        let n = tree.node(id);
        let (l, c) = lines.line_col(&text, n.start as usize);
        println!("{}{BOLD}{}{BOLD:#} {DIM}{}:{}{DIM:#} {}", "  ".repeat(depth), node_name(g, &tree, lang, id), l + 1, c + 1, snippet(&text, n.start as usize, n.end as usize));
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
    let (scanned, candidates) = zoomie_lint::grammar::probe(g, lang, &text, parent, pos, limit);
    let byte = text[pos..].chars().next().map_or("end of file".into(), |c| format!("{c:?}"));
    println!("\n{BOLD}candidates{BOLD:#} {DIM}children of{DIM:#} {} {DIM}at{DIM:#} {byte}", node_name(g, &tree, lang, container));
    if !scanned {
        println!("  {DIM}the scan of this layer skips this position (no child can start with it){DIM:#}");
    }
    use zoomie_lint::grammar::Probe;
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
            println!("  {CYAN}{name}{CYAN:#} {what}{mark}");
        }
    }
    if ruled_out > 0 {
        println!("  {DIM}(+{ruled_out} matchers that cannot start with this character){DIM:#}");
    }
    if winner.is_none() && best.is_none() {
        println!("  {DIM}nothing starts here: the text belongs to the layer above{DIM:#}");
    }
    ExitCode::SUCCESS
}

pub fn tree(linter: &Linter, file: &Path, all: bool) -> ExitCode {
    let (text, lang) = match read_source(linter, file) {
        Ok(x) => x,
        Err(code) => return code,
    };
    let g = &linter.grammar;
    let tree = Tree::parse(g, lang, &text);
    let lines = zoomie_lint::line_index::LineIndex::new(&text);
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
        println!(
            "{}{CYAN}{label}{CYAN:#}{BOLD}{name}{BOLD:#} {DIM}{}:{}{DIM:#} {snippet}",
            "  ".repeat(depth),
            line + 1,
            col + 1
        );
    }
    if !all {
        println!("{DIM}(nested brackets/escapes hidden; pass --all to see every node){DIM:#}");
    }
    ExitCode::SUCCESS
}

fn is_noise(name: &str) -> bool {
    matches!(name, "parentheses" | "brackets" | "braces") || name.ends_with("escape")
}

/// `--profile`: the most expensive matchers and rules, on stderr.
pub fn profile(linter: &Linter) {
    use anstream::eprintln;
    use zoomie_lint::engine::Profile;
    let Some(p) = &linter.profile else { return };
    let ms = |nanos: u64| nanos as f64 / 1e6;
    eprintln!(
        "{BOLD}profile{BOLD:#} {DIM}(summed over threads){DIM:#}\n  parsing {:.1} ms {DIM}(native {:.1}, tokens {:.1}, pattern layers {:.1}){DIM:#}, rules {:.1} ms",
        ms(Profile::get(&p.parse_nanos)),
        ms(Profile::get(&p.native_nanos)),
        ms(Profile::get(&p.token_nanos)),
        ms(Profile::get(&p.layer_nanos)),
        ms(Profile::get(&p.rules_nanos))
    );
    let mut matchers: Vec<(u64, u64, &str)> = linter
        .grammar
        .matchers
        .iter()
        .enumerate()
        .map(|(i, m)| (Profile::get(&p.matcher_steps[i]), Profile::get(&p.matcher_runs[i]), m.name.as_str()))
        .filter(|(steps, _, _)| *steps > 0)
        .collect();
    matchers.sort_by(|a, b| b.0.cmp(&a.0));
    eprintln!("\n  {BOLD}matchers{BOLD:#} {DIM}by interpreter steps (DFA-only matchers do not show up){DIM:#}");
    for (steps, runs, name) in matchers.iter().take(15) {
        eprintln!("  {steps:>14} steps {runs:>11} runs  {CYAN}{name}{CYAN:#}");
    }
    let mut overlays: Vec<(u64, u64, &str)> = linter
        .grammar
        .matchers
        .iter()
        .enumerate()
        .map(|(i, m)| (Profile::get(&p.overlay_nanos[i]), Profile::get(&p.overlay_nodes[i]), m.name.as_str()))
        .filter(|(nanos, _, _)| *nanos > 0)
        .collect();
    overlays.sort_by(|a, b| b.0.cmp(&a.0));
    eprintln!("\n  {BOLD}layer searches{BOLD:#} {DIM}by time, part of parsing{DIM:#}");
    for (nanos, nodes, name) in overlays.iter().take(10) {
        eprintln!("  {:>11.1} ms {nodes:>11} nodes  {CYAN}{name}{CYAN:#}", ms(*nanos));
    }
    let mut rules: Vec<(u64, u64, &str)> = linter
        .rules
        .iter()
        .enumerate()
        .map(|(i, f)| (Profile::get(&p.rule_nanos[i]), Profile::get(&p.rule_evals[i]), f.name.as_str()))
        .filter(|(_, evals, _)| *evals > 0)
        .collect();
    rules.sort_by(|a, b| b.0.cmp(&a.0));
    eprintln!("\n  {BOLD}rules{BOLD:#} {DIM}by time{DIM:#}");
    for (nanos, evals, name) in rules.iter().take(15) {
        eprintln!("  {:>11.1} ms {evals:>11} nodes  {CYAN}{name}{CYAN:#}", ms(*nanos));
    }
    let mut facts: Vec<(u64, u64, &str)> = zoomie_lint::expr::facts::FACTS
        .iter()
        .enumerate()
        .map(|(i, f)| (Profile::get(&p.fact_nanos[i]), Profile::get(&p.fact_calls[i]), f.name))
        .filter(|(_, calls, _)| *calls > 0)
        .collect();
    facts.sort_by(|a, b| b.0.cmp(&a.0));
    eprintln!("\n  {BOLD}facts{BOLD:#} {DIM}by time, part of the rules' (a whole-file analysis counts on the first fact needing it){DIM:#}");
    for (nanos, calls, name) in facts.iter().take(15) {
        eprintln!("  {:>11.1} ms {calls:>11} calls  {CYAN}{name}{CYAN:#}", ms(*nanos));
    }
}

pub fn functions() -> ExitCode {
    println!("{BOLD}variables{BOLD:#}");
    for (name, doc) in BUILTIN_VARS {
        println!("  {CYAN}{name}{CYAN:#} {DIM}- {doc}{DIM:#}");
    }
    for f in zoomie_lint::expr::facts::FACTS.iter() {
        println!("  {CYAN}{}{CYAN:#} {DIM}- {}{DIM:#}", f.name, f.doc);
    }
    println!("  {DIM}plus the node's fields by label, options by name, and `matcher` / `matcher.field` for ancestors{DIM:#}");
    println!("\n{BOLD}functions{BOLD:#}");
    for (signature, doc) in zoomie_lint::expr::SPECIAL_FORMS {
        println!("  {CYAN}{signature}{CYAN:#} {DIM}- {doc}{DIM:#}");
    }
    for f in FUNCTIONS {
        println!("  {CYAN}{}{CYAN:#} {DIM}- {}{DIM:#}", f.signature, f.doc);
    }
    ExitCode::SUCCESS
}
