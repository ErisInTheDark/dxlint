// Refreshes the profile the Windows binary is optimised with, by linting
// real projects on an instrumented build, one core:
//   node scripts/build-profile.mjs <project> [<project> ...]
// Needs `rustup component add llvm-tools-preview`. Run build:binaries after.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { root, windowsProfile } from "./binary-sources.mjs";

const projects = process.argv.slice(2);
if (projects.length === 0) throw new Error("name at least one project to train on");
const target = "x86_64-pc-windows-msvc";
const work = path.join(root, "target", "profile");
const raw = path.join(work, "raw");
fs.rmSync(raw, { recursive: true, force: true });

function run(program, args, options) {
  const result = spawnSync(program, args, { stdio: ["ignore", "ignore", "inherit"], ...options });
  if (result.error) throw result.error;
  return result.status;
}

const flags = `-Cprofile-generate=${raw.replaceAll("\\", "/")}`;
// The linker's temp files stay in the workspace, as build:binaries keeps them.
const tmp = path.join(root, "target", "tmp");
fs.mkdirSync(tmp, { recursive: true });
const built = run("cargo", ["build", "--release", "--locked", "--bin", "dxlint", "-j", "1", "--target", target, "--target-dir", path.join(work, "generate")],
  { cwd: root, env: { ...process.env, TMP: tmp, TEMP: tmp, CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS: flags } });
if (built !== 0) process.exit(built ?? 1);
const instrumented = path.join(work, "generate", target, "release", "dxlint.exe");
for (const project of projects) {
  // A lint that finds problems exits non-zero: only the profile matters here.
  run(instrumented, ["--no-cache", "--jobs", "1", "--no-workbench-view", "--format", "jsonl"],
    { cwd: path.resolve(project), env: { ...process.env, DXLINT_LIBRARY: root } });
}
const sysroot = spawnSync("rustc", ["--print", "sysroot"], { encoding: "utf8" }).stdout.trim();
const merged = run(path.join(sysroot, "lib", "rustlib", target, "bin", "llvm-profdata.exe"), ["merge", "-o", windowsProfile, raw]);
if (merged !== 0) process.exit(merged ?? 1);
console.log(`${path.relative(root, windowsProfile)}: ${fs.statSync(windowsProfile).size} bytes`);
