# es-no-useless-concat

Reports adjacent string or template literals joined with `+` when one literal would do. Concatenations split across lines are allowed.

| | |
| --- | --- |
| Upstream | [`no-useless-concat`](https://eslint.org/docs/latest/rules/no-useless-concat) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 20 / 20 corpus cases |

## Enable

```dx
import .ts
rule es-no-useless-concat
```

## Examples

Incorrect:

```ts
const value = "a" + "b";
const digits = `1` + "0";
```

Correct:

```ts
const value = "ab";
const digits = "10";
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-concat) documentation (MIT).
