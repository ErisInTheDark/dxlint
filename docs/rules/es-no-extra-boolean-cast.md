# es-no-extra-boolean-cast

Reports boolean conversions in places that already convert their value to a boolean. Conditions do not need an extra `!!` or `Boolean()` wrapper.

| | |
| --- | --- |
| Upstream | [`es-no-extra-boolean-cast`](https://eslint.org/docs/latest/rules/no-extra-boolean-cast) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 499 / 499 corpus cases |

## Enable

```dx
import .ts
rule es-no-extra-boolean-cast
```

## Examples

Incorrect:

```ts
if (!!foo) {
  work();
}
while (Boolean(foo)) {
  work();
}
```

Correct:

```ts
if (foo) {
  work();
}
while (foo) {
  work();
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `enforceForInnerExpressions` | `boolean` | `false` | Check casts nested in expressions used in boolean contexts. |
| `enforceForLogicalOperands` | `boolean` | `false` | Check casts nested in logical expressions; upstream deprecates this in favour of `enforceForInnerExpressions`. |

```dx
rule es-no-extra-boolean-cast {
  set enforceForInnerExpressions true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-extra-boolean-cast) documentation (MIT).
