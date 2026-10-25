# es-no-namespace

Reports TypeScript namespaces and old-style named modules in ordinary source files. Prefer ES module imports and exports; quoted ambient module declarations and definition files are allowed.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-namespace`](https://typescript-eslint.io/rules/no-namespace) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 43 / 43 corpus cases |

## Enable

```dx
import .ts
rule es-no-namespace
```

## Examples

Incorrect:

```ts
namespace foo {}
module bar {}
```

Correct:

```ts
declare module "foo" {}
export const value = 1;
```

## Differences from upstream

- The upstream `allowDeclarations` and `allowDefinitionFiles` options are not exposed. dxlint exempts definition files and quoted module declarations, but reports named `declare namespace` declarations in ordinary files.
- This is the TypeScript namespace rule, not eslint-plugin-import's rule against namespace imports.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-namespace) documentation (MIT).
