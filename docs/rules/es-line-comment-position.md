# es-line-comment-position

Reports line comments in the wrong position: beside code or on their own line. Block comments are not checked.

| | |
| --- | --- |
| Upstream | [`@stylistic/line-comment-position`](https://eslint.style/rules/line-comment-position) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 31 / 36 corpus cases |

## Enable

```dx
import .ts
rule es-line-comment-position
```

## Examples

Incorrect:

```ts
1 + 1; // explanation
```

Correct:

```ts
// explanation
1 + 1;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `options` | Array of `"above"`, `"beside"`, or position objects | `[]` | Upstream-style position and exemption arguments. |

An empty array uses `"above"`. An object accepts `position`, `ignorePattern`, and `applyDefaultIgnorePatterns`. The latter defaults to `true` and exempts common tool directives.

```dx
rule es-line-comment-position {
  set options [{ position: "beside", applyDefaultIgnorePatterns: false }]
}
```

## Differences from upstream

- Put upstream arguments inside `options`, rather than setting `position` directly.
- Five corpus cases depend on ESLint inline configuration or suppression directives. dxlint uses its own configuration and suppression contract.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/line-comment-position) documentation (MIT).
