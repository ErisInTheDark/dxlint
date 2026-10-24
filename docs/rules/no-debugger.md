# no-debugger

Reports `debugger` statements. Remove them from shipped code and use debugger breakpoints during development.

| | |
| --- | --- |
| Upstream | [`no-debugger`](https://eslint.org/docs/latest/rules/no-debugger) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 2 / 2 corpus cases |

## Enable

```dx
import .ts
rule no-debugger
```

## Examples

Incorrect:

```ts
function isTruthy(x) {
  debugger;
  return Boolean(x);
}
```

Correct:

```ts
function isTruthy(x) {
  return Boolean(x);
}
```

## Differences from upstream

- dxlint does not offer upstream's suggestion to remove the statement.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-debugger) documentation (MIT).
