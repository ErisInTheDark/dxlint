// Prepares and publishes the npm package in package/.
//   --prepare-version [--version <x.y.z>]  bump (or set) package/package.json's version; prints it
//   --pack                                 npm pack into package/
//   --publish --version <x.y.z>            npm publish, after checking the committed version
// The library (std/, rules/), README.md and LICENSE are copied in from the
// repository root for the pack and removed afterwards; the binaries in
// package/bin are committed (scripts/build-binaries.mjs).
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageDir = path.join(root, "package");
const manifestPath = path.join(packageDir, "package.json");
const copied = ["std", "rules", "README.md", "LICENSE"];
const binaries = ["dxlint-win32-x64.exe", "dxlint-linux-x64"];

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
  // npm keeps the file mode in the tarball; git checkouts on Windows can drop it.
  fs.chmodSync(path.join(packageDir, "bin", "dxlint-linux-x64"), 0o755);
  fs.chmodSync(path.join(packageDir, "bin", "dxlint"), 0o755);
  try {
    for (const name of copied) {
      fs.cpSync(path.join(root, name), path.join(packageDir, name), { recursive: true });
    }
    run();
  } finally {
    for (const name of copied) fs.rmSync(path.join(packageDir, name), { recursive: true, force: true });
  }
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
    withLibrary(() => npm(["pack", "--pack-destination", packageDir]));
  } else if (mode === "--publish") {
    const committed = readManifest().version;
    if (!value || committed !== value) throw new Error(`Expected version ${value}, package/package.json has ${committed}.`);
    withLibrary(() => npm(["publish"]));
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
