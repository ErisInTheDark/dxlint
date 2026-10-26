# es-prefer-spread

Reports `.apply()` calls that can use argument spread without changing the receiver. Spread makes variadic calls easier to read.

| | |
| --- | --- |
| Upstream | [`prefer-spread`](https://eslint.org/docs/latest/rules/prefer-spread) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:style` |
| Parity | 33 / 33 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-spread
```

## Examples

Incorrect:

```ts
foo.apply(null, args);
obj.foo.apply(obj, args);
```

Correct:

```ts
foo(...args);
obj.foo(...args);
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-spread) documentation (MIT).
