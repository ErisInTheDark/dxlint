# es-prefer-const

Reports `let` bindings that are assigned once and never reassigned. `const` makes that intent visible at the declaration.

| | |
| --- | --- |
| Upstream | [`prefer-const`](https://eslint.org/docs/latest/rules/prefer-const) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 135 / 138 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-const
```

## Examples

Incorrect:

```ts
let a = 3;
console.log(a);
```

Correct:

```ts
const a = 3;
console.log(a);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `destructuring` | `"any" \| "all"` | `"any"` | Report individual eligible destructured bindings or only groups where all qualify. |
| `ignoreReadBeforeAssign` | `boolean` | `false` | Ignore a binding read before its first assignment. |

```dx
rule es-prefer-const {
  set destructuring "all"
  set ignoreReadBeforeAssign true
}
```

## Differences from upstream

- Two corpus cases use another ESLint plugin's `markVariableAsUsed` hook. dxlint does not run ESLint plugins.
- One corpus case depends on ESLint inline configuration. dxlint uses its own configuration syntax.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-const) documentation (MIT).
