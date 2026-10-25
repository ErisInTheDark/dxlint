# es-no-mixed-spaces-and-tabs

Reports indentation containing both tabs and spaces. The optional smart-tabs mode allows spaces after tabs for alignment.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-mixed-spaces-and-tabs`](https://eslint.style/rules/no-mixed-spaces-and-tabs) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 59 / 59 corpus cases |

## Enable

```dx
import .ts
rule es-no-mixed-spaces-and-tabs
```

## Examples

Incorrect (the return line starts with a tab followed by two spaces):

```ts
function add(x, y) {
	  return x + y;
}
```

Correct:

```ts
function add(x, y) {
  return x + y;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"default" \| "smart-tabs"` | `"default"` | Disallow mixed indentation, or allow tabs followed by alignment spaces. |

```dx
rule es-no-mixed-spaces-and-tabs {
  set mode "smart-tabs"
}
```

## Differences from upstream

- The upstream string setting is named `mode` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-mixed-spaces-and-tabs) documentation (MIT).
