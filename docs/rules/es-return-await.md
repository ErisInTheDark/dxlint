# es-return-await

Checks whether promises returned from async functions should be awaited. By default, it requires `await` where rejection handling or resource cleanup depends on it, and forbids it in ordinary returns. Awaiting a value known not to be a promise is also reported.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/return-await`](https://typescript-eslint.io/rules/return-await) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 103 / 103 corpus cases |

## Enable

```dx
import .ts
rule es-return-await
```

## Examples

Incorrect:

```ts
async function result() {
  try {
    return Promise.reject("failed");
  } catch {
    return "fallback";
  }
}
```

Correct:

```ts
async function result() {
  try {
    return await Promise.reject("failed");
  } catch {
    return "fallback";
  }
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"`, `"error-handling-correctness-only"`, `"in-try-catch"` or `"never"` | `"in-try-catch"` | Choose when returned promises must be awaited. |

```dx
rule es-return-await {
  set mode "always"
}
```

- `always`: await every returned promise.
- `in-try-catch`: await in error-handling contexts; do not await elsewhere.
- `error-handling-correctness-only`: require awaiting in error-handling contexts; allow either form elsewhere.
- `never`: forbid awaiting returned promises, even when it changes rejection handling.

Error-handling contexts include `try` blocks, `catch` blocks with a `finally`, and returns after a `using` declaration within its scope.

## Differences from upstream

- The upstream string option is named `mode` in dxlint.
- Changes to promise awaiting in error-handling contexts are manual reports, not suggestions or autofixes. Other reports can be autofixed.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/return-await) documentation (MIT).
