# jsx-shorthand-boolean

Reports JSX attributes explicitly set to `true`. Leaving the value out means the same thing and makes the attribute shorter.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-shorthand-boolean`](https://eslint.style/rules/jsx-shorthand-boolean) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 7 / 7 corpus cases |

## Enable

```dx
import .ts
rule jsx-shorthand-boolean
```

## Examples

Incorrect:

```tsx
<input readOnly={true} />;
```

Correct:

```tsx
<input readOnly />;
<input readOnly={false} />;
<input readOnly={condition} />;
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-shorthand-boolean) documentation (MIT).
