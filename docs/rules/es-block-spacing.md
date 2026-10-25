# es-block-spacing

Controls spaces immediately inside block braces when the next or previous token is on the same line. By default, non-empty single-line blocks need spaces.

| | |
| --- | --- |
| Upstream | [`@stylistic/block-spacing`](https://eslint.style/rules/block-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 101 / 101 corpus cases |

## Enable

```dx
import .ts
rule es-block-spacing
```

## Examples

Incorrect:

```ts
function answer() {return 42;}
```

Correct:

```ts
function answer() { return 42; }
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always"` or `"never"` | `"always"` | Require or forbid spaces inside non-empty blocks. |

```dx
rule es-block-spacing {
  set style "never"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-block-spacing:after` | Space after opening braces. |
| `es-block-spacing:before` | Space before closing braces. |

`rule es-block-spacing` turns on every variant; `rule es-block-spacing:after off` turns one off.

## Differences from upstream

- The upstream string option is named `style` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/block-spacing) documentation (MIT).
