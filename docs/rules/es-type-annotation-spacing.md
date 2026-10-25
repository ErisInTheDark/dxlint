# es-type-annotation-spacing

Controls spaces around TypeScript type annotation punctuation. The default disallows spaces before a colon and requires a space after it.

| | |
| --- | --- |
| Upstream | [`@stylistic/type-annotation-spacing`](https://eslint.style/rules/type-annotation-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 325 / 325 corpus cases |

## Enable

```dx
import .ts
rule es-type-annotation-spacing
```

## Examples

Incorrect:

```ts
let foo:string = "bar";
let other : string = "bar";
```

Correct:

```ts
let foo: string = "bar";
let other: string = "bar";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `before` | boolean | `false` | Require space before annotation punctuation. |
| `after` | boolean | `true` | Require space after annotation punctuation. |
| `overrides` | object described below | `{ colon: null, variable: null, property: null, parameter: null, returnType: null, questionMark: null }` | Override spacing by annotation context. |

Each override accepts `{ before?: boolean, after?: boolean }`. The keys are `colon`, `variable`, `property`, `parameter`, `returnType` and `questionMark`. All except `colon` also accept `"ignore"`. Unset fields inherit the base policy.

For `questionMark`, `before` controls space before `?`, and `after` controls space between `?` and `:`; that inner space is disallowed by default.

```dx
rule es-type-annotation-spacing {
  set before false
  set after true
  set overrides { property: "ignore" }
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-type-annotation-spacing:before` | Space before annotation punctuation. |
| `es-type-annotation-spacing:after` | Space after the colon. |
| `es-type-annotation-spacing:between` | Space between `?` and `:`. |

`rule es-type-annotation-spacing` turns on every variant; `rule es-type-annotation-spacing:between off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/type-annotation-spacing) documentation (MIT).
