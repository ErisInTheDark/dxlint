# restrict-plus-operands

Checks the types used with `+` and `+=` to catch accidental coercion and incompatible addition. Numbers, bigints and strings are supported, with options allowing selected mixed types.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/restrict-plus-operands`](https://typescript-eslint.io/rules/restrict-plus-operands) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:types` |
| Parity | 119 / 119 corpus cases |

## Enable

```dx
import .ts
rule restrict-plus-operands
```

## Examples

Incorrect:

```ts
const total = 1n + 1;
const join = (a: string, b: never) => a + b;
```

Correct:

```ts
const total = 1n + 1n;
const join = (a: string, b: string) => a + b;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowAny` | `boolean` | `true` | Allow operands typed as `any`. |
| `allowBoolean` | `boolean` | `true` | Allow boolean operands. |
| `allowNullish` | `boolean` | `true` | Allow `null` and `undefined` operands. |
| `allowNumberAndString` | `boolean` | `true` | Allow strings combined with numbers or bigints. |
| `allowRegExp` | `boolean` | `true` | Allow regular expressions in string concatenation. |
| `skipCompoundAssignments` | `boolean` | `false` | Skip `+=` assignments. |

```dx
rule restrict-plus-operands {
  set allowAny false
  set allowNumberAndString false
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/restrict-plus-operands) documentation (MIT).
