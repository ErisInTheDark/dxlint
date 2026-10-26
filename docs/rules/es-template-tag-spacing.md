# es-template-tag-spacing

Controls whitespace between a template tag and its template literal. By default, the tag is attached directly to the opening backtick.

| | |
| --- | --- |
| Upstream | [`@stylistic/template-tag-spacing`](https://eslint.style/rules/template-tag-spacing) |
| Needs types | no |
| Fix | autofix (except comment-separated tags) |
| Rulesets | `rules:formatting` |
| Parity | 63 / 63 corpus cases |

## Enable

```dx
import .ts
rule es-template-tag-spacing
```

## Examples

Incorrect:

```ts
func `Hello world`;
```

Correct:

```ts
func`Hello world`;
```

## Options

`style` is the default option: `rule es-template-tag-spacing set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always" \| "never"` | `"never"` | Require or disallow space after a template tag. |

```dx
rule es-template-tag-spacing {
  set style "always"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-template-tag-spacing` | Unwanted whitespace without comments; fixable. |
| `es-template-tag-spacing:comment` | Comment-separated tags; report only. |
| `es-template-tag-spacing:missing` | Missing required space; fixable. |

`rule es-template-tag-spacing` turns on every variant; `rule es-template-tag-spacing:comment off` turns one off.

## Differences from upstream

- The upstream string option is named `style` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/template-tag-spacing) documentation (MIT).
