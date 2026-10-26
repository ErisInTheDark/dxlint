# Using dxlint

This page covers what you need to lint a project: installing, writing a config, picking rules, and running dxlint locally, in CI and in your editor. Writing your own rules is in [advanced.md](advanced.md).

## Install

```sh
npm install --save-dev @inthedark/dxlint
```

This installs the `dxlint` command for your project (`npx dxlint`, or `"lint": "dxlint"` in `package.json` scripts). The package holds prebuilt binaries for Windows and Linux (x64) and the built-in rule library.

## The config file

dxlint reads the nearest `.dx` file in the current directory or above it. Paths in it are relative to its own directory, and `.gitignore` is respected.

```dx
import .ts
import rules:recommended

rule es-eqeqeq error
rule es-no-console off

ignore "dist" "coverage" "src/generated/**"
```

- `import .ts` loads the JavaScript and TypeScript support, with every one of its rules, all **off**.
- `import rules:recommended` turns on a set of rules (see below).
- `rule <name>` lines turn rules on, change their severity or options, or turn them off.
- `ignore` skips files and folders (globs). As in `.gitignore`, a `!glob` takes files back and the last matching glob decides: `ignore "dist/**" "!dist/types/**"`.

`dxlint check` validates the config without linting. A config with mistakes still lints with every part that built; the mistakes are reported and the run fails.

## Languages

| import | lints |
| --- | --- |
| `import .ts` | `.js` `.mjs` `.cjs` `.jsx` `.ts` `.tsx` `.mts` `.cts` |
| `import .rs` | Rust |
| `import .css` | CSS |
| `import .json` | JSON and JSONC |
| `import .md` | Markdown |
| `import .py`, `.c`, `.toml` | Python, C, TOML (structure and text rules) |
| `import .dx` | `.dx` files |

Every language also brings the text rules `shared-trailing-whitespace`, `shared-max-line-length`, `shared-tab-indent`, `shared-blank-lines` and `shared-final-newline`.

## Rulesets

A ruleset turns on one kind of rule. No rule is in two rulesets, and rulesets skip rules of languages you did not import. Type-aware rules sit beside the others: dxlint reads types without running TypeScript.

| ruleset | turns on | as |
| --- | --- | --- |
| `rules:recommended` | `rules:bugs`, `rules:suspicious` and `rules:type-safety` together | |
| `rules:bugs` | code that is wrong: it throws, never runs, or cannot do what it says (`es-no-dupe-keys`, `es-no-undef`, `es-no-floating-promises`, ...) | error |
| `rules:suspicious` | likely mistakes, or code that does nothing (`es-no-unused-vars`, `es-no-fallthrough`, `es-no-empty`, ...) | warn |
| `rules:type-safety` | code that switches type checking off or lets `any` leak (`es-no-explicit-any`, `es-ban-ts-comment`, `es-no-unsafe-*`, ...) | warn |
| `rules:style` | one consistent, modern way to write things (`es-prefer-const`, `es-eqeqeq`, `es-prefer-optional-chain`, ...) | warn |
| `rules:formatting` | layout and spacing, as ESLint Stylistic's defaults lay code out, and the `rs-*` Rust rules | warn |
| `rules:es-no-semi` | after `rules:formatting`: no statement semicolons, with a guard `;` starting lines that begin with `(`, `[` or a template (`;[1, 2].forEach(f)`) | |

Each rule's page and [the rule list](rules/README.md) say which ruleset turns it on. Anything a ruleset turned on can be turned off again with `rule <name> off`, and `severity warn` sets every rule on above it to `warn` (or `hint`, `info`, `error`):

```dx
import rules:recommended
severity warn                        # bugs warn too
rule es-no-floating-promises error   # except this one
```

Rulesets and your own `rule` lines apply top to bottom, so when two of them set the same option, the later one wins: put your own settings after your imports. Turning a rule on again (`rule es-semi`) keeps the options and severity it already has. Importing the same ruleset a second time does nothing.

## Configuring rules

```dx
rule es-no-cond-assign                  # on, at its default severity
rule es-eqeqeq error                    # on, as an error
rule es-no-console off                  # off
rule es-semi:always info                # one variant of a rule
rule es-no-misused-promises             # a family: every variant
rule es-no-misused-promises:spreads off # ...minus one
rule es-quotes set "single"             # a rule's default option, inline
rule es-brace-style:* {                 # other options go in a block
  set style "stroustrup"
  set allowSingleLine true
}
```

- Severities: `off`, `hint`, `info`, `warn`, `error`. Only `error` makes the exit code 1.
- Option names and values are the upstream ones (`allowSingleLine`, `"stroustrup"`). Every rule's page in [rules/](rules/README.md) lists them, and `dxlint describe <rule>` prints them for your config.
- Some rules are split into **variants**, `rule:variant` (`es-semi:always`, `es-no-unused-vars:parameters`). The family name covers all of them, for turning on, configuring and suppressing.
- `*` globs over rule names: `rule es-jsx-* off`.
- A misspelled rule name is an error with a suggestion.

`dxlint rules` lists every rule your config defines, with severity, options and message.

## Settings for some files

A `files` block changes rules for the files its globs match (`!` globs take files back out):

