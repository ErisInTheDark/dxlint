# no-duplicate-case

Reports repeated test expressions in a `switch` statement. A duplicated case often means a copied branch was not updated.

| | |
| --- | --- |
| Upstream | [`no-duplicate-case`](https://eslint.org/docs/latest/rules/no-duplicate-case) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 29 / 29 corpus cases |

## Enable

```dx
import .ts
rule no-duplicate-case
```

## Examples

Incorrect:

```ts
switch (value) {
  case 1: break;
  case 2: break;
  case 1: break;
}
```

Correct:

```ts
switch (value) {
  case 1: break;
  case 2: break;
  case 3: break;
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-duplicate-case) documentation (MIT).
