# es-no-negated-condition

Reports negated conditions in ternaries and `if` statements with an `else` branch. A positive condition and swapped branches are often easier to read; guard clauses without an `else` are allowed.

| | |
| --- | --- |
| Upstream | [`no-negated-condition`](https://eslint.org/docs/latest/rules/no-negated-condition) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 19 / 19 corpus cases |

## Enable

```dx
import .ts
rule es-no-negated-condition
```

## Examples

Incorrect:

```ts
const result = !ready ? fallback : value;
```

Correct:

```ts
const result = ready ? value : fallback;
if (!ready) {
  return;
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-negated-condition) documentation (MIT).
