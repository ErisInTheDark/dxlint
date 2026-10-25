# es-use-isnan

Reports comparisons with `NaN`, including `Number.NaN`. Equality never matches `NaN`, even against itself; use `Number.isNaN()` to test a value instead.

| | |
| --- | --- |
| Upstream | [`es-use-isnan`](https://eslint.org/docs/latest/rules/use-isnan) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 234 / 234 corpus cases |

## Enable

```dx
import .ts
rule es-use-isnan
```

## Examples

Incorrect:

```js
if (value === NaN) { handle(); }
if (value !== Number.NaN) { handle(); }
```

Correct:

```js
if (Number.isNaN(value)) { handle(); }
if (!Number.isNaN(value)) { handle(); }
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-use-isnan` | Comparisons with `NaN`. |
| `es-use-isnan:case` | `case NaN` clauses, which cannot match. |
| `es-use-isnan:switch` | `switch (NaN)`, which cannot match a case. |
| `es-use-isnan:index-of` | Searches for `NaN` using `indexOf` or `lastIndexOf`; off by default. |

`rule es-use-isnan` turns on every variant; `rule es-use-isnan:case off` turns one off.

## Differences from upstream

- The `enforceForSwitchCase` option is represented by the `:case` and `:switch` variants. Disable both to stop checking switch statements.
- The `enforceForIndexOf` option is represented by `es-use-isnan:index-of`, rather than a Boolean option.
- Switch diagnostics explicitly explain that matching `NaN` is impossible and recommend `Number.isNaN`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/use-isnan) documentation (MIT).
