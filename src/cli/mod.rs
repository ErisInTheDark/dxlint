//! Command line interface.

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
    /// List formatters with their severity, options and descriptions
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
        #[command(flatten)]
        config: ConfigArg,
    },
    /// List the functions available in formatter expressions
    Functions,
    /// Run the language server on stdio
    Lsp,
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        None => lint::run(cli.lint),
        Some(Command::Check { config }) => match load(&config) {
            Ok(_) => {
                eprintln!("config ok");
                ExitCode::SUCCESS
            }
            Err(code) => code,
        },
        Some(Command::Rules { config }) => match load(&config) {
            Ok(linter) => info::rules(&linter),
            Err(code) => code,
        },
        Some(Command::Tree { file, all, config }) => match load(&config) {
            Ok(linter) => info::tree(&linter, &file, all),
            Err(code) => code,
        },
        Some(Command::Functions) => info::functions(),
        Some(Command::Lsp) => crate::lsp::run(),
    }
}

/// Loads and builds the linter, printing config errors on failure. Uses the
/// explicit `--config`, else the nearest `zoomie.zlint` upward.
pub fn load(arg: &ConfigArg) -> Result<Linter, ExitCode> {
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

pub fn load_from(path: &Path, provider: &dyn SourceProvider) -> Result<Linter, ExitCode> {
    let root = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let built = build(config::load_file(path, provider), root);
    match built.linter {
        Some(l) => Ok(l),
        None => {
            render::config_errors(&built.errors, &built.sources);
            Err(ExitCode::from(2))
        }
    }
}
