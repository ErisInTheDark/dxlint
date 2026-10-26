# es-no-loss-of-precision

Reports number literals whose written precision is lost when represented as a JavaScript number. JavaScript's double-precision format cannot preserve every large integer or long decimal.

| | |
| --- | --- |
| Upstream | [`no-loss-of-precision`](https://eslint.org/docs/latest/rules/no-loss-of-precision) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 141 / 141 corpus cases |

## Enable

```dx
import .ts
rule es-no-loss-of-precision
```

## Examples

Incorrect:

```ts
const value = 9007199254740993;
```

Correct:

```ts
const value = 9007199254740991;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-loss-of-precision) documentation (MIT).
