# es-no-octal

Reports legacy octal number literals such as `071`. Their leading zero is easy to overlook; use decimal or explicit `0o` notation instead.

| | |
| --- | --- |
| Upstream | [`es-no-octal`](https://eslint.org/docs/latest/rules/no-octal) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 17 / 17 corpus cases |

## Enable

```dx
import .ts
rule es-no-octal
```

## Examples

Incorrect:

```js
const num = 071;
```

Correct:

```js
const num = 57;
const octal = 0o71;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-octal) documentation (MIT).
