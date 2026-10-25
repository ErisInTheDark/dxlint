# es-no-nested-ternary

Reports ternary expressions nested inside another ternary's result branches. Use a simple conditional expression or an `if` chain for clearer control flow.

| | |
| --- | --- |
| Upstream | [`es-no-nested-ternary`](https://eslint.org/docs/latest/rules/no-nested-ternary) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 4 / 4 corpus cases |

## Enable

```dx
import .ts
rule es-no-nested-ternary
```

## Examples

Incorrect:

```ts
const thing = foo ? bar : baz === qux ? quxx : foobar;
```

Correct:

```ts
const thing = foo ? bar : foobar;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-nested-ternary) documentation (MIT).
