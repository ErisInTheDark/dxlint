# es-no-unsafe-negation

Reports a negation applied directly to the left operand of `in` or `instanceof`. Parenthesise the whole comparison when you mean to negate its result.

| | |
| --- | --- |
| Upstream | [`es-no-unsafe-negation`](https://eslint.org/docs/latest/rules/no-unsafe-negation) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 29 / 29 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-negation
```

## Examples

Incorrect:

```ts
if (!key in object) {}
if (!obj instanceof Ctor) {}
```

Correct:

```ts
if (!(key in object)) {}
if (!(obj instanceof Ctor)) {}
```

## Differences from upstream

- dxlint has no `enforceForOrderingRelations` option; `<`, `>`, `<=`, and `>=` are not checked.
- Upstream offers suggestions for moving parentheses. dxlint reports without a suggestion.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unsafe-negation) documentation (MIT).
