# es-init-declarations

Requires variables to be initialised when declared, or forbids initialisers with `"never"`. TypeScript ambient declarations are exempt because they cannot have runtime initialisers.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/init-declarations`](https://typescript-eslint.io/rules/init-declarations) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 158 / 158 corpus cases |

## Enable

```dx
import .ts
rule es-init-declarations
```

## Examples

Incorrect:

```ts
let count;
```

Correct:

```ts
let count = 0;
declare let externalCount: number;
```

## Options

`mode` is the default option: `rule es-init-declarations set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never"` | `"always"` | Require or forbid initialisers. |
| `ignoreForLoopInit` | `boolean` | `false` | Allow initialisers in loop headers in never mode. |

```dx
rule es-init-declarations {
  set mode "never"
  set ignoreForLoopInit true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-init-declarations` | Missing initialisers in always mode. |
| `es-init-declarations:never` | Unwanted initialisers in never mode. |

`rule es-init-declarations` turns on every variant; `rule es-init-declarations:never off` turns one off.

## Differences from upstream

- Like the TypeScript extension of ESLint's core rule, this rule permits ambient `declare` variables.
- Diagnostics can have different report spans for TypeScript declarations, despite matching semantic corpus results.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/init-declarations) documentation (MIT).
