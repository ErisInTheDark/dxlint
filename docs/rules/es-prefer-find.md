# es-prefer-find

Reports filtering an array just to take its first matching element. `find()` returns that element without building an intermediate array.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-find`](https://typescript-eslint.io/rules/prefer-find) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:types` |
| Parity | 47 / 47 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-find
```

## Examples

Incorrect:

```ts
[1, 2, 3].filter(x => x > 1)[0];
[1, 2, 3].filter(x => x > 1).at(0);
```

Correct:

```ts
[1, 2, 3].find(x => x > 1);
```

`find()` stops at the first match. Changing from `filter()` changes how often a callback runs, so check for side effects before rewriting.

## Variants

| Variant | Checks |
| --- | --- |
| `es-prefer-find` | Indexing the filtered result at zero. |
| `es-prefer-find:at` | Calling `.at(0)` on the filtered result. |

`rule es-prefer-find` turns on every variant; `rule es-prefer-find:at off` turns one off.

## Differences from upstream

- dxlint reports the pattern without upstream's editor suggestion to replace it with `find()`.
- Both variants currently use a message mentioning `.filter(...)[0]`, including `.at(0)` reports.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-find) documentation (MIT).
