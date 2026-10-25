# es-no-unsafe-assignment

Reports assignments of `any` to variables and properties. Also checks destructuring, spreads and unsafe generic type arguments.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-assignment`](https://typescript-eslint.io/rules/no-unsafe-assignment) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:type-safety`, `rules:recommended` |
| Parity | 92 / 92 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-assignment
```

## Examples

Incorrect:

```ts
const value = 1 as any;
```

Correct:

```ts
const value = 1;
```

Assigning `any` to `unknown` is allowed because the receiving code must narrow it before use.

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-unsafe-assignment` | Unsafe assigned values. |
| `es-no-unsafe-assignment:array` | Unsafe array destructuring. |
| `es-no-unsafe-assignment:generic` | Unsafe generic type arguments. |
| `es-no-unsafe-assignment:object` | Unsafe object destructuring. |
| `es-no-unsafe-assignment:spread` | Unsafe spreads. |
| `es-no-unsafe-assignment:tuple` | Unsafe tuple destructuring. |

`rule es-no-unsafe-assignment` turns on every variant; `rule es-no-unsafe-assignment:tuple off` turns one off.

## Differences from upstream

- None known: every upstream test case matches.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-assignment) documentation (MIT).
