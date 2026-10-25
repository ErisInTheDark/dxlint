# es-no-useless-rename

Reports imports, exports, and destructuring bindings that rename a name to itself. Use the shorter shorthand when the name is unchanged.

| | |
| --- | --- |
| Upstream | [`es-no-useless-rename`](https://eslint.org/docs/latest/rules/no-useless-rename) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 162 / 162 corpus cases |

## Enable

```dx
import .ts
rule es-no-useless-rename
```

## Examples

Incorrect:

```ts
import { foo as foo } from "bar";
export { foo as foo };
const { value: value } = object;
```

Correct:

```ts
import { foo } from "bar";
export { foo };
const { value } = object;
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-useless-rename:import` | Import specifiers. |
| `es-no-useless-rename:export` | Export specifiers. |
| `es-no-useless-rename:destructuring` | Object destructuring bindings. |

`rule es-no-useless-rename` turns on every variant; `rule es-no-useless-rename:import off` turns one off.

## Differences from upstream

- There are no `ignoreImport`, `ignoreExport`, or `ignoreDestructuring` options. Turn off the corresponding variant instead.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-rename) documentation (MIT).
