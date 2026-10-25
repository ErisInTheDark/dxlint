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

rule eqeqeq error
rule no-console off

ignore "dist" "coverage" "src/generated/**"
```

- `import .ts` loads the JavaScript and TypeScript support, with every one of its rules, all **off**.
- `import rules:recommended` turns on a set of rules (see below).
- `rule <name>` lines turn rules on, change their severity or options, or turn them off.
- `ignore` skips files and folders (globs).

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

Every language also brings the text rules `trailing-whitespace`, `max-line-length`, `tab-indent`, `blank-lines` and `final-newline`.

## Rulesets

A ruleset turns on a category of rules at their default severity. Rulesets skip rules of languages you did not import.

| ruleset | turns on |
| --- | --- |
| `rules:recommended` | what ESLint and typescript-eslint recommend without type information |
| `rules:correctness` | code that is almost certainly wrong (`no-cond-assign`, `use-isnan`, `valid-typeof`, ...) |
| `rules:suspicious` | likely mistakes, or code that does nothing (`no-fallthrough`, `no-self-assign`, ...) |
| `rules:conventions` | one consistent way to write things (`prefer-const`, `eqeqeq`, ...) |
| `rules:format` | layout and spacing, as ESLint Stylistic's defaults lay code out, and the `rs-*` Rust rules |
| `rules:types` | rules that use type information (`no-floating-promises`, ...) |

Anything a ruleset turned on can be turned off again with `rule <name> off`.

## Configuring rules

```dx
rule no-cond-assign                  # on, at its default severity
rule eqeqeq error                    # on, as an error
rule no-console off                  # off
rule semi:always info                # one variant of a rule
rule no-misused-promises             # a family: every variant
rule no-misused-promises:spreads off # ...minus one
rule brace-style:* {                 # options go in a block
  set style "stroustrup"
  set allowSingleLine true
}
rule max-lines {
  set max 500
  ignore "src/generated/**"          # this rule only, not these files
}
rule no-magic-numbers {
  files "src/**"                     # this rule only on these files
}
```

- Severities: `off`, `hint`, `info`, `warn`, `error`. Only `error` makes the exit code 1.
- Option names and values are the upstream ones (`allowSingleLine`, `"stroustrup"`). Every rule's page in [rules/](rules/README.md) lists them, and `dxlint describe <rule>` prints them for your config.
- Some rules are split into **variants**, `rule:variant` (`semi:always`, `no-unused-vars:parameters`). The family name covers all of them, for turning on, configuring and suppressing.
- `*` globs over rule names: `rule jsx-* off`.
- A misspelled rule name is an error with a suggestion.

`dxlint rules` lists every rule your config defines, with severity, options and message.

## Type-aware rules

Rules such as `no-floating-promises`, `no-misused-promises` or `prefer-nullish-coalescing` read types. dxlint does not run TypeScript: it reads your `tsconfig.json`, the files your code imports, `@types` packages and TypeScript's own `lib` files from `node_modules/typescript`. Keep `typescript` installed for those `lib` files.

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
| `// dxlint-ignore no-console eqeqeq` | only these rules (globs and family names work) |
| `/* dxlint-ignore-start */` ... `/* dxlint-ignore-end */` | everything in between |
| `// dxlint-ignore-file no-console` | the whole file |

`eslint-disable` comments are not read.

## Nested configs

Each file is linted by the nearest `.dx` at or above it, in the CLI and in the editor. A nested config replaces its parents for the files below it; import the parent to build on it:

```dx
import ../.dx

rule no-console off
```

When `dxlint` walks into a folder that has its own `.dx`, it skips that folder and says so. Name the folder (`dxlint . packages/app`) to lint it too, with its own config. `-c` lints every named path with that one config. For rules that only differ by folder, `files` and `ignore` inside a rule block do the job without a second config.

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

- Rule names drop the plugin prefix: `@typescript-eslint/no-floating-promises` and `@stylistic/indent` are `no-floating-promises` and `indent`. Where typescript-eslint extends a core rule (`no-unused-vars`, `no-shadow`, ...), dxlint has one rule that behaves like the typescript-eslint version.
- Options keep their upstream names and values; a rule's option object becomes `set` lines.
- `files`/`ignores` blocks become `ignore` lines, or `files`/`ignore` inside a rule.
- Replace `eslint-disable` comments with `dxlint-ignore`.
- Each rule's page in [rules/](rules/README.md) has a parity row: how many of the upstream rule's own test cases dxlint matches.

## Editors

The VS Code extension (`editors/vscode`) runs the project's own `@inthedark/dxlint` as a language server: diagnostics as you type from the unsaved buffer, quick fixes, fix-all on save, and background linting of the rest of the workspace. In `.dx` files it adds highlighting, hover, go to definition, references, completion and an outline. Any editor with LSP support can run `dxlint lsp` over stdio.
