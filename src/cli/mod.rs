//! Command line interface.

mod eval;
mod info;
mod lint;
mod render;

#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};

use crate::config::source::{normalize_path, Disk, SourceProvider};
use crate::config;
use crate::engine::symbols::Symbols;
use crate::engine::{build, build_for_lint, Linter};
use crate::native::ts::reads;

#[derive(serde::Deserialize)]
struct SourceRecord {
    path: PathBuf,
    text: String,
}

/// Owns invocation overlays, including restoration on malformed input.
#[derive(Default)]
struct Sources {
    files: Vec<(u64, PathBuf)>,
    previous: Vec<(PathBuf, Option<Arc<str>>)>,
}

impl Sources {
    fn load(path: &Path, cwd: &Path, input: impl Read) -> Result<Self, String> {
        if path == Path::new("-") {
            Self::read(BufReader::new(input), cwd)
        } else {
            let file = std::fs::File::open(cwd.join(path)).map_err(|error| format!("{}: {error}", path.display()))?;
            Self::read(BufReader::new(file), cwd)
        }
    }

    fn read(reader: impl BufRead, cwd: &Path) -> Result<Self, String> {
        let mut sources = Self::default();
        let mut seen = HashSet::new();
        for (index, line) in reader.lines().enumerate() {
            let parse = || -> Result<(), String> {
                let line = line.map_err(|error| error.to_string())?;
                let record: SourceRecord = serde_json::from_str(&line).map_err(|error| error.to_string())?;
                if record.path.as_os_str().is_empty() {
                    return Err("source path is empty".into());
                }
                let path = normalize_path(&cwd.join(&record.path));
                if !seen.insert(path.clone()) {
                    return Err(format!("duplicate source path: {}", record.path.display()));
                }
                sources.previous.push((path.clone(), reads::buffer(&path)));
                sources.files.push((record.text.len() as u64, path.clone()));
                reads::set_buffer(&path, &record.text);
                Ok(())
            };
            parse().map_err(|error| format!("sources line {}: {error}", index + 1))?;
        }
        Ok(sources)
    }
}

impl Drop for Sources {
    fn drop(&mut self) {
        for (path, previous) in &self.previous {
            match previous {
                Some(text) => reads::set_buffer(path, text),
                None => reads::drop_buffer(path),
            }
        }
    }
}

struct Output<'a> {
    cwd: &'a Path,
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
}

impl Output<'_> {
    fn out(&mut self, args: std::fmt::Arguments<'_>) {
        writeln!(self.stdout, "{args}").expect("write stdout");
    }

    fn err(&mut self, args: std::fmt::Arguments<'_>) {
        writeln!(self.stderr, "{args}").expect("write stderr");
    }
}

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
    /// JSONL source records ({path,text}); `-` reads stdin instead of a file
    #[arg(long, global = true, value_name = "FILE|-")]
    pub sources: Option<PathBuf>,
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

