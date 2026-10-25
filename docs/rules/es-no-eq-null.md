# es-no-eq-null

Reports loose equality comparisons with `null`. Use strict equality when you mean only `null`, rather than both `null` and `undefined`.

| | |
| --- | --- |
| Upstream | [`es-no-eq-null`](https://eslint.org/docs/latest/rules/no-eq-null) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 5 / 5 corpus cases |

## Enable

```dx
import .ts
rule es-no-eq-null
```

## Examples

Incorrect:

```ts
if (foo == null) {
  bar();
}
```

Correct:

```ts
if (foo === null) {
  bar();
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-eq-null) documentation (MIT).
