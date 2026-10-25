# es-no-dupe-else-if

Reports conditions already covered by earlier branches in an `if` / `else if` chain. Those branches cannot run unless the conditions rely on side effects.

| | |
| --- | --- |
| Upstream | [`es-no-dupe-else-if`](https://eslint.org/docs/latest/rules/no-dupe-else-if) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 89 / 89 corpus cases |

## Enable

```dx
import .ts
rule es-no-dupe-else-if
```

## Examples

Incorrect:

```ts
if (a) {
  foo();
} else if (a) {
  bar();
}
```

Correct:

```ts
if (a) {
  foo();
} else if (b) {
  bar();
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-dupe-else-if) documentation (MIT).
