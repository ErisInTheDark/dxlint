# arrow-spacing

Controls spaces before and after an arrow function's `=>`. By default, both sides require a space.

| | |
| --- | --- |
| Upstream | [`@stylistic/arrow-spacing`](https://eslint.style/rules/arrow-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 48 / 48 corpus cases |

## Enable

```dx
import .ts
rule arrow-spacing
```

## Examples

Incorrect:

```ts
const identity = (value)=>value;
```

Correct:

```ts
const identity = (value) => value;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `before` | `boolean` | `true` | Require spaces before `=>`; false forbids them. |
| `after` | `boolean` | `true` | Require spaces after `=>`; false forbids them. |

```dx
rule arrow-spacing {
  set before false
  set after false
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `arrow-spacing:before` | Missing space before the arrow. |
| `arrow-spacing:after` | Missing space after the arrow. |
| `arrow-spacing:unexpected-before` | Forbidden space before the arrow. |
| `arrow-spacing:unexpected-after` | Forbidden space after the arrow. |

`rule arrow-spacing` turns on every variant; `rule arrow-spacing:before off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/arrow-spacing) documentation (MIT).
