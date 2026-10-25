# es-no-unsafe-unary-minus

Reports unary `-` applied to values that are not numeric. Negating a string or object can silently produce `NaN`; numbers, bigints, and `any` are allowed.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-unary-minus`](https://typescript-eslint.io/rules/no-unsafe-unary-minus) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:types` |
| Parity | 23 / 23 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-unary-minus
```

## Examples

Incorrect:

```ts
declare const text: string;
-text;
```

Correct:

```ts
declare const value: number | bigint;
-value;
-42;
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-unary-minus) documentation (MIT).
