//! Command line interface.

mod eval;
mod info;
mod lint;
mod render;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use zoomie_lint::config::source::{normalize_path, Disk, SourceProvider};
use zoomie_lint::config;
use zoomie_lint::engine::symbols::Symbols;
use zoomie_lint::engine::{build, build_for_lint, Linter};

#[derive(Parser)]
#[command(name = "zlint", version, about = "Extremely fast linting with rules written in .zlint", subcommand_precedence_over_arg = true)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    lint: LintArgs,
    #[command(flatten)]
    config: ConfigArg,
}

#[derive(clap::Args, Clone)]
pub struct ConfigArg {
    /// Config file (default: the nearest `.zlint` above the current directory)
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct LintArgs {
    /// Files or directories to lint (default: the project root)
    pub paths: Vec<PathBuf>,
    /// Limit lint workers and parallel rule groups (default: available CPUs)
    #[arg(long)]
    pub jobs: Option<std::num::NonZeroUsize>,
    /// Apply fixes in place
    #[arg(long)]
    pub fix: bool,
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Human)]
    pub format: Format,
    /// Print timing and throughput
    #[arg(long)]
    pub stats: bool,
    /// Report which matchers and rules cost the most time (implies --no-cache)
    #[arg(long)]
    pub profile: bool,
    /// Lint every file, without reading or writing `.zlintcache`
    #[arg(long)]
    pub no_cache: bool,
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
    Check,
    /// List rules with their severity, options and messages
    Rules,
    /// Explain a name from the configuration: a rule, layer, field,
    /// attribute, macro, group, option, builtin, fact, function or keyword,
    /// with where it is defined and used
    Describe {
        /// The name (`no-console`, `ts_call`, `callee`, `type_flags`, `group`)
        name: String,
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
    },
    /// Evaluate an expression on every node of a layer, with the project's
    /// config (attributes, facts and macros included)
    Eval {
        /// Target, as in `in <target> rule`: `ts_call`, `ts_call.callee`, `a, b`
        target: String,
        /// The expression (`type`, `callee.resolved`, `len(text) > 80`)
        expr: String,
        /// Files to evaluate in
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// List the functions available in rule expressions
    Functions,
    /// Run the language server on stdio
    Lsp,
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    let config = &cli.config;
    match cli.command {
        None => lint::run(cli.lint, config),
        Some(Command::Check) => match load(config) {
            Ok(Project { config_ok: true, .. }) => {
                eprintln!("config ok");
                ExitCode::SUCCESS
            }
            Ok(_) => ExitCode::from(2),
            Err(code) => code,
        },
        Some(Command::Rules) => match load(config) {
            Ok(p) => info::rules(&p.linter),
            Err(code) => code,
        },
        Some(Command::Describe { name }) => match load(config) {
            Ok(p) => info::describe(p.symbols.as_deref().expect("tooling symbols"), &name),
            Err(code) => code,
        },
        Some(Command::Tree { file, all, at }) => match load(config) {
            Ok(p) => match at {
                Some(at) => info::explain(&p.linter, &file, &at),
                None => info::tree(&p.linter, &file, all),
            },
            Err(code) => code,
        },
        Some(Command::Eval { target, expr, files }) => eval::run(config, &target, &expr, &files),
        Some(Command::Functions) => info::functions(),
        Some(Command::Lsp) => crate::lsp::run(),
    }
}

/// A built linter, and whether its config had no errors.
pub struct Project {
    pub linter: Linter,
    pub symbols: Option<std::sync::Arc<Symbols>>,
    /// `false` when config errors were printed: the linter still runs with
    /// every part that built, but the command should fail in the end.
    pub config_ok: bool,
}

/// Loads and builds the linter, printing any config errors. Uses the
/// explicit `--config`, else the nearest `.zlint` upward. Only a
/// missing config is fatal: a broken one lints with what still works.
pub fn load(arg: &ConfigArg) -> Result<Project, ExitCode> {
    load_with(arg, false)
}

pub fn load_for_lint(arg: &ConfigArg) -> Result<Project, ExitCode> {
    load_with(arg, true)
}

fn load_with(arg: &ConfigArg, runtime: bool) -> Result<Project, ExitCode> {
    let path = match &arg.config {
        Some(p) => normalize_path(p),
        None => match config::find(&std::env::current_dir().unwrap_or_default()) {
            Some(p) => p,
            None => {
                render::error(&config::not_found_message(&std::env::current_dir().unwrap_or_default()));
                return Err(ExitCode::from(2));
            }
        },
    };
    load_from_with(&path, &Disk, runtime)
}

pub fn load_from(path: &Path, provider: &dyn SourceProvider) -> Result<Project, ExitCode> {
    load_from_with(path, provider, false)
}

fn load_from_with(path: &Path, provider: &dyn SourceProvider, runtime: bool) -> Result<Project, ExitCode> {
    let root = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let loaded = config::load_file(path, provider);
    let built = if runtime { build_for_lint(loaded, root) } else { build(loaded, root) };
    let config_ok = built.errors.is_empty();
    if !config_ok {
        render::config_errors(&built.errors, &built.sources);
    }
    match built.linter {
        Some(linter) => Ok(Project { linter, symbols: built.symbols, config_ok }),
        None => Err(ExitCode::from(2)),
    }
}
