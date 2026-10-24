# space-unary-ops

Controls spaces beside unary operators. Word operators require a space by default, while symbolic operators stay attached to their operands.

| | |
| --- | --- |
| Upstream | [`@stylistic/space-unary-ops`](https://eslint.style/rules/space-unary-ops) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 122 / 122 corpus cases |

## Enable

```dx
import .ts
rule space-unary-ops
```

## Examples

Incorrect:

```ts
typeof!foo;
++ foo;
```

Correct:

```ts
typeof !foo;
++foo;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `words` | boolean | `true` | Require spaces beside word operators where spacing is optional. |
| `nonwords` | boolean | `false` | Require spaces beside symbolic unary operators. |
| `overrides` | `{ [key: string]: boolean }` | `{}` | Override individual operators; `ts-non-null` controls TypeScript non-null assertions. |

```dx
rule space-unary-ops {
  set words true
  set nonwords false
  set overrides { "++": true }
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/space-unary-ops) documentation (MIT).
