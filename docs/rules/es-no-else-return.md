# es-no-else-return

Reports an `else` after a branch that always returns. Moving that code after the `if` removes unnecessary nesting.

| | |
| --- | --- |
| Upstream | [`no-else-return`](https://eslint.org/docs/latest/rules/no-else-return) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 91 / 91 corpus cases |

## Enable

```dx
import .ts
rule es-no-else-return
```

## Examples

Incorrect:

```ts
function foo() {
  if (x) {
    return y;
  } else {
    return z;
  }
}
```

Correct:

```ts
function foo() {
  if (x) {
    return y;
  }
  return z;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowElseIf` | `boolean` | `true` | Allow returning `else if` branches. |

```dx
rule es-no-else-return {
  set allowElseIf false
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-else-return) documentation (MIT).
