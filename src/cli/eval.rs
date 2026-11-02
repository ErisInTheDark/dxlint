//! `dxlint eval <target> <expr> files...`: what an expression evaluates to on
//! every node of a layer, with everything the project config defines.
//!
//! The question is asked as a rule in a virtual file read after the project
//! config: attributes, macros, facts and options all resolve exactly as they
//! would in a rule, and mistakes are reported like any config error.

use std::path::PathBuf;
use anstyle::Style;

use super::{load_with, render, ConfigArg, Output};
use crate::config::source::{Disk, SourceProvider};
use crate::line_index::LineIndex;

const RULE: &str = "dxlint-eval";
const DIM: Style = Style::new().dimmed();
const BOLD: Style = Style::new().bold();

pub fn run(output: &mut Output<'_>, arg: &ConfigArg, target: &str, expr: &str, files: &[PathBuf]) -> u8 {
    let cwd = output.cwd;
    // `null` shows as `null` rather than as an empty message.
    let question = format!("in {target} rule {RULE} info {{ value ({expr}); then (result) if result == null then \"null\" else str(result) }}\n");
    let project = match load_with(output, arg, Some(("(dxlint eval)", &question)), false) {
        Ok(p) => p,
        Err(code) => return code,
    };
    if !project.config_ok {
        return 2;
    }
    let linter = project.linter;
    let mut failed = false;
    for file in files {
        let path = cwd.join(file);
        let text = match Disk.read(&path) {
            Ok(t) => t,
            Err(e) => {
                render::error(output, &format!("cannot read {}: {e}", file.display()));
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
            output.out(format_args!("{DIM}{name}:{}:{}{DIM:#} {node} {BOLD}=>{BOLD:#} {}", line + 1, col + 1, d.message));
        }
        if diags.is_empty() {
            output.out(format_args!("{DIM}{name}: no `{target}` nodes{DIM:#}"));
        }
    }
    if failed {
        1
    } else {
        0
    }
}
