# es-semi-spacing

Controls spaces before and after semicolons. By default, semicolons have no space before them and a space after them when another token follows on the same line.

| | |
| --- | --- |
| Upstream | [`@stylistic/semi-spacing`](https://eslint.style/rules/semi-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 83 / 83 corpus cases |

## Enable

```dx
import .ts
rule es-semi-spacing
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
rule es-semi-spacing {
  set before false
  set after true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-semi-spacing:before` | Space before semicolons. |
| `es-semi-spacing:after` | Space after semicolons. |

`rule es-semi-spacing` turns on every variant; `rule es-semi-spacing:before off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/semi-spacing) documentation (MIT).
