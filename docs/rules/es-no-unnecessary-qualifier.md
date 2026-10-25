# es-no-unnecessary-qualifier

Reports enum and namespace qualifiers when the member is already in scope. The shorter reference names the same declaration.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-qualifier`](https://typescript-eslint.io/rules/no-unnecessary-qualifier) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 17 / 17 corpus cases |

## Enable

```dx
import .ts
rule es-no-unnecessary-qualifier
```

## Examples

Incorrect:

```ts
enum A {
  B,
  C = A.B,
}
```

Correct:

```ts
enum A {
  B,
  C = B,
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-qualifier) documentation (MIT).
