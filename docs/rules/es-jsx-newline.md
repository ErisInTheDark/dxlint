# es-jsx-newline

Requires or prevents blank lines between adjacent JSX elements and expressions. The default adds blank lines to separate children visually.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-newline`](https://eslint.style/rules/jsx-newline) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 31 / 31 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-newline
```

## Examples

Incorrect:

```tsx
<div>
  <Button>{data.label}</Button>
  <List />
</div>;
```

Correct:

```tsx
<div>
  <Button>{data.label}</Button>

  <List />
</div>;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `prevent` | `boolean` | `false` | Forbid blank lines instead of requiring them. |
| `allowMultilines` | `boolean` | `false` | In prevent mode, allow blank lines around multiline children. |

```dx
rule es-jsx-newline {
  set prevent true
  set allowMultilines true
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-newline) documentation (MIT).
