# es-no-cond-assign

Reports assignments used as conditions where a comparison may have been intended. By default, extra parentheses mark an intentional assignment.

| | |
| --- | --- |
| Upstream | [`es-no-cond-assign`](https://eslint.org/docs/latest/rules/no-cond-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 45 / 45 corpus cases |

## Enable

```dx
import .ts
rule es-no-cond-assign
```

## Examples

Incorrect:

```ts
if (x = 0) {
  run();
}
```

Correct:

```ts
if (x === 0) {
  run();
}
while ((node = node.parentNode)) {
  visit(node);
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"except-parens"` or `"always"` | `"except-parens"` | Allow explicitly parenthesised assignments, or forbid assignments anywhere inside a condition. |

```dx
rule es-no-cond-assign {
  set mode "always"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-cond-assign` | Direct assignments in tests with the default mode. |
| `es-no-cond-assign:always` | Assignments within conditions when `mode` is `"always"`. |

`rule es-no-cond-assign` turns on every variant; `rule es-no-cond-assign:always off` turns one off.

## Differences from upstream

- Set `mode` rather than passing ESLint's positional string option.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-cond-assign) documentation (MIT).
