// Prepares and publishes the npm package in package/.
//   --prepare-version [--version <x.y.z>]  bump (or set) package/package.json's version; prints it
//   --pack                                 npm pack into package/
//   --publish --version <x.y.z>            pack, then npm publish that tarball, after checking the committed version
// The library (std/, rules/), README.md and LICENSE are copied in from the
// repository root for the pack and removed afterwards; the binaries in
// package/bin are committed (scripts/build-binaries.mjs) and must match
// package/source.sha256, or packing stops (scripts/binary-sources.mjs). The tarball's
// executables are marked 0755 in the tarball itself, so a pack on any host
// (Windows has no executable bit) ships the same package.
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { checkStamp } from "./binary-sources.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageDir = path.join(root, "package");
const manifestPath = path.join(packageDir, "package.json");
const copied = ["std", "rules", "README.md", "LICENSE"];
const binaries = ["dxlint-win32-x64.exe", "dxlint-linux-x64"];
// Tarball entries that must be executable: the launcher and the Linux binary.
const executables = ["package/bin/dxlint", "package/bin/dxlint-linux-x64"];
const BLOCK = 512;

function stableVersion(value) {
  const match = typeof value === "string" && /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(value);
  if (!match) throw new Error(`Version must be major.minor.patch; got ${JSON.stringify(value)}.`);
  return match.slice(1).map(Number);
}

function readManifest() {
  return JSON.parse(fs.readFileSync(manifestPath, "utf8"));
}

function prepareVersion(requested) {
  const manifest = readManifest();
  const [major, minor, patch] = stableVersion(manifest.version);
  const version = requested || `${major}.${minor}.${patch + 1}`;
  stableVersion(version);
  if (version !== manifest.version) {
    manifest.version = version;
    fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  }
  return version;
}

function withLibrary(run) {
  for (const binary of binaries) {
    if (!fs.existsSync(path.join(packageDir, "bin", binary))) throw new Error(`package/bin/${binary} is missing; run npm run build:binaries.`);
  }
  checkStamp();
  try {
    for (const name of copied) {
      fs.cpSync(path.join(root, name), path.join(packageDir, name), { recursive: true });
    }
    run();
  } finally {
    for (const name of copied) fs.rmSync(path.join(packageDir, name), { recursive: true, force: true });
  }
}

// Sets the mode of `names` to 0755 in the gzipped tar at `file` (ustar
// headers, with pax `path` records for long names).
function markExecutable(file, names) {
  const tar = zlib.gunzipSync(fs.readFileSync(file));
  const marked = new Set();
  let paxPath = null;
  for (let at = 0; at + BLOCK <= tar.length;) {
    const header = tar.subarray(at, at + BLOCK);
    if (header.every(byte => byte === 0)) break;
    const field = (start, length) => header.subarray(start, start + length).toString("latin1").replace(/\0[\s\S]*$/, "");
    const size = parseInt(field(124, 12).trim() || "0", 8);
    const type = field(156, 1);
    const prefix = field(345, 155);
    const name = paxPath ?? (prefix ? `${prefix}/${field(0, 100)}` : field(0, 100));
    paxPath = null;
    if (type === "x") {
      paxPath = /(?:^|\n)\d+ path=([^\n]*)\n/.exec(tar.subarray(at + BLOCK, at + BLOCK + size).toString("utf8"))?.[1] ?? null;
    } else if (names.includes(name)) {
      header.write("0000755\0", 100, 8, "latin1");
      header.fill(" ", 148, 156, "latin1");
      const sum = header.reduce((total, byte) => total + byte, 0);
      header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, 8, "latin1");
      marked.add(name);
    }
    at += BLOCK + Math.ceil(size / BLOCK) * BLOCK;
  }
  const missing = names.filter(name => !marked.has(name));
  if (missing.length) throw new Error(`${path.basename(file)} has no ${missing.join(", ")}`);
  fs.writeFileSync(file, zlib.gzipSync(tar, { level: 9 }));
}

// npm pack into package/, with executables marked; returns the tarball path.
function pack() {
  const { name, version } = readManifest();
  const file = path.join(packageDir, `${name.replace(/^@/, "").replace("/", "-")}-${version}.tgz`);
  withLibrary(() => npm(["pack", "--pack-destination", packageDir]));
  markExecutable(file, executables);
  return file;
}

function npm(args) {
  // npm is a .cmd shim on Windows, which only a shell runs.
  const result = process.platform === "win32"
    ? spawnSync(["npm", ...args.map(arg => `"${arg}"`)].join(" "), { cwd: packageDir, stdio: "inherit", shell: true })
    : spawnSync("npm", args, { cwd: packageDir, stdio: "inherit" });
  if (result.status !== 0) throw new Error(`npm ${args.join(" ")} failed`);
}

function main(args) {
  const [mode, flag, value] = args;
  if (flag !== undefined && (flag !== "--version" || !value) || args.length > 3) throw new Error("usage: see the header of scripts/publish-package.mjs");
  if (mode === "--prepare-version") {
    process.stdout.write(`${prepareVersion(value)}\n`);
  } else if (mode === "--pack") {
    pack();
  } else if (mode === "--publish") {
    const committed = readManifest().version;
    if (!value || committed !== value) throw new Error(`Expected version ${value}, package/package.json has ${committed}.`);
    const file = pack();
    try {
      npm(["publish", file]);
    } finally {
      fs.rmSync(file, { force: true });
    }
  } else {
    throw new Error("usage: see the header of scripts/publish-package.mjs");
  }
}

try {
  main(process.argv.slice(2));
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 1;
}
