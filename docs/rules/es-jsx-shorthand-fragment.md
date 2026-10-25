# es-jsx-shorthand-fragment

Prefers `<>...</>` over `Fragment` and `React.Fragment` when no props are needed. A fragment with a prop such as `key` keeps its explicit name.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-shorthand-fragment`](https://eslint.style/rules/jsx-shorthand-fragment) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 11 / 11 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-shorthand-fragment
```

## Examples

Incorrect:

```tsx
<Fragment><div /></Fragment>;
<React.Fragment><span /></React.Fragment>;
```

Correct:

```tsx
<><div /></>;
<><span /></>;
<Fragment key={id}><div /></Fragment>;
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-shorthand-fragment) documentation (MIT).
