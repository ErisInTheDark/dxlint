# require-array-sort-compare

Requires a comparison function for array `sort` and `toSorted` calls. Without one, numbers are compared as strings, so `10` sorts before `2`.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/require-array-sort-compare`](https://typescript-eslint.io/rules/require-array-sort-compare) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 36 / 37 corpus cases |

## Enable

```dx
import .ts
rule require-array-sort-compare
```

## Examples

Incorrect:

```ts
[1, 2, 3, 10].sort();
```

Correct:

```ts
[1, 2, 3, 10].sort((a, b) => a - b);
['one', 'two'].sort();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoreStringArrays` | `boolean` | `true` | Allow sorting arrays whose elements are all strings without a comparison function. |

```dx
rule require-array-sort-compare {
  set ignoreStringArrays false
}
```

## Differences from upstream

- A type parameter constrained to the generic spelling `Array<string>` can be reported despite `ignoreStringArrays` (`T extends string[]` is not).

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/require-array-sort-compare) documentation (MIT).
