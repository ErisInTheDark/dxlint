# prefer-nullish-coalescing

Reports default-value expressions that can use `??` or `??=`. Unlike truthiness checks, nullish coalescing preserves values such as `0`, `false`, and an empty string.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-nullish-coalescing`](https://typescript-eslint.io/rules/prefer-nullish-coalescing) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 620 / 620 corpus cases |

## Enable

```dx
import .ts
rule prefer-nullish-coalescing
```

## Examples

Incorrect:

```ts
declare const value: string | null;
const result = value || 'fallback';
```

Correct:

```ts
declare const value: string | null;
const result = value ?? 'fallback';
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoreConditionalTests` | `boolean` | `true` | Ignore truthiness in conditional tests. |
| `ignoreBooleanCoercion` | `boolean` | `false` | Ignore expressions passed to `Boolean`. |
| `ignoreMixedLogicalExpressions` | `boolean` | `false` | Ignore mixed logical expressions. |
| `ignoreTernaryTests` | `boolean` | `false` | Ignore ternary replacements. |
| `ignoreIfStatements` | `boolean` | `false` | Ignore conditional assignments. |
| `ignoreBigint` | `boolean` | `false` | Ignore bigint truthiness. |
| `ignoreBoolean` | `boolean` | `false` | Ignore boolean truthiness. |
| `ignoreNumber` | `boolean` | `false` | Ignore number truthiness. |
| `ignoreString` | `boolean` | `false` | Ignore string truthiness. |
| `ignoreUnknown` | `boolean` | `false` | Ignore unknown truthiness. |
| `allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing` | `boolean` | `false` | Suppress the strict-null-check requirement in the `strict` variant. |

```dx
rule prefer-nullish-coalescing {
  set ignoreString true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `prefer-nullish-coalescing` | Default-value expressions and conditional assignments. |
| `prefer-nullish-coalescing:strict` | Whether `strictNullChecks` is enabled. |

`rule prefer-nullish-coalescing` turns on every variant; `rule prefer-nullish-coalescing:strict off` turns one off.

## Differences from upstream

- Use the separate `ignoreBigint`, `ignoreBoolean`, `ignoreNumber`, and `ignoreString` options instead of upstream's `ignorePrimitives` object. dxlint also exposes `ignoreUnknown`.
- dxlint reports these cases without upstream's suggested edits.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-nullish-coalescing) documentation (MIT).
