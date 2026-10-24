# default-case-last

Put a switch's `default` clause after all its cases. This rule does not require a default clause to exist.

| | |
| --- | --- |
| Upstream | [`default-case-last`](https://eslint.org/docs/latest/rules/default-case-last) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 37 / 37 corpus cases |

## Enable

```dx
import .ts
rule default-case-last
```

## Examples

Incorrect:

```ts
switch (value) {
  default: fallback(); break;
  case 1: run(); break;
}
```

Correct:

```ts
switch (value) {
  case 1: run(); break;
  default: fallback(); break;
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/default-case-last) documentation (MIT).
