# es-list-style

Reports inconsistent spacing and wrapping inside brackets. It covers lists such as arrays, object members, arguments, and named imports.

| | |
| --- | --- |
| Upstream | [`@stylistic/list-style`](https://eslint.style/rules/list-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 129 / 129 corpus cases |

## Enable

```dx
import .ts
rule es-list-style
```

## Examples

Incorrect:

```ts
const values = [ 1, 2 ];
```

Correct:

```ts
const values = [1, 2];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `empty` | `"ignore"`, `"always"`, or `"never"` | `"ignore"` | Spacing in empty lists. |
| `singleLine` | `{ spacing?: "always" or "never"; maxItems?: number }` | `{ spacing: "never", maxItems: null }` | Inner spaces and wrapping threshold. |
| `multiLine` | `{ minItems?: number }` | `{ minItems: 0 }` | Minimum items before keeping multiline layout. |
| `overrides` | Map of bracket/node names to `"off"` or list policy objects | `{ "{}": { singleLine: { spacing: "always" } } }` | Override policy by bracket pair or node kind. |

Override objects accept `empty`, `singleLine`, and `multiLine`. For example, `"{}"` changes brace spacing independently of `"[]"` and `"()"`.

```dx
rule es-list-style {
  set singleLine { spacing: "never", maxItems: 3 }
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/list-style) documentation (MIT).
