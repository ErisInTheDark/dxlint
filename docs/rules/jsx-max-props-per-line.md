# jsx-max-props-per-line

Limits how many JSX props can share a line. A spread attribute counts as one prop; the default limit is one for both single-line and multiline tags.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-max-props-per-line`](https://eslint.style/rules/jsx-max-props-per-line) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 41 / 41 corpus cases |

## Enable

```dx
import .ts
rule jsx-max-props-per-line
```

## Examples

Incorrect:

```tsx
<Hello firstName="John" lastName="Smith" />;
```

Correct:

```tsx
<Hello
  firstName="John"
  lastName="Smith"
/>;
```

## Options

Each variant has its own limit.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `number(1..)` | `1` | Maximum number of props per line. |

```dx
rule jsx-max-props-per-line
rule jsx-max-props-per-line:singleline {
  set max 3
}
rule jsx-max-props-per-line:multiline {
  set max 1
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `jsx-max-props-per-line:singleline` | Single-line opening tags. |
| `jsx-max-props-per-line:multiline` | Multiline opening tags. |

`rule jsx-max-props-per-line` turns on every variant; `rule jsx-max-props-per-line:singleline off` turns one off.

## Differences from upstream

- Upstream's `maximum.single` and `maximum.multi` become variant-specific `max` options. Disable the singleline variant to reproduce `when: "multiline"`.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-max-props-per-line) documentation (MIT).
