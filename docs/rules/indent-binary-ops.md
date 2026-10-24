# indent-binary-ops

Reports uneven indentation in multiline binary expressions. Use it alongside `indent`, with the same indentation width, to align continued operators and operands.

| | |
| --- | --- |
| Upstream | [`@stylistic/indent-binary-ops`](https://eslint.style/rules/indent-binary-ops) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 50 / 50 corpus cases |

## Enable

```dx
import .ts
rule indent-binary-ops
```

## Examples

Incorrect:

```ts
if (a
    && b
      && c
) {
  foo();
}
```

Correct:

```ts
if (a
  && b
  && c
) {
  foo();
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `number \| "tab"` | `2` | Indentation width or tabs. |

```dx
rule indent {
  set mode 2
}
rule indent-binary-ops {
  set mode 2
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/indent-binary-ops) documentation (MIT).
