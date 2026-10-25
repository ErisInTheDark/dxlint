# es-no-unsafe-call

Reports calls through values typed as `any` or the unsafe `Function` type. These types do not establish a safe call signature.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-call`](https://typescript-eslint.io/rules/no-unsafe-call) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:type-safety`, `rules:recommended` |
| Parity | 38 / 39 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-call
```

## Examples

Incorrect:

```ts
declare const callback: any;
callback();
```

Correct:

```ts
declare const callback: () => void;
callback();
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-unsafe-call` | Function calls. |
| `es-no-unsafe-call:new` | Constructor calls. |
| `es-no-unsafe-call:tag` | Tagged templates. |

`rule es-no-unsafe-call` turns on every variant; `rule es-no-unsafe-call:tag off` turns one off.

## Differences from upstream

- With `noImplicitThis` disabled, an object method's `this` can be inferred differently. Unsafe receiver calls such as `this()` may therefore be missed.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-call) documentation (MIT).
