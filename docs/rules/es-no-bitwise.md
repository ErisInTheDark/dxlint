# es-no-bitwise

Reports bitwise operators. In code that does not manipulate bits, `&` or `|` may be a typo for `&&` or `||`.

| | |
| --- | --- |
| Upstream | [`no-bitwise`](https://eslint.org/docs/latest/rules/no-bitwise) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 26 / 26 corpus cases |

## Enable

```dx
import .ts
rule es-no-bitwise
```

## Examples

Incorrect:

```ts
const x = y | z;
const inverted = ~value;
flags &= mask;
```

Correct:

```ts
const x = y || z;
const both = y && z;
flags += value;
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-bitwise:binary` | Binary bitwise operators and their assignment forms. |
| `es-no-bitwise:unary` | Bitwise negation (`~`). |

`rule es-no-bitwise` turns on every variant; `rule es-no-bitwise:unary off` turns one off.

## Differences from upstream

- Upstream's `allow` and `int32Hint` options are not exposed as dxlint rule options.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-bitwise) documentation (MIT).
