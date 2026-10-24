# jsx-closing-tag-location

Requires the closing tag of multiline JSX to be on its own line and correctly aligned. Single-line elements can keep their closing tags beside their contents.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-closing-tag-location`](https://eslint.style/rules/jsx-closing-tag-location) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 16 / 16 corpus cases |

## Enable

```dx
import .ts
rule jsx-closing-tag-location
```

## Examples

Incorrect:

```tsx
<Hello>
  world
  </Hello>;
```

Correct:

```tsx
<Hello>
  world
</Hello>;
<Hello>world</Hello>;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"tag-aligned" \| "line-aligned"` | `"tag-aligned"` | Align with the opening tag or with its line's indentation. |

```dx
rule jsx-closing-tag-location {
  set mode "line-aligned"
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-closing-tag-location) documentation (MIT).
