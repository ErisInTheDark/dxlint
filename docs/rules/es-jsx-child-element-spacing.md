# es-jsx-child-element-spacing

Reports ambiguous spacing between JSX text and inline elements on neighbouring lines. JSX can remove those newlines, leaving words joined together; an explicit space makes the intended output clear.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-child-element-spacing`](https://eslint.style/rules/jsx-child-element-spacing) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 28 / 28 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-child-element-spacing
```

## Examples

Incorrect:

```tsx
<div>
  Here is a
  <a>link</a>
</div>;
```

Correct:

```tsx
<div>
  Here is a
  {' '}
  <a>link</a>
</div>;
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-child-element-spacing) documentation (MIT).
