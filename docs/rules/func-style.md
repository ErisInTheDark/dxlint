# func-style

Choose function declarations or expressions for functions assigned to names. Callback arguments are outside this style policy.

| | |
| --- | --- |
| Upstream | [`func-style`](https://eslint.org/docs/latest/rules/func-style) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 120 / 120 corpus cases |

## Enable

```dx
import .ts
rule func-style
```

## Examples

Incorrect:

```ts
function foo() {}
```

Correct:

```ts
const foo = function() {};
const bar = () => {};
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"declaration"` / `"expression"` | `"expression"` | Preferred function form. |
| `allowArrowFunctions` | `boolean` | `false` | Permit arrows when declarations are required. |
| `allowTypeAnnotation` | `boolean` | `false` | Permit function variables with explicit type annotations. |
| `overrides` | `{ namedExports: "" \| "declaration" \| "expression" \| "ignore" }` | `{ namedExports: "" }` | Named-export policy; an empty string inherits `style`. |

```dx
rule func-style {
  set style "declaration"
  set allowArrowFunctions true
}
```

## Differences from upstream

- The positional function policy is named `style`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/func-style) documentation (MIT).
