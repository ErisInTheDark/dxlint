# es-keyword-spacing

Reports missing or unwanted spaces around keywords. Per-keyword overrides let you keep exceptions without changing the whole policy.

| | |
| --- | --- |
| Upstream | [`@stylistic/keyword-spacing`](https://eslint.style/rules/keyword-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 1177 / 1177 corpus cases |

## Enable

```dx
import .ts
rule es-keyword-spacing
```

## Examples

Incorrect:

```ts
if(ready) {}else {}
```

Correct:

```ts
if (ready) {} else {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `before` | `boolean` | `true` | Require spaces before keywords. |
| `after` | `boolean` | `true` | Require spaces after keywords. |
| `overrides` | `{ [key: string]: format_keyword_policy }` | `{}` | Keyword-specific `before` and `after` settings. |

```dx
rule es-keyword-spacing {
  set overrides { if: { after: false } }
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-keyword-spacing:before` | Spaces before keywords. |
| `es-keyword-spacing:after` | Spaces after keywords. |

`rule es-keyword-spacing` turns on every variant; `rule es-keyword-spacing:before off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/keyword-spacing) documentation (MIT).
