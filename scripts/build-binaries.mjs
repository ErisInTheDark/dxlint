// Builds the release binaries the npm package ships into package/bin.
//   node scripts/build-binaries.mjs [win32-x64|linux-x64|darwin-arm64 ...]   (default: all)
// linux-x64 is a static musl build linked with rust-lld (.cargo/config.toml),
// so it builds on Windows too: `rustup target add x86_64-unknown-linux-musl`.
// The macOS build links with the toolchain's rust-lld (scripts/darwin-link.cmd,
// so Windows hosts only) against zig's libSystem stub (no Apple SDK):
// `pip install ziglang`, `rustup target add aarch64-apple-darwin`. Zig's own
// linker built binaries that crash at start. Apple Silicon only: Intel Macs
// get no binary.
// One cargo run builds every target side by side, on all cores.
// Building every platform stamps package/source.sha256; a partial build
// removes the stamp, so publishing waits for a full one.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { root, removeStamp, sourceHash, writeStamp } from "./binary-sources.mjs";
const platforms = {
  // The host build: it also refreshes target/release/dxlint.exe, for editors pointed there.
  "win32-x64": { target: "x86_64-pc-windows-msvc", built: "dxlint.exe", shipped: "dxlint-win32-x64.exe", env: windowsEnv, refresh: "dxlint.exe" },
  "linux-x64": { target: "x86_64-unknown-linux-musl", built: "dxlint", shipped: "dxlint-linux-x64" },
  "darwin-arm64": { target: "aarch64-apple-darwin", built: "dxlint", shipped: "dxlint-darwin-arm64", env: darwinEnv },
};

function windowsEnv() {
  if (process.platform !== "win32") throw new Error("win32-x64 builds on Windows only");
  return {};
}

// rust-lld as ld64.lld, finding libSystem in the ziglang Python package.
function darwinEnv() {
  if (process.platform !== "win32") throw new Error("darwin-arm64 builds on Windows only (scripts/darwin-link.cmd)");
  const found = spawnSync("python", ["-c", "import ziglang, os; print(os.path.dirname(ziglang.__file__))"], { encoding: "utf8" });
  if (found.status !== 0) throw new Error("macOS builds need zig's libSystem stub: pip install ziglang");
  const stubs = path.join(found.stdout.trim(), "lib", "libc", "darwin").replaceAll("\\", "/");
  const sysroot = spawnSync("rustc", ["--print", "sysroot"], { encoding: "utf8" }).stdout.trim();
  const host = /^host: (.+)$/m.exec(spawnSync("rustc", ["-vV"], { encoding: "utf8" }).stdout)[1];
  return {
    SDKROOT: stubs,
    DXLINT_RUST_LLD: path.join(sysroot, "lib", "rustlib", host, "bin", "rust-lld.exe"),
    CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER: path.join(root, "scripts", "darwin-link.cmd"),
    CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS: `-C linker-flavor=ld64.lld -C link-arg=-L${stubs}`,
  };
}

const wanted = process.argv.slice(2);
for (const name of wanted) if (!platforms[name]) throw new Error(`unknown platform ${name}; known: ${Object.keys(platforms).join(", ")}`);
const building = (wanted.length ? [...new Set(wanted)] : Object.keys(platforms)).map(name => ({ name, ...platforms[name] }));
const env = Object.assign({ ...process.env }, ...building.map(platform => platform.env?.() ?? {}));
const hash = sourceHash();
removeStamp();
const result = spawnSync("cargo", ["build", "--release", "--locked", "--bin", "dxlint", ...building.flatMap(platform => ["--target", platform.target])],
  { cwd: root, stdio: "inherit", env });
if (result.status !== 0) process.exit(result.status ?? 1);
for (const { name, target, built, shipped, refresh } of building) {
  const binary = path.join(root, "target", target, "release", built);
  const destination = path.join(root, "package", "bin", shipped);
  fs.copyFileSync(binary, destination);
  fs.chmodSync(destination, 0o755);
  console.log(`${name}: package/bin/${shipped}`);
  if (refresh) {
    try {
      fs.mkdirSync(path.join(root, "target", "release"), { recursive: true });
      fs.copyFileSync(binary, path.join(root, "target", "release", refresh));
    } catch (error) {
      console.warn(`target/release/${refresh} not refreshed (${error.code}): close what runs it and copy ${path.relative(root, binary)}`);
    }
  }
}
if (building.length === Object.keys(platforms).length) {
  writeStamp(hash);
  console.log("stamped package/source.sha256");
}
