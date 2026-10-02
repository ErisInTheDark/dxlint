// Thin client: all linting happens in `zlint lsp`.
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function createClient() {
  const config = vscode.workspace.getConfiguration("zlint");
  const command = config.get("path") || "zlint";
  const serverOptions = {
    run: { command, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command, args: ["lsp"], transport: TransportKind.stdio },
  };
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
  client = createClient();
  await client.start();
}

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("zlint.restart", async () => {
      if (client) {
        await client.stop();
      }
      await start();
    }),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("zlint") && client) {
        await client.stop();
        await start();
      }
    }),
  );
  await start();
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
