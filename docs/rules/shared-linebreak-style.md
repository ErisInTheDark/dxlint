# shared-linebreak-style

Reports line endings that differ from the selected style. By default, files must use LF rather than CRLF.

| | |
| --- | --- |
| Upstream | [`@stylistic/linebreak-style`](https://eslint.style/rules/linebreak-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 26 / 28 corpus cases |

## Enable

```dx
import .ts
rule shared-linebreak-style
```

## Examples

The annotations describe the actual line-ending bytes, not characters to add to the source.

Incorrect:

```ts
const value = 1; // line ends with CRLF (\r\n)
```

Correct:

```ts
const value = 1; // line ends with LF (\n)
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"unix"` or `"windows"` | `"unix"` | Require LF or CRLF. |

```dx
rule shared-linebreak-style {
  set mode "windows"
}
```

## Differences from upstream

- The upstream string argument is named `mode` in dxlint.
- Two corpus fixtures were labelled binary and rejected by the test harness before linting. The parity shortfall is not an observed line-ending check failure.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/linebreak-style) documentation (MIT).
