# no-self-compare

Reports comparisons with the same expression on both sides. For a `NaN` test, use `Number.isNaN` instead of comparing a value to itself.

| | |
| --- | --- |
| Upstream | [`no-self-compare`](https://eslint.org/docs/latest/rules/no-self-compare) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 21 / 21 corpus cases |

## Enable

```dx
import .ts
rule no-self-compare
```

## Examples

Incorrect:

```ts
if (x === x) {
  x = 20;
}
```

Correct:

```ts
if (x === y) {
  x = 20;
}
if (Number.isNaN(x)) {
  handleInvalidNumber();
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-self-compare) documentation (MIT).
