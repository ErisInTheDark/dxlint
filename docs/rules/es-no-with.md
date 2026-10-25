# es-no-with

Reports `with` statements. They make name lookup depend on an object's properties, obscuring which variable a name refers to.

| | |
| --- | --- |
| Upstream | [`es-no-with`](https://eslint.org/docs/latest/rules/no-with) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 2 / 2 corpus cases |

## Enable

```dx
import .ts
rule es-no-with
```

## Examples

Incorrect:

```js
with (point) {
  r = Math.sqrt(x * x + y * y);
}
```

Correct:

```js
const r = ({ x, y }) => Math.sqrt(x * x + y * y);
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-with) documentation (MIT).
