# es-semi-style

Controls which line holds a semicolon separating statements. By default, semicolons belong at the end of the preceding statement.

| | |
| --- | --- |
| Upstream | [`@stylistic/semi-style`](https://eslint.style/rules/semi-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:es-no-semi` |
| Parity | 85 / 85 corpus cases |

## Enable

```dx
import .ts
rule es-semi-style
```

## Examples

Incorrect:

```ts
foo()
;[1, 2, 3].forEach(bar)
```

Correct:

```ts
foo();
[1, 2, 3].forEach(bar)
```

## Options

`style` is the default option: `rule es-semi-style set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"last" \| "first"` | `"last"` | Put semicolons on the preceding or following line. Semicolons in `for` headers remain at line ends. |

```dx
rule es-semi-style {
  set style "first"
}
```

## Differences from upstream

- The upstream string option is named `style` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/semi-style) documentation (MIT).
