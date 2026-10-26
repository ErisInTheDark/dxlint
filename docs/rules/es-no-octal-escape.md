# es-no-octal-escape

Reports legacy octal escape sequences in strings. Use Unicode or hexadecimal escapes to make the intended character explicit.

| | |
| --- | --- |
| Upstream | [`no-octal-escape`](https://eslint.org/docs/latest/rules/no-octal-escape) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 94 / 94 corpus cases |

## Enable

```dx
import .ts
rule es-no-octal-escape
```

## Examples

Incorrect:

```js
const text = "Copyright \251";
```

Correct:

```js
const text = "Copyright \u00A9";
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-octal-escape) documentation (MIT).
