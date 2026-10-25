# es-no-return-assign

Reports assignments inside returned expressions. Separating assignment from return makes it clear whether the code meant to assign or compare.

| | |
| --- | --- |
| Upstream | [`es-no-return-assign`](https://eslint.org/docs/latest/rules/no-return-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 30 / 30 corpus cases |

## Enable

```dx
import .ts
rule es-no-return-assign
```

## Examples

Incorrect:

```ts
function update() {
  return foo = bar + 2;
}
const copy = (a, b) => a = b;
```

Correct:

```ts
function update() {
  foo = bar + 2;
  return foo;
}
const copy = (a, b) => (a = b);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"except-parens" \| "always"` | `"except-parens"` | Allow explicitly parenthesised assignments, or forbid every returned assignment. |

```dx
rule es-no-return-assign {
  set mode "always"
}
```

## Differences from upstream

- ESLint's positional string option is named `mode` in dxlint.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-return-assign) documentation (MIT).
