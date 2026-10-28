// Thin client: all linting happens in `dxlint lsp`.
const fs = require("fs");
const path = require("path");
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

const EXE = process.platform === "win32" ? ".exe" : "";
const PLATFORM_BINARY = `dxlint-${process.platform}-${process.arch}${EXE}`;
const PACKAGE = path.join("node_modules", "@inthedark", "dxlint");
// An install or build may write the binary more than once; restart once it settles.
const REBUILD_SETTLE_MS = 500;
// Binaries are polled rather than watched: `files.watcherExclude` often
// excludes `node_modules`, and polling works before a file exists.
const BUILD_POLL_MS = 1000;

let client;
// Where binaries are copied to before running (per workspace).
let copiesDir;
// Identity of the package binary the server runs.
let runningBuild;
let rebuildTimer;
// Binary paths currently polled by `watchBuilds`.
let watchedBuilds = [];

function workspaceRoots() {
  return (vscode.workspace.workspaceFolders || [])
    .filter((folder) => folder.uri.scheme === "file")
    .map((folder) => folder.uri.fsPath);
}

// Where node finds the `@inthedark/dxlint` package from a folder: its
// node_modules, or one above it (workspace roots); pnpm links it there too.
// Checked by hand each time: node's own resolver caches, so an upgrade would
// keep the old path until VS Code restarts.
function packageDirs(root) {
  const dirs = [];
  for (let dir = root; ; dir = path.dirname(dir)) {
    dirs.push(path.join(dir, PACKAGE));
    if (path.dirname(dir) === dir) {
      return dirs;
    }
  }
}

// The dxlint package's binary for this platform: the project's dependency,
// else (in dxlint's own checkout) the package being built.
function candidates() {
  return workspaceRoots().flatMap((root) => [
    ...packageDirs(root).map((dir) => path.join(dir, "bin", PLATFORM_BINARY)),
    path.join(root, "package", "bin", PLATFORM_BINARY),
  ]);
}

function unwatchBuilds() {
  for (const file of watchedBuilds) {
    fs.unwatchFile(file);
  }
  watchedBuilds = [];
}

function watchBuilds() {
  unwatchBuilds();
  watchedBuilds = candidates();
  for (const file of watchedBuilds) {
    fs.watchFile(file, { interval: BUILD_POLL_MS }, (curr, prev) => {
      if (curr.mtimeMs !== prev.mtimeMs || curr.size !== prev.size) {
        onBuildChanged();
      }
    });
  }
}

function statBinary(file) {
  try {
    const stat = fs.statSync(file);
    return stat.isFile() ? { path: file, mtimeMs: stat.mtimeMs, size: stat.size, key: `${file}|${stat.mtimeMs}|${stat.size}` } : undefined;
  } catch {
    return undefined;
  }
}

// The first package binary that exists. Trusted workspaces only: this
// executes a binary from the workspace.
function workspaceBinary() {
  return vscode.workspace.isTrusted ? candidates().map(statBinary).find(Boolean) : undefined;
}

// The library (std/, rules/) a binary runs with: the nearest directory above
// it holding both, as the binary itself would find from where it lies.
function libraryOf(binary) {
  let dir = path.dirname(fs.realpathSync(binary));
  for (;;) {
    if (["std", "rules"].every((name) => fs.existsSync(path.join(dir, name)))) {
      return dir;
    }
    const parent = path.dirname(dir);
    if (parent === dir) {
      throw new Error(`no std/ and rules/ above ${binary}`);
    }
    dir = parent;
  }
}

// Runs a copy, so npm or a rebuild can always replace the original (a running
// exe is locked on Windows). Copies are named by build identity, and older
// ones are removed while no server runs from them.
function copyOfBuild(build) {
  if (!copiesDir) {
    return build.path;
  }
  const name = `dxlint-${Math.trunc(build.mtimeMs)}-${build.size}${EXE}`;
  const copy = path.join(copiesDir, name);
  fs.mkdirSync(copiesDir, { recursive: true });
  for (const old of fs.readdirSync(copiesDir)) {
    if (old !== name) {
      try {
        fs.rmSync(path.join(copiesDir, old), { force: true });
      } catch {
        // Still running (another window's server); the next start retries.
      }
    }
  }
  if (!fs.existsSync(copy)) {
    const partial = `${copy}.partial`;
    fs.copyFileSync(build.path, partial);
    fs.renameSync(partial, copy);
  }
  return copy;
}

// `dxlint.path` if set (relative paths resolve against the first workspace
// folder; a bare name is looked up on PATH), else the package's binary.
function resolveServer(configured) {
  if (configured) {
    const [root] = workspaceRoots();
    if (!/[\\/]/.test(configured)) {
      // On Windows a PATH command is often an npm `.cmd` shim, which only a shell runs.
      return { command: configured, shell: process.platform === "win32" };
    }
    return { command: path.isAbsolute(configured) || !root ? configured : path.resolve(root, configured) };
  }
  const build = workspaceBinary();
  if (!build) {
    throw new Error("this project has no @inthedark/dxlint");
  }
  return { library: libraryOf(build.path), command: copyOfBuild(build), build };
}

