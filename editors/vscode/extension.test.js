const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const crypto = require("node:crypto");
const test = require("node:test");

async function activate({ sidecar = true, configured, mismatched = false, missingLibrary = false } = {}) {
  const root = path.resolve("workspace"), binary = path.join(root, "target", "release", "zlint.exe");
  const bytes = Buffer.from("binary"), library = path.join(root, "target", "release", "libraries", "snapshot");
  const files = new Map([[binary, bytes]]);
  if (sidecar) files.set(`${binary}.library.json`, Buffer.from(JSON.stringify({
    binarySha256: mismatched ? "different" : crypto.createHash("sha256").update(bytes).digest("hex"),
    library: path.relative(path.dirname(binary), library),
  })));
  const errors = [], servers = [];
  const noop = () => ({ dispose() {} });
  const vscode = {
    workspace: {
      isTrusted: true, workspaceFolders: [{ uri: { scheme: "file", fsPath: root } }],
      getConfiguration: () => ({ get: (name, fallback) => name === "path" ? configured : fallback }),
      onDidChangeWorkspaceFolders: noop, onDidChangeConfiguration: noop,
      onDidGrantWorkspaceTrust: noop, createFileSystemWatcher: noop,
    },
    window: {
      onDidChangeActiveTextEditor: noop, onDidChangeVisibleTextEditors: noop,
      tabGroups: { onDidChangeTabs: noop }, showErrorMessage: text => { errors.push(text); return Promise.resolve(); },
    },
    commands: { registerCommand: noop },
  };
  class LanguageClient {
    constructor(_id, _name, server) { servers.push(server); }
    async start() {}
    isRunning() { return false; }
  }
  const io = {
    statSync(file) {
      if (!files.has(file)) throw new Error("missing");
      return { isFile: () => true, mtimeMs: 1, size: files.get(file).length };
    },
    readFileSync: file => { if (!files.has(file)) throw new Error("missing"); return files.get(file); },
    existsSync: file => files.has(file) || !missingLibrary && [library, path.join(library, "std"), path.join(library, "rules")].includes(file),
    mkdirSync() {}, readdirSync: () => [], watchFile() {}, unwatchFile() {},
    copyFileSync: (from, to) => files.set(to, files.get(from)),
    renameSync: (from, to) => { files.set(to, files.get(from)); files.delete(from); },
  };
  const context = { module: { exports: {} }, process: { platform: "win32", env: { ZLINT_LIBRARY: "unrelated-live-library" } },
    setTimeout, clearTimeout, require: name => ({
      fs: io, path, crypto, "node:crypto": crypto, vscode,
      "vscode-languageclient/node": { LanguageClient },
    })[name] };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "extension.js"), "utf8"), context);
  await context.module.exports.activate({ storageUri: { fsPath: path.resolve("extension-storage") }, subscriptions: [] });
  return { servers, errors, binary, library };
}

test("copied workspace executable keeps its matching library, not inherited live rules", async () => {
  const result = await activate();
  assert.equal(result.errors.length, 0);
  assert.notEqual(result.servers[0].command, result.binary);
  assert.equal(result.servers[0].options.env.ZLINT_LIBRARY, result.library);
});
test("missing or mismatched workspace library cannot launch an unpinned editor server", async () => {
  for (const options of [{ sidecar: false }, { mismatched: true }, { missingLibrary: true }]) {
    const result = await activate(options);
    assert.equal(result.servers.length, 0);
    assert.equal(result.errors.length, 1);
  }
});
test("configured custom executable keeps its user-provided library", async () => {
  const result = await activate({ configured: "custom-zlint" });
  assert.equal(result.errors.length, 0);
  assert.equal(result.servers[0].command, "custom-zlint");
  assert.equal(result.servers[0].options.env.ZLINT_LIBRARY, "unrelated-live-library");
});
