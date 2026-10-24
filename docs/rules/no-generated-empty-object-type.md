# no-generated-empty-object-type

Reports type operations that resolve to the empty object type, `{}`. That type accepts any non-nullish value, including primitives, so an accidentally empty result can weaken a type.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-generated-empty-object-type`](https://typescript-eslint.io/rules/no-generated-empty-object-type) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 25 / 27 corpus cases |

## Enable

```dx
import .ts
rule no-generated-empty-object-type
```

## Examples

Incorrect:

```ts
type Data = { name: string; value: number };
type NullableData = null | Data;
type Unexpected = Omit<NullableData, "name">;
```

Correct:

```ts
type Data = { name: string; value: number };
type NullableData = null | Data;
type Expected = Omit<NonNullable<NullableData>, "name">;
```

## Differences from upstream

- Empty structural intersections and neutral `unknown` wrappers are not fully recognised. For example, `NonNullable<unknown>` can be missed.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-generated-empty-object-type) documentation (MIT).
