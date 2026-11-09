// Checks a change with one compile on one core: builds every workspace
// crate's test binary, runs their tests, then rule fixtures and self-lint
// from the dxlint crate's binary.
// Usage: npm run check [-- [--no-tests] [--no-fixtures] [fixture paths...]]
// Fixture paths default to all of fixtures/.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const flags = new Set(args.filter(arg => arg.startsWith("--")));
const fixtures = args.filter(arg => !arg.startsWith("--"));
const tmp = path.join(root, "target", "tmp");
fs.mkdirSync(tmp, { recursive: true });
const env = { ...process.env, TMP: tmp, TEMP: tmp };

let failed = false;
const step = (name, command, commandArgs, extraEnv = {}, stderr = "inherit") => {
  const started = Date.now();
  const result = spawnSync(command, commandArgs, { cwd: root, env: { ...env, ...extraEnv }, stdio: ["ignore", "pipe", stderr], encoding: "utf8", maxBuffer: 1 << 30 });
  const seconds = ((Date.now() - started) / 1000).toFixed(1);
  return { ...result, done: ok => {
    failed ||= !ok;
    console.log(`${ok ? "ok  " : "FAIL"} ${name} (${seconds} s)`);
  } };
};

const build = step("build", "cargo", ["test", "--workspace", "--lib", "-j", "1", "--no-run", "--message-format=json-render-diagnostics"]);
// Each crate's test binary, by crate name.
const binaries = new Map(build.stdout.split("\n").filter(line => line.startsWith("{")).map(line => JSON.parse(line))
  .filter(message => message.reason === "compiler-artifact" && message.target?.kind?.includes("lib") && message.profile?.test && message.executable)
  .map(message => [message.target.name, message.executable]));
const executable = binaries.get("dxlint");
build.done(build.status === 0 && executable !== undefined);
if (!executable) process.exit(1);

if (!flags.has("--no-tests")) {
  for (const [crate, binary] of binaries) {
    const tests = step(`tests ${crate}`, binary, ["--test-threads=1", "-q"]);
    const summary = tests.stdout.split("\n").filter(line => line.startsWith("test result") || line.includes("FAILED") || line.includes("panicked"));
    summary.forEach(line => console.log(`     ${line}`));
    tests.done(tests.status === 0);
  }
}

const cli = (name, cliArgs, stderr) =>
  step(name, executable, ["--ignored", "--exact", "cli::tests::cli_from_env", "-q", "--test-threads=1"], { DXLINT_CLI_ARGS: cliArgs }, stderr);

if (!flags.has("--no-fixtures")) {
  // Failing scenarios' details go to a file: the summary counts them.
  const details = path.join(tmp, "check-fixtures.txt");
  const file = fs.openSync(details, "w");
  const run = cli("fixtures", ["fixtures", ...fixtures, "--jobs", "1"].join(" "), file);
  fs.closeSync(file);
  const summary = run.stdout.split("\n").find(line => /scenarios, \d+ failed/.test(line));
  console.log(`     ${summary?.trim() ?? "no summary"}; failures in ${path.relative(root, details)}`);
  run.done(summary !== undefined);
}

const lint = cli("self-lint", "--jobs 1 --no-workbench-view");
lint.stdout.split("\n").filter(line => line.trim() && !line.startsWith("running") && !line.startsWith("test result")).slice(-3).forEach(line => console.log(`     ${line.trim()}`));
lint.done(lint.status === 0);
process.exit(failed ? 1 : 0);
