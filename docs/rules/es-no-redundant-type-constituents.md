# es-no-redundant-type-constituents

Reports union and intersection members that add no useful type information. For example, `string | "ready"` is just `string`, while `any` overrides other members.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-redundant-type-constituents`](https://typescript-eslint.io/rules/no-redundant-type-constituents) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 104 / 104 corpus cases |

## Enable

```dx
import .ts
rule es-no-redundant-type-constituents
```

## Examples

Incorrect:

```ts
type Name = string | unknown;
type State = string | "ready";
type Enabled = boolean & true;
```

Correct:

```ts
type Name = unknown;
type State = string;
type Enabled = true;
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-redundant-type-constituents` | Members that override every other member. |
| `es-no-redundant-type-constituents:error` | Error types acting like `any`. |
| `es-no-redundant-type-constituents:literal` | Union literals covered by a primitive. |
| `es-no-redundant-type-constituents:overridden` | Special types overridden by other members. |
| `es-no-redundant-type-constituents:primitive` | Intersection primitives narrowed by literals. |

`rule es-no-redundant-type-constituents` turns on every variant; `rule es-no-redundant-type-constituents:literal off` turns one off.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-redundant-type-constituents) documentation (MIT).
