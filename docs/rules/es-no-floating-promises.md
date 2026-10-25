# es-no-floating-promises

Reports promise expressions whose rejection is not handled. Await or return the promise, attach a rejection handler, or explicitly discard it with `void`.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-floating-promises`](https://typescript-eslint.io/rules/no-floating-promises) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 167 / 181 corpus cases |

## Enable

```dx
import .ts
rule es-no-floating-promises
```

## Examples

Incorrect:

```ts
Promise.reject("failure").catch();
[1, 2, 3].map(async x => x + 1);
```

Correct:

```ts
await Promise.all([1, 2, 3].map(async x => x + 1));
Promise.reject("failure").catch(error => console.error(error));
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `checkThenables` | `boolean` | `false` | Include promise-like types with fulfilment and rejection callbacks. |
| `ignoreVoid` | `boolean` | `true` | Accept `void` as an explicit discard; it does not handle rejection at runtime. |
| `ignoreIIFE` | `boolean` | `false` | Ignore directly invoked function expressions. |
| `allowForKnownSafePromises` | `string[]` | `[]` | Promise type names whose rejection is handled elsewhere. |
| `allowForKnownSafeCalls` | `string[]` | `[]` | Function names whose returned promise is handled elsewhere. |

```dx
rule es-no-floating-promises {
  set ignoreVoid false
  set checkThenables true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-floating-promises` | Unhandled promises and ineffective rejection handlers. |
| `es-no-floating-promises:array` | Discarded arrays containing promises. |

`rule es-no-floating-promises` turns on every variant; `rule es-no-floating-promises:array off` turns one off.

## Differences from upstream

- Safe promise and call allowances accept names, not upstream's richer type-or-value specifier objects.
- dxlint does not offer upstream's editor suggestions for adding `await` or `void`.
- Inherited promise constructors and their result types can lose promise information.
- Constrained generic arrays can miss promise elements, such as `S extends Array<T | Promise<T>>`.
- Generator resume and delegated results are not fully checked.
- Promise results after spread arguments can be missed.
- Safe-promise allowances can incorrectly accept structural or intersection aliases when the allowance cannot be resolved.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-floating-promises) documentation (MIT).
