# es-no-unused-labels

Reports labels that no `break` or `continue` refers to. Removing unused labels makes control flow easier to follow.

| | |
| --- | --- |
| Upstream | [`es-no-unused-labels`](https://eslint.org/docs/latest/rules/no-unused-labels) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 35 / 35 corpus cases |

## Enable

```dx
import .ts
rule es-no-unused-labels
```

## Examples

Incorrect:

```ts
outer: {
  work();
}
```

Correct:

```ts
outer: {
  if (finished()) break outer;
  work();
}
```

## Differences from upstream

- dxlint withholds an autofix when removing the label could change directive parsing or automatic semicolon insertion, or remove an adjacent comment.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unused-labels) documentation (MIT).
