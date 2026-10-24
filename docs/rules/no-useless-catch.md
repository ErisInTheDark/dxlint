# no-useless-catch

Reports catch clauses that only rethrow the same error. Removing such a catch leaves the error propagation unchanged; a useful `finally` block can remain.

| | |
| --- | --- |
| Upstream | [`no-useless-catch`](https://eslint.org/docs/latest/rules/no-useless-catch) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 16 / 16 corpus cases |

## Enable

```dx
import .ts
rule no-useless-catch
```

## Examples

Incorrect:

```ts
try {
  work();
} catch (error) {
  throw error;
}
```

Correct:

```ts
try {
  work();
} catch (error) {
  log(error);
  throw error;
}
```

## Variants

The base check reports the redundant try/catch wrapper when there is no `finally`.

| Variant | Checks |
| --- | --- |
| `no-useless-catch:clause` | Redundant catch clauses alongside a `finally` block. |

`rule no-useless-catch` turns on every variant; `rule no-useless-catch:clause off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-catch) documentation (MIT).
