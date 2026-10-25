# es-no-floating-decimal

Requires digits on both sides of a decimal point. This makes number literals easier to distinguish from property access.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-floating-decimal`](https://eslint.style/rules/no-floating-decimal) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 8 / 8 corpus cases |

## Enable

```dx
import .ts
rule es-no-floating-decimal
```

## Examples

Incorrect:

```ts
const half = .5;
const whole = 2.;
```

Correct:

```ts
const half = 0.5;
const whole = 2.0;
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-floating-decimal:leading` | Missing digit before the decimal point. |
| `es-no-floating-decimal:trailing` | Missing digit after the decimal point. |

`rule es-no-floating-decimal` turns on every variant; `rule es-no-floating-decimal:leading off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-floating-decimal) documentation (MIT).
