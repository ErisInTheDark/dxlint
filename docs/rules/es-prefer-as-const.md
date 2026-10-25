# es-prefer-as-const

Reports repeated literal types that can be expressed with a const assertion. `as const` asks TypeScript to infer the literal type instead of spelling it twice.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-as-const`](https://typescript-eslint.io/rules/prefer-as-const) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 46 / 46 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-as-const
```

## Examples

Incorrect:

```ts
let foo = "bar" as "bar";
let count: 2 = 2;
```

Correct:

```ts
let foo = "bar" as const;
let count = 2 as const;
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-prefer-as-const` | Literal type assertions. |
| `es-prefer-as-const:annotation` | Literal annotations on variables and class properties. |

`rule es-prefer-as-const` turns on every variant; `rule es-prefer-as-const:annotation off` turns one off.

## Differences from upstream

- Assertion reports have an autofix; annotation reports do not provide an editor suggestion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-as-const) documentation (MIT).
