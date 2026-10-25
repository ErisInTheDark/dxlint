# es-jsx-one-expression-per-line

Requires JSX children to occupy separate lines rather than sharing a line with other children or their parent tags. The fixer inserts the missing line breaks.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-one-expression-per-line`](https://eslint.style/rules/jsx-one-expression-per-line) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 116 / 116 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-one-expression-per-line
```

## Examples

Incorrect:

```tsx
<App><Hello /></App>;
```

Correct:

```tsx
<App>
  <Hello />
</App>;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "single-child" \| "single-line"` | `"always"` | Always split children, allow one child inline, or allow children that fit on one line. |

```dx
rule es-jsx-one-expression-per-line {
  set mode "single-child"
}
```

## Differences from upstream

- Use `mode` instead of upstream's `allow` option; `"always"` corresponds to upstream's `"none"`.
- Upstream's `"literal"` and `"non-jsx"` exceptions are not exposed.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-one-expression-per-line) documentation (MIT).
