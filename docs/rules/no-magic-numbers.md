# no-magic-numbers

Reports numeric literals used without a meaningful name. Named constants explain what a number represents and make related uses easier to change.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-magic-numbers`](https://typescript-eslint.io/rules/no-magic-numbers) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 284 / 284 corpus cases |

## Enable

```dx
import .ts
rule no-magic-numbers
```

## Examples

Incorrect:

```ts
const price = 100;
const finalPrice = price + price * 0.25;
```

Correct:

```ts
const price = 100;
const tax = 0.25;
const finalPrice = price + price * tax;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `enforceConst` | `boolean` | `false` | Require named numeric declarations to use `const`. |
| `ignore` | `(number or string)[]` | `[]` | Allowed numbers or bigint strings such as `"1n"`. |

```dx
rule no-magic-numbers {
  set enforceConst true
  set ignore [0, 1, "1n"]
}
```

## Differences from upstream

- Combines the ESLint core rule and its TypeScript extension, including numeric literal types, enums, and readonly class properties.
- Only `ignore` and `enforceConst` are exposed. Upstream's `ignoreArrayIndexes`, `ignoreDefaultValues`, `ignoreClassFieldInitialValues`, `detectArray`, `ignoreEnums`, `ignoreNumericLiteralTypes`, `ignoreReadonlyClassProperties`, and `ignoreTypeIndexes` options are unavailable.
- Object-property values, radix arguments, and JSX numeric uses are excluded by the implementation rather than exposed as options.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-magic-numbers) documentation (MIT).
