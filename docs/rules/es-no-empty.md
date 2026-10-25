# es-no-empty

Reports empty blocks and empty `switch` statements. A comment inside the block can explain why doing nothing is intentional.

| | |
| --- | --- |
| Upstream | [`es-no-empty`](https://eslint.org/docs/latest/rules/no-empty) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 35 / 35 corpus cases |

## Enable

```dx
import .ts
rule es-no-empty
```

## Examples

Incorrect:

```ts
if (foo) {}
switch (foo) {}
```

Correct:

```ts
if (foo) {
  // Intentionally empty.
}
switch (foo) {
  case 1: bar(); break;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowEmptyCatch` | `boolean` | `false` | Allow empty catch blocks without a comment. |

```dx
rule es-no-empty {
  set allowEmptyCatch true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-empty) documentation (MIT).
