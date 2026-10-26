# es-space-before-function-paren

Controls spacing before function parameter lists. By default, a space is required for named and anonymous functions, async arrows and catch clauses.

| | |
| --- | --- |
| Upstream | [`@stylistic/space-before-function-paren`](https://eslint.style/rules/space-before-function-paren) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 111 / 112 corpus cases |

## Enable

```dx
import .ts
rule es-space-before-function-paren
```

## Examples

Incorrect:

```ts
function foo() {}
const bar = function() {};
```

Correct:

```ts
function foo () {}
const bar = function () {};
```

## Options

`style` is the default option: `rule es-space-before-function-paren set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always" \| "never"` | `"always"` | Default spacing policy. |
| `named` | `"always" \| "never" \| "ignore" \| null` | `null` | Named function override; null uses `style`. |
| `anonymous` | `"always" \| "never" \| "ignore" \| null` | `null` | Anonymous function override. |
| `asyncArrow` | `"always" \| "never" \| "ignore" \| null` | `null` | Async arrow override. |
| `catch` | `"always" \| "never" \| "ignore" \| null` | `null` | Catch parameter override. |

```dx
rule es-space-before-function-paren {
  set named "never"
  set anonymous "always"
}
```

## Differences from upstream

- The upstream string policy is named `style`; category overrides are direct options.
- Flow syntax is outside the TypeScript target. One Flow-specific corpus case is not matched.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/space-before-function-paren) documentation (MIT).
