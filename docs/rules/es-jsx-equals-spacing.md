# es-jsx-equals-spacing

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
rule es-jsx-equals-spacing
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

`mode` is the default option: `rule es-jsx-equals-spacing set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never"` | `"never"` | Require or forbid surrounding spaces. |

```dx
rule es-jsx-equals-spacing {
  set mode "always"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-jsx-equals-spacing:before` | Space before equals. |
| `es-jsx-equals-spacing:after` | Space after equals. |

`rule es-jsx-equals-spacing` turns on every variant; `rule es-jsx-equals-spacing:before off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-equals-spacing) documentation (MIT).
