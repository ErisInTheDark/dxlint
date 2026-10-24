# complexity

Limits cyclomatic complexity: the number of independent paths through a function. Class field initialisers and static blocks are checked separately.

| | |
| --- | --- |
| Upstream | [`complexity`](https://eslint.org/docs/latest/rules/complexity) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 163 / 163 corpus cases |

## Enable

```dx
import .ts
rule complexity
```

## Examples

These examples use `max` set to `2`.

Incorrect:

```ts
function choose(value) {
  if (value === 1) return 'one';
  if (value === 2) return 'two';
  return 'other';
}
```

Correct:

```ts
function choose(value) {
  if (value === 1) return 'one';
  return 'other';
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(0..)` | `20` | Maximum allowed complexity. |
| `variant` | `"classic"` or `"modified"` | `"classic"` | Count switch branches separately or count the switch as one branch. |

```dx
rule complexity {
  set max 2
  set variant "modified"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `complexity` | Functions, arrows and methods. |
| `complexity:implicit` | Class field initialisers and static blocks. |

`rule complexity` turns on every variant; `rule complexity:implicit off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/complexity) documentation (MIT).
