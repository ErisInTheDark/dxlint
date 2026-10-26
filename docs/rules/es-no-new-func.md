# es-no-new-func

Reports use of the global `Function` constructor, with or without `new`. Building functions from strings makes code harder to inspect and debug, much like `eval`.

| | |
| --- | --- |
| Upstream | [`no-new-func`](https://eslint.org/docs/latest/rules/no-new-func) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 23 / 23 corpus cases |

## Enable

```dx
import .ts
rule es-no-new-func
```

## Examples

Incorrect:

```ts
const add = new Function("a", "b", "return a + b");
const sum = Function("a", "b", "return a + b");
```

Correct:

```ts
const add = function (a, b) {
  return a + b;
};
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-new-func) documentation (MIT).
