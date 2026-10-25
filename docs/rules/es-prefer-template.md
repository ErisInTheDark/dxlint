# es-prefer-template

Reports string concatenation with non-literal values. Template literals keep interpolated values next to the surrounding text.

| | |
| --- | --- |
| Upstream | [`es-prefer-template`](https://eslint.org/docs/latest/rules/prefer-template) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 87 / 87 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-template
```

## Examples

Incorrect:

```ts
const greeting = 'Hello, ' + name + '!';
```

Correct:

```ts
const greeting = `Hello, ${name}!`;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-template) documentation (MIT).
