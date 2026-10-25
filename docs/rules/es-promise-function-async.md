# es-promise-function-async

Requires Promise-returning functions to be `async`. This gives callers one error path: a rejected Promise, rather than either a synchronous throw or a rejection.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/promise-function-async`](https://typescript-eslint.io/rules/promise-function-async) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 54 / 54 corpus cases |

## Enable

```dx
import .ts
rule es-promise-function-async
```

## Examples

Incorrect:

```ts
function returnsPromise() {
  return Promise.resolve('value');
}
```

Correct:

```ts
async function returnsPromise() {
  return Promise.resolve('value');
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowAny` | `boolean` | `true` | Allow `any` and `unknown` return types. |
| `allowedPromiseNames` | `string[]` | `[]` | Additional Promise type names. |

```dx
rule es-promise-function-async {
  set allowAny false
}
```

## Differences from upstream

- Upstream's `checkArrowFunctions`, `checkFunctionDeclarations`, `checkFunctionExpressions`, and `checkMethodDeclarations` switches are not exposed. dxlint checks all these function forms.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/promise-function-async) documentation (MIT).
