# no-const-assign

Reports writes to constant bindings. Reassigning a `const` variable throws at runtime; changing an object's properties is different from replacing its binding.

| | |
| --- | --- |
| Upstream | [`no-const-assign`](https://eslint.org/docs/latest/rules/no-const-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 33 / 33 corpus cases |

## Enable

```dx
import .ts
rule no-const-assign
```

## Examples

Incorrect:

```ts
const a = 0;
a = 1;
```

Correct:

```ts
const a = 0;
console.log(a);
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-const-assign) documentation (MIT).
