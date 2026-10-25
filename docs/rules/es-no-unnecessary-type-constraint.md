# es-no-unnecessary-type-constraint

Reports generic type parameters constrained to `any` or `unknown`. These constraints do not restrict the parameter.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-type-constraint`](https://typescript-eslint.io/rules/no-unnecessary-type-constraint) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 34 / 34 corpus cases |

## Enable

```dx
import .ts
rule es-no-unnecessary-type-constraint
```

## Examples

Incorrect:

```ts
function foo<T extends unknown>() {}
```

Correct:

```ts
function foo<T>() {}
```

## Differences from upstream

- dxlint reports the constraint without offering upstream's removal suggestion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-type-constraint) documentation (MIT).
