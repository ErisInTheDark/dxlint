# es-jsx-wrap-multilines

Reports multiline JSX without the configured parentheses. Wrapping JSX makes expression boundaries easier to see.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-wrap-multilines`](https://eslint.style/rules/jsx-wrap-multilines) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 146 / 146 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-wrap-multilines
```

## Examples

Incorrect:

```tsx
const view = <div>
  <p>Hello</p>
</div>;
```

Correct:

```tsx
const view = (
  <div>
    <p>Hello</p>
  </div>
);
```

## Options

Each option accepts `"parens"`, `"parens-new-line"`, `"ignore"`, or a boolean. `"parens-new-line"` requires parentheses on separate lines; `true` acts like `"parens"` and `false` disables that context. Conditional branches inside declarations or assignments inherit that surrounding policy when `condition` is ignored.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `declaration` | `jsx_wrap_mode` | `"parens"` | Variable initialisers. |
| `assignment` | `jsx_wrap_mode` | `"parens"` | Assignment values. |
| `return` | `jsx_wrap_mode` | `"parens"` | Return expressions. |
| `arrow` | `jsx_wrap_mode` | `"parens"` | Arrow function bodies. |
| `condition` | `jsx_wrap_mode` | `"ignore"` | Conditional branches. |
| `logical` | `jsx_wrap_mode` | `"ignore"` | Logical right operands. |
| `prop` | `jsx_wrap_mode` | `"ignore"` | JSX attribute expressions. |
| `propertyValue` | `jsx_wrap_mode` | `"ignore"` | Object property values. |

```dx
rule es-jsx-wrap-multilines {
  set declaration "parens-new-line"
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-wrap-multilines) documentation (MIT).
