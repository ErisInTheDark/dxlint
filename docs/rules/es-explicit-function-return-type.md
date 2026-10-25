# es-explicit-function-return-type

Require return type annotations on functions and methods. These annotations make a function's result clear without reading its body.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/explicit-function-return-type`](https://typescript-eslint.io/rules/explicit-function-return-type) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:style` |
| Parity | 138 / 138 corpus cases |

## Enable

```dx
import .ts
rule es-explicit-function-return-type
```

## Examples

Incorrect:

```ts
function answer() { return 42; }
const name = () => "Ada";
```

Correct:

```ts
function answer(): number { return 42; }
const name = (): string => "Ada";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowConciseArrowFunctionExpressionsStartingWithVoid` | `boolean` | `false` | Exempt concise arrows starting with `void`. |
| `allowDirectConstAssertionInArrowFunctions` | `boolean` | `true` | Exempt concise arrows returning a const assertion. |
| `allowedNames` | `string[]` | `[]` | Exempt named functions and members. |
| `allowExpressions` | `boolean` | `false` | Exempt function expressions outside declarations. |
| `allowFunctionsWithoutTypeParameters` | `boolean` | `false` | Exempt nongeneric functions. |
| `allowHigherOrderFunctions` | `boolean` | `true` | Exempt functions returning only functions. |
| `allowIIFEs` | `boolean` | `false` | Exempt functions directly within calls. |
| `allowTypedFunctionExpressions` | `boolean` | `true` | Exempt contextually typed function expressions. |

```dx
rule es-explicit-function-return-type {
  set allowExpressions true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-explicit-function-return-type` | Functions. |
| `es-explicit-function-return-type:arrow` | Arrow functions. |
| `es-explicit-function-return-type:method` | Methods. |

`rule es-explicit-function-return-type` turns on every variant; `rule es-explicit-function-return-type:arrow off` turns one off.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/explicit-function-return-type) documentation (MIT).
