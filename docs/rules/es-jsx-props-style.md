# es-jsx-props-style

Keeps JSX prop wrapping consistent with the first prop's position. If the first prop is beside the tag name, later props stay on that line; if it starts a new line, props are wrapped.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-props-style`](https://eslint.style/rules/jsx-props-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 28 / 28 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-props-style
```

## Examples

Incorrect:

```tsx
<App
  foo bar
/>;
```

Correct:

```tsx
<App
  foo
  bar
/>;
<App foo bar />;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` on `:singleline` | `int(0..) \| null` | `null` | Wrap a single-line tag above this prop count; null means no limit. |
| `min` on `:multiline` | `int(0..)` | `0` | Minimum prop count for retaining multiline wrapping. |
| `maxPerLine` on both | `int(1..)` | `1` | Maximum props per line when wrapping. |

```dx
rule es-jsx-props-style
rule es-jsx-props-style:singleline {
  set max 2
}
rule es-jsx-props-style:multiline {
  set min 3
  set maxPerLine 1
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-jsx-props-style:singleline` | Wrapping single-line opening tags when a limit is set. |
| `es-jsx-props-style:multiline` | Consistency and minimum prop count for multiline opening tags. |

`rule es-jsx-props-style` turns on every variant; `rule es-jsx-props-style:singleline off` turns one off.

## Differences from upstream

- Upstream's `singleLine.maxItems` becomes `:singleline`'s `max`; `multiLine.minItems` becomes `:multiline`'s `min`. dxlint also exposes `maxPerLine`.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-props-style) documentation (MIT).
