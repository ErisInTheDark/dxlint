# no-non-null-asserted-optional-chain

Reports non-null assertions applied to optional-chain results. Optional chaining can intentionally return `undefined`, so asserting that result is non-null hides a possible failure.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-non-null-asserted-optional-chain`](https://typescript-eslint.io/rules/no-non-null-asserted-optional-chain) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 23 / 23 corpus cases |

## Enable

```dx
import .ts
rule no-non-null-asserted-optional-chain
```

## Examples

Incorrect:

```ts
foo?.bar!;
(foo?.bar)!;
```

Correct:

```ts
foo?.bar;
foo?.bar();
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-non-null-asserted-optional-chain` | Assertions directly after an optional chain. |
| `no-non-null-asserted-optional-chain:paren` | Assertions around a parenthesised optional-chain result. |

`rule no-non-null-asserted-optional-chain` turns on every variant; `rule no-non-null-asserted-optional-chain:paren off` turns one off.

## Differences from upstream

- Upstream suggestions for removing the assertion are not supplied.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-non-null-asserted-optional-chain) documentation (MIT).
