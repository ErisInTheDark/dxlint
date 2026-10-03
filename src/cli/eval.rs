//! `zlint eval <target> <expr> files...`: what an expression evaluates to on
//! every node of a layer, with everything the project config defines.
//!
//! The question is asked as a rule in a virtual config file next to the real
//! one, which imports it: attributes, macros, facts and options all resolve
//! exactly as they would in a rule, and mistakes are reported like any
//! config error.

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anstream::println;
use anstyle::Style;

use super::{load_from, render, ConfigArg};
use zoomie_lint::config::source::{normalize_path, Disk, SourceProvider};
use zoomie_lint::config::{self, CONFIG_NAME};
use zoomie_lint::line_index::LineIndex;

const RULE: &str = "zlint-eval";
const DIM: Style = Style::new().dimmed();
const BOLD: Style = Style::new().bold();

/// The real config files, plus the virtual one holding the question.
struct WithQuestion {
    path: PathBuf,
    text: String,
}

impl SourceProvider for WithQuestion {
    fn read(&self, path: &Path) -> io::Result<String> {
        if path == self.path {
            Ok(self.text.clone())
        } else {
            Disk.read(path)
        }
    }
}

pub fn run(arg: &ConfigArg, target: &str, expr: &str, files: &[PathBuf]) -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let real = match &arg.config {
        Some(p) => normalize_path(p),
        None => match config::find(&cwd) {
            Some(p) => p,
            None => {
                render::error(&format!("no {CONFIG_NAME} found in this directory or any parent; pass one with --config"));
                return ExitCode::from(2);
            }
        },
    };
    let dir = real.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = real.file_stem().and_then(|s| s.to_str()).unwrap_or("zoomie");
    let question = WithQuestion {
        path: dir.join("(zlint eval).zlint"),
        // `null` shows as `null` rather than as an empty message.
        text: format!("import \"./{stem}\"\nin {target} rule {RULE} info (if ({expr}) == null then \"null\" else str({expr}))\n"),
    };
    let project = match load_from(&question.path, &question) {
        Ok(p) => p,
        Err(code) => return code,
    };
    if !project.config_ok {
        return ExitCode::from(2);
    }
    let linter = project.linter;
    let mut failed = false;
    for file in files {
        let path = cwd.join(file);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                render::error(&format!("cannot read {}: {e}", file.display()));
                failed = true;
                continue;
            }
        };
        let lines = LineIndex::new(&text);
        let mut diags: Vec<_> = linter.lint(&path, &text).into_iter().filter(|d| d.rule == RULE).collect();
        diags.sort_by_key(|d| (d.start, d.end));
        let name = file.to_string_lossy().replace('\\', "/");
        for d in &diags {
            let (line, col) = lines.line_col(&text, d.start);
            let mut node: String = text[d.start..d.end].chars().take(50).collect();
            let shown = node.len();
            node = node.replace('\r', "").replace('\n', "⏎").replace('\t', "→");
            if d.end - d.start > shown {
                node.push('…');
            }
            println!("{DIM}{name}:{}:{}{DIM:#} {node} {BOLD}=>{BOLD:#} {}", line + 1, col + 1, d.message);
        }
        if diags.is_empty() {
            println!("{DIM}{name}: no `{target}` nodes{DIM:#}");
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
