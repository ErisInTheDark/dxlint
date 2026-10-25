# es-no-unreachable

Reports code that cannot run after an unconditional exit. Also reports instance fields in derived classes whose constructors never call `super()`.

| | |
| --- | --- |
| Upstream | [`es-no-unreachable`](https://eslint.org/docs/latest/rules/no-unreachable) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 70 / 70 corpus cases |

## Enable

```dx
import .ts
rule es-no-unreachable
```

## Examples

Incorrect:

```ts
function foo() {
  return true;
  console.log("done");
}
```

Correct:

```ts
function foo() {
  console.log("done");
  return true;
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-unreachable` | Unreachable statements. |
| `es-no-unreachable:derived` | Unreachable instance fields in derived classes. |

`rule es-no-unreachable` turns on every variant; `rule es-no-unreachable:derived off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unreachable) documentation (MIT).
