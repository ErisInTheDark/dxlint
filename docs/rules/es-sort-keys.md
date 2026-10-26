# es-sort-keys

Requires object keys in alphabetical order. Case-sensitive ascending order is the default; spreads separate sorting groups.

| | |
| --- | --- |
| Upstream | [`sort-keys`](https://eslint.org/docs/latest/rules/sort-keys) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 215 / 215 corpus cases |

## Enable

```dx
import .ts
rule es-sort-keys
```

## Examples

Incorrect:

```ts
const object = { a: 1, c: 3, b: 2 };
```

Correct:

```ts
const object = { a: 1, b: 2, c: 3 };
```

## Options

`mode` is the default option: `rule es-sort-keys set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"asc" \| "desc"` | `"asc"` | Key order. |
| `caseSensitive` | boolean | `true` | Distinguish uppercase and lowercase letters. |
| `natural` | boolean | `false` | Compare digit runs numerically. |
| `minKeys` | integer, at least 2 | `2` | Minimum object size to check. |
| `allowLineSeparatedGroups` | boolean | `false` | Restart sorting after blank lines. |
| `ignoreComputedKeys` | boolean | `false` | Restart sorting at computed keys. |

```dx
rule es-sort-keys {
  set mode "asc"
  set natural true
}
```

## Differences from upstream

- The upstream order string is named `mode` in dxlint.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/sort-keys) documentation (MIT).
