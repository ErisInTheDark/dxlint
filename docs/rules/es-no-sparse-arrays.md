# es-no-sparse-arrays

Reports holes in array literals, often caused by an accidental extra comma. A trailing comma after an element is allowed.

| | |
| --- | --- |
| Upstream | [`no-sparse-arrays`](https://eslint.org/docs/latest/rules/no-sparse-arrays) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 6 / 6 corpus cases |

## Enable

```dx
import .ts
rule es-no-sparse-arrays
```

## Examples

Incorrect:

```ts
const items = [,];
const colours = ["red", , "blue"];
```

Correct:

```ts
const items = [];
const colours = ["red", "blue",];
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-sparse-arrays) documentation (MIT).
