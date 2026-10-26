# es-computed-property-spacing

Controls spaces inside computed property brackets, such as `object[key]` and `{ [key]: value }`. By default, spaces are forbidden; line breaks are not checked.

| | |
| --- | --- |
| Upstream | [`@stylistic/computed-property-spacing`](https://eslint.style/rules/computed-property-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 136 / 136 corpus cases |

## Enable

```dx
import .ts
rule es-computed-property-spacing
```

## Examples

Incorrect:

```ts
object[ key ];
const result = { [ key ]: value };
```

Correct:

```ts
object[key];
const result = { [key]: value };
```

## Options

`style` is the default option: `rule es-computed-property-spacing set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always"` or `"never"` | `"never"` | Require or forbid spaces. |
| `enforceForClassMembers` | `boolean` | `true` | Also check computed class member keys. |

```dx
rule es-computed-property-spacing {
  set style "always"
  set enforceForClassMembers false
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-computed-property-spacing:after` | Space after opening brackets. |
| `es-computed-property-spacing:before` | Space before closing brackets. |

`rule es-computed-property-spacing` turns on every variant; `rule es-computed-property-spacing:after off` turns one off.

## Differences from upstream

- The upstream string option is named `style` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/computed-property-spacing) documentation (MIT).
