# es-prefer-function-type

Reports interfaces and object type literals containing only one call signature when a function type can express the same contract. Function types avoid an unnecessary body around the signature.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-function-type`](https://typescript-eslint.io/rules/prefer-function-type) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 23 / 23 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-function-type
```

## Examples

Incorrect:

```ts
interface Example {
  (): string;
}
```

Correct:

```ts
type Example = () => string;
```

Types with extra properties, multiple call signatures or required inheritance are not simply interchangeable with a single function type.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-function-type) documentation (MIT).
