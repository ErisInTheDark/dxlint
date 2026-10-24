# no-empty-character-class

Reports empty character classes in regular expression literals. An empty class matches nothing and usually indicates an unfinished pattern.

| | |
| --- | --- |
| Upstream | [`no-empty-character-class`](https://eslint.org/docs/latest/rules/no-empty-character-class) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 42 / 42 corpus cases |

## Enable

```dx
import .ts
rule no-empty-character-class
```

## Examples

Incorrect:

```ts
const pattern = /^abc[]/;
```

Correct:

```ts
const pattern = /^abc[a-z]/;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-empty-character-class) documentation (MIT).
