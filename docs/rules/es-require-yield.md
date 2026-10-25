# es-require-yield

Reports nonempty generator functions that never yield a value. A plain function is usually clearer when iteration is not needed. Empty generator functions are allowed.

| | |
| --- | --- |
| Upstream | [`es-require-yield`](https://eslint.org/docs/latest/rules/require-yield) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 15 / 15 corpus cases |

## Enable

```dx
import .ts
rule es-require-yield
```

## Examples

Incorrect:

```ts
function* values() {
  return 10;
}
```

Correct:

```ts
function* values() {
  yield 5;
  return 10;
}
function result() {
  return 10;
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/require-yield) documentation (MIT).
