# es-no-duplicate-type-constituents

Reports repeated types in unions and intersections, including aliases that resolve to the same type. It also reports explicit `undefined` in optional parameter types.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-duplicate-type-constituents`](https://typescript-eslint.io/rules/no-duplicate-type-constituents) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 82 / 82 corpus cases |

## Enable

```dx
import .ts
rule es-no-duplicate-type-constituents
```

## Examples

Incorrect:

```ts
type T = string | string | number;
const fn = (a?: string | undefined) => {};
```

Correct:

```ts
type T = string | number;
const fn = (a?: string) => {};
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-duplicate-type-constituents` | Duplicated union and intersection members. |
| `es-no-duplicate-type-constituents:optional` | Redundant `undefined` on optional parameters. |

`rule es-no-duplicate-type-constituents` turns on every variant; `rule es-no-duplicate-type-constituents:optional off` turns one off.

## Differences from upstream

- The upstream `ignoreIntersections` and `ignoreUnions` options are not exposed.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-duplicate-type-constituents) documentation (MIT).
