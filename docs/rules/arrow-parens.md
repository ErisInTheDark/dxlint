# arrow-parens

Enforces consistent parentheses around arrow parameters. By default, even a single untyped parameter must have parentheses.

| | |
| --- | --- |
| Upstream | [`@stylistic/arrow-parens`](https://eslint.style/rules/arrow-parens) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 103 / 110 corpus cases |

## Enable

```dx
import .ts
rule arrow-parens
```

## Examples

Incorrect:

```ts
const identity = value => value;
```

Correct:

```ts
const identity = (value) => value;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always"` or `"as-needed"` | `"always"` | Require parentheses or omit them when legal. |
| `requireForBlockBody` | `boolean` | `false` | In `as-needed` mode, retain parentheses for block bodies. |

```dx
rule arrow-parens {
  set style "as-needed"
  set requireForBlockBody true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `arrow-parens` | Unnecessary parentheses. |
| `arrow-parens:missing` | Missing parentheses. |

`rule arrow-parens` turns on every variant; `rule arrow-parens:missing off` turns one off.

## Differences from upstream

- The upstream string option is named `style` in dxlint.
- Seven corpus cases use the Flow parser dialect, which is outside the TypeScript rule's supported input.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/arrow-parens) documentation (MIT).
