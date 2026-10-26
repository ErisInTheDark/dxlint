# es-implicit-arrow-linebreak

Requires a consistent position for an arrow function's implicitly returned expression. Block bodies are not checked.

| | |
| --- | --- |
| Upstream | [`@stylistic/implicit-arrow-linebreak`](https://eslint.style/rules/implicit-arrow-linebreak) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 62 / 62 corpus cases |

## Enable

```dx
import .ts
rule es-implicit-arrow-linebreak
```

## Examples

Incorrect:

```ts
(foo) =>
  bar;
```

Correct:

```ts
(foo) => bar;
(foo) => {
  return bar;
};
```

## Options

`mode` is the default option: `rule es-implicit-arrow-linebreak set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"beside" \| "below"` | `"beside"` | Keep the expression beside the arrow, or require it below. |

```dx
rule es-implicit-arrow-linebreak {
  set mode "below"
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/implicit-arrow-linebreak) documentation (MIT).
