# es-no-extra-label

Reports labels on `break` and `continue` when the same statement would be targeted without the label. Removing the reference makes control flow easier to follow.

| | |
| --- | --- |
| Upstream | [`es-no-extra-label`](https://eslint.org/docs/latest/rules/no-extra-label) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 34 / 34 corpus cases |

## Enable

```dx
import .ts
rule es-no-extra-label
```

## Examples

Incorrect:

```ts
A: while (a) {
  break A;
}
```

Correct:

```ts
while (a) {
  break;
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-extra-label) documentation (MIT).