```dx
files "src/server/**" "!src/server/legacy/**" {
  rule es-semi set "never"
  rule es-no-console off
}
files "**/*.test.ts" {
  severity warn                         # everything on stays on, as warnings
  rule es-no-explicit-any off
}
files "scripts/**" {
  import rules:style                    # a ruleset for these files only
}
```

- Inside go `rule` lines, `severity` and ruleset imports; new rules and languages are defined outside.
- Lines apply top to bottom, blocks included: for a file in two blocks the later one wins, and a later line outside every block changes all files.
- A block that only changes severities costs nothing extra; a rule given other option values is compiled once more per distinct set of values.

## Type-aware rules

Rules such as `es-no-floating-promises`, `es-no-misused-promises` or `es-prefer-nullish-coalescing` read types. dxlint does not run TypeScript: it reads your `tsconfig.json`, the files your code imports, `@types` packages and TypeScript's own `lib` files from `node_modules/typescript`. Keep `typescript` installed for those `lib` files.

These rules need no extra configuration. Where dxlint cannot tell a type for certain, it stays quiet rather than guessing, so some upstream findings are missed; each rule's page lists its known differences.

## Running

| command | |
| --- | --- |
| `dxlint` | lint the whole project |
| `dxlint src/a.ts src/lib` | lint some files or folders |
| `dxlint --fix` | apply fixes in place |
| `dxlint --format jsonl` | one JSON object per diagnostic, streamed |
| `dxlint --jobs 1` | lint on one thread (default: every core) |
| `dxlint --no-cache` | ignore and do not write `.dxcache` |
| `dxlint --stats` | timing, and how many files came from the cache |
| `dxlint -c other.dx` | use another config |
| `dxlint describe <name>` | explain a rule, option, layer or other name |

Exit codes: `0` no errors, `1` errors found, `2` broken config or input.

## The cache

Results are kept in `.dxcache` beside the config; add it to `.gitignore`. A file is linted again only when its text changed, the config changed, or something its result read changed: an imported declaration, module resolution, a `tsconfig.json` or `package.json`. Editing one file re-checks the files that depend on it, so a cached run is safe before a release; `--no-cache` is never needed for correctness.

## Suppressing a finding

Write these in any comment:

| comment | suppresses |
| --- | --- |
| `// dxlint-ignore` on its own line | the next line, every rule |
| `foo(); // dxlint-ignore` | this line |
| `// dxlint-ignore es-no-console es-eqeqeq` | only these rules (globs and family names work) |
| `/* dxlint-ignore-start */` ... `/* dxlint-ignore-end */` | everything in between |
| `// dxlint-ignore-file es-no-console` | the whole file |

`eslint-disable` comments are not read.

## Nested configs

Each file is linted by the nearest `.dx` at or above it, in the CLI and in the editor. A nested config that starts by importing its parent builds on it:

```dx
import ../.dx

rule es-no-console off
```

Such a config is a `files` block that lives in its folder: when it holds only what a block can (rule lines, `severity`, ruleset imports), a run of the parent lints the folder in the same pass, with one linter. Any other nested `.dx` is a project of its own: `dxlint` skips its folder and says so; name the folder (`dxlint . packages/app`) to lint it too, with its own config. `-c` lints every named path with that one config and folds nothing.

## Agents in Workbench

In a Workbench agent's shell, `dxlint` lints the agent's view of the repository (`wb git arc held`): files other live threads claim read as the view has them, the rest from disk. `--fix` never rewrites those files. `--no-workbench-view` lints the disk as it is.

## Language settings

JavaScript environment settings (ESLint's `languageOptions`) live in a `language` block:

```dx
import .ts

language .js {
  set globals { app: "readonly", cache: "writable" }
  set sourceType "commonjs"
  files "scripts/**"
}
```

`ecmaVersion`, `sourceType`, `globals`, `impliedStrict` and `globalReturn` are supported. `files` and `ignore` limit a block to some files.

## Coming from ESLint

- Rule names are the upstream name without its plugin prefix, after a language prefix: `@typescript-eslint/no-floating-promises` and `@stylistic/indent` are `es-no-floating-promises` and `es-indent`, the same for JavaScript and TypeScript. Text rules every language shares, such as `eol-last`, are `shared-eol-last`. Where typescript-eslint extends a core rule (`es-no-unused-vars`, `es-no-shadow`, ...), dxlint has one rule that behaves like the typescript-eslint version.
- Options keep their upstream names and values; a rule's option object becomes `set` lines.
- A config object with `files`, `ignores` and `rules` becomes a `files` block, its `ignores` as `!` globs; top-level `ignores` become `ignore` lines.
- Replace `eslint-disable` comments with `dxlint-ignore`.
- Each rule's page in [rules/](rules/README.md) has a parity row: how many of the upstream rule's own test cases dxlint matches.

## Editors

The VS Code extension (`editors/vscode`) runs the project's own `@inthedark/dxlint` as a language server: diagnostics as you type from the unsaved buffer, quick fixes, fix-all on save, and background linting of the rest of the workspace. In `.dx` files it adds highlighting, hover, go to definition, references, completion and an outline. Any editor with LSP support can run `dxlint lsp` over stdio.
