# no-unnecessary-type-parameters

Reports type parameters that do not relate multiple parts of a signature. A concrete type, constraint or `unknown` is usually clearer.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-type-parameters`](https://typescript-eslint.io/rules/no-unnecessary-type-parameters) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 170 / 170 corpus cases |

## Enable

```dx
import .ts
rule no-unnecessary-type-parameters
```

## Examples

Incorrect:

```ts
function second<A, B>(a: A, b: B): B {
  return b;
}
```

Correct:

```ts
function second<B>(a: unknown, b: B): B {
  return b;
}
```

Uses in inferred return types count; uses only inside the implementation do not. Removing a generic constraint can also change excess-property checks at call sites.

## Differences from upstream

- dxlint reports the issue without offering upstream's replacement suggestion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-type-parameters) documentation (MIT).
