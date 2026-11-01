// The macOS linker for scripts/build-binaries.mjs: rustc runs it (through
// darwin-link.cmd) as ld64.lld, and it runs rust-lld (DXLINT_RUST_LLD) after
// two fixes:
// - libc and libm are libSystem on macOS, and the only stub on hand is
//   libSystem.tbd, so -lc and -lm are dropped;
// - rustc writes response files with Windows paths, which lld reads with
//   backslashes as escapes, so their backslashes become slashes.
import fs from "node:fs";
import { spawnSync } from "node:child_process";

const aliases = /^"?-l[cm]"?$/;
const args = process.argv.slice(2).filter(arg => !aliases.test(arg)).map(arg => {
  if (!arg.startsWith("@")) return arg;
  const file = `${arg.slice(1)}.lld`;
  const lines = fs.readFileSync(arg.slice(1), "utf8").split(/\r?\n/).filter(line => !aliases.test(line));
  fs.writeFileSync(file, lines.join("\n").replaceAll("\\", "/"));
  return `@${file}`;
});
const result = spawnSync(process.env.DXLINT_RUST_LLD, args, { stdio: "inherit" });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
