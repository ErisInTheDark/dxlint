//! `zlint rules`, `zlint tree`, `zlint functions`.

use std::path::Path;
use std::process::ExitCode;

use anstream::println;
use anstyle::{AnsiColor, Style};

use super::render;
use zoomie_lint::config::ast::Severity;
use zoomie_lint::engine::Linter;
use zoomie_lint::expr::functions::FUNCTIONS;
use zoomie_lint::expr::BUILTIN_VARS;
use zoomie_lint::grammar::Tree;

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const CYAN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)));

pub fn rules(linter: &Linter) -> ExitCode {
    let mut rules: Vec<_> = linter.rules.iter().collect();
    rules.sort_by(|a, b| a.name.cmp(&b.name));
    for f in rules {
        let sev = render::severity_style(f.severity);
        let sev_text = if f.severity == Severity::Off { format!("{DIM}off{DIM:#}") } else { format!("{sev}{}{sev:#}", f.severity.as_str()) };
        let fix = if f.has_fix() { format!(" {DIM}fixable{DIM:#}") } else { String::new() };
        println!("{BOLD}{}{BOLD:#} {sev_text} {DIM}in{DIM:#} {CYAN}{}{CYAN:#}{fix}", f.name, f.target_text);
        if let Some(d) = f.message_template() {
            println!("    {d}");
        }
        for o in &f.options {
            let changed = if o.value != o.default { format!(" {DIM}(default {}){DIM:#}", show(&o.default)) } else { String::new() };
            let desc = o.description.as_deref().map(|d| format!(" {DIM}- {d}{DIM:#}")).unwrap_or_default();
            println!("    {CYAN}{}{CYAN:#} = {}{changed}{desc}", o.name, show(&o.value));
        }
    }
    ExitCode::SUCCESS
}

fn show(v: &zoomie_lint::expr::Value) -> String {
    use zoomie_lint::expr::Value;
    match v {
        Value::Str(s) => format!("{s:?}"),
        Value::List(items) => format!("[{}]", items.iter().map(show).collect::<Vec<_>>().join(", ")),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

pub fn tree(linter: &Linter, file: &Path, all: bool) -> ExitCode {
    let path = std::env::current_dir().unwrap_or_default().join(file);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            render::error(&format!("cannot read {}: {e}", file.display()));
            return ExitCode::from(2);
        }
    };
    let rel = linter.relative(&path);
    let Some(lang) = linter.grammar.lang_for(&rel) else {
        render::error(&format!("no `lang` owns {}", file.display()));
        return ExitCode::from(2);
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
        let name = if tree.is_root(id) {
            format!(".{}", g.langs[lang].name)
        } else if tree.is_text(id) {
            String::new()
        } else {
            g.matchers[node.matcher as usize].name.clone()
        };
        let label = tree.label(id).map(|l| format!("{}: ", g.labels.name(l))).unwrap_or_default();
        if !all && label.is_empty() && depth > 1 && is_noise(&name) {
            continue;
        }
        let (line, col) = lines.line_col(&text, node.start as usize);
        let mut snippet: String = text[node.start as usize..node.end as usize].chars().take(60).collect();
        snippet = snippet.replace('\r', "").replace('\n', "⏎").replace('\t', "→");
        if (node.end - node.start) as usize > snippet.len() {
            snippet.push('…');
        }
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
    matches!(name, "paren" | "brack" | "brace") || name.ends_with("escape")
}

/// `--profile`: the most expensive matchers and rules, on stderr.
pub fn profile(linter: &Linter) {
    use anstream::eprintln;
    use zoomie_lint::engine::Profile;
    let Some(p) = &linter.profile else { return };
    let ms = |nanos: u64| nanos as f64 / 1e6;
    eprintln!(
        "{BOLD}profile{BOLD:#} {DIM}(summed over threads){DIM:#}\n  parsing {:.1} ms, rules {:.1} ms",
        ms(Profile::get(&p.parse_nanos)),
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
}

pub fn functions() -> ExitCode {
    println!("{BOLD}variables{BOLD:#}");
    for (name, doc) in BUILTIN_VARS {
        println!("  {CYAN}{name}{CYAN:#} {DIM}- {doc}{DIM:#}");
    }
    println!("  {DIM}plus the node's fields by label, options by name, and `matcher` / `matcher.field` for ancestors{DIM:#}");
    println!("\n{BOLD}functions{BOLD:#}");
    for f in FUNCTIONS {
        println!("  {CYAN}{}{CYAN:#} {DIM}- {}{DIM:#}", f.signature, f.doc);
    }
    ExitCode::SUCCESS
}
