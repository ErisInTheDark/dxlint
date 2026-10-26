# es-jsx-indent-props

Reports inconsistent indentation of JSX props that start a new line. Props use four spaces by default, with tabs or first-prop alignment available.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-indent-props`](https://eslint.style/rules/jsx-indent-props) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 40 / 40 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-indent-props
```

## Examples

Incorrect:

```tsx
<Hello
  firstName="John"
/>;
```

Correct:

```tsx
<Hello
    firstName="John"
/>;
```

## Options

`mode` is the default option: `rule es-jsx-indent-props set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `int \| "tab" \| "first"` | `4` | Spaces per level, tabs, or alignment with the first prop. |

```dx
rule es-jsx-indent-props {
  set mode 2
}
```

## Differences from upstream

- The upstream `ignoreTernaryOperator` option is not exposed. The indentation setting is named `mode`.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-indent-props) documentation (MIT).
