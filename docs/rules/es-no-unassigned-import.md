# es-no-unassigned-import

Reports imports without imported bindings. These imports run a module only for side effects, which can make dependencies harder to understand or remove.

| | |
| --- | --- |
| Upstream | [`import/no-unassigned-import`](https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/no-unassigned-import.md) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:style` |

## Enable

```dx
import .ts
rule es-no-unassigned-import
```

## Examples

Incorrect:

```ts
import "should";
```

Correct:

```ts
import should from "should";
```

## Differences from upstream

- Checks ES module import declarations, not unassigned CommonJS `require()` calls.
- The upstream `allow` glob option is not available.
- Uses the message “Imported module should be assigned”.

Adapted from the [eslint-plugin-import](https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/no-unassigned-import.md) documentation (MIT).
