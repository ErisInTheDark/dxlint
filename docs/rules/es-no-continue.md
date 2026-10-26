# es-no-continue

Reports `continue` statements, including labelled ones. Use an `if` block when your code style prefers one clear path through each loop iteration.

| | |
| --- | --- |
| Upstream | [`no-continue`](https://eslint.org/docs/latest/rules/no-continue) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 6 / 6 corpus cases |

## Enable

```dx
import .ts
rule es-no-continue
```

## Examples

Incorrect:

```ts
let sum = 0;
for (let i = 0; i < 10; i++) {
  if (i >= 5) continue;
  sum += i;
}
```

Correct:

```ts
let sum = 0;
for (let i = 0; i < 10; i++) {
  if (i < 5) sum += i;
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-continue) documentation (MIT).
