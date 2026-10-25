# es-no-delete-var

Reports use of `delete` on a variable. `delete` removes object properties, not local bindings.

| | |
| --- | --- |
| Upstream | [`es-no-delete-var`](https://eslint.org/docs/latest/rules/no-delete-var) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 2 / 2 corpus cases |

## Enable

```dx
import .ts
rule es-no-delete-var
```

## Examples

Incorrect:

```js
var x;
delete x;
```

Correct:

```js
const object = { x: 1 };
delete object.x;
```

The incorrect example is non-strict script code; JavaScript also forbids deleting a variable in strict mode.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-delete-var) documentation (MIT).
