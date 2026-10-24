# jsx-curly-newline

Checks line breaks directly inside JSX expression braces. By default, either both braces have an inner line break or neither does.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-curly-newline`](https://eslint.style/rules/jsx-curly-newline) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 24 / 24 corpus cases |

## Enable

```dx
import .ts
rule jsx-curly-newline
```

## Examples

Incorrect:

```tsx
<div>
  { foo
  }
</div>;
```

Correct:

```tsx
<div>
  {
    foo
  }
</div>;
<div>{foo}</div>;
```

## Options

Each variant has its own policy.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"consistent" \| "require" \| "forbid"` | `"consistent"` | Match both sides, require breaks, or forbid breaks. |

```dx
rule jsx-curly-newline
rule jsx-curly-newline:multiline {
  set mode "require"
}
rule jsx-curly-newline:singleline {
  set mode "forbid"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `jsx-curly-newline:multiline` | Expressions whose contents span multiple lines. |
| `jsx-curly-newline:singleline` | Expressions whose contents occupy one line. |

`rule jsx-curly-newline` turns on every variant; `rule jsx-curly-newline:singleline off` turns one off.

## Differences from upstream

- Upstream's `multiline` and `singleline` object keys become variants with a `mode` option. Use `"forbid"` on both instead of the `"never"` shortcut.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-curly-newline) documentation (MIT).
