# es-no-for-in-array

Reports `for-in` loops over arrays. These loops skip holes, produce string keys, and may visit extra or inherited properties; use `for-of` for values.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-for-in-array`](https://typescript-eslint.io/rules/no-for-in-array) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 24 / 24 corpus cases |

## Enable

```dx
import .ts
rule es-no-for-in-array
```

## Examples

Incorrect:

```ts
declare const array: string[];
for (const i in array) {
  console.log(array[i]);
}
```

Correct:

```ts
declare const array: string[];
for (const value of array) {
  console.log(value);
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-for-in-array) documentation (MIT).
