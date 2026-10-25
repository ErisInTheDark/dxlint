# es-no-useless-call

Reports `.call()` and `.apply()` uses that can be replaced with a direct call without changing the intended `this` value. Calls that deliberately select a different receiver are allowed.

| | |
| --- | --- |
| Upstream | [`es-no-useless-call`](https://eslint.org/docs/latest/rules/no-useless-call) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 44 / 44 corpus cases |

## Enable

```dx
import .ts
rule es-no-useless-call
```

## Examples

Incorrect:

```ts
foo.call(undefined, 1, 2);
obj.foo.apply(obj, [1, 2]);
```

Correct:

```ts
foo(1, 2);
obj.foo(1, 2);
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-call) documentation (MIT).
