# es-jsx-curly-brace-presence

Reports unnecessary braces around JSX string literals, or requires braces when configured. Props, text children and JSX-valued props can use separate policies.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-curly-brace-presence`](https://eslint.style/rules/jsx-curly-brace-presence) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 134 / 135 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-curly-brace-presence
```

## Examples

Incorrect:

```tsx
<App>{'Hello world'}</App>;
<App prop={'Hello world'} />;
```

Correct:

```tsx
<App>Hello world</App>;
<App prop="Hello world" />;
```

## Options

`mode` is the default option: `rule es-jsx-curly-brace-presence set <value>` sets it.

`"always"` requires braces, `"never"` removes unnecessary braces, and `"ignore"` skips that context.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never" \| "ignore"` | `"never"` | Default literal policy. |
| `props` | `"always" \| "never" \| "ignore" \| null` | `null` | Prop string policy; null inherits mode. |
| `children` | `"always" \| "never" \| "ignore" \| null` | `null` | Text-child policy; null inherits mode. |
| `propElementValues` | `"always" \| "never" \| "ignore"` | `"ignore"` | Policy for JSX elements used as prop values. |

```dx
rule es-jsx-curly-brace-presence {
  set props "never"
  set children "never"
  set propElementValues "always"
}
```

## Differences from upstream

- The upstream string shortcut becomes `mode`.
- Some string-expression children beside a nested element are missed. For example, strings around the inner `div` in `<App>{'foo'}<div>{'bar'}</div>{'baz'}</App>` may not all report.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-curly-brace-presence) documentation (MIT).
