# es-template-curly-spacing

Controls spaces inside template substitutions. By default, `${` and `}` stay attached to the expression on the same line.

| | |
| --- | --- |
| Upstream | [`@stylistic/template-curly-spacing`](https://eslint.style/rules/template-curly-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 57 / 57 corpus cases |

## Enable

```dx
import .ts
rule es-template-curly-spacing
```

## Examples

Incorrect:

```ts
const greeting = `hello, ${ people.name }!`;
```

Correct:

```ts
const greeting = `hello, ${people.name}!`;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never"` | `"never"` | Require or disallow spaces inside substitutions. |

```dx
rule es-template-curly-spacing {
  set mode "always"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-template-curly-spacing:after` | Unwanted space after `${`. |
| `es-template-curly-spacing:before` | Unwanted space before `}`. |
| `es-template-curly-spacing:missing-after` | Required space after `${`. |
| `es-template-curly-spacing:missing-before` | Required space before `}`. |

`rule es-template-curly-spacing` turns on every variant; `rule es-template-curly-spacing:before off` turns one off.

## Differences from upstream

- The upstream string option is named `mode` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/template-curly-spacing) documentation (MIT).
