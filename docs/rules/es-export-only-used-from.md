# es-export-only-used-from

Reports exports that only certain code uses, so they can move next to it. Where [`es-module-only-used-from`](es-module-only-used-from.md) judges a whole file, this judges each exported name.

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
rule es-export-only-used-from set ["src/save/upgrade/**"]
```

For several places, give each its own name and message with [`extends`](../advanced.md#rules):

```dx
import .ts
rule only-upgrades-use-this extends es-export-only-used-from error "only save upgrades use this: move it under src/save/upgrade/" {
  set from ["src/save/upgrade/**"]
  set ignore ["src/test/**"]
  set keepTags ["@public"]
}
rule only-tests-use-this extends es-export-only-used-from set ["src/test/**"]
```

## Examples

With `from` set to `["old/**"]`:

Incorrect:

```ts
// helper.ts: only old/upgrade.ts imports `table`
export const table = [1, 2];
```

Correct:

```ts
// helper.ts: main.ts, which nothing imports, imports `table` too
export const table = [1, 2];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `from` | `string[]` | `[]` | Globs of the files that alone may use an export this way, relative to the config. |
| `ignore` | `string[]` | `[]` | Globs of files whose imports don't count, and whose own exports aren't reported, relative to the config. |
| `keepTags` | `string[]` | `[]` | Comment tags that keep an export, such as `@public`. No tag is special by default. |

A name is reported when walking up from every file that uses it reaches a `from` file and no entry point, a file nothing else imports. Exports of files in `from` are never reported. A name nothing uses isn't reported either: see [`es-no-unused-exports`](es-no-unused-exports.md), which also lists what counts as a use and how tags work.

Walks go through files, not names: if `helper.ts` is used by a file only `from` code uses, it counts as only `from` code, whichever of that file's exports needs it. A tag only silences that name's own report, so tagging `x` doesn't make what its file imports count as live.

Code outside the program of the `tsconfig.json` beside the config and its `references` is invisible, so a name used from there and from `from` is reported unless tagged.
