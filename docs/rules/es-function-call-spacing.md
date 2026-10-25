# es-function-call-spacing

Require or forbid whitespace before invocation parentheses. The default keeps function names and their opening parentheses together.

| | |
| --- | --- |
| Upstream | [`@stylistic/function-call-spacing`](https://eslint.style/rules/function-call-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 218 / 218 corpus cases |

## Enable

```dx
import .ts
rule es-function-call-spacing
```

## Examples

Incorrect:

```ts
fn ();
new Date ();
```

Correct:

```ts
fn();
new Date();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"` / `"never"` | `"never"` | Require or forbid invocation spacing. |
| `allowNewlines` | `boolean` | `false` | Allow line breaks in `always` mode. |
| `chain_before` | `boolean` | `true` | In `always` mode, require space before optional-call `?.`. |
| `chain_after` | `boolean` | `true` | In `always` mode, require space after optional-call `?.`. |

```dx
rule es-function-call-spacing {
  set mode "always"
  set chain_before false
  set chain_after true
}
```

## Differences from upstream

- The positional policy is named `mode`.
- dxlint exposes optional-call spacing through `chain_before` and `chain_after`.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/function-call-spacing) documentation (MIT).
