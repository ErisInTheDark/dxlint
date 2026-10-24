# key-spacing

Reports inconsistent spaces around property colons. It can also align keys or values across neighbouring lines.

| | |
| --- | --- |
| Upstream | [`@stylistic/key-spacing`](https://eslint.style/rules/key-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 282 / 282 corpus cases |

## Enable

```dx
import .ts
rule key-spacing
```

## Examples

Incorrect:

```ts
const item = { name : "book", price:2 };
```

Correct:

```ts
const item = { name: "book", price: 2 };
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `beforeColon` | `boolean` | `false` | Require a space before colons. |
| `afterColon` | `boolean` | `true` | Require a space after colons. |
| `mode` | `"strict"` or `"minimum"` | `"strict"` | Exact spacing or at least the required spacing. |
| `singleLine` | `format_key_line_policy` or `null` | `null` | Single-line spacing override. |
| `multiLine` | `format_key_line_policy` or `null` | `null` | Multiline spacing override. |
| `align` | `"colon"`, `"value"`, `format_key_alignment`, or `null` | `null` | Align multiline colons or values. |

Line policies accept `beforeColon`, `afterColon`, and `mode`; `multiLine` can also specify `align`. An alignment object accepts `on` (`"colon"` or `"value"`), `beforeColon`, `afterColon`, and `mode`.

```dx
rule key-spacing {
  set align "value"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `key-spacing` | Ordinary spacing before colons. |
| `key-spacing:after` | Ordinary spacing after colons. |
| `key-spacing:aligned-before` | Before-colon alignment. |
| `key-spacing:aligned-after` | After-colon alignment. |

`rule key-spacing` turns on every variant; `rule key-spacing:after off` turns one off.

## Differences from upstream

- The upstream `ignoredNodes` option is not exposed.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/key-spacing) documentation (MIT).
