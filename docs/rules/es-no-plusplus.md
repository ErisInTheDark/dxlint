# es-no-plusplus

Reports the increment and decrement operators, `++` and `--`. Compound assignment is more explicit and avoids line-break-sensitive update syntax.

| | |
| --- | --- |
| Upstream | [`no-plusplus`](https://eslint.org/docs/latest/rules/no-plusplus) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 23 / 23 corpus cases |

## Enable

```dx
import .ts
rule es-no-plusplus
```

## Examples

Incorrect:

```ts
let foo = 0;
foo++;
```

Correct:

```ts
let foo = 0;
foo += 1;
```

## Differences from upstream

- The upstream `allowForLoopAfterthoughts` option is not exposed. Updates in a `for` loop's final expression are also reported.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-plusplus) documentation (MIT).
