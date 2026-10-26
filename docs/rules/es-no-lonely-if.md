# es-no-lonely-if

Reports an `if` that is the only statement in an `else` block. An `else if` expresses the same choice without an extra block.

| | |
| --- | --- |
| Upstream | [`no-lonely-if`](https://eslint.org/docs/latest/rules/no-lonely-if) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 17 / 17 corpus cases |

## Enable

```dx
import .ts
rule es-no-lonely-if
```

## Examples

Incorrect:

```ts
if (first) {
  one();
} else {
  if (second) {
    two();
  }
}
```

Correct:

```ts
if (first) {
  one();
} else if (second) {
  two();
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-lonely-if) documentation (MIT).
