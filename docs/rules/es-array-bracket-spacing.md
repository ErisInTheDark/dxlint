# es-array-bracket-spacing

Enforces spaces inside array brackets, including destructuring patterns. By default, spaces immediately inside the brackets are not allowed.

| | |
| --- | --- |
| Upstream | [`@stylistic/array-bracket-spacing`](https://eslint.style/rules/array-bracket-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |

## Enable

```dx
import .ts
rule es-array-bracket-spacing
```

## Examples

Incorrect:

```ts
const values = [ 'foo', 'bar' ];
const [ first, second ] = values;
```

Correct:

```ts
const values = ['foo', 'bar'];
const [first, second] = values;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always"` or `"never"` | `"never"` | Require or forbid spaces inside brackets. |
| `singleValue` | `boolean` or `null` | `null` | Override spacing for one element. |
| `objectsInArrays` | `boolean` or `null` | `null` | Override spacing beside object elements. |
| `arraysInArrays` | `boolean` or `null` | `null` | Override spacing beside nested arrays. |

`null` keeps the main policy. Empty arrays do not need spaces; line breaks are allowed.

```dx
rule es-array-bracket-spacing {
  set style "always"
  set singleValue false
}
```

## Differences from upstream

- The upstream string option is named `style` in dxlint.
- No corpus parity count is available for this rule.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/array-bracket-spacing) documentation (MIT).
