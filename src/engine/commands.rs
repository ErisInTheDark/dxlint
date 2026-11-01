//! Programs the root config declares (`command name "program args"`), which
//! rules run through `command_output`. A run's output is kept with the
//! inputs it reported, and holds while their text on disk does: a lint
//! records those inputs as reads, so caches and relints follow them like
//! any file a lint read.
//!
//! Output protocol: leading `dxlint-input <path>` lines name what the run
//! read (relative to the root); the rest of stdout is the payload. A
//! non-zero exit, output that is not UTF-8, or the time limit is an error.
//!
//! A language server runs commands off its lint threads ([`Commands::run_in_background`]):
//! until a run finishes, its answer is pending, and the lints that asked
//! read a marker that changes when it finishes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::fx_hash::FxHashMap;

use super::reads;

const TIME_LIMIT: Duration = Duration::from_secs(60);
const INPUT_PREFIX: &str = "dxlint-input ";
/// Lines of stderr an error keeps.
const STDERR_TAIL: usize = 8;

/// What a rule gets for a command and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Ready(Arc<str>),
    Failed(Arc<str>),
    /// Running in the background; the lint is redone when it finishes.
    Pending,
    /// This host runs no commands (an untrusted workspace).
    Disabled,
    Unknown,
}

type Key = (String, Vec<String>);

/// A finished run: the payload or error, and each reported input's text
/// hash on disk.
struct Run {
    result: Result<Arc<str>, Arc<str>>,
    inputs: Vec<(PathBuf, u64)>,
}

enum Slot {
    Done(Arc<Run>),
    Running,
}

/// Called with a finished background run's marker path.
type Finished = dyn Fn(PathBuf) + Send + Sync;

#[derive(Default)]
pub struct Commands {
    root: PathBuf,
    declared: FxHashMap<String, Vec<String>>,
    runs: Mutex<FxHashMap<Key, Slot>>,
    mode: Mutex<Mode>,
}

#[derive(Default)]
enum Mode {
    /// Runs on the asking thread (the CLI).
    #[default]
    Inline,
    Background(Arc<Finished>),
    Disabled,
}

impl Commands {
    pub fn new(root: PathBuf, declared: impl IntoIterator<Item = (String, Vec<String>)>) -> Commands {
        Commands { root, declared: declared.into_iter().collect(), ..Default::default() }
    }

    /// From now on, runs happen off the asking thread; `finished` gets the
    /// marker path of each finished run, to relint what read it.
    pub fn run_in_background(&self, finished: Arc<Finished>) {
        *self.mode.lock().expect("command mode") = Mode::Background(finished);
    }

    pub fn disable(&self) {
        *self.mode.lock().expect("command mode") = Mode::Disabled;
    }

    /// Forgets failed runs (a failure reports no inputs to wait on), and in
    /// the background has the lints that saw them asked again.
    pub fn forget_failures(&self) {
        let mut failed = Vec::new();
        self.runs.lock().expect("command runs").retain(|key, slot| {
            let keep = !matches!(slot, Slot::Done(run) if run.result.is_err());
            if !keep { failed.push(self.marker(key)) }
            keep
        });
        let Mode::Background(finished) = &*self.mode.lock().expect("command mode") else { return };
        for marker in failed {
            reads::set_buffer(&marker, "retrying");
            finished(marker);
        }
    }

    /// `name`'s output for `args`, recording what it depends on.
    pub fn answer(self: &Arc<Self>, name: &str, args: &[String]) -> Answer {
        let Some(argv) = self.declared.get(name) else {
            return Answer::Failed(format!("no command `{name}` in the root config").into());
        };
        let key: Key = (name.to_owned(), args.to_vec());
        let marker = self.marker(&key);
        let mode = match &*self.mode.lock().expect("command mode") {
            Mode::Inline => None,
            Mode::Background(finished) => Some(finished.clone()),
            Mode::Disabled => return Answer::Disabled,
        };
        let mut runs = self.runs.lock().expect("command runs");
        if let Some(Slot::Done(run)) = runs.get(&key) {
            if run.inputs.iter().all(|(path, hash)| reads::disk_hash(path) == *hash) {
                let run = run.clone();
                drop(runs);
                return self.recorded(&run, &marker);
            }
        }
        match mode {
            None => {
                let run = Arc::new(self.run(argv, args));
                runs.insert(key, Slot::Done(run.clone()));
                drop(runs);
                self.recorded(&run, &marker)
            }
            Some(finished) => {
                // The lint reads the marker as it is now; the run changes it.
                reads::read(&marker);
                if !matches!(runs.get(&key), Some(Slot::Running)) {
                    runs.insert(key.clone(), Slot::Running);
                    let (commands, argv) = (self.clone(), argv.clone());
                    std::thread::spawn(move || {
                        let run = Arc::new(commands.run(&argv, &key.1));
                        let done = format!("{:?}", run.result.as_ref().map(|payload| reads::hash_bytes(payload.as_bytes())));
                        commands.runs.lock().expect("command runs").insert(key, Slot::Done(run));
                        reads::set_buffer(&marker, &done);
                        finished(marker);
                    });
                }
                Answer::Pending
            }
        }
    }

