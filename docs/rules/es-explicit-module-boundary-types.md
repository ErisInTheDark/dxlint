# es-explicit-module-boundary-types

Require argument and return types on exported functions and public methods of exported classes. Explicit module boundaries tell callers what goes in and what comes back.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/explicit-module-boundary-types`](https://typescript-eslint.io/rules/explicit-module-boundary-types) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:style` |
| Parity | 154 / 154 corpus cases |

## Enable

```dx
import .ts
rule es-explicit-module-boundary-types
```

## Examples

Incorrect:

```ts
export function greet(name) { return `Hello ${name}`; }
```

Correct:

```ts
export function greet(name: string): string { return `Hello ${name}`; }
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowArgumentsExplicitlyTypedAsAny` | `boolean` | `false` | Accept explicit `any` parameter types. |
| `allowDirectConstAssertionInArrowFunctions` | `boolean` | `true` | Exempt concise arrows returning const assertions. |
| `allowedNames` | `string[]` | `[]` | Exempt named functions and members. |
| `allowHigherOrderFunctions` | `boolean` | `true` | Exempt return types for functions returning only functions. |
| `allowTypedFunctionExpressions` | `boolean` | `true` | Exempt contextually typed functions. |
| `allowOverloadFunctions` | `boolean` | `false` | Exempt implementations with overload signatures. |

```dx
rule es-explicit-module-boundary-types {
  set allowOverloadFunctions true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-explicit-module-boundary-types` | Function return types. |
| `es-explicit-module-boundary-types:arrow` | Arrow return types. |
| `es-explicit-module-boundary-types:method` | Method return types. |
| `es-explicit-module-boundary-types:argument` | Missing parameter types and forbidden explicit `any`. |

`rule es-explicit-module-boundary-types` turns on every variant; `rule es-explicit-module-boundary-types:argument off` turns one off.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/explicit-module-boundary-types) documentation (MIT).
