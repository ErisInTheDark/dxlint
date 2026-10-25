// Builds the release binaries the npm package ships into package/bin.
//   node scripts/build-binaries.mjs [win32-x64|linux-x64 ...]   (default: both)
// linux-x64 is a static musl build linked with rust-lld (.cargo/config.toml),
// so it builds on Windows too: `rustup target add x86_64-unknown-linux-musl`.
// Building every platform stamps package/source.sha256; a partial build
// removes the stamp, so publishing waits for a full one.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { root, removeStamp, sourceHash, writeStamp } from "./binary-sources.mjs";
const platforms = {
  // The host build: it also refreshes target/release, which the editor extension runs.
  "win32-x64": { target: null, built: "dxlint.exe", shipped: "dxlint-win32-x64.exe" },
  "linux-x64": { target: "x86_64-unknown-linux-musl", built: "dxlint", shipped: "dxlint-linux-x64" },
};

const wanted = process.argv.slice(2);
for (const name of wanted) if (!platforms[name]) throw new Error(`unknown platform ${name}; known: ${Object.keys(platforms).join(", ")}`);
const building = wanted.length ? [...new Set(wanted)] : Object.keys(platforms);
const hash = sourceHash();
removeStamp();
for (const name of building) {
  const { target, built, shipped } = platforms[name];
  if (name === "win32-x64" && process.platform !== "win32") throw new Error("win32-x64 builds on Windows only");
  const result = spawnSync("cargo", ["build", "--release", "--locked", "--bin", "dxlint", ...(target ? ["--target", target] : [])], { cwd: root, stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
  const destination = path.join(root, "package", "bin", shipped);
  fs.copyFileSync(path.join(root, "target", ...(target ? [target] : []), "release", built), destination);
  fs.chmodSync(destination, 0o755);
  console.log(`${name}: package/bin/${shipped}`);
}
if (building.length === Object.keys(platforms).length) {
  writeStamp(hash);
  console.log("stamped package/source.sha256");
}
