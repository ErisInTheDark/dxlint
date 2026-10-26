# es-prefer-exponentiation-operator

Reports calls to `Math.pow` that can use the `**` operator. The operator expresses exponentiation directly and is available in ES2016 and later.

| | |
| --- | --- |
| Upstream | [`prefer-exponentiation-operator`](https://eslint.org/docs/latest/rules/prefer-exponentiation-operator) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 203 / 203 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-exponentiation-operator
```

## Examples

Incorrect:

```ts
const foo = Math.pow(2, 8);
const bar = Math.pow(-1, n);
```

Correct:

```ts
const foo = 2 ** 8;
const bar = (-1) ** n;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-exponentiation-operator) documentation (MIT).
