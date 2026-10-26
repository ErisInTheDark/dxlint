# es-no-var

Reports `var` declarations. Use `let` or `const` to make block scope explicit and avoid accidental changes to variables outside a block.

| | |
| --- | --- |
| Upstream | [`no-var`](https://eslint.org/docs/latest/rules/no-var) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 76 / 76 corpus cases |

## Enable

```dx
import .ts
rule es-no-var
```

## Examples

Incorrect:

```ts
var x = "y";
var CONFIG = {};
```

Correct:

```ts
let x = "y";
const CONFIG = {};
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-var) documentation (MIT).
