# es-no-confusing-non-null-assertion

Reports non-null assertions next to assignment, equality, `in` or `instanceof` operators. These combinations can look like inequality or negated membership tests.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-confusing-non-null-assertion`](https://typescript-eslint.io/rules/no-confusing-non-null-assertion) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:style` |
| Parity | 19 / 19 corpus cases |

## Enable

```dx
import .ts
rule es-no-confusing-non-null-assertion
```

## Examples

Incorrect:

```ts
declare const foo: { bar?: string };
const matches = foo.bar! == "hello";
```

Correct:

```ts
declare const foo: { bar?: string };
const matches = foo.bar == "hello";
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-confusing-non-null-assertion` | Assertions next to equality, `in` and `instanceof` operators. |
| `es-no-confusing-non-null-assertion:assign` | Assertions next to assignments, such as `a! = b`. |

`rule es-no-confusing-non-null-assertion` turns on every variant; `rule es-no-confusing-non-null-assertion:assign off` turns one off.

## Differences from upstream

- dxlint does not offer upstream's suggestions to remove or parenthesise the assertion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-confusing-non-null-assertion) documentation (MIT).
