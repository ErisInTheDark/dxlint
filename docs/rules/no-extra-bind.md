# no-extra-bind

Reports `.bind()` calls that do not change the function's behaviour. Binding an arrow function or a function that does not use `this` adds unnecessary work.

| | |
| --- | --- |
| Upstream | [`no-extra-bind`](https://eslint.org/docs/latest/rules/no-extra-bind) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:suspicious` |
| Parity | 49 / 49 corpus cases |

## Enable

```dx
import .ts
rule no-extra-bind
```

## Examples

Incorrect:

```ts
const getName = (function () {
  return "ESLint";
}).bind({ name: "ESLint" });
```

Correct:

```ts
const getName = (function () {
  return this.name;
}).bind({ name: "ESLint" });
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-extra-bind) documentation (MIT).
