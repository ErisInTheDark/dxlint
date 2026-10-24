# no-constant-binary-expression

Reports comparisons whose result is fixed and logical expressions that always or never short-circuit. These often reveal mistaken precedence or comparisons of object identity instead of contents.

| | |
| --- | --- |
| Upstream | [`no-constant-binary-expression`](https://eslint.org/docs/latest/rules/no-constant-binary-expression) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 313 / 313 corpus cases |

## Enable

```dx
import .ts
rule no-constant-binary-expression
```

## Examples

Incorrect:

```ts
const isEmpty = someArr === [];
const value = +x == null;
const fallback = condition ? x : {} || DEFAULT;
```

Correct:

```ts
const isEmpty = someArr.length === 0;
const value = x == null;
const fallback = (condition ? x : {}) || DEFAULT;
```

## Differences from upstream

- Upstream's `checkRelationalComparisons` option is not exposed by dxlint.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-constant-binary-expression) documentation (MIT).
