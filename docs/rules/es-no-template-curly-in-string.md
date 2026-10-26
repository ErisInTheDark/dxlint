# es-no-template-curly-in-string

Reports `${...}` placeholders inside ordinary quoted strings. These strings do not interpolate expressions; use backticks for template literals.

| | |
| --- | --- |
| Upstream | [`no-template-curly-in-string`](https://eslint.org/docs/latest/rules/no-template-curly-in-string) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 22 / 22 corpus cases |

## Enable

```dx
import .ts
rule es-no-template-curly-in-string
```

## Examples

Incorrect:

```ts
"Hello ${name}!";
'Time: ${12 * 60 * 60 * 1000}';
```

Correct:

```ts
`Hello ${name}!`;
`Time: ${12 * 60 * 60 * 1000}`;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-template-curly-in-string) documentation (MIT).
