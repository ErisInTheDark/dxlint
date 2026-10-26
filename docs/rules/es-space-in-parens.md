# es-space-in-parens

Controls spaces immediately inside parentheses. The default removes spaces; exceptions can reverse that policy beside selected nested delimiters or inside empty parentheses.

| | |
| --- | --- |
| Upstream | [`@stylistic/space-in-parens`](https://eslint.style/rules/space-in-parens) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 139 / 139 corpus cases |

## Enable

```dx
import .ts
rule es-space-in-parens
```

## Examples

Incorrect:

```ts
foo( "bar" );
const value = ( 1 + 2 ) * 3;
```

Correct:

```ts
foo("bar");
const value = (1 + 2) * 3;
```

## Options

`mode` is the default option: `rule es-space-in-parens set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never"` | `"never"` | Require or disallow inner spaces. |
| `exceptions` | `("{}" \| "[]" \| "()" \| "empty")[]` | `[]` | Reverse the policy for these adjacent delimiters or empty parentheses. |

```dx
rule es-space-in-parens {
  set mode "never"
  set exceptions ["{}"]
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-space-in-parens:after` | Space after `(`. |
| `es-space-in-parens:before` | Space before `)`. |

`rule es-space-in-parens` turns on every variant; `rule es-space-in-parens:before off` turns one off.

## Differences from upstream

- The upstream string option is named `mode` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/space-in-parens) documentation (MIT).
