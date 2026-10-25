# es-no-mixed-operators

Requires parentheses around mixtures of operators in the same configured group. Parentheses make the intended order clear without relying on readers remembering precedence.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-mixed-operators`](https://eslint.style/rules/no-mixed-operators) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:formatting` |
| Parity | 40 / 40 corpus cases |

## Enable

```dx
import .ts
rule es-no-mixed-operators
```

## Examples

Incorrect:

```ts
const result = a + b * c;
const ready = a && b || c;
```

Correct:

```ts
const result = a + (b * c);
const ready = (a && b) || c;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `groups` | `string[][]` | See below | Mixtures within each group require parentheses; different groups are independent. |
| `allowSamePrecedence` | `boolean` | `true` | Allow different operators with equal precedence. |

Default groups:

```dx
rule es-no-mixed-operators {
  set groups [
    ["+", "-", "*", "/", "%", "**"],
    ["&", "|", "^", "~", "<<", ">>", ">>>"],
    ["==", "!=", "===", "!==", ">", ">=", "<", "<="],
    ["&&", "||"],
    ["in", "instanceof"]
  ]
  set allowSamePrecedence true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-mixed-operators:child` | The nested operator in a mixed pair. |
| `es-no-mixed-operators:parent` | The enclosing operator in a mixed pair. |

`rule es-no-mixed-operators` turns on every variant; `rule es-no-mixed-operators:child off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-mixed-operators) documentation (MIT).
