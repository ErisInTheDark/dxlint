# dxlint for VS Code

Runs [dxlint](https://github.com/ErisInTheDark/dxlint) as a language server, for two jobs.

**Linting your code**, as you type:

- diagnostics for open files on every edit, from the unsaved buffer
- the rest of the workspace linted in the background, streamed file by file, reusing `.dxcache`
- an edit re-lints the other files it affects (an importer of a changed export), open or not
- quick fixes, "Fix all dxlint problems", and Format Document applying every fix
- every diagnostic's rule code links to where the rule is defined

**Writing `.dx` configs and rules**:

- syntax highlighting
- config errors live in the `.dx` file, with links to related places ("first defined here")
- hover on any name: rules with their final severity and options, layers with their fields, attributes, macros, functions, keywords
- go to definition (into the built-in library, read-only), find all references, outline
- completion: languages and rulesets after `import`, rules after `rule`, a rule's options after `set`
- with `dxlint.inspectHover`, hovering in a linted file shows the layers at the cursor and their facts

The extension activates in workspaces that contain a `.dx` file.

## Which dxlint it runs

The project's own dxlint: the `@inthedark/dxlint` package, found the way Node finds it (the workspace's `node_modules` or one above it, pnpm links included). Install it with `npm install --save-dev @inthedark/dxlint` (or your package manager's equivalent). In dxlint's own checkout it runs `package/bin` instead. `dxlint.path` overrides both.

The binary runs from a copy, with the rule library from its package, so an upgrade or rebuild never hits a locked file; the server restarts when the package's binary changes. The workspace must be trusted.

## Settings

| setting | default | |
| --- | --- | --- |
| `dxlint.path` | empty (auto) | path to the binary; relative paths resolve against the first workspace folder |
| `dxlint.scanWorkspace` | `true` | lint the whole workspace in the background, not just open files |
| `dxlint.inspectHover` | `false` | hover in linted files shows dxlint's layers and facts at the cursor |

To use dxlint as a formatter, set `"editor.defaultFormatter": "ErisInTheDark.dxlint"` for that language.

## Building

`npm install`, then `npm run package` builds `dxlint.vsix`; `npm test` runs the extension's tests.
