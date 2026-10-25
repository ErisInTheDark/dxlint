# es-no-inferrable-types

Reports type annotations already clear from an initial or default value. Removing these annotations reduces noise and can allow more specific inferred types.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-inferrable-types`](https://typescript-eslint.io/rules/no-inferrable-types) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 99 / 99 corpus cases |

## Enable

```dx
import .ts
rule es-no-inferrable-types
```

## Examples

Incorrect:

```ts
const count: number = 10;
function update(enabled: boolean = true) {}
```

Correct:

```ts
const count = 10;
function update(enabled = true) {}
```

## Differences from upstream

- Upstream's `ignoreParameters` and `ignoreProperties` options are not exposed. Parameters and ordinary properties are checked; readonly and optional properties retain annotations that widen their types.
- Autofix is withheld when deleting an annotation would also delete comments.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-inferrable-types) documentation (MIT).
