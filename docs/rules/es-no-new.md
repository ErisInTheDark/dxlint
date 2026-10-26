# es-no-new

Reports constructor calls used as standalone statements, discarding the new object. Store or otherwise use the result, or call an ordinary function when only side effects are intended.

| | |
| --- | --- |
| Upstream | [`no-new`](https://eslint.org/docs/latest/rules/no-new) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 3 / 3 corpus cases |

## Enable

```dx
import .ts
rule es-no-new
```

## Examples

Incorrect:

```ts
new Thing();
```

Correct:

```ts
const thing = new Thing();
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-new) documentation (MIT).
