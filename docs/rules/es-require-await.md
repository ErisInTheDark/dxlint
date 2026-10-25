# es-require-await

Reports async functions that neither await work nor return promises. Removing an unnecessary `async` makes the function's intent clearer.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/require-await`](https://typescript-eslint.io/rules/require-await) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 128 / 128 corpus cases |

## Enable

```dx
import .ts
rule es-require-await
```

## Examples

Incorrect:

```ts
async function returnNumber() {
  return 1;
}
```

Correct:

```ts
function returnNumber() {
  return 1;
}
async function returnPromise() {
  return Promise.resolve(1);
}
```

## Differences from upstream

- Follows the TypeScript extension of ESLint's core rule: promise returns can justify `async`, and async generators are checked using their yielded values.
- Reports are manual; dxlint does not offer the upstream suggestion to remove `async`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/require-await) documentation (MIT).