    /// A virtual file standing for a command run, read by lints waiting
    /// for it.
    fn marker(&self, (name, args): &Key) -> PathBuf {
        let mut marker = self.root.join(".dxlint-commands").join(name);
        for arg in args { marker.push(arg.replace(['/', '\\', ':'], "_")) }
        marker
    }

    /// `run`'s answer, after recording its inputs (and the marker) as reads.
    fn recorded(&self, run: &Run, marker: &Path) -> Answer {
        reads::read(marker);
        for (path, _) in &run.inputs { reads::read(path); }
        match &run.result {
            Ok(payload) => Answer::Ready(payload.clone()),
            Err(error) => Answer::Failed(error.clone()),
        }
    }

    fn run(&self, argv: &[String], args: &[String]) -> Run {
        match execute(&self.root, argv, args) {
            Ok(stdout) => {
                let (inputs, payload) = split_output(&stdout);
                let inputs = inputs.into_iter().map(|path| {
                    let path = crate::config::source::normalize_path(&self.root.join(path));
                    let hash = reads::disk_hash(&path);
                    (path, hash)
                }).collect();
                Run { result: Ok(payload.into()), inputs }
            }
            Err(error) => Run { result: Err(error.into()), inputs: Vec::new() },
        }
    }
}

/// The paths a run reported reading, and its payload.
fn split_output(stdout: &str) -> (Vec<&str>, &str) {
    let mut inputs = Vec::new();
    let mut rest = stdout;
    while let Some(line) = rest.strip_prefix(INPUT_PREFIX) {
        let (path, after) = line.split_once('\n').unwrap_or((line, ""));
        inputs.push(path.trim_end_matches('\r'));
        rest = after;
    }
    (inputs, rest)
}

/// Runs `argv` plus `args` in `root` with no stdin, within the time limit;
/// stdout, or why it failed.
fn execute(root: &Path, argv: &[String], args: &[String]) -> Result<String, String> {
    use std::io::Read;
    let (program, fixed) = argv.split_first().ok_or("empty command")?;
    let mut child = std::process::Command::new(program)
        .args(fixed).args(args).current_dir(root)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
        .spawn().map_err(|e| format!("cannot run `{program}`: {e}"))?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out = std::thread::spawn(move || { let mut bytes = Vec::new(); stdout.read_to_end(&mut bytes).map(|_| bytes) });
    let err = std::thread::spawn(move || { let mut text = String::new(); let _ = stderr.read_to_string(&mut text); text });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? { break status }
        if started.elapsed() > TIME_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("`{program}` ran longer than {} s", TIME_LIMIT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = out.join().map_err(|_| "stdout reader panicked")?.map_err(|e| e.to_string())?;
    let stderr = err.join().unwrap_or_default();
    if !status.success() {
        let lines: Vec<&str> = stderr.lines().collect();
        let tail = lines[lines.len().saturating_sub(STDERR_TAIL)..].join("\n");
        return Err(format!("`{program}` failed ({status}){}{tail}", if tail.is_empty() { "" } else { ":\n" }));
    }
    String::from_utf8(stdout).map_err(|_| format!("`{program}` printed output that is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_splits_into_reported_inputs_and_payload() {
        assert_eq!(split_output("dxlint-input a.ts\r\ndxlint-input b/c.ts\nline 1\ndxlint-input not-an-input\n"),
            (vec!["a.ts", "b/c.ts"], "line 1\ndxlint-input not-an-input\n"));
        assert_eq!(split_output("payload only"), (vec![], "payload only"));
    }

    #[test]
    fn runs_again_only_when_a_reported_input_changes_and_failures_say_why() {
        let dir = crate::config::source::normalize_path(&std::env::temp_dir().join("dxlint-commands-test"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("input.txt"), "a").expect("input");
        let script = "const fs = require('fs'); fs.appendFileSync('runs.txt', 'x'); process.stdout.write('dxlint-input input.txt\\n' + fs.readFileSync('input.txt', 'utf8'))";
        let node = |code: &str| vec!["node".to_owned(), "-e".to_owned(), code.to_owned()];
        let commands = Arc::new(Commands::new(dir.clone(), [("read".to_owned(), node(script)), ("fail".to_owned(), node("console.error('boom'); process.exit(3)"))]));
        let runs = || std::fs::read_to_string(dir.join("runs.txt")).map_or(0, |runs| runs.len());
        assert_eq!(commands.answer("read", &[]), Answer::Ready("a".into()));
        assert_eq!(commands.answer("read", &[]), Answer::Ready("a".into()));
        assert_eq!(runs(), 1);
        std::fs::write(dir.join("input.txt"), "b").expect("input");
        assert_eq!(commands.answer("read", &[]), Answer::Ready("b".into()));
        assert_eq!(runs(), 2);
        assert!(matches!(commands.answer("fail", &[]), Answer::Failed(why) if why.contains("boom")));
        assert!(matches!(commands.answer("nope", &[]), Answer::Failed(why) if why.contains("no command `nope`")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
