# es-prefer-promise-reject-errors

Requires Promise rejection reasons to be errors. Error objects retain stack information that a rejected string or number cannot provide.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-promise-reject-errors`](https://typescript-eslint.io/rules/prefer-promise-reject-errors) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 236 / 236 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-promise-reject-errors
```

## Examples

Incorrect:

```ts
Promise.reject('error');
new Promise((resolve, reject) => reject('error'));
```

Correct:

```ts
Promise.reject(new Error('error'));
new Promise((resolve, reject) => reject(new Error('error')));
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowEmptyReject` | `boolean` | `false` | Allow rejection without a reason. |
| `allow` | `(string \| type_specifier_file \| type_specifier_lib \| type_specifier_package)[]` | `[]` | Additional allowed type names or types selected by declaration origin. |
| `allowThrowingAny` | `boolean` | `false` | Allow values typed as `any`. |
| `allowThrowingUnknown` | `boolean` | `false` | Allow values typed as `unknown`. |

```dx
rule es-prefer-promise-reject-errors {
  set allowEmptyReject true
}
```

## Differences from upstream

- This is the type-aware extension of ESLint's core rule; both core and TypeScript corpus cases are included above.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-promise-reject-errors) documentation (MIT).
