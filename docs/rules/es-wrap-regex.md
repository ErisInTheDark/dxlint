# es-wrap-regex

Requires parentheses around regular expression literals used as the object of a property access. This helps distinguish their slashes from division operators.

| | |
| --- | --- |
| Upstream | [`@stylistic/wrap-regex`](https://eslint.style/rules/wrap-regex) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 8 / 8 corpus cases |

## Enable

```dx
import .ts
rule es-wrap-regex
```

## Examples

Incorrect:

```js
function matches() {
  return /foo/.test("bar");
}
```

Correct:

```js
function matches() {
  return (/foo/).test("bar");
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/wrap-regex) documentation (MIT).
