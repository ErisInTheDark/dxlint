# es-no-this-before-super

Reports use of `this` or a `super` property before `super()` in a derived constructor. Accessing the instance before its base constructor runs can throw a reference error.

| | |
| --- | --- |
| Upstream | [`no-this-before-super`](https://eslint.org/docs/latest/rules/no-this-before-super) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 63 / 63 corpus cases |

## Enable

```dx
import .ts
rule es-no-this-before-super
```

## Examples

Incorrect:

```ts
class A extends B {
  constructor() {
    this.a = 0;
    super();
  }
}
```

Correct:

```ts
class A extends B {
  constructor() {
    super();
    this.a = 0;
  }
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-this-before-super` | Early access to `this`. |
| `es-no-this-before-super:super` | Early access to a `super` property. |

`rule es-no-this-before-super` turns on every variant; `rule es-no-this-before-super:super off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-this-before-super) documentation (MIT).
