# use-isnan

Reports comparisons with `NaN`, including `Number.NaN`. Equality never matches `NaN`, even against itself; use `Number.isNaN()` to test a value instead.

| | |
| --- | --- |
| Upstream | [`use-isnan`](https://eslint.org/docs/latest/rules/use-isnan) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 234 / 234 corpus cases |

## Enable

```dx
import .ts
rule use-isnan
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
| `use-isnan` | Comparisons with `NaN`. |
| `use-isnan:case` | `case NaN` clauses, which cannot match. |
| `use-isnan:switch` | `switch (NaN)`, which cannot match a case. |
| `use-isnan:index-of` | Searches for `NaN` using `indexOf` or `lastIndexOf`; off by default. |

`rule use-isnan` turns on every variant; `rule use-isnan:case off` turns one off.

## Differences from upstream

- The `enforceForSwitchCase` option is represented by the `:case` and `:switch` variants. Disable both to stop checking switch statements.
- The `enforceForIndexOf` option is represented by `use-isnan:index-of`, rather than a Boolean option.
- Switch diagnostics explicitly explain that matching `NaN` is impossible and recommend `Number.isNaN`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/use-isnan) documentation (MIT).
