// Thin client: all linting happens in `zlint lsp`.
const fs = require("fs");
const path = require("path");
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

const BINARY = process.platform === "win32" ? "zlint.exe" : "zlint";

let client;

function workspaceRoots() {
  return (vscode.workspace.workspaceFolders || [])
    .filter((folder) => folder.uri.scheme === "file")
    .map((folder) => folder.uri.fsPath);
}

// Most recently built `target/{release,debug}/zlint` in the workspace, so
// working on zlint itself uses the fresh build. Trusted workspaces only:
// this executes a workspace binary.
function workspaceBuild() {
  if (!vscode.workspace.isTrusted) {
    return undefined;
  }
  let newest;
  for (const root of workspaceRoots()) {
    for (const profile of ["release", "debug"]) {
      const candidate = path.join(root, "target", profile, BINARY);
      let stat;
      try {
        stat = fs.statSync(candidate);
      } catch {
        continue;
      }
      if (stat.isFile() && (!newest || stat.mtimeMs > newest.mtimeMs)) {
        newest = { path: candidate, mtimeMs: stat.mtimeMs };
      }
    }
  }
  return newest && newest.path;
}

// `zlint.path` if set (relative paths resolve against the first workspace
// folder), else a workspace build, else `zlint` from PATH.
function resolveServerCommand(configured) {
  if (configured) {
    const isBareName = !/[\\/]/.test(configured);
    const [root] = workspaceRoots();
    if (isBareName || path.isAbsolute(configured) || !root) {
      return configured;
    }
    return path.resolve(root, configured);
  }
  return workspaceBuild() || "zlint";
}

function createClient(command) {
  const config = vscode.workspace.getConfiguration("zlint");
  // No `transport`: TransportKind.stdio appends `--stdio`, which `zlint lsp`
  // rejects. Plain executables talk over stdio anyway.
  const serverOptions = { command, args: ["lsp"] };
  const clientOptions = {
    // The server decides which files it owns (via `lang` in .zlint), so
    // every file document is offered to it.
    documentSelector: [{ scheme: "file" }],
    initializationOptions: { scanWorkspace: config.get("scanWorkspace", true) },
    synchronize: {
      // Disk changes refresh background results and reload the config.
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*"),
    },
  };
  return new LanguageClient("zlint", "zoomie-lint", serverOptions, clientOptions);
}

async function start() {
  const configured = vscode.workspace.getConfiguration("zlint").get("path");
  const command = resolveServerCommand(configured);
  client = createClient(command);
  try {
    await client.start();
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
  context.subscriptions.push(
    vscode.commands.registerCommand("zlint.restart", restart),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("zlint")) {
        await restart();
      }
    }),
    // Workspace builds are only used once the workspace is trusted.
    vscode.workspace.onDidGrantWorkspaceTrust(restart),
  );
  await start();
}

function deactivate() {
  return client && client.needsStop() ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
