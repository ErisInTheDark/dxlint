# no-unnecessary-type-conversion

Reports conversion idioms that do not change a value's type or value. This includes primitive conversion calls, string concatenation and unary conversions.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-type-conversion`](https://typescript-eslint.io/rules/no-unnecessary-type-conversion) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 70 / 70 corpus cases |

## Enable

```dx
import .ts
rule no-unnecessary-type-conversion
```

## Examples

Incorrect:

```ts
String("123");
Number(123);
Boolean(true);
```

Correct:

```ts
function convert(value: string | number) {
  return String(value);
}
```

## Differences from upstream

- dxlint reports redundant conversions without offering removal suggestions.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-type-conversion) documentation (MIT).
