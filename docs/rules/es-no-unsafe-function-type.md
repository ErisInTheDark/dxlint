# es-no-unsafe-function-type

Reports the built-in `Function` type, which accepts functions without checking their parameters or return values. Declare a function signature instead.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-function-type`](https://typescript-eslint.io/rules/no-unsafe-function-type) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:type-safety`, `rules:recommended` |
| Parity | 8 / 8 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-function-type
```

## Examples

Incorrect:

```ts
let stringToNumber: Function;
stringToNumber = (text: string) => text.length;
```

Correct:

```ts
let stringToNumber: (text: string) => number;
stringToNumber = text => text.length;
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-function-type) documentation (MIT).
