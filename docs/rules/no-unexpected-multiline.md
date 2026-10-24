# no-unexpected-multiline

Reports confusing line breaks that continue an expression instead of starting a new statement. A missing semicolon can turn the next line into a call, index access, tagged template or division.

| | |
| --- | --- |
| Upstream | [`no-unexpected-multiline`](https://eslint.org/docs/latest/rules/no-unexpected-multiline) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 58 / 58 corpus cases |

## Enable

```dx
import .ts
rule no-unexpected-multiline
```

## Examples

Incorrect:

```ts
const hello = "world"
[1, 2, 3].forEach(addNumber);
```

Correct:

```ts
const hello = "world";
[1, 2, 3].forEach(addNumber);
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unexpected-multiline) documentation (MIT).
