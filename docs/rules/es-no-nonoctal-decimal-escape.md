# es-no-nonoctal-decimal-escape

Reports `\8` and `\9` in string literals. These legacy escapes are unnecessary and unavailable in strict-mode JavaScript.

| | |
| --- | --- |
| Upstream | [`no-nonoctal-decimal-escape`](https://eslint.org/docs/latest/rules/no-nonoctal-decimal-escape) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 84 / 84 corpus cases |

## Enable

```dx
import .ts
rule es-no-nonoctal-decimal-escape
```

## Examples

Incorrect:

```js
const foo = "w\8less";
const bar = "December 1\9";
```

Correct:

```js
const foo = "w8less";
const bar = "December 19";
```

## Differences from upstream

- Upstream suggestions for removing or escaping the backslash are not supplied.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-nonoctal-decimal-escape) documentation (MIT).
