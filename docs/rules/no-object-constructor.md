# no-object-constructor

Reports calls to `Object` without an argument, with or without `new`. An object literal is shorter and avoids relying on the global constructor; wrapping a supplied value is allowed.

| | |
| --- | --- |
| Upstream | [`no-object-constructor`](https://eslint.org/docs/latest/rules/no-object-constructor) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 12 / 12 corpus cases |

## Enable

```dx
import .ts
rule no-object-constructor
```

## Examples

Incorrect:

```ts
const a = Object();
const b = new Object();
```

Correct:

```ts
const a = {};
const b = Object("foo");
```

## Differences from upstream

- Upstream suggestions to use object literals are not supplied.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-object-constructor) documentation (MIT).
