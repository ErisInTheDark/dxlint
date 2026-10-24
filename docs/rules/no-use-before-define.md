# no-use-before-define

Reports references that appear before their declarations. This catches confusing declaration order and some accesses that would fail at runtime.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-use-before-define`](https://typescript-eslint.io/rules/no-use-before-define) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 480 / 481 corpus cases |

## Enable

```dx
import .ts
rule no-use-before-define
```

## Examples

Incorrect:

```ts
alert(value);
const value = 10;
```

Correct:

```ts
const value = 10;
alert(value);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"` or `"nofunc"` | `"always"` | `"nofunc"` permits references before function declarations. |
| `functions` | `boolean` | `true` | Check function declarations. |
| `classes` | `boolean` | `true` | Check earlier class references across execution contexts. |
| `variables` | `boolean` | `true` | Check earlier variable references across execution contexts. |
| `enums` | `boolean` | `true` | Check enum declarations. |
| `typedefs` | `boolean` | `true` | Check type and interface declarations. |
| `allowNamedExports` | `boolean` | `false` | Allow export specifiers before their declarations. |
| `ignoreTypeReferences` | `boolean` | `true` | Allow type references before declarations. |

```dx
rule no-use-before-define {
  set functions false
  set ignoreTypeReferences false
}
```

## Differences from upstream

- The TypeScript upstream rule extends ESLint's core rule with type, interface, and enum handling. dxlint combines those checks.
- Upstream's `"nofunc"` shorthand is available as `set mode "nofunc"`.
- A strict-mode reference outside a block can miss the upstream warning for a function declared only inside that later block: `"use strict"; a(); { function a() {} }`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-use-before-define) and [ESLint](https://eslint.org/docs/latest/rules/no-use-before-define) documentation (MIT).
