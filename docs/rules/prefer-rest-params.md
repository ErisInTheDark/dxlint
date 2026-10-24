# prefer-rest-params

Reports use of the implicit `arguments` object. Rest parameters name the argument list and provide a real array.

| | |
| --- | --- |
| Upstream | [`prefer-rest-params`](https://eslint.org/docs/latest/rules/prefer-rest-params) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:conventions` |
| Parity | 11 / 11 corpus cases |

## Enable

```dx
import .ts
rule prefer-rest-params
```

## Examples

Incorrect:

```ts
function foo() {
  console.log(arguments);
}
```

Correct:

```ts
function foo(...args: unknown[]) {
  console.log(args);
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-rest-params) documentation (MIT).
