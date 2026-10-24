# use-unknown-in-catch-callback-variable

Requires Promise rejection callback parameters to have type `unknown` rather than `any` or a narrower assumed error type. Promises can reject with any value, so check the value before using it as an error.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/use-unknown-in-catch-callback-variable`](https://typescript-eslint.io/rules/use-unknown-in-catch-callback-variable) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 57 / 57 corpus cases |

## Enable

```dx
import .ts
rule use-unknown-in-catch-callback-variable
```

## Examples

Incorrect:

```ts
Promise.reject(new Error("failed")).catch(err => {
  console.log(err);
});
Promise.reject(new Error("failed")).catch((err: any) => {
  console.log(err);
});
```

Correct:

```ts
Promise.reject(new Error("failed")).catch((err: unknown) => {
  console.log(err);
});
```

The rule also checks the rejection callback passed as the second argument to `.then()`. TypeScript's `useUnknownInCatchVariables` option controls ordinary `catch` clauses, not these Promise callbacks.

## Differences from upstream

- dxlint reports the problem without offering upstream's suggested edits.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/use-unknown-in-catch-callback-variable) documentation (MIT).
