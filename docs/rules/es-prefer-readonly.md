# es-prefer-readonly

Requires private members to be `readonly` when they are not reassigned outside the constructor. This makes the class's mutation contract clearer.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-readonly`](https://typescript-eslint.io/rules/prefer-readonly) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 162 / 162 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-readonly
```

## Examples

Incorrect:

```ts
class Container {
  private value = true;
}
```

Correct:

```ts
class Container {
  private readonly value = true;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `onlyInlineLambdas` | `boolean` | `false` | Limit initialised members to direct arrow functions; available on both variants. |

```dx
rule es-prefer-readonly {
  set onlyInlineLambdas true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-prefer-readonly` | Private class fields. |
| `es-prefer-readonly:parameter` | Private constructor parameter properties. |

`rule es-prefer-readonly` turns on every variant; `rule es-prefer-readonly:parameter off` turns one off.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-readonly) documentation (MIT).
