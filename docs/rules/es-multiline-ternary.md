# es-multiline-ternary

Reports missing or unwanted line breaks between conditional expression operands. It does not choose whether the `?` or `:` belongs before or after a line break.

| | |
| --- | --- |
| Upstream | [`@stylistic/multiline-ternary`](https://eslint.style/rules/multiline-ternary) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 150 / 150 corpus cases |

## Enable

```dx
import .ts
rule es-multiline-ternary
```

## Examples

Incorrect:

```ts
const value = ready ? first : second;
```

Correct:

```ts
const value = ready
  ? first
  : second;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"`, `"always-multiline"`, or `"never"` | `"always"` | Require line breaks, require them only for multiline expressions, or forbid them. |
| `ignoreJSX` | `boolean` | `false` | Skip conditional expressions in JSX. |

```dx
rule es-multiline-ternary {
  set mode "always-multiline"
}
```

## Differences from upstream

- The upstream string argument is named `mode`.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/multiline-ternary) documentation (MIT).
