const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");

const root = path.resolve("workspace");
const packageBinary = path.join(root, "package", "bin", "dxlint-win32-x64.exe");
const installed = path.join(root, "node_modules", "@inthedark", "dxlint");
const installedBinary = path.join(installed, "bin", "dxlint-win32-x64.exe");

// Activates the extension against a fake file system: `files` maps binary
// paths to mtimes, `dirs` lists existing directories.
async function activate({ files = {}, dirs = [], configured } = {}) {
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
  const copies = new Set();
  const io = {
    statSync(file) {
      if (!(file in files)) throw new Error("missing");
      return { isFile: () => true, mtimeMs: files[file], size: 1 };
    },
    realpathSync: file => file,
    existsSync: file => file in files || copies.has(file) || dirs.includes(file),
    mkdirSync() {}, readdirSync: () => [], watchFile() {}, unwatchFile() {}, rmSync() {},
    copyFileSync: (_from, to) => copies.add(to),
    renameSync: (from, to) => { copies.delete(from); copies.add(to); },
  };
  const context = { module: { exports: {} }, process: { platform: "win32", arch: "x64", env: { DXLINT_LIBRARY: "inherited-library" } },
    setTimeout, clearTimeout, require: name => ({
      fs: io, path, vscode, "vscode-languageclient/node": { LanguageClient },
    })[name] };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "extension.js"), "utf8"), context);
  await context.module.exports.activate({ storageUri: { fsPath: path.resolve("extension-storage") }, subscriptions: [] });
  return { servers, errors };
}

const library = dir => [path.join(dir, "std"), path.join(dir, "rules")];

test("a project runs its dev dependency from a copy, with the package's library", async () => {
  const { servers, errors } = await activate({ files: { [installedBinary]: 1 }, dirs: library(installed) });
  assert.deepEqual(errors, []);
  assert.notEqual(servers[0].command, installedBinary);
  assert.equal(servers[0].options.env.DXLINT_LIBRARY, installed);
});

test("a package installed in a folder above the workspace (a monorepo root) is found", async () => {
  const above = path.join(path.dirname(root), "node_modules", "@inthedark", "dxlint");
  const { servers, errors } = await activate({ files: { [path.join(above, "bin", "dxlint-win32-x64.exe")]: 1 }, dirs: library(above) });
  assert.deepEqual(errors, []);
  assert.equal(servers[0].options.env.DXLINT_LIBRARY, above);
});

test("dxlint's own checkout runs its package with the checkout's library", async () => {
  const { servers, errors } = await activate({ files: { [packageBinary]: 1 }, dirs: library(root) });
  assert.deepEqual(errors, []);
  assert.equal(servers[0].options.env.DXLINT_LIBRARY, root);
});

test("without the package nothing starts, and the error says how to install it", async () => {
  const { servers, errors } = await activate();
  assert.equal(servers.length, 0);
  assert.match(errors[0], /npm install --save-dev @inthedark\/dxlint/);
});

test("a package binary with no library above it does not start", async () => {
  const { servers, errors } = await activate({ files: { [installedBinary]: 1 } });
  assert.equal(servers.length, 0);
  assert.equal(errors.length, 1);
});

test("a configured path wins and keeps the inherited library", async () => {
  const { servers } = await activate({ files: { [installedBinary]: 1 }, dirs: library(installed), configured: "C:/tools/dxlint.exe" });
  assert.equal(servers[0].command, "C:/tools/dxlint.exe");
  assert.equal(servers[0].options.env.DXLINT_LIBRARY, "inherited-library");
});

test("a configured bare name runs from PATH through a shell", async () => {
  const { servers } = await activate({ configured: "dxlint" });
  assert.equal(servers[0].command, "dxlint");
  assert.equal(servers[0].options.shell, true);
});
