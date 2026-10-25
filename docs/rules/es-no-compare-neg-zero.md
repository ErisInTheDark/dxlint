# es-no-compare-neg-zero

Reports comparisons against `-0`. Equality operators do not distinguish positive and negative zero; use `Object.is` when the sign matters.

| | |
| --- | --- |
| Upstream | [`es-no-compare-neg-zero`](https://eslint.org/docs/latest/rules/no-compare-neg-zero) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 50 / 50 corpus cases |

## Enable

```dx
import .ts
rule es-no-compare-neg-zero
```

## Examples

Incorrect:

```ts
if (x === -0) {
  handleZero();
}
```

Correct:

```ts
if (Object.is(x, -0)) {
  handleNegativeZero();
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-compare-neg-zero) documentation (MIT).
