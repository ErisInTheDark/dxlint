// Thin client: all linting happens in `zlint lsp`.
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

const BINARY = process.platform === "win32" ? "zlint.exe" : "zlint";
// Cargo may write a fresh build more than once; restart once it settles.
const REBUILD_SETTLE_MS = 500;
// Builds are polled rather than watched: `files.watcherExclude` often
// excludes `target`, and polling works before the file exists.
const BUILD_POLL_MS = 1000;

let client;
// Where workspace builds are copied to before running (per workspace).
let copiesDir;
// Identity of the workspace build the server runs, if it runs one.
let runningBuild;
let rebuildTimer;
// Build paths currently polled by `watchBuilds`.
let watchedBuilds = [];

function workspaceRoots() {
  return (vscode.workspace.workspaceFolders || [])
    .filter((folder) => folder.uri.scheme === "file")
    .map((folder) => folder.uri.fsPath);
}

function buildCandidates() {
  return workspaceRoots().flatMap((root) => ["release", "debug"].map((profile) => path.join(root, "target", profile, BINARY)));
}

function unwatchBuilds() {
  for (const file of watchedBuilds) {
    fs.unwatchFile(file);
  }
  watchedBuilds = [];
}

function watchBuilds() {
  unwatchBuilds();
  watchedBuilds = buildCandidates();
  for (const file of watchedBuilds) {
    fs.watchFile(file, { interval: BUILD_POLL_MS }, (curr, prev) => {
      if (curr.mtimeMs !== prev.mtimeMs || curr.size !== prev.size) {
        onBuildChanged();
      }
    });
  }
}

// Most recently built `target/{release,debug}/zlint` in the workspace, so
// working on zlint itself uses the fresh build. Trusted workspaces only:
// this executes a workspace binary.
function workspaceBuild() {
  if (!vscode.workspace.isTrusted) {
    return undefined;
  }
  let newest;
  for (const candidate of buildCandidates()) {
    let stat;
    try {
      stat = fs.statSync(candidate);
    } catch {
      continue;
    }
    if (stat.isFile() && (!newest || stat.mtimeMs > newest.mtimeMs)) {
      newest = { path: candidate, mtimeMs: stat.mtimeMs, size: stat.size, key: `${candidate}|${stat.mtimeMs}|${stat.size}` };
    }
  }
  return newest;
}

// Runs a copy of the build, so cargo can always replace the original (a
// running exe is locked on Windows). Copies are named by build identity, and
// older ones are removed while no server runs from them.
function copyOfBuild(build) {
  if (!copiesDir) {
    return build.path;
  }
  const name = `zlint-${Math.trunc(build.mtimeMs)}-${build.size}${path.extname(BINARY)}`;
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

function libraryOfBuild(build) {
  const sidecar = JSON.parse(fs.readFileSync(`${build.path}.library.json`, "utf8"));
  const hash = crypto.createHash("sha256").update(fs.readFileSync(build.path)).digest("hex");
  if (sidecar.binarySha256 !== hash || typeof sidecar.library !== "string") {
    throw new Error("workspace build library does not match the executable");
  }
  const library = path.resolve(path.dirname(build.path), sidecar.library);
  if (!["std", "rules"].every(dir => fs.existsSync(path.join(library, dir)))) {
    throw new Error("workspace build library is missing");
  }
  return library;
}

// `zlint.path` if set (relative paths resolve against the first workspace
// folder), else a workspace build, else `zlint` from PATH.
function resolveServer(configured) {
  if (configured) {
    const isBareName = !/[\\/]/.test(configured);
    const [root] = workspaceRoots();
    if (isBareName || path.isAbsolute(configured) || !root) {
      return { command: configured };
    }
    return { command: path.resolve(root, configured) };
  }
  const build = workspaceBuild();
  return build ? { library: libraryOfBuild(build), command: copyOfBuild(build), build } : { command: "zlint" };
}

// A workspace build changed: restart once it settles, if it is now a
// different build than the one running.
function onBuildChanged() {
  clearTimeout(rebuildTimer);
  rebuildTimer = setTimeout(() => {
    if (vscode.workspace.getConfiguration("zlint").get("path")) {
      return;
    }
    const build = workspaceBuild();
    if (build && build.key !== runningBuild) {
      restart();
    }
  }, REBUILD_SETTLE_MS);
}

function createClient(command, library) {
  const config = vscode.workspace.getConfiguration("zlint");
  // No `transport`: TransportKind.stdio appends `--stdio`, which `zlint lsp`
  // rejects. Plain executables talk over stdio anyway.
  const serverOptions = { command, args: ["lsp"], options: {
    env: { ...process.env, ...(library ? { ZLINT_LIBRARY: library } : {}) },
  } };
  const clientOptions = {
    // The server decides which files it owns (via `lang` in .zlint), so
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
  return new LanguageClient("zlint", "zoomie-lint", serverOptions, clientOptions);
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
    client.sendNotification("zlint/focus", focusParams()).catch(() => {});
  }
}

async function start() {
  const configured = vscode.workspace.getConfiguration("zlint").get("path");
  let command = configured || "zlint";
  runningBuild = undefined;
  try {
    const server = resolveServer(configured);
    command = server.command;
    client = createClient(command, server.library);
    await client.start();
    runningBuild = server.build && server.build.key;
    sendFocus();
  } catch (error) {
    // Not awaited: activation shouldn't block on the user dismissing this.
    const openSettings = "Open Settings";
    vscode.window
      .showErrorMessage(
        `zoomie-lint: couldn't start \`${command}\` (${error.message || error}). ` +
          "Put zlint on PATH, build it in this workspace, or set zlint.path.",
        openSettings,
      )
      .then((choice) => {
        if (choice === openSettings) {
          vscode.commands.executeCommand("workbench.action.openSettings", "zlint.path");
        }
      });
  }
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
    vscode.commands.registerCommand("zlint.restart", restart),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("zlint")) {
        await restart();
      }
    }),
    // Workspace builds are only used once the workspace is trusted.
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
