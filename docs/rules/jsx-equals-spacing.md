# jsx-equals-spacing

Requires consistent spaces around `=` in JSX attributes. The default forbids spaces on either side.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-equals-spacing`](https://eslint.style/rules/jsx-equals-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 24 / 24 corpus cases |

## Enable

```dx
import .ts
rule jsx-equals-spacing
```

## Examples

Incorrect:

```tsx
<Hello name = {firstname} />;
```

Correct:

```tsx
<Hello name={firstname} />;
<Hello name />;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never"` | `"never"` | Require or forbid surrounding spaces. |

```dx
rule jsx-equals-spacing {
  set mode "always"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `jsx-equals-spacing:before` | Space before equals. |
| `jsx-equals-spacing:after` | Space after equals. |

`rule jsx-equals-spacing` turns on every variant; `rule jsx-equals-spacing:before off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-equals-spacing) documentation (MIT).
