# no-unnecessary-boolean-literal-compare

Reports equality comparisons between booleans and `true` or `false`. Use the boolean directly, or negate it, instead.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-boolean-literal-compare`](https://typescript-eslint.io/rules/no-unnecessary-boolean-literal-compare) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | `rules:types` |
| Parity | 70 / 72 corpus cases |

## Enable

```dx
import .ts
rule no-unnecessary-boolean-literal-compare
```

## Examples

Incorrect:

```ts
declare const condition: boolean;
if (condition === true) {}
```

Correct:

```ts
declare const condition: boolean;
if (condition) {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowComparingNullableBooleansToTrue` | boolean | `true` | Allow nullable booleans to be compared with `true`. |
| `allowComparingNullableBooleansToFalse` | boolean | `true` | Allow nullable booleans to be compared with `false`. |
| `allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing` | boolean | `false` | Suppress the strict-null-check requirement; owned by `:strict`. |

```dx
rule no-unnecessary-boolean-literal-compare {
  set allowComparingNullableBooleansToTrue false
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-unnecessary-boolean-literal-compare` | Redundant boolean comparisons. |
| `no-unnecessary-boolean-literal-compare:strict` | Missing `strictNullChecks`. |

`rule no-unnecessary-boolean-literal-compare` turns on every variant; `rule no-unnecessary-boolean-literal-compare:strict off` turns one off.

## Differences from upstream

- ESLint inline configuration and suppression directives do not use dxlint's configuration contract.
- Contextually typed generic arrow parameters can lose their boolean constraint, causing missed comparisons such as `(condition ?? true) !== false`.
- A strict-null-check configuration warning uses the start of the source file rather than upstream's non-source line-zero location.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-boolean-literal-compare) documentation (MIT).
