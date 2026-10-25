# es-consistent-type-definitions

Use either interfaces or type aliases consistently for object shapes. Other aliases, such as unions and primitive types, are not affected.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/consistent-type-definitions`](https://typescript-eslint.io/rules/consistent-type-definitions) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:conventions` |
| Parity | 33 / 33 corpus cases |

## Enable

```dx
import .ts
rule es-consistent-type-definitions
```

## Examples

Incorrect:

```ts
type Point = { x: number };
```

Correct:

```ts
interface Point { x: number }
type Name = string;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"interface"` / `"type"` | `"interface"` | Preferred object definition. |

```dx
rule es-consistent-type-definitions {
  set mode "type"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-consistent-type-definitions` | Object aliases when interfaces are preferred. |
| `es-consistent-type-definitions:interface` | Interfaces when aliases are preferred. |

`rule es-consistent-type-definitions` turns on every variant; `rule es-consistent-type-definitions:interface off` turns one off.

## Differences from upstream

- The positional style option is named `mode`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/consistent-type-definitions) documentation (MIT).
