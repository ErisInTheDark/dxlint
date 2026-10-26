# es-no-ternary

Reports conditional expressions using `? :`. Use this rule when a project prefers explicit `if`/`else` branches.

| | |
| --- | --- |
| Upstream | [`no-ternary`](https://eslint.org/docs/latest/rules/no-ternary) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 4 / 4 corpus cases |

## Enable

```dx
import .ts
rule es-no-ternary
```

## Examples

Incorrect:

```ts
const foo = isBar ? baz : qux;
```

Correct:

```ts
let foo;
if (isBar) {
  foo = baz;
} else {
  foo = qux;
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-ternary) documentation (MIT).
