# es-no-unused-imports

Reports imported names that are never used. It checks default, namespace, named, and TypeScript import-equals bindings; a JSDoc `{@link Name}` reference also counts as a use.

| | |
| --- | --- |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |

## Enable

```dx
import .ts
rule es-no-unused-imports
```

## Examples

Incorrect:

```ts
import { readFile } from "node:fs";
export const value = 1;
```

Correct:

```ts
import { readFile } from "node:fs";
export const read = readFile;
```
