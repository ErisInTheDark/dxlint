# es-no-unsafe-finally

Reports `return`, `throw`, `break`, and `continue` statements that leave a `finally` block. They can replace a pending return or error from the corresponding `try` or `catch`.

| | |
| --- | --- |
| Upstream | [`no-unsafe-finally`](https://eslint.org/docs/latest/rules/no-unsafe-finally) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 30 / 30 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-finally
```

## Examples

Incorrect:

```ts
function read() {
  try {
    return 1;
  } finally {
    return 3;
  }
}
```

Correct:

```ts
function read() {
  try {
    return 1;
  } finally {
    console.log("finished");
  }
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unsafe-finally) documentation (MIT).
