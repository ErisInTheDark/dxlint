# es-no-dupe-args

Reports repeated parameter names in function declarations and expressions. A later parameter with the same name hides an earlier one and can indicate a typo.

| | |
| --- | --- |
| Upstream | [`no-dupe-args`](https://eslint.org/docs/latest/rules/no-dupe-args) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 13 / 13 corpus cases |

## Enable

```dx
import .ts
rule es-no-dupe-args
```

## Examples

Incorrect:

```js
function foo(a, b, a) {
  console.log(a, b);
}
```

Correct:

```js
function foo(a, b, c) {
  console.log(a, b, c);
}
```

The incorrect example is non-strict script code. Strict JavaScript also rejects duplicate parameters; arrows and class methods forbid them regardless of strict mode.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-dupe-args) documentation (MIT).
