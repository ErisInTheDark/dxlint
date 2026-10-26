# es-no-label-var

Reports statement labels sharing a name with an in-scope variable. Distinct names make it clear whether a name refers to a value or a control-flow target.

| | |
| --- | --- |
| Upstream | [`no-label-var`](https://eslint.org/docs/latest/rules/no-label-var) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 5 / 5 corpus cases |

## Enable

```dx
import .ts
rule es-no-label-var
```

## Examples

Incorrect:

```ts
const x = 1;
x: for (;;) {
  break x;
}
```

Correct:

```ts
const x = 1;
outer: for (;;) {
  break outer;
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-label-var) documentation (MIT).