// The package binary changed (installed, updated, rebuilt): restart once it
// settles, if it is now a different binary than the one running.
function onBuildChanged() {
  clearTimeout(rebuildTimer);
  rebuildTimer = setTimeout(() => {
    if (vscode.workspace.getConfiguration("dxlint").get("path")) {
      return;
    }
    const build = workspaceBinary();
    if (build && build.key !== runningBuild) {
      restart();
    }
  }, REBUILD_SETTLE_MS);
}

function createClient({ command, library, shell }) {
  const config = vscode.workspace.getConfiguration("dxlint");
  // No `transport`: TransportKind.stdio appends `--stdio`, which `dxlint lsp`
  // rejects. Plain executables talk over stdio anyway.
  const serverOptions = { command, args: ["lsp"], options: {
    env: { ...process.env, ...(library ? { DXLINT_LIBRARY: library } : {}) },
    ...(shell ? { shell: true } : {}),
  } };
  const clientOptions = {
    // The server decides which files it owns (via `lang` in .dx), so
    // every file document is offered to it.
    documentSelector: [{ scheme: "file" }],
    initializationOptions: {
      scanWorkspace: config.get("scanWorkspace", true),
      inspectHover: config.get("inspectHover", false),
    },
    synchronize: {
      // Disk changes refresh background results and reload the config.
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*"),
    },
  };
  return new LanguageClient("dxlint", "dxlint", serverOptions, clientOptions);
}

// Files the user is looking at (active first) and files open in tabs, so the
// server lints them before the rest of the workspace.
function focusParams() {
  const isFile = (uri) => uri.scheme === "file";
  const asUri = (uri) => client.code2ProtocolConverter.asUri(uri);
  const active = vscode.window.activeTextEditor;
  const editors = [active, ...vscode.window.visibleTextEditors.filter((e) => e !== active)].filter(Boolean);
  const tabs = vscode.window.tabGroups.all
    .flatMap((group) => group.tabs)
    .map((tab) => tab.input)
    .flatMap((input) =>
      input instanceof vscode.TabInputText
        ? [input.uri]
        : input instanceof vscode.TabInputTextDiff
          ? [input.modified]
          : [],
    );
  return {
    visible: editors.map((e) => e.document.uri).filter(isFile).map(asUri),
    tabs: tabs.filter(isFile).map(asUri),
  };
}

function sendFocus() {
  if (client && client.isRunning()) {
    // A client stopping mid-send has nothing left to prioritize.
    client.sendNotification("dxlint/focus", focusParams()).catch(() => {});
  }
}

async function start() {
  const configured = vscode.workspace.getConfiguration("dxlint").get("path");
  let command = configured || "dxlint";
  runningBuild = undefined;
  try {
    const server = resolveServer(configured);
    command = server.command;
    client = createClient(server);
    await client.start();
    runningBuild = server.build && server.build.key;
    sendFocus();
  } catch (error) {
    // Not awaited: activation shouldn't block on the user dismissing this.
    const openSettings = "Open Settings";
    vscode.window
      .showErrorMessage(
        `dxlint: couldn't start \`${command}\` (${error.message || error}). ` +
          "Install it in the project (npm install --save-dev @inthedark/dxlint) or set dxlint.path.",
        openSettings,
      )
      .then((choice) => {
        if (choice === openSettings) {
          vscode.commands.executeCommand("workbench.action.openSettings", "dxlint.path");
        }
      });
  }
}

// A dxlint fix or suggestion left a name to choose (`placeholder(...)` in
// the rule): start renaming it, so the language's own rename updates every
// use.
async function renamePlaceholder(uri, position) {
  const editor = await vscode.window.showTextDocument(vscode.Uri.parse(uri));
  const at = new vscode.Position(position.line, position.character);
  editor.selection = new vscode.Selection(at, at);
  await vscode.commands.executeCommand("editor.action.rename");
}

async function restart() {
  if (client && client.needsStop()) {
    await client.stop();
  }
  await start();
}

async function activate(context) {
  copiesDir = context.storageUri && path.join(context.storageUri.fsPath, "bin");
  watchBuilds();
  context.subscriptions.push(
    { dispose: unwatchBuilds },
    vscode.workspace.onDidChangeWorkspaceFolders(watchBuilds),
    vscode.commands.registerCommand("dxlint.restart", restart),
    vscode.commands.registerCommand("dxlint.renamePlaceholder", renamePlaceholder),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("dxlint")) {
        await restart();
      }
    }),
    // The package binary is only used once the workspace is trusted.
    vscode.workspace.onDidGrantWorkspaceTrust(restart),
    vscode.window.onDidChangeActiveTextEditor(sendFocus),
    vscode.window.onDidChangeVisibleTextEditors(sendFocus),
    vscode.window.tabGroups.onDidChangeTabs(sendFocus),
  );
  await start();
}

function deactivate() {
  clearTimeout(rebuildTimer);
  unwatchBuilds();
  return client && client.needsStop() ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
