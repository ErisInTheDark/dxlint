# await-thenable

Reports awaiting values that are not promises or other thenables. It also checks promise aggregators, `for await...of`, and `await using` for values that do not support their asynchronous operation.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/await-thenable`](https://typescript-eslint.io/rules/await-thenable) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:types` |
| Parity | 129 / 132 corpus cases |

## Enable

```dx
import .ts
rule await-thenable
```

## Examples

Incorrect:

```ts
await 'value';
const createValue = () => 'value';
await createValue();
```

Correct:

```ts
await Promise.resolve('value');
const createValue = async () => 'value';
await createValue();
```

## Variants

| Variant | Checks |
| --- | --- |
| `await-thenable` | Awaited non-thenables. |
| `await-thenable:aggregate` | Non-promise array elements passed to promise aggregators. |
| `await-thenable:iterable` | Non-promise elements in other aggregator inputs. |
| `await-thenable:for-of` | Synchronous rather than asynchronous iterables in `for await...of`. |
| `await-thenable:using` | Resources lacking asynchronous disposal in `await using`. |

`rule await-thenable` turns on every variant; `rule await-thenable:using off` turns one off.

## Differences from upstream

- dxlint does not provide upstream's suggestions to remove or change asynchronous syntax.
- The corpus exposes incomplete type answers for instances of subclasses of `Array`, such as `class Items extends Array {}; await new Items();`.
- Well-known symbol absence is not always known: an object with `Symbol.dispose` may escape the `await using` check for missing `Symbol.asyncDispose`.
- Inherited generic iterable elements are not always substituted correctly, affecting aggregators receiving an interface extending `Array<T>`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/await-thenable) documentation (MIT).
