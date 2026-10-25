# es-prefer-numeric-literals

Reports `parseInt` calls that convert a fixed binary, octal, or hexadecimal string. Numeric literals express these values without a runtime conversion.

| | |
| --- | --- |
| Upstream | [`es-prefer-numeric-literals`](https://eslint.org/docs/latest/rules/prefer-numeric-literals) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 91 / 91 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-numeric-literals
```

## Examples

Incorrect:

```ts
parseInt('111110111', 2);
Number.parseInt('1F7', 16);
```

Correct:

```ts
0b111110111;
0x1F7;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-numeric-literals) documentation (MIT).
