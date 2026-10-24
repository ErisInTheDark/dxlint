# symbol-description

Requires a description when creating a symbol. Descriptions make symbols easier to identify in logs and debuggers.

| | |
| --- | --- |
| Upstream | [`symbol-description`](https://eslint.org/docs/latest/rules/symbol-description) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 8 / 8 corpus cases |

## Enable

```dx
import .ts
rule symbol-description
```

## Examples

Incorrect:

```ts
const key = Symbol();
```

Correct:

```ts
const key = Symbol("key");
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/symbol-description) documentation (MIT).
