# es-no-non-null-asserted-nullish-coalescing

Reports non-null assertions on the left side of `??`. The assertion says the value cannot be nullish, while the fallback is designed to handle exactly that possibility.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-non-null-asserted-nullish-coalescing`](https://typescript-eslint.io/rules/no-non-null-asserted-nullish-coalescing) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 33 / 33 corpus cases |

## Enable

```dx
import .ts
rule es-no-non-null-asserted-nullish-coalescing
```

## Examples

Incorrect:

```ts
foo! ?? bar;
foo()! ?? bar;
```

Correct:

```ts
foo ?? bar;
foo() ?? bar;
```

## Differences from upstream

- Upstream suggestions for removing the assertion are not supplied.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-non-null-asserted-nullish-coalescing) documentation (MIT).
