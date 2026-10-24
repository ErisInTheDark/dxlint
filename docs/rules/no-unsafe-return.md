# no-unsafe-return

Reports unsafe `any` return values, including arrays and promises. It also checks unsafe generic arguments against an explicit return type; returning `any` to a suitable `unknown` type is allowed.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-return`](https://typescript-eslint.io/rules/no-unsafe-return) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 61 / 62 corpus cases |

## Enable

```dx
import .ts
rule no-unsafe-return
```

## Examples

Incorrect:

```ts
function read() {
  return 1 as any;
}
const names = () => [] as any[];
```

Correct:

```ts
function read() {
  return 1;
}
const names = () => ["apple"];
```

## Variants

The base check covers unsafe direct, array, and async return values.

| Variant | Checks |
| --- | --- |
| `no-unsafe-return:assignment` | Unsafe generic arguments against the declared return type. |

`rule no-unsafe-return` turns on every variant; `rule no-unsafe-return:assignment off` turns one off.

## Differences from upstream

- Deeply nested generic promises can hide an unsafe `any` result after awaiting. A return such as `Promise.resolve<Promise<Promise<any>>>(value)` can be missed.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-return) documentation (MIT).
