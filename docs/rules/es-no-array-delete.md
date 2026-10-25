# es-no-array-delete

Reports deleting an array element. `delete` leaves a hole without changing the array's length; use `splice` when you mean to remove an element.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-array-delete`](https://typescript-eslint.io/rules/no-array-delete) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 31 / 31 corpus cases |

## Enable

```dx
import .ts
rule es-no-array-delete
```

## Examples

Incorrect:

```ts
declare const arr: number[];
delete arr[0];
```

Correct:

```ts
declare const arr: number[];
arr.splice(0, 1);
```

## Differences from upstream

- dxlint does not offer upstream's replacement suggestion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-array-delete) documentation (MIT).
