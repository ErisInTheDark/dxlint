# no-multi-str

Reports quoted strings continued across physical lines with a backslash. Use one line, concatenation, or a template literal instead.

| | |
| --- | --- |
| Upstream | [`no-multi-str`](https://eslint.org/docs/latest/rules/no-multi-str) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 7 / 7 corpus cases |

## Enable

```dx
import .ts
rule no-multi-str
```

## Examples

Incorrect:

```ts
const text = "some very \
long text";
```

Correct:

```ts
const text = "some very " +
  "long text";
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-multi-str) documentation (MIT).
