# es-prefer-enum-initializers

Reports enum members without explicit values. Initialisers prevent changing one member from silently changing later numeric values.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-enum-initializers`](https://typescript-eslint.io/rules/prefer-enum-initializers) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 8 / 8 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-enum-initializers
```

## Examples

Incorrect:

```ts
enum Direction {
  Up,
  Down,
}
```

Correct:

```ts
enum Direction {
  Up = 1,
  Down = 2,
}
```

## Differences from upstream

- dxlint reports missing initialisers without upstream's editor suggestions for possible values.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-enum-initializers) documentation (MIT).
