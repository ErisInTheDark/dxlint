# es-arrow-parens

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
rule es-arrow-parens
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

`style` is the default option: `rule es-arrow-parens set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always"` or `"as-needed"` | `"always"` | Require parentheses or omit them when legal. |
| `requireForBlockBody` | `boolean` | `false` | In `as-needed` mode, retain parentheses for block bodies. |

```dx
rule es-arrow-parens {
  set style "as-needed"
  set requireForBlockBody true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-arrow-parens` | Unnecessary parentheses. |
| `es-arrow-parens:missing` | Missing parentheses. |

`rule es-arrow-parens` turns on every variant; `rule es-arrow-parens:missing off` turns one off.

## Differences from upstream

- The upstream string option is named `style` in dxlint.
- Seven corpus cases use the Flow parser dialect, which is outside the TypeScript rule's supported input.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/arrow-parens) documentation (MIT).
