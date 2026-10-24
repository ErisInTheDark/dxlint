# no-unused-vars

Reports bindings that are declared or assigned but never usefully read. This includes variables, parameters, caught errors, and destructured array elements; values used only in type queries can also be reported.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unused-vars`](https://typescript-eslint.io/rules/no-unused-vars) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 1039 / 1049 corpus cases |

## Enable

```dx
import .ts
rule no-unused-vars
```

## Examples

Incorrect:

```ts
const value = 10;
export function read(unused: string) {
  return 5;
}
```

Correct:

```ts
const value = 10;
export function read(input: string) {
  return input + value;
}
```

## Options

Options belong to the variants listed below.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignorePattern` | `regex` | `""` | Allow matching names to be unused; on variables, parameters, caught errors, and array elements. |
| `reportUsedIgnorePattern` | `boolean` | `false` | Report ignored names that are actually used; on the same four variants. |
| `enableAutofixRemoval` | `{ imports?: boolean }` | `{ imports: null }` | On `:variables`, set `imports` to `true` to remove unused import bindings automatically. |
| `mode` | `"all"` or `"after-used"` | `"after-used"` | On `:parameters`, check all parameters or only those after the last used positional parameter. |
| `vars` | `"all"` or `"local"` | `"all"` | On `:globals`, skip declared script globals when set to `"local"`. |

```dx
rule no-unused-vars:parameters {
  set mode "all"
  set ignorePattern /^_/
}
rule no-unused-vars:variables {
  set enableAutofixRemoval { imports: true }
}
```

Removing a whole import can remove its module's side effects. Enable import removal only when that is safe for your project.

## Variants

| Variant | Checks |
| --- | --- |
| `no-unused-vars:variables` | Variable, function, type, and import bindings. |
| `no-unused-vars:parameters` | Function parameters. |
| `no-unused-vars:caught-errors` | Catch bindings. |
| `no-unused-vars:array-elements` | Names introduced by array destructuring. |
| `no-unused-vars:globals` | Unused names declared by `/* global ... */` comments. |

`rule no-unused-vars` turns on every variant; `rule no-unused-vars:caught-errors off` turns one off.

## Differences from upstream

- The TypeScript upstream rule extends ESLint's core rule. dxlint uses variants instead of one shared option object.
- Upstream `varsIgnorePattern`, `argsIgnorePattern`, `caughtErrorsIgnorePattern`, and `destructuredArrayIgnorePattern` become `ignorePattern` on the corresponding variant. `args` becomes parameter `mode`; disable the parameter or caught-error variant to skip that category.
- Upstream's `vars: "local"` is not a family-wide option here: `vars` belongs to `:globals`. There are no direct `ignoreRestSiblings`, `ignoreUsingDeclarations`, or `ignoreClassWithStaticInitBlock` options.
- dxlint does not run ESLint plugins or their `markVariableAsUsed` hooks. Nine corpus cases depend on those hooks.
- JSX factory imports can be counted differently depending on the configured JSX environment; one corpus case differs.
- A JSX import diagnostic can use different wording. Upstream import-removal suggestions are not supplied; dxlint only offers the opt-in autofix.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unused-vars) and [ESLint](https://eslint.org/docs/latest/rules/no-unused-vars) documentation (MIT).
