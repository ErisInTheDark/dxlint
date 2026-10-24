# quotes

Enforces one string delimiter style: double quotes by default. Templates with interpolation, tags, or literal newlines remain available when needed.

| | |
| --- | --- |
| Upstream | [`@stylistic/quotes`](https://eslint.style/rules/quotes) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 172 / 172 corpus cases |

## Enable

```dx
import .ts
rule quotes
```

## Examples

Incorrect:

```ts
const text = 'single';
```

Correct:

```ts
const text = "double";
const greeting = `Hello, ${name}!`;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"double" \| "single" \| "backtick"` | `"double"` | Preferred string delimiter. |
| `avoidEscape` | `boolean` | `false` | Allow alternate quotes to avoid escapes. |
| `allowTemplateLiterals` | `"never" \| "avoidEscape" \| "always"` | `"never"` | Allow otherwise unnecessary backticks; `"avoidEscape"` also requires `avoidEscape`. |

```dx
rule quotes {
  set style "single"
  set avoidEscape true
}
```

## Differences from upstream

- Set the upstream positional delimiter through `style`.
- Upstream's `ignoreStringLiterals` option is not available.
- Messages use the words `doublequote`, `singlequote`, or `backtick` for the selected delimiter.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/quotes) documentation (MIT).
