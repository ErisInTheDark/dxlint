# es-one-var-declaration-per-line

Reports variable declarators that should start on separate lines. By default, line breaks are required around initialised declarators, but uninitialised names may share a line.

| | |
| --- | --- |
| Upstream | [`@stylistic/one-var-declaration-per-line`](https://eslint.style/rules/one-var-declaration-per-line) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 38 / 38 corpus cases |

## Enable

```dx
import .ts
rule es-one-var-declaration-per-line
```

## Examples

Incorrect:

```ts
let a, b, c = 0;
```

Correct:

```ts
let a, b,
  c = 0;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"initializations" \| "always"` | `"initializations"` | Require line breaks around initialisations or every declarator. |

```dx
rule es-one-var-declaration-per-line {
  set mode "always"
}
```

## Differences from upstream

- Set the upstream positional string through the named `mode` option.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/one-var-declaration-per-line) documentation (MIT).
