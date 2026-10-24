# no-ex-assign

Reports assignments to a caught exception parameter. Overwriting it loses the original error and can hide the reason a catch block ran.

| | |
| --- | --- |
| Upstream | [`no-ex-assign`](https://eslint.org/docs/latest/rules/no-ex-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 8 / 8 corpus cases |

## Enable

```dx
import .ts
rule no-ex-assign
```

## Examples

Incorrect:

```ts
try {
  work();
} catch (e) {
  e = 10;
}
```

Correct:

```ts
try {
  work();
} catch (e) {
  const fallback = 10;
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-ex-assign) documentation (MIT).
