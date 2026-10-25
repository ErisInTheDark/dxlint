# es-no-wrapper-object-types

Reports global wrapper types such as `String`, `Number` and `Boolean`. Lowercase types describe primitive values without also accepting their wrapper objects.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-wrapper-object-types`](https://typescript-eslint.io/rules/no-wrapper-object-types) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 58 / 58 corpus cases |

## Enable

```dx
import .ts
rule es-no-wrapper-object-types
```

## Examples

Incorrect:

```ts
let name: String;
let count: Number;
let enabled: Boolean;
```

Correct:

```ts
let name: string;
let count: number;
let enabled: boolean;
```

Local types with the same names are not treated as global wrappers.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-wrapper-object-types) documentation (MIT).
