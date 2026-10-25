# es-no-unnecessary-template-expression

Reports template substitutions that can be incorporated into the surrounding text, or replaced with the string expression itself. Checks both values and template literal types.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-template-expression`](https://typescript-eslint.io/rules/no-unnecessary-template-expression) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | `rules:types` |
| Parity | 184 / 184 corpus cases |

## Enable

```dx
import .ts
rule es-no-unnecessary-template-expression
```

## Examples

Incorrect:

```ts
const text = `a${"b"}`;
type Text = `A${"B"}`;
```

Correct:

```ts
const text = `ab`;
type Text = `AB`;
```

This does not require ordinary quoted strings instead of templates without substitutions.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-template-expression) documentation (MIT).
