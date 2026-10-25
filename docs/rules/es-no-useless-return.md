# es-no-useless-return

Reports bare `return;` statements that do not change a function's control flow. Returns that supply a value or skip later work are allowed.

| | |
| --- | --- |
| Upstream | [`es-no-useless-return`](https://eslint.org/docs/latest/rules/no-useless-return) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 49 / 49 corpus cases |

## Enable

```dx
import .ts
rule es-no-useless-return
```

## Examples

Incorrect:

```ts
function run() {
  work();
  return;
}
```

Correct:

```ts
function run() {
  work();
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-return) documentation (MIT).
