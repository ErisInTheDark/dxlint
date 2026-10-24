# prefer-for-of

Reports index-based loops whose index is used only to read the current element. A `for-of` loop expresses that iteration without managing an index.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-for-of`](https://typescript-eslint.io/rules/prefer-for-of) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 68 / 70 corpus cases |

## Enable

```dx
import .ts
rule prefer-for-of
```

## Examples

Incorrect:

```ts
declare const array: string[];
for (let i = 0; i < array.length; i++) {
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

The rule checks loop structure, not whether the object is actually iterable. Objects with a `length` property can also match.

## Differences from upstream

- Two corpus cases use `yield` outside a generator. dxlint does not reproduce the upstream parser's recovery for that invalid code.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-for-of) documentation (MIT).
