# es-switch-colon-spacing

Controls spaces around `case` and `default` colons when the adjacent tokens are on the same line. By default, there is no space before the colon and a space after it.

| | |
| --- | --- |
| Upstream | [`@stylistic/switch-colon-spacing`](https://eslint.style/rules/switch-colon-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 46 / 46 corpus cases |

## Enable

```dx
import .ts
rule es-switch-colon-spacing
```

## Examples

Incorrect:

```ts
switch (value) {
  case 0 :break;
  default :run();
}
```

Correct:

```ts
switch (value) {
  case 0: break;
  default: run();
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `before` | boolean | `false` | Require space before colons. |
| `after` | boolean | `true` | Require space after colons. |

```dx
rule es-switch-colon-spacing {
  set before false
  set after true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-switch-colon-spacing:before` | Space before clause colons. |
| `es-switch-colon-spacing:after` | Space after clause colons. |

`rule es-switch-colon-spacing` turns on every variant; `rule es-switch-colon-spacing:before off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/switch-colon-spacing) documentation (MIT).
