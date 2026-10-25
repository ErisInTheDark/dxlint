# es-no-unsafe-enum-comparison

Reports comparisons between enum values and values without a shared enum type, including switch cases. Referring to enum members by name avoids depending on their underlying values.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-enum-comparison`](https://typescript-eslint.io/rules/no-unsafe-enum-comparison) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 85 / 85 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-enum-comparison
```

## Examples

Incorrect:

```ts
enum Fruit { Apple }
declare const fruit: Fruit;
fruit === 0;
```

Correct:

```ts
enum Fruit { Apple }
declare const fruit: Fruit;
fruit === Fruit.Apple;
```

## Differences from upstream

- Upstream can suggest replacing a literal with an enum member. dxlint reports the comparison without a suggestion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-enum-comparison) documentation (MIT).
