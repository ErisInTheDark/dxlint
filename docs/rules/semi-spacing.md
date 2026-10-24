# semi-spacing

Controls spaces before and after semicolons. By default, semicolons have no space before them and a space after them when another token follows on the same line.

| | |
| --- | --- |
| Upstream | [`@stylistic/semi-spacing`](https://eslint.style/rules/semi-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 83 / 83 corpus cases |

## Enable

```dx
import .ts
rule semi-spacing
```

## Examples

Incorrect:

```ts
let foo ;
let bar;let baz;
```

Correct:

```ts
let foo;
let bar; let baz;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `before` | boolean | `false` | Require a space before a semicolon. |
| `after` | boolean | `true` | Require a space after a semicolon on the same line. |

```dx
rule semi-spacing {
  set before false
  set after true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `semi-spacing:before` | Space before semicolons. |
| `semi-spacing:after` | Space after semicolons. |

`rule semi-spacing` turns on every variant; `rule semi-spacing:before off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/semi-spacing) documentation (MIT).
