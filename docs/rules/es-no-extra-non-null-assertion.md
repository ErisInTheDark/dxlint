# es-no-extra-non-null-assertion

Reports non-null assertions that add nothing, such as repeated `!` assertions. A single assertion already removes `null` and `undefined` from the value's type.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-extra-non-null-assertion`](https://typescript-eslint.io/rules/no-extra-non-null-assertion) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 13 / 13 corpus cases |

## Enable

```dx
import .ts
rule es-no-extra-non-null-assertion
```

## Examples

Incorrect:

```ts
const bar = foo!!!.bar;
```

Correct:

```ts
const bar = foo!.bar;
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-extra-non-null-assertion) documentation (MIT).
