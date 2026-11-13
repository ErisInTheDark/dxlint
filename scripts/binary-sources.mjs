// What the package binaries are compiled from, and the stamp tying
// package/bin to it: build-binaries.mjs writes package/source.sha256 after
// building every platform; publish-package.mjs refuses a stale stamp.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const stampPath = path.join(root, "package", "source.sha256");
export const windowsProfile = path.join(root, "scripts", "dxlint-win32-x64.profdata");
const inputs = ["src", "crates", "Cargo.toml", "Cargo.lock", ".cargo/config.toml", path.relative(root, windowsProfile)];

function files(entry) {
  const full = path.join(root, entry);
  if (!fs.existsSync(full)) return [];
  if (!fs.statSync(full).isDirectory()) return [entry];
  return fs.readdirSync(full).flatMap(name => files(path.join(entry, name)));
}

// Line endings are normalised: a Windows build and a Linux checkout agree.
export function sourceHash() {
  const hash = crypto.createHash("sha256");
  for (const file of inputs.flatMap(files).map(file => file.replaceAll("\\", "/")).sort()) {
    hash.update(`${file}\0`);
    hash.update(fs.readFileSync(path.join(root, file), "utf8").replaceAll("\r\n", "\n"));
    hash.update("\0");
  }
  return hash.digest("hex");
}

// `hash`: taken before building, so sources edited mid-build stay stale.
export function writeStamp(hash) {
  fs.writeFileSync(stampPath, `${hash}\n`);
}

export function removeStamp() {
  fs.rmSync(stampPath, { force: true });
}

export function checkStamp() {
  const stamped = fs.existsSync(stampPath) ? fs.readFileSync(stampPath, "utf8").trim() : null;
  if (stamped !== sourceHash()) {
    throw new Error("package/bin was built from other sources than src/, crates/ and the Cargo files hold now; run npm run build:binaries");
  }
}

// Linked checkouts may be running a binary, which Windows won't let anything
// overwrite or delete but does let move: it moves aside until nothing runs it.
const replaced = path.join(root, "target", "replaced-binaries");
let moved = 0;

// Moves `file` out of the way, if it is there.
export function moveAside(file) {
  fs.mkdirSync(replaced, { recursive: true });
  for (const old of fs.readdirSync(replaced)) {
    try {
      fs.rmSync(path.join(replaced, old), { force: true });
    } catch {
      // Still running.
    }
  }
  if (fs.existsSync(file)) fs.renameSync(file, path.join(replaced, `${path.basename(file)}-${process.pid}-${moved++}`));
}

// Copies `from` over `to`, even while `to` runs.
export function replaceFile(from, to) {
  moveAside(to);
  fs.copyFileSync(from, to);
}
