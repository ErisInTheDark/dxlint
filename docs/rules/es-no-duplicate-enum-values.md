# es-no-duplicate-enum-values

Reports enum members explicitly assigned the same literal value. Unique values make it easier to distinguish members and avoid accidental aliases.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-duplicate-enum-values`](https://typescript-eslint.io/rules/no-duplicate-enum-values) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 40 / 40 corpus cases |

## Enable

```dx
import .ts
rule es-no-duplicate-enum-values
```

## Examples

Incorrect:

```ts
enum E {
  A = 0,
  B = 0,
}
```

Correct:

```ts
enum E {
  A = 0,
  B = 1,
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-duplicate-enum-values) documentation (MIT).
