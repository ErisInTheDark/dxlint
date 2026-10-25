# es-no-import-type-side-effects

Requires a top-level `import type` when every named specifier is type-only. Under TypeScript's `verbatimModuleSyntax`, inline type specifiers alone can leave behind an unwanted runtime side-effect import.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-import-type-side-effects`](https://typescript-eslint.io/rules/no-import-type-side-effects) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 15 / 15 corpus cases |

## Enable

```dx
import .ts
rule es-no-import-type-side-effects
```

## Examples

Incorrect:

```ts
import { type A, type B } from "mod";
```

Correct:

```ts
import type { A, B } from "mod";
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-import-type-side-effects) documentation (MIT).
