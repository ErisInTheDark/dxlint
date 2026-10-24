# keyword-spacing

Reports missing or unwanted spaces around keywords. Per-keyword overrides let you keep exceptions without changing the whole policy.

| | |
| --- | --- |
| Upstream | [`@stylistic/keyword-spacing`](https://eslint.style/rules/keyword-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 1177 / 1177 corpus cases |

## Enable

```dx
import .ts
rule keyword-spacing
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
rule keyword-spacing {
  set overrides { if: { after: false } }
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `keyword-spacing:before` | Spaces before keywords. |
| `keyword-spacing:after` | Spaces after keywords. |

`rule keyword-spacing` turns on every variant; `rule keyword-spacing:before off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/keyword-spacing) documentation (MIT).
