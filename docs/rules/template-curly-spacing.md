# template-curly-spacing

Controls spaces inside template substitutions. By default, `${` and `}` stay attached to the expression on the same line.

| | |
| --- | --- |
| Upstream | [`@stylistic/template-curly-spacing`](https://eslint.style/rules/template-curly-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 57 / 57 corpus cases |

## Enable

```dx
import .ts
rule template-curly-spacing
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
rule template-curly-spacing {
  set mode "always"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `template-curly-spacing:after` | Unwanted space after `${`. |
| `template-curly-spacing:before` | Unwanted space before `}`. |
| `template-curly-spacing:missing-after` | Required space after `${`. |
| `template-curly-spacing:missing-before` | Required space before `}`. |

`rule template-curly-spacing` turns on every variant; `rule template-curly-spacing:before off` turns one off.

## Differences from upstream

- The upstream string option is named `mode` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/template-curly-spacing) documentation (MIT).
