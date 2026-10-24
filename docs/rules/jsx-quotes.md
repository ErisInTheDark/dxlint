# jsx-quotes

Requires consistent quote marks around JSX attribute strings. A string may use the other quote style when its contents include the preferred quote, since JSX attributes cannot escape quotes like JavaScript strings can.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-quotes`](https://eslint.style/rules/jsx-quotes) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 18 / 18 corpus cases |

## Enable

```dx
import .ts
rule jsx-quotes
```

## Examples

Incorrect:

```tsx
<a title='link' />;
```

Correct:

```tsx
<a title="link" />;
<a title='"' />;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"prefer-double" \| "prefer-single"` | `"prefer-double"` | Preferred attribute-string quote style. |

```dx
rule jsx-quotes {
  set mode "prefer-single"
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-quotes) documentation (MIT).
