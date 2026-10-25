# es-operator-assignment

Requires or forbids compound arithmetic and bitwise assignments such as `+=`. Logical assignments such as `&&=` are not equivalent to ordinary repeated assignment and are not covered.

| | |
| --- | --- |
| Upstream | [`es-operator-assignment`](https://eslint.org/docs/latest/rules/operator-assignment) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 119 / 119 corpus cases |

## Enable

```dx
import .ts
rule es-operator-assignment
```

## Examples

Incorrect:

```ts
x = x + y;
x[0] = x[0] / y;
```

Correct:

```ts
x += y;
x[0] /= y;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never"` | `"always"` | Require or forbid compound assignment. |

```dx
rule es-operator-assignment {
  set mode "never"
}
```

## Differences from upstream

- Set the upstream positional string through the named `mode` option.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/operator-assignment) documentation (MIT).
