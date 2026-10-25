# es-no-useless-catch

Reports catch clauses that only rethrow the same error. Removing such a catch leaves the error propagation unchanged; a useful `finally` block can remain.

| | |
| --- | --- |
| Upstream | [`es-no-useless-catch`](https://eslint.org/docs/latest/rules/no-useless-catch) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 16 / 16 corpus cases |

## Enable

```dx
import .ts
rule es-no-useless-catch
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
| `es-no-useless-catch:clause` | Redundant catch clauses alongside a `finally` block. |

`rule es-no-useless-catch` turns on every variant; `rule es-no-useless-catch:clause off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-catch) documentation (MIT).
