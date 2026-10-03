//! Command line interface.

mod eval;
mod info;
mod lint;
mod render;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use zoomie_lint::config::source::{normalize_path, Disk, SourceProvider};
use zoomie_lint::config::{self, CONFIG_NAME};
use zoomie_lint::engine::{build, Linter};

#[derive(Parser)]
#[command(name = "zlint", version, about = "Extremely fast linting with rules written in .zlint", args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    lint: LintArgs,
}

#[derive(clap::Args, Clone)]
pub struct ConfigArg {
    /// Config file (default: the nearest zoomie.zlint above the current directory)
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct LintArgs {
    /// Files or directories to lint (default: the project root)
    pub paths: Vec<PathBuf>,
    /// Apply fixes in place
    #[arg(long)]
    pub fix: bool,
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Human)]
    pub format: Format,
    /// Print timing and throughput
    #[arg(long)]
    pub stats: bool,
    /// Report which matchers and rules cost the most time
    #[arg(long)]
    pub profile: bool,
    #[command(flatten)]
    pub config: ConfigArg,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Readable, grouped by file, sorted
    Human,
    /// One JSON object per diagnostic, streamed as files finish
    Jsonl,
}

#[derive(Subcommand)]
enum Command {
    /// Check the configuration without linting anything
    Check {
        #[command(flatten)]
        config: ConfigArg,
    },
    /// List rules with their severity, options and messages
    Rules {
        #[command(flatten)]
        config: ConfigArg,
    },
    /// Print the layer tree zlint builds for a file (for writing matchers)
    Tree {
        file: PathBuf,
        /// Show every node, including nested brackets and escapes
        #[arg(long)]
        all: bool,
        /// Explain one position (`12:5`): the layers covering it, and what
        /// every candidate matcher does there
        #[arg(long, value_name = "LINE:COL")]
        at: Option<String>,
        #[command(flatten)]
        config: ConfigArg,
    },
    /// Evaluate an expression on every node of a layer, with the project's
    /// config (attributes, facts and macros included)
    Eval {
        /// Target, as in `in <target> rule`: `ts_call`, `ts_call.callee`, `a or b`
        target: String,
        /// The expression (`type`, `callee.resolved`, `len(text) > 80`)
        expr: String,
        /// Files to evaluate in
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[command(flatten)]
        config: ConfigArg,
    },
    /// List the functions available in rule expressions
    Functions,
    /// Run the language server on stdio
    Lsp,
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        None => lint::run(cli.lint),
        Some(Command::Check { config }) => match load(&config) {
            Ok(Project { config_ok: true, .. }) => {
                eprintln!("config ok");
                ExitCode::SUCCESS
            }
            Ok(_) => ExitCode::from(2),
            Err(code) => code,
        },
        Some(Command::Rules { config }) => match load(&config) {
            Ok(p) => info::rules(&p.linter),
            Err(code) => code,
        },
        Some(Command::Tree { file, all, at, config }) => match load(&config) {
            Ok(p) => match at {
                Some(at) => info::explain(&p.linter, &file, &at),
                None => info::tree(&p.linter, &file, all),
            },
            Err(code) => code,
        },
        Some(Command::Eval { target, expr, files, config }) => eval::run(&config, &target, &expr, &files),
        Some(Command::Functions) => info::functions(),
        Some(Command::Lsp) => crate::lsp::run(),
    }
}

/// A built linter, and whether its config had no errors.
pub struct Project {
    pub linter: Linter,
    /// `false` when config errors were printed: the linter still runs with
    /// every part that built, but the command should fail in the end.
    pub config_ok: bool,
}

/// Loads and builds the linter, printing any config errors. Uses the
/// explicit `--config`, else the nearest `zoomie.zlint` upward. Only a
/// missing config is fatal: a broken one lints with what still works.
pub fn load(arg: &ConfigArg) -> Result<Project, ExitCode> {
    let path = match &arg.config {
        Some(p) => normalize_path(p),
        None => match config::find(&std::env::current_dir().unwrap_or_default()) {
            Some(p) => p,
            None => {
                render::error(&format!("no {CONFIG_NAME} found in this directory or any parent; pass one with --config"));
                return Err(ExitCode::from(2));
            }
        },
    };
    load_from(&path, &Disk)
}

pub fn load_from(path: &Path, provider: &dyn SourceProvider) -> Result<Project, ExitCode> {
    let root = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let built = build(config::load_file(path, provider), root);
    let config_ok = built.errors.is_empty();
    if !config_ok {
        render::config_errors(&built.errors, &built.sources);
    }
    match built.linter {
        Some(linter) => Ok(Project { linter, config_ok }),
        None => Err(ExitCode::from(2)),
    }
}
