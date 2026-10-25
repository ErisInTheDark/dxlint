# es-jsx-curly-spacing

Requires or forbids spaces inside JSX expression braces. By default it forbids spaces in attributes and leaves child expressions alone.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-curly-spacing`](https://eslint.style/rules/jsx-curly-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 296 / 296 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-curly-spacing
```

## Examples

Incorrect:

```tsx
<Hello name={ firstname } />;
```

Correct:

```tsx
<Hello name={firstname} />;
<Hello>{ firstname }</Hello>;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `options` | Array of `"always"`, `"never"` or configuration objects | `[]` | Spacing policy and context overrides. |

Configuration objects accept `when` (`"always"` or `"never"`), `allowMultiline` (boolean), `spacing.objectLiterals` (`"always"` or `"never"`), and `attributes` / `children` (booleans or nested configurations with those spacing settings). The implicit policy is `"never"` with multiline allowed, attributes enabled and children disabled.

```dx
rule es-jsx-curly-spacing {
  set options [{when: "never", children: true}]
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-jsx-curly-spacing:after` | Space after the opening brace. |
| `es-jsx-curly-spacing:before` | Space before the closing brace. |

`rule es-jsx-curly-spacing` turns on every variant; `rule es-jsx-curly-spacing:after off` turns one off.

## Differences from upstream

- Put upstream's positional spacing options into the `options` array.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-curly-spacing) documentation (MIT).
