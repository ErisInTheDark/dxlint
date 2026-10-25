# es-import-no-deprecated

Reports imports and uses of exports marked with JSDoc `@deprecated`. The diagnostic includes the deprecation reason when one is available.

| | |
| --- | --- |
| Upstream | [`import/no-deprecated`](https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/no-deprecated.md) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |

## Enable

```dx
import .ts
rule es-import-no-deprecated
```

## Examples

Given this module:

```ts
// answer.ts
/** @deprecated Use calculate instead. */
export function multiply(six: number, nine: number) { return 42; }
export function calculate(six: number, nine: number) { return six * nine; }
```

Incorrect:

```ts
import { multiply } from "./answer";
multiply(6, 9);
```

Correct:

```ts
import { calculate } from "./answer";
calculate(6, 9);
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-import-no-deprecated:default` | Default imports of deprecated exports. |
| `es-import-no-deprecated:specifier` | Named import specifiers. |
| `es-import-no-deprecated:use` | References to deprecated named and default imports, outside JSX tag names. |
| `es-import-no-deprecated:namespace` | Deprecated members accessed through namespace imports. |

`rule es-import-no-deprecated` turns on every variant; `rule es-import-no-deprecated:use off` turns one off.

## Differences from upstream

- This is the import plugin rule, not `@typescript-eslint/no-deprecated`; it focuses on imported names.
- dxlint follows its own module and symbol resolution. ESLint's `import/docstyle` setting and TomDoc `Deprecated:` comments are not supported.
- No corpus parity count is available for this port.

Adapted from the [eslint-plugin-import](https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/no-deprecated.md) documentation (MIT).
