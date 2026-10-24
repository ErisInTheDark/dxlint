# jsx-first-prop-new-line

Checks whether the first JSX prop starts beside the tag name or on a new line. The default requires a new line for multiline opening tags with more than one prop.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-first-prop-new-line`](https://eslint.style/rules/jsx-first-prop-new-line) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 30 / 30 corpus cases |

## Enable

```dx
import .ts
rule jsx-first-prop-new-line
```

## Examples

Incorrect:

```tsx
<Hello personal
  foo="bar"
/>;
```

Correct:

```tsx
<Hello
  personal
  foo="bar"
/>;
<Hello personal />;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never" \| "multiline" \| "multiline-multiprop" \| "multiprop"` | `"multiline-multiprop"` | First-prop placement policy. |

`"always"` requires a new line; `"never"` keeps the first prop beside the name. `"multiline"` requires a new line for multiline tags. `"multiprop"` keeps a lone prop beside the name but permits a new line when there are several. `"multiline-multiprop"` requires a new line when the tag is multiline and has several props.

```dx
rule jsx-first-prop-new-line {
  set mode "always"
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-first-prop-new-line) documentation (MIT).
