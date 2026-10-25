# es-no-deprecated

Reports references to symbols marked with a JSDoc `@deprecated` tag. Use the replacement API named in the tag when one is available.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-deprecated`](https://typescript-eslint.io/rules/no-deprecated) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 246 / 266 corpus cases |

## Enable

```dx
import .ts
rule es-no-deprecated
```

## Examples

Incorrect:

```ts
/** @deprecated Use apiV2 instead. */
declare function apiV1(): Promise<string>;
declare function apiV2(): Promise<string>;
await apiV1();
```

Correct:

```ts
/** @deprecated Use apiV2 instead. */
declare function apiV1(): Promise<string>;
declare function apiV2(): Promise<string>;
await apiV2();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | `(string or type_specifier_file or type_specifier_lib or type_specifier_package)[]` | `[]` | Deprecated symbols permitted by name or source-aware type/value specifier. |

```dx
rule es-no-deprecated {
  set allow ["apiV1"]
}
```

Source-aware entries use `from: "file"`, `from: "lib"` or `from: "package"` and a `name`; file entries may specify `path`, and package entries specify `package`.

## Differences from upstream

- Deprecated JSX props can be missed because their attribute use does not always lead back to the prop declaration.
- Deprecated base constructors can go unreported at `super()` calls.
- Computed numeric property names reached through aliases can lose the deprecated declaration.
- Some Node.js declaration references and overloads are not resolved completely, so deprecated APIs can be missed.
- Names destructured from a namespace import (`const { X } = ns`) and re-exports (`export { X } from "./m"`) do not read the publishing module's export-specifier tags.
- Node16 dynamic imports of CommonJS modules can expose a different namespace shape, losing deprecation tags on nested default exports.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-deprecated) documentation (MIT).
