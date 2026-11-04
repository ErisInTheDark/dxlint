# es-module-only-used-from

Reports modules that only certain code uses, so they can move next to it. Use it to keep legacy or test-only code from leaking into live folders.

| | |
| --- | --- |
| Upstream | none |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | not measured |

## Enable

```dx
import .ts
rule es-module-only-used-from set ["src/save/upgrade/**"]
```

For several places, give each its own name and message with [`extends`](../advanced.md#rules):

```dx
import .ts
rule only-upgrades-use-this extends es-module-only-used-from error "only save upgrades use this: move it under src/save/upgrade/" {
  set from ["src/save/upgrade/**"]
  set ignore ["src/test/**"]
}
rule only-tests-use-this extends es-module-only-used-from set ["src/test/**"]
```

## Examples

With `from` set to `["old/**"]`:

Incorrect:

```ts
// helper.ts: old/upgrade.ts imports it, and nothing else does
export const table = [1, 2];
```

Correct:

```ts
// helper.ts: main.ts, which nothing imports, imports it too
export const table = [1, 2];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `from` | `string[]` | `[]` | Globs of the files that alone may use a module this way, relative to the config. |
| `ignore` | `string[]` | `[]` | Globs of files whose imports don't count, relative to the config. |

A module is reported when walking up its importers reaches a `from` file and no entry point, a file nothing else imports. The walk does not go past `from` files, so a file used only by a file only `from` code uses is reported too. A cycle nothing else imports reports nothing unless `from` code uses it. Files in `from` are never reported.

An import is any `import` or `export ... from`, `import x = require()`, `import()` with a quoted specifier, or `import("x")` type, in the program of the `tsconfig.json` beside the config and its `references`. Each is resolved with the importing file's nearest `tsconfig.json`. A plain `require()` call doesn't count, and neither do files outside that program. A referenced project that can't be read is skipped.
