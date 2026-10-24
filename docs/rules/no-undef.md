# no-undef

Reports references to names that have not been declared. This catches misspellings and accidental globals.

| | |
| --- | --- |
| Upstream | [`no-undef`](https://eslint.org/docs/latest/rules/no-undef) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 103 / 103 corpus cases |

## Enable

```dx
import .ts
rule no-undef
```

## Examples

Incorrect:

```ts
const result = someFunction();
```

Correct:

```ts
function someFunction() {
  return 1;
}
const result = someFunction();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `typeof` | boolean | `false` | Also report undeclared names used in `typeof`. |

```dx
rule no-undef {
  set typeof true
}
```

By default, `typeof missingName` is allowed because it does not throw a reference error.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-undef) documentation (MIT).