/// Runs one invocation without changing the process directory or exiting.
/// Writers receive terminal style sequences; the host chooses colour policy.
pub fn run<I, T, R, W, E>(args: I, cwd: &Path, input: R, mut stdout: W, mut stderr: E) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
    R: Read + Send + 'static,
    W: Write + Send + 'static,
    E: Write,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let writer: &mut dyn Write = if error.use_stderr() { &mut stderr } else { &mut stdout };
            write!(writer, "{}", error.render().ansi()).expect("write arguments");
            return error.exit_code() as u8;
        }
    };
    // Clap global arguments cannot reference root-only conflict IDs.
    if cli.lint.sources.is_some() && (!cli.lint.paths.is_empty() || cli.lint.fix) {
        let mut output = Output { cwd, stdout: &mut stdout, stderr: &mut stderr };
        render::error(&mut output, "--sources cannot be combined with positional lint paths or --fix");
        return 2;
    }
    if matches!(&cli.command, Some(Command::Lsp)) {
        if cli.lint.sources.is_some() {
            let mut output = Output { cwd, stdout: &mut stdout, stderr: &mut stderr };
            render::error(&mut output, "--sources cannot be combined with lsp");
            return 2;
        }
        return crate::lsp::run(input, stdout, &mut stderr);
    }
    let mut output = Output { cwd, stdout: &mut stdout, stderr: &mut stderr };
    let sources = match cli.lint.sources.as_ref().map(|path| Sources::load(path, cwd, input)).transpose() {
        Ok(sources) => sources,
        Err(error) => {
            render::error(&mut output, &error);
            return 2;
        }
    };
    let config = &cli.config;
    match cli.command {
        None => lint::run(&mut output, cli.lint, config, sources.as_ref()),
        Some(Command::Check) => match load(&mut output, config) {
            Ok(Project { config_ok: true, .. }) => {
                output.err(format_args!("config ok"));
                0
            }
            Ok(_) => 2,
            Err(code) => code,
        },
        Some(Command::Rules) => match load(&mut output, config) {
            Ok(p) => info::rules(&mut output, &p.linter),
            Err(code) => code,
        },
        Some(Command::Describe { name }) => match load(&mut output, config) {
            Ok(p) => info::describe(&mut output, p.symbols.as_deref().expect("tooling symbols"), &name),
            Err(code) => code,
        },
        Some(Command::Tree { file, all, at }) => match load(&mut output, config) {
            Ok(p) => match at {
                Some(at) => info::explain(&mut output, &p.linter, &file, &at),
                None => info::tree(&mut output, &p.linter, &file, all),
            },
            Err(code) => code,
        },
        Some(Command::Eval { target, expr, files }) => eval::run(&mut output, config, &target, &expr, &files),
        Some(Command::Functions) => info::functions(&mut output),
        Some(Command::Lsp) => unreachable!("LSP owns its streams"),
    }
}

/// A built linter, and whether its config had no errors.
pub struct Project {
    pub linter: Linter,
    pub symbols: Option<std::sync::Arc<Symbols>>,
    /// `false` when config errors were printed: the linter still runs with
    /// every part that built, but the command should fail in the end.
    pub config_ok: bool,
    /// Reading the config with its imports, then building the linter.
    pub load_time: std::time::Duration,
    pub build_time: std::time::Duration,
}

/// Loads and builds the linter, printing any config errors. Uses the
/// explicit `--config`, else the nearest `.zlint` upward. Only a
/// missing config is fatal: a broken one lints with what still works.
fn load(output: &mut Output<'_>, arg: &ConfigArg) -> Result<Project, u8> {
    load_with(output, arg, false)
}

fn load_for_lint(output: &mut Output<'_>, arg: &ConfigArg) -> Result<Project, u8> {
    load_with(output, arg, true)
}

fn load_with(output: &mut Output<'_>, arg: &ConfigArg, runtime: bool) -> Result<Project, u8> {
    let path = match &arg.config {
        Some(p) => normalize_path(&output.cwd.join(p)),
        None => match config::find(output.cwd) {
            Some(p) => p,
            None => {
                render::error(output, &config::not_found_message(output.cwd));
                return Err(2);
            }
        },
    };
    load_from_with(output, &path, &Disk, runtime)
}

fn load_from(output: &mut Output<'_>, path: &Path, provider: &dyn SourceProvider) -> Result<Project, u8> {
    load_from_with(output, path, provider, false)
}

fn load_from_with(output: &mut Output<'_>, path: &Path, provider: &dyn SourceProvider, runtime: bool) -> Result<Project, u8> {
    let root = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let started = std::time::Instant::now();
    let loaded = config::load_file(path, provider);
    let load_time = started.elapsed();
    let built = if runtime { build_for_lint(loaded, root) } else { build(loaded, root) };
    let build_time = started.elapsed() - load_time;
    let config_ok = built.errors.is_empty();
    if !config_ok {
        render::config_errors(output, &built.errors, &built.sources);
    }
    match built.linter {
        Some(linter) => Ok(Project { linter, symbols: built.symbols, config_ok, load_time, build_time }),
        None => Err(2),
    }
}
