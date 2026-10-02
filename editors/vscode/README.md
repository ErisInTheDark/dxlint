# zoomie-lint for VS Code

Runs `zlint lsp` and shows its results as you type:

- diagnostics for open files on every edit
- a background scan of the whole workspace, streamed file by file with progress
- quick fixes for fixable problems, plus "Fix all zlint problems in this file"
- formatting (`Format Document`) applies every fix
- `.zlint` syntax highlighting, and config errors shown live in the `.zlint` files

## Setup

1. Build zlint: `cargo build --release` at the repo root, and put `target/release/zlint` on your `PATH` (or set `zlint.path`)
2. In this folder: `npm install`
3. Open the folder in VS Code and press F5, or package it with `npx @vscode/vsce package` and install the `.vsix`

The extension activates in workspaces that contain a `zoomie.zlint`.

## Settings

| setting | default | |
|---|---|---|
| `zlint.path` | `zlint` | path to the binary |
| `zlint.scanWorkspace` | `true` | lint the whole workspace in the background, not just open files |

To use zlint as the formatter for a language, set `"editor.defaultFormatter": "zoomie.zoomie-lint"` for it.
