# es-jsx-self-closing-comp

Reports JSX elements with an unnecessary separate closing tag because they have no children. Both HTML tags and custom components can use the shorter self-closing form.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-self-closing-comp`](https://eslint.style/rules/jsx-self-closing-comp) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 47 / 47 corpus cases |

## Enable

```dx
import .ts
rule es-jsx-self-closing-comp
```

## Examples

Incorrect:

```tsx
<div className="content"></div>;
<Hello name="John"></Hello>;
```

Correct:

```tsx
<div className="content" />;
<Hello name="John" />;
<Hello>{' '}</Hello>;
```

## Differences from upstream

- Upstream's `component` and `html` switches are not exposed; both kinds are checked.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-self-closing-comp) documentation (MIT).
