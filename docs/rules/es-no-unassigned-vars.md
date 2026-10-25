# es-no-unassigned-vars

Reports variables that are read but never assigned a value. Such variables always contain `undefined`, often because an assignment was forgotten.

| | |
| --- | --- |
| Upstream | [`es-no-unassigned-vars`](https://eslint.org/docs/latest/rules/no-unassigned-vars) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 23 / 23 corpus cases |

## Enable

```dx
import .ts
rule es-no-unassigned-vars
```

## Examples

Incorrect:

```ts
let status;
if (status === "ready") {
  console.log("Ready!");
}
```

Correct:

```ts
let status = "ready";
if (status === "ready") {
  console.log("Ready!");
}
```

Unused variables are left to `es-no-unused-vars`. Ambient `declare` variables can receive values outside this file.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unassigned-vars) documentation (MIT).
