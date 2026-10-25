# es-prefer-reduce-type-parameter

Reports assertions on the initial value passed to `Array#reduce`. An explicit type argument gives the accumulator its intended type without weakening checks with an assertion.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-reduce-type-parameter`](https://typescript-eslint.io/rules/prefer-reduce-type-parameter) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 31 / 31 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-reduce-type-parameter
```

## Examples

Incorrect:

```ts
[1, 2, 3].reduce((arr, num) => arr.concat(num * 2), [] as number[]);
```

Correct:

```ts
[1, 2, 3].reduce<number[]>((arr, num) => arr.concat(num * 2), []);
```

## Differences from upstream

- None known: every upstream test case matches.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-reduce-type-parameter) documentation (MIT).
