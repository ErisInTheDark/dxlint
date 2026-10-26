# es-no-labels

Reports labeled statements and labeled `break` or `continue`. Avoiding labels can make control flow easier to follow.

| | |
| --- | --- |
| Upstream | [`no-labels`](https://eslint.org/docs/latest/rules/no-labels) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 29 / 29 corpus cases |

## Enable

```dx
import .ts
rule es-no-labels
```

## Examples

Incorrect:

```ts
outer: while (ready) {
  break outer;
}
```

Correct:

```ts
while (ready) {
  break;
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-labels` | Labeled statements. |
| `es-no-labels:break` | Labels on `break`. |
| `es-no-labels:continue` | Labels on `continue`. |

`rule es-no-labels` turns on every variant; `rule es-no-labels:break off` turns one off.

## Differences from upstream

- Upstream's `allowLoop` and `allowSwitch` options are not exposed. Enabled variants check all labels in their category.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-labels) documentation (MIT).
