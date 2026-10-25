# es-for-direction

Report for-loop counters that move away from their stop condition. This catches likely infinite loops, but does not prove that a loop starts or terminates.

| | |
| --- | --- |
| Upstream | [`es-for-direction`](https://eslint.org/docs/latest/rules/for-direction) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 100 / 100 corpus cases |

## Enable

```dx
import .ts
rule es-for-direction
```

## Examples

Incorrect:

```ts
for (let i = 0; i < 10; i--) {}
```

Correct:

```ts
for (let i = 0; i < 10; i++) {}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/for-direction) documentation (MIT).
