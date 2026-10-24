# no-loop-func

Reports functions created in loops that capture variables modified outside their own scope. Such closures can all observe the same final value instead of one value per iteration.

| | |
| --- | --- |
| Upstream | [`no-loop-func`](https://eslint.org/docs/latest/rules/no-loop-func) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 96 / 96 corpus cases |

## Enable

```dx
import .ts
rule no-loop-func
```

## Examples

Incorrect:

```ts
for (var i = 0; i < 10; i++) {
  funcs[i] = function () { return i; };
}
```

Correct:

```ts
for (let i = 0; i < 10; i++) {
  funcs[i] = function () { return i; };
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-loop-func) documentation (MIT).
