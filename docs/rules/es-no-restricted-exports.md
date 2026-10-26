# es-no-restricted-exports

Reports exported names listed in your project's restrictions. No names are restricted until you configure them.

| | |
| --- | --- |
| Upstream | [`no-restricted-exports`](https://eslint.org/docs/latest/rules/no-restricted-exports) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 183 / 183 corpus cases |

## Enable

```dx
import .ts
rule es-no-restricted-exports
```

## Examples

With `restrictedNamedExports` set to `["foo"]`:

Incorrect:

```ts
export const foo = 1;
export { value as foo } from "some_module";
```

Correct:

```ts
export const quux = 1;
export { value as publicValue } from "some_module";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `restrictedNamedExports` | `string[]` | `[]` | Exported names to reject. |
| `restrictedNamedExportsPattern` | `js_regex` | `""` | Pattern matching forbidden exported names, excluding `default`. |

```dx
rule es-no-restricted-exports {
  set restrictedNamedExports ["foo"]
}
```

An `export * from "module"` does not inspect that module for restricted names.

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-restricted-exports:declarations` | Exported declaration names. |
| `es-no-restricted-exports:direct` | Direct default exports when a custom policy marks them. |
| `es-no-restricted-exports:names` | Named exports and re-exports. |
| `es-no-restricted-exports:namespace` | Namespace re-export names. |

`rule es-no-restricted-exports` turns on every variant; `rule es-no-restricted-exports:namespace off` turns one off.

## Differences from upstream

- The `restrictDefaultExports` option is not exposed. A custom `.dx` policy can set `no_restricted_default_export_checked` on the export forms it forbids.
- Pattern options use dxlint's bounded JavaScript regex support; see [the language reference](../language.md).

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-restricted-exports) documentation (MIT).
