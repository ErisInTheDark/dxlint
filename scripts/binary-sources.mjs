// What the package binaries are compiled from, and the stamp tying
// package/bin to it: build-binaries.mjs writes package/source.sha256 after
// building every platform; publish-package.mjs refuses a stale stamp.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const stampPath = path.join(root, "package", "source.sha256");
const inputs = ["src", "Cargo.toml", "Cargo.lock", ".cargo/config.toml"];

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
    throw new Error("package/bin was built from other sources than src/ and the Cargo files hold now; run npm run build:binaries");
  }
}
