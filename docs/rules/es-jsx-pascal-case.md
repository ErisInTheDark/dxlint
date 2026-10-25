# es-jsx-pascal-case

Reports user-defined JSX component names that are not PascalCase. Lowercase intrinsic tags such as `div` are left alone.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-pascal-case`](https://eslint.style/rules/jsx-pascal-case) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 43 / 43 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-pascal-case
```

## Examples

Incorrect:

```tsx
<Test_component />;
<TEST_COMPONENT />;
```

Correct:

```tsx
<TestComponent />;
<CSSTransitionGroup />;
<div />;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowAllCaps` | `boolean` | `false` | Permit SCREAMING_SNAKE_CASE. |
| `allowLeadingUnderscore` | `boolean` | `false` | Permit a single leading underscore. |
| `allowNamespace` | `boolean` | `false` | Check only the first namespace segment. |
| `ignore` | `string[]` | `[]` | Component-name globs to skip. |

```dx
rule es-jsx-pascal-case {
  set allowAllCaps true
  set ignore ["Legacy_*"]
}
```

## Differences from upstream

- Ignore patterns are matched by dxlint's glob-to-regex conversion, not the upstream picomatch package; advanced picomatch syntax is not guaranteed.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-pascal-case) documentation (MIT).
