# es-no-underscore-dangle

Reports names with leading or trailing underscores. Underscores are a naming convention, not access control; private class fields can enforce privacy.

| | |
| --- | --- |
| Upstream | [`no-underscore-dangle`](https://eslint.org/docs/latest/rules/no-underscore-dangle) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 116 / 116 corpus cases |

## Enable

```dx
import .ts
rule es-no-underscore-dangle
```

## Examples

Incorrect:

```ts
let foo_;
foo._bar();
```

Correct:

```ts
let foo;
foo.bar();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | string[] | `[]` | Names allowed to contain dangling underscores. |
| `allowAfterSuper` | boolean | `false` | Allow underscored members on `super`. |
| `allowAfterThis` | boolean | `false` | Allow underscored members on `this`. |
| `allowAfterThisConstructor` | boolean | `false` | Allow underscored members on `this.constructor`. |
| `allowFunctionParams` | boolean | `true` | Allow underscored parameter names. |
| `allowInArrayDestructuring` | boolean | `true` | Allow underscored bindings in array patterns. |
| `allowInObjectDestructuring` | boolean | `true` | Allow underscored bindings in object patterns. |
| `enforceInClassFields` | boolean | `false` | Check class fields, including private fields. |
| `enforceInMethodNames` | boolean | `false` | Check method names. |

```dx
rule es-no-underscore-dangle {
  set allowAfterThis true
  set enforceInClassFields true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-underscore-dangle) documentation (MIT).
