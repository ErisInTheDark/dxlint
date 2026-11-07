//! Programs the root config declares (`command name "program args"`), which
//! rules run through `command_output`. A run's output is kept with the
//! inputs it reported, and holds while their text on disk does: a lint
//! records those inputs as reads, so caches and relints follow them like
//! any file a lint read.
//!
//! Output protocol: leading `dxlint-input <path>` lines name what the run
//! read (relative to the root); the rest of stdout is the payload. A
//! non-zero exit, output that is not UTF-8, or the time limit is an error.
//! A `batch` command runs once, without arguments, for every argument list:
//! its payload holds a section per list, each starting with a
//! `dxlint-payload <args>` (or `dxlint-error <args>`, the rest being why)
//! line.
//!
//! A language server runs commands off its lint threads ([`Commands::run_in_background`]):
//! until a run finishes, its answer is pending, and the lints that asked
//! read a marker that changes when it finishes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::fx_hash::FxHashMap;

use super::reads;

const TIME_LIMIT: Duration = Duration::from_secs(60);
const INPUT_PREFIX: &str = "dxlint-input ";
const PAYLOAD_PREFIX: &str = "dxlint-payload ";
const ERROR_PREFIX: &str = "dxlint-error ";
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

/// A successful run as saved between processes; inputs relative to the root.
pub struct KeptRun {
    pub name: String,
    pub argv: Vec<String>,
    pub args: Vec<String>,
    pub payload: Arc<str>,
    pub inputs: Vec<(String, u64)>,
}

/// Called with a finished background run's marker path.
type Finished = dyn Fn(PathBuf) + Send + Sync;

/// Replacements stop here: files all waiting on one run would otherwise each
/// cost a thread, and the run ends anyway.
const MAX_LANES_PER_JOB: usize = 8;

/// A run on the asking thread: waiters wake when it ends, and a run that never
/// got its result (a panic) gives way to the next asker.
struct Claim<'c> {
    commands: &'c Commands,
    key: &'c Key,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        let mut runs = self.commands.runs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(runs.get(self.key), Some(Slot::Running)) { runs.remove(self.key); }
        drop(runs);
        self.commands.finished.notify_all();
    }
}

/// A worker pool's size against how many of its workers sit waiting for a
/// command: those aren't working, so others may take their place.
#[derive(Default)]
pub struct Lanes {
    counts: Mutex<LaneCounts>,
    changed: Condvar,
}

#[derive(Default)]
struct LaneCounts {
    workers: usize,
    blocked: usize,
}

impl LaneCounts {
    fn working(&self) -> usize {
        self.workers.saturating_sub(self.blocked)
    }

    fn wants_worker(&self, jobs: usize) -> bool {
        self.working() < jobs && self.workers < jobs * MAX_LANES_PER_JOB
    }
}

/// While alive, one worker counts as waiting.
pub struct Blocked<'l>(&'l Lanes);

impl Drop for Blocked<'_> {
    fn drop(&mut self) {
        self.0.counts.lock().expect("lanes").blocked -= 1;
        self.0.changed.notify_all();
    }
}

impl Lanes {
    fn block(&self) -> Blocked<'_> {
        self.counts.lock().expect("lanes").blocked += 1;
        self.changed.notify_all();
        Blocked(self)
    }

    /// One more worker is starting.
    fn enter(&self) {
        self.counts.lock().expect("lanes").workers += 1;
    }

    /// A worker ended, however it ended.
    pub fn left(&self) {
        self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).workers -= 1;
        self.changed.notify_all();
    }

    /// Whether more than `jobs` workers work: one may end after its file.
    pub fn surplus(&self, jobs: usize) -> bool {
        self.counts.lock().expect("lanes").working() > jobs
    }

    /// Starts workers until `more_work` says none could help and all ended,
    /// keeping `jobs` of them working.
    pub fn supervise(&self, jobs: usize, more_work: impl Fn() -> bool, mut start: impl FnMut()) {
        loop {
            let counts = self.counts.lock().expect("lanes");
            if more_work() && counts.wants_worker(jobs) {
                drop(counts);
                self.enter();
                start();
                continue;
            }
            if !more_work() && counts.workers == 0 { return }
            drop(self.changed.wait(counts).expect("lanes"));
        }
    }
}

