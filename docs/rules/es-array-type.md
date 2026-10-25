# es-array-type

Enforces one notation for TypeScript array types: `T[]` or `Array<T>`. Mutable and readonly arrays can use different policies.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/array-type`](https://typescript-eslint.io/rules/array-type) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:conventions` |
| Parity | 193 / 193 corpus cases |

## Enable

```dx
import .ts
rule es-array-type
```

## Examples

Incorrect:

```ts
const values: Array<string> = ['a', 'b'];
const names: ReadonlyArray<string> = ['a', 'b'];
```

Correct:

```ts
const values: string[] = ['a', 'b'];
const names: readonly string[] = ['a', 'b'];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"array"`, `"array-simple"` or `"generic"` | `"array"` | Array notation, set separately on each variant. |

`array` prefers `T[]`; `generic` prefers `Array<T>`. `array-simple` uses brackets for simple named types and generic notation for more complex types.

```dx
rule es-array-type:mutable {
  set mode "array-simple"
}
rule es-array-type:readonly {
  set mode "generic"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-array-type:mutable` | Mutable array types. |
| `es-array-type:readonly` | Readonly array types. |

`rule es-array-type` turns on every variant; `rule es-array-type:readonly off` turns one off.

## Differences from upstream

- Upstream's `default` and `readonly` options become the `mode` option on the mutable and readonly variants.
- Each variant defaults to `"array"` independently; changing the mutable policy does not also change the readonly policy.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/array-type) documentation (MIT).
