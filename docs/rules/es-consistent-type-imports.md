# es-consistent-type-imports

Use explicit type imports for names used only as types. The opposite policy can instead require ordinary imports.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/consistent-type-imports`](https://typescript-eslint.io/rules/consistent-type-imports) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 381 / 381 corpus cases |

## Enable

```dx
import .ts
rule es-consistent-type-imports
```

## Examples

Incorrect:

```ts
import { Foo } from "Foo";
type T = Foo;
```

Correct:

```ts
import type { Foo } from "Foo";
type T = Foo;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `disallowTypeAnnotations` | `boolean` | `true` | Forbid `import("module")` type annotations. |
| `fixStyle` | `"inline-type-imports"` / `"separate-type-imports"` | `"separate-type-imports"` | Put `type` on individual specifiers or on a separate import. |
| `prefer` | `"type-imports"` / `"no-type-imports"` | `"type-imports"` | Preferred import kind. |

```dx
rule es-consistent-type-imports {
  set fixStyle "inline-type-imports"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-consistent-type-imports` | Whole import declarations. |
| `es-consistent-type-imports:annotation` | `import()` type annotations; no autofix. |
| `es-consistent-type-imports:inline` | Inline type specifiers under `no-type-imports`. |

`rule es-consistent-type-imports` turns on every variant; `rule es-consistent-type-imports:annotation off` turns one off.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/consistent-type-imports) documentation (MIT).
