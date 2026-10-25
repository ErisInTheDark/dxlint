# es-no-import-assign

Reports writes to imported bindings and namespace import members. Imports are read-only views of another module's exports.

| | |
| --- | --- |
| Upstream | [`es-no-import-assign`](https://eslint.org/docs/latest/rules/no-import-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 116 / 116 corpus cases |

## Enable

```dx
import .ts
rule es-no-import-assign
```

## Examples

Incorrect:

```ts
import mod from "./mod.js";
import * as namespace from "./mod.js";
mod = 1;
namespace.value = 2;
Object.assign(namespace, { value: 3 });
```

Correct:

```ts
import mod from "./mod.js";
mod.property = 1;
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-import-assign` | Writes to imported bindings. |
| `es-no-import-assign:member` | Writes and deletes on namespace members. |
| `es-no-import-assign:call` | Known mutating calls targeting namespaces. |

`rule es-no-import-assign` turns on every variant; `rule es-no-import-assign:call off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-import-assign) documentation (MIT).
