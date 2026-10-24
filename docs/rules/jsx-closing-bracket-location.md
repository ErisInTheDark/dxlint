# jsx-closing-bracket-location

Checks where the final `>` or `/>` of a multiline JSX opening tag appears. By default it belongs on its own line, aligned with the opening tag.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-closing-bracket-location`](https://eslint.style/rules/jsx-closing-bracket-location) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 109 / 109 corpus cases |

## Enable

```dx
import .ts
rule jsx-closing-bracket-location
```

## Examples

Incorrect:

```tsx
<Hello
  firstName="John"
  lastName="Smith" />;
```

Correct:

```tsx
<Hello
  firstName="John"
  lastName="Smith"
/>;
```

## Options

Each variant has its own `mode`.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"after-props" \| "props-aligned" \| "tag-aligned" \| "line-aligned"` | `"tag-aligned"` | Place the bracket after the last prop, under that prop, under the tag, or at the containing line's indentation. |

```dx
rule jsx-closing-bracket-location
rule jsx-closing-bracket-location:self-closing {
  set mode "after-props"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `jsx-closing-bracket-location:paired` | Opening tags that have a separate closing tag. |
| `jsx-closing-bracket-location:self-closing` | Self-closing tags. |

`rule jsx-closing-bracket-location` turns on every variant; `rule jsx-closing-bracket-location:paired off` turns one off.

## Differences from upstream

- Use the two variants instead of upstream's `nonEmpty` and `selfClosing` options. Each variant's `mode` replaces the location string.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-closing-bracket-location) documentation (MIT).
