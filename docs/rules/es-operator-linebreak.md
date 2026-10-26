# es-operator-linebreak

Controls which side of an operator a line break appears on. By default, operators end the previous line, except the conditional `?` and `:` operators.

| | |
| --- | --- |
| Upstream | [`@stylistic/operator-linebreak`](https://eslint.style/rules/operator-linebreak) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 167 / 167 corpus cases |

## Enable

```dx
import .ts
rule es-operator-linebreak
```

## Examples

Incorrect:

```ts
const height = top
  + middle;
```

Correct:

```ts
const height = top +
  middle;
```

## Options

`mode` is the default option: `rule es-operator-linebreak set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"before" \| "after" \| "none" \| null` | `null` | Operator placement; `null` uses the default after/before policy. |
| `overrides` | Map from operator to `"before" \| "after" \| "none" \| "ignore"` | `{}` | Override placement for particular operators. |

`"none"` forbids breaks on either side; `"ignore"` disables checks for that operator.

```dx
rule es-operator-linebreak {
  set mode "before"
  set overrides { "=": "after" }
}
```

## Differences from upstream

- Set the upstream positional string through `mode`. The `null` default preserves upstream's special placement for `?` and `:`.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/operator-linebreak) documentation (MIT).
