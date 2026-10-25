# es-no-misused-promises

Reports promises used where a boolean, ordinary object, or synchronous callback is expected. These uses often mean an `await` or rejection handler is missing; use `es-no-floating-promises` for unhandled promise statements.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-misused-promises`](https://typescript-eslint.io/rules/no-misused-promises) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 239 / 248 corpus cases |

## Enable

```dx
import .ts
rule es-no-misused-promises
```

## Examples

Incorrect:

```ts
const promise = Promise.resolve(true);
if (promise) {
  console.log("ready");
}
```

Correct:

```ts
const promise = Promise.resolve(true);
if (await promise) {
  console.log("ready");
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `flagUnions` | `"none" \| "strict" \| "all"` | `"none"` | On `:conditionals`, ignore promise unions, report unions whose awaited members match their non-promise members, or report all promise unions. |

```dx
rule es-no-misused-promises:conditionals {
  set flagUnions "all"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-misused-promises:conditionals` | Promises tested as booleans. |
| `es-no-misused-promises:predicates` | Promise-returning predicate callbacks. |
| `es-no-misused-promises:spreads` | Promises spread into objects. |
| `es-no-misused-promises:void-arguments` | Async callbacks passed to void-returning parameters. |
| `es-no-misused-promises:void-attributes` | Async callbacks assigned to synchronous JSX attributes. |
| `es-no-misused-promises:void-inherited-methods` | Async methods implementing or extending synchronous methods. |
| `es-no-misused-promises:void-properties` | Async callbacks stored in synchronous properties. |
| `es-no-misused-promises:void-returns` | Async callbacks returned where synchronous callbacks are expected. |
| `es-no-misused-promises:void-variables` | Async callbacks assigned to synchronous variables. |

`rule es-no-misused-promises` turns on every variant; `rule es-no-misused-promises:void-arguments off` turns one off.

## Differences from upstream

- Upstream's `checksConditionals`, `checksSpreads`, and `checksVoidReturn` switches are separate dxlint variants, not object options.
- Callable union and intersection targets can lose the expected callback signature.
- Spreading callback arrays or tuples into rest parameters can miss async callbacks, for example `consume(...callbacks)`.
- `typeof f<T>` of a variable holding a generic function (`declare const f: <T>(fn: T) => T`) does not instantiate it, so the expected callback type can be missed.
- An inherited method reached through `typeof` a class-expression binding names the class differently in the diagnostic.
- Assigning an object with an async `[Symbol.dispose]` method to a synchronous disposable type can be missed.
- With `flagUnions "strict"`, awaited array and tuple types are not always recognised as matching their non-promise union members.
- A generic inherited type can have a different display name in a diagnostic.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-misused-promises) documentation (MIT).
