# es-strict-boolean-expressions

Reports unsafe types in boolean conditions, logical negations, assertion arguments and array predicates. Explicit checks make it clear whether zero, an empty string or a missing value should count as false.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/strict-boolean-expressions`](https://typescript-eslint.io/rules/strict-boolean-expressions) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 214 / 218 corpus cases |

## Enable

```dx
import .ts
rule es-strict-boolean-expressions
```

Use TypeScript's `strictNullChecks` compiler option.

## Examples

Incorrect:

```ts
declare const number: number | undefined;
if (number) {
  console.log(number);
}
```

Correct:

```ts
declare const number: number | undefined;
if (number != null) {
  console.log(number);
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowAny` | boolean | `false` | Allow `any` and `unknown`. |
| `allowNullableBoolean` | boolean | `false` | Allow nullable booleans. |
| `allowNullableEnum` | boolean | `false` | Allow nullable enums. |
| `allowNullableNumber` | boolean | `false` | Allow nullable numbers. |
| `allowNullableObject` | boolean | `true` | Allow nullable objects. |
| `allowNullableString` | boolean | `false` | Allow nullable strings. |
| `allowNumber` | boolean | `true` | Allow numbers and bigints. |
| `allowString` | boolean | `true` | Allow non-nullable strings. |
| `allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing` | boolean | `false` | Run without strict null checking; results may be unreliable. |

```dx
rule es-strict-boolean-expressions {
  set allowString false
  set allowNumber false
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-strict-boolean-expressions` | Conditions, negations, logical operands and assertion arguments. |
| `es-strict-boolean-expressions:callback` | Array predicate callback return types. |
| `es-strict-boolean-expressions:strict` | Required strict-null-checking configuration. |

`rule es-strict-boolean-expressions` turns on every variant; `rule es-strict-boolean-expressions:callback off` turns one off.

## Differences from upstream

- Upstream offers suggestions for some diagnostics; dxlint does not provide fixes or suggestions here.
- ESLint suppression comments are not interpreted.
- Nested logical expressions can miss an inherited boolean-test position, as in `(1 && []) || ('a' && {})`.
- Contextual parameter types in block-bodied `find` predicates can differ, such as `items.find(x => { return x; })`.
- Without `strictNullChecks`, nullable values can be normalised differently, changing the accompanying condition diagnostic.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/strict-boolean-expressions) documentation (MIT).
