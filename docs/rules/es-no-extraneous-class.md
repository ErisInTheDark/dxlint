# es-no-extraneous-class

Reports empty classes, constructor-only classes and classes used only as static namespaces. Module-level functions and values usually express those roles without an extra class.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-extraneous-class`](https://typescript-eslint.io/rules/no-extraneous-class) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:conventions` |
| Parity | 28 / 28 corpus cases |

## Enable

```dx
import .ts
rule es-no-extraneous-class
```

## Examples

Incorrect:

```ts
class StaticConstants {
  static readonly version = 42;
}
class HelloWorldLogger {
  constructor() {
    console.log("Hello, world!");
  }
}
```

Correct:

```ts
export const version = 42;
function logHelloWorld() {
  console.log("Hello, world!");
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-extraneous-class:constructor` | Classes with only a constructor and no parameter properties. |
| `es-no-extraneous-class:empty` | Classes with no members. |
| `es-no-extraneous-class:static` | Classes with only static members. |

`rule es-no-extraneous-class` turns on every variant; `rule es-no-extraneous-class:empty off` turns one off.

## Differences from upstream

- Use variant switches instead of `allowConstructorOnly`, `allowEmpty` or `allowStaticOnly`. For example, `rule es-no-extraneous-class:constructor off` allows constructor-only classes.
- The upstream `allowWithDecorator` option is not exposed.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-extraneous-class) documentation (MIT).
