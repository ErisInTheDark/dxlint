# no-shadow

Reports declarations that hide names from an outer scope. Unlike redeclarations, shadowed names live in separate nested scopes.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-shadow`](https://typescript-eslint.io/rules/no-shadow) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 607 / 607 corpus cases |

## Enable

```dx
import .ts
rule no-shadow
```

## Examples

Incorrect:

```ts
const count = 1;
function read() {
  const count = 2;
  return count;
}
```

Correct:

```ts
const count = 1;
function read() {
  const localCount = 2;
  return localCount;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | `string[]` | `[]` | Names allowed to shadow. |
| `builtinGlobals` | `boolean` | `false` | Include configured globals in shadow checks. |
| `hoist` | `"all" \| "functions" \| "never" \| "types" \| "functions-and-types"` | `"functions"` | Which later declarations count as outer bindings. |
| `ignoreOnInitialization` | `boolean` | `false` | Allow callback declarations within initialisers. |
| `ignoreTypeValueShadow` | `boolean` | `true` | Allow shadowing across type and value namespaces. |
| `ignoreFunctionTypeParameterNameValueShadow` | `boolean` | `true` | Allow function-type parameter names to shadow value names. |

```dx
rule no-shadow {
  set hoist "functions-and-types"
  set allow ["resolve", "reject"]
}
```

## Differences from upstream

- The TypeScript upstream rule extends ESLint's core rule.
- dxlint defaults `hoist` to `"functions"`; typescript-eslint defaults it to `"functions-and-types"`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-shadow) documentation (MIT).
