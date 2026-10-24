# jsx-function-call-newline

Requires line breaks around JSX used as a function argument. By default only JSX that already spans several lines needs those surrounding breaks.

| | |
| --- | --- |
| Upstream | [`@stylistic/jsx-function-call-newline`](https://eslint.style/rules/jsx-function-call-newline) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 27 / 27 corpus cases |

## Enable

```dx
import .ts
rule jsx-function-call-newline
```

## Examples

Incorrect:

```tsx
fn(<span>
  foo
</span>);
```

Correct:

```tsx
fn(
  <span>
    foo
  </span>
);
fn(<div />);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "multiline"` | `"multiline"` | Require surrounding breaks for all JSX arguments, or only multiline JSX. |

```dx
rule jsx-function-call-newline {
  set mode "always"
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/jsx-function-call-newline) documentation (MIT).
