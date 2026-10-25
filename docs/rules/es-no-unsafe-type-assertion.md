# es-no-unsafe-type-assertion

Reports type assertions that narrow a value's type without a runtime check, including unsafe assertions involving `any`. Prefer type guards, or an assertion that only broadens the type.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-type-assertion`](https://typescript-eslint.io/rules/no-unsafe-type-assertion) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 96 / 102 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-type-assertion
```

## Examples

Incorrect:

```ts
function choose() {
  return Math.random() < 0.5 ? 42 : "oops";
}
const value = choose() as number;
```

Correct:

```ts
function choose() {
  return Math.random() < 0.5 ? 42 : "oops";
}
const value = choose() as number | string | boolean;
```

## Differences from upstream

- Some array and generic assertions are misclassified by structural assignability, such as `unknown[] as number[]` or narrowing a promise's value type.
- Printed mapped types can list properties or modifiers differently in diagnostic messages.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-type-assertion) documentation (MIT).
