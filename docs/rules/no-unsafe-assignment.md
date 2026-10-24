# no-unsafe-assignment

Reports assignments of `any` to variables and properties. Also checks destructuring, spreads and unsafe generic type arguments.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-assignment`](https://typescript-eslint.io/rules/no-unsafe-assignment) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 89 / 92 corpus cases |

## Enable

```dx
import .ts
rule no-unsafe-assignment
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
| `no-unsafe-assignment` | Unsafe assigned values. |
| `no-unsafe-assignment:array` | Unsafe array destructuring. |
| `no-unsafe-assignment:generic` | Unsafe generic type arguments. |
| `no-unsafe-assignment:object` | Unsafe object destructuring. |
| `no-unsafe-assignment:spread` | Unsafe spreads. |
| `no-unsafe-assignment:tuple` | Unsafe tuple destructuring. |

`rule no-unsafe-assignment` turns on every variant; `rule no-unsafe-assignment:tuple off` turns one off.

## Differences from upstream

- Nested inferred arrays can stop destructuring checks before the unsafe leaf. For example, `const [[[[x]]]] = [[[[1 as any]]]]` can be missed.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-assignment) documentation (MIT).
