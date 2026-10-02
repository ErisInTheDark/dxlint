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
    let mut fs: Vec<_> = linter.formatters.iter().collect();
    fs.sort_by(|a, b| a.name.cmp(&b.name));
    for f in fs {
        let sev = render::severity_style(f.severity);
        let sev_text = if f.severity == Severity::Off { format!("{DIM}off{DIM:#}") } else { format!("{sev}{}{sev:#}", f.severity.as_str()) };
        let fix = if f.has_fix() { format!(" {DIM}fixable{DIM:#}") } else { String::new() };
        println!("{BOLD}{}{BOLD:#} {sev_text} {DIM}on{DIM:#} {CYAN}{}{CYAN:#}{fix}", f.name, f.target_text);
        if let Some(d) = f.description() {
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
