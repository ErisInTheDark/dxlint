# es-no-restricted-imports

Reports imports and re-exports forbidden by module path or imported name. Use it to keep private modules private or steer code towards a preferred API.

| | |
| --- | --- |
| Upstream | [`no-restricted-imports`](https://eslint.org/docs/latest/rules/no-restricted-imports) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 262 / 263 corpus cases |

## Enable

```dx
import .ts
rule es-no-restricted-imports
```

## Examples

With `paths` set to `[{ name: "fs" }]`:

Incorrect:

```ts
import fs from "fs";
export * from "fs";
```

Correct:

```ts
import crypto from "crypto";
export { foo } from "bar";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `paths` | `restriction_import_path[]` | `[]` | Exact module paths and optional name restrictions. |
| `patterns` | `restriction_import_pattern[]` | `[]` | Module path groups or regex patterns and optional name restrictions. |

Path records require `name`. Both record types accept `message`, `importNames`, `allowImportNames` and `allowTypeImports` (default `false`). Use `"default"` to restrict a default import.

Pattern records accept `group` (glob strings, with `!` exclusions) or `regex`. They also accept `caseSensitive` (default `false`), `importNamePattern` and `allowImportNamePattern`. Choose a deny-list or an allow-list rather than combining opposite policies.

```dx
rule es-no-restricted-imports {
  set paths [{ name: "fs" }]
  set patterns [{ group: ["private/*"], allowTypeImports: true }]
}
```

Type-only exceptions cover imports and re-exports. Dynamic `import()` calls are not checked. TypeScript `import = require()` supports whole-module restrictions, not imported-name restrictions.

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-restricted-imports` | Exact path restrictions on whole imports and exports. |
| `es-no-restricted-imports:names` | Imported names for exact paths. |
| `es-no-restricted-imports:patterns` | Whole-module pattern restrictions. |
| `es-no-restricted-imports:pattern-names` | Imported names for matching module patterns. |

`rule es-no-restricted-imports` turns on every variant; `rule es-no-restricted-imports:patterns off` turns one off.

## Differences from upstream

- `paths` and `patterns` use records; ESLint's bare-string shorthand is not supported.
- dxlint uses its own configuration and suppression syntax, not `eslint-disable-line`. The corpus shortfall is an ESLint directive case.
- Regex options use dxlint's bounded JavaScript regex support; see [the language reference](../language.md).

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-restricted-imports) documentation (MIT).
