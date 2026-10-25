# es-comma-style

Controls whether a comma belongs at the end of its item line or the start of the following line. The default requires commas at the end.

| | |
| --- | --- |
| Upstream | [`@stylistic/comma-style`](https://eslint.style/rules/comma-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 140 / 140 corpus cases |

## Enable

```dx
import .ts
rule es-comma-style
```

## Examples

Incorrect:

```ts
const values = [
  'apples'
  , 'oranges'
];
```

Correct:

```ts
const values = [
  'apples',
  'oranges'
];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"last"` or `"first"` | `"last"` | Put commas on the preceding or following item line. |

```dx
rule es-comma-style {
  set style "first"
}
```

## Differences from upstream

- The upstream string option is named `style`.
- Upstream's AST-node `exceptions` object is not exposed.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/comma-style) documentation (MIT).
