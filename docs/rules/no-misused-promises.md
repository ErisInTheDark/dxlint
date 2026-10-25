# no-misused-promises

Reports promises used where a boolean, ordinary object, or synchronous callback is expected. These uses often mean an `await` or rejection handler is missing; use `no-floating-promises` for unhandled promise statements.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-misused-promises`](https://typescript-eslint.io/rules/no-misused-promises) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:types` |
| Parity | 236 / 248 corpus cases |

## Enable

```dx
import .ts
rule no-misused-promises
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
rule no-misused-promises:conditionals {
  set flagUnions "all"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-misused-promises:conditionals` | Promises tested as booleans. |
| `no-misused-promises:predicates` | Promise-returning predicate callbacks. |
| `no-misused-promises:spreads` | Promises spread into objects. |
| `no-misused-promises:void-arguments` | Async callbacks passed to void-returning parameters. |
| `no-misused-promises:void-attributes` | Async callbacks assigned to synchronous JSX attributes. |
| `no-misused-promises:void-inherited-methods` | Async methods implementing or extending synchronous methods. |
| `no-misused-promises:void-properties` | Async callbacks stored in synchronous properties. |
| `no-misused-promises:void-returns` | Async callbacks returned where synchronous callbacks are expected. |
| `no-misused-promises:void-variables` | Async callbacks assigned to synchronous variables. |

`rule no-misused-promises` turns on every variant; `rule no-misused-promises:void-arguments off` turns one off.

## Differences from upstream

- Upstream's `checksConditionals`, `checksSpreads`, and `checksVoidReturn` switches are separate dxlint variants, not object options.
- Callable union and intersection targets can lose the expected callback signature.
- Spreading callback arrays or tuples into rest parameters can miss async callbacks, for example `consume(...callbacks)`.
- Explicit generic call arguments do not always reach the expected callback type.
- An inherited method reached through `typeof` a class-expression binding names the class differently in the diagnostic.
- Assigning an object with an async `[Symbol.dispose]` method to a synchronous disposable type can be missed.
- With `flagUnions "strict"`, awaited array and tuple types are not always recognised as matching their non-promise union members.
- A generic inherited type can have a different display name in a diagnostic.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-misused-promises) documentation (MIT).
