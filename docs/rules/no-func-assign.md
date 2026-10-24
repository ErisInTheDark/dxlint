# no-func-assign

Reports writes to function declaration names. Use a variable holding a function expression when reassignment is intentional.

| | |
| --- | --- |
| Upstream | [`no-func-assign`](https://eslint.org/docs/latest/rules/no-func-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 15 / 15 corpus cases |

## Enable

```dx
import .ts
rule no-func-assign
```

## Examples

Incorrect:

```ts
function foo() {}
foo = bar;
```

Correct:

```ts
let foo = function () {};
foo = bar;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-func-assign) documentation (MIT).
