// Builds the release binaries the npm package ships into package/bin.
//   node scripts/build-binaries.mjs [win32-x64|linux-x64|darwin-arm64 ...]   (default: all)
// linux-x64 is a static musl build linked with rust-lld (.cargo/config.toml),
// so it builds on Windows too: `rustup target add x86_64-unknown-linux-musl`.
// The macOS builds link with zig, which carries the system library stubs
// (no Apple SDK): `pip install ziglang`, `cargo install cargo-zigbuild`,
// `rustup target add aarch64-apple-darwin`. Apple Silicon only: Intel Macs
// get no binary.
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
  "darwin-arm64": { target: "aarch64-apple-darwin", built: "dxlint", shipped: "dxlint-darwin-arm64", zig: true },};

// cargo-zigbuild's zig (the ziglang Python package's), with its caches under
// target/ (sandboxed shells cannot write the user cache).
function zigEnv() {
  const found = spawnSync("python", ["-c", "import ziglang, os; print(os.path.dirname(ziglang.__file__))"], { encoding: "utf8" });
  if (found.status !== 0) throw new Error("macOS builds need zig: pip install ziglang");
  const zig = path.join(found.stdout.trim(), process.platform === "win32" ? "zig.exe" : "zig");
  const cache = path.join(root, "target", "zig-cache");
  return { ...process.env, CARGO_ZIGBUILD_ZIG_PATH: zig, CARGO_ZIGBUILD_CACHE_DIR: path.join(root, "target", "zigbuild-cache"), ZIG_GLOBAL_CACHE_DIR: cache, ZIG_LOCAL_CACHE_DIR: cache };
}

const wanted = process.argv.slice(2);
for (const name of wanted) if (!platforms[name]) throw new Error(`unknown platform ${name}; known: ${Object.keys(platforms).join(", ")}`);
const building = wanted.length ? [...new Set(wanted)] : Object.keys(platforms);
const hash = sourceHash();
removeStamp();
for (const name of building) {
  const { target, built, shipped, zig } = platforms[name];
  if (name === "win32-x64" && process.platform !== "win32") throw new Error("win32-x64 builds on Windows only");
  const result = spawnSync("cargo", [zig ? "zigbuild" : "build", "--release", "--locked", "--bin", "dxlint", ...(target ? ["--target", target] : [])],
    { cwd: root, stdio: "inherit", env: zig ? zigEnv() : process.env });
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