/// A command as the root config declares it.
pub struct Declared {
    pub argv: Vec<String>,
    pub batch: bool,
}

#[derive(Default)]
pub struct Commands {
    root: PathBuf,
    declared: FxHashMap<String, Declared>,
    runs: Mutex<FxHashMap<Key, Slot>>,
    /// Signals a finished run to those waiting for it.
    finished: Condvar,
    pub lanes: Lanes,
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
    pub fn new(root: PathBuf, declared: impl IntoIterator<Item = (String, Declared)>) -> Commands {
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

    /// The finished runs worth keeping past this process: their outputs
    /// depend on their command and inputs, not on this dxlint or its rules.
    pub fn kept_runs(&self) -> Vec<KeptRun> {
        let runs = self.runs.lock().expect("command runs");
        runs.iter().filter_map(|((name, args), slot)| {
            let Slot::Done(run) = slot else { return None };
            let payload = run.result.as_ref().ok()?.clone();
            let argv = self.declared.get(name)?.argv.clone();
            let inputs = run.inputs.iter().map(|(path, hash)| {
                (path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy().replace('\\', "/"), *hash)
            }).collect();
            Some(KeptRun { name: name.clone(), argv, args: args.clone(), payload, inputs })
        }).collect()
    }

    /// Takes back runs a previous process kept, for commands still declared
    /// alike. Each holds only while its inputs do, as any finished run.
    pub fn restore(&self, kept: Vec<KeptRun>) {
        let mut runs = self.runs.lock().expect("command runs");
        for run in kept {
            if self.declared.get(&run.name).is_none_or(|declared| declared.argv != run.argv) { continue }
            let inputs = run.inputs.into_iter().map(|(path, hash)| (crate::config::source::normalize_path(&self.root.join(path)), hash)).collect();
            runs.entry((run.name, run.args)).or_insert_with(|| Slot::Done(Arc::new(Run { result: Ok(run.payload), inputs })));
        }
    }

    /// `name`'s output for `args`, recording what it depends on.
    pub fn answer(self: &Arc<Self>, name: &str, args: &[String]) -> Answer {
        let Some(declared) = self.declared.get(name) else {
            return Answer::Failed(format!("no command `{name}` in the root config").into());
        };
        if !declared.batch { return self.run_answer(name, &declared.argv, args) }
        match self.run_answer(name, &declared.argv, &[]) {
            Answer::Ready(payload) => section(&payload, &args.join(" "))
                .unwrap_or_else(|| Answer::Failed(format!("`{name}` printed no section for `{}`", args.join(" ")).into())),
            other => other,
        }
    }

    /// The run of `argv` plus `args` (named `name`), recording what it
    /// depends on.
    fn run_answer(self: &Arc<Self>, name: &str, argv: &[String], args: &[String]) -> Answer {
        let key: Key = (name.to_owned(), args.to_vec());
        let marker = self.marker(&key);
        let mode = match &*self.mode.lock().expect("command mode") {
            Mode::Inline => None,
            Mode::Background(finished) => Some(finished.clone()),
            Mode::Disabled => return Answer::Disabled,
        };
        let mut runs = self.runs.lock().expect("command runs");
        loop {
            match runs.get(&key) {
                Some(Slot::Done(run)) if run.inputs.iter().all(|(path, hash)| reads::disk_hash(path) == *hash) => {
                    let run = run.clone();
                    drop(runs);
                    return self.recorded(&run, &marker);
                }
                Some(Slot::Running) if mode.is_none() => {
                    let _blocked = self.lanes.block();
                    runs = self.finished.wait(runs).expect("command runs");
                }
                _ => break,
            }
        }
        match mode {
            None => {
                runs.insert(key.clone(), Slot::Running);
                drop(runs);
                let claim = Claim { commands: self, key: &key };
                let run = {
                    let _blocked = self.lanes.block();
                    Arc::new(self.run(argv, args))
                };
                self.runs.lock().expect("command runs").insert(key.clone(), Slot::Done(run.clone()));
                drop(claim);
                self.recorded(&run, &marker)
            }
            Some(finished) => {
                // The lint reads the marker as it is now; the run changes it.
                reads::read(&marker);
                if !matches!(runs.get(&key), Some(Slot::Running)) {
                    runs.insert(key.clone(), Slot::Running);
                    let (commands, argv) = (self.clone(), argv.to_vec());
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

/// A batch payload's section for `args`: its text, or the error it gives.
fn section(payload: &str, args: &str) -> Option<Answer> {
    let mut found: Option<(bool, String)> = None;
    for line in payload.split_inclusive('\n') {
        let header = line.trim_end_matches(['\r', '\n']);
        let starts = header.strip_prefix(PAYLOAD_PREFIX).map(|of| (true, of)).or_else(|| header.strip_prefix(ERROR_PREFIX).map(|of| (false, of)));
        match (starts, &mut found) {
            (Some(_), Some(_)) => break,
            (Some((ok, of)), None) if of == args => found = Some((ok, String::new())),
            (Some(_), None) => {}
            (None, Some((_, text))) => text.push_str(line),
            (None, None) => {}
        }
    }
    found.map(|(ok, text)| if ok { Answer::Ready(text.into()) } else { Answer::Failed(text.trim_end().to_owned().into()) })
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
    fn batch_payloads_answer_each_argument_list_from_its_own_section() {
        let payload = "dxlint-payload a 1\nx\r\ny\ndxlint-error b\nunknown version b\ndxlint-payload c\n";
        assert_eq!(section(payload, "a 1"), Some(Answer::Ready("x\r\ny\n".into())));
        assert_eq!(section(payload, "b"), Some(Answer::Failed("unknown version b".into())));
        assert_eq!(section(payload, "c"), Some(Answer::Ready("".into())));
        assert_eq!(section(payload, "d"), None);
    }

    #[test]
    fn a_running_command_keeps_no_other_command_waiting() {
        let dir = crate::config::source::normalize_path(&std::env::temp_dir().join("dxlint-commands-lock-test"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let node = |code: &str| Declared { argv: vec!["node".to_owned(), "-e".to_owned(), code.to_owned()], batch: false };
        let waits = "const fs = require('fs'); fs.writeFileSync('started', ''); const start = Date.now(); while (!fs.existsSync('token')) { if (Date.now() - start > 20000) process.exit(3); Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 20); } process.stdout.write('ok')";
        let commands = Arc::new(Commands::new(dir.clone(), [("waits".to_owned(), node(waits)), ("writes".to_owned(), node("require('fs').writeFileSync('token', '')"))]));
        let running = commands.clone();
        let asked = std::thread::spawn(move || running.answer("waits", &[]));
        while !dir.join("started").exists() { std::thread::yield_now() }
        assert_eq!(commands.answer("writes", &[]), Answer::Ready("".into()));
        assert_eq!(asked.join().expect("no panic"), Answer::Ready("ok".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn wants_worker(lanes: &Lanes, jobs: usize) -> bool {
        lanes.counts.lock().expect("lanes").wants_worker(jobs)
    }

    #[test]
    fn supervised_workers_are_replaced_while_blocked_and_leave_after() {
        let lanes = Lanes::default();
        lanes.enter();
        assert!(!wants_worker(&lanes, 1));
        assert!(!lanes.surplus(1));
        let blocked = lanes.block();
        assert!(wants_worker(&lanes, 1));
        lanes.enter();
        assert!(!wants_worker(&lanes, 1));
        drop(blocked);
        assert!(lanes.surplus(1));
        lanes.left();
        assert!(!lanes.surplus(1));
    }

    #[test]
    fn replacements_stop_at_the_cap() {
        let lanes = Lanes::default();
        let blocked: Vec<_> = (0..MAX_LANES_PER_JOB).map(|_| { lanes.enter(); lanes.block() }).collect();
        assert!(!wants_worker(&lanes, 1));
        drop(blocked);
    }

    #[test]
    fn runs_again_only_when_a_reported_input_changes_and_failures_say_why() {
        let dir = crate::config::source::normalize_path(&std::env::temp_dir().join("dxlint-commands-test"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("input.txt"), "a").expect("input");
        let script = "const fs = require('fs'); fs.appendFileSync('runs.txt', 'x'); process.stdout.write('dxlint-input input.txt\\n' + fs.readFileSync('input.txt', 'utf8'))";
        let node = |code: &str| Declared { argv: vec!["node".to_owned(), "-e".to_owned(), code.to_owned()], batch: false };
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
