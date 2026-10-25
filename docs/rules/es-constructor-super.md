# es-constructor-super

Require valid `super()` calls in constructors. Derived constructors must initialise the base class, while constructors without a base class must not call `super()`.

| | |
| --- | --- |
| Upstream | [`es-constructor-super`](https://eslint.org/docs/latest/rules/constructor-super) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 82 / 82 corpus cases |

## Enable

```dx
import .ts
rule es-constructor-super
```

## Examples

Incorrect:

```ts
class Child extends Parent {
  constructor() {}
}
```

Correct:

```ts
class Child extends Parent {
  constructor() { super(); }
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-constructor-super` | Constructor paths that omit required base initialisation. |
| `es-constructor-super:call` | Invalid or repeated `super()` calls. |

`rule es-constructor-super` turns on every variant; `rule es-constructor-super:call off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/constructor-super) documentation (MIT).
