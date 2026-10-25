# es-jsx-tag-spacing

Checks spaces beside JSX angle brackets and slashes. The default keeps bracket tokens together and requires a space before a self-closing slash.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-tag-spacing`](https://eslint.style/rules/jsx-tag-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 74 / 74 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-tag-spacing
```

## Examples

Incorrect:

```tsx
<Hello/>;
<Hello firstName="John"/>;
```

Correct:

```tsx
<Hello />;
<Hello firstName="John" />;
```

## Options

`"allow"` disables an individual spacing check.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `closingSlash` | `"always" \| "never" \| "allow"` | `"never"` | Space between slash and bracket in closing tokens. |
| `beforeSelfClosing` | `"always" \| "proportional-always" \| "never" \| "allow"` | `"always"` | Space before a self-closing slash. |
| `afterOpening` | `"always" \| "allow-multiline" \| "never" \| "allow"` | `"never"` | Space after an opening bracket. |
| `beforeClosing` | `"always" \| "proportional-always" \| "never" \| "allow"` | `"allow"` | Space before a closing bracket. |

`"allow-multiline"` permits an opening line break. `"proportional-always"` uses spacing appropriate to the single-line or multiline tag layout.

```dx
rule es-jsx-tag-spacing {
  set beforeSelfClosing "always"
  set beforeClosing "never"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-jsx-tag-spacing` | Bracket and slash spacing. |
| `es-jsx-tag-spacing:before-self-closing` | Missing space before a self-closing slash. |

`rule es-jsx-tag-spacing` turns on every variant; `rule es-jsx-tag-spacing:before-self-closing off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-tag-spacing) documentation (MIT).
