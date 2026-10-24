# dxlint for VS Code

Runs `dxlint lsp` and shows its results as you type:

- diagnostics for open files on every edit
- a background scan of the whole workspace, streamed file by file with progress, reusing `.dxcache` results
- an edit re-lints the other files it affects (an importer of a changed export), open or not
- quick fixes for fixable problems, plus "Fix all dxlint problems in this file"
- formatting (`Format Document`) applies every fix
- `.dx` syntax highlighting, and config errors shown live in the `.dx` files, with links to related places ("first defined here", "still used here")
- in `.dx` files: completion of built-in languages and rulesets after `import `, of every defined rule after `rule `, and of a rule's options after `set ` in its block; hover on any name (rules with their final severity and option values, layers with their fields and rules, attributes, macros, groups, builtins, facts, functions, keywords), go to definition (std modules open read-only), find all references, completion and an outline
- every diagnostic's rule code links to where the rule is defined
- with `dxlint.inspectHover`, hovering in a linted file shows the layers at the cursor and their facts: handy when writing rules

## Setup

1. Build dxlint: `cargo build --release` at the repo root. The extension finds the binary by:
   - `dxlint.path`, if set
   - the newest `target/release/dxlint` or `target/debug/dxlint` in the (trusted) workspace, so working on dxlint itself just works.
     It runs a copy, so `cargo build` never hits a locked exe, and it restarts automatically when you rebuild
   - `dxlint` on your `PATH` (eg `cargo install --path .`)
2. In this folder: `npm install`
3. Open the folder in VS Code and press F5, or package it with `npx @vscode/vsce package` and install the `.vsix`

The extension activates in workspaces that contain a `.dx` config.

## Settings

| setting | default | |
|---|---|---|
| `dxlint.path` | empty (auto) | path to the binary; relative paths resolve against the first workspace folder |
| `dxlint.scanWorkspace` | `true` | lint the whole workspace in the background, not just open files |
| `dxlint.inspectHover` | `false` | hover in linted files shows dxlint's layers and facts at the cursor |

To use dxlint as the formatter for a language, set `"editor.defaultFormatter": "dxlint.dxlint"` for it.
