# es-no-implied-eval

Reports code evaluated from strings by timers, `execScript`, or the `Function` constructor. Pass a function instead to avoid eval-like security and performance risks.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-implied-eval`](https://typescript-eslint.io/rules/no-implied-eval) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 245 / 245 corpus cases |

## Enable

```dx
import .ts
rule es-no-implied-eval
```

## Examples

Incorrect:

```ts
setTimeout("alert('Hi!')", 100);
const add = new Function("a", "b", "return a + b");
```

Correct:

```ts
setTimeout(() => alert("Hi!"), 100);
const add = (a: number, b: number) => a + b;
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-implied-eval` | Typed timer arguments that are not functions. |
| `es-no-implied-eval:core` | String timer arguments. |
| `es-no-implied-eval:exec` | `execScript` calls. |
| `es-no-implied-eval:function` | `Function` calls and construction. |

`rule es-no-implied-eval` turns on every variant; `rule es-no-implied-eval:function off` turns one off.

## Differences from upstream

- Combines ESLint's core checks with the TypeScript extension's type-aware checks under one rule family.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-implied-eval) documentation (MIT).
