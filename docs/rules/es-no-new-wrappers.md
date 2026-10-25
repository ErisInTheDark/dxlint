# es-no-new-wrappers

Reports `new String`, `new Number`, and `new Boolean`. Wrapper objects behave differently from primitive values: even `new Boolean(false)` is truthy.

| | |
| --- | --- |
| Upstream | [`es-no-new-wrappers`](https://eslint.org/docs/latest/rules/no-new-wrappers) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 11 / 11 corpus cases |

## Enable

```dx
import .ts
rule es-no-new-wrappers
```

## Examples

Incorrect:

```ts
const text = new String("hello");
const enabled = new Boolean(false);
```

Correct:

```ts
const text = "hello";
const enabled = false;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-new-wrappers) documentation (MIT).
