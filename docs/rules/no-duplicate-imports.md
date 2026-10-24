# no-duplicate-imports

Reports imports and re-exports from the same module when their declarations can be combined. Keeping related imports together makes a module's dependencies easier to read.

| | |
| --- | --- |
| Upstream | [`no-duplicate-imports`](https://eslint.org/docs/latest/rules/no-duplicate-imports) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 85 / 85 corpus cases |

## Enable

```dx
import .ts
rule no-duplicate-imports
```

## Examples

Incorrect:

```ts
import { merge } from "module";
import { find } from "module";
```

Correct:

```ts
import { merge, find } from "module";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowSeparateTypeImports` | `boolean` | `false` | Allow separate type and value imports from the same module. |

```dx
rule no-duplicate-imports {
  set allowSeparateTypeImports true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-duplicate-imports:imports` | Declarations duplicating an earlier import. |
| `no-duplicate-imports:exports` | Declarations duplicating an earlier re-export. |

`rule no-duplicate-imports` turns on every variant; `rule no-duplicate-imports:exports off` turns one off.

## Differences from upstream

- There is no `includeExports` option. Enabling the family enables export checks too; use only `rule no-duplicate-imports:imports` for ESLint's default import-only behaviour.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-duplicate-imports) documentation (MIT).
