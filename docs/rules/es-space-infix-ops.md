# es-space-infix-ops

Requires spaces around infix operators, assignments and conditional-expression punctuation. This makes adjacent operators and operands easier to distinguish.

| | |
| --- | --- |
| Upstream | [`@stylistic/space-infix-ops`](https://eslint.style/rules/space-infix-ops) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 273 / 273 corpus cases |

## Enable

```dx
import .ts
rule es-space-infix-ops
```

## Examples

Incorrect:

```ts
const sum=1+2;
const result = ready?yes:no;
```

Correct:

```ts
const sum = 1 + 2;
const result = ready ? yes : no;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `int32Hint` | boolean | `false` | Allow a terminal `value\|0` integer conversion without spaces. |

```dx
rule es-space-infix-ops {
  set int32Hint true
}
```

## Differences from upstream

- Upstream's `ignoreOperators` and `ignoreTypes` options are not supported.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/space-infix-ops) documentation (MIT).
